// SPDX-License-Identifier: MPL-2.0

//! 虚拟内存 (VM)。

extern crate alloc;
use alloc::fmt;
use core::ops::Range;

pub use kspace::*;
use ostd_pod::Pod;

use crate::{arch::mm::PagingConsts, mm::page_table::PageTableEntryTrait};
pub(crate) mod dma;
pub mod frame;
pub(crate) mod heap_allocator;
mod io;
pub(crate) mod kspace;
mod offset;
pub(crate) mod page_prop;
pub(crate) mod page_table;
pub mod stat;
pub mod tlb;
pub mod vm_space;

pub use self::{
    dma::{Daddr, DmaCoherent, DmaDirection, DmaStream, DmaStreamSlice, HasDaddr},
    frame::{
        allocator::FrameAllocOptions,
        segment::{Segment, USegment},
        untyped::{UFrame, UntypedMem},
        Frame,
    },
    io::{
        Fallible, FallibleVmRead, FallibleVmWrite, Infallible, PodOnce, VmIo, VmIoOnce, VmReader,
        VmWriter,
    },
    page_prop::{CachePolicy, PageFlags, PageProperty, PrivilegedPageFlags},
    vm_space::VmSpace,
};
/// 虚拟地址类型
pub type Vaddr = usize;

/// 物理地址类型
pub type Paddr = usize;

use core::fmt::Debug;

/// 页表节点或帧的级别
pub type PagingLevel = u8;

/// 定义分页系统的最小常量集合。
/// 这为大多数通用架构的分页模式提供了抽象。
pub(crate) trait PagingConstsTrait: Clone + Debug + Default + Send + Sync + 'static {
    /// 最小页面大小。
    /// 这也是一级页表的页面大小。
    const BASE_PAGE_SIZE: usize;

    /// 页表中的层级数量。
    /// 层级编号从叶节点到根节点，例如
    /// 对于 RISC-V 的 SV39 分页机制，页表共包括 3 层：
    /// 第2层为根页表，第1层为中间页表，第0层为叶页表（映射最终的物理页帧）。
    const NR_LEVELS: PagingLevel;

    /// 页表项（PTE）可以直接用于转换虚拟地址的最高级别。
    /// 这会影响页表支持的最大页面大小。
    const HIGHEST_TRANSLATION_LEVEL: PagingLevel;

    /// 页表项（PTE）的大小
    const PTE_SIZE: usize;

    /// 地址宽度可能是 BASE_PAGE_SIZE.ilog2() + NR_LEVELS * IN_FRAME_INDEX_BITS。
    /// 如果比这个短，最高级别中的高位位将被忽略。
    const ADDRESS_WIDTH: usize;
}

/// 页面大小
pub const PAGE_SIZE: usize = page_size::<PagingConsts>(1);

/// 计算指定分页级别对应的页面大小
///
/// 在分页系统中，不同级别对应不同大小的页面：
/// - 级别1: 基本页面大小 (通常为4KB)
/// - 级别2: 中等页面大小 (通常为2MB，即"大页")
/// - 级别3: 更大页面大小 (通常为1GB，即"超大页")
/// - 更高级别: 根据架构支持可能有不同大小
///
/// 计算公式基于基本页大小和每级页表的条目数关系，使用位移操作计算。
pub(crate) const fn page_size<C: PagingConstsTrait>(level: PagingLevel) -> usize {
    C::BASE_PAGE_SIZE << (nr_subpage_per_huge::<C>().ilog2() as usize * (level as usize - 1))
}

/// 一个大页中的子页数量
pub(crate) const fn nr_subpage_per_huge<C: PagingConstsTrait>() -> usize {
    C::BASE_PAGE_SIZE / C::PTE_SIZE
}

/// 指定级别的大页中包含的基本页数量
#[expect(dead_code)]
pub(crate) const fn nr_base_per_page<C: PagingConstsTrait>(level: PagingLevel) -> usize {
    page_size::<C>(level) / C::BASE_PAGE_SIZE
}

/// 用户空间的最大虚拟地址（不包含）
///
/// 典型的64位系统至少有48位虚拟地址空间。
/// 为内核预留一半地址空间的典型方法是
/// 使用最高的48位虚拟地址空间。
///
/// 另外，最顶部的页面不被视为可用，因为这是
/// 一些x86_64 CPU bug的解决方案。详见
/// <https://github.com/torvalds/linux/blob/480e035fc4c714fb5536e64ab9db04fedc89e910/arch/x86/include/asm/page_64.h#L68-L78>
/// 了解具体原因。
pub const MAX_USERSPACE_VADDR: Vaddr = 0x0000_8000_0000_0000 - PAGE_SIZE;

/// 内核地址空间
///
/// 这是大多数48位宽度架构中定义的高规范地址。
pub const KERNEL_VADDR_RANGE: Range<Vaddr> = 0xffff_8000_0000_0000..0xffff_ffff_ffff_0000;

/// 获取物理地址的特征
pub trait HasPaddr {
    /// 返回物理地址
    fn paddr(&self) -> Paddr;
}

/// 检查给定地址是否按页对齐
pub const fn is_page_aligned(p: usize) -> bool {
    (p & (PAGE_SIZE - 1)) == 0
}
