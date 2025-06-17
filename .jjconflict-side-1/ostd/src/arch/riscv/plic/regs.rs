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

/* -------- 规范规定的偏移/步长 --------
 * 见 RISC-V PLIC Spec §5.1 及 SiFive “Interrupt-Cookbook” §2.1 */
pub const PRIORITY_OFFSET: usize  = 0x0;
pub const PRIORITY_STRIDE: usize  = 0x4;

pub const ENABLE_OFFSET: usize    = 0x2000;
pub const ENABLE_STRIDE: usize    = 0x80;     // 一个 context 的 IE 区域 = 0x80B

pub const CONTEXT_OFFSET: usize   = 0x20_0000;
pub const CONTEXT_STRIDE: usize   = 0x1000;   // 一个 context 的门限+claim = 0x1000B

/* ---------- 便捷地址计算 ---------- */
#[inline(always)]
pub const fn priority_addr(base: usize, irq: u32) -> *mut u32 {
    (base + PRIORITY_OFFSET + irq as usize * PRIORITY_STRIDE) as *mut u32
}
#[inline(always)]
pub const fn enable_word_addr(base: usize, ctx: u32, word: u32) -> *mut u32 {
    let off = ENABLE_OFFSET + ENABLE_STRIDE * ctx as usize + (word * 4) as usize;
    (base + off) as *mut u32
}
#[inline(always)]
pub const fn threshold_addr(base: usize, ctx: u32) -> *mut u32 {
    (base + CONTEXT_OFFSET + CONTEXT_STRIDE * ctx as usize) as *mut u32
}
#[inline(always)]
pub const fn claim_addr(base: usize, ctx: u32) -> *mut u32 {
    (base + CONTEXT_OFFSET + CONTEXT_STRIDE * ctx as usize + 4) as *mut u32
}
