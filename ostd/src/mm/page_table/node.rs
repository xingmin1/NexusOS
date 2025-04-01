// SPDX-License-Identifier: MPL-2.0

//! 本模块定义了页表节点的抽象和句柄。
//!
//! 在许多架构文档中，页表节点通常也被称为页表。它本质上是一个包含页表项（PTE）的页面，这些页表项映射到子页表节点或所映射的页面。
//!
//! 本模块利用页面元数据来管理页表页面，从而更容易做到以下保证：
//!   - 当页表节点仍被父级页表节点、
//!   - 或页表节点的句柄、
//!   - 或处理器所使用时，
//! 不会被释放。
//!
//! 这一机制通过在页面元数据中使用引用计数实现。如果上述条件不满足，则在丢弃最后一个引用时，该页表节点及其子节点都会被释放。
//!
//! 仅通过页表节点的物理地址就可获得其独占访问权，这通过页面元数据中的锁实现。该独占性仅对内核代码有效，而处理器的MMU在锁持有期间仍能访问页表节点。
//! 因此，对PTE的修改应在其指向的实体初始化之后进行。本模块负责处理这一点。

mod child;
mod entry;

use alloc::sync::Arc;
use core::{
    cell::SyncUnsafeCell,
    marker::PhantomData,
    mem::ManuallyDrop,
    sync::atomic::{AtomicU8, Ordering},
};

pub(crate) use self::{child::Child, entry::Entry};
use super::{nr_subpage_per_huge, PageTableEntryTrait};
use crate::{
    arch::mm::{PageTableEntry, PagingConsts},
    mm::{
        frame::{allocator::FrameAllocOptions, inc_frame_ref_count, Frame},
        kspace::paddr_to_vaddr,
        Paddr, PagingConstsTrait, PagingLevel,
    },
};

/// 页表节点的原始句柄。
///
/// 该句柄用于引用页表节点，其创建和销毁会影响页表节点的引用计数。如果该原始句柄是最后一个引用，
/// 则在丢弃时页表节点及其所有子节点会被释放。
///
/// 只有 CPU 或 PTE 可以使用原始句柄访问页表节点；若在内核代码中访问页表节点，请使用 [`PageTableNode`] 句柄。
#[derive(Debug)]
pub(super) struct RawPageTableNode<EntryType: PageTableEntryTrait, PagingConsts: PagingConstsTrait>
where
    [(); PagingConsts::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<PagingConsts>()]:,
{
    // raw: Paddr,
    // level: PagingLevel,
    table: Arc<
        SyncUnsafeCell<[Child<EntryType, PagingConsts>; nr_subpage_per_huge::<PagingConsts>()]>,
    >,
    meta: Arc<PageTablePageMeta<EntryType, PagingConsts>>,
    // _phantom: PhantomData<(EntryType, PagingConsts)>,
}

impl<EntryType: PageTableEntryTrait, PagingConsts: PagingConstsTrait>
    RawPageTableNode<EntryType, PagingConsts>
where
    [(); PagingConsts::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<PagingConsts>()]:,
{
    pub(super) fn paddr(&self) -> Paddr {
        self.meta.paddr
    }

    pub(super) fn level(&self) -> PagingLevel {
        self.meta.level
    }

    /// 通过获取锁，将原始句柄转换为可访问的页表节点句柄。
    pub(super) fn lock(&self) -> PageTableNode<EntryType, PagingConsts> {
        let level = self.level();

        // 获取锁。
        let meta = &self.meta;
        while meta
            .lock
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }

        debug_assert_eq!(meta.level, level);

        PageTableNode::<EntryType, PagingConsts> { page: self }
    }

    /// 创建句柄的浅拷贝。
    pub(super) fn clone_shallow(&self) -> Self {
        self.inc_ref_count();
        // todo!();
        Self {
            table: self.table.clone(),
            meta: self.meta.clone(),
        }
    }

    /// 激活页表（前提是它是根页表）。
    ///
    /// 此方法通过使处理器成为页表的所有者来确保不会释放正在使用中的页表。激活时，
    /// 会将上个激活页表的引用计数减1，将当前页表的引用计数加1。
    ///
    /// # Safety
    ///
    /// 调用者必须确保待激活的页表拥有正确的内核映射，并且其常量参数与当前CPU匹配。
    ///
    /// # Panics
    ///
    /// 仅顶级页表才能使用此函数激活。
    pub(crate) unsafe fn activate(&self) {
        use crate::arch::mm::{activate_page_table, current_page_table_paddr};

        assert_eq!(self.level(), PagingConsts::NR_LEVELS);

        let last_activated_paddr = current_page_table_paddr();

        if last_activated_paddr == self.raw() {
            return;
        }

        activate_page_table(self.raw(), self.level());

        // 当前页表引用计数加1。
        self.inc_ref_count();

        // 恢复并丢弃上个激活的页表。
        // TODO: 使用一个CPU本地的变量来存储上一个激活的页表元数据。
        todo!()
    }

    /// 激活（根）页表，假定这是第一次激活。
    ///
    /// 此方法不会尝试丢弃上一个激活的页表，与 [`Self::activate()`] 在其他方面一致。
    pub(super) unsafe fn first_activate(&self) {
        use crate::arch::mm::activate_page_table;

        self.inc_ref_count();

        activate_page_table(self.raw(), self.level());
    }

    fn inc_ref_count(&self) {
        // 安全性说明：我们持有该页的引用计数，可安全地将其加1。
        unsafe {
            inc_frame_ref_count(self.paddr());
        }
    }

    pub(crate) fn get_child(&self, idx: usize) -> &Child<EntryType, PagingConsts> {
        &self.table[idx]
    }

    /// 分配一个新的空页表节点。
    ///
    /// 此函数返回一个原生句柄。为了性能，新创建的句柄不会设置锁位，因锁操作为独占且解锁开销较高。
    pub(super) fn alloc_empty_pt(level: PagingLevel, is_tracked: MapTrackingStatus) -> Self {
        let page = FrameAllocOptions::new()
            .zeroed(true)
            .alloc_frame()
            .expect("Failed to allocate a page table node");
        let page = ManuallyDrop::new(page);
        // 分配的帧已置零，确保零值代表无效的PTE。
        debug_assert!(EntryType::new_absent().as_bytes().iter().all(|&b| b == 0));

        let meta = PageTablePageMeta::new_unlocked(level, is_tracked, page.start_paddr());

        Self {
            table: Arc::new([Child::None; nr_subpage_per_huge::<PagingConsts>()]),
            meta: Arc::new(meta),
        }
    }

    /*     pub(super) fn split_if_untracked_huge(&mut self, idx: usize) -> Option<PageTableNode<'_, EntryType, PagingConsts>> {
        let cur_child = self.table.get_mut()[idx];
        if !matches!(cur_child, Child::Untracked(_, _, _)) {
            return None;
        }

        let
    } */

    // /// 根据物理地址和级别恢复句柄。
    // ///
    // /// # Safety
    // ///
    // /// 调用者必须确保该物理地址有效，且指向的页表节点已被遗忘。被遗忘的页表节点只能恢复一次，
    // /// 并且其级别必须与页表节点的级别一致。
    // pub(super) unsafe fn from_raw_parts(paddr: Paddr, level: PagingLevel) -> Self {
    //     Self {
    //         raw: paddr,
    //         level,
    //         _phantom: PhantomData,
    //     }
    // }
}

// impl<EntryType: PageTableEntryTrait, PagingConsts: PagingConstsTrait>
//     From<RawPageTableNode<EntryType, PagingConsts>>
//     for Frame<PageTablePageMeta<EntryType, PagingConsts>>
// where
//     [(); PagingConsts::NR_LEVELS as usize]:,
// {
//     fn from(raw: RawPageTableNode<EntryType, PagingConsts>) -> Self {
//         let raw = ManuallyDrop::new(raw);
//         // 安全性说明：原始句柄中的物理地址有效，且我们正在将所有权转移至新的句柄，无需增加引用计数。
//         unsafe { Frame::<PageTablePageMeta<EntryType, PagingConsts>>::from_raw(raw.paddr()) }
//     }
// }

impl<EntryType: PageTableEntryTrait, PagingConsts: PagingConstsTrait> Drop
    for RawPageTableNode<EntryType, PagingConsts>
where
    [(); PagingConsts::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<PagingConsts>()]:,
{
    fn drop(&mut self) {
        // 安全性说明：原始句柄中的物理地址有效，通过恢复句柄来减少引用计数。
        drop(unsafe {
            Frame::<PageTablePageMeta<EntryType, PagingConsts>>::from_raw(self.paddr())
        });
    }
}

/// 页表节点的可变句柄。
///
/// 该句柄可操作其子页表节点，并拥有它们的句柄，确保子节点不会在该页表节点之前销毁。克隆页表节点将创建其深拷贝；
/// 当页表节点无其他引用时，丢弃它也会丢弃所有子句柄。你还可以将该页表节点设为另一个页表节点的子节点。
#[derive(Debug)]
pub(super) struct PageTableNode<
    'a,
    EntryType: PageTableEntryTrait = PageTableEntry,
    Consts: PagingConstsTrait = PagingConsts,
> where
    [(); Consts::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<Consts>()]:,
{
    // page: Frame<PageTablePageMeta<EntryType, PagingConsts>>,
    page: &'a RawPageTableNode<EntryType, Consts>,
}

impl<'a, EntryType: PageTableEntryTrait, Consts: PagingConstsTrait>
    PageTableNode<'a, EntryType, Consts>
where
    [(); Consts::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<Consts>()]:,
{
    /// 借用节点中指定索引处的项。
    ///
    /// # Panics
    ///
    /// 若索引超出 [`nr_subpage_per_huge<PagingConsts>`] 范围，则会触发 panic。
    pub(super) fn entry(&mut self, idx: usize) -> Entry<'_, '_, EntryType, Consts> {
        assert!(idx < nr_subpage_per_huge::<Consts>());
        // 安全性说明：索引在有效范围内。
        unsafe { Entry::new_at(self, idx) }
    }

    /// 获取页表节点的级别。
    pub(super) fn level(&self) -> PagingLevel {
        self.page.level()
    }

    /// 获取页表节点的映射跟踪状态。
    pub(super) fn is_tracked(&self) -> MapTrackingStatus {
        self.page.is_tracked()
    }

    // /// 分配一个新的空页表节点。
    // ///
    // /// 此函数返回一个拥有所有权的句柄。为了性能，新创建的句柄不会设置锁位，因锁操作为独占且解锁开销较高。
    // pub(super) fn alloc(level: PagingLevel, is_tracked: MapTrackingStatus) -> Self {
    //     todo!("TODO: 移到上一级");
    //     // let page = FrameAllocOptions::new()
    //     //     .zeroed(true)
    //     //     .alloc_frame()
    //     //     .expect("Failed to allocate a page table node");
    //     // // 分配的帧已置零，确保零值代表无效的PTE。
    //     // debug_assert!(EntryType::new_absent().as_bytes().iter().all(|&b| b == 0));
    //
    //     // let meta = PageTablePageMeta::new_locked(level, is_tracked, page.start_paddr());
    //
    //     // Self { page }
    // }

    // /// 将句柄转换为原始句柄，以便存储于PTE或CPU中。
    // pub(super) fn into_raw(self) -> RawPageTableNode<EntryType, PagingConsts> {
    //     let this = ManuallyDrop::new(self);
    //
    //     // 释放锁。
    //     this.page.meta().lock.store(0, Ordering::Release);
    //
    //     // 安全性说明：提供的物理地址有效且级别正确，引用计数保持不变。
    //     unsafe { RawPageTableNode::from_raw_parts(this.page.start_paddr(), this.page.meta().level) }
    // }
    //
    // /// 在保留当前句柄的同时获取一个原始句柄副本。
    // pub(super) fn clone_raw(&self) -> RawPageTableNode<EntryType, PagingConsts> {
    //     let page = ManuallyDrop::new(self.page.clone());
    //
    //     // 安全性说明：提供的物理地址有效且级别正确，引用计数增加1。
    //     unsafe { RawPageTableNode::from_raw_parts(page.start_paddr(), page.meta().level) }
    // }

    /// 获取节点中有效PTE的数量。
    pub(super) fn nr_children(&self) -> u16 {
        // 安全性说明：因持有锁故拥有独占访问权。
        unsafe { *self.page.meta().nr_children.get() }
    }

    /// 读取给定索引处的非拥有性PTE。
    ///
    /// 非拥有性PTE表示如果其指向一个页面，则不会增加页面的引用计数，原PTE依旧拥有该子页面。
    ///
    /// # Safety
    ///
    /// 调用者必须确保索引在有效范围内。
    unsafe fn read_pte(&self, idx: usize) -> EntryType {
        debug_assert!(idx < nr_subpage_per_huge::<PagingConsts>());
        let ptr = paddr_to_vaddr(self.page.start_paddr()) as *const EntryType;
        // 安全性说明：索引在有效范围内，且PTE为普通数据。
        unsafe { ptr.add(idx).read() }
    }

    /// 在指定索引处写入页表项。
    ///
    /// 此操作会导致旧的子节点泄漏（若旧PTE存在）。
    ///
    /// 该PTE所代表的子节点所有权将转移至此节点，且操作后该PTE失效。
    ///
    /// # Safety
    ///
    /// 调用者必须确保：
    ///  1. 索引在有效范围内；
    ///  2. PTE所代表的子节点与此页表节点兼容（参见 [`Child::is_compatible`]）。
    unsafe fn write_pte(&mut self, idx: usize, pte: EntryType) {
        debug_assert!(idx < nr_subpage_per_huge::<PagingConsts>());
        let ptr = paddr_to_vaddr(self.page.start_paddr()) as *mut EntryType;
        // 安全性说明：索引在有效范围内，且PTE为普通数据。
        unsafe { ptr.add(idx).write(pte) }
    }

    /// 获取节点中有效PTE数量的可变引用。
    fn nr_children_mut(&mut self) -> &mut u16 {
        // 安全性说明：因持有锁故具有独占访问权。
        unsafe { &mut *self.page.meta().nr_children.get() }
    }
}

impl<EntryType: PageTableEntryTrait, PagingConsts: PagingConstsTrait> Drop
    for PageTableNode<'_, EntryType, PagingConsts>
where
    [(); PagingConsts::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<PagingConsts>()]:,
{
    fn drop(&mut self) {
        // 释放锁。
        self.page.meta().lock.store(0, Ordering::Release);
    }
}

/// 各类页表页面的元数据。
/// 注意：泛型参数不会影响内存布局。
#[derive(Debug)]
pub(crate) struct PageTablePageMeta<
    EntryType: PageTableEntryTrait = PageTableEntry,
    Consts: PagingConstsTrait = PagingConsts,
> {
    /// 有效PTE的数量；当持有锁时，该值可修改。
    pub nr_children: SyncUnsafeCell<u16>,
    /// 页表页面的级别，一个页表页面不能被不同级别的页表引用。
    pub level: PagingLevel,
    /// 页表页面的锁。
    pub lock: AtomicU8,
    /// 节点映射的页面是否被跟踪。
    pub is_tracked: MapTrackingStatus,
    /// 页表节点的物理地址。
    pub paddr: Paddr,
    _phantom: core::marker::PhantomData<(EntryType, Consts)>,
}

/// 描述页表中记录的物理地址对应页面是否由元数据跟踪。
///
/// 此枚举是Asterinas操作系统内存管理的核心部分，用于确定当页表节点被销毁时，
/// 如何处理其引用的物理页面。不同的跟踪状态对应不同的内存管理策略，
/// 使系统能够同时处理普通内存和特殊内存（如设备映射内存）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum MapTrackingStatus {
    /// 此页表节点不能包含指向任何物理页面的直接映射，只能包含指向子页表节点的引用。
    ///
    /// 典型用例:
    /// - 顶层页目录（如PGD、PUD）通常为此状态，因为它们只包含指向下一级页表的指针
    /// - 四级页表架构中的顶级页目录表（PML4表）
    /// - 仅用于间接映射的中间级页表
    NotApplicable,

    /// 所映射的物理页面不由内核元数据系统跟踪，在页表释放时不会自动释放这些页面。
    /// 若存在子页表节点，其所指向的页面仍应根据子节点的跟踪状态处理。
    ///
    /// 典型用例:
    /// - 直接映射的设备内存（如显存、网卡缓冲区）
    /// - 通过IOMMU映射的DMA缓冲区
    /// - ROM/固件区域映射
    /// - 使用物理地址直接映射的内存区域（如通过map_pa方法）
    Untracked,

    /// 所映射的物理页面由内核元数据系统完全跟踪，页表释放时会根据引用计数决定是否释放页面。
    /// 若存在子页表节点，其所指向的页面也应被跟踪。
    ///
    /// 典型用例:
    /// - 用户空间应用程序的内存页面
    /// - 内核堆分配的内存
    /// - 通过普通内存分配器获取的Frame对象
    /// - 需要完整生命周期管理的动态分配内存
    Tracked,
}

impl<EntryType: PageTableEntryTrait, PagingConsts: PagingConstsTrait>
    PageTablePageMeta<EntryType, PagingConsts>
{
    pub fn new_locked(level: PagingLevel, is_tracked: MapTrackingStatus, paddr: Paddr) -> Self {
        Self {
            nr_children: SyncUnsafeCell::new(0),
            level,
            lock: AtomicU8::new(0),
            is_tracked,
            paddr,
            _phantom: PhantomData,
        }
    }
}

// // 安全性说明：`PageTablePageMeta` 的内存布局在所有泛型参数下保持一致，并满足相关要求。
// impl<EntryType: PageTableEntryTrait, PagingConsts: PagingConstsTrait> AnyFrameMeta for PageTablePageMeta<EntryType, PagingConsts>
// where
//     [(); PagingConsts::NR_LEVELS as usize]:,
// {
//     fn on_drop(&mut self, reader: &mut VmReader<Infallible>) {
//         let nr_children = self.nr_children.get_mut();

//         if (*nr_children) == 0 {
//             return;
//         }

//         let level = self.level;
//         let is_tracked = self.is_tracked;

//         // 丢弃所有子节点。
//         while let Ok(pte) = reader.read_once::<EntryType>() {
//             // 若直接使用 `Child::from_pte`，会使 `drop` 函数的开销增加约50%。Rust对内联和优化不安全代码中的无用代码较为保守，
//             // 因此此处我们手动将该逻辑内联。
//             if pte.is_present() {
//                 let paddr = pte.paddr();
//                 if !pte.is_last(level) {
//                     // 安全性说明：该PTE指向一个页表节点，子节点的所有权转移后立即释放。
//                     // 无论跟踪状态如何，页表节点都需要被释放
//                     drop(unsafe { Frame::<Self>::from_raw(paddr) });
//                 } else if is_tracked == MapTrackingStatus::Tracked {
//                     // 安全性说明：该PTE指向一个受跟踪的页面，子页面的所有权转移后立即释放。
//                     // 只有被标记为Tracked的页面才会被释放，Untracked页面由外部管理其生命周期
//                     drop(unsafe { Frame::<dyn AnyFrameMeta>::from_raw(paddr) });
//                 }
//                 // 注意：当is_tracked为Untracked时，我们不会释放最终级页面，因为它们可能是
//                 // 设备内存或其他特殊内存，其生命周期不由页表管理
//             }
//         }
//     }
// }
