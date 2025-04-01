// SPDX-License-Identifier: MPL-2.0

//! 非类型化物理内存管理。
//!
//! 如 [`crate::mm::frame`] 中详述，非类型化内存可以使用宽松的规则访问，
//! 但我们不能创建对它们的引用。本模块提供了非类型化帧和段的声明，以及
//! 为它们实现的额外功能（如 [`VmIo`]）。

use super::{Frame, Segment, Untyped};
use crate::{
    mm::{
        io::{FallibleVmRead, FallibleVmWrite, Infallible, VmIo, VmReader, VmWriter},
        kspace::paddr_to_vaddr,
    },
    Error, Result,
};

/// 指向非类型化帧的智能指针。
///
/// 在此对象存在期间，帧的用途不会改变。
pub type UFrame = Frame<Untyped>;

/// 非类型化的物理内存范围。
///
/// 非类型化帧或段可以被内核或用户安全地读写。
pub trait UntypedMem {
    /// 借用一个可以读取非类型化内存的读取器。
    fn reader(&self) -> VmReader<'_, Infallible>;
    /// 借用一个可以写入非类型化内存的写入器。
    fn writer(&self) -> VmWriter<'_, Infallible>;
}

macro_rules! impl_untyped_for {
    ($t:ident) => {
        impl UntypedMem for $t<Untyped> {
                    fn reader(&self) -> VmReader<'_, Infallible> {
                        let ptr = paddr_to_vaddr(self.start_paddr()) as *const u8;
                        // 安全性：只有非类型化帧允许被读取。
                        unsafe { VmReader::from_kernel_space(ptr, self.size()) }
                    }

                    fn writer(&self) -> VmWriter<'_, Infallible> {
                        let ptr = paddr_to_vaddr(self.start_paddr()) as *mut u8;
                        // 安全性：只有非类型化帧允许被写入。
                        unsafe { VmWriter::from_kernel_space(ptr, self.size()) }
                    }
                }

                impl VmIo for $t<Untyped> {
            fn read(&self, offset: usize, writer: &mut VmWriter) -> Result<()> {
                        let read_len = writer.avail().min(self.size().saturating_sub(offset));
                        // 考虑潜在的整数溢出进行边界检查
                        let max_offset = offset.checked_add(read_len).ok_or(Error::Overflow)?;
                        if max_offset > self.size() {
                            return Err(Error::InvalidArgs);
                        }
                        let len = self
                            .reader()
                            .skip(offset)
                            .read_fallible(writer)
                            .map_err(|(e, _)| e)?;
                        debug_assert!(len == read_len);
                        Ok(())
                    }

            fn write(&self, offset: usize, reader: &mut VmReader) -> Result<()> {
                let write_len = reader.remain().min(self.size().saturating_sub(offset));
                // 考虑潜在的整数溢出进行边界检查
                let max_offset = offset.checked_add(write_len).ok_or(Error::Overflow)?;
                if max_offset > self.size() {
                    return Err(Error::InvalidArgs);
                }
                let len = self
                    .writer()
                    .skip(offset)
                    .write_fallible(reader)
                    .map_err(|(e, _)| e)?;
                debug_assert!(len == write_len);
                Ok(())
            }
        }
    };
}

impl_untyped_for!(Frame);
impl_untyped_for!(Segment);

// 以下是 `xarray` 的实现。

use core::{marker::PhantomData, mem::ManuallyDrop, ops::Deref};

/// `FrameRef` 是一个可以作为 `&'a Frame<m>` 工作的结构体。
///
/// 这仅对 [`crate::collections::xarray`] 有用。
pub struct FrameRef<'a> {
    inner: ManuallyDrop<Frame<Untyped>>,
    _marker: PhantomData<&'a Frame<Untyped>>,
}

impl<'a> Deref for FrameRef<'a> {
    type Target = Frame<Untyped>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

// 安全性：`Frame` 本质上是一个可以用作 `*const` 指针的 `*const MetaSlot`。
// 该指针也对齐到 4。
unsafe impl xarray::ItemEntry for Frame<Untyped> {
    type Ref<'a>
        = FrameRef<'a>
    where
        Self: 'a;

    fn into_raw(self) -> *const () {
        let ptr = self.ptr;
        let _ = ManuallyDrop::new(self);
        ptr as *const ()
    }

    unsafe fn from_raw(raw: *const ()) -> Self {
        Self {
            ptr: raw as *const _,
            _marker: PhantomData,
        }
    }

    unsafe fn raw_as_ref<'a>(raw: *const ()) -> Self::Ref<'a> {
        Self::Ref {
            inner: ManuallyDrop::new(Frame {
                ptr: raw as *const _,
                _marker: PhantomData,
            }),
            _marker: PhantomData,
        }
    }
}
