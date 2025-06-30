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
        Trap::Interrupt(_) => {
            IS_KERNEL_INTERRUPTED.store(true);
            let estat = estat::read();
            match estat.cause() {
                Trap::Interrupt(Interrupt::Timer) => {
                    crate::arch::timer::time_interrupt_handler();
                }
                _ => todo!(),
            }
            IS_KERNEL_INTERRUPTED.store(false);
        }
        Trap::Exception(e) => {
            panic!("Cannot handle kernel cpu exception: {e:?}. Trapframe: \n{f:#x?}.",);
        }
        _ => todo!(),
    }
}
