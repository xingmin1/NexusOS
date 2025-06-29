use core::ops::ControlFlow;

use nexus_error::{errno_with_message, Errno, Result};
use ostd::{user::UserContextApi, Pod};

use crate::thread::ThreadState;

/// 每个字段固定 65B
pub const UTS_LEN: usize = 65;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod)]
struct Utsname {
    sysname:     [u8; UTS_LEN],
    nodename:    [u8; UTS_LEN],
    release:     [u8; UTS_LEN],
    version:     [u8; UTS_LEN],
    machine:     [u8; UTS_LEN],
    domainname:  [u8; UTS_LEN],
}

impl Utsname {
    fn current() -> Self {
        const SYSNAME:     &str = "NexusOS";
        const NODENAME:    &str = "localhost";
        const RELEASE:     &str = "0.1.0";
        const VERSION:     &str = "0.1.0";
        const MACHINE:     &str = "riscv32";
        const DOMAINNAME:  &str = "";

        fn fill(src: &str) -> [u8; UTS_LEN] {
            let mut buf = [0u8; UTS_LEN];
            let bytes = src.as_bytes();
            let len = bytes.len().min(UTS_LEN - 1); // 预留结尾 \0
            buf[..len].copy_from_slice(&bytes[..len]);
            buf
        }

        Self {
            sysname:    fill(SYSNAME),
            nodename:   fill(NODENAME),
            release:    fill(RELEASE),
            version:    fill(VERSION),
            machine:    fill(MACHINE),
            domainname: fill(DOMAINNAME),
        }
    }
}

pub async fn do_uname(
    state: &mut ThreadState,
    uc: &mut ostd::cpu::UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    let [buf_ptr, ..] = uc.syscall_arguments();
    if buf_ptr == 0 {
        return Err(errno_with_message(Errno::EFAULT, "buf is NULL"));
    }

    // 写入用户缓冲区；内部包含完整越界/页故障检测
    state
        .process_vm
        .write_val(buf_ptr as _, &Utsname::current())?;

    Ok(ControlFlow::Continue(Some(0)))
}
