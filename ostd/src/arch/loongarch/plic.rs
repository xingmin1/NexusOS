//! EIOINTC + PLATIC 组合封装

mod regs;
mod fdt;

use alloc::{boxed::Box, vec::Vec};
use core::sync::atomic::{fence, Ordering};
use spin::{Mutex, Once};

use regs::{eiointc, platic, NUM_IRQS};
use ::fdt::Fdt;

/// 全局 Once 单例（所有 hart 共享）
static PLIC: Once<Plic> = Once::new();

/// BSP 调用：全局初始化
///
/// - 解析 FDT，完成地址映射  
/// - 调用 EIOINTC 完成硬件初始化  
/// - 为每 64 个 IRQ 预分配一把自旋锁，防止 SMP 并发读改写  
pub unsafe fn init_global(dt: &Fdt) {
    use crate::mm::paddr_to_vaddr;

    let info = fdt::parse(dt);

    let virt_base = paddr_to_vaddr(info.platic_base);
    let groups = ((NUM_IRQS + 63) / 64) as usize;

    let locks = (0..groups)
        .map(|_| Mutex::new(()))
        .collect::<Vec<_>>()
        .into_boxed_slice();

    eiointc::init(info.core_cnt);

    PLIC.call_once(|| Plic {
        platic_vbase: virt_base,
        enable_locks: Box::leak(locks),
    });

    crate::early_println!(
        "[PLIC] LoongArch: cores={}, platic={:#x}, groups={}",
        info.core_cnt,
        info.platic_base,
        groups
    );
}

/// 供陷入处理器调用
#[inline]
pub(crate) fn handle() -> &'static Plic {
    PLIC.get().expect("PLIC not initialised")
}

pub(crate) struct Plic {
    /// PLATIC MMIO 虚拟基地址（BSP 映射后全局共享）
    platic_vbase: usize,
    /// 每 64 个中断源一把锁，避免并发修改 IOCSR
    enable_locks: &'static [Mutex<()>],
}

impl Plic {
    /// LoongArch EIOINTC 固定优先级，不可配置；接口保留以兼容。
    #[inline(always)]
    pub fn set_priority(&self, _irq: u32, _prio: u8) {}

    /// 使能 IRQ
    pub fn enable(&self, _hart_id: usize, irq: u32) {
        let idx = irq as usize;
        if idx >= NUM_IRQS {
            return;
        }
        let _g = self.enable_locks[idx / 64].lock();

        // 解除 PLATIC 屏蔽（必要时）
        self.unmask_platic(idx);

        // 设置 EIOINTC 使能位
        unsafe { eiointc::enable_irq(idx) };

        fence(Ordering::SeqCst);
    }

    /// 禁用 IRQ
    pub fn disable(&self, _hart_id: usize, irq: u32) {
        let idx = irq as usize;
        if idx >= NUM_IRQS {
            return;
        }
        let _g = self.enable_locks[idx / 64].lock();

        unsafe { eiointc::disable_irq(idx) };
        self.mask_platic(idx);

        fence(Ordering::SeqCst);
    }

    /// 快速路径 claim（无锁）
    #[inline(always)]
    pub fn claim(&self, _hart_id: usize) -> u32 {
        eiointc::claim_irq().unwrap_or(0) as u32
    }

    /// 完成 IRQ
    #[inline(always)]
    pub fn complete(&self, _hart_id: usize, irq: u32) {
        if irq != 0 {
            unsafe { eiointc::complete_irq(irq as usize) };
        }
    }

    /// 如果中断号落在 PLATIC 可见范围内（0‑63），对其进行 UNMASK。
    fn unmask_platic(&self, idx: usize) {
        use platic::*;

        if idx < 32 {
            let mut mask = self.read_w(INT_MASK);
            mask &= !(1 << idx);
            self.write_w(INT_MASK, mask);
            self.write_b(HTMSI_VECTOR0 + idx, idx as u8);
        } else if idx < 64 {
            let mut mask = self.read_w(INT_MASK);
            mask &= !(1 << (idx - 32));
            self.write_w(INT_MASK, mask);
            self.write_b(HTMSI_VECTOR32 + idx, idx as u8);
        }
    }

    /// MASK 掉 PLATIC 中断（0‑63）
    fn mask_platic(&self, idx: usize) {
        use platic::*;

        if idx < 32 {
            let mut mask = self.read_w(INT_MASK);
            mask |= 1 << idx;
            self.write_w(INT_MASK, mask);
        } else if idx < 64 {
            let mut mask = self.read_w(INT_MASK);
            mask |= 1 << (idx - 32);
            self.write_w(INT_MASK, mask);
        }
    }

    /// MMIO 原语 —— 8/32‑bit 读写
    #[inline(always)]
    fn write_b(&self, off: usize, val: u8) {
        unsafe { ((self.platic_vbase + off) as *mut u8).write_volatile(val) }
    }
    #[inline(always)]
    fn write_w(&self, off: usize, val: u32) {
        unsafe { ((self.platic_vbase + off) as *mut u32).write_volatile(val) }
    }
    #[inline(always)]
    fn read_w(&self, off: usize) -> u32 {
        unsafe { ((self.platic_vbase + off) as *const u32).read_volatile() }
    }
}
