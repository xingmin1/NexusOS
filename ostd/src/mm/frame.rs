// SPDX-License-Identifier: MPL-2.0

//! 帧（物理内存页）管理。
//!
//! 帧是物理内存中对齐的、连续的字节范围。基本帧和大页帧（映射为"huge pages"）
//! 的大小取决于具体架构。帧可以通过页表映射到虚拟地址空间。
//!
//! 帧可以通过帧句柄（即 [`Frame`]）来访问。帧句柄是指向帧的引用计数指针。
//! 当一个帧的所有句柄都被释放时，该帧会被释放并可以被重用。连续的帧由
//! [`Segment`] 管理。
//!
//! 帧有多种类型。最顶层的分类是"类型化"帧和"非类型化"帧。类型化帧存放
//! 必须遵循 Rust 可见性、生命周期和借用规则的 Rust 对象，因此不能直接
//! 操作。非类型化帧是可以直接操作的原始内存。因此只有非类型化帧可以：
//!  - 安全地共享给外部实体，如设备驱动或用户空间应用程序
//!  - 或者通过忽略 Rust 的"别名 XOR 可变性"规则的读写器直接操作
//!
//! 帧的类型由其元数据的类型决定。

pub mod allocator;
pub mod meta;
pub mod segment;
pub mod untyped;

use core::{
    marker::PhantomData,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
};

use meta::{mapping, MetaSlot, REF_COUNT_UNUSED};
pub use segment::Segment;

use super::{PagingLevel, PAGE_SIZE};
use crate::prelude::{Paddr, Vaddr};

static MAX_PADDR: AtomicUsize = AtomicUsize::new(0);

pub trait MemoryType: Sized + Send + Sync + 'static {
    fn is_typed() -> bool {
        true
    }
}

pub struct Typed;

impl MemoryType for Typed {}

pub struct Untyped;

impl MemoryType for Untyped {
    fn is_typed() -> bool {
        false
    }
}

pub struct Unknown;

impl MemoryType for Unknown {
    fn is_typed() -> bool {
        true
    }
}

/// 指向帧的智能指针。
///
/// 帧是物理内存中连续的字节范围。[`Frame`] 类型是指向帧的引用计数智能指针。
#[derive(Debug)]
#[repr(transparent)]
pub struct Frame<M: MemoryType> {
    ptr: *const MetaSlot,
    _marker: PhantomData<M>,
}

unsafe impl<M: MemoryType> Send for Frame<M> {}

unsafe impl<M: MemoryType> Sync for Frame<M> {}

impl<M: MemoryType + ?Sized> Frame<M> {
    /// 从一个原始的未使用页面获取 [`Frame`]。
    ///
    /// # Panic（异常）
    ///
    /// 以下情况会导致函数异常：
    ///  - 物理地址超出范围或未对齐
    ///  - 页面已在使用中
    pub fn from_unused(paddr: Paddr) -> Self {
        assert_eq!(paddr % PAGE_SIZE, 0);
        assert!(paddr < MAX_PADDR.load(Ordering::Relaxed) as Paddr);

        let vaddr = mapping::frame_to_meta(paddr);
        let ptr = vaddr as *const MetaSlot;

        // 安全性：`ptr` 指向一个有效的 `MetaSlot`，该 MetaSlot 永远不会被可变借用，
        // 因此获取其不可变引用是安全的。
        let slot = unsafe { &*ptr };

        // 由于现在帧仅有引用计数状态，使用Relaxed内存序即可
        // 原子操作本身已经保证了引用计数的一致性
        // 之后对帧的操作不需要与以前对帧的写操作同步，因为不会读未初始化的数据
        slot.ref_count
            .compare_exchange(REF_COUNT_UNUSED, 1, Ordering::Relaxed, Ordering::Relaxed)
            .expect("尝试获取新句柄时发现帧已在使用中");

        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    /// 获取帧的起始物理地址。
    pub fn start_paddr(&self) -> Paddr {
        mapping::meta_to_frame(self.ptr as Vaddr)
    }

    /// 获取页面的分页级别。
    ///
    /// 这是映射该帧的页表项的级别，它决定了帧的大小。
    ///
    /// 目前级别始终为 1，这意味着帧是常规页帧。
    pub const fn level(&self) -> PagingLevel {
        1
    }

    /// 获取页面的字节大小。
    pub const fn size(&self) -> usize {
        PAGE_SIZE
    }

    /// 获取帧的引用计数。
    ///
    /// 返回帧的所有引用数量，包括所有现有的帧句柄（[`Frame`]），
    /// 以及页表中指向该帧的所有映射。
    ///
    /// # 安全性
    ///
    /// 此函数调用是安全的，但使用时需要特别注意。引用计数可能随时被其他线程
    /// 更改，包括在调用此方法和使用其结果之间的时间。
    pub fn reference_count(&self) -> u32 {
        self.ref_count().load(Ordering::Relaxed)
    }

    fn ref_count(&self) -> &AtomicU32 {
        unsafe { &(*self.ptr).ref_count }
    }

    /// 遗忘帧句柄。
    ///
    /// 这将导致帧被泄漏而不调用自定义析构函数。
    ///
    /// 返回帧的物理地址，以便之后可以使用 [`Frame::from_raw`] 恢复帧。
    /// 当一些架构数据结构（如页表）需要持有帧句柄时，这很有用。
    pub(crate) fn into_raw(self) -> Paddr {
        let paddr = self.start_paddr();
        core::mem::forget(self);
        paddr
    }

    /// 从物理地址恢复一个被遗忘的 `Frame`。
    ///
    /// # 安全性
    ///
    /// 调用者应该只恢复之前使用 [`Frame::into_raw`] 遗忘的 `Frame`。
    ///
    /// 且恢复操作对于一个被遗忘的 `Frame` 只能执行一次。否则会发生双重释放。
    ///
    /// 同时，调用者需要确保帧的使用是正确的。此函数不会检查使用情况。
    pub(crate) unsafe fn from_raw(paddr: Paddr) -> Self {
        let vaddr = mapping::frame_to_meta(paddr);
        let ptr = vaddr as *const MetaSlot;

        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    fn slot(&self) -> &MetaSlot {
        // 安全性：`ptr` 指向一个有效的 `MetaSlot`，该 MetaSlot 永远不会被可变借用，
        // 因此获取其不可变引用是安全的。
        unsafe { &*self.ptr }
    }
}

impl<M: MemoryType + ?Sized> Clone for Frame<M> {
    fn clone(&self) -> Self {
        // 安全性：我们已经持有了帧的引用。
        unsafe { self.slot().inc_ref_count() };

        Self {
            ptr: self.ptr,
            _marker: PhantomData,
        }
    }
}

impl<M: MemoryType> Drop for Frame<M> {
    fn drop(&mut self) {
        let last_ref_cnt = self.slot().ref_count.fetch_sub(1, Ordering::Release);
        debug_assert!(last_ref_cnt != 0 && last_ref_cnt != REF_COUNT_UNUSED);

        if last_ref_cnt == 1 {
            // 这里需要一个内存屏障，原因与 `Arc::drop` 的实现中所述相同：
            // <https://doc.rust-lang.org/std/sync/struct.Arc.html#method.drop>
            // 与本函数中最开始的 `Release` 配对，保证后面释放帧等操作不会被重排序到前面（使用帧时）。
            core::sync::atomic::fence(Ordering::Acquire);

            // 安全性：这是最后一个引用，即将被释放。
            unsafe {
                meta::drop_last_in_place(self.ptr as *mut MetaSlot);
            }
        }
    }
}

/// 将帧的引用计数增加一。
///
/// # 安全性
///
/// 调用者必须确保以下条件：
///  1. 物理地址必须代表一个有效的帧；
///  2. 调用者必须已经持有该帧的引用。
pub(crate) unsafe fn inc_frame_ref_count(paddr: Paddr) {
    debug_assert!(paddr % PAGE_SIZE == 0);
    debug_assert!(paddr < MAX_PADDR.load(Ordering::Relaxed) as Paddr);

    let vaddr: Vaddr = mapping::frame_to_meta(paddr);
    // 安全性：`vaddr` 指向一个有效的 `MetaSlot`，该 MetaSlot 永远不会被可变借用，
    // 因此获取其不可变引用是安全的。
    let slot = unsafe { &*(vaddr as *const MetaSlot) };

    // 安全性：我们已经持有了帧的引用。
    unsafe { slot.inc_ref_count() };
}
