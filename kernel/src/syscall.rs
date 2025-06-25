use core::ffi::c_long;

use nexus_error::{return_errno_with_message, Result};
use ostd::{cpu::UserContext, user::UserContextApi};
use syscall_numbers::native::*;

use crate::thread::{clone::do_clone, ThreadState};

/// 系统调用处理函数
/// 
/// 返回值:
/// - Ok(None) 表示系统调用成功，但不需要返回值
/// - Ok(Some(isize)) 表示系统调用成功，并返回一个isize值
/// - Err(Report) 表示系统调用失败，并返回一个错误
pub async fn syscall(state: &mut ThreadState, context: &mut UserContext) -> Result<Option<isize>> {
    match context.syscall_number() as c_long {
        SYS_clone => {
            let res = do_clone(state, context).await?;
            Ok(Some(res as isize))
        }
        _ => {
            return_errno_with_message!(nexus_error::Errno::ENOSYS, "syscall not implemented")
        }
    }
}