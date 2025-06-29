use core::ops::ControlFlow;

use nexus_error::{return_errno_with_message, Errno, Result};
use ostd::user::UserContextApi;
use tracing::debug;
use ostd::mm::PAGE_SIZE;
use crate::thread::ThreadState;

pub async fn do_munmap(
    state: &ThreadState,
    cx: &mut ostd::cpu::UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    let [addr, len, ..] = cx.syscall_arguments();
    let addr = addr;
    let len  = len;

    if addr % PAGE_SIZE != 0 || len == 0 || len > isize::MAX as usize {
        return_errno_with_message!(Errno::EINVAL, "bad addr/len");
    }
    let len = len.next_multiple_of(PAGE_SIZE);
    debug!("munmap {:x} + {:#x}", addr, len);

    state.process_vm.root_vmar().remove_mapping(addr..addr + len).await?;
    Ok(ControlFlow::Continue(Some(0)))
}
