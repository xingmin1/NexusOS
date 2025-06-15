/* SPDX-License-Identifier: MPL-2.0 */

//! Platform-Level Interrupt Controller (PLIC) 驱动（支持多核 & DTB 自动探测）
//
//! 设计目标：
//! 1. 运行时零锁的 claim/complete 快速路径；
//! 2. 支持任意数量 hart/context & IRQ；
//! 3. 自动从 DTB 解析基址/IRQ 数/context 数；
//! 4. 与现有 IrqLine 框架保持兼容（提供旧版 enable/claim/complete 包装）。
//!
//! 目前仅实现 S-mode context（index = hart * 2 + 1），但整体框架已预留多模式、
//! 多实例扩展点。

use core::ptr::{read_volatile, write_volatile};

use riscv::register::mhartid;
use spin::Once;

use crate::arch::boot::DEVICE_TREE;

mod regs;
use regs::*;

/// 全局唯一的 PLIC 实例
static PLIC: Once<Plic> = Once::new();

/// PLIC 结构体（只包含只读元数据；并发安全）
pub struct Plic {
    base: usize,
    num_irqs: u32,
    num_contexts: u32,
}

impl Plic {
    /// BSP 在早期调用，全局一次性初始化
    ///
    /// # Safety
    ///
    /// 调用者保证 `base` 为有效的 PLIC MMIO 起始物理地址，并且在
    /// 之后保持映射 & 有效。
    pub unsafe fn init_global(base: usize, num_irqs: u32, num_contexts: u32) -> &'static Self {
        PLIC.call_once(|| Plic {
            base,
            num_irqs,
            num_contexts,
        });

        let me = PLIC.get().unwrap();
        // 将所有 IRQ 优先级清零
        for irq in 1..=num_irqs {
            write32(me.priority_ptr(irq), 0);
        }
        me
    }

    /// 各 hart 在进入 S-mode 前调用
    pub fn per_hart_init(&self, hart: usize) {
        self.set_threshold(hart, 0);
        // 关闭全部 IRQ
        let words = ((self.num_irqs + 31) / 32) as usize;
        for w in 0..words {
            self.write_enable_word(hart, w as u32, 0);
        }
    }

    /* ---------- 高层 API ---------- */

    #[inline]
    pub fn enable(&self, hart: usize, irq: u32) {
        if irq == 0 || irq > self.num_irqs {
            return;
        }
        let word = irq / 32;
        let bit = irq % 32;
        let ptr = self.enable_ptr(hart, word);
        unsafe {
            let val = read32(ptr);
            write32(ptr, val | (1 << bit));
        }
    }

    #[inline]
    pub fn disable(&self, hart: usize, irq: u32) {
        if irq == 0 || irq > self.num_irqs {
            return;
        }
        let word = irq / 32;
        let bit = irq % 32;
        let ptr = self.enable_ptr(hart, word);
        unsafe {
            let val = read32(ptr);
            write32(ptr, val & !(1 << bit));
        }
    }

    #[inline]
    pub fn set_priority(&self, irq: u32, prio: u8) {
        if irq == 0 || irq > self.num_irqs {
            return;
        }
        write32(self.priority_ptr(irq), prio as u32);
    }

    #[inline(always)]
    pub fn claim(&self) -> u32 {
        unsafe { read32(self.claim_ptr(hart_id())) }
    }

    #[inline(always)]
    pub fn complete(&self, id: u32) {
        if id != 0 {
            unsafe { write32(self.claim_ptr(hart_id()), id) }
        }
    }

    /* ---------- 内部 addr 帮助函数 ---------- */

    #[inline(always)]
    const fn priority_ptr(&self, irq: u32) -> *mut u32 {
        (self.base + PRIORITY_BASE + irq as usize * PRIORITY_STRIDE) as *mut u32
    }

    #[inline(always)]
    const fn enable_ptr(&self, hart: usize, word: u32) -> *mut u32 {
        (self.base
            + ENABLE_BASE
            + hart * ENABLE_STRIDE
            + (word as usize) * 4) as *mut u32
    }

    #[inline(always)]
    const fn threshold_ptr(&self, hart: usize) -> *mut u32 {
        (self.base + CONTEXT_BASE + hart * CONTEXT_STRIDE) as *mut u32
    }

    #[inline(always)]
    const fn claim_ptr(&self, hart: usize) -> *mut u32 {
        (self.threshold_ptr(hart) as usize + 4) as *mut u32
    }

    #[inline]
    fn set_threshold(&self, hart: usize, thr: u8) {
        write32(self.threshold_ptr(hart), thr as u32);
    }

    #[inline]
    fn write_enable_word(&self, hart: usize, word: u32, val: u32) {
        unsafe { write32(self.enable_ptr(hart, word), val) };
    }
}

/* ---------- 向旧接口兼容的包裹函数 ---------- */

/// 从 DTB 探测 PLIC 信息，返回 (base, nr_irqs, nr_contexts)
pub fn probe_from_fdt() -> Option<(usize, u32, u32)> {
    let fdt = DEVICE_TREE.get()?;
    let node = fdt
        .find_node("/soc/interrupt-controller")
        .or_else(|| fdt.find_node("/interrupt-controller"))?;

    let reg = node.reg()?.next()?;
    let base = reg.starting_address as usize;
    let mut num_irqs = node.property_u32("riscv,ndev").unwrap_or(0);
    if num_irqs == 0 {
        // 保底：QEMU virt 默认 53
        num_irqs = 53;
    }
    let num_ctx = node
        .interrupts_extended()
        .map(|it| it.count() as u32 / 2)
        .unwrap_or(1);
    Some((base, num_irqs, num_ctx))
}

/// 全局初始化包装
pub unsafe fn init_global(base: usize, nirq: u32, nctx: u32) {
    Plic::init_global(base, nirq, nctx);
}

/// 每 hart 初始化包装
pub fn per_hart_init(hart: usize) {
    if let Some(p) = PLIC.get() {
        p.per_hart_init(hart);
    }
}

/// enable 包装（沿用旧 API）
#[inline]
pub fn enable(irq: u32) {
    if let Some(p) = PLIC.get() {
        p.enable(hart_id(), irq);
    }
}

/// disable 包装
#[inline]
pub fn disable(irq: u32) {
    if let Some(p) = PLIC.get() {
        p.disable(hart_id(), irq);
    }
}

/// claim 包装
#[inline(always)]
pub fn claim() -> u32 {
    PLIC.get().map(|p| p.claim()).unwrap_or(0)
}

/// complete 包装
#[inline(always)]
pub fn complete(id: u32) {
    if let Some(p) = PLIC.get() {
        p.complete(id);
    }
}

/// 当前 hart id
#[inline(always)]
fn hart_id() -> usize {
    mhartid::read()
}
