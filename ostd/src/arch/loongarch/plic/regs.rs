use core::arch::asm;

/// 硬件支持的最大中断数
pub const NUM_IRQS: usize = 256;

#[inline(always)]
unsafe fn iocsr_read_d(off: usize) -> u64 {
    let val: u64;
    asm!("iocsrrd.d {0}, {1}", out(reg) val, in(reg) off);
    val
}

#[inline(always)]
unsafe fn iocsr_write_d(off: usize, val: u64) {
    asm!("iocsrwr.d {0}, {1}", in(reg) val, in(reg) off);
}

#[inline(always)]
unsafe fn iocsr_write_h(off: usize, val: u16) {
    asm!("iocsrwr.h {0}, {1}", in(reg) val, in(reg) off);
}

#[inline(always)]
unsafe fn iocsr_write_b(off: usize, val: u8) {
    asm!("iocsrwr.b {0}, {1}", in(reg) val, in(reg) off);
}

pub(super) mod eiointc {
    use super::*;

    // 寄存器偏移
    const EXT_IOI_EN_BASE:     usize = 0x1600;
    const EXT_IOI_BOUNCE_BASE: usize = 0x1680;
    const EXT_IOI_SR_BASE:     usize = 0x1700;
    const PERCORE_EXT_IOI_SR_BASE: usize = 0x1800;
    const EXT_IOI_MAP_BASE:    usize = 0x14c0;
    const EXT_IOI_MAP_CORE_BASE: usize = 0x1c00;
    const EXT_IOI_NODE_TYPE_BASE: usize = 0x14a0;

    /// 一次性初始化（仅 BSP 调用）
    pub unsafe fn init(core_cnt: usize) {
        assert!(core_cnt <= 4, "LoongArch EIOINTC 仅支持 ≤4 核");

        // 打开 EIOINTC 主开关
        let mut v = iocsr_read_d(0x420);
        v |= 1 << 49;
        iocsr_write_d(0x420, v);

        // IRQ → INTx 路由：N → N/32
        for pin in 0..8 {
            iocsr_write_b(EXT_IOI_MAP_BASE + pin, pin as u8);
        }

        // 轮转到前 core_cnt 个核
        let mask = ((1 << core_cnt) - 1) as u8;
        for irq in 0..256 {
            iocsr_write_b(EXT_IOI_MAP_CORE_BASE + irq, mask);
        }

        // 映射方式保持 node0
        iocsr_write_h(EXT_IOI_NODE_TYPE_BASE, 0x01);
    }

    /// 使能单个 IRQ
    pub unsafe fn enable_irq(no: usize) {
        let grp = no >> 6;
        let bit = no & 63;
        let off = grp << 3;

        // EN
        let mut en = iocsr_read_d(EXT_IOI_EN_BASE + off);
        en |= 1u64 << bit;
        iocsr_write_d(EXT_IOI_EN_BASE + off, en);

        // Round-Robin
        let mut rr = iocsr_read_d(EXT_IOI_BOUNCE_BASE + off);
        rr |= 1u64 << bit;
        iocsr_write_d(EXT_IOI_BOUNCE_BASE + off, rr);
    }

    /// 禁用 IRQ
    pub unsafe fn disable_irq(no: usize) {
        let grp = no >> 6;
        let bit = no & 63;
        let off = grp << 3;

        let mut en = iocsr_read_d(EXT_IOI_EN_BASE + off);
        en &= !(1u64 << bit);
        iocsr_write_d(EXT_IOI_EN_BASE + off, en);
    }

    /// 当前 hart 上待处理最高优先级 IRQ（若无返回 None）
    pub fn claim_irq() -> Option<usize> {
        for grp in (0..4).rev() {
            let flags = unsafe { iocsr_read_d(PERCORE_EXT_IOI_SR_BASE + (grp << 3)) };
            if flags != 0 {
                return Some(grp * 64 + 63 - flags.leading_zeros() as usize);
            }
        }
        None
    }

    /// 完成 IRQ
    pub unsafe fn complete_irq(no: usize) {
        let grp = no >> 6;
        let bit = no & 63;
        let off = grp << 3;

        let mut sr = iocsr_read_d(EXT_IOI_SR_BASE + off);
        sr &= !(1u64 << bit);
        iocsr_write_d(EXT_IOI_SR_BASE + off, sr);
    }
}

pub(super) mod platic {
    // PLATIC 寄存器偏移
    pub const INT_MASK:      usize = 0x020;
    pub const HTMSI_VECTOR0: usize = 0x200;
    pub const HTMSI_VECTOR32:usize = 0x220;
}
