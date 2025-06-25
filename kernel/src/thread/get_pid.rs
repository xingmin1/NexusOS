use core::ops::ControlFlow;

use nexus_error::Result;

use crate::thread::ThreadState;

pub async fn do_getpid(
    state: &mut ThreadState,
    _uc: &mut ostd::cpu::UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    Ok(ControlFlow::Continue(Some(
        state
            .thread_group
            .id() as isize,
    )))
}
