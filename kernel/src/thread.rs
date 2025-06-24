pub mod exception;
pub mod init_stack;
pub mod loader;

use alloc::{
    ffi::CString, sync::{Arc, Weak}, vec::Vec, vec,
};
use core::str;

use exception::{handle_page_fault_from_vmar, PageFaultInfo};
use loader::load_elf_to_vm;
use ostd::{
    arch::qemu::{exit_qemu, QemuExitCode},
    cpu::{CpuException, PinCurrentCpu, UserContext},
    mm::{FallibleVmRead, VmSpace, VmWriter},
    sync::GuardRwArc,
    task::{
        disable_preempt, scheduler::blocking_future::BlockingFuture, CurrentTask, Task, TaskOptions,
    },
    user::{ReturnReason, UserContextApi, UserMode, UserSpace},
};
use tracing::{debug, error, info, warn};

use crate::{error::Result, vm::ProcessVm};

#[derive(Clone)]
pub struct ThreadSharedInfo {
    tid: u64,
    parent: Weak<ThreadSharedInfo>,
    children: GuardRwArc<Vec<Arc<ThreadSharedInfo>>>,
    // credentials: Arc<Credentials>,
    // namespaces: Arc<NamespaceInfo>,
    // signal_handling: Arc<SignalHandling>,
    // exit_code: GuardRwLock<Option<i32>>,
    // prof_clock: Arc<ProfClock>,
}

pub struct ThreadState {
    pub task: Arc<Task>,
    // pub thread_group: Arc<ThreadGroup>,
    pub shared_info: Arc<ThreadSharedInfo>,
    pub process_vm: Arc<ProcessVm>,
    // pub memory_manager: Arc<MemoryManager>,
    // pub file_table: Arc<FileTable>,
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

    pub async fn spawn(&mut self) -> Result<Arc<ThreadSharedInfo>> {
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
            tid: 0,
            parent: Weak::new(),
            children: GuardRwArc::new(vec![]),
        });
        let thread_local_data = ThreadLocalData {
            process_vm: process_vm.clone(),
        };
        let task = Arc::new(user_task_options.local_data(thread_local_data).build());
        let thread_state = ThreadState {
            task: task.clone(),
            process_vm: process_vm.clone(),
            shared_info: thread_shared_info.clone(),
            user_brk: 0,
        };

        let task_future = async move {
            let current = &thread_state.task;
            {
                let disable_preempt = disable_preempt();
                let cpu_id = disable_preempt.current_cpu().as_usize();
                info!("用户任务开始在CPU {} 上执行", cpu_id);
            }
            let user_space = current.user_space().unwrap();
            let mut user_mode = UserMode::new(user_space);
            info!("创建用户模式完成，准备进入用户空间");

            loop {
                // The execute method returns when system
                // calls or CPU exceptions occur or some
                // events specified by the kernel occur.
                debug!("准备切换到用户空间执行");
                let return_reason = user_mode.execute(|| false).await;
                debug!("从用户空间返回，原因: {:?}", return_reason);

                // The CPU registers of the user space
                // can be accessed and manipulated via
                // the `UserContext` abstraction.
                let user_context = user_mode.context_mut();
                if return_reason == ReturnReason::UserException {
                    if user_context.trap_information().code == CpuException::UserEnvCall {
                        debug!("处理系统调用，系统调用号: {}", user_context.a7());
                        handle_syscall(
                            user_context,
                            &thread_state.process_vm.root_vmar().vm_space(),
                        );
                    } else {
                        debug!("处理异常");
                        if let Ok(page_fault_info) =
                            PageFaultInfo::try_from(user_context.trap_information())
                        {
                            let _ = handle_page_fault_from_vmar(
                                &thread_state.process_vm.root_vmar(),
                                &page_fault_info,
                            )
                            .block();
                        }
                    }
                }
            }
        };
        let _join_handle = task.run(task_future);

        Ok(thread_shared_info)
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

fn handle_syscall(user_context: &mut UserContext, vm_space: &VmSpace) {
    const SYS_WRITE: usize = 64;
    const SYS_EXIT: usize = 93;

    // RISC-V的系统调用号放在a7寄存器中
    match user_context.a7() {
        SYS_WRITE => {
            // RISC-V，系统调用参数放在a0-a6寄存器中
            let (fd, buf_addr, buf_len) = (user_context.a0(), user_context.a1(), user_context.a2());
            debug!(
                "处理write系统调用: fd={}, buf_addr=0x{:x}, buf_len={}",
                fd, buf_addr, buf_len
            );
            let buf = {
                let mut buf = vec![0u8; buf_len];
                // Copy data from the user space without
                // unsafe pointer dereferencing.
                let mut reader = vm_space.reader(buf_addr, buf_len).unwrap();
                reader
                    .read_fallible(&mut VmWriter::from(&mut buf as &mut [u8]))
                    .unwrap();
                debug!("从用户空间读取数据成功，长度: {}", buf_len);
                buf
            };
            // Use the console for output safely.
            let content = str::from_utf8(&buf).unwrap();
            info!("用户程序输出: {}", content);
            // Manipulate the user-space CPU registers safely.
            user_context.set_a0(buf_len);
            debug!("write系统调用处理完成，返回值: {}", buf_len);
        }
        SYS_EXIT => {
            let exit_code = user_context.a0();
            info!("处理exit系统调用，退出码: {}", exit_code);
            exit_qemu(QemuExitCode::Success);
        }
        syscall_num => {
            warn!("未实现的系统调用: {}", syscall_num);
            unimplemented!();
        }
    }
}
