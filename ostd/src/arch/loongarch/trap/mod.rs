// SPDX-License-Identifier: MPL-2.0

//! Handles trap.

mod trap;

use align_ext::AlignExt;
use bitflags::bitflags;
use loongArch64::register::{
    badv,
    estat::{self, Exception, Interrupt, Trap},
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
    /// LoongArch 缺页异常错误码标志
    pub struct PageFaultErrorCode: usize {
        /// 读取访问导致的缺页 (Load Page Fault)
        const READ          = 1 << 0;
        /// 写入访问导致的缺页 (Store Page Fault)  
        const WRITE         = 1 << 1;
        /// 指令执行导致的缺页 (Fetch Page Fault)
        const INSTRUCTION   = 1 << 2;
        /// 页面修改错误 (Page Modify Fault)
        const MODIFY        = 1 << 3;
    }
}

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
    match estat::read().cause() {
        Trap::Interrupt(interrupt) => {
            IS_KERNEL_INTERRUPTED.store(true);
            handle_interrupt(interrupt, f);
            IS_KERNEL_INTERRUPTED.store(false);
        }
        Trap::Exception(e) => {
            // 在LoongArch中，badv寄存器包含导致异常的地址
            let badv_value = badv::read().vaddr();
            
            match e {
                // 页错误异常处理
                Exception::LoadPageFault | 
                Exception::StorePageFault | 
                Exception::FetchPageFault |
                Exception::PageModifyFault |
                Exception::PageNonReadableFault |
                Exception::PageNonExecutableFault |
                Exception::PagePrivilegeIllegal => {
                    // badv寄存器包含导致异常的虚拟地址
                    let page_fault_addr = badv_value;
                    
                    // 根据地址范围区分用户/内核页错误
                    if (0..MAX_USERSPACE_VADDR).contains(&page_fault_addr) {
                        handle_user_page_fault(f, e, page_fault_addr);
                    } else {
                        handle_kernel_page_fault(f, e, page_fault_addr);
                    }
                }
                
                // 地址错误异常处理
                Exception::AddressError => {
                    log::error!(
                        "Address Error Exception: address: {:#x}, era: {:#x}",
                        badv_value, f.era
                    );
                    panic!("Unhandled address error exception");
                }
                
                // 系统调用处理（不应在内核态出现）
                Exception::Syscall => {
                    log::error!(
                        "Unexpected syscall in kernel mode, era: {:#x}",
                        f.era
                    );
                    panic!("Unexpected syscall in kernel mode");
                }
                
                // 断点处理
                Exception::Breakpoint => {
                    log::trace!(
                        "Breakpoint Exception at era: {:#x}",
                        f.era
                    );
                    // 断点指令在LoongArch中是4字节
                    f.era += 4;
                }
                
                // 其他异常
                _ => {
                    log::error!(
                        "Unhandled kernel exception: {:?}, era: {:#x}, badv: {:#x}",
                        e, f.era, badv_value
                    );
                    panic!("Cannot handle kernel cpu exception: {e:?}. Trapframe: \n{f:#x?}.");
                }
            }
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

/// FIXME: this is a hack because we don't allocate kernel space for IO memory. We are currently
/// using the linear mapping for IO memory. This is not a good practice.
fn handle_kernel_page_fault(_f: &TrapFrame, e: Exception, page_fault_vaddr: usize) {
    let error_code = match e {
        Exception::FetchPageFault | Exception::PageNonExecutableFault => {
            PageFaultErrorCode::INSTRUCTION
        }
        Exception::LoadPageFault | Exception::PageNonReadableFault => PageFaultErrorCode::READ,
        Exception::StorePageFault | Exception::PageModifyFault => PageFaultErrorCode::WRITE,
        Exception::PagePrivilegeIllegal => PageFaultErrorCode::empty(), // 权限错误
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

    // 在LoongArch中，创建新的映射后，需要刷新TLB
    crate::arch::loongarch::mm::tlb_flush_addr(vaddr);
}

/// Handles page fault from user space.
fn handle_user_page_fault(f: &mut TrapFrame, e: Exception, page_fault_addr: usize) {
    let current_task = Task::current().unwrap();
    let user_space = current_task
        .user_space()
        .expect("the user space is missing when a page fault from the user happens.");

    let error_code = match e {
        Exception::FetchPageFault | Exception::PageNonExecutableFault => {
            PageFaultErrorCode::INSTRUCTION
        }
        Exception::LoadPageFault | Exception::PageNonReadableFault => PageFaultErrorCode::READ,
        Exception::StorePageFault | Exception::PageModifyFault => PageFaultErrorCode::WRITE,
        Exception::PagePrivilegeIllegal => PageFaultErrorCode::empty(),
        _ => panic!("not a page fault exception"),
    };

    // 注意这里是处理内核访问用户空间时的页错误
    // 尽管处于内核态，但如果是在用户空间范围内的地址引起的页错误，
    // 我们仍需要正确处理它

    let info = CpuExceptionInfo {
        page_fault_addr,
        code: e.into(),
        error_code: error_code.bits(),
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

    // 未来可考虑实现异常表恢复机制
    panic!(
        "Cannot handle user space page fault (in kernel mode); Trapframe:{:#x?}.",
        f
    );
}
