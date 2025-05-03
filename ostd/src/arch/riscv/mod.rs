// SPDX-License-Identifier: MPL-2.0

//! Platform-specific code for the RISC-V platform.

pub mod boot;
pub(crate) mod cpu;
pub mod device;
pub mod iommu;
pub mod irq;
pub(crate) mod mm;
pub(crate) mod pci;
pub mod qemu;
pub mod serial;
pub mod timer;
pub mod trap;

use core::sync::atomic::Ordering;

pub use cpu::wait_for_interrupt;

#[cfg(feature = "cvm_guest")]
pub(crate) fn init_cvm_guest() {
    // Unimplemented, no-op
}

pub(crate) fn init_on_bsp() {
    // 获取在早期启动阶段确定的主处理器 hart ID
    let bsp_hart_id = boot::bsp_hart_id();

    // Safety：该函数仅在BSP上调用一次
    unsafe {
        trap::init(true);

        crate::cpu::init_num_cpus();
        
        // Safety：在此之前没有访问过CPU本地对象，且当前在BSP上
        crate::cpu::local::init_on_bsp();

        crate::cpu::set_this_cpu_id(bsp_hart_id as u32);
    }

    irq::init();
    crate::sync::init();

    // 在timer::init之前，irq::init之后 初始化SMP子系统
    // 因为timer::init可能通过TLB刷新触发IPI
    // 且需要先初始化IRQ_ALLOCATOR
    crate::smp::init();

    crate::boot::smp::boot_all_aps(bsp_hart_id as u32);

    timer::init();
}

/// 在 AP 上初始化架构特定的功能。
///
/// # Safety
///
/// 本函数将执行以下操作：
/// - 将`sscratch`寄存器设置为0
/// - 将`stvec`寄存器设置为内部异常向量地址
///
/// 后续**严禁**修改这些寄存器的值。
///
pub(crate) unsafe fn init_on_ap(hart_id: u32) {
    enable_cpu_features();
    // Safety：调用者已经保证不会修改`sscratch`和`stvec`
    unsafe {
        trap::init(false);
    }

    irq::enable_all_local();

    log::trace!("Hart {} finished arch-specific AP initialization.", hart_id);
}

pub(crate) fn interrupts_ack(_irq_number: usize) {
    // 1. Supervisor Software Interrupt Pending 已经在 trap_handler 中清除了
    // 2. CPU_IPI_QUEUES 相应的指令已经在处理时 pop 了
}

/// 返回时间戳计数器(TSC)的频率，单位是Hz
pub fn tsc_freq() -> u64 {
    timer::TIMEBASE_FREQ.load(Ordering::Relaxed)
}

/// 读取处理器时间戳计数器(TSC)的当前值
pub fn read_tsc() -> u64 {
    riscv::register::time::read64()
}

pub(crate) fn enable_cpu_features() {
    unsafe {
        riscv::register::sstatus::set_fs(riscv::register::sstatus::FS::Clean);
    }
}
