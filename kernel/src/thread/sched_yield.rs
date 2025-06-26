use core::ops::ControlFlow;

use nexus_error::Result;
use ostd::cpu::UserContext;

use crate::thread::ThreadState;

pub async fn do_sched_yield(_state: &mut ThreadState, _context: &mut UserContext) -> Result<ControlFlow<i32, Option<isize>>> {
    ostd::task::scheduler::yield_now().await;
    Ok(ControlFlow::Continue(Some(0)))
}