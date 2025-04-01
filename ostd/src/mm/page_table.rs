// SPDX-许可证标识符: MPL-2.0

use core::{fmt::Debug, marker::PhantomData, ops::Range};

use ostd_pod::Pod;

use super::{
    nr_subpage_per_huge, page_prop::PageProperty, page_size, Paddr, PagingConstsTrait, PagingLevel,
    Vaddr,
};
use crate::{
    arch::mm::{PageTableEntry, PagingConsts},
    mm::io::PodOnce,
};

mod node;
use node::*;
pub mod cursor;
pub use cursor::{Cursor, CursorMut, PageTableItem};
#[cfg(ktest)]
mod test;

pub(crate) mod boot_pt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageTableError {
    /// 提供的虚拟地址范围无效。
    InvalidVaddrRange(Vaddr, Vaddr),
    /// 提供的虚拟地址无效。
    InvalidVaddr(Vaddr),
    /// 使用的虚拟地址未对齐。
    UnalignedVaddr,
}

/// 这是一种编译时技术，用于强制开发者区分内核全局页表实例、进程专用的用户页表实例以及设备页表实例。
pub trait PageTableMode: Clone + Debug + 'static {
    /// 页表可管理的虚拟地址范围。
    const VADDR_RANGE: Range<Vaddr>;

    /// 检查给定范围是否在有效虚拟地址范围内。
    fn covers(r: &Range<Vaddr>) -> bool {
        Self::VADDR_RANGE.start <= r.start && r.end <= Self::VADDR_RANGE.end
    }
}

#[derive(Clone, Debug)]
pub struct UserMode {}

impl PageTableMode for UserMode {
    const VADDR_RANGE: Range<Vaddr> = 0..super::MAX_USERSPACE_VADDR;
}

#[derive(Clone, Debug)]
pub struct KernelMode {}

impl PageTableMode for KernelMode {
    const VADDR_RANGE: Range<Vaddr> = super::KERNEL_VADDR_RANGE;
}

// 以下是由分页常量决定的一些常数。

/// 计算在一个大页（或页框）中，用于索引页表项的位数。
/// 该值是大页内页表项总数的二进制表示所需的位数，用于从虚拟地址中提取页内索引字段。
const fn nr_pte_index_bits<C: PagingConstsTrait>() -> usize {
    // 取大页中页表项数量的二进制对数，即可确定所需的位数。
    nr_subpage_per_huge::<C>().ilog2() as usize
}

/// 根据给定的虚拟地址和页表层级计算对应的页表项索引。
///
/// 参数说明:
///   va：待查询映射的虚拟地址。
///   level：当前所在的页表层级（层级1表示最低层，即叶子节点）。
///
/// 计算过程:
///   1. 首先，通过 C::BASE_PAGE_SIZE.ilog2() 计算基本页大小对应的位数（即页内偏移位数）。
///   2. 再加上 (level - 1) 倍的 nr_pte_index_bits，得到需要从虚拟地址中右移的总位数。
///   3. 将虚拟地址右移该位数后，使用 (nr_subpage_per_huge::<C>() - 1) 作为掩码提取当前页节点中的索引。
const fn pte_index<C: PagingConstsTrait>(va: Vaddr, level: PagingLevel) -> usize {
    va >> (C::BASE_PAGE_SIZE.ilog2() as usize + nr_pte_index_bits::<C>() * (level as usize - 1))
        & (nr_subpage_per_huge::<C>() - 1)
}

/// 页表的句柄。
/// 页表可以追踪映射物理页的生命周期。
#[derive(Debug)]
pub struct PageTable<
    Mode: PageTableMode,
    EntryType: PageTableEntryTrait = PageTableEntry,
    PagingConst: PagingConstsTrait = PagingConsts,
> where
    [(); PagingConst::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<PagingConst>()]:,
{
    root: RawPageTableNode<EntryType, PagingConst>,
    _phantom: PhantomData<Mode>,
}

impl PageTable<UserMode> {
    pub fn activate(&self) {
        // 安全性: 由于内核映射被共享，激活用户模式页表是安全的。
        unsafe {
            self.root.activate();
        }
    }

    /// 清空页表。
    ///
    /// # 安全性
    ///
    /// 调用者必须确保：
    ///  1. 没有其他光标正在访问该页表。
    ///  2. 没有其他CPU激活该页表。
    pub(crate) unsafe fn clear(&self) {
        let mut root_node = self.root.lock();
        const NR_PTES_PER_NODE: usize = nr_subpage_per_huge::<PagingConsts>();
        for i in 0..NR_PTES_PER_NODE / 2 {
            let root_entry = root_node.entry(i);
            if !root_entry.is_none() {
                let old = root_entry.replace(Child::None);
                // 因为没有其他人访问旧的子节点，所以释放它是安全的。
                // TODO: 看一下怎么释放子节点。
                drop(old);
            }
        }
    }
}

impl PageTable<KernelMode> {
    /// 创建一个新的用户页表。
    ///
    /// 这应该是创建用户页表的唯一方式，即复制内核页表并共享所有内核映射。
    pub fn create_user_page_table(&self) -> PageTable<UserMode> {
        let mut root_node = self.root.lock();
        let mut new_node = RawPageTableNode::alloc_empty_pt(
            PagingConsts::NR_LEVELS,
            MapTrackingStatus::NotApplicable,
        );

        // 在内核空间范围内进行浅拷贝根节点。
        // 用户空间范围不会被拷贝。
        const NR_PTES_PER_NODE: usize = nr_subpage_per_huge::<PagingConsts>();
        for i in NR_PTES_PER_NODE / 2..NR_PTES_PER_NODE {
            let root_entry = root_node.entry(i);
            if !root_entry.is_none() {
                let _ = new_node.entry(i).replace(root_entry.to_owned());
            }
        }

        PageTable::<UserMode> {
            root: new_node.into_raw(),
            _phantom: PhantomData,
        }
    }

    /// 显式地使虚拟地址范围在内核和用户页表之间共享。
    /// 在生成用户页表之前映射的页也将被共享。
    /// 虚拟地址范围应与根级页大小对齐。考虑到usize溢出，调用者应提供根级页的索引范围，而不是虚拟地址范围。
    pub fn make_shared_tables(&self, root_index: Range<usize>) {
        const NR_PTES_PER_NODE: usize = nr_subpage_per_huge::<PagingConsts>();

        let start = root_index.start;
        debug_assert!(start >= NR_PTES_PER_NODE / 2);
        debug_assert!(start < NR_PTES_PER_NODE);

        let end = root_index.end;
        debug_assert!(end <= NR_PTES_PER_NODE);

        let mut root_node = self.root.lock();
        for i in start..end {
            let root_entry = root_node.entry(i);
            if root_entry.is_none() {
                let nxt_level = PagingConsts::NR_LEVELS - 1;
                let is_tracked = if super::kspace::should_map_as_tracked(
                    i * page_size::<PagingConsts>(nxt_level),
                ) {
                    MapTrackingStatus::Tracked
                } else {
                    MapTrackingStatus::Untracked
                };
                let node = RawPageTableNode::alloc_empty_pt(nxt_level, is_tracked);
                let _ = root_entry.replace(Child::PageTable(node.into_raw()));
            }
        }
    }

    /// 保护内核页表中给定的虚拟地址范围。
    ///
    /// 此方法在保护时会刷新TLB条目。
    ///
    /// # 安全性
    ///
    /// 调用者必须确保保护操作不会影响内核的内存安全。
    pub unsafe fn protect_flush_tlb(
        &self,
        vaddr: &Range<Vaddr>,
        mut op: impl FnMut(&mut PageProperty),
    ) -> Result<(), PageTableError> {
        let mut cursor = CursorMut::new(self, vaddr)?;
        while let Some(range) = cursor.protect_next(vaddr.end - cursor.virt_addr(), &mut op) {
            crate::arch::mm::tlb_flush_addr(range.start);
        }
        Ok(())
    }
}

impl<'a, M: PageTableMode, E: PageTableEntryTrait, C: PagingConstsTrait> PageTable<M, E, C>
where
    [(); C::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<C>()]:,
{
    /// 创建一个新的空页表。仅适用于内核页表和IOMMU页表。
    pub fn empty() -> Self {
        PageTable {
            root: RawPageTableNode::alloc_empty_pt(C::NR_LEVELS, MapTrackingStatus::NotApplicable),
            _phantom: PhantomData,
        }
    }

    pub(crate) unsafe fn first_activate_unchecked(&self) {
        self.root.first_activate();
    }

    /// 根页表的物理地址。
    ///
    /// 直接将根页表的物理地址提供给硬件是危险的，因为页表节点可能会被丢弃，导致使用后释放 (UAF)。
    pub unsafe fn root_paddr(&self) -> Paddr {
        self.root.paddr()
    }

    pub unsafe fn map(
        &self,
        vaddr: &Range<Vaddr>,
        paddr: &Range<Paddr>,
        prop: PageProperty,
    ) -> Result<(), PageTableError> {
        self.cursor_mut(vaddr)?.map_pa(paddr, prop);
        Ok(())
    }

    /// 查询给定虚拟地址处单个字节的映射情况。
    ///
    /// 请注意，如果有多个光标同时访问同一虚拟地址范围，此函数可能无法反映出准确的结果，就像硬件MMU遍历时那样。
    #[cfg(ktest)]
    pub fn query(&self, vaddr: Vaddr) -> Option<(Paddr, PageProperty)> {
        // 安全性: 根节点是一个有效的页表节点，因此该地址是有效的。
        unsafe { page_walk::<E, C>(self.root_paddr(), vaddr) }
    }

    /// 创建一个新的光标，独占访问指定的用于映射的虚拟地址范围。
    ///
    /// 如果已有其他光标正在访问该范围，新光标可能会等待直到前一个光标被销毁。
    pub fn cursor_mut(
        &'a self,
        va: &Range<Vaddr>,
    ) -> Result<CursorMut<'a, M, E, C>, PageTableError> {
        CursorMut::new(self, va)
    }

    /// 创建一个新的光标，独占访问指定的用于查询的虚拟地址范围。
    ///
    /// 如果已有其他光标正在访问该范围，新光标可能会等待直到前一个光标被销毁。
    /// 光标对映射的修改也可能被其他光标的映射操作阻塞或覆盖。
    pub fn cursor(&'a self, va: &Range<Vaddr>) -> Result<Cursor<'a, M, E, C>, PageTableError> {
        Cursor::new(self, va)
    }

    /// 创建对同一页表的新引用。
    /// 调用者必须确保内核页表不会被复制。
    /// 这仅对IOMMU页表有用，在其他情况下请三思使用。
    pub unsafe fn shallow_copy(&self) -> Self {
        PageTable {
            root: self.root.clone_shallow(),
            _phantom: PhantomData,
        }
    }
}

/// 软件模拟的MMU地址转换过程。
/// 如果给定的虚拟地址存在有效映射，则返回对应的物理地址及其映射信息。
///
/// # 安全性
///
/// 调用者必须确保root_paddr是指向根页表节点的有效指针。
///
/// # 关于页表节点释放、重用后再读问题的说明
///
/// 由于硬件MMU和软件页表遍历方法在读取时均不会锁定页表，
/// 它们可能读取到已被回收重用的页表节点中的页表项。
///
/// 为了缓解此问题，页表节点默认不会被主动回收，直到找到适当的解决方案。
#[cfg(ktest)]
pub(super) unsafe fn page_walk<E: PageTableEntryTrait, C: PagingConstsTrait>(
    root_paddr: Paddr,
    vaddr: Vaddr,
) -> Option<(Paddr, PageProperty)> {
    use crate::mm::kspace::paddr_to_vaddr;

    let _guard = crate::trap::disable_local();

    let mut cur_level = C::NR_LEVELS;
    let mut cur_pte = {
        let node_addr = paddr_to_vaddr(root_paddr);
        let offset = pte_index::<C>(vaddr, cur_level);
        // 安全性: 偏移量不会超过 PAGE_SIZE 的大小。
        unsafe { (node_addr as *const E).add(offset).read() }
    };

    while cur_level > 1 {
        if !cur_pte.is_present() {
            return None;
        }

        if cur_pte.is_last(cur_level) {
            debug_assert!(cur_level <= C::HIGHEST_TRANSLATION_LEVEL);
            break;
        }

        cur_level -= 1;
        cur_pte = {
            let node_addr = paddr_to_vaddr(cur_pte.paddr());
            let offset = pte_index::<C>(vaddr, cur_level);
            // 安全性: 偏移量不会超过 PAGE_SIZE 的大小。
            unsafe { (node_addr as *const E).add(offset).read() }
        };
    }

    if cur_pte.is_present() {
        Some((
            cur_pte.paddr() + (vaddr & (page_size::<C>(cur_level) - 1)),
            cur_pte.prop(),
        ))
    } else {
        None
    }
}

/// 定义架构特定页表项的接口。
///
/// 注意：默认的PTE应为空映射，不指向任何对象。
pub trait PageTableEntryTrait:
    Clone + Copy + Debug + Default + Pod + PodOnce + Sized + Send + Sync + 'static
{
    /// 创建一组新的无效页表标志，表示空页。
    ///
    /// 注意：当前实现要求全零PTE表示空页。
    fn new_absent() -> Self {
        Self::default()
    }

    /// 如果标志显示存在有效映射则返回true。
    ///
    /// 对于由 [`Self::new_absent`] 创建的PTE，该方法应返回false；
    /// 而对于通过 [`Self::new_page`] 或 [`Self::new_pt`] 创建并经 [`Self::set_prop`] 修改的PTE，该方法应返回true。
    fn is_present(&self) -> bool;

    /// 创建一个新的PTE，使用给定的物理地址和标志来映射一个页面。
    fn new_page(paddr: Paddr, level: PagingLevel, prop: PageProperty) -> Self;

    /// 创建一个映射到子页表的PTE。
    fn new_pt(paddr: Paddr) -> Self;

    /// 从PTE中获取物理地址。
    /// PTE中记录的物理地址可能是：
    /// - 下一级页表的物理地址；
    /// - 或映射页的物理地址。
    fn paddr(&self) -> Paddr;

    fn prop(&self) -> PageProperty;

    /// 设置PTE的页面属性。
    ///
    /// 仅当PTE存在时才会执行此操作，否则该方法不进行任何操作。
    fn set_prop(&mut self, prop: PageProperty);

    /// 如果PTE映射的是页面而非子页表，则返回true。
    ///
    /// 给出了条目所在的页表级别，因为如amd64这类架构在中间级仅使用大页标志。
    fn is_last(&self, level: PagingLevel) -> bool;
}
