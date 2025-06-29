use core::ops::ControlFlow;
use core::sync::atomic::Ordering::Relaxed;

use nexus_error::{errno_with_message, Errno, Result};
use ostd::user::UserContextApi;

use crate::{
    thread::ThreadState,
    time::{ticks_since_boot, tms::Tms},
};

pub async fn do_times(
    state: &mut ThreadState,
    uc: &mut ostd::cpu::UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    let buf_ptr = uc.syscall_arguments()[0] as usize;

    if buf_ptr != 0 {
        // 当前进程 CPU 时间（简化：以启动至今的嘀嗒近似）
        let elapsed = ticks_since_boot() - state.shared_info.start_ticks;
        let src = {
            let cputime = &state.shared_info.cpu_times;
            Tms {
                tms_utime:  cputime.utime.load(Relaxed).max(elapsed),
                tms_stime:  cputime.stime.load(Relaxed),
                tms_cutime: cputime.cutime.load(Relaxed),
                tms_cstime: cputime.cstime.load(Relaxed),
            }
        };

        state
            .process_vm
            .write_val(buf_ptr as _, &src)
            .map_err(|_| errno_with_message(Errno::EFAULT, "invalid tms pointer"))?;
    }

    let ticks = ticks_since_boot() as isize;
    
    Ok(ControlFlow::Continue(Some(ticks)))
}
