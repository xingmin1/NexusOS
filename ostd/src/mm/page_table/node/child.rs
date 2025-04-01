// SPDX-License-Identifier: MPL-2.0

//! 本模块定义了页表节点子项（Child）的类型。

use core::mem::ManuallyDrop;

use super::{MapTrackingStatus, PageTableEntryTrait, RawPageTableNode};
use crate::{
    arch::mm::{PageTableEntry, PagingConsts},
    mm::{
        frame::{Frame, MemoryType, Unknown},
        nr_subpage_per_huge,
        page_prop::PageProperty,
        Paddr, PagingConstsTrait, PagingLevel,
    },
};

/// 页表节点的子项。
///
/// 这是一个拥有页表节点子项所有权的句柄。如果子项是页表节点或页面，
/// 则该句柄会持有相应页面的引用计数。
#[derive(Debug)]
pub(crate) enum Child<
    EntryType: PageTableEntryTrait = PageTableEntry,
    PagingConst: PagingConstsTrait = PagingConsts,
> where
    [(); PagingConst::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<PagingConst>()]:,
{
    PageTable(RawPageTableNode<EntryType, PagingConst>),
    Frame(Frame<Unknown>, PageProperty),
    /// 未由句柄追踪的页面。
    Untracked(Paddr, PagingLevel, PageProperty),
    None,
}

impl<EntryType: PageTableEntryTrait, PagingConst: PagingConstsTrait> Child<EntryType, PagingConst>
where
    [(); PagingConst::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<PagingConst>()]:,
{
    /// 判断子项是否为空（即不映射任何有效内容）。
    pub(crate) fn is_none(&self) -> bool {
        matches!(self, Child::None)
    }

    /// 判断该子项是否与指定的节点兼容。
    ///
    /// 换句话说，该方法检查子项是否可以作为层级为 node_level 且跟踪状态为 is_tracked 的节点的子项。
    pub(super) fn is_compatible(
        &self,
        node_level: PagingLevel,
        is_tracked: MapTrackingStatus,
    ) -> bool {
        match self {
            Child::PageTable(pt) => node_level == pt.level() + 1,
            Child::Frame(p, _) => {
                node_level == p.level() && is_tracked == MapTrackingStatus::Tracked
            }
            Child::Untracked(_, level, _) => {
                node_level == *level && is_tracked == MapTrackingStatus::Untracked
            }
            Child::None => true,
        }
    }

    /// 借出子项的的页表项（PTE）。
    pub(super) fn get_entry(&self) -> EntryType {
        match self {
            Child::PageTable(pt) => EntryType::new_pt(pt.paddr()),
            Child::Frame(page, prop) => {
                let level = page.level();
                EntryType::new_page(page.start_paddr(), level, *prop)
            }
            Child::Untracked(pa, level, prop) => EntryType::new_page(*pa, *level, *prop),
            Child::None => EntryType::new_absent(),
        }
    }

    // /// 将页表项（PTE）还原为对应的子项。
    // ///
    // /// # 安全性说明
    // ///
    // /// 提供的 PTE 必须源自 [`Child::get_entry`] 的转换，
    // /// 并且提供的层级及跟踪状态信息必须与转换时所丢失的信息一致
    // /// （严格来说，这些参数必须与原始子项兼容，详见 [`Child::is_compatible`]）。
    // ///
    // /// 对于同一个通过 [`Child::get_entry`] 得到的 PTE，此方法只能调用一次。
    // pub(super) unsafe fn from_pte(
    //     pte: EntryType,
    //     level: PagingLevel,
    //     is_tracked: MapTrackingStatus,
    // ) -> Self {
    //     if !pte.is_present() {
    //         return Child::None;
    //     }
    //
    //     let paddr = pte.paddr();
    //
    //     if !pte.is_last(level) {
    //         // 安全性说明：该物理地址指向指定层级上有效的页表节点。
    //         return Child::PageTable(unsafe { RawPageTableNode::from_raw_parts(paddr, level - 1) });
    //     }
    //
    //     match is_tracked {
    //         MapTrackingStatus::Tracked => {
    //             // 安全性说明：该物理地址指向一个有效页面。
    //             let page = unsafe { Frame::<dyn MemoryType>::from_raw(paddr) };
    //             Child::Frame(page, pte.prop())
    //         }
    //         MapTrackingStatus::Untracked => Child::Untracked(paddr, level, pte.prop()),
    //         MapTrackingStatus::NotApplicable => panic!("Invalid tracking status"),
    //     }
    // }
    //
    // /// 为子项增加额外的所有权引用。
    // ///
    // /// # 安全性
    // ///
    // /// 提供的 PTE 必须源自 [`Child::get_entry`]（与 [`Child::from_pte`] 的要求一致），
    // /// 且不得对已通过 [`Child::from_pte`] 恢复的 PTE 使用此方法。
    // pub(super) unsafe fn clone_from_pte(
    //     pte: &EntryType,
    //     level: PagingLevel,
    //     is_tracked: MapTrackingStatus,
    // ) -> Self {
    //     if !pte.is_present() {
    //         return Child::None;
    //     }
    //
    //     let paddr = pte.paddr();
    //
    //     if !pte.is_last(level) {
    //         // 安全性说明：该物理地址有效，且 PTE 已持有该页面的引用计数。
    //         unsafe { inc_frame_ref_count(paddr) };
    //         // 安全性说明：该物理地址指向指定层级上有效的页表节点。
    //         return Child::PageTable(unsafe { RawPageTableNode::from_raw_parts(paddr, level - 1) });
    //     }
    //
    //     match is_tracked {
    //         MapTrackingStatus::Tracked => {
    //             // 安全性说明：该物理地址有效，且 PTE 已持有该页面的引用计数。
    //             unsafe { inc_frame_ref_count(paddr) };
    //             // 安全性说明：该物理地址指向一个有效页面。
    //             let page = unsafe { Frame::<dyn MemoryType>::from_raw(paddr) };
    //             Child::Frame(page, pte.prop())
    //         }
    //         MapTrackingStatus::Untracked => Child::Untracked(paddr, level, pte.prop()),
    //         MapTrackingStatus::NotApplicable => panic!("Invalid tracking status"),
    //     }
    // }
}
