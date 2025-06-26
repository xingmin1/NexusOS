use core::ops::ControlFlow;

use alloc::sync::Arc;
use crate::thread::{ThreadState, ThreadSharedInfo};
use nexus_error::{errno_with_message, Errno, Result};
use ostd::user::UserContextApi;

/// 简化：仅支持 `pid == -1` 或子进程 tid
pub async fn do_wait4(state: &mut ThreadState, uc: &mut ostd::cpu::UserContext) -> Result<ControlFlow<i32, Option<isize>>> {
    let [pid, status_ptr, options, ..] = uc.syscall_arguments();

    if options & 0x01 != 0 { /* WNOHANG */
        if let Some((tid, code)) = try_collect(&state.shared_info, pid).await? {
            tracing::info!("wait4: tid={}, code={}", tid, code);
            copy_status(state, status_ptr, (code & 0xff) << 8);
            return Ok(ControlFlow::Continue(Some(tid as isize)));
        }
        return Ok(ControlFlow::Continue(Some(0))); // 立即返回
    }

    loop {
        if let Some((tid, code)) = try_collect(&state.shared_info, pid).await? {
            tracing::info!("wait4: tid={}, code={}", tid, code);
            copy_status(state, status_ptr, (code & 0xff) << 8);
            return Ok(ControlFlow::Continue(Some(tid as isize)));
        }
        // 还未退出，阻塞等待
        state.shared_info.lifecycle.wait().await;
    }

    async fn try_collect(parent: &ThreadSharedInfo, pid: usize) -> Result<Option<(u64, i32)>> {
        let mut child_to_wait: Option<Arc<ThreadSharedInfo>> = None;
        let mut has_matching_child = false;

        {
            let children = parent.children.read();
            for child in children.iter() {
                if pid_match(pid, child.tid) {
                    has_matching_child = true;
                    child_to_wait = Some(child.clone());
                }
            }
        }

        if let Some(child) = child_to_wait {
            let code = child.lifecycle.wait().await;
            parent.children.write().retain(|c| c.tid != child.tid);
            return Ok(Some((child.tid, code)));
        }

        if has_matching_child {
            Ok(None)
        } else {
            Err(errno_with_message(Errno::ECHILD, "No child process found"))
        }
    }

    fn pid_match(request: usize, tid: u64) -> bool {
        request as i64 == -1 || request as u64 == tid
    }

    fn copy_status(ts: &mut ThreadState, status_ptr: usize, code: i32) {
        if status_ptr != 0 {
            ts.process_vm.write_val(status_ptr as _, &code).unwrap();
        }
    }
}
