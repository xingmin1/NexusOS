// SPDX-License-Identifier: MPL-2.0

//! Interrupts.
use alloc::{boxed::Box, fmt::Debug, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicPtr, Ordering};

use crossbeam_queue::ArrayQueue;
use id_alloc::IdAlloc;
use spin::Once;

use crate::{
    cpu::CpuId, cpu_local, sync::{blocking::Mutex, spin::Spinlock, GuardSpinLock, PreemptDisabled, SpinLockGuard}, trap::TrapFrame
};

/// The global allocator for software defined IRQ lines.
pub(crate) static IRQ_ALLOCATOR: Once<GuardSpinLock<IdAlloc>> = Once::new();

/// 中断号类型
pub type IrqNum = u16;

/// 软件中断号起始值
pub const SOFTWARE_IRQ_BASE: IrqNum = 1024;

/// 软件中断号容量
pub const SOFTWARE_IRQ_CAP: usize = 256;

/// 最大中断号
const MAX_IRQS: usize = SOFTWARE_IRQ_BASE as usize + SOFTWARE_IRQ_CAP;

#[link_section = ".bss.cpu_local"]
static IRQ_TABLE: [AtomicPtr<IrqLine>; MAX_IRQS] = {
    const Z: AtomicPtr<IrqLine> = AtomicPtr::new(core::ptr::null_mut());
    [Z; MAX_IRQS]
};

#[inline]
pub(crate) unsafe fn register_line(n: IrqNum, p: *const IrqLine) {
    IRQ_TABLE[n as usize].store(p as *mut IrqLine, Ordering::Release);
}

#[inline]
pub(crate) unsafe fn unregister_line(n: IrqNum) {
    IRQ_TABLE[n as usize].store(core::ptr::null_mut(), Ordering::Release);
}

#[inline]
pub(crate) fn is_slot_empty(n: IrqNum) -> bool {
    IRQ_TABLE[n as usize].load(Ordering::Acquire).is_null()
}

pub(crate) fn is_plic_source(id: IrqNum) -> bool {
    id != 0 && (id as usize) < SOFTWARE_IRQ_BASE as usize
}

static SOFT_ALLOC: Once<GuardSpinLock<IdAlloc>> = Once::new();

pub(crate) fn alloc_soft_irq() -> Option<IrqNum> {
    SOFT_ALLOC
        .get()
        .unwrap()
        .lock()
        .alloc()
        .map(|i| SOFTWARE_IRQ_BASE as u16 + i as u16)
}

pub(crate) static IRQ_LIST: Once<Vec<IrqLine>> = Once::new();

cpu_local! {
    pub(crate) static CPU_IPI_QUEUES: Once<ArrayQueue<IrqNum>> = Once::new();
}

pub(crate) fn init() {
    let mut list: Vec<IrqLine> = Vec::new();
    for i in 0..256 {
        list.push(IrqLine {
            irq_num: i as u16,
            callback_list: GuardSpinLock::new(Vec::new()),
        });
    }
    IRQ_LIST.call_once(|| list);
    CALLBACK_ID_ALLOCATOR
        .call_once(|| Mutex::new_with_raw_mutex(IdAlloc::with_capacity(256), Spinlock::new()));
    IRQ_ALLOCATOR.call_once(|| GuardSpinLock::new(IdAlloc::with_capacity(256)));
    SOFT_ALLOC.call_once(|| GuardSpinLock::new(IdAlloc::with_capacity(SOFTWARE_IRQ_CAP)));
    for cpu_id in crate::cpu::all_cpus() {
        CPU_IPI_QUEUES
            .get_on_cpu(cpu_id)
            .call_once(|| ArrayQueue::new(32));
    }
}

/// 启用 Supervisor 中断（SIE 位）和特定的 S 级中断（定时器、软件、外部）。
pub(crate) fn enable_all_local() {
    unsafe {
        riscv::interrupt::enable();
        riscv::register::sie::set_sext();
        riscv::register::sie::set_ssoft();
        riscv::register::sie::set_stimer();
        riscv::register::sstatus::set_sum();
    }
}

pub(crate) fn enable_local() {
    // if !crate::IN_BOOTSTRAP_CONTEXT.load(Ordering::Relaxed) {
    //     crate::prelude::println!("enable_local");
    // }
    unsafe {
        riscv::interrupt::enable();
    }
}

#[track_caller]
pub(crate) fn disable_local() {
    // if !crate::IN_BOOTSTRAP_CONTEXT.load(Ordering::Relaxed) {
    //     crate::prelude::println!("disable_local, caller: {}", core::panic::Location::caller());
    // }
    riscv::interrupt::disable();
}

pub(crate) fn is_local_enabled() -> bool {
    riscv::register::sstatus::read().sie()
}

static CALLBACK_ID_ALLOCATOR: Once<Mutex<IdAlloc, Spinlock>> = Once::new();

/// 中断回调函数封装结构
///
/// 用于封装中断发生时需要执行的回调函数及其唯一标识。
/// 包含实际的中断处理函数和用于标识该回调的唯一ID。
pub struct CallbackElement {
    /// 实际的中断处理函数闭包
    ///
    /// 该函数接受一个 TrapFrame 引用作为参数，用于处理中断时的上下文信息。
    /// 需要满足跨线程安全（Send + Sync）和静态生命周期要求。
    function: Box<dyn Fn(&TrapFrame) + Send + Sync + 'static>,

    /// 回调函数的唯一标识符
    ///
    /// 用于在注册/注销回调时进行唯一性标识和管理。
    id: usize,
}

impl CallbackElement {
    /// 执行注册的回调函数
    ///
    /// # 参数
    /// - `element`: 中断发生时保存的上下文信息，包含寄存器状态等关键信息。
    ///
    /// 该方法会调用存储在结构体中的实际中断处理函数，传递当前的中断上下文。
    pub fn call(&self, element: &TrapFrame) {
        self.function.call((element,));
    }
}

impl Debug for CallbackElement {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CallbackElement")
            .field("id", &self.id)
            .finish()
    }
}

/// An interrupt request (IRQ) line.
#[derive(Debug)]
pub(crate) struct IrqLine {
    pub(crate) irq_num: IrqNum,
    pub(crate) callback_list: GuardSpinLock<Vec<CallbackElement>>,
}

impl IrqLine {
    /// Acquire an interrupt request line.
    ///
    /// # Safety
    ///
    /// This function is marked unsafe as manipulating interrupt lines is
    /// considered a dangerous operation.
    #[expect(clippy::redundant_allocation)]
    pub unsafe fn acquire(irq_num: IrqNum) -> Arc<&'static Self> {
        let idx = irq_num as usize;
        let ptr = IRQ_TABLE[idx].load(Ordering::Acquire);

        let line_ref: &'static IrqLine = if ptr.is_null() {
            let new_line = Box::leak(Box::new(IrqLine {
                irq_num,
                callback_list: GuardSpinLock::new(Vec::new()),
            }));
            IRQ_TABLE[idx].store(new_line as *mut _, Ordering::Release);
            new_line
        } else {
            &*ptr
        };

        Arc::new(line_ref)
    }

    /// Get the IRQ number.
    #[allow(unused)]
    pub fn num(&self) -> u16 {
        self.irq_num
    }

    pub fn callback_list(
        &self,
    ) -> SpinLockGuard<alloc::vec::Vec<CallbackElement>, PreemptDisabled> {
        self.callback_list.lock()
    }

    /// Register a callback that will be invoked when the IRQ is active.
    ///
    /// A handle to the callback is returned. Dropping the handle
    /// automatically unregisters the callback.
    ///
    /// For each IRQ line, multiple callbacks may be registered.
    pub fn on_active<F>(&self, callback: F) -> IrqCallbackHandle
    where
        F: Fn(&TrapFrame) + Sync + Send + 'static,
    {
        let allocate_id = CALLBACK_ID_ALLOCATOR.get().unwrap().lock().alloc().unwrap();
        self.callback_list.lock().push(CallbackElement {
            function: Box::new(callback),
            id: allocate_id,
        });
        IrqCallbackHandle {
            irq_num: self.irq_num,
            id: allocate_id,
        }
    }
}

/// The handle to a registered callback for a IRQ line.
///
/// When the handle is dropped, the callback will be unregistered automatically.
#[must_use]
#[derive(Debug)]
pub struct IrqCallbackHandle {
    irq_num: IrqNum,
    id: usize,
}

impl Drop for IrqCallbackHandle {
    fn drop(&mut self) {
        unsafe {
            let ptr = IRQ_TABLE[self.irq_num as usize].load(Ordering::Acquire);
            if !ptr.is_null() {
                (*ptr)
                    .callback_list
                    .lock()
                    .retain(|item| item.id != self.id);
            }
        }
        CALLBACK_ID_ALLOCATOR.get().unwrap().lock().free(self.id);
    }
}

/// 发送 IPI 时可能发生的错误
#[derive(Debug, PartialEq, Eq)]
pub enum IpiSendError {
    /// 目标 CPU 的 IPI 命令队列已满
    QueueFull,
    /// 底层 SBI send_ipi 调用失败
    SbiError(sbi_spec::binary::Error),
}

impl From<sbi_spec::binary::Error> for IpiSendError {
    fn from(err: sbi_spec::binary::Error) -> Self {
        IpiSendError::SbiError(err)
    }
}

/// 发送处理器间中断到指定 CPU
///
/// # 安全性
///
/// 调用者需确保 CPU ID 和中断号对应的操作是安全的
pub(crate) unsafe fn send_ipi(cpu_id: CpuId, irq_num: IrqNum) -> Result<(), IpiSendError> {
    let hart_id = cpu_id.as_usize();
    crate::early_println!("send_ipi: send IPI to CPU {}", hart_id);

    let queue = CPU_IPI_QUEUES
        .get_on_cpu(cpu_id)
        .get()
        .expect("CPU_IPI_QUEUES is not initialized");

    queue.push(irq_num).map_err(|_| IpiSendError::QueueFull)?;

    sbi_rt::send_ipi(build_hart_mask(hart_id))
        .into_result()
        .inspect_err(|e| {
            log::error!(
                "send_ipi: send IPI to CPU {} failed, SBI error: {:?}",
                hart_id,
                e
            );
        })?;
    Ok(())
}

/// 构建支持大范围 hart ID 的掩码
fn build_hart_mask(hart_id: usize) -> sbi_rt::HartMask {
    let (base, mask) = ((hart_id / 32) * 32, 1 << (hart_id % 32));
    sbi_rt::HartMask::from_mask_base(mask, base)
}
