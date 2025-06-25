use core::ops::ControlFlow;

use nexus_error::Result;

use crate::thread::ThreadState;

pub async fn do_getppid(
    state: &mut ThreadState,
    _uc: &mut ostd::cpu::UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    Ok(ControlFlow::Continue(Some(
        state
            .shared_info
            .parent
            .upgrade()
            .map_or_else(|| 1, |parent| parent.tid) as isize,
    )))
}
