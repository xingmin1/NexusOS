pub mod exception;
pub mod init_stack;
pub mod loader;
pub mod id;
pub mod state;
pub mod clone;
pub mod thread_group;
pub mod fd_table;
pub mod exit;
pub mod wait;
pub mod execve;
pub mod get_ppid;
pub mod get_pid;

use alloc::{
    ffi::CString, sync::{Arc, Weak}, vec::Vec, vec,
};
use nexus_error::Error;
use pin_project_lite::pin_project;
use syscall_numbers::riscv32::sys_call_name;
use core::{future::Future, ops::ControlFlow, pin::Pin, str, task::{Context, Poll}};

use exception::{handle_page_fault_from_vmar, PageFaultInfo};
use loader::load_elf_to_vm;
use ostd::{
    cpu::{CpuException, PinCurrentCpu, UserContext}, mm::VmSpace, sync::GuardRwArc, task::{
        disable_preempt, scheduler::blocking_future::BlockingFuture, CurrentTask, JoinHandle, Task, TaskOptions
    }, user::{ReturnReason, UserContextApi, UserMode, UserSpace}
};
use tracing::{debug, error, info,};

use crate::{error::Result, syscall::syscall, thread::{fd_table::{FdTable, StdIoSource}, state::Lifecycle, thread_group::ThreadGroup}, vm::ProcessVm};

#[derive(Clone)]
pub struct ThreadSharedInfo {
    tid: u64,
    parent: Weak<ThreadSharedInfo>,
    children: GuardRwArc<Vec<Arc<ThreadSharedInfo>>>,
    lifecycle: Lifecycle,
    // credentials: Arc<Credentials>,
    // namespaces: Arc<NamespaceInfo>,
    // signal_handling: Arc<SignalHandling>,
    // exit_code: GuardRwLock<Option<i32>>,
    // prof_clock: Arc<ProfClock>,
}

pub struct ThreadState {
    pub task: Arc<Task>,
    pub thread_group: Arc<ThreadGroup>,
    pub shared_info: Arc<ThreadSharedInfo>,
    pub process_vm: Arc<ProcessVm>,
    // pub memory_manager: Arc<MemoryManager>,
    pub fd_table: Arc<FdTable>,
    // pub signal_mask: GuardRwLock<SigSet>,
    // pub signal_actions: GuardRwLock<SigActions>,
    // pub exit_signal: GuardRwLock<Option<SignalInfo>>,
    // pub virtual_memory: Arc<VirtualMemory>,
    // pub futex_state: Arc<Futexes>,
    // pub shared_memory: Arc<ostd_UserSpace>,
    // pub errno: Cell<i32>,
    pub user_brk: usize,
}

pub struct ThreadLocalData {
    pub process_vm: Arc<ProcessVm>,
}

pub trait GetThreadLocalData {
    fn get_thread_local_data(&self) -> Option<&ThreadLocalData>;
}

impl GetThreadLocalData for CurrentTask {
    fn get_thread_local_data(&self) -> Option<&ThreadLocalData> {
        self.local_data().downcast_ref()
    }
}

#[macro_export]
macro_rules! current_thread_data {
    () => {
        {
            let task = ostd::task::Task::current().expect("current task is not found");
            $crate::thread::GetThreadLocalData::get_thread_local_data(&task).expect("thread data is not found")
        }
    }
}

pub struct ThreadBuilder<'a> {
    path: Option<&'a str>,
    argv: Option<Vec<CString>>,
    envp: Option<Vec<CString>>,
}

impl<'a> ThreadBuilder<'a> {
    pub fn new() -> Self {
        Self { path: None, argv: None, envp: None }
    }

    pub fn path(mut self, path: &'a str) -> Self {
        self.path = Some(path);
        self
    }

    #[allow(unused)]
    pub fn argv(mut self, argv: Vec<CString>) -> Self {
        self.argv = Some(argv);
        self
    }

    #[allow(unused)]
    pub fn envp(mut self, envp: Vec<CString>) -> Self {
        self.envp = Some(envp);
        self
    }

    pub async fn spawn(&mut self) -> Result<(Arc<ThreadSharedInfo>, JoinHandle<()>)> {
        let process_vm = Arc::new(ProcessVm::alloc());
        let user_task_options = create_user_task(
            &process_vm,
            self.path.take().unwrap(),
            self.argv.take().unwrap_or_default(),
            self.envp.take().unwrap_or_default(),
        )
        .await
        .inspect_err(|e| {
            error!("create_user_task 失败: {:?}", e);
        })?;
        info!("创建用户任务完成，准备运行");

        let thread_shared_info = Arc::new(ThreadSharedInfo {
            tid: id::alloc(),
            parent: Weak::new(),
            children: GuardRwArc::new(vec![]),
            lifecycle: Lifecycle::new(),
        });
        let thread_group = ThreadGroup::new_leader(thread_shared_info.clone());
        let thread_local_data = ThreadLocalData {
            process_vm: process_vm.clone(),
        };
        let task = Arc::new(user_task_options.local_data(thread_local_data).build());
        let fd_table = FdTable::with_stdio(0, None, Some(StdIoSource::Serial), None).await?;
        let thread_state = ThreadState {
            task: task.clone(),
            thread_group: thread_group.clone(),
            process_vm: process_vm.clone(),
            shared_info: thread_shared_info.clone(),
            user_brk: 0,
            fd_table,
        };

        let vm_space = process_vm.root_vmar().vm_space().clone();
        let future = task_future(thread_state);
        let join_handle = task.run(ThreadFuture::new(vm_space, future));

        Ok((thread_shared_info, join_handle))
    }
}

async fn create_user_task(process_vm: &ProcessVm, path: &str, argv: Vec<CString>, envp: Vec<CString>) -> Result<TaskOptions> {
    info!("开始创建用户任务");
    let elf_load_info = load_elf_to_vm(process_vm, path, argv, envp)
        .await
        .inspect_err(|e| {
            error!("load_elf_to_vm 失败: {:?}", e);
        })?;

    let vm_space = process_vm.root_vmar().vm_space().clone();
    let mut user_context = UserContext::default();
    user_context.set_instruction_pointer(elf_load_info.entry as _);
    user_context.set_stack_pointer(elf_load_info.user_sp as _);
    let user_space = Arc::new(UserSpace::new(vm_space.clone(), user_context));

    info!(
        "创建用户上下文完成，入口点: 0x{:x}",
        user_context.instruction_pointer()
    );

    // Kernel tasks are managed by the Framework,
    // while scheduling algorithms for them can be
    // determined by the users of the Framework.
    info!("构建用户任务");

    Ok(TaskOptions::new().user_space(Some(user_space)))
}

pin_project! {
    struct ThreadFuture<F> {
        vm_space: Arc<VmSpace>,
        #[pin]
        future: F,
    }
}

impl<F> ThreadFuture<F> {
    pub fn new(vm_space: Arc<VmSpace>, future: F) -> Self {
        Self { vm_space, future }
    }
}

impl<F> Future for ThreadFuture<F>
where F: Future<Output = ()> + Send + 'static
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.vm_space.activate();
        self.project().future.poll(cx)
    }
}

pub fn task_future(mut thread_state: ThreadState) -> impl Future<Output = ()> + Send + 'static {
    async move {
        let current = &thread_state.task;
        {
            let disable_preempt = disable_preempt();
            let cpu_id = disable_preempt.current_cpu().as_usize();
            info!("用户任务开始在CPU {} 上执行", cpu_id);
        }
        let user_space = current.user_space().unwrap().clone();
        let mut user_mode = UserMode::new(&user_space);
        info!("创建用户模式完成，准备进入用户空间");
        let current_id = thread_state.shared_info.tid;
    
        let code = loop {
            // The execute method returns when system
            // calls or CPU exceptions occur or some
            // events specified by the kernel occur.
            debug!("准备切换到用户空间执行: {}", current_id);
            let return_reason = user_mode.execute(|| false).await;
            debug!("从用户空间返回，原因: {:?} 线程id: {}", return_reason, current_id);
    
            // The CPU registers of the user space
            // can be accessed and manipulated via
            // the `UserContext` abstraction.
            let user_context = user_mode.context_mut();
            if return_reason == ReturnReason::UserException {
                if user_context.trap_information().code == CpuException::UserEnvCall {
                    let syscall_number = user_context.syscall_number() as i64;
                    debug!("处理系统调用，系统调用号: {}， {}", syscall_number, sys_call_name(syscall_number).unwrap_or("unknown"));
                    let res = syscall(&mut thread_state, user_context).await;
                    match res {
                        Ok(ControlFlow::Continue(Some(ret))) => {
                            user_context.set_syscall_return_value(ret as _);
                        }
                        Ok(ControlFlow::Continue(None)) => {}
                        Ok(ControlFlow::Break(code)) => {
                            break code;
                        }
                        Err(e) => {
                            error!("系统调用失败: {:?}", e);
                            let ret = e.downcast_ref::<Error>().map(|e| e.error() as _).unwrap_or(-1);
                            user_context.set_syscall_return_value(ret as _);
                        }
                    }
                } else {
                    debug!("处理异常");
                    if let Ok(page_fault_info) =
                        PageFaultInfo::try_from(user_context.trap_information())
                    {
                        if handle_page_fault_from_vmar(
                            &thread_state.process_vm.root_vmar(),
                            &page_fault_info,
                        )
                        .block()
                        .is_err() {
                            error!("处理页错误失败");
                        }
                    }
                }
            }
        };
        // 在用户 loop 跳出（正常或错误）后，进行收尾
        thread_state.shared_info.lifecycle.exit(code);
    }
}