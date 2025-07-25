// SPDX-License-Identifier: MPL-2.0

use alloc::sync::Arc;
use core::{
    cell::UnsafeCell,
    fmt,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

use super::{guard::Guardian, LocalIrqDisabled, PreemptDisabled};

/// A spin lock.
///
/// # Guard behavior
///
/// The type `G' specifies the guard behavior of the spin lock. While holding the lock,
/// - if `G` is [`PreemptDisabled`], preemption is disabled;
/// - if `G` is [`LocalIrqDisabled`], local IRQs are disabled.
///
/// The `G` can also be provided by other crates other than ostd,
/// if it behaves similar like [`PreemptDisabled`] or [`LocalIrqDisabled`].
///
/// The guard behavior can be temporarily upgraded from [`PreemptDisabled`] to
/// [`LocalIrqDisabled`] using the [`disable_irq`] method.
///
/// [`disable_irq`]: Self::disable_irq
#[repr(transparent)]
pub struct GuardSpinLock<T: ?Sized, G = PreemptDisabled> {
    phantom: PhantomData<G>,
    /// Only the last field of a struct may have a dynamically sized type.
    /// That's why SpinLockInner is put in the last field.
    inner: SpinLockInner<T>,
}

struct SpinLockInner<T: ?Sized> {
    lock: AtomicBool,
    val: UnsafeCell<T>,
}

impl<T, G> GuardSpinLock<T, G> {
    /// Creates a new spin lock.
    pub const fn new(val: T) -> Self {
        let lock_inner = SpinLockInner {
            lock: AtomicBool::new(false),
            val: UnsafeCell::new(val),
        };
        Self {
            phantom: PhantomData,
            inner: lock_inner,
        }
    }
}

impl<T: ?Sized> GuardSpinLock<T, PreemptDisabled> {
    /// Converts the guard behavior from disabling preemption to disabling IRQs.
    pub fn disable_irq(&self) -> &GuardSpinLock<T, LocalIrqDisabled> {
        let ptr = self as *const GuardSpinLock<T, PreemptDisabled>;
        let ptr = ptr as *const GuardSpinLock<T, LocalIrqDisabled>;
        // SAFETY:
        // 1. The types `SpinLock<T, PreemptDisabled>`, `SpinLockInner<T>` and `SpinLock<T,
        //    IrqDisabled>` have the same memory layout guaranteed by `#[repr(transparent)]`.
        // 2. The specified memory location can be borrowed as an immutable reference for the
        //    specified lifetime.
        unsafe { &*ptr }
    }
}

impl<T: ?Sized, G: Guardian> GuardSpinLock<T, G> {
    /// Acquires the spin lock.
    pub fn lock(&self) -> SpinLockGuard<T, G> {
        // Notice the guard must be created before acquiring the lock.
        let inner_guard = G::guard();
        self.acquire_lock();
        SpinLockGuard_ {
            lock: self,
            guard: inner_guard,
        }
    }

    /// Acquires the spin lock through an [`Arc`].
    ///
    /// The method is similar to [`lock`], but it doesn't have the requirement
    /// for compile-time checked lifetimes of the lock guard.
    ///
    /// [`lock`]: Self::lock
    pub fn lock_arc(self: &Arc<Self>) -> ArcSpinLockGuard<T, G> {
        let inner_guard = G::guard();
        self.acquire_lock();
        SpinLockGuard_ {
            lock: self.clone(),
            guard: inner_guard,
        }
    }

    /// Tries acquiring the spin lock immedidately.
    pub fn try_lock(&self) -> Option<SpinLockGuard<T, G>> {
        let inner_guard = G::guard();
        if self.try_acquire_lock() {
            let lock_guard = SpinLockGuard_ {
                lock: self,
                guard: inner_guard,
            };
            return Some(lock_guard);
        }
        None
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// This method is zero-cost: By holding a mutable reference to the lock, the compiler has
    /// already statically guaranteed that access to the data is exclusive.
    pub fn get_mut(&mut self) -> &mut T {
        self.inner.val.get_mut()
    }

    /// Acquires the spin lock, otherwise busy waiting
    fn acquire_lock(&self) {
        while !self.try_acquire_lock() {
            core::hint::spin_loop();
        }
    }

    fn try_acquire_lock(&self) -> bool {
        self.inner
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    fn release_lock(&self) {
        self.inner.lock.store(false, Ordering::Release);
    }
}

impl<T: ?Sized + fmt::Debug, G> fmt::Debug for GuardSpinLock<T, G> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner.val, f)
    }
}

// SAFETY: Only a single lock holder is permitted to access the inner data of Spinlock.
unsafe impl<T: ?Sized + Send, G> Send for GuardSpinLock<T, G> {}
unsafe impl<T: ?Sized + Send, G> Sync for GuardSpinLock<T, G> {}

/// A guard that provides exclusive access to the data protected by a [`SpinLock`].
pub type SpinLockGuard<'a, T, G> = SpinLockGuard_<T, &'a GuardSpinLock<T, G>, G>;
/// A guard that provides exclusive access to the data protected by a `Arc<SpinLock>`.
pub type ArcSpinLockGuard<T, G> = SpinLockGuard_<T, Arc<GuardSpinLock<T, G>>, G>;

/// The guard of a spin lock.
#[clippy::has_significant_drop]
#[must_use]
pub struct SpinLockGuard_<T: ?Sized, R: Deref<Target = GuardSpinLock<T, G>>, G: Guardian> {
    guard: G::Guard,
    lock: R,
}

impl<T: ?Sized, R: Deref<Target = GuardSpinLock<T, G>>, G: Guardian> SpinLockGuard_<T, R, G> {
    /// Returns a reference to the guard.
    pub fn guard(&self) -> &G::Guard {
        &self.guard
    }
}

impl<T: ?Sized, R: Deref<Target = GuardSpinLock<T, G>>, G: Guardian> Deref
    for SpinLockGuard_<T, R, G>
{
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.inner.val.get() }
    }
}

impl<T: ?Sized, R: Deref<Target = GuardSpinLock<T, G>>, G: Guardian> DerefMut
    for SpinLockGuard_<T, R, G>
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.inner.val.get() }
    }
}

impl<T: ?Sized, R: Deref<Target = GuardSpinLock<T, G>>, G: Guardian> Drop
    for SpinLockGuard_<T, R, G>
{
    fn drop(&mut self) {
        self.lock.release_lock();
    }
}

impl<T: ?Sized + fmt::Debug, R: Deref<Target = GuardSpinLock<T, G>>, G: Guardian> fmt::Debug
    for SpinLockGuard_<T, R, G>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized, R: Deref<Target = GuardSpinLock<T, G>>, G: Guardian> !Send
    for SpinLockGuard_<T, R, G>
{
}

// SAFETY: `SpinLockGuard_` can be shared between tasks/threads in same CPU.
// As `lock()` is only called when there are no race conditions caused by interrupts.
unsafe impl<T: ?Sized + Sync, R: Deref<Target = GuardSpinLock<T, G>> + Sync, G: Guardian> Sync
    for SpinLockGuard_<T, R, G>
{
}
