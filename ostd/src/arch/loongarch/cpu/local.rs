// SPDX-License-Identifier: MPL-2.0

//! Architecture dependent CPU-local information utilities.

pub(crate) fn get_base() -> u64 {
    let mut addr;
    unsafe {
        core::arch::asm!(
            "move {addr}, $r21",
            addr = out(reg) addr,
            options(preserves_flags, nostack)
        );
    }
    addr
}

pub(crate) fn set_base(base: u64) {
    unsafe {
        core::arch::asm!(
            "move $r21, {base}",
            base = in(reg) base,
            options(preserves_flags, nostack)
        );
    }
}