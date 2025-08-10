// SPDX-License-Identifier: MPL-2.0

//! Handles trap.

mod trap;

use loongArch64::register::estat::Interrupt;
pub use trap::{GeneralRegs, TrapFrame, UserContext};

use crate::cpu_local_cell;

cpu_local_cell! {
    static IS_KERNEL_INTERRUPTED: bool = false;
}

/// Initialize interrupt handling on LoongArch.
pub unsafe fn init(_on_bsp: bool) {
    self::trap::init();
}

/// Returns true if this function is called within the context of an IRQ handler
/// and the IRQ occurs while the CPU is executing in the kernel mode.
/// Otherwise, it returns false.
pub fn is_kernel_interrupted() -> bool {
    IS_KERNEL_INTERRUPTED.load()
}

/// Handle traps (only from kernel).
#[no_mangle]
extern "C" fn trap_handler(f: &mut TrapFrame) {
    use loongArch64::register::estat::{self, Trap};

    match estat::read().cause() {
        Trap::Interrupt(interrupt) => {
            IS_KERNEL_INTERRUPTED.store(true);
            handle_interrupt(interrupt, f);
            IS_KERNEL_INTERRUPTED.store(false);
        }
        Trap::Exception(e) => {
            panic!("Cannot handle kernel cpu exception: {e:?}. Trapframe: \n{f:#x?}.",);
        }
        _ => todo!(),
    }
}

/// 处理 LoongArch 硬件中断。
///
/// - IPI: 关闭本地中断，从 per-CPU IPI 队列取出并分发回调。
/// - Timer: 委托给通用时间中断处理。
/// - 外部中断: 通过 PLIC/EIOINTC 读取并逐个分发，随后 complete。
pub(crate) fn handle_interrupt(interrupt: Interrupt, f: &mut TrapFrame) {
    match interrupt {
        // 核间中断：消费本地 IPI 队列并触发已注册的回调
        Interrupt::IPI => {
            let guard: crate::trap::DisabledLocalIrqGuard = crate::trap::disable_local();
            log::trace!("IPI Interrupt");

            let cpu_local_deref_guard = crate::arch::irq::CPU_IPI_QUEUES.get_with(&guard);
            let ipi_queue = cpu_local_deref_guard
                .get()
                .expect("CPU_IPI_QUEUES is not initialized");

            while let Some(irq_num) = ipi_queue.pop() {
                log::trace!("IPI: dispatch software IRQ {}", irq_num);
                crate::trap::call_irq_callback_functions(f, irq_num as usize);
            }
            log::trace!("IPI Interrupt end");
            // 注：如需显式清除 IPI pending，可在此处写 IOCSR 的 IPI_CLEAR；
            // 由于当前 EIOINTC/PLATIC 路径不依赖该位，暂不做显式清理。
        }

        // 本地定时器中断
        Interrupt::Timer => {
            crate::arch::timer::time_interrupt_handler();
        }

        // 其他硬件外部中断统一走 PLIC/EIOINTC 路径
        _ => {
            use crate::arch::loongarch::plic;
            log::trace!("External Interrupt");

            // LoongArch 的 PLIC/EIOINTC 实现当前忽略 hart_id 参数
            let hart_id = 0usize;

            loop {
                let irq_id = plic::handle().claim(hart_id) as usize;
                if irq_id == 0 {
                    break;
                }
                crate::trap::call_irq_callback_functions(f, irq_id);
                plic::handle().complete(hart_id, irq_id as u32);
            }
        }
    }
}
