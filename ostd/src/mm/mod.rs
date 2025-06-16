// SPDX-License-Identifier: MPL-2.0

//! Virtual memory (VM).

/// Virtual addresses.
pub type Vaddr = usize;

/// Physical addresses.
pub type Paddr = usize;

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

use core::{fmt::Debug, ops::Range};

pub use self::{
    dma::{Daddr, DmaCoherent, DmaDirection, DmaStream, DmaStreamSlice, HasDaddr},
    frame::{
        allocator::FrameAllocOptions,
        segment::{Segment, USegment},
        unique::UniqueFrame,
        untyped::{AnyUFrameMeta, UFrame, UntypedMem},
        Frame,
    },
    io::{
        Fallible, FallibleVmRead, FallibleVmWrite, Infallible, PodOnce, VmIo, VmIoOnce, VmReader,
        VmWriter,
    },
    page_prop::{CachePolicy, PageFlags, PageProperty, PrivilegedPageFlags},
    vm_space::VmSpace,
};
pub(crate) use self::{
    frame::meta::init as init_page_meta, kspace::paddr_to_vaddr,
};
use crate::arch::mm::PagingConsts;

/// The level of a page table node or a frame.
pub type PagingLevel = u8;

/// A minimal set of constants that determines the paging system.
/// This provides an abstraction over most paging modes in common architectures.
pub(crate) trait PagingConstsTrait: Clone + Debug + Default + Send + Sync + 'static {
    /// The smallest page size.
    /// This is also the page size at level 1 page tables.
    const BASE_PAGE_SIZE: usize;

    /// The number of levels in the page table.
    /// The numbering of levels goes from deepest node to the root node. For example,
    /// the level 1 to 5 on AMD64 corresponds to Page Tables, Page Directory Tables,
    /// Page Directory Pointer Tables, Page-Map Level-4 Table, and Page-Map Level-5
    /// Table, respectively.
    const NR_LEVELS: PagingLevel;

    /// The highest level that a PTE can be directly used to translate a VA.
    /// This affects the the largest page size supported by the page table.
    const HIGHEST_TRANSLATION_LEVEL: PagingLevel;

    /// The size of a PTE.
    const PTE_SIZE: usize;

    /// The address width may be BASE_PAGE_SIZE.ilog2() + NR_LEVELS * IN_FRAME_INDEX_BITS.
    /// If it is shorter than that, the higher bits in the highest level are ignored.
    const ADDRESS_WIDTH: usize;
}

/// The page size
pub const PAGE_SIZE: usize = page_size::<PagingConsts>(1);

/// The page size at a given level.
pub(crate) const fn page_size<C: PagingConstsTrait>(level: PagingLevel) -> usize {
    C::BASE_PAGE_SIZE << (nr_subpage_per_huge::<C>().ilog2() as usize * (level as usize - 1))
}

/// The number of sub pages in a huge page.
pub(crate) const fn nr_subpage_per_huge<C: PagingConstsTrait>() -> usize {
    C::BASE_PAGE_SIZE / C::PTE_SIZE
}

/// The number of base pages in a huge page at a given level.
#[expect(dead_code)]
pub(crate) const fn nr_base_per_page<C: PagingConstsTrait>(level: PagingLevel) -> usize {
    page_size::<C>(level) / C::BASE_PAGE_SIZE
}

/// The maximum virtual address of user space (non inclusive).
///
/// Typical 64-bit systems have at least 48-bit virtual address space.
/// A typical way to reserve half of the address space for the kernel is
/// to use the highest 48-bit virtual address space.
///
/// Also, the top page is not regarded as usable since it's a workaround
/// for some x86_64 CPUs' bugs. See
/// <https://github.com/torvalds/linux/blob/480e035fc4c714fb5536e64ab9db04fedc89e910/arch/x86/include/asm/page_64.h#L68-L78>
/// for the rationale.
pub const MAX_USERSPACE_VADDR: Vaddr = 0x0000_8000_0000_0000 - PAGE_SIZE;

/// The kernel address space.
///
/// There are the high canonical addresses defined in most 48-bit width
/// architectures.
pub const KERNEL_VADDR_RANGE: Range<Vaddr> = 0xffff_8000_0000_0000..0xffff_ffff_ffff_0000;

/// Gets physical address trait
pub trait HasPaddr {
    /// Returns the physical address.
    fn paddr(&self) -> Paddr;
}

/// Checks if the given address is page-aligned.
pub const fn is_page_aligned(p: usize) -> bool {
    (p & (PAGE_SIZE - 1)) == 0
}

use crate::{
    arch::mm::PageTableEntry,
    mm::{
        kspace::{KERNEL_PAGE_TABLE, LINEAR_MAPPING_BASE_VADDR, LINEAR_MAPPING_VADDR_RANGE},
        page_table::page_walk,
        vm_space::ACTIVATED_VM_SPACE,
    },
};

/// 利用 CPU 中的当前页表根完成 VA→PA 转换。（仅在boot阶段使用？）
///
/// 内核与用户地址均可调用；未映射返回 `None`。
///
/// 在 RISC-V 上，“读 satp 得根物理页帧”硬件延迟并不大，但 序列化行为 使其在高性能核心里等价于一次 TLB flush 级别 的停顿，远慢于一次 L1 load。
/// 在 x86 上，读 CR3/SATP 并不便宜：它强制流水线重启，开销常等于几十条普通指令；在高并发内核里这比一次完整页表遍历还重。
/// RISC-V csrr a0, satp 被 micro-arch 文档标注为“serialize commit phase”——代价与 x86 CR3 级别相仿 。
/// AArch64 AT/DSB ISH 组合仍比直接切 TTBR0_EL1 快，因为后者同样需要流水线清空并刷新 TLB。
pub fn register_v2p(va: Vaddr) -> Option<Paddr> {
    use crate::{
        arch::mm::{current_page_table_paddr, PageTableEntry, PagingConsts},
        mm::page_table::page_walk,
    };

    let root = current_page_table_paddr();
    // SAFETY: root 取自硬件寄存器，一定指向有效的顶级页表。
    unsafe { page_walk::<PageTableEntry, PagingConsts>(root, va) }.map(|(pa, _)| pa)
}

/// 线性直映：常数时间
#[inline(always)]
pub fn linear_v2p(va: Vaddr) -> Option<Paddr> {
    if LINEAR_MAPPING_VADDR_RANGE.contains(&va) {
        Some(va - LINEAR_MAPPING_BASE_VADDR)
    } else {
        None
    }
}

/// 通用页表遍历（root 可变）
#[inline(always)]
unsafe fn page_table_v2p(root_pa: Paddr, va: Vaddr) -> Option<Paddr> {
    page_walk::<PageTableEntry, PagingConsts>(root_pa, va).map(|(pa, _flags)| pa)
}

/// 仅针对高半区（非线性部分）
#[inline(always)]
pub fn kspace_v2p(va: Vaddr) -> Option<Paddr> {
    // 先试 O(1) 直映
    if let Some(pa) = linear_v2p(va) {
        return Some(pa);
    }
    // 其余走共享内核页表
    unsafe { page_table_v2p(KERNEL_PAGE_TABLE.get().unwrap().root_paddr(), va) }
}

/// 当前地址空间（可能是用户，也可能是内核 idle）
#[inline(always)]
pub fn current_v2p(va: Vaddr) -> Option<Paddr> {
    // 如果没有激活进程，则沿用内核页表
    let ptr = ACTIVATED_VM_SPACE.load();
    if ptr.is_null() {
        return kspace_v2p(va);
    }
    unsafe { &*ptr }.vaddr_to_paddr(va)
}

/// 将 VA 转换为 PA
#[inline(always)]
pub fn vaddr_to_paddr(va: Vaddr) -> Option<Paddr> {
    // 快速判定：若落在高半区直接调用 kspace_v2p
    if va >= KERNEL_VADDR_RANGE.start {
        kspace_v2p(va)
    } else {
        current_v2p(va)
    }
}
