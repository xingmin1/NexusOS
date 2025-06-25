use core::{ffi::c_long, ops::ControlFlow};

use nexus_error::{return_errno_with_message, Result};
use ostd::{cpu::UserContext, user::UserContextApi};
use syscall_numbers::native::*;

use crate::thread::{clone::do_clone, ThreadState};

/// 系统调用处理函数
/// 
/// 返回值:
/// - Ok(ControlFlow::Continue(isize)) 表示系统调用成功，并继续运行，返回值为 isize
/// - Ok(ControlFlow::Break(i32)) 表示系统调用成功，并退出运行，结束码为 i32
/// - Err(Report) 表示系统调用失败，并返回一个错误
pub async fn syscall(state: &mut ThreadState, context: &mut UserContext) -> Result<ControlFlow<i32, isize>> {
    match context.syscall_number() as c_long {
        SYS_clone => do_clone(state, context).await,
        _ => {
            return_errno_with_message!(nexus_error::Errno::ENOSYS, "syscall not implemented")
        }
    }
}