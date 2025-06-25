//! clone  — 按 CLONE_THREAD 分治创建“线程”或“进程”。


use core::ops::ControlFlow;

use alloc::{sync::{Arc, Weak}, vec};

use bitflags::bitflags;
use nexus_error::{return_errno_with_message, Errno, Result};
use ostd::{
    cpu::UserContext,
    sync::GuardRwArc,
    task::TaskOptions,
    user::{UserContextApi, UserSpace},
};

use crate::{
    thread::{
        id, task_future, ThreadLocalData, ThreadSharedInfo, ThreadState,
        ThreadGroup,
        fd_table::FdTable,
    },
    vm::ProcessVm,
};

bitflags! {
    /// 支持的 clone 标志
    #[derive(Clone, Copy, Debug)]
    pub struct CloneFlags: u64 {
        const CSIGNAL        = 0x000000ff;
        const CLONE_VM       = 0x00000100;
        const CLONE_FS       = 0x00000200;
        const CLONE_FILES    = 0x00000400;
        const CLONE_SIGHAND  = 0x00000800;
        const CLONE_PARENT   = 0x00008000;
        const CLONE_THREAD   = 0x00010000;
        const CLONE_SETTLS   = 0x00080000;
        const PARENT_SETTID  = 0x00100000;
        const CHILD_CLEARTID = 0x00200000;
        const CHILD_SETTID   = 0x01000000;
    }
}

/// clone 入口：由 syscall 调起  
/// 返回父进程视角下的新 tid；子线程/进程在返回点得 0。  
pub async fn do_clone(parent: &mut ThreadState, uc: &mut UserContext) -> Result<ControlFlow<i32, Option<isize>>> {
    let [raw_flags, child_stack, tls, _parent_tidptr, _child_tidptr, _] = uc.syscall_arguments();
    let flags = CloneFlags::from_bits_truncate(raw_flags as u64);

    // 基本合法性校验
    if flags.contains(CloneFlags::CLONE_SIGHAND) && !flags.contains(CloneFlags::CLONE_VM) {
        return_errno_with_message!(Errno::EINVAL, "CLONE_SIGHAND 需要 CLONE_VM");
    }
    if flags.contains(CloneFlags::CLONE_THREAD)
        && !flags.contains(CloneFlags::CLONE_SIGHAND | CloneFlags::CLONE_VM)
    {
        return_errno_with_message!(
            Errno::EINVAL,
            "CLONE_THREAD 需要同时包含 CLONE_SIGHAND | CLONE_VM"
        );
    }

    // 分治逻辑
    let new_tid = if flags.contains(CloneFlags::CLONE_THREAD) {
        clone_thread(parent, uc, flags, child_stack, tls).await?
    } else {
        clone_process(parent, uc, flags, child_stack, tls).await?
    };

    // 父进程返回 new_tid；子在自身上下文里已置 0
    uc.set_syscall_return_value(new_tid as _);
    Ok(ControlFlow::Continue(Some(new_tid as isize)))
}

// 线程克隆
async fn clone_thread(
    parent: &mut ThreadState,
    uc: &mut UserContext,
    flags: CloneFlags,  
    child_stack: usize,
    tls: usize,
) -> Result<u64> {
    // 与父线程同属线程组 / 地址空间 / 进程上下文
    let child_vm = parent.process_vm.clone();
    let tgroup   = parent.thread_group.clone();

    // 根据 CLONE_FILES 决定 FD 表共享或复制
    let child_fd_table = if flags.contains(CloneFlags::CLONE_FILES) {
        parent.fd_table.clone()
    } else {
        parent.fd_table.dup_table().await
    };

    spawn_child(
        parent.shared_info.parent.clone(),
        parent,
        uc,
        child_vm,
        tgroup,
        child_fd_table,
        flags,
        child_stack,
        tls,
        false,
    )
    .await
}

// 进程克隆
async fn clone_process(
    parent: &mut ThreadState,
    uc: &mut UserContext,
    flags: CloneFlags,
    child_stack: usize,
    tls: usize,
) -> Result<u64> {
    // CLONE_VM => 共享地址空间；否则 fork
    let child_vm = if flags.contains(CloneFlags::CLONE_VM) {
        parent.process_vm.clone()
    } else {
        Arc::new(ProcessVm::fork_from(&parent.process_vm).await?)
    };

    // 创建新的线程组（进程）
    let tgroup_leader_info = Arc::new(ThreadSharedInfo {
        tid: id::alloc(),
        parent: Arc::downgrade(&parent.shared_info),
        children: GuardRwArc::new(vec![]),
        lifecycle: parent.shared_info.lifecycle.clone(),
    });
    let tgroup = ThreadGroup::new_leader(tgroup_leader_info);

    // FD 表：CLONE_FILES => 共享；否则深拷贝
    let fd_table = if flags.contains(CloneFlags::CLONE_FILES) {
        parent.fd_table.clone()
    } else {
        parent.fd_table.dup_table().await
    };

    let parent_process = if flags.contains(CloneFlags::CLONE_PARENT) {
        parent.shared_info.parent.clone()
    } else {
        Arc::downgrade(&parent.thread_group.leader())
    };

    spawn_child(
        parent_process,
        parent,
        uc,
        child_vm,
        tgroup,
        fd_table,
        flags,
        child_stack,
        tls,
        true,
    )
    .await
}

// 公共：真正生成并调度子 Task  
async fn spawn_child(
    parent_process: Weak<ThreadSharedInfo>,
    parent_thread: &mut ThreadState,
    _uc: &mut UserContext,
    child_vm: Arc<ProcessVm>,
    tgroup: Arc<ThreadGroup>,
    fd_table: Arc<FdTable>,
    flags: CloneFlags,
    child_stack: usize,
    tls: usize,
    is_child_process: bool,
) -> Result<u64> {
    let new_tid = id::alloc();

    // ThreadSharedInfo 
    let child_shared = Arc::new(ThreadSharedInfo {
        tid: new_tid,
        parent: parent_process,
        children: GuardRwArc::new(vec![]),
        lifecycle: parent_thread.shared_info.lifecycle.clone(),
    });
    tgroup.attach(child_shared.clone());

    // 用户上下文 
    let user_space = parent_thread.task.user_space().unwrap().clone();
    let mut child_uc = user_space.user_mode().context().clone();
    child_uc.set_syscall_return_value(0); // 子线程/进程得 0
    if child_stack != 0 {
        child_uc.set_stack_pointer(child_stack);
    }
    if flags.contains(CloneFlags::CLONE_SETTLS) {
        child_uc.set_tls_pointer(tls);
    }
    let child_us = Arc::new(UserSpace::new(user_space.vm_space().clone(), child_uc));

    // Task 
    let thread_local = ThreadLocalData {
        process_vm: child_vm.clone(),
    };
    let child_task = Arc::new(
        TaskOptions::new()
            .user_space(Some(child_us))
            .local_data(thread_local)
            .build(),
    );

    // ThreadState 
    let child_ts = ThreadState {
        task: child_task.clone(),
        thread_group: tgroup.clone(),
        process_vm: child_vm,
        shared_info: child_shared.clone(),
        fd_table,
        user_brk: parent_thread.user_brk,
    };

    if is_child_process && let Some(parent_process) = child_shared.parent.upgrade() {
        parent_process.children.write().push(child_shared);
    }

    // 调度 
    child_task.run(task_future(child_ts));
    Ok(new_tid)
}
