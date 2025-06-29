mod fs;

use core::{ffi::c_long, ops::ControlFlow};

use nexus_error::error_stack::ResultExt;
use nexus_error::{errno_with_message, Result};
use ostd::{cpu::UserContext, user::UserContextApi};
use syscall_numbers::native::*;
use tracing::warn;

use crate::syscall::fs::{do_chdir, do_close, do_dup, do_dup3, do_fstat, do_getcwd, do_getdents64, do_linkat, do_mkdirat, do_mount, do_openat, do_pipe2, do_read, do_umount2, do_unlinkat, do_write};
use crate::thread::{clone::do_clone, execve::do_execve, get_pid::do_getpid, get_ppid::do_getppid, sched_yield::do_sched_yield, wait::do_wait4, ThreadState};
use crate::thread::exit::do_exit;

#[allow(non_upper_case_globals)]

/// 系统调用处理函数
/// 
/// 返回值:
/// - Ok(ControlFlow::Continue(Some(isize))) 表示系统调用成功，并继续运行，返回值为 isize
/// - Ok(ControlFlow::Continue(None)) 表示系统调用成功，并继续运行，无返回值
/// - Ok(ControlFlow::Break(i32)) 表示系统调用成功，并退出运行，结束码为 i32
/// - Err(Report) 表示系统调用失败，并返回一个错误
pub async fn syscall(state: &mut ThreadState, context: &mut UserContext) -> Result<ControlFlow<i32, Option<isize>>> {
    match context.syscall_number() as c_long {
        SYS_clone => do_clone(state, context).await,
        SYS_wait4 => do_wait4(state, context).await,
        SYS_exit => do_exit(state, context, false).await,
        SYS_exit_group => do_exit(state, context, true).await,
        SYS_execve => do_execve(state, context).await,
        SYS_getpid => do_getpid(state, context).await,
        SYS_getppid => do_getppid(state, context).await,
        SYS_openat => do_openat(state, context).await,
        SYS_close => do_close(state, context).await,
        SYS_read => do_read(state, context).await,
        SYS_write => do_write(state, context).await,
        SYS_getdents64 => do_getdents64(state, context).await,
        SYS_linkat => do_linkat(state, context).await,
        SYS_unlinkat => do_unlinkat(state, context).await,
        SYS_mkdirat => do_mkdirat(state, context).await,
        SYS_mount => do_mount(state, context).await,
        SYS_umount2 => do_umount2(state, context).await,
        SYS_fstat => do_fstat(state, context).await,
        SYS_sched_yield => do_sched_yield(state, context).await,
        SYS_getcwd => do_getcwd(state, context).await,
        SYS_pipe2 => do_pipe2(state, context).await,
        SYS_dup => do_dup(state, context).await,
        SYS_dup3 => do_dup3(state, context).await,
        SYS_chdir => do_chdir(state, context).await,
        num => {
            warn!("syscall not implemented: number={}, name={}, args={:?}", num, sys_call_name(num).unwrap_or("unknown"), context.syscall_arguments());
            Err(errno_with_message(nexus_error::Errno::ENOSYS, "syscall not implemented")).attach_printable_lazy(|| {
                alloc::format!("number={}, name={}, args={:?}", num, sys_call_name(num).unwrap_or("unknown"), context.syscall_arguments())
            })
        }
    }
}