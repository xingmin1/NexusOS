// SPDX-License-Identifier: MPL-2.0

//! The CPU-local variable implementation.

use core::{marker::Sync, ops::Deref};

use super::{__cpu_local_end, __cpu_local_start};
use crate::{arch, cpu::CpuId, trap::DisabledLocalIrqGuard};

/// Defines a CPU-local variable.
///
/// The accessors of the CPU-local variables are defined with [`CpuLocal`].
///
/// You can get the reference to the inner object on one CPU by calling
/// [`CpuLocal::get_on_cpu`]. Also if you intend to access the inner object
/// on the current CPU, you can use [`CpuLocal::get_with`]. The latter
/// accessors can be used even if the inner object is not `Sync`.
///
/// # Example
///
/// ```rust
/// use ostd::{cpu_local, cpu::PinCurrentCpu, task::disable_preempt, trap};
/// use core::{sync::atomic::{AtomicU32, Ordering}, cell::Cell};
///
/// cpu_local! {
///     static FOO: AtomicU32 = AtomicU32::new(1);
///     pub static BAR: Cell<usize> = Cell::new(2);
/// }
///
/// fn not_an_atomic_function() {
///     let preempt_guard = disable_preempt();
///     let ref_of_foo = FOO.get_on_cpu(preempt_guard.current_cpu());
///     let val_of_foo = ref_of_foo.load(Ordering::Relaxed);
///     println!("FOO VAL: {}", val_of_foo);
///
///     let irq_guard = trap::disable_local();
///     let bar_guard = BAR.get_with(&irq_guard);
///     let val_of_bar = bar_guard.get();
///     println!("BAR VAL: {}", val_of_bar);
/// }
/// ```
#[macro_export]
macro_rules! cpu_local {
    ($( $(#[$attr:meta])* $vis:vis static $name:ident: $t:ty = $init:expr; )*) => {
        $(
            #[link_section = ".cpu_local"]
            $(#[$attr])* $vis static $name: $crate::cpu::local::CpuLocal<$t> = {
                let val = $init;
                // SAFETY: The per-CPU variable instantiated is statically
                // stored in the special `.cpu_local` section.
                unsafe {
                    $crate::cpu::local::CpuLocal::__new(val)
                }
            };
        )*
    };
}

/// CPU-local objects.
///
/// CPU-local objects are instantiated once per CPU core. They can be shared to
/// other cores. In the context of a preemptible kernel task, when holding the
/// reference to the inner object, the object is always the one in the original
/// core (when the reference is created), no matter which core the code is
/// currently running on.
///
/// For the difference between [`CpuLocal`] and [`super::CpuLocalCell`], see
/// [`super`].
pub struct CpuLocal<T: 'static>(T);

impl<T: 'static> CpuLocal<T> {
    /// Creates a new CPU-local object.
    ///
    /// Please do not call this function directly. Instead, use the
    /// `cpu_local!` macro.
    ///
    /// # Safety
    ///
    /// The caller should ensure that the object initialized by this
    /// function resides in the `.cpu_local` section. Otherwise the
    /// behavior is undefined.
    #[doc(hidden)]
    pub const unsafe fn __new(val: T) -> Self {
        Self(val)
    }

    /// Get access to the underlying value on the current CPU with a
    /// provided IRQ guard.
    ///
    /// By this method, you can borrow a reference to the underlying value
    /// even if `T` is not `Sync`. Because that it is per-CPU and IRQs are
    /// disabled, no other running tasks can access it.
    pub fn get_with<'a>(
        &'static self,
        guard: &'a DisabledLocalIrqGuard,
    ) -> CpuLocalDerefGuard<'a, T> {
        CpuLocalDerefGuard {
            cpu_local: self,
            guard,
        }
    }
    /// 通过闭包函数访问当前CPU上的底层值
    ///
    /// 即使类型T不满足Sync trait，也能通过此方法安全地借用其引用。
    /// 因为这是CPU本地数据且本地中断被禁用，其他运行中的任务无法访问它。
    ///
    /// 参数:
    ///   - f: 接收T的引用并返回U的闭包函数
    /// 返回:
    ///   - 闭包函数的执行结果
    pub fn with<U>(&'static self, f: impl FnOnce(&T) -> U) -> U {
        let _guard = crate::trap::disable_local(); // 禁用本地中断，确保独占访问
        f(unsafe { &*self.as_ptr() }) // 安全地解引用指针并执行闭包
    }

    /// Get access to the underlying value through a raw pointer.
    ///
    /// This function calculates the virtual address of the CPU-local object
    /// based on the CPU-local base address and the offset in the BSP.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the reference to `self` is static.
    pub(crate) unsafe fn as_ptr(&'static self) -> *const T {
        super::has_init::assert_true();

        let offset = self.get_offset();

        let local_base = arch::cpu::local::get_base() as usize;
        let local_va = local_base + offset;

        // A sanity check about the alignment.
        debug_assert_eq!(local_va % core::mem::align_of::<T>(), 0);

        local_va as *mut T
    }

    /// Get the offset of the CPU-local object in the CPU-local area.
    fn get_offset(&'static self) -> usize {
        let bsp_va = self as *const _ as usize;
        let bsp_base = __cpu_local_start as usize;
        // The implementation should ensure that the CPU-local object resides in the `.cpu_local`.
        debug_assert!(bsp_va + core::mem::size_of::<T>() <= __cpu_local_end as usize);

        bsp_va - bsp_base
    }
}

impl<T: 'static + Sync> CpuLocal<T> {
    /// Get access to the copy of value on a specific CPU.
    ///
    /// # Panics
    ///
    /// Panics if the CPU ID is out of range.
    pub fn get_on_cpu(&'static self, cpu_id: CpuId) -> &'static T {
        let target_cpu_id = cpu_id.as_usize() as u32; // Get the target u32 ID
        let bsp_id = crate::cpu::CpuId::bsp(); // Get the actual BSP ID

        // If requesting the BSP's value, return the statically linked one.
        if target_cpu_id == bsp_id.as_usize() as u32 {
            return &self.0;
        }

        // SAFETY: Here we use `Once::get_unchecked` to make getting the CPU-
        // local base faster. The storages must be initialized here so it is
        // safe to do so.
        let ap_areas = unsafe { super::AP_CPU_LOCAL_AREAS.get_unchecked() };

        let ap_segment = ap_areas[target_cpu_id as usize]
            .as_ref()
            .unwrap_or_else(|| {
                panic!("No CPU local area found for requested AP {}", target_cpu_id)
            });

        let base = crate::mm::paddr_to_vaddr(ap_segment.start_paddr());
        let offset = self.get_offset();
        let ptr = (base + offset) as *const T;

        // SAFETY: 指针指向特定AP的已分配和初始化区域。
        // 且在初始化后，区域具有静态生命周期。
        unsafe { &*ptr }
    }
}

// SAFETY: At any given time, only one task can access the inner value `T` of a
// CPU-local variable if `T` is not `Sync`. We guarantee it by disabling the
// reference to the inner value, or turning off preemptions when creating
// the reference.
unsafe impl<T: 'static> Sync for CpuLocal<T> {}

// Prevent valid instances of `CpuLocal` from being copied to any memory areas
// outside the `.cpu_local` section.
impl<T: 'static> !Copy for CpuLocal<T> {}
impl<T: 'static> !Clone for CpuLocal<T> {}

// In general, it does not make any sense to send instances of `CpuLocal` to
// other tasks as they should live on other CPUs to make sending useful.
impl<T: 'static> !Send for CpuLocal<T> {}

/// A guard for accessing the CPU-local object.
///
/// It ensures that the CPU-local object is accessed with IRQs disabled.
/// It is created by [`CpuLocal::borrow_with`].
#[must_use]
pub struct CpuLocalDerefGuard<'a, T: 'static> {
    cpu_local: &'static CpuLocal<T>,
    #[expect(dead_code)]
    guard: &'a DisabledLocalIrqGuard,
}

impl<T: 'static> Deref for CpuLocalDerefGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: it should be properly initialized before accesses.
        // And we do not create a mutable reference over it. The IRQs
        // are disabled so it can only be referenced from this task.
        unsafe { &*self.cpu_local.as_ptr() }
    }
}
