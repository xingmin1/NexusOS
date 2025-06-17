// SPDX-License-Identifier: MPL-2.0

//! Architecture dependent CPU-local information utilities.

// NOTE: 使用'gp'（全局指针）实现CPU本地存储可能不符合标准（标准应使用'tp'线程指针）
// 当前可行的原因：
// 1. set_base将基地址存入gp
// 2. 陷阱处理程序(trap.S)在用户态陷入时不会覆盖内核的gp
// TODO: 需要重构为使用'tp'，需修改trap.S和上下文切换代码(switch.S)来处理tp的保存/恢复

pub(crate) unsafe fn set_base(addr: u64) {
    core::arch::asm!(
        "mv gp, {addr}",
        addr = in(reg) addr,
        options(preserves_flags, nostack)
    );
}

pub(crate) fn get_base() -> u64 {
    let mut gp;
    unsafe {
        core::arch::asm!(
            "mv {gp}, gp",
            gp = out(reg) gp,
            options(preserves_flags, nostack)
        );
    }
    gp
}
