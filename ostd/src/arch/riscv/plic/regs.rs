/* SPDX-License-Identifier: MPL-2.0 */

//! PLIC MMIO 寄存器常量 & 低级读写

#[inline(always)]
pub unsafe fn read32(addr: *const u32) -> u32 {
    core::ptr::read_volatile(addr)
}

#[inline(always)]
pub unsafe fn write32(addr: *mut u32, val: u32) {
    core::ptr::write_volatile(addr, val);
}

/* ------- 常量 ------- */

pub const PRIORITY_BASE: usize = 0x0000;
pub const PRIORITY_STRIDE: usize = 0x4;

pub const ENABLE_BASE: usize = 0x2000;
pub const ENABLE_STRIDE: usize = 0x80;

pub const CONTEXT_BASE: usize = 0x200000;
pub const CONTEXT_STRIDE: usize = 0x1000;
