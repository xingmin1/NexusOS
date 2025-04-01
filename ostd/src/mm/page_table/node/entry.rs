// SPDX-License-Identifier: MPL-2.0

//! 本模块提供了操作页表节点中各个条目的接口。

use super::{Child, MapTrackingStatus, PageTableEntryTrait, PageTableNode, RawPageTableNode};
use crate::mm::{
    nr_subpage_per_huge, page_prop::PageProperty, page_size, PagingConstsTrait, PagingLevel,
};

/// 页表节点中单个条目的视图。
///
/// 可通过 [`PageTableNode::entry`] 方法从节点中借用获取此视图。
/// 此结构为页表节点中某条目的静态引用，不涉及对子对象的动态引用计数，
/// 可用于生成具有所有权的 [`Child`] 句柄。
pub(crate) struct Entry<'a, 'b, EntryType: PageTableEntryTrait, PagingConsts: PagingConstsTrait>
where
    [(); PagingConsts::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<PagingConsts>()]:,
{
    /// 页表项数据。
    ///
    /// 将页表项缓存在此处以减少对节点的重复读取。
    /// 不能直接持有 `&mut EntryType` 引用，因为其他 CPU 可能会修改该内存（访问位/脏页位），
    /// 这可能违反 Rust 的别名规则，导致未定义行为。
    pte: EntryType,
    /// 条目在节点中的索引。
    idx: usize,
    /// 包含该条目的页表节点。
    node: &'a mut PageTableNode<'b, EntryType, PagingConsts>,
}

impl<'a, 'b, EntryType: PageTableEntryTrait, PagingConsts: PagingConstsTrait>
    Entry<'a, 'b, EntryType, PagingConsts>
where
    [(); PagingConsts::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<PagingConsts>()]:,
{
    /// 判断该条目是否没有映射任何内容。
    pub(crate) fn is_none(&self) -> bool {
        !self.pte.is_present()
    }

    /// 判断该条目是否映射到另一个页表节点。
    pub(crate) fn is_node(&self) -> bool {
        self.pte.is_present() && !self.pte.is_last(self.node.level())
    }

    /// 获取子对象的拥有权句柄。
    pub(crate) fn to_owned(&self) -> Child<EntryType, PagingConsts> {
        // 安全性：该条目表示一个有效条目，并且包含正确的节点信息。
        // unsafe { Child::clone_from_pte(&self, self.node.level(), self.node.is_tracked()) }
        let child = &self.node.page.table[self.idx];
        match child {
            Child::PageTable(raw) => Child::PageTable(raw.clone_shallow()),
            Child::Frame(frame, page_property) => Child::Frame(frame.clone(), *page_property),
            Child::Untracked(paddr, level, page_property) => {
                Child::Untracked(*paddr, *level, *page_property)
            }
            Child::None => Child::None,
        }
    }

    /// 修改条目的映射属性。
    ///
    /// 只有在条目映射有效时才会更新属性。
    pub(crate) fn protect(&mut self, op: &mut impl FnMut(&mut PageProperty)) {
        if !self.pte.is_present() {
            return;
        }

        let prop = self.pte.prop();
        let mut new_prop = prop;
        op(&mut new_prop);

        if prop == new_prop {
            return;
        }

        self.pte.set_prop(new_prop);

        // 安全性说明：
        //  1. 索引位于有效范围内。
        //  2. 我们仅修改了页表项的映射属性，保持与当前节点的兼容性。
        unsafe { self.node.write_pte(self.idx, self.pte) };
    }

    /// 使用新子节点替换当前条目，并返回旧的子节点。
    ///
    /// # Panics
    ///
    /// 如果传入的子节点与节点不兼容，则该方法会 panic，
    /// 兼容性由 [`Child::is_compatible`] 方法判断。
    pub(crate) fn replace(
        self,
        new_child: Child<EntryType, PagingConsts>,
    ) -> Child<EntryType, PagingConsts> {
        assert!(new_child.is_compatible(self.node.level(), self.node.is_tracked()));

        // 安全性：该条目表示一个有效条目，其节点信息正确。旧的页表项将被新子对象覆盖，不再使用。
        // let old_child =
        //     unsafe { Child::from_pte(self.pte, self.node.level(), self.node.is_tracked()) };

        // 安全性：
        //  1. 索引位于有效范围内。
        //  2. 新页表项经过了本函数开头的assert检查，与当前页表节点匹配。
        unsafe { self.node.write_pte(self.idx, new_child.get_entry()) };

        let old_child =
            // 安全性
            //  1. 由于获取 `Entry` 时已经lock了，所以对于此 `Entry` 的操作是独占的。
            //  2. 由于创建时已经保证索引位于有效范围内，且从来没有修改过索引的值，所以索引仍然有效。
            //     因此，目标地址是对齐的。
            //  3. 由于 `RawPageTableNode::table` 在创建时已经正确初始化了，所以目标地址对 `Child` 类型是有效的。
            unsafe { core::ptr::replace(&mut self.node.page.table[self.idx], new_child) };

        if old_child.is_none() && !new_child.is_none() {
            *self.node.nr_children_mut() += 1;
        } else if !old_child.is_none() && new_child.is_none() {
            *self.node.nr_children_mut() -= 1;
        }

        old_child
    }

    /// 若条目映射为未跟踪的大页，则将其拆分为更小的页面。
    ///
    /// 如果该条目确实映射为未跟踪的大页，则拆分成由子页表节点管理的小页面，返回新的子页表节点。
    /// 如果不是未跟踪的大页映射，则返回 `None`。
    pub(crate) fn split_if_untracked_huge(
        self,
    ) -> Option<PageTableNode<'b, EntryType, PagingConsts>> {
        let level = self.node.level();

        if !(self.pte.is_last(level)
            && level > 1
            && self.node.is_tracked() == MapTrackingStatus::Untracked)
        {
            return None;
        }

        let pa = self.pte.paddr();
        let prop = self.pte.prop();

        // let mut new_page = self.alloc(
        //     level - 1,
        //     MapTrackingStatus::Untracked,
        // );
        let new_page = self
            .node
            .alloc_empty_pt(level - 1, MapTrackingStatus::Untracked);
        self.replace(Child::PageTable(new_page));
        let cur_child = &mut self.node.page.table[self.idx];

        if let Child::PageTable(ref mut page) = *cur_child {
            let mut new_page = page.lock();
            for i in 0..nr_subpage_per_huge::<PagingConsts>() {
                let small_pa = pa + i * page_size::<PagingConsts>(level - 1);
                let _ = new_page
                    .entry(i)
                    .replace(Child::Untracked(small_pa, level - 1, prop));
            }

            let _ = self.replace(Child::PageTable(page.clone_raw()));

            Some(new_page)
        } else {
            None
        }
    }

    /// 在节点中创建一个新的条目。
    ///
    /// # 安全性
    ///
    /// 调用者必须保证给定的索引在节点的有效范围内。
    pub(super) unsafe fn new_at(
        node: &'a mut PageTableNode<EntryType, PagingConsts>,
        idx: usize,
    ) -> Self {
        // 安全性：索引在有效范围内。
        let pte = unsafe { node.read_pte(idx) };
        Self { pte, idx, node }
    }
}
