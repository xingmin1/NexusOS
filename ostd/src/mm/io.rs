// SPDX-License-Identifier: MPL-2.0

//! 用于读写虚拟内存(VM)对象的抽象。
//!
//! 本模块提供了安全、灵活的内存读写抽象层，可用于访问内核空间和用户空间的内存。
//! 它是操作系统内核中安全内存访问的基础设施，通过类型系统来保证操作的安全性。
//!
//! # 安全性
//!
//! 本模块提供的核心虚拟内存(VM)访问 API 是 [`VmReader`] 和 [`VmWriter`]，
//! 它们允许安全地向内存区域写入或从内存区域读取。
//! `VmReader` 和 `VmWriter` 对象可以从类型化内存(如 `&[u8]`)或非类型化内存(如 [`UFrame`])
//! 的内存区域构造。在底层，`VmReader` 和 `VmWriter` 必须通过它们的 [`from_user_space`] 和
//! [`from_kernel_space`] 方法构造，其安全性取决于给定的内存区域是否有效。
//!
//! [`UFrame`]: crate::mm::UFrame
//! [`from_user_space`]: `VmReader::from_user_space`
//! [`from_kernel_space`]: `VmReader::from_kernel_space`
//!
//! 以下是内存区域被认为有效的条件列表:
//!
//! - 整个内存区域必须是类型化内存或非类型化内存，不能同时是两者。
//!
//! - 如果内存区域是类型化的，我们要求:
//!   - 必须满足官方 Rust 文档中的[有效性要求]，并且
//!   - 内存区域的类型(必须存在因为内存是类型化的)必须是普通旧数据(POD)，
//!     这样写入器就可以安全地用任意数据填充它。
//!
//! [有效性要求]: core::ptr#safety
//!
//! - 如果内存区域是非类型化的，我们要求:
//!   - 在有效性要求生效期间底层页面必须保持活动状态，并且
//!   - 内核必须只使用本模块提供的 API 访问内存区域，但是来自硬件设备或用户程序的外部访问不计算在内。
//!
//! 我们对非类型化内存有最后一个要求，因为目前尚未指定与其他访问内存区域的方式(如原子/易失性内存加载/存储)
//! 的安全交互。如果适当且必要，这可能在将来放宽。
//!
//! 注意，非类型化内存上的数据竞争是明确允许的(因为页面可以映射到用户空间，这使得无法避免数据竞争)。
//! 但是，它们可能会产生错误的结果，如复制意外的字节，但不会导致安全性问题。
//!
//! # Pod (Plain Old Data) 特质解析
//! Pod是"Plain Old Data"（朴素老式数据）的缩写，它是一个标记特质（marker trait），用于表示那些可以安全地按字节级别进行操作的数据类型。
//!
//! ## Pod特质的含义
//! Pod类型具有以下特性：
//!
//! - 可以安全地在内存表示和类型实例之间转换（任意mem::size_of::<T>()字节可以安全地解释为该类型）
//! - 必须实现Copy和Sized特质
//! - 不依赖于特定的内部位模式（bit pattern）语义限制
//!
//! # 可失败性设计
//!
//! 本模块通过泛型参数区分内核空间和用户空间操作的安全性：
//!
//! - `Infallible`：表示内核空间操作，这些操作不会失败，内存访问始终有效
//! - `Fallible`：表示用户空间操作，这些操作可能会因页面错误等原因失败
//!
//! # 使用示例
//!
//! ```
//! // 从内核空间读取数据
//! let mut reader = unsafe { VmReader::from_kernel_space(kernel_ptr, len) };
//! let data: u32 = reader.read_val()?;
//!
//! // 向用户空间写入数据
//! let mut writer = unsafe { VmWriter::from_user_space(user_ptr, len) };
//! writer.write_val(&data)?;
//! ```

use align_ext::AlignExt;
use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use core::marker::PhantomData;

use const_assert::{Assert, IsTrue};
use inherit_methods_macro::inherit_methods;
use ostd_pod::Pod;

use crate::{
    arch::mm::{__memcpy_fallible, __memset_fallible},
    mm::{
        kspace::{KERNEL_BASE_VADDR, KERNEL_END_VADDR},
        MAX_USERSPACE_VADDR,
    },
    Error, Result,
};

/// 一个允许从/向 VM 对象读/写数据的特征，
/// 例如，[`USegment`]、[`Vec<UFrame>`] 和 [`UFrame`]。
///
/// 此特征为虚拟内存对象提供了统一的读写接口，支持不同粒度的操作：
/// 从整块内存拷贝到单个值的读写。所有操作都严格保证完整性，
/// 不允许部分成功的读写操作。
///
/// # 并发
///
/// 这些方法可以由多个并发的读写线程执行。在这种情况下，
/// 如果需要并发读写的结果具有可预测性或原子性，
/// 用户应该添加额外的机制来实现这些属性。
///
/// # 不允许短读/短写
///
/// 所有的读写操作都必须完整处理请求的数据量，如果无法完成，
/// 将返回错误而不是部分处理数据。这简化了错误处理逻辑。
///
/// [`USegment`]: crate::mm::USegment
/// [`UFrame`]: crate::mm::UFrame
pub trait VmIo: Send + Sync {
    /// 将指定偏移处的请求数据读入给定的 `VmWriter`。
    ///
    /// # 不允许短读
    ///
    /// 成功时，`writer` 必须完全写入请求的数据。
    /// 如果由于任何原因，请求的数据只能部分获得，
    /// 那么该方法应返回错误。
    fn read(&self, offset: usize, writer: &mut VmWriter) -> Result<()>;

    /// 将指定偏移处的指定字节数读入给定的缓冲区。
    ///
    /// # 不允许短读
    ///
    /// 类似于 [`read`]。
    ///
    /// [`read`]: VmIo::read
    fn read_bytes(&self, offset: usize, buf: &mut [u8]) -> Result<()> {
        let mut writer = VmWriter::from(buf).to_fallible();
        self.read(offset, &mut writer)
    }

    /// 读取指定偏移处的指定类型的值。
    ///
    /// 这是一个便捷方法，用于读取单个POD类型的值。
    /// 如果偏移处没有足够的字节来构成完整的类型T，将返回错误。
    fn read_val<T: Pod>(&self, offset: usize) -> Result<T> {
        let mut val = T::new_uninit();
        self.read_bytes(offset, val.as_bytes_mut())?;
        Ok(val)
    }

    /// 读取指定偏移处的指定类型的切片。
    ///
    /// # 不允许短读
    ///
    /// 类似于 [`read`]。
    ///
    /// [`read`]: VmIo::read
    fn read_slice<T: Pod>(&self, offset: usize, slice: &mut [T]) -> Result<()> {
        let len_in_bytes = core::mem::size_of_val(slice);
        let ptr = slice as *mut [T] as *mut u8;
        // 安全性：切片可以转换为可写字节切片，因为元素都是普通旧数据(Pod)类型。
        let buf = unsafe { core::slice::from_raw_parts_mut(ptr, len_in_bytes) };
        self.read_bytes(offset, buf)
    }

    /// 将给定 `VmReader` 中的所有数据写入指定偏移处。
    ///
    /// # 不允许短写
    ///
    /// 成功时，必须将 `reader` 中的数据完全写入 VM 对象。
    /// 如果由于任何原因，输入数据只能部分写入，
    /// 那么该方法应返回错误。
    fn write(&self, offset: usize, reader: &mut VmReader) -> Result<()>;

    /// 将给定缓冲区中的指定字节数写入指定偏移处。
    ///
    /// # 不允许短写
    ///
    /// 类似于 [`write`]。
    ///
    /// [`write`]: VmIo::write
    fn write_bytes(&self, offset: usize, buf: &[u8]) -> Result<()> {
        let mut reader = VmReader::from(buf).to_fallible();
        self.write(offset, &mut reader)
    }

    /// 将指定类型的值写入指定偏移处。
    ///
    /// 这是一个便捷方法，用于写入单个POD类型的值。
    /// 如果目标位置没有足够的空间写入整个类型T，将返回错误。
    fn write_val<T: Pod>(&self, offset: usize, new_val: &T) -> Result<()> {
        self.write_bytes(offset, new_val.as_bytes())?;
        Ok(())
    }

    /// 将指定类型的切片写入指定偏移处。
    ///
    /// # 不允许短写
    ///
    /// 类似于 [`write`]。
    ///
    /// [`write`]: VmIo::write
    fn write_slice<T: Pod>(&self, offset: usize, slice: &[T]) -> Result<()> {
        let len_in_bytes = core::mem::size_of_val(slice);
        let ptr = slice as *const [T] as *const u8;
        // 安全性：切片可以转换为可读字节切片，因为元素都是普通旧数据(Pod)类型。
        let buf = unsafe { core::slice::from_raw_parts(ptr, len_in_bytes) };
        self.write_bytes(offset, buf)
    }

    /// 从指定偏移处开始写入迭代器(`iter`)提供的一系列值。
    ///
    /// 写入过程会在 VM 对象没有足够剩余空间或迭代器返回 `None` 时停止。
    /// 如果写入了任何值，函数返回 `Ok(nr_written)`，其中 `nr_written` 是写入的值的数量。
    ///
    /// 此方法写入的每个值都会对齐到 `align` 字节边界。
    /// 自然地，当 `align` 等于 `0` 或 `1` 时，该参数不起作用：
    /// 值将以最紧凑的方式写入。
    ///
    /// # 示例
    ///
    /// 使用 `write_values` 可以轻松地用相同的值初始化 VM 对象。
    ///
    /// ```
    /// use core::iter;
    ///
    /// let _nr_values = vm_obj.write_vals(0, iter::repeat(0_u32), 0).unwrap();
    /// ```
    ///
    /// # Panics
    ///
    /// 如果 `align` 大于 2 但不是 2 的幂，在 release 模式下此方法会 panic。
    fn write_vals<'a, T: Pod + 'a, I: Iterator<Item = &'a T>>(
        &self,
        offset: usize,
        iter: I,
        align: usize,
    ) -> Result<usize> {
        let mut nr_written = 0;

        let (mut offset, item_size) = if (align >> 1) == 0 {
            // align 是 0 或 1
            (offset, core::mem::size_of::<T>())
        } else {
            // align 大于 2
            (
                offset.align_up(align),
                core::mem::size_of::<T>().align_up(align),
            )
        };

        for item in iter {
            match self.write_val(offset, item) {
                Ok(_) => {
                    offset += item_size;
                    nr_written += 1;
                }
                Err(e) => {
                    if nr_written > 0 {
                        return Ok(nr_written);
                    }
                    return Err(e);
                }
            }
        }

        Ok(nr_written)
    }
}

/// 一个允许使用单个非撕裂内存加载/存储从/向 VM 对象读/写数据的特征。
///
/// 另请参见 [`VmIo`]，它允许从/向 VM 对象读/写数据，但不保证使用单个非撕裂内存加载/存储。
pub trait VmIoOnce {
    /// 使用单个非撕裂内存加载在指定偏移处读取 `PodOnce` 类型的值。
    ///
    /// 除了偏移是显式指定的之外，此方法的语义与 [`VmReader::read_once`] 相同。
    fn read_once<T: PodOnce>(&self, offset: usize) -> Result<T>;

    /// 使用单个非撕裂内存存储在指定偏移处写入 `PodOnce` 类型的值。
    ///
    /// 除了偏移是显式指定的之外，此方法的语义与 [`VmWriter::write_once`] 相同。
    fn write_once<T: PodOnce>(&self, offset: usize, new_val: &T) -> Result<()>;
}

macro_rules! impl_vm_io_pointer {
    ($typ:ty,$from:tt) => {
        #[inherit_methods(from = $from)]
        impl<T: VmIo> VmIo for $typ {
            fn read(&self, offset: usize, writer: &mut VmWriter) -> Result<()>;
            fn read_bytes(&self, offset: usize, buf: &mut [u8]) -> Result<()>;
            fn read_val<F: Pod>(&self, offset: usize) -> Result<F>;
            fn read_slice<F: Pod>(&self, offset: usize, slice: &mut [F]) -> Result<()>;
            fn write(&self, offset: usize, reader: &mut VmReader) -> Result<()>;
            fn write_bytes(&self, offset: usize, buf: &[u8]) -> Result<()>;
            fn write_val<F: Pod>(&self, offset: usize, new_val: &F) -> Result<()>;
            fn write_slice<F: Pod>(&self, offset: usize, slice: &[F]) -> Result<()>;
        }
    };
}

impl_vm_io_pointer!(&T, "(**self)");
impl_vm_io_pointer!(&mut T, "(**self)");
impl_vm_io_pointer!(Box<T>, "(**self)");
impl_vm_io_pointer!(Arc<T>, "(**self)");

macro_rules! impl_vm_io_once_pointer {
    ($typ:ty,$from:tt) => {
        #[inherit_methods(from = $from)]
        impl<T: VmIoOnce> VmIoOnce for $typ {
            fn read_once<F: PodOnce>(&self, offset: usize) -> Result<F>;
            fn write_once<F: PodOnce>(&self, offset: usize, new_val: &F) -> Result<()>;
        }
    };
}

impl_vm_io_once_pointer!(&T, "(**self)");
impl_vm_io_once_pointer!(&mut T, "(**self)");
impl_vm_io_once_pointer!(Box<T>, "(**self)");
impl_vm_io_once_pointer!(Arc<T>, "(**self)");

/// 用于 [`VmReader`] 和 [`VmWriter`] 的标记结构，
/// 表示对底层内存区域的读写是可失败的。
pub struct Fallible;
/// 用于 [`VmReader`] 和 [`VmWriter`] 的标记结构，
/// 表示对底层内存区域的读写是不可失败的。
pub struct Infallible;

/// 将 `len` 字节从 `src` 复制到 `dst`。
///
/// # 安全性
///
/// - `src` 必须对 `len` 字节的读取[有效]。
/// - `dst` 必须对 `len` 字节的写入[有效]。
///
/// [有效]: crate::mm::io#safety
unsafe fn memcpy(dst: *mut u8, src: *const u8, len: usize) {
    // 此方法通过调用 `volatile_copy_memory` 实现。注意，即使有 "volatile" 关键字，
    // 数据竞争在 Rust 文档和 C/C++ 标准中仍被视为未定义行为(UB)。一般来说，UB 使得
    // 整个程序的行为不可预测，通常是由于编译器假设不存在 UB 的优化导致的。
    // 然而，在这种特殊情况下，考虑到 Linux 内核使用 "volatile" 关键字来实现
    // `READ_ONCE` 和 `WRITE_ONCE`，除非编译器也破坏了 Linux 内核，否则它不太可能
    // 破坏我们的代码。
    //
    // 更多细节和未来的可能性，请参见
    // <https://github.com/asterinas/asterinas/pull/1001#discussion_r1667317406>。
    // TODO: 看看有没有替代写法
    core::intrinsics::volatile_copy_memory(dst, src, len);
}

/// 将 `len` 字节从 `src` 复制到 `dst`。
/// 如果遇到无法解决的页面错误，此函数将提前停止复制。
///
/// 返回成功复制的字节数。
///
/// 在以下情况下，此方法可能会导致复制意外的字节，但只要满足安全性要求就不会导致安全问题：
/// - 源和目标重叠。
/// - 当前上下文没有关联有效的用户空间(例如，在内核线程中)。
///
/// # 安全性
///
/// - `src` 必须对 `len` 字节的读取[有效]或在用户空间中有 `len` 字节。
/// - `dst` 必须对 `len` 字节的写入[有效]或在用户空间中有 `len` 字节。
///
/// [有效]: crate::mm::io#safety
unsafe fn memcpy_fallible(dst: *mut u8, src: *const u8, len: usize) -> usize {
    let failed_bytes = __memcpy_fallible(dst, src, len);
    len - failed_bytes
}

/// 用指定的 `value` 填充 `dst` 处的 `len` 字节内存。
/// 如果遇到无法解决的页面错误，此函数将提前停止填充。
///
/// 返回成功设置的字节数。
///
/// # 安全性
///
/// - `dst` 必须对 `len` 字节的写入[有效]或在用户空间中有 `len` 字节。
///
/// [有效]: crate::mm::io#safety
unsafe fn memset_fallible(dst: *mut u8, value: u8, len: usize) -> usize {
    let failed_bytes = __memset_fallible(dst, value, len);
    len - failed_bytes
}

/// 从 `VmWriter` 进行可失败的内存读取。
///
/// 此特征提供了一个可能失败的读取操作，主要用于处理用户空间内存，
/// 这些内存可能因页面错误或权限问题导致访问失败。
pub trait FallibleVmRead<F> {
    /// 将所有数据读入写入器，直到满足以下三个条件之一：
    /// 1. 读取器没有剩余数据。
    /// 2. 写入器没有可用空间。
    /// 3. 读取器/写入器遇到某些错误。
    ///
    /// 成功时，返回读取的字节数；
    /// 出错时，返回错误和到目前为止读取的字节数。
    fn read_fallible(
        &mut self,
        writer: &mut VmWriter<'_, F>,
    ) -> core::result::Result<usize, (Error, usize)>;
}

/// 从 `VmReader` 进行可失败的内存写入。
///
/// 此特征提供了一个可能失败的写入操作，主要用于处理用户空间内存，
/// 这些内存可能因页面错误或权限问题导致访问失败。
pub trait FallibleVmWrite<F> {
    /// 从读取器写入所有数据，直到满足以下三个条件之一：
    /// 1. 读取器没有剩余数据。
    /// 2. 写入器没有可用空间。
    /// 3. 读取器/写入器遇到某些错误。
    ///
    /// 成功时，返回写入的字节数；
    /// 出错时，返回错误和到目前为止写入的字节数。
    fn write_fallible(
        &mut self,
        reader: &mut VmReader<'_, F>,
    ) -> core::result::Result<usize, (Error, usize)>;
}

/// `VmReader` 是一个用于从连续内存范围读取数据的读取器。
///
/// `VmReader` 提供了安全地从内存中读取数据的抽象，隐藏了底层内存访问的细节，
/// 并处理内核空间和用户空间内存的不同安全保证。根据Fallibility类型参数，
/// 读取操作可以是可失败的（用户空间）或不可失败的（内核空间）。
///
/// `VmReader` 读取的内存范围可以在内核空间或用户空间中。
/// 当操作范围在内核空间时，该范围内的内存保证有效，
/// 相应的内存读取是不可失败的。
/// 当操作范围在用户空间时，确保创建 `VmReader` 的进程的页表
/// 在 `'a` 的整个生命周期内都是活动的，相应的内存读取被认为是可失败的。
///
/// 当与 `VmWriter` 一起执行读取时，如果其中一个表示类型化内存，
/// 它可以确保此读取器中的读取范围和写入器中的写入范围不重叠。
///
/// 注意：上述重叠是在虚拟地址级别和物理地址级别。
/// 对于重叠的非类型化地址中的 `VmReader` 和 `VmWriter` 操作结果没有保证，
/// 处理这种情况是用户的责任。
pub struct VmReader<'a, Fallibility = Fallible> {
    cursor: *const u8,
    end: *const u8,
    phantom: PhantomData<(&'a [u8], Fallibility)>,
}

macro_rules! impl_read_fallible {
    ($reader_fallibility:ty, $writer_fallibility:ty) => {
        impl<'a> FallibleVmRead<$writer_fallibility> for VmReader<'a, $reader_fallibility> {
                    fn read_fallible(
                        &mut self,
                        writer: &mut VmWriter<'_, $writer_fallibility>,
                    ) -> core::result::Result<usize, (Error, usize)> {
                        let copy_len = self.remain().min(writer.avail());
                        if copy_len == 0 {
                            return Ok(0);
                        }

                        // 安全性：源和目标是读取器和写入器指定的内存范围的子集，
                        // 所以它们要么对读写有效，要么在用户空间中。
                        let copied_len = unsafe {
                            let copied_len = memcpy_fallible(writer.cursor, self.cursor, copy_len);
                            self.cursor = self.cursor.add(copied_len);
                            writer.cursor = writer.cursor.add(copied_len);
                            copied_len
                        };
                        if copied_len < copy_len {
                            Err((Error::PageFault, copied_len))
                        } else {
                            Ok(copied_len)
                        }
                    }
                }
    };
}

macro_rules! impl_write_fallible {
    ($writer_fallibility:ty, $reader_fallibility:ty) => {
        impl<'a> FallibleVmWrite<$reader_fallibility> for VmWriter<'a, $writer_fallibility> {
            fn write_fallible(
                &mut self,
                reader: &mut VmReader<'_, $reader_fallibility>,
            ) -> core::result::Result<usize, (Error, usize)> {
                reader.read_fallible(self)
            }
        }
    };
}

impl_read_fallible!(Fallible, Infallible);
impl_read_fallible!(Fallible, Fallible);
impl_read_fallible!(Infallible, Fallible);
impl_write_fallible!(Fallible, Infallible);
impl_write_fallible!(Fallible, Fallible);
impl_write_fallible!(Infallible, Fallible);

impl<'a> VmReader<'a, Infallible> {
    /// 从指针和长度构造一个 `VmReader`，表示内核空间中的内存范围。
    ///
    /// 这种构造方式创建的读取器可以安全地读取内核空间内存，且操作不会失败。
    /// 这是处理内核内部数据结构的首选方式。
    ///
    /// # 安全性
    ///
    /// `ptr` 在整个生命周期 `a` 内必须对 `len` 字节的读取[有效]。
    ///
    /// [有效]: crate::mm::io#safety
    pub unsafe fn from_kernel_space(ptr: *const u8, len: usize) -> Self {
        // Rust 允许给零大小对象的引用一个很小的地址，
        // 这个地址可能会落在内核虚拟地址空间范围之外。
        // 所以当 `len` 为零时，我们不应该也不需要检查 `ptr`。
        debug_assert!(len == 0 || KERNEL_BASE_VADDR <= ptr as usize);
        debug_assert!(len == 0 || ptr.add(len) as usize <= KERNEL_END_VADDR);

        Self {
            cursor: ptr,
            end: ptr.add(len),
            phantom: PhantomData,
        }
    }

    /// 将所有数据读入写入器，直到满足以下两个条件之一：
    /// 1. 读取器没有剩余数据。
    /// 2. 写入器没有可用空间。
    ///
    /// 返回读取的字节数。
    ///
    /// 这是一个不可失败的操作，适用于内核空间内存的读取。
    pub fn read(&mut self, writer: &mut VmWriter<'_, Infallible>) -> usize {
        let copy_len = self.remain().min(writer.avail());
        if copy_len == 0 {
            return 0;
        }

        // 安全性：源和目标是读取器和写入器指定的内存范围的子集，
        // 所以它们对读写有效。
        unsafe {
            memcpy(writer.cursor, self.cursor, copy_len);
            self.cursor = self.cursor.add(copy_len);
            writer.cursor = writer.cursor.add(copy_len);
        }

        copy_len
    }

    /// 读取 `Pod` 类型的值。
    ///
    /// 这是内核空间内存读取的便捷方法，用于直接获取单个POD类型的值。
    /// 不同于可失败的版本，这个方法更适合处理已知有效的内存区域。
    ///
    /// 如果 `Pod` 类型的长度超过 `self.remain()`，
    /// 此方法将返回 `Err`。
    pub fn read_val<T: Pod>(&mut self) -> Result<T> {
        if self.remain() < core::mem::size_of::<T>() {
            return Err(Error::InvalidArgs);
        }

        let mut val = T::new_uninit();
        let mut writer = VmWriter::from(val.as_bytes_mut());

        self.read(&mut writer);
        Ok(val)
    }

    /// 使用单个非撕裂内存加载读取 `PodOnce` 类型的值。
    ///
    /// 如果 `PodOnce` 类型的长度超过 `self.remain()`，此方法将返回 `Err`。
    ///
    /// 如果 `Pod` 类型对于当前架构来说太大，必须拆分成多个内存加载的话，
    /// 此方法将无法编译。
    ///
    /// # Panics
    ///
    /// 如果读取器的当前位置不满足类型 `T` 的对齐要求，此方法将 panic。
    pub fn read_once<T: PodOnce>(&mut self) -> Result<T> {
        if self.remain() < core::mem::size_of::<T>() {
            return Err(Error::InvalidArgs);
        }

        let cursor = self.cursor.cast::<T>();
        assert!(cursor.is_aligned());

        // 安全性：我们已经检查了剩余字节数至少是 `T` 的大小，
        // 并且光标相对于类型 `T` 正确对齐。所有其他安全要求
        // 与 `Self::read` 相同。
        let val = unsafe { cursor.read_volatile() };
        self.cursor = unsafe { self.cursor.add(core::mem::size_of::<T>()) };

        Ok(val)
    }

    /// 转换为可失败的读取器。
    ///
    /// 这允许将不可失败的读取器（通常用于内核空间）转换为可失败的读取器，
    /// 这在需要统一API处理不同类型内存的场景下很有用。
    pub fn to_fallible(self) -> VmReader<'a, Fallible> {
        // 安全性：转换为可失败的读取器是安全的，因为
        // 1. 可失败性是一个零大小的标记类型，
        // 2. 不可失败的读取器涵盖了可失败读取器的功能。
        unsafe { core::mem::transmute(self) }
    }
}

impl VmReader<'_, Fallible> {
    /// 从指针和长度构造一个 `VmReader`，表示用户空间中的内存范围。
    ///
    /// 这种构造方式创建的读取器可以读取用户空间内存，但操作可能会失败，
    /// 例如因为页面错误或权限不足。这是处理来自用户程序的数据的首选方式。
    ///
    /// # 安全性
    ///
    /// 虚拟地址范围 `ptr..ptr + len` 必须在用户空间中。
    pub unsafe fn from_user_space(ptr: *const u8, len: usize) -> Self {
        debug_assert!((ptr as usize).checked_add(len).unwrap_or(usize::MAX) <= MAX_USERSPACE_VADDR);

        Self {
            cursor: ptr,
            end: ptr.add(len),
            phantom: PhantomData,
        }
    }

    /// 读取 `Pod` 类型的值。
    ///
    /// 这是用户空间内存读取的便捷方法，处理可能出现的页面错误等异常情况。
    /// 如果读取失败，会保持读取器的状态不变，便于重试或错误处理。
    ///
    /// 如果 `Pod` 类型的长度超过 `self.remain()`，
    /// 或者无法完全读取该值，
    /// 此方法将返回 `Err`。
    ///
    /// 如果内存读取失败，此方法将返回 `Err`，
    /// 并且当前读取器的光标保持指向原始起始位置。
    pub fn read_val<T: Pod>(&mut self) -> Result<T> {
        if self.remain() < core::mem::size_of::<T>() {
            return Err(Error::InvalidArgs);
        }

        let mut val = T::new_uninit();
        let mut writer = VmWriter::from(val.as_bytes_mut());
        self.read_fallible(&mut writer)
            .map_err(|(err, copied_len)| {
                // SAFETY: The `copied_len` is the number of bytes read so far.
                // So the `cursor` can be moved back to the original position.
                unsafe {
                    self.cursor = self.cursor.sub(copied_len);
                }
                err
            })?;
        Ok(val)
    }

    /// 将所有剩余字节收集到一个 `Vec<u8>` 中。
    ///
    /// 这是一个实用方法，用于从用户空间读取整块数据到内核缓冲区。
    /// 如果内存读取失败，光标会保持在原始位置，方便错误处理。
    ///
    /// 如果内存读取失败，此方法将返回 `Err`，
    /// 并且当前读取器的光标保持指向原始起始位置。
    pub fn collect(&mut self) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; self.remain()];
        self.read_fallible(&mut buf.as_mut_slice().into())
            .map_err(|(err, copied_len)| {
                // 安全性：copied_len 是已读取的字节数。
                // 因此光标可以移回原始位置。
                unsafe {
                    self.cursor = self.cursor.sub(copied_len);
                }
                err
            })?;
        Ok(buf)
    }
}

impl<Fallibility> VmReader<'_, Fallibility> {
    /// 返回剩余数据的字节数。
    pub const fn remain(&self) -> usize {
        // 安全性：end 大于或等于 cursor。
        unsafe { self.end.sub_ptr(self.cursor) }
    }

    /// 返回光标指针，指向下一个要读取的字节的地址。
    pub const fn cursor(&self) -> *const u8 {
        self.cursor
    }

    /// 返回是否还有剩余数据可读。
    pub const fn has_remain(&self) -> bool {
        self.remain() > 0
    }

    /// 限制剩余数据的长度。
    ///
    /// 此方法确保满足 `self.remain() <= max_remain` 的后置条件。
    pub const fn limit(mut self, max_remain: usize) -> Self {
        if max_remain < self.remain() {
            // 安全性：新的 end 小于旧的 end。
            unsafe { self.end = self.cursor.add(max_remain) };
        }
        self
    }

    /// 跳过前 `nbytes` 个字节的数据。
    /// 剩余数据的长度相应减少。
    ///
    /// # Panic
    ///
    /// 如果 `nbytes` 大于 `self.remain()`，则方法会 panic。
    pub fn skip(mut self, nbytes: usize) -> Self {
        assert!(nbytes <= self.remain());

        // 安全性：新的 cursor 小于或等于 end。
        unsafe { self.cursor = self.cursor.add(nbytes) };
        self
    }
}

impl<'a> From<&'a [u8]> for VmReader<'a, Infallible> {
    fn from(slice: &'a [u8]) -> Self {
        // 安全性：
        // - 内存范围指向类型化内存。
        // - 由于指针是从生命周期为 'a 的不可变引用转换而来，因此满足读取访问的有效性要求。
        // - 类型（即 u8 切片）是普通旧数据。
        unsafe { Self::from_kernel_space(slice.as_ptr(), slice.len()) }
    }
}

/// `VmWriter` 是一个用于向连续内存范围写入数据的写入器。
///
/// `VmWriter` 提供了安全地向内存中写入数据的抽象，隐藏了底层内存访问的细节，
/// 并处理内核空间和用户空间内存的不同安全保证。根据Fallibility类型参数，
/// 写入操作可以是可失败的（用户空间）或不可失败的（内核空间）。
///
/// `VmWriter` 写入的内存范围可以在内核空间或用户空间中。
/// 当操作范围在内核空间时，该范围内的内存保证有效，
/// 相应的内存写入是不可失败的。
/// 当操作范围在用户空间时，确保创建 `VmWriter` 的进程的页表
/// 在 'a 生命周期内保持活动，相应的内存写入被视为可失败的。
///
/// 当使用 `VmReader` 执行写入时，如果其中一个表示类型化内存，
/// 可以确保此写入器中的写入范围和读取器中的读取范围不重叠。
///
/// 注意：上述重叠是在虚拟地址级别和物理地址级别。
/// 对于重叠的非类型化地址中的 `VmReader` 和 `VmWriter` 操作结果
/// 没有保证，处理这种情况是用户的责任。
pub struct VmWriter<'a, Fallibility = Fallible> {
    cursor: *mut u8,
    end: *mut u8,
    phantom: PhantomData<(&'a mut [u8], Fallibility)>,
}

impl<'a> VmWriter<'a, Infallible> {
    /// 从指针和长度构造一个 `VmWriter`，表示内核空间中的内存范围。
    ///
    /// # 安全性
    ///
    /// `ptr` 在整个生命周期 'a 内必须对 `len` 字节的写入[有效]。
    ///
    /// [valid]: crate::mm::io#safety
    pub unsafe fn from_kernel_space(ptr: *mut u8, len: usize) -> Self {
        // 如果将零大小切片转换为指针，该指针可能为空
        // 且不在我们的内核空间范围内。
        debug_assert!(len == 0 || KERNEL_BASE_VADDR <= ptr as usize);
        debug_assert!(len == 0 || ptr.add(len) as usize <= KERNEL_END_VADDR);

        Self {
            cursor: ptr,
            end: ptr.add(len),
            phantom: PhantomData,
        }
    }

    /// 从读取器写入所有数据，直到满足以下两个条件之一：
    /// 1. 读取器没有剩余数据。
    /// 2. 写入器没有可用空间。
    ///
    /// 返回写入的字节数。
    pub fn write(&mut self, reader: &mut VmReader<'_, Infallible>) -> usize {
        reader.read(self)
    }

    /// 写入 `Pod` 类型的值。
    ///
    /// 如果 `Pod` 类型的长度超过 `self.avail()`，
    /// 此方法将返回 `Err`。
    pub fn write_val<T: Pod>(&mut self, new_val: &T) -> Result<()> {
        if self.avail() < core::mem::size_of::<T>() {
            return Err(Error::InvalidArgs);
        }

        let mut reader = VmReader::from(new_val.as_bytes());
        self.write(&mut reader);
        Ok(())
    }

    /// 使用一个非撕裂内存存储写入 `PodOnce` 类型的值。
    ///
    /// 如果 `PodOnce` 类型的长度超过 `self.remain()`，此方法将返回 `Err`。
    ///
    /// # Panic
    ///
    /// 如果写入器的当前位置不满足类型 `T` 的对齐要求，此方法将 panic。
    pub fn write_once<T: PodOnce>(&mut self, new_val: &T) -> Result<()> {
        if self.avail() < core::mem::size_of::<T>() {
            return Err(Error::InvalidArgs);
        }

        let cursor = self.cursor.cast::<T>();
        assert!(cursor.is_aligned());

        // 安全性：我们已检查剩余字节数至少为 T 的大小，
        // 且光标相对于类型 T 正确对齐。所有其他安全要求
        // 与 Self::writer 相同。
        unsafe { cursor.cast::<T>().write_volatile(*new_val) };
        self.cursor = unsafe { self.cursor.add(core::mem::size_of::<T>()) };

        Ok(())
    }

    /// 通过重复 `value` 填充可用空间。
    ///
    /// 返回写入的值的数量。
    ///
    /// # Panic
    ///
    /// 可用空间的大小必须是 `value` 大小的倍数。
    /// 否则，方法会 panic。
    pub fn fill<T: Pod>(&mut self, value: T) -> usize {
        let avail = self.avail();

        assert!((self.cursor as *mut T).is_aligned());
        assert!(avail % core::mem::size_of::<T>() == 0);

        let written_num = avail / core::mem::size_of::<T>();

        for i in 0..written_num {
            // 安全性：`written_num` 是由可用大小和类型 T 的大小计算得出的，
            // 因此 `add` 操作和 `write` 操作是有效的，并且只会操作
            // 此写入器管理的内存。
            unsafe {
                (self.cursor as *mut T).add(i).write_volatile(value);
            }
        }

        // 可用空间已被填满，因此光标可以移动到末尾。
        self.cursor = self.end;
        written_num
    }

    /// 转换为可失败的写入器。
    pub fn to_fallible(self) -> VmWriter<'a, Fallible> {
        // 安全性：转换为可失败的写入器是安全的，因为
        // 1. 可失败性是一个零大小的标记类型，
        // 2. 不可失败的写入器涵盖了可失败写入器的功能。
        unsafe { core::mem::transmute(self) }
    }
}

impl VmWriter<'_, Fallible> {
    /// 从指针和长度构造一个 `VmWriter`，表示用户空间中的内存范围。
    ///
    /// 当前上下文在整个生命周期 'a 内应该始终与有效的用户空间相关联。
    /// 这是为了正确的语义，而不是安全性要求。
    ///
    /// # 安全性
    ///
    /// `ptr` 必须在用户空间中占用 `len` 字节。
    pub unsafe fn from_user_space(ptr: *mut u8, len: usize) -> Self {
        debug_assert!((ptr as usize).checked_add(len).unwrap_or(usize::MAX) <= MAX_USERSPACE_VADDR);

        Self {
            cursor: ptr,
            end: ptr.add(len),
            phantom: PhantomData,
        }
    }

    /// 写入 `Pod` 类型的值。
    ///
    /// 如果 `Pod` 类型的长度超过 `self.avail()`，
    /// 或者无法完全写入该值，
    /// 此方法将返回 `Err`。
    ///
    /// 如果内存写入失败，此方法将返回 `Err`，
    /// 并且当前写入器的光标保持指向原始起始位置。
    pub fn write_val<T: Pod>(&mut self, new_val: &T) -> Result<()> {
        if self.avail() < core::mem::size_of::<T>() {
            return Err(Error::InvalidArgs);
        }

        let mut reader = VmReader::from(new_val.as_bytes());
        self.write_fallible(&mut reader)
            .map_err(|(err, copied_len)| {
                // 安全性：copied_len 是已写入的字节数。
                // 因此光标可以移回原始位置。
                unsafe {
                    self.cursor = self.cursor.sub(copied_len);
                }
                err
            })?;
        Ok(())
    }

    /// 向目标内存写入 `len` 个零。
    ///
    /// 此方法尝试用零填充最多 `len` 个字节。如果从当前光标位置开始的
    /// 可用内存小于 `len`，它将只填充可用空间。
    ///
    /// 如果由于不可解决的页面错误导致内存写入失败，此方法
    /// 将返回 `Err` 以及已设置的长度。
    pub fn fill_zeros(&mut self, len: usize) -> core::result::Result<usize, (Error, usize)> {
        let len_to_set = self.avail().min(len);
        if len_to_set == 0 {
            return Ok(0);
        }

        // 安全性：目标是当前写入器指定的内存范围的子集，
        // 因此它要么有效可写，要么在用户空间中。
        let set_len = unsafe {
            let set_len = memset_fallible(self.cursor, 0u8, len_to_set);
            self.cursor = self.cursor.add(set_len);
            set_len
        };
        if set_len < len_to_set {
            Err((Error::PageFault, set_len))
        } else {
            Ok(len_to_set)
        }
    }
}

impl<Fallibility> VmWriter<'_, Fallibility> {
    /// 返回可用空间的字节数。
    pub const fn avail(&self) -> usize {
        // 安全性：end 大于或等于 cursor。
        unsafe { self.end.sub_ptr(self.cursor) }
    }

    /// 返回光标指针，指向下一个要写入的字节的地址。
    pub const fn cursor(&self) -> *mut u8 {
        self.cursor
    }

    /// 返回是否有可用空间可写。
    pub const fn has_avail(&self) -> bool {
        self.avail() > 0
    }

    /// 限制可用空间的长度。
    ///
    /// 此方法确保满足 `self.avail() <= max_avail` 的后置条件。
    pub const fn limit(mut self, max_avail: usize) -> Self {
        if max_avail < self.avail() {
            // 安全性：新的 end 小于旧的 end。
            unsafe { self.end = self.cursor.add(max_avail) };
        }
        self
    }

    /// 跳过前 `nbytes` 个字节的数据。
    /// 可用空间的长度相应减少。
    ///
    /// # Panic
    ///
    /// 如果 `nbytes` 大于 `self.avail()`，则方法会 panic。
    pub fn skip(mut self, nbytes: usize) -> Self {
        assert!(nbytes <= self.avail());

        // 安全性：新的 cursor 小于或等于 end。
        unsafe { self.cursor = self.cursor.add(nbytes) };
        self
    }
}

impl<'a> From<&'a mut [u8]> for VmWriter<'a, Infallible> {
    fn from(slice: &'a mut [u8]) -> Self {
        // 安全性：
        // - 内存范围指向类型化内存。
        // - 由于指针是从生命周期为 'a 的可变引用转换而来，因此满足写入访问的有效性要求。
        // - 类型（即 u8 切片）是普通旧数据。
        unsafe { Self::from_kernel_space(slice.as_mut_ptr(), slice.len()) }
    }
}

/// 可以用一条指令读取或写入的 POD 类型的标记特征。
///
/// 我们目前依赖此特征来确保 `ptr::read_volatile` 和 `ptr::write_volatile`
/// 创建的内存操作不会撕裂。然而，Rust 文档没有提供这样的保证，
/// 甚至 LLVM LangRef 中的措辞也是模糊的。
///
/// 目前，我们只能_希望_这不会在未来版本的 Rust 或 LLVM 编译器中出现问题。
/// 然而，这在实践中不太可能发生，因为 Linux 内核也使用 "volatile" 语义
/// 来实现 `READ_ONCE`/`WRITE_ONCE`。
///
/// TODO: 需要补充 volatile 的文档以及总结整理下面的注释内容，判断一下是不是原子操作。
///
/// `write_volatile` 只保证：
///
/// - 操作不会被编译器优化掉
/// - 如果操作对应单一硬件指令，就不会撕裂
///
/// 但它不保证：
///
/// - 对其他线程的即时可见性
/// - 与其他内存操作的顺序关系
/// - 任何跨CPU缓存同步
///
/// 这就是为什么代码注释中提到：
/// 目前，我们只能_希望_这不会在未来版本的 Rust 或 LLVM 编译器中出现问题。
///
/// 应用场景
///
/// 非撕裂操作适用于：需要避免数据损坏，但不需要线程间同步的场景
/// 原子操作适用于：需要完整线程安全保证的并发编程场景
///
/// 您的总结基本正确，但需要补充一些细节和注意事项：
///
/// 1. **`volatile` 的核心特性**：
///    - 保证每次读写都严格按代码顺序执行（编译器层面防优化）
///    - 通过阻止编译器优化来避免指令消除，但硬件层面的优化（如 CPU 乱序执行）仍可能发生
///    - 是否单指令取决于目标架构（如 x86 的 32-bit 对齐写通常是原子的，而 64-bit 在 32-bit CPU 上可能需要多条指令）
///
/// 2. **即时可见性误区**：
///    - `volatile` 不保证跨线程可见性（缺乏内存屏障）
///    - 即使单核系统中，若涉及 DMA 等硬件直接访问内存的情况，`volatile` 的可见性仍然有效
///
/// 3. **原子操作 vs `volatile`**：
///    ```rust
///    // volatile 示例（无内存顺序保证）
///    unsafe { ptr.write_volatile(value) }
///
///    // 原子操作示例（明确的顺序保证）
///    atomic.store(value, Ordering::Release)
///    ```
///    原子操作通过内存排序参数（`Ordering`）提供 happens-before 关系，而 `volatile` 没有这种机制
///
/// 4. **典型应用场景对比**：
///    - **`volatile` 适用**：
///      * 硬件寄存器写入
///      * 内存映射 I/O
///      * 与信号处理程序共享的全局变量
///      * 防止编译器优化特定内存访问（如基准测试）
///    - **原子操作适用**：
///      * 多线程计数器
///      * 无锁数据结构
///      * 线程间标志传递
///
/// 5. **关于代码注释的担忧**：
///    - Rust 的 `volatile` 语义基于 LLVM 的 volatile 概念
///    - 未来可能的危险场景：
///      * 编译器开始自动矢量化 volatile 操作
///      * LLVM 对 volatile 指令进行更激进的优化
///      * 目标架构出现新的内存模型变体
///
/// 6. **重要补充**：
///    - `volatile` 不能替代内存屏障（如需要保证写操作的全局可见性，需要配合 `atomic::fence`）
///    - 对 `volatile` 变量的连续多次访问可能被合并（需用 `read_volatile`/`write_volatile` 保护每次操作）
///    - 在 Rust 中，`UnsafeCell` 是唯一合法的 volatile 操作目标（普通变量的 volatile 操作是 UB）
///
/// 结论：您的理解正确，但实际使用中需要特别注意：
/// 1. 严格区分硬件交互场景和线程同步场景
/// 2. 优先使用标准库的原子类型（`Atomic*`）处理并发问题
/// 3. 对 `volatile` 的使用要配合目标平台的架构手册验证
/// 4. 在 unsafe 块中必须人工保证内存安全
pub trait PodOnce: Pod {}

impl<T: Pod> PodOnce for T where Assert<{ is_pod_once::<T>() }>: IsTrue {}

#[cfg(target_arch = "x86_64")]
const fn is_pod_once<T: Pod>() -> bool {
    let size = size_of::<T>();

    size == 1 || size == 2 || size == 4 || size == 8
}

#[cfg(target_arch = "riscv64")]
const fn is_pod_once<T: Pod>() -> bool {
    let size = size_of::<T>();

    size == 1 || size == 2 || size == 4 || size == 8
}
