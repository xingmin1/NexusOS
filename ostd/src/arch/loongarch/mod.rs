// SPDX-License-Identifier: MPL-2.0

//! Platform-specific code for the LoongArch platform.

use core::sync::atomic::Ordering;

pub mod boot;
pub mod cpu;
pub mod device;
pub mod iommu;
pub(crate) mod irq;
pub(crate) mod mm;
pub(crate) mod pci;
pub mod qemu;
pub mod serial;
// pub mod task;
pub mod timer;
pub mod trap;
pub mod plic;

pub(crate) use cpu::wait_for_interrupt;

#[cfg(feature = "cvm_guest")]
pub(crate) fn init_cvm_guest() {
    // Unimplemented, no-op
}

/// Return the frequency of TSC. The unit is Hz.
pub fn tsc_freq() -> u64 {
    timer::TIMEBASE_FREQ.load(Ordering::Relaxed)
}

/// Reads the current value of the processor’s time-stamp counter (TSC).
pub fn read_tsc() -> u64 {
    let mut pmc: usize;
    unsafe {
        core::arch::asm!(
            "rdtime.d {}, $zero",
            out(reg) pmc,
        );
    }
    pmc as u64
}

pub(crate) fn enable_cpu_features() {
    // enable float point
    loongArch64::register::euen::set_fpe(true);
}

pub(crate) unsafe fn init_on_bsp() {
    let bsp_hart_id = boot::bsp_hart_id();

    // SAFETY: this function is only called once on BSP.
    unsafe {
        trap::init(true);
        
        crate::cpu::init_num_cpus();
        
        // Safety：在此之前没有访问过CPU本地对象，且当前在BSP上
        crate::cpu::local::init_on_bsp();

        crate::cpu::set_this_cpu_id(bsp_hart_id as u32);
    }
    irq::init();

    unsafe {
        // 初始化全局 PLIC
        crate::arch::loongarch::plic::init_global(boot::DEVICE_TREE.get().unwrap());
    }

    crate::sync::init();

    // 在timer::init之前，irq::init之后 初始化SMP子系统
    // 因为timer::init可能通过TLB刷新触发IPI
    // 且需要先初始化IRQ_ALLOCATOR
    crate::smp::init();

    crate::boot::smp::boot_all_aps(bsp_hart_id as u32);

    timer::init();
    // let _ = crate::bus::pci::init();
}

pub(crate) unsafe fn init_on_ap(ap_hart_id: u32) {
    unimplemented!()
}

pub(crate) fn interrupts_ack(_irq_number: usize) {
    unimplemented!()
}

// No TDX for LoongArch
#[macro_export]
macro_rules! if_tdx_enabled {
    ($if_block:block else $else_block:block) => {{
        $else_block
    }};
    ($if_block:block) => {};
}
