// SPDX-License-Identifier: MPL-2.0

//! 引导页表模块，用于操作系统初始化阶段的内存映射管理。
//!
//! 在操作系统启动过程中，需要先建立一个临时的页表系统来进行初始内存映射，
//! 然后才能初始化正式的内存管理系统。引导页表提供了这一临时机制，它能够：
//!
//! 1. 利用固件或引导加载器提供的初始页表作为基础
//! 2. 在此基础上创建新的映射，以支持内核初始化
//! 3. 跟踪自身分配的页表资源，确保正确释放
//! 4. 在正式页表系统准备就绪后安全地解除自身
//!
//! 引导页表使用 `PageFlags::AVAIL1` 标志位来标记和区分自身分配的页表帧，
//! 这使得它能够在被释放时只回收自己分配的资源，而不干扰固件或加载器设置的页表结构。

use core::{
    result::Result,
    sync::atomic::{AtomicU32, Ordering},
};

use super::{pte_index, PageTableEntryTrait};
use crate::{
    arch::mm::{PageTableEntry, PagingConsts},
    cpu::num_cpus,
    cpu_local_cell,
    mm::{
        frame::allocator::FRAME_ALLOCATOR,
        kspace::paddr_to_vaddr,
        nr_subpage_per_huge,
        page_prop::{PageFlags, PageProperty},
        Paddr, PagingConstsTrait, PagingLevel, Vaddr, PAGE_SIZE,
    },
    sync::SpinLock,
};

type FrameNumber = usize;

/// 引导页表单例 [`BootPageTable`] 的访问器。
///
/// 用户需要提供一个闭包来访问引导页表。该函数会获取锁并以引导页表的
/// 可变引用作为参数调用闭包。
///
/// 当没有CPU激活引导页表时，它将被释放。如果引导页表已被释放，
/// 此函数将返回 [`Err`]。
///
/// # 示例
///
/// ```
/// with_borrow(|boot_pt| {
///     unsafe { boot_pt.map_base_page(vaddr, frame_num, prop) };
/// }).expect("引导页表已被释放");
/// ```
pub(crate) fn with_borrow<F, R>(f: F) -> Result<R, ()>
where
    F: FnOnce(&mut BootPageTable) -> R,
{
    let mut boot_pt = BOOT_PAGE_TABLE.lock();

    if IS_DISMISSED.load() {
        return Err(());
    }

    // 懒初始化
    if boot_pt.is_none() {
        // 安全性：此函数仅被调用一次
        *boot_pt = Some(unsafe { BootPageTable::from_current_pt() });
    }

    let r = f(boot_pt.as_mut().unwrap());

    Ok(r)
}

/// 解除引导页表。
///
/// 在CPU上调用此函数表示该CPU不再需要引导页表。当所有CPU都解除引导页表后，
/// 引导页表将被销毁，并释放其分配的所有页表帧（通过检查 `PageFlags::AVAIL1` 标志）。
///
/// # 安全性
///
/// 调用者应确保：
///  - 该CPU上已激活另一个合法的页表；
///  - 每个CPU上此函数只被调用一次；
///  - 解除后，该CPU上不再调用 [`with_borrow`]；
///  - 在激活另一个页表后、解除前，该CPU上不调用 [`with_borrow`]。
pub(crate) unsafe fn dismiss() {
    IS_DISMISSED.store(true);
    if DISMISS_COUNT.fetch_add(1, Ordering::SeqCst) as usize == num_cpus() - 1 {
        BOOT_PAGE_TABLE.lock().take();
    }
}

/// 引导页表单例实例。
static BOOT_PAGE_TABLE: SpinLock<Option<BootPageTable>> = SpinLock::new(None);
/// 如果达到CPU数量，引导页表将被释放。
static DISMISS_COUNT: AtomicU32 = AtomicU32::new(0);
cpu_local_cell! {
    /// 标记此CPU上的引导页表是否已解除。
    static IS_DISMISSED: bool = false;
}

/// 引导阶段映射管理的简单引导页表单例。
///
/// 引导页表在操作系统初始化早期阶段提供临时的内存映射功能，它能够：
/// - 在固件或引导加载器提供的初始页表基础上创建新映射
/// - 修改已有映射的保护属性
/// - 跟踪自身分配的页表资源
///
/// ## 资源管理机制
///
/// 引导页表使用 `PageFlags::AVAIL1` 标志位来标记由自身分配的页表帧。
/// 这种标记机制确保了：
/// 1. 能够区分哪些页表帧是由引导页表分配的，哪些是由固件或加载器预先设置的
/// 2. 在引导页表被销毁时，只释放它自己分配的帧，不干扰系统关键页表结构
/// 3. 防止内存泄漏，确保所有引导阶段分配的临时资源都能被正确回收
///
/// 这种机制对于从引导阶段向正常运行阶段的安全过渡至关重要。
pub(crate) struct BootPageTable<
    E: PageTableEntryTrait = PageTableEntry,
    C: PagingConstsTrait = PagingConsts,
> {
    root_pt: FrameNumber,
    _pretend_to_use: core::marker::PhantomData<(E, C)>,
}

impl<E: PageTableEntryTrait, C: PagingConstsTrait> BootPageTable<E, C> {
    /// 从当前页表根物理地址创建新的引导页表。
    ///
    /// 此方法获取当前活动的页表物理地址，并以此为基础创建引导页表。
    /// 在创建过程中，会遍历整个页表结构并清除所有 `PageFlags::AVAIL1` 标志，
    /// 确保不会误认为固件或加载器提供的页表是由引导页表分配的。
    ///
    /// # 安全性
    ///
    /// 此函数应仅在初始化阶段调用一次。
    /// 否则，会导致由固件、加载器或设置代码建立的页表帧被重复释放。
    unsafe fn from_current_pt() -> Self {
        let root_pt = crate::arch::mm::current_page_table_paddr() / C::BASE_PAGE_SIZE;
        // 确保固件页表中的第一个可用位未被设置
        // 遍历页表中的所有条目，清除PageFlags::AVAIL1标志
        // 这一步骤确保我们不会误认为固件提供的页表是由引导页表分配的
        dfs_walk_on_leave::<E, C>(root_pt, C::NR_LEVELS, &mut |pte: &mut E| {
            let prop = pte.prop();
            if prop.flags.contains(PageFlags::AVAIL1) {
                pte.set_prop(PageProperty::new(prop.flags - PageFlags::AVAIL1));
            }
        });
        Self {
            root_pt,
            _pretend_to_use: core::marker::PhantomData,
        }
    }

    /// 返回引导页表的根物理地址。
    pub(crate) fn root_address(&self) -> Paddr {
        self.root_pt * C::BASE_PAGE_SIZE
    }

    /// 将虚拟地址映射到指定的物理帧。
    /// 在最后一级页表中创建实际的页面映射
    ///
    /// 此函数在引导页表中创建从虚拟地址到物理帧的映射，是操作系统初始化过程中
    /// 建立内存映射的基础操作。函数会自动创建必要的中间级页表结构，并在需要时
    /// 分配新的页表帧（这些新帧会被标记为 `PageFlags::AVAIL1`）。
    ///
    /// # 实现细节
    ///
    /// 函数分两个主要阶段工作：
    /// 1. **页表路径准备**：从最高级页表开始，逐级向下遍历，确保到达目标页面的
    ///    整个页表路径都存在。如果中间级页表不存在，会自动分配新的页表帧。
    /// 2. **页面映射创建**：在最后一级页表中，将虚拟地址映射到指定的物理帧，
    ///    并设置指定的页面属性。
    ///
    /// # 参数
    ///
    /// - `from`: 要映射的虚拟地址
    /// - `to`: 目标物理帧号（物理地址除以页大小）
    /// - `prop`: 页面属性，包含访问权限和缓存策略等信息
    ///
    /// # Panics
    ///
    /// 在以下情况会触发panic：
    /// - 尝试在已映射的大页上映射基本页
    /// - 尝试映射已有映射的虚拟地址
    ///
    /// # 安全性
    ///
    /// 此函数标记为unsafe，因为：
    /// - 直接操作物理内存和页表结构
    /// - 映射错误的地址可能导致系统崩溃或安全漏洞
    /// - 调用者必须确保映射的虚拟地址范围是合法的，特别是避免映射内核关键区域
    pub unsafe fn map_base_page(&mut self, from: Vaddr, to: FrameNumber, prop: PageProperty) {
        let mut pt = self.root_pt;
        let mut level = C::NR_LEVELS;
        // 遍历到页表的最后一级，确保整个页表路径存在
        while level > 1 {
            let index = pte_index::<C>(from, level);
            let pte_ptr = unsafe { (paddr_to_vaddr(pt * C::BASE_PAGE_SIZE) as *mut E).add(index) };
            let pte = unsafe { pte_ptr.read() };
            pt = if !pte.is_present() {
                // 页表项不存在，分配新的子页表
                let pte = self.alloc_child();
                unsafe { pte_ptr.write(pte) };
                pte.paddr() / C::BASE_PAGE_SIZE
            } else if pte.is_last(level) {
                // 页表项存在且是大页映射，不允许在大页上映射基本页
                panic!("在引导页表中映射已映射的大页");
            } else {
                // 页表项存在且指向下一级页表，继续遍历
                pte.paddr() / C::BASE_PAGE_SIZE
            };
            level -= 1;
        }
        // 在最后一级页表中创建实际的页面映射
        let index = pte_index::<C>(from, 1);
        let pte_ptr = unsafe { (paddr_to_vaddr(pt * C::BASE_PAGE_SIZE) as *mut E).add(index) };
        let pte = unsafe { pte_ptr.read() };
        if pte.is_present() {
            panic!("在引导页表中映射已映射的页面");
        }
        unsafe { pte_ptr.write(E::new_page(to * C::BASE_PAGE_SIZE, 1, prop)) };
    }

    /// 修改已映射页面的保护属性。
    ///
    /// 此函数允许调整已映射页面的属性（如读/写/执行权限、缓存策略等），是操作系统
    /// 内存保护机制的基础。如果目标页面是大页的一部分，函数会自动将大页拆分为
    /// 多个基本页，以便能够单独修改目标页面的属性。
    ///
    /// # 实现细节
    ///
    /// 函数分两个主要阶段工作：
    /// 1. **页表遍历与大页处理**：从最高级页表开始，逐级向下遍历。如果遇到大页映射，
    ///    会执行拆分操作，将大页转换为多个具有相同初始属性的基本页。
    /// 2. **属性修改**：在最后一级页表中，应用提供的闭包修改目标页面的属性。
    ///
    /// ## 大页拆分机制
    ///
    /// 当需要修改大页中某个基本页的属性时，函数会：
    /// - 分配一个新的页表帧作为子页表（标记为 `PageFlags::AVAIL1`）
    /// - 为大页覆盖的每个基本页创建页表项，保持原有物理地址和属性
    /// - 将原大页页表项替换为指向新子页表的页表项
    /// - 然后继续处理目标页面
    ///
    /// # 参数
    ///
    /// - `virt_addr`: 要修改保护属性的虚拟地址
    /// - `op`: 一个闭包，接收页面属性的可变引用，用于修改属性
    ///
    /// # Panics
    ///
    /// 在以下情况会触发panic：
    /// - 尝试修改未映射页面的属性
    ///
    /// # 安全性
    ///
    /// 此函数标记为unsafe，因为：
    /// - 直接操作物理内存和页表结构
    /// - 错误的属性修改可能导致系统崩溃或安全漏洞
    /// - 调用者必须确保修改的虚拟地址范围是合法的，特别是避免错误地修改内核关键区域的属性
    ///
    /// # 使用场景
    ///
    /// - 将代码段标记为只读和可执行
    /// - 将数据段标记为可读写但不可执行
    /// - 调整设备内存区域的缓存策略
    pub unsafe fn protect_base_page(
        &mut self,
        virt_addr: Vaddr,
        mut op: impl FnMut(&mut PageProperty),
    ) {
        let mut pt = self.root_pt;
        let mut level = C::NR_LEVELS;
        // 遍历到页表的最后一级
        while level > 1 {
            let index = pte_index::<C>(virt_addr, level);
            let pte_ptr = unsafe { (paddr_to_vaddr(pt * C::BASE_PAGE_SIZE) as *mut E).add(index) };
            let pte = unsafe { pte_ptr.read() };
            pt = if !pte.is_present() {
                panic!("在引导页表中保护未映射的页面");
            } else if pte.is_last(level) {
                // 拆分大页：将一个大页转换为多个具有相同初始属性的基本页
                let child_pte = self.alloc_child();
                let child_frame_pa = child_pte.paddr();
                let huge_pa = pte.paddr();
                // 为大页覆盖的每个基本页创建页表项
                for i in 0..nr_subpage_per_huge::<C>() {
                    let nxt_ptr = unsafe { (paddr_to_vaddr(child_frame_pa) as *mut E).add(i) };
                    unsafe {
                        nxt_ptr.write(E::new_page(
                            huge_pa + i * C::BASE_PAGE_SIZE,
                            level - 1,
                            pte.prop(),
                        ))
                    };
                }
                // 将原大页页表项替换为指向新子页表的页表项
                unsafe { pte_ptr.write(E::new_pt(child_frame_pa)) };
                child_frame_pa / C::BASE_PAGE_SIZE
            } else {
                // 页表项存在且指向下一级页表，继续遍历
                pte.paddr() / C::BASE_PAGE_SIZE
            };
            level -= 1;
        }
        // 在最后一级页表中修改页面属性
        let index = pte_index::<C>(virt_addr, 1);
        let pte_ptr = unsafe { (paddr_to_vaddr(pt * C::BASE_PAGE_SIZE) as *mut E).add(index) };
        let pte = unsafe { pte_ptr.read() };
        if !pte.is_present() {
            panic!("在引导页表中保护未映射的页面");
        }
        // 获取当前属性，应用修改闭包，然后更新页表项
        let mut prop = pte.prop();
        op(&mut prop);
        unsafe { pte_ptr.write(E::new_page(pte.paddr(), 1, prop)) };
    }

    /// 分配一个新的子页表帧并创建指向它的页表项。
    ///
    /// 此方法分配一个新的物理帧用作页表，将其清零，并创建一个指向该帧的页表项。
    /// 重要的是，该方法会在页表项中设置 `PageFlags::AVAIL1` 标志，以标记这是由
    /// 引导页表分配的帧，而不是由固件或加载器预先设置的。这种标记对于资源管理
    /// 至关重要，确保在引导页表被销毁时能够正确释放这些帧。
    ///
    /// # 返回值
    ///
    /// 返回一个指向新分配页表帧的页表项，带有 `PageFlags::AVAIL1` 标志。
    fn alloc_child(&mut self) -> E {
        let frame = FRAME_ALLOCATOR.get().unwrap().lock().alloc(1).unwrap();
        // 清零新分配的页表帧
        let vaddr = paddr_to_vaddr(frame * PAGE_SIZE) as *mut u8;
        unsafe { core::ptr::write_bytes(vaddr, 0, PAGE_SIZE) };

        // 创建指向新页表帧的页表项，并标记为引导页表分配的资源
        let mut pte = E::new_pt(frame * C::BASE_PAGE_SIZE);
        let prop = pte.prop();
        pte.set_prop(PageProperty::new(prop.flags | PageFlags::AVAIL1));

        pte
    }
}

/// 深度优先遍历页表结构，在离开节点时执行操作。
///
/// 这是一个后序遍历算法，用于遍历多级页表结构。与普通DFS不同，
/// 操作会在处理完所有子节点后、离开当前节点时执行，这种模式特别适合
/// 资源释放和清理工作。
///
/// # 算法特点
///
/// - **后序遍历**：先递归处理所有子页表，再处理当前页表项
/// - **仅处理中间节点**：只遍历指向下级页表的页表项，不处理叶子节点（直接映射物理页面的条目）
/// - **支持DAG结构**：可以处理可能是有向无环图(DAG)而非严格树形的页表结构
///
/// # 参数
///
/// - `pt`: 当前页表帧的编号（物理地址除以页大小）
/// - `level`: 当前页表的级别（如x86_64中，PML4=4, PDPT=3, PD=2, PT=1）
/// - `op`: 在离开页表节点时执行的闭包，接收指向该节点的页表项的可变引用
///
/// # 使用场景
///
/// 1. 初始化时清除页表中的特定标志位（如 `PageFlags::AVAIL1`）
/// 2. 释放引导页表分配的页表帧（通过检查 `PageFlags::AVAIL1` 标志）
/// 3. 在销毁页表时进行资源清理
fn dfs_walk_on_leave<E: PageTableEntryTrait, C: PagingConstsTrait>(
    pt: FrameNumber,
    level: PagingLevel,
    op: &mut impl FnMut(&mut E),
) {
    // 只处理非叶子页表（级别≥2）
    if level >= 2 {
        // 将物理帧号转换为虚拟地址，并创建页表项切片
        let pt_vaddr = paddr_to_vaddr(pt * C::BASE_PAGE_SIZE) as *mut E;
        let pt = unsafe { core::slice::from_raw_parts_mut(pt_vaddr, nr_subpage_per_huge::<C>()) };

        // 遍历页表中的所有条目
        for pte in pt {
            // 只处理存在的且指向下级页表的条目
            if pte.is_present() && !pte.is_last(level) {
                // 递归处理子页表
                dfs_walk_on_leave::<E, C>(pte.paddr() / C::BASE_PAGE_SIZE, level - 1, op);
                // 在处理完子页表后，对当前页表项执行操作
                op(pte)
            }
        }
    }
}

impl<E: PageTableEntryTrait, C: PagingConstsTrait> Drop for BootPageTable<E, C> {
    fn drop(&mut self) {
        dfs_walk_on_leave::<E, C>(self.root_pt, C::NR_LEVELS, &mut |pte| {
            // 检查页表项是否带有AVAIL1标志，这表明它是由引导页表分配的
            // 只释放引导页表自己分配的页表帧，保留固件或加载器提供的页表结构
            if pte.prop().flags.contains(PageFlags::AVAIL1) {
                let pt = pte.paddr() / C::BASE_PAGE_SIZE;
                FRAME_ALLOCATOR.get().unwrap().lock().dealloc(pt, 1);
            }
            // 固件提供的页表可能是DAG而非树。
            // 清除它以避免第二次遇到时重复释放。
            *pte = E::new_absent();
        });
    }
}

#[cfg(ktest)]
use crate::prelude::*;

#[cfg(ktest)]
#[ktest]
fn test_boot_pt_map_protect() {
    use super::page_walk;
    use crate::{
        arch::mm::{PageTableEntry, PagingConsts},
        mm::{
            frame::allocator::FrameAllocOptions,
            page_prop::{CachePolicy, PageFlags},
        },
    };

    let root_frame = FrameAllocOptions::new().alloc_frame().unwrap();
    let root_paddr = root_frame.start_paddr();

    let mut boot_pt = BootPageTable::<PageTableEntry, PagingConsts> {
        root_pt: root_paddr / PagingConsts::BASE_PAGE_SIZE,
        _pretend_to_use: core::marker::PhantomData,
    };

    let from1 = 0x1000;
    let to1 = 0x2;
    let prop1 = PageProperty::new(PageFlags::RW, CachePolicy::Writeback);
    unsafe { boot_pt.map_base_page(from1, to1, prop1) };
    assert_eq!(
        unsafe { page_walk::<PageTableEntry, PagingConsts>(root_paddr, from1 + 1) },
        Some((to1 * PAGE_SIZE + 1, prop1))
    );
    unsafe { boot_pt.protect_base_page(from1, |prop| prop.flags = PageFlags::RX) };
    assert_eq!(
        unsafe { page_walk::<PageTableEntry, PagingConsts>(root_paddr, from1 + 1) },
        Some((
            to1 * PAGE_SIZE + 1,
            PageProperty::new(PageFlags::RX, CachePolicy::Writeback)
        ))
    );

    let from2 = 0x2000;
    let to2 = 0x3;
    let prop2 = PageProperty::new(PageFlags::RX, CachePolicy::Uncacheable);
    unsafe { boot_pt.map_base_page(from2, to2, prop2) };
    assert_eq!(
        unsafe { page_walk::<PageTableEntry, PagingConsts>(root_paddr, from2 + 2) },
        Some((to2 * PAGE_SIZE + 2, prop2))
    );
    unsafe { boot_pt.protect_base_page(from2, |prop| prop.flags = PageFlags::RW) };
    assert_eq!(
        unsafe { page_walk::<PageTableEntry, PagingConsts>(root_paddr, from2 + 2) },
        Some((
            to2 * PAGE_SIZE + 2,
            PageProperty::new(PageFlags::RW, CachePolicy::Uncacheable)
        ))
    );
}
