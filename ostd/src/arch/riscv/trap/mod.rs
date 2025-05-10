// SPDX-License-Identifier: MPL-2.0

//! Handles trap.

mod trap;

use align_ext::AlignExt;
use bitflags::bitflags;
use riscv::{
    interrupt::{Exception, Interrupt},
    register::{scause::Trap, stval},
};
pub use trap::{GeneralRegs, TrapFrame, UserContext};

use super::cpu::CpuExceptionInfo;
use crate::{
    cpu_local_cell,
    mm::{
        kspace::{KERNEL_PAGE_TABLE, LINEAR_MAPPING_BASE_VADDR, LINEAR_MAPPING_VADDR_RANGE},
        CachePolicy, PageFlags, PageProperty, PrivilegedPageFlags as PrivFlags,
        MAX_USERSPACE_VADDR, PAGE_SIZE,
    },
    task::Task,
    trap::disable_local,
};

bitflags! {
    /// RISC-V 缺页异常错误码标志
    pub struct PageFaultErrorCode: usize {
        /// 读取访问导致的缺页 (Load Page Fault)
        const READ          = 1 << 0;
        /// 写入访问导致的缺页 (Store/AMO Page Fault)
        const WRITE         = 1 << 1;
        /// 指令执行导致的缺页 (Instruction Page Fault)
        const INSTRUCTION   = 1 << 2;
    }
}

cpu_local_cell! {
    static IS_KERNEL_INTERRUPTED: bool = false;
}

/// Initialize interrupt handling on RISC-V,
/// and enables Supervisor interrupts (timer, software, external).
///
/// # Safety
///
/// This function will:
/// - Set `sscratch` to 0.
/// - Set `stvec` to internal exception vector.
///
/// You **MUST NOT** modify these registers later.
pub unsafe fn init(_on_bsp: bool) {
    // Safety：调用者已经保证不会修改`sscratch`和`stvec`
    self::trap::init();
    crate::arch::irq::enable_all_local();
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
    match riscv::interrupt::cause::<Interrupt, Exception>() {
        Trap::Interrupt(interrupt) => {
            IS_KERNEL_INTERRUPTED.store(true);
            handle_interrupt(interrupt, f);
            IS_KERNEL_INTERRUPTED.store(false);
        }
        Trap::Exception(e) => {
            // 在RISC-V中，stval寄存器包含依赖于异常类型的附加信息
            let stval = stval::read();
            
            match e {
                // 页错误异常处理
                Exception::InstructionPageFault | 
                Exception::LoadPageFault | 
                Exception::StorePageFault => {
                    // 在RISC-V中，stval寄存器包含导致异常的虚拟地址
                    let page_fault_addr = stval;
                    
                    // 虽然这是内核trap处理函数，但我们仍然需要根据地址范围区分用户/内核页错误
                    // 这是因为内核代码也可能会访问用户空间的内存
                    if (0..MAX_USERSPACE_VADDR).contains(&(page_fault_addr as usize)) {
                        handle_user_page_fault(f, e, page_fault_addr as usize);
                    } else {
                        handle_kernel_page_fault(f, e, page_fault_addr as usize);
                    }
                }
                
                // 地址不对齐异常处理
                Exception::InstructionMisaligned |
                Exception::LoadMisaligned |
                Exception::StoreMisaligned => {
                    // stval寄存器包含导致异常的不对齐地址
                    log::error!(
                        "Address Misaligned Exception: {:?}, address: {:#x}, sepc: {:#x}",
                        e, stval, f.sepc
                    );
                    panic!("Unhandled address misaligned exception");
                }
                
                // 其他访问故障
                Exception::InstructionFault |
                Exception::LoadFault |
                Exception::StoreFault => {
                    log::error!(
                        "Access Fault Exception: {:?}, address: {:#x}, sepc: {:#x}",
                        e, stval, f.sepc
                    );
                    panic!("Unhandled access fault exception");
                }
                
                // 环境调用（系统调用）处理
                Exception::UserEnvCall => {
                    // 这不应该在内核态出现，因为这是用户态发起的系统调用
                    log::error!(
                        "Unexpected user environment call in kernel mode, sepc: {:#x}",
                        f.sepc
                    );
                    panic!("Unexpected user environment call in kernel mode");
                }
                
                Exception::SupervisorEnvCall => {
                    // 内核态的环境调用
                    log::trace!(
                        "Supervisor Environment Call Exception: sepc: {:#x}",
                        f.sepc
                    );
                    // 增加sepc来跳过ecall指令（4字节长度，此处未考虑压缩指令集（2字节长度））
                    f.sepc += 4;
                    // TODO: 处理环境调用的具体逻辑
                }
                
                // 非法指令处理
                Exception::IllegalInstruction => {
                    // stval寄存器包含导致异常的非法指令
                    log::error!(
                        "Illegal Instruction Exception, instruction: {:#x}, sepc: {:#x}",
                        stval, f.sepc
                    );
                    panic!("Illegal instruction at {:#x}", f.sepc);
                }
                
                // 断点处理
                Exception::Breakpoint => {
                    // 断点通常由ebreak指令触发
                    log::trace!(
                        "Breakpoint Exception at sepc: {:#x}",
                        f.sepc
                    );
                    f.sepc += 4;
                }
            }
        }
    }
}

pub(crate) fn handle_interrupt(interrupt: Interrupt, f: &mut TrapFrame) {
    match interrupt {
        Interrupt::SupervisorSoft => {
            let guard: crate::trap::DisabledLocalIrqGuard = disable_local();
            log::trace!("Supervisor Software Interrupt");
            unsafe {
                riscv::register::sip::clear_ssoft();
            }

            let cpu_local_deref_guard = crate::arch::irq::CPU_IPI_QUEUES.get_with(&guard);
            let cpi_ipi_queue = cpu_local_deref_guard
                .get()
                .expect("CPU_IPI_QUEUES is not initialized");

            while let Some(irq_num) = cpi_ipi_queue.pop() {
                log::trace!("Supervisor Software Interrupt: {}", irq_num);
                crate::trap::call_irq_callback_functions(f, irq_num as usize);
            }
            log::trace!("Supervisor Software Interrupt end");
        }
        Interrupt::SupervisorTimer => {
            crate::arch::timer::time_interrupt_handler();
        }
        Interrupt::SupervisorExternal => {
            log::trace!("Supervisor External Interrupt");
        }
    }
}

/// FIXME: this is a hack because we don't allocate kernel space for IO memory. We are currently
/// using the linear mapping for IO memory. This is not a good practice.
fn handle_kernel_page_fault(_f: &TrapFrame, e: Exception, page_fault_vaddr: usize) {
    let error_code = match e {
        Exception::InstructionPageFault => PageFaultErrorCode::INSTRUCTION,
        Exception::LoadPageFault => PageFaultErrorCode::READ,
        Exception::StorePageFault => PageFaultErrorCode::WRITE,
        _ => panic!("not a page fault exception"),
    };

    log::debug!(
        "kernel page fault: address {:?}, error code {:?}, exception: {:?}",
        page_fault_vaddr as *const (),
        error_code,
        e
    );

    assert!(
        LINEAR_MAPPING_VADDR_RANGE.contains(&page_fault_vaddr),
        "kernel page fault: the address is outside the range of the linear mapping",
    );

    assert!(
        !error_code.contains(PageFaultErrorCode::INSTRUCTION),
        "kernel page fault: the direct mapping cannot be executed",
    );

    // 进行映射
    let page_table = KERNEL_PAGE_TABLE
        .get()
        .expect("kernel page fault: the kernel page table is not initialized");
    let vaddr = page_fault_vaddr.align_down(PAGE_SIZE);
    let paddr = vaddr - LINEAR_MAPPING_BASE_VADDR;

    // SAFETY:
    // 1. 我们已经检查页错误地址位于物理内存直接映射的地址范围内。
    // 2. 我们将该地址映射到具有正确标志的物理页面，其正确性遵循物理内存直接映射的语义。
    unsafe {
        page_table
            .map(
                &(vaddr..vaddr + PAGE_SIZE),
                &(paddr..paddr + PAGE_SIZE),
                PageProperty {
                    flags: PageFlags::RW, // 设置可读可写
                    cache: CachePolicy::Uncacheable,
                    priv_flags: PrivFlags::GLOBAL,
                },
            )
            .unwrap();
    }

    // 在RISC-V中，创建新的映射后，需要刷新TLB
    // RISC-V规范要求显式刷新TLB
    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("sfence.vma {}, zero", in(reg) vaddr);
    }
}

/// Handles page fault from user space.
fn handle_user_page_fault(f: &mut TrapFrame, e: Exception, page_fault_addr: usize) {
    let current_task = Task::current().unwrap();
    let user_space = current_task
        .user_space()
        .expect("the user space is missing when a page fault from the user happens.");

    let error_code: PageFaultErrorCode = match e {
        Exception::InstructionPageFault => PageFaultErrorCode::INSTRUCTION,
        Exception::LoadPageFault => PageFaultErrorCode::READ,
        Exception::StorePageFault => PageFaultErrorCode::WRITE,
        _ => panic!("not a page fault exception"),
    };

    // 注意这里是处理内核访问用户空间时的页错误
    // 尽管处于内核态，但如果是在用户空间范围内的地址引起的页错误，
    // 我们仍需要正确处理它

    let info = CpuExceptionInfo {
        stval: page_fault_addr,
        code: e,
    };

    log::debug!(
        "User space page fault (in kernel mode): address {:?}, error code {:?}, exception: {:?}",
        page_fault_addr as *const (),
        error_code,
        e
    );

    let res = user_space.vm_space().handle_page_fault(&info);
    if res.is_ok() {
        return;
    }

    // 在x86的实现中有异常表恢复机制，但在RISC-V中我们暂不实现
    // 未来可考虑类似机制
    panic!(
        "Cannot handle user space page fault (in kernel mode); Trapframe:{:#x?}.",
        f
    );
}
