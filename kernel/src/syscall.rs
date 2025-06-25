use core::{ffi::c_long, ops::ControlFlow};

use nexus_error::{return_errno_with_message, Result};
use ostd::{cpu::UserContext, user::UserContextApi};
use syscall_numbers::native::*;

use crate::thread::{clone::do_clone, execve::do_execve, get_pid::do_getpid, get_ppid::do_getppid, wait::do_wait4, ThreadState};
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
        _ => {
            return_errno_with_message!(nexus_error::Errno::ENOSYS, "syscall not implemented")
        }
    }
}