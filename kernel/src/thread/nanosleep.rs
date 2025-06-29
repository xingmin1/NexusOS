use core::{ops::ControlFlow, time::Duration};

use nexus_error::{errno_with_message, Errno, Result};
use ostd::{task::sleep, user::UserContextApi};

use crate::{
    thread::ThreadState,
    time::Timespec,
};

pub async fn do_nanosleep(
    state: &mut ThreadState,
    uc: &mut ostd::cpu::UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    let req_time = uc.syscall_arguments()[0] as usize;
    let _rem_time = uc.syscall_arguments()[1] as usize;

    if req_time == 0 {
        return Err(errno_with_message(Errno::EINVAL, "invalid time"));
    }

    let vm = &state.process_vm;
    let req_time = vm.read_val::<Timespec>(req_time as _).map_err(|_| errno_with_message(Errno::EFAULT, "invalid time"))?;

    let req_time = Duration::from(req_time);

    sleep(req_time).await;

    Ok(ControlFlow::Continue(Some(0))) 
}
