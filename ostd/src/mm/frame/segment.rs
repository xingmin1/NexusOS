// SPDX-License-Identifier: MPL-2.0

//! 连续帧范围。

use core::{mem::ManuallyDrop, ops::Range};

use super::{inc_frame_ref_count, Frame, MemoryType, Typed, Untyped};
use crate::{mm::PAGE_SIZE, prelude::Paddr};

/// 连续的同质物理内存帧范围。
///
/// 这是多个连续帧的句柄。它比拥有一个帧句柄数组更轻量级。
///
/// 所有权通过帧的引用计数机制实现。在构造 [`Segment`] 时，
/// 帧句柄被创建然后遗忘，只留下引用计数。当释放它时，
/// 帧句柄被恢复并释放，减少引用计数。
#[derive(Debug)]
#[repr(transparent)]
pub struct Segment<T: MemoryType> {
    range: Range<Paddr>,
    _marker: core::marker::PhantomData<T>,
}

/// 非类型化物理内存帧的连续范围。
///
/// 在此对象存在期间，帧的用途不会改变。
pub type USegment = Segment<Untyped>;

impl<T: MemoryType + ?Sized> Drop for Segment<T> {
    fn drop(&mut self) {
        for paddr in self.range.clone().step_by(PAGE_SIZE) {
            // 安全性：对于每个帧，在创建 `Segment` 对象时都会有一个被遗忘的句柄。
            drop(unsafe { Frame::<T>::from_raw(paddr) });
        }
    }
}

impl<T: MemoryType + ?Sized> Clone for Segment<T> {
    fn clone(&self) -> Self {
        for paddr in self.range.clone().step_by(PAGE_SIZE) {
            // 安全性：对于每个帧，在创建 `Segment` 对象时都会有一个被遗忘的句柄，
            // 所以我们已经拥有这些帧的引用计数。
            unsafe { inc_frame_ref_count(paddr) };
        }
        Self {
            range: self.range.clone(),
            _marker: core::marker::PhantomData,
        }
    }
}

impl<T: MemoryType> Segment<T> {
    /// 从未使用的帧创建一个新的 [`Segment`]。
    ///
    /// # Panic（异常）
    ///
    /// 以下情况会导致函数异常：
    ///  - 物理地址无效或未对齐；
    ///  - 任何帧已在使用中。
    pub fn from_unused(range: Range<Paddr>) -> Self {
        for paddr in range.clone().step_by(PAGE_SIZE) {
            let _ = ManuallyDrop::new(Frame::<T>::from_unused(paddr));
        }
        Self {
            range,
            _marker: core::marker::PhantomData,
        }
    }

    /// 获取连续帧的起始物理地址。
    pub fn start_paddr(&self) -> Paddr {
        self.range.start
    }

    /// 获取连续帧的结束物理地址。
    pub fn end_paddr(&self) -> Paddr {
        self.range.end
    }

    /// 获取连续帧的字节长度。
    pub fn size(&self) -> usize {
        self.range.end - self.range.start
    }

    /// 在给定的字节偏移处将帧分割成两部分。
    ///
    /// 分割后的帧不能为空。因此偏移量既不能为零也不能为帧的长度。
    ///
    /// # Panic（异常）
    ///
    /// 如果偏移量超出范围、位于任一端点，或者不是基本页面对齐的，函数会异常。
    pub fn split(self, offset: usize) -> (Self, Self) {
        assert!(offset % PAGE_SIZE == 0);
        assert!(0 < offset && offset < self.size());

        let old = ManuallyDrop::new(self);
        let at = old.range.start + offset;

        (
            Self {
                range: old.range.start..at,
                _marker: core::marker::PhantomData,
            },
            Self {
                range: at..old.range.end,
                _marker: core::marker::PhantomData,
            },
        )
    }

    /// 获取字节偏移范围内帧的额外句柄。
    ///
    /// 切片的字节偏移范围由连续帧起始位置的偏移量索引。
    /// 产生的帧持有额外的引用计数。
    ///
    /// # Panic（异常）
    ///
    /// 如果字节偏移范围超出边界，或者字节偏移范围的任一端点不是基本页面对齐的，函数会异常。
    pub fn slice(&self, range: &Range<usize>) -> Self {
        assert!(range.start % PAGE_SIZE == 0 && range.end % PAGE_SIZE == 0);
        let start = self.range.start + range.start;
        let end = self.range.start + range.end;
        assert!(start <= end && end <= self.range.end);

        for paddr in (start..end).step_by(PAGE_SIZE) {
            // 安全性：我们已经拥有这些帧的引用计数，因为对于每个帧，
            // 在创建 `Segment` 对象时都会有一个被遗忘的句柄。
            unsafe { inc_frame_ref_count(paddr) };
        }

        Self {
            range: start..end,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<T: MemoryType + ?Sized> From<Frame<T>> for Segment<T> {
    fn from(frame: Frame<T>) -> Self {
        let pa = frame.start_paddr();
        let _ = ManuallyDrop::new(frame);
        Self {
            range: pa..pa + PAGE_SIZE,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<T: MemoryType + ?Sized> Iterator for Segment<T> {
    type Item = Frame<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.range.start < self.range.end {
            // 安全性：在创建 `Segment` 对象时，范围内的每个帧都会有一个被遗忘的句柄。
            let frame = unsafe { Frame::<T>::from_raw(self.range.start) };
            self.range.start += PAGE_SIZE;
            // 结束位置不能是非页面对齐的。
            debug_assert!(self.range.start <= self.range.end);
            Some(frame)
        } else {
            None
        }
    }
}

impl TryFrom<Segment<Typed>> for USegment {
    type Error = Segment<Typed>;

    /// 尝试将 [`Segment<Typed>`] 转换为 [`USegment`]。
    ///
    /// 如果页面的用途与预期用途不同，它将返回动态页面本身。
    fn try_from(seg: Segment<Typed>) -> core::result::Result<Self, Self::Error> {
        // 安全性：对于每个页面，在创建 `Segment` 对象时都会有一个被遗忘的句柄。
        let first_frame = unsafe { Frame::<Typed>::from_raw(seg.range.start) };
        let first_frame = ManuallyDrop::new(first_frame);
        if !first_frame.dyn_meta().is_untyped() {
            return Err(seg);
        }
        // 由于段是同质的，我们可以安全地假设其余的帧也是相同类型。
        // 这里我们只进行调试检查。
        #[cfg(debug_assertions)]
        {
            for paddr in seg.range.clone().step_by(PAGE_SIZE) {
                let frame = unsafe { Frame::<Typed>::from_raw(paddr) };
                let frame = ManuallyDrop::new(frame);
                debug_assert!(frame.dyn_meta().is_untyped());
            }
        }
        // 安全性：元数据可以强制转换，结构体可以转换。
        Ok(unsafe { core::mem::transmute::<Segment<Typed>, USegment>(seg) })
    }
}
