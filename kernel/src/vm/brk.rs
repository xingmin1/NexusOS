use core::ops::ControlFlow;

use nexus_error::Result;
use ostd::user::UserContextApi;
use tracing::debug;

use crate::thread::ThreadState;

pub async fn do_brk(state: &mut ThreadState, context: &mut ostd::cpu::UserContext) -> Result<ControlFlow<i32, Option<isize>>>  {
    let [new_brk, ..] = context.syscall_arguments();

    let new_heap_end = if new_brk == 0 {
        None
    } else {
        Some(new_brk as usize)
    };

    debug!("new heap end = {:x?}", new_brk);

    let new_heap_end = state
        .process_vm
        .heap
        .brk(new_heap_end, state.process_vm.root_vmar())
        .await?;

    Ok(ControlFlow::Continue(Some(new_heap_end as isize)))
}
