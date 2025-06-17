//! arch/riscv/plic/mod.rs
//! 高层安全 API + Once 静态单例

#![allow(dead_code)]

mod regs;
mod fdt;

use ::fdt::Fdt;
use regs::*;
use spin::{Once, Mutex};
use core::sync::atomic::{fence, Ordering};
use alloc::{boxed::Box, vec::Vec};

/// 全局单例
static PLIC: Once<Plic> = Once::new();

/// 初始化（仅 BSP 调用一次）
pub unsafe fn init_global(fdt: &Fdt) {
    use crate::mm::paddr_to_vaddr;

    let info = fdt::parse(&fdt);
    crate::early_println!("[PLIC] Info: base={:#x}, num_irqs={}", info.base, info.num_irqs);

    let virt_base = paddr_to_vaddr(info.base);
    crate::early_println!("[PLIC] Virtual base: {:#x}", virt_base);

    /* 预生成 enable word 锁 */
    let words = ((info.num_irqs + 31) / 32) as usize;
    crate::early_println!("[PLIC] Allocating {} enable word locks", words);
    
    let locks = (0..words)
        .map(|_| Mutex::new(()))
        .collect::<Vec<_>>()
        .into_boxed_slice();

    PLIC.call_once(|| {
        crate::early_println!("[PLIC] Initialized: num_irqs={}", info.num_irqs);
        Plic {
            base:      virt_base,
            num_irqs:  info.num_irqs,
            contexts:  info.contexts,
            enable_locks: Box::leak(locks),
        }
    });
}

/// AP / BSP 公用 hart‑local初始化
pub unsafe fn per_hart_init(hart_id: usize) {
    let plic = PLIC.get().expect("PLIC not initialized");
    let ctx   = plic.context_for_hart(hart_id);

    /* 1. 关闭所有 IRQ */
    let words = ((plic.num_irqs + 31) / 32) as usize;
    for w in 0..words {
        tracing::debug!("Disabling IRQ {} for hart {} plic base: {:#x} ctx: {}", w, hart_id, plic.base, ctx);
        write32(enable_word_addr(plic.base, ctx, w as u32), 0);
    }

    /* 2. 设置阈值=0 => 接收所有优先级中断 */
    write32(threshold_addr(plic.base, ctx), 0);
}

/// 返回静态引用供 trap handler 调用
#[inline]
pub fn handle() -> &'static Plic {
    PLIC.get().expect("PLIC not initialized")
}

/// PLIC 结构体
pub struct Plic {
    base: usize,
    num_irqs: u32,
    contexts: &'static [u32],
    enable_locks: &'static [Mutex<()>],
}

impl Plic {
    #[inline(always)]
    /// 获取 hart 的 context
    pub fn context_for_hart(&self, hart_id: usize) -> u32 {
        self.contexts.get(hart_id).copied().unwrap_or( (hart_id as u32)*2 + 1 )
        /* 若 DTB 无映射，回退到 QEMU virt 规则：M=0,S=1 偏移 2×hart */ }

    /// 设置优先级 (1‑7)，0=禁用
    pub fn set_priority(&self, irq: u32, prio: u8) {
        debug_assert!(irq > 0 && irq <= self.num_irqs);
        unsafe { write32(priority_addr(self.base, irq), prio as u32) };
    }

    /// 启用 IRQ
    pub fn enable(&self, hart_id: usize, irq: u32) {
        let ctx = self.context_for_hart(hart_id);
        let word = (irq / 32) as usize;
        let bit  = irq % 32;
        let _g   = self.enable_locks[word].lock();
        unsafe {
            let reg = enable_word_addr(self.base, ctx, word as u32);
            let cur = read32(reg);
            write32(reg, cur | (1 << bit));
            fence(Ordering::SeqCst);      // 必要 I/O 屏障
        }
    }

    /// 禁用 IRQ
    pub fn disable(&self, hart_id: usize, irq: u32) {
        let ctx = self.context_for_hart(hart_id);
        let word = (irq / 32) as usize;
        let bit  = irq % 32;
        let _g   = self.enable_locks[word].lock();
        unsafe {
            let reg = enable_word_addr(self.base, ctx, word as u32);
            let cur = read32(reg);
            write32(reg, cur & !(1 << bit));
            fence(Ordering::SeqCst);
        }
    }

    /// 快速路径 Claim
    #[inline(always)]
    pub fn claim(&self, hart_id: usize) -> u32 {
        let ctx = self.context_for_hart(hart_id);
        unsafe { read32(claim_addr(self.base, ctx)) }
    }

    /// 完成 IRQ
    #[inline(always)]
    pub fn complete(&self, hart_id: usize, irq: u32) {
        if irq != 0 {
            let ctx = self.context_for_hart(hart_id);
            unsafe { write32(claim_addr(self.base, ctx), irq) }
        }
    }
}
