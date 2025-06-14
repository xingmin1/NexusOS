// SPDX-License-Identifier: MPL-2.0

//! Platform-Level Interrupt Controller (PLIC) minimal support for RISC-V virt machine.
//!
//! 目前仅实现如下功能：
//! 1. `enable`  ‑ 为指定 IRQ 设置优先级=1 并在 hart0 的 enable 寄存器打开中断位。
//! 2. `claim`   ‑ 从 hart0 的 claim 寄存器读取 pending IRQ，0 表示无可用。
//! 3. `complete`- 写回 claim 寄存器通知 PLIC 处理完毕。
//!
//! 本实现仅覆盖 QEMU virt 默认 PLIC，基址 `0x0c00_0000`，先仅只使用 hart0。
//! 后续可根据实际 SoC 通过 FDT 解析基址和 hart ID 并扩展。

use core::ptr::{read_volatile, write_volatile};

/// PLIC MMIO 基址（QEMU virt 默认值）
const PLIC_BASE: usize = 0x0c00_0000;
/// 每个 IRQ 的优先级寄存器偏移 = 4 * irq_id
const PRIORITY_OFFSET: usize = 0x0;
/// enable array 基址
const ENABLE_BASE: usize = 0x2000;
/// 一个 hart 的 enable 寄存器跨度
const ENABLE_STRIDE: usize = 0x80;
/// context (threshold/claim) 基址
const CONTEXT_BASE: usize = 0x20_0000;
/// 一个 hart 的 context 寄存器跨度
const CONTEXT_STRIDE: usize = 0x1000;

/// 为 hart0 获取 enable 寄存器地址
#[inline(always)]
fn hart0_enable_addr(irq_id: u32) -> *mut u32 {
    let word_index = irq_id as usize / 32;
    (PLIC_BASE + ENABLE_BASE + word_index * 4) as *mut u32
}

/// 为 hart0 获取 priority 寄存器地址
#[inline(always)]
fn priority_addr(irq_id: u32) -> *mut u32 {
    (PLIC_BASE + PRIORITY_OFFSET + irq_id as usize * 4) as *mut u32
}

/// hart0 threshold 寄存器地址
#[inline(always)]
fn threshold_addr() -> *mut u32 {
    (PLIC_BASE + CONTEXT_BASE) as *mut u32
}

/// hart0 claim/complete 寄存器地址
#[inline(always)]
fn claim_addr() -> *mut u32 {
    (PLIC_BASE + CONTEXT_BASE + 4) as *mut u32
}

/// 启用指定 IRQ（irq_id>0）
pub fn enable(irq_id: u32) {
    assert!(irq_id > 0);
    unsafe {
        // set priority = 1
        write_volatile(priority_addr(irq_id), 1u32);

        // enable bit in enable register
        let enable_reg = hart0_enable_addr(irq_id);
        let current = read_volatile(enable_reg);
        write_volatile(enable_reg, current | (1u32 << (irq_id % 32)));

        // set threshold = 0 (do once)
        write_volatile(threshold_addr(), 0);
    }
}

/// 从 PLIC 读取 pending IRQ，0 表示没有。
#[inline]
pub fn claim() -> u32 {
    unsafe { read_volatile(claim_addr()) }
}

/// 通知 PLIC 完成 IRQ 处理。
#[inline]
pub fn complete(irq_id: u32) {
    if irq_id != 0 {
        unsafe {
            write_volatile(claim_addr(), irq_id);
        }
    }
} 