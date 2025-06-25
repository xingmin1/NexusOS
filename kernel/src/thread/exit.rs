use core::ops::ControlFlow;

use alloc::sync::Arc;
use nexus_error::Result;
use ostd::user::UserContextApi;
use crate::thread::ThreadState;
use crate::vm::ProcessVm;
use tracing::info;

/// 终结当前线程或线程组
pub async fn do_exit(
    state: &mut ThreadState,
    uc: &mut ostd::cpu::UserContext,
    group_exit: bool,
) -> Result<ControlFlow<i32, Option<isize>>> {
    let code = uc.syscall_arguments()[0] as i32 & 0xff;

    if group_exit {
        // 组内所有线程立即 zombie
        for thr in state.thread_group.members().read().iter() {
            thr.lifecycle.exit(code);
        }
    } else {
        state.shared_info.lifecycle.exit(code);
    }

    // 如果当前 Task 是最后一个活跃者，回收地址空间
    maybe_reap_process(state.process_vm.clone(), state.thread_group.clone()).await;

    info!(tid = state.shared_info.tid, "thread exit, code = {}", code);

    Ok(ControlFlow::Break(code))
}

/// 若线程组全为 zombie，则释放 VM 与 FD
async fn maybe_reap_process(vm: Arc<ProcessVm>, tg: Arc<crate::thread::thread_group::ThreadGroup>) {
    if tg.members().read().iter().all(|m| m.lifecycle.is_zombie()) {
        vm.root_vmar().clear().await.ok();
    }
}
