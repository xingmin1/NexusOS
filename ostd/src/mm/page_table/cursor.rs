// SPDX-License-Identifier: MPL-2.0

//! 用于在页表上进行映射和查询的页表游标。
//!
//! ## 页表锁协议
//!
//! 我们提供了一种细粒度的锁协议，允许对页表进行并发访问。该协议最初由
//! Ruihan Li <lrh2000@pku.edu.cn> 提出。
//!
//! [`CursorMut::new`] 接受一个地址范围，表示此游标可能访问的页表项。
//!
//! 然后，[`CursorMut::new`] 会找到一个中间页表（不一定是最后级或顶级），
//! 该页表代表的地址范围包含整个指定的地址范围。它会从根页表到中间页表获取所有锁，
//! 但随后会释放除中间页表锁之外的所有锁。CursorMut 然后维护从中间页表到游标
//! 当前操作的叶节点的锁保护。
//!
//! 例如，如果我们要映射下面显示的地址范围：
//!
//! ```plain
//! 顶级页表节点                       A
//!                                 /
//!                                B
//!                               / \
//! 最后一级页表节点                C   D
//! 最后一级页表项              ---**...**---
//!                             \__ __/
//!                                V
//!                     我们要映射的地址范围
//! ```
//!
//! 当调用 [`CursorMut::new`] 时，它将：
//!  1. `lock(A)`，`lock(B)`，`unlock(A)`；
//!  2. `guards = [ locked(B) ]`。
//!
//! 当调用 [`CursorMut::map`] 时，它将：
//!  1. `lock(C)`，`guards = [ locked(B), locked(C) ]`；
//!  2. 在 `C` 中映射一些页面；
//!  3. `unlock(C)`，`lock_guard = [ locked(B) ]`；
//!  4. `lock(D)`，`lock_guard = [ locked(B), locked(D) ]`；
//!  5. 在 D 中映射一些页面；
//!  6. `unlock(D)`，`lock_guard = [ locked(B) ]`；
//!
//!
//! ## 有效性
//!
//! 页表游标 API 将保证页表作为一个数据结构，其占用的内存不会遭受数据竞争。
//! 这由页表锁协议保证。换句话说，API 提供的任何操作（只要满足安全要求）
//! 都不会破坏页表数据结构（或其他内存）。
//!
//! 然而，页表游标创建 API，[`CursorMut::new`] 或 [`Cursor::new`]，
//! 不保证对您声明的虚拟地址区域的独占访问。从锁协议中可以看出，
//! 有可能创建两个声明相同虚拟地址范围的游标（一个覆盖另一个）。
//! 在这种情况下，如果较大的游标想要修改较小游标覆盖的页表项，它可能会被阻塞。
//! 此外，如果较大的游标销毁了较小游标的父页表节点，它不会被阻塞，
//! 且较小游标的更改将不可见。页表游标的用户应该添加额外的入口点检查，
//! 以防止这些定义的行为（如果不需要的话）。

use core::{any::TypeId, marker::PhantomData, mem::ManuallyDrop, ops::Range};

use align_ext::AlignExt;

use super::{
    page_size, pte_index, Child, Entry, KernelMode, MapTrackingStatus, PageTable,
    PageTableEntryTrait, PageTableError, PageTableMode, PageTableNode, PagingConstsTrait,
    PagingLevel, UserMode,
};
use crate::{
    mm::{
        frame::{Frame, MemoryType, Unknown},
        kspace::should_map_as_tracked,
        nr_subpage_per_huge,
        page_prop::PageProperty,
        Paddr, Vaddr,
    },
    task::{disable_preempt, DisabledPreemptGuard},
};

#[derive(Clone, Debug)]
pub enum PageTableItem {
    NotMapped {
        va: Vaddr,
        len: usize,
    },
    Mapped {
        va: Vaddr,
        page: Frame<Unknown>,
        prop: PageProperty,
    },
    MappedUntracked {
        va: Vaddr,
        pa: Paddr,
        len: usize,
        prop: PageProperty,
    },
}

/// 用于遍历页表的游标。
///
/// 槽位是任何级别的页表项（PTE），对应于当前级别"页面大小"的特定虚拟内存范围。
///
/// 游标能够移动到下一个槽位，读取页面属性，甚至直接跳转到虚拟地址。
/// 我们使用保护栈模拟递归，并采用页表锁定协议提供并发性。
#[derive(Debug)]
pub struct Cursor<'a, M: PageTableMode, E: PageTableEntryTrait, C: PagingConstsTrait>
where
    [(); C::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<C>()]:,
{
    /// 游标的锁保护。1级页表锁保护在索引0处，N级页表锁保护在索引N-1处。
    ///
    /// 销毁游标时，锁将按从低到高的顺序释放，与获取顺序完全相反。
    /// 这种行为由Rust的默认drop实现保证：
    /// <https://doc.rust-lang.org/reference/destructors.html>。
    guards: [Option<PageTableNode<'a, E, C>>; C::NR_LEVELS as usize],
    /// 游标指向的页表级别。
    level: PagingLevel,
    /// 从`guard_level`到`level`，锁保存在`guards`中。
    guard_level: PagingLevel,
    /// 游标当前指向的虚拟地址。
    va: Vaddr,
    /// 被锁定的虚拟地址范围。
    barrier_va: Range<Vaddr>,
    #[expect(dead_code)]
    preempt_guard: DisabledPreemptGuard,
    // _phantom: PhantomData<&'a PageTable<M, E, C>>,
    page_table: &'a PageTable<M, E, C>,
}

impl<'a, Mode: PageTableMode, E: PageTableEntryTrait, C: PagingConstsTrait> Cursor<'a, Mode, E, C>
where
    [(); C::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<C>()]:,
{
    /// 创建一个游标，声明对给定范围的读取访问权限。
    ///
    /// 创建的游标只能在给定范围内查询或跳转。超出范围的访问将导致
    /// 恐慌或作为返回值的错误，这取决于访问方法。
    ///
    /// 注意，此函数不确保对声明的虚拟地址范围的独占访问。
    /// 使用此游标的访问可能会被阻塞或失败。
    pub fn new(pt: &'a PageTable<Mode, E, C>, va: &Range<Vaddr>) -> Result<Self, PageTableError> {
        if !Mode::covers(va) || va.is_empty() {
            return Err(PageTableError::InvalidVaddrRange(va.start, va.end));
        }
        if va.start % C::BASE_PAGE_SIZE != 0 || va.end % C::BASE_PAGE_SIZE != 0 {
            return Err(PageTableError::UnalignedVaddr);
        }

        let mut cursor = Self {
            guards: core::array::from_fn(|_| None),
            level: C::NR_LEVELS,
            guard_level: C::NR_LEVELS,
            va: va.start,
            barrier_va: va.clone(),
            preempt_guard: disable_preempt(),
            page_table: pt,
        };

        // let mut cur_pt_addr = pt.root.paddr();

        let child = &pt.root.get_child(pte_index::<C>(va.start, cursor.level));
        cursor.level -= 1;

        let raw;

        // 向下获取适当的锁。游标应持有包含虚拟地址范围的页表节点的锁。
        //
        // 在向下过程中，将释放过高级别的先前保护。
        loop {
            let start_idx = pte_index::<C>(va.start, cursor.level);
            let level_too_high = {
                let end_idx = pte_index::<C>(va.end - 1, cursor.level);
                cursor.level > 1 && start_idx == end_idx
            };
            if !level_too_high {
                break;
            }

            // let cur_pt_ptr = paddr_to_vaddr(cur_pt_addr) as *mut E;
            // 安全性：
            //
            // 根页表的生命周期足够长，因此该指针及其索引均有效；
            // 同时，在我们使用期间，子页表节点也不会被其他线程回收。
            // let cur_pte = unsafe { cur_pt_ptr.add(start_idx).read() };
            raw = match raw.get_child(start_idx) {
                Child::PageTable(pt) => pt,
                Child::None => break,
                Child::Frame(_, _) => break,
                Child::Untracked(_, _, _) => {
                    panic!("尝试在未跟踪区域上映射跟踪页面");
                }
            };
            // if cur_pte.is_present() {
            //     if cur_pte.is_last(cursor.level) {
            //         break;
            //     } else {
            //         cur_pt_addr = cur_pte.paddr();
            //     }
            // } else {
            //     break;
            // }
            cursor.level -= 1;
        }

        // 安全性：
        //
        // 当前的地址和层级对应于已转换成页表项的子节点，我们通过浅克隆该节点获得一个新的句柄。
        // let raw = unsafe { RawPageTableNode::<E, C>::from_raw_parts(cur_pt_addr, cursor.level) };
        let _inc_ref = ManuallyDrop::new(raw.clone_shallow());
        let lock = raw.lock();
        cursor.guards[cursor.level as usize - 1] = Some(lock);
        cursor.guard_level = cursor.level;

        Ok(cursor)
    }

    /// 获取当前槽位的映射信息。
    pub fn query(&mut self) -> Result<PageTableItem, PageTableError> {
        if self.va >= self.barrier_va.end {
            return Err(PageTableError::InvalidVaddr(self.va));
        }

        loop {
            let level = self.level;
            let va = self.va;

            match self.cur_entry().to_owned() {
                Child::PageTable(pt) => {
                    self.push_level(pt.lock());
                    continue;
                }
                Child::None => {
                    return Ok(PageTableItem::NotMapped {
                        va,
                        len: page_size::<C>(level),
                    });
                }
                Child::Frame(page, prop) => {
                    return Ok(PageTableItem::Mapped { va, page, prop });
                }
                Child::Untracked(pa, plevel, prop) => {
                    debug_assert_eq!(*plevel, level);
                    return Ok(PageTableItem::MappedUntracked {
                        va,
                        pa,
                        len: page_size::<C>(level),
                        prop,
                    });
                }
            }
        }
    }

    /// 在当前层级中前进到下一个页表项。
    ///
    /// 若当前页表节点已经到达末尾，则会尽可能上升到父页表的下一页。
    pub(crate) fn move_forward(&mut self) {
        let page_size = page_size::<C>(self.level);
        let next_va = self.va.align_down(page_size) + page_size;
        while self.level < self.guard_level && pte_index::<C>(next_va, self.level) == 0 {
            self.pop_level();
        }
        self.va = next_va;
    }

    /// 跳转到指定的虚拟地址。
    /// 如果目标地址超出有效范围，则返回 `Err`。
    ///
    /// # Panic
    ///
    /// 当地址对齐不正确时，此方法会触发 panic。
    pub fn jump(&mut self, va: Vaddr) -> Result<(), PageTableError> {
        assert_eq!(va % C::BASE_PAGE_SIZE, 0);

        if !self.barrier_va.contains(&va) {
            return Err(PageTableError::InvalidVaddr(va));
        }

        loop {
            let cur_node_start = self.va & !(page_size::<C>(self.level + 1) - 1);
            let cur_node_end = cur_node_start + page_size::<C>(self.level + 1);
            // 如果目标地址位于当前页表节点内，则可直接跳转。
            if cur_node_start <= va && va < cur_node_end {
                self.va = va;
                return Ok(());
            }

            // 特殊情况：游标已耗尽，停在下一节点的起始处，但由于父节点未被锁定，下一节点也未被锁定。
            if self.va >= self.barrier_va.end && self.level == self.guard_level {
                self.va = va;
                return Ok(());
            }

            debug_assert!(self.level < self.guard_level);
            self.pop_level();
        }
    }

    pub fn virt_addr(&self) -> Vaddr {
        self.va
    }

    /// 上升一级。
    ///
    /// 如果当前页没有任何映射（游标只会向前移动），则会释放该页；必要时，在游标销毁前会进行最后的清理。
    ///
    /// 注意：调用此方法前必须已经获得相应的锁，被丢弃的层级也会被同时解锁。
    fn pop_level(&mut self) {
        self.guards[self.level as usize - 1] = None;
        self.level += 1;

        // TODO: 若页表节点变为空，则释放该页表。
    }

    /// 下降一级，进入子页表。
    fn push_level(&mut self, child_pt: PageTableNode<E, C>) {
        self.level -= 1;
        debug_assert_eq!(self.level, child_pt.level());
        self.guards[self.level as usize - 1] = Some(child_pt);
    }

    fn should_map_as_tracked(&self) -> bool {
        (TypeId::of::<Mode>() == TypeId::of::<KernelMode>()
            || TypeId::of::<Mode>() == TypeId::of::<UserMode>())
            && should_map_as_tracked(self.va)
    }

    fn cur_entry(&mut self) -> Entry<'_, '_, E, C> {
        let node = self.guards[self.level as usize - 1].as_mut().unwrap();
        node.entry(pte_index::<C>(self.va, self.level))
    }
}

impl<M: PageTableMode, E: PageTableEntryTrait, C: PagingConstsTrait> Iterator
    for Cursor<'_, M, E, C>
where
    [(); C::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<C>()]:,
{
    type Item = PageTableItem;

    fn next(&mut self) -> Option<Self::Item> {
        let result = self.query();
        if result.is_ok() {
            self.move_forward();
        }
        result.ok()
    }
}

/// 可进行映射、解除映射或保护操作的页表游标。
///
/// 此游标拥有 [`Cursor`] 的所有功能；在页表中，一个虚拟地址范围无论是否可变，都只能由一个游标访问。
#[derive(Debug)]
pub struct CursorMut<'a, M: PageTableMode, E: PageTableEntryTrait, C: PagingConstsTrait>(
    Cursor<'a, M, E, C>,
)
where
    [(); C::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<C>()]:;

impl<'a, M: PageTableMode, E: PageTableEntryTrait, C: PagingConstsTrait> CursorMut<'a, M, E, C>
where
    [(); C::NR_LEVELS as usize]:,
    [(); nr_subpage_per_huge::<C>()]:,
{
    /// 创建一个游标，声明对给定范围具有写权限。
    ///
    /// 该游标仅能在指定范围内执行映射、查询或跳转操作，超出范围的访问会返回错误或触发 panic，
    /// 具体行为取决于所调用的方法。
    ///
    /// 注意：此函数与 [`Cursor::new`] 相同，并不保证对所声明虚拟地址范围的独占访问，其操作可能会被阻塞或失败。
    pub(super) fn new(
        pt: &'a PageTable<M, E, C>,
        va: &Range<Vaddr>,
    ) -> Result<Self, PageTableError> {
        Cursor::new(pt, va).map(|inner| Self(inner))
    }

    /// 跳转到指定的虚拟地址。
    ///
    /// 此方法与 [`Cursor::jump`] 的行为一致。
    ///
    /// # Panic
    ///
    /// 如果目标地址超出游标操作的有效范围或地址对齐错误，将触发 panic。
    pub fn jump(&mut self, va: Vaddr) -> Result<(), PageTableError> {
        self.0.jump(va)
    }

    /// 获取当前的虚拟地址。
    pub fn virt_addr(&self) -> Vaddr {
        self.0.virt_addr()
    }

    /// 获取当前槽位的映射信息。
    pub fn query(&mut self) -> Result<PageTableItem, PageTableError> {
        self.0.query()
    }

    /// 将从当前地址开始的一段区域映射到一个 [`Frame<dyn MemoryType>`] 上。
    ///
    /// 如果该区域之前已有映射，则返回之前映射的 [`Frame<dyn MemoryType>`]。
    ///
    /// # Panic
    ///
    /// 当发生下列情况之一时会触发 panic：
    ///  - 待映射的虚拟地址范围超出有效范围；
    ///  - 虚拟地址的对齐不满足页面要求；
    ///  - 当前区域已映射为大页，而调用者期望映射为较小页面。
    ///
    /// # 安全性
    ///
    /// 调用者必须确保待映射的虚拟地址范围不会危害内核内存安全。
    pub unsafe fn map(
        &mut self,
        page: Frame<Unknown>,
        prop: PageProperty,
    ) -> Option<Frame<Unknown>> {
        let end = self.0.va + page.size();
        assert!(end <= self.0.barrier_va.end);

        // 若当前情况不适用于直接映射，则下降层级进行处理。
        while self.0.level > C::HIGHEST_TRANSLATION_LEVEL
            || self.0.va % page_size::<C>(self.0.level) != 0
            || self.0.va + page_size::<C>(self.0.level) > end
        {
            debug_assert!(self.0.should_map_as_tracked());
            let cur_level = self.0.level;
            let cur_entry = self.0.cur_entry();
            match cur_entry.to_owned() {
                Child::PageTable(pt) => {
                    self.0.push_level(pt.lock());
                }
                Child::None => {
                    let pt =
                        PageTableNode::<E, C>::alloc(cur_level - 1, MapTrackingStatus::Tracked);
                    let _ = cur_entry.replace(Child::PageTable(pt.clone_raw()));
                    self.0.push_level(pt);
                }
                Child::Frame(_, _) => {
                    panic!("Mapping a smaller page in an already mapped huge page");
                }
                Child::Untracked(_, _, _) => {
                    panic!("Mapping a tracked page in an untracked range");
                }
            }
            continue;
        }
        debug_assert_eq!(self.0.level, page.level());

        // 执行当前页的映射操作。
        let old = self.0.cur_entry().replace(Child::Frame(page, prop));
        self.0.move_forward();

        match old {
            Child::Frame(old_page, _) => Some(old_page),
            Child::None => None,
            Child::PageTable(_) => {
                todo!("Dropping page table nodes while mapping requires TLB flush")
            }
            Child::Untracked(_, _, _) => panic!("Mapping a tracked page in an untracked range"),
        }
    }

    /// 将从当前地址开始的一段区域映射到物理地址范围上。
    ///
    /// 此函数会尽可能使用大页进行映射，并在必要时拆分大页成较小的页面。
    /// 若输入范围较大，最终的映射可能如下（当支持极大页时）：
    ///
    /// ```text
    /// start                                                             end
    ///   |----|----------------|--------------------------------|----|----|
    ///    base      huge                     very huge           base base
    ///    4KiB      2MiB                       1GiB              4KiB  4KiB
    /// ```
    ///
    /// 实际上，为了安全和代码简洁，不建议使用此方法。
    ///
    /// # Panic
    ///
    /// 如果待映射的虚拟地址范围超出有效范围，则触发 panic。
    ///
    /// # 安全性
    ///
    /// 调用者必须确保：
    ///  - 待映射的范围不会危害内核内存安全；
    ///  - 待映射的物理地址有效且安全可用；
    ///  - 当前虚拟地址范围允许映射未跟踪页面。
    pub unsafe fn map_pa(&mut self, pa: &Range<Paddr>, prop: PageProperty) {
        let end = self.0.va + pa.len();
        let mut pa = pa.start;
        assert!(end <= self.0.barrier_va.end);

        while self.0.va < end {
            // 确保不在内核保留或共享的页表中进行映射或释放操作。
            // 尽管对于所有架构而言这通常是不变的，并且会被编译器优化掉（因为 `C::NR_LEVELS - 1 > C::HIGHEST_TRANSLATION_LEVEL`）。
            let is_kernel_shared_node =
                TypeId::of::<M>() == TypeId::of::<KernelMode>() && self.0.level >= C::NR_LEVELS - 1;
            if self.0.level > C::HIGHEST_TRANSLATION_LEVEL
                || is_kernel_shared_node
                || self.0.va % page_size::<C>(self.0.level) != 0
                || self.0.va + page_size::<C>(self.0.level) > end
                || pa % page_size::<C>(self.0.level) != 0
            {
                let cur_level = self.0.level;
                let cur_entry = self.0.cur_entry();
                match cur_entry.to_owned() {
                    Child::PageTable(pt) => {
                        self.0.push_level(pt.lock());
                    }
                    Child::None => {
                        let pt = PageTableNode::<E, C>::alloc(
                            cur_level - 1,
                            MapTrackingStatus::Untracked,
                        );
                        let _ = cur_entry.replace(Child::PageTable(pt.clone_raw()));
                        self.0.push_level(pt);
                    }
                    Child::Frame(_, _) => {
                        panic!("在已映射大页上映射较小页面");
                    }
                    Child::Untracked(_, _, _) => {
                        let split_child = cur_entry.split_if_untracked_huge().unwrap();
                        self.0.push_level(split_child);
                    }
                }
                continue;
            }

            // 执行当前页的映射操作。
            debug_assert!(!self.0.should_map_as_tracked());
            let level = self.0.level;
            let _ = self
                .0
                .cur_entry()
                .replace(Child::Untracked(pa, level, prop));

            // 前进游标。
            pa += page_size::<C>(level);
            self.0.move_forward();
        }
    }

    /// 在游标后续范围内寻找并移除第一个存在映射的页面。
    ///
    /// 查找范围以当前虚拟地址为起点，长度为指定值。
    ///
    /// 如果成功移除了一个页面（无论后续是否还有页面需要解除映射），则函数会停止并返回该页面
    /// （返回的页面为解除映射前存在映射的页面）。
    /// 同时，在成功移除页面后，游标将前进至该页面之后的下一页；若后续范围内没有映射，
    /// 则游标停在范围末端并返回 [`PageTableItem::NotMapped`]。
    ///
    /// # 安全性
    ///
    /// 调用者必须确保待解除映射的范围不会危害内核内存安全。
    ///
    /// # Panic
    ///
    /// 如果指定的范围末端覆盖了大页的一部分，而下一页正属于该大页，则会触发 panic。
    pub unsafe fn take_next(&mut self, len: usize) -> PageTableItem {
        let start = self.0.va;
        assert!(len % page_size::<C>(1) == 0);
        let end = start + len;
        assert!(end <= self.0.barrier_va.end);

        while self.0.va < end {
            let cur_va = self.0.va;
            let cur_level = self.0.level;
            let cur_entry = self.0.cur_entry();

            // 如果当前页面已无映射，则跳过。
            if cur_entry.is_none() {
                if self.0.va + page_size::<C>(self.0.level) > end {
                    self.0.va = end;
                    break;
                }
                self.0.move_forward();
                continue;
            }

            // 如果当前条目为子页表节点，或该页面不满足对齐要求，或超出指定范围，则下降层级处理。
            if cur_entry.is_node()
                || cur_va % page_size::<C>(cur_level) != 0
                || cur_va + page_size::<C>(cur_level) > end
            {
                let child = cur_entry.to_owned();
                match child {
                    Child::PageTable(pt) => {
                        let pt = pt.lock();
                        // 如果下一层存在映射，则下降层级以节省时间。
                        if pt.nr_children() != 0 {
                            self.0.push_level(pt);
                        } else {
                            if self.0.va + page_size::<C>(self.0.level) > end {
                                self.0.va = end;
                                break;
                            }
                            self.0.move_forward();
                        }
                    }
                    Child::None => {
                        unreachable!("已检查空映射情况");
                    }
                    Child::Frame(_, _) => {
                        panic!("尝试解除部分大页映射");
                    }
                    Child::Untracked(_, _, _) => {
                        let split_child = cur_entry.split_if_untracked_huge().unwrap();
                        self.0.push_level(split_child);
                    }
                }
                continue;
            }

            // 解除当前页面的映射，并返回该页面。
            let old = cur_entry.replace(Child::None);

            self.0.move_forward();

            return match old {
                Child::Frame(page, prop) => PageTableItem::Mapped {
                    va: self.0.va,
                    page,
                    prop,
                },
                Child::Untracked(pa, level, prop) => {
                    debug_assert_eq!(level, self.0.level);
                    PageTableItem::MappedUntracked {
                        va: self.0.va,
                        pa: *pa,
                        len: page_size::<C>(level),
                        prop: *prop,
                    }
                }
                Child::PageTable(_) | Child::None => unreachable!(),
            };
        }

        // 如果循环结束，说明在该范围内未找到任何映射页面。
        PageTableItem::NotMapped { va: start, len }
    }

    /// 对指定范围内下一个映射槽执行保护操作。
    ///
    /// 查找范围以当前虚拟地址为起点，长度为指定值。
    ///
    /// 如果实际对某个页面执行了保护操作，则函数会停止并返回该页面受保护的地址范围，
    /// 即使后续页面也需保护，也只返回此次保护的页面范围。
    /// 同时，在实际保护后，游标会前进至该页面之后的下一页；若后续范围内没有映射，
    /// 则游标停在范围末端并返回 [`None`]。
    ///
    /// # 安全性
    ///
    /// 调用者必须确保进行保护操作的地址范围不会危害内核内存安全。
    ///
    /// # Panic
    ///
    /// 当下列情况发生时会触发 panic：
    ///  - 待保护范围超出游标操作的有效范围；
    ///  - 指定的虚拟地址范围仅覆盖了页面的一部分。
    pub unsafe fn protect_next(
        &mut self,
        len: usize,
        op: &mut impl FnMut(&mut PageProperty),
    ) -> Option<Range<Vaddr>> {
        let end = self.0.va + len;
        assert!(end <= self.0.barrier_va.end);

        while self.0.va < end {
            let cur_va = self.0.va;
            let cur_level = self.0.level;
            let mut cur_entry = self.0.cur_entry();

            // 如果当前页面没有映射，则跳过。
            if cur_entry.is_none() {
                self.0.move_forward();
                continue;
            }

            // 如果当前条目为页表节点，则下降层级处理。
            if cur_entry.is_node() {
                let Child::PageTable(pt) = cur_entry.to_owned() else {
                    unreachable!("已检查类型");
                };
                let pt = pt.lock();
                // 如果下一层存在映射，则下降层级以节省时间；否则直接前进。
                if pt.nr_children() != 0 {
                    self.0.push_level(pt);
                } else {
                    self.0.move_forward();
                }
                continue;
            }

            // 若当前页面过大且试图仅保护其部分内容，则下降层级将大页拆分为小页。
            if cur_va % page_size::<C>(cur_level) != 0 || cur_va + page_size::<C>(cur_level) > end {
                let split_child = cur_entry
                    .split_if_untracked_huge()
                    .expect("尝试保护大页的一部分内容");
                self.0.push_level(split_child);
                continue;
            }

            // 对当前页面执行保护操作。
            cur_entry.protect(op);

            let protected_va = self.0.va..self.0.va + page_size::<C>(self.0.level);
            self.0.move_forward();

            return Some(protected_va);
        }

        None
    }

    /// 将给定游标中的映射复制到当前游标中。
    ///
    /// 当前游标所在范围必须没有任何映射。该函数允许源游标在复制前对映射进行操作，
    /// 相当于先对其执行保护操作再复制。注意，仅复制映射信息，并不复制实际的页面。
    ///
    /// 仅支持复制已跟踪的映射，因为未跟踪的映射认为没有复制的意义。
    ///
    /// 操作完成后，两个游标均会按照指定的长度前进。
    ///
    /// # 安全性
    ///
    /// 调用者必须确保：
    ///  - 被复制的范围不会危害内核内存安全；
    ///  - 两个游标都处于跟踪映射区域中。
    ///
    /// # Panic
    ///
    /// 如果发生下列情况之一，将触发 panic：
    ///  - 任一复制范围超出游标有效操作范围；
    ///  - 任一指定虚拟地址范围仅覆盖了页面的一部分；
    ///  - 当前游标所在范围内存在已映射页面。
    pub unsafe fn copy_from(
        &mut self,
        src: &mut Self,
        len: usize,
        op: &mut impl FnMut(&mut PageProperty),
    ) {
        assert!(len % page_size::<C>(1) == 0);
        let this_end = self.0.va + len;
        assert!(this_end <= self.0.barrier_va.end);
        let src_end = src.0.va + len;
        assert!(src_end <= src.0.barrier_va.end);

        while self.0.va < this_end && src.0.va < src_end {
            let src_va = src.0.va;
            let mut src_entry = src.0.cur_entry();

            match src_entry.to_owned() {
                Child::PageTable(pt) => {
                    let pt = pt.lock();
                    // 如果下一层存在映射，则下降处理以节省时间。
                    if pt.nr_children() != 0 {
                        src.0.push_level(pt);
                    } else {
                        src.0.move_forward();
                    }
                    continue;
                }
                Child::None => {
                    src.0.move_forward();
                    continue;
                }
                Child::Untracked(_, _, _) => {
                    panic!("不能复制未跟踪的映射");
                }
                Child::Frame(page, mut prop) => {
                    let mapped_page_size = page.size();

                    // 对页面执行保护操作。
                    src_entry.protect(op);

                    // 执行复制操作。
                    op(&mut prop);
                    self.jump(src_va).unwrap();
                    let original = self.map(page, prop);
                    assert!(original.is_none());

                    // 仅前进源游标，因为 `Self::map` 已自动前进。
                    // 此断言确保两者移动的长度一致。
                    debug_assert_eq!(mapped_page_size, page_size::<C>(src.0.level));
                    src.0.move_forward();
                }
            }
        }
    }
}
