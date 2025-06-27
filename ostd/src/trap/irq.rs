// SPDX-License-Identifier: MPL-2.0

use core::fmt::Debug;
use smallvec::SmallVec;
#[cfg(target_arch = "riscv64")]
use crate::arch::riscv::plic;

use crate::{
    arch::irq::{self, IrqCallbackHandle},
    prelude::*,
    sync::GuardTransfer,
    trap::TrapFrame,
    Error,
};

/// Type alias for the irq callback function.
pub type IrqCallbackFunction = dyn Fn(&TrapFrame) + Sync + Send + 'static;

/// An Interrupt ReQuest(IRQ) line. User can use [`alloc`] or [`alloc_specific`] to get specific IRQ line.
///
/// The IRQ number is guaranteed to be external IRQ number and user can register callback functions to this IRQ resource.
/// When this resource is dropped, all the callback in this will be unregistered automatically.
///
/// [`alloc`]: Self::alloc
/// [`alloc_specific`]: Self::alloc_specific
pub type IrqNum = u16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceKind {
    External,
    Software,
}

/// 中断线
#[must_use]
#[derive(Debug)]
pub struct IrqLine {
    irq_num: IrqNum,
    kind: SourceKind,
    #[expect(clippy::redundant_allocation)]
    inner_irq: Arc<&'static irq::IrqLine>,
    callbacks: SmallVec<[IrqCallbackHandle; 4]>,
}

impl IrqLine {
    /// Allocates a specific *external* IRQ line.
    pub fn alloc_specific(irq: IrqNum) -> Result<Self> {
        if irq::is_slot_empty(irq) {
            Ok(Self::new(irq, SourceKind::External))
        } else {
            Err(Error::NotEnoughResources.into())
        }
    }

    /// Allocates the next available *software* IRQ (IPI).
    pub fn alloc_software() -> Result<Self> {
        irq::alloc_soft_irq()
            .map(|n| Self::new(n, SourceKind::Software))
            .ok_or(Error::NotEnoughResources.into())
    }

    // /// Allocates an available IRQ line.
    // pub fn alloc() -> Result<Self> {
    //     let Some(irq_num) = IRQ_ALLOCATOR.get().unwrap().lock().alloc() else {
    //         return Err(Error::NotEnoughResources);
    //     };
    //     Ok(Self::new(irq_num as IrqNum, SourceKind::External))
    // }

    fn new(irq_num: IrqNum, kind: SourceKind) -> Self {
        // SAFETY: IRQ 号由上层逻辑保证合法
        let inner_irq = unsafe { irq::IrqLine::acquire(irq_num) };
        // 注册到全局快查表
        unsafe { irq::register_line(irq_num, *inner_irq.as_ref() as *const _) };

        // 若为外部中断且来自 PLIC，则自动启用
        #[cfg(target_arch = "riscv64")]
        if kind == SourceKind::External && irq::is_plic_source(irq_num) {
            let plic = plic::handle();

            use crate::cpu::all_cpus;
            all_cpus().for_each(|cpu| {
                plic.enable(cpu.as_usize(), irq_num as u32);
                plic.set_priority(irq_num as u32, 1);
            });
        }

        Self {
            irq_num,
            kind,
            inner_irq,
            callbacks: SmallVec::new(),
        }
    }

    /// Gets the IRQ number.
    pub fn num(&self) -> IrqNum {
        self.irq_num
    }

    /// Registers a callback that will be invoked when the IRQ is active.
    ///
    /// For each IRQ line, multiple callbacks may be registered.
    pub fn on_active<F>(&mut self, callback: F)
    where
        F: Fn(&TrapFrame) + Sync + Send + 'static,
    {
        self.callbacks.push(self.inner_irq.on_active(callback))
    }

    /// Checks if there are no registered callbacks.
    pub fn is_empty(&self) -> bool {
        self.callbacks.is_empty()
    }

    #[allow(unused)]
    pub(crate) fn inner_irq(&self) -> &'static irq::IrqLine {
        &self.inner_irq
    }
}

impl Clone for IrqLine {
    fn clone(&self) -> Self {
        Self {
            irq_num: self.irq_num,
            kind: self.kind,
            inner_irq: self.inner_irq.clone(),
            callbacks: SmallVec::new(),
        }
    }
}

impl Drop for IrqLine {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner_irq) != 1 {
            return;
        }

        // 取消注册
        unsafe { irq::unregister_line(self.irq_num) };

        // 若是外部中断且来自 PLIC，则关闭
        #[cfg(target_arch = "riscv64")]
        if self.kind == SourceKind::External && irq::is_plic_source(self.irq_num) {
            // plic::disable(self.irq_num as usize);
        }
    }
}

/// Disables all IRQs on the current CPU (i.e., locally).
///
/// This function returns a guard object, which will automatically enable local IRQs again when
/// it is dropped. This function works correctly even when it is called in a _nested_ way.
/// The local IRQs shall only be re-enabled when the most outer guard is dropped.
///
/// This function can play nicely with [`SpinLock`] as the type uses this function internally.
/// One can invoke this function even after acquiring a spin lock. And the reversed order is also ok.
///
/// [`SpinLock`]: crate::sync::SpinLock
///
/// # Example
///
/// ```rust
/// use ostd::irq;
///
/// {
///     let _ = irq::disable_local();
///     todo!("do something when irqs are disabled");
/// }
/// ```
pub fn disable_local() -> DisabledLocalIrqGuard {
    DisabledLocalIrqGuard::new()
}

/// A guard for disabled local IRQs.
#[clippy::has_significant_drop]
#[must_use]
pub struct DisabledLocalIrqGuard {
    was_enabled: bool,
}

impl !Send for DisabledLocalIrqGuard {}

impl DisabledLocalIrqGuard {
    #[track_caller]
    fn new() -> Self {
        let was_enabled = irq::is_local_enabled();
        if was_enabled {
            irq::disable_local();
        }
        Self { was_enabled }
    }
}

impl GuardTransfer for DisabledLocalIrqGuard {
    fn transfer_to(&mut self) -> Self {
        let was_enabled = self.was_enabled;
        self.was_enabled = false;
        Self { was_enabled }
    }
}

impl Drop for DisabledLocalIrqGuard {
    fn drop(&mut self) {
        if self.was_enabled {
            irq::enable_local();
        }
    }
}
