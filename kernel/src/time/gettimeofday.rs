use core::ops::ControlFlow;

use nexus_error::{errno_with_message, Errno, Result};
use ostd::user::UserContextApi;

use crate::{
    thread::ThreadState,
    time::{duration_since_boot, Timespec},
};

pub async fn do_gettimeofday(
    state: &mut ThreadState,
    uc: &mut ostd::cpu::UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    let buf_ptr = uc.syscall_arguments()[0] as usize;

    if buf_ptr != 0 {
        let now = duration_since_boot();
        let src: Timespec = now.into();

        state
            .process_vm
            .write_val(buf_ptr as _, &src)
            .map_err(|_| errno_with_message(Errno::EFAULT, "invalid timeval pointer"))?;
    }

    Ok(ControlFlow::Continue(Some(0)))  
}
