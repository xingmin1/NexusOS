// SPDX‑License‑Identifier: MPL‑2.0

use alloc::{boxed::Box, fmt::Debug, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicPtr, Ordering};

use crossbeam_queue::ArrayQueue;
use id_alloc::IdAlloc;
use loongArch64::{ipi, register::ecfg::LineBasedInterrupt};
use spin::Once;

use crate::{
    cpu::CpuId,
    cpu_local,
    sync::{blocking::Mutex, spin::Spinlock, GuardSpinLock, PreemptDisabled, SpinLockGuard},
    trap::TrapFrame,
};

pub type IrqNum = u16;
pub const SOFTWARE_IRQ_BASE: IrqNum = 1024;
pub const SOFTWARE_IRQ_CAP: usize = 256;
const MAX_IRQS: usize = SOFTWARE_IRQ_BASE as usize + SOFTWARE_IRQ_CAP;

/// 全局静态表
#[link_section = ".bss.cpu_local"]
static IRQ_TABLE: [AtomicPtr<IrqLine>; MAX_IRQS] = {
    const Z: AtomicPtr<IrqLine> = AtomicPtr::new(core::ptr::null_mut());
    [Z; MAX_IRQS]
};

#[inline]
pub(crate) unsafe fn register_line(n: IrqNum, p: *const IrqLine) {
    IRQ_TABLE[n as usize].store(p as *mut _, Ordering::Release);
}
#[inline]
pub(crate) unsafe fn unregister_line(n: IrqNum) {
    IRQ_TABLE[n as usize].store(core::ptr::null_mut(), Ordering::Release);
}
#[inline]
pub(crate) fn is_slot_empty(n: IrqNum) -> bool {
    IRQ_TABLE[n as usize].load(Ordering::Acquire).is_null()
}
#[inline]
pub(crate) fn is_plic_source(id: IrqNum) -> bool {
    id != 0 && (id as usize) < SOFTWARE_IRQ_BASE as usize
}

static IRQ_ALLOCATOR: Once<GuardSpinLock<IdAlloc>> = Once::new();
static SOFT_ALLOC: Once<GuardSpinLock<IdAlloc>> = Once::new();
static CALLBACK_ID_ALLOCATOR: Once<Mutex<IdAlloc, Spinlock>> = Once::new();

cpu_local! {
    /// Per‑CPU IPI queue
    pub(crate) static CPU_IPI_QUEUES: Once<ArrayQueue<IrqNum>> = Once::new();
}

/// 回调封装
pub struct CallbackElement {
    function: Box<dyn Fn(&TrapFrame) + Send + Sync + 'static>,
    id: usize,
}
impl CallbackElement {
    pub fn call(&self, tf: &TrapFrame) {
        (self.function)(tf);
    }
}
impl Debug for CallbackElement {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CallbackElement")
            .field("id", &self.id)
            .finish()
    }
}

/// IRQ line handle
#[derive(Debug)]
pub struct IrqLine {
    irq_num: IrqNum,
    callback_list: GuardSpinLock<Vec<CallbackElement>>,
}
impl IrqLine {
    pub unsafe fn acquire(n: IrqNum) -> Arc<&'static Self> {
        let slot = &IRQ_TABLE[n as usize];
        let ptr = slot.load(Ordering::Acquire);
        let line_ref = if ptr.is_null() {
            let leaked = Box::leak(Box::new(Self {
                irq_num: n,
                callback_list: GuardSpinLock::new(Vec::new()),
            }));
            slot.store(leaked, Ordering::Release);
            leaked
        } else {
            &*ptr
        };
        Arc::new(line_ref)
    }
    pub fn num(&self) -> IrqNum {
        self.irq_num
    }
    pub fn callback_list(&self) -> SpinLockGuard<Vec<CallbackElement>, PreemptDisabled> {
        self.callback_list.lock()
    }
    pub fn on_active<F>(&self, f: F) -> IrqCallbackHandle
    where
        F: Fn(&TrapFrame) + Send + Sync + 'static,
    {
        let id = CALLBACK_ID_ALLOCATOR.get().unwrap().lock().alloc().unwrap();
        self.callback_list.lock().push(CallbackElement {
            function: Box::new(f),
            id,
        });
        IrqCallbackHandle {
            irq_num: self.irq_num,
            id,
        }
    }
}

/// Callback handle (drops = unregister)
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
                (*ptr).callback_list.lock().retain(|c| c.id != self.id);
            }
        }
        CALLBACK_ID_ALLOCATOR.get().unwrap().lock().free(self.id);
    }
}

/// 全局初始化
pub(crate) fn init() {
    IRQ_ALLOCATOR.call_once(|| GuardSpinLock::new(IdAlloc::with_capacity(256)));
    SOFT_ALLOC.call_once(|| GuardSpinLock::new(IdAlloc::with_capacity(SOFTWARE_IRQ_CAP)));
    CALLBACK_ID_ALLOCATOR
        .call_once(|| Mutex::new_with_raw_mutex(IdAlloc::with_capacity(256), Spinlock::new()));
    for cpu in crate::cpu::all_cpus() {
        CPU_IPI_QUEUES
            .get_on_cpu(cpu)
            .call_once(|| ArrayQueue::new(32));
    }
}

/// 本地中断允许
pub(crate) fn enable_local() {
    loongArch64::register::ecfg::set_lie(LineBasedInterrupt::all()); // HWI0‑7 + SWI0/1 + IPI + TI:contentReference[oaicite:2]{index=2}
}
/// 等价别名
pub(crate) fn enable_all_local() {
    enable_local();
}

pub(crate) fn disable_local() {
    loongArch64::register::ecfg::set_lie(LineBasedInterrupt::empty());
}
pub(crate) fn is_local_enabled() -> bool {
    !loongArch64::register::ecfg::read().lie().is_empty()
}

/// IPI 发送
#[derive(Debug, PartialEq, Eq)]
pub enum IpiSendError {
    QueueFull,
}

pub unsafe fn send_ipi(cpu_id: CpuId, irq_num: IrqNum) -> Result<(), IpiSendError> {
    let queue = CPU_IPI_QUEUES
        .get_on_cpu(cpu_id)
        .get()
        .expect("CPU_IPI_QUEUES not init");
    queue.push(irq_num).map_err(|_| IpiSendError::QueueFull)?;
    ipi::send_ipi_single(cpu_id.as_usize(), 1);
    Ok(())
}

/// 分配软件 IRQ
pub(crate) fn alloc_soft_irq() -> Option<IrqNum> {
    SOFT_ALLOC
        .get()
        .unwrap()
        .lock()
        .alloc()
        .map(|i| SOFTWARE_IRQ_BASE + i as u16)
}
