// SPDX-License-Identifier: MPL-2.0

//! Utilities for printing page‑table structures at run‑time.


use core::fmt::Write;

use crate::{
    early_print, mm::{
        nr_subpage_per_huge, paddr_to_vaddr, page_size, page_table::PageTableEntryTrait, PagingConstsTrait, PagingLevel, Vaddr
    }, prelude::Paddr
};

/// 打印整棵页表或其中一段区间。
///
/// * `root_paddr` —— 顶级页表物理地址；通常来自 `PageTable::root_paddr()`  
/// * `range`      —— 需要关注的虚拟地址区间（闭区间左闭右开）；若要整棵树传 `0..usize::MAX`  
/// * `out`        —— 任意实现 `core::fmt::Write` 的目标（串口、日志缓冲等）  
/// 
/// # Safety
/// 和 `page_walk` 一样，本函数在内核态对物理地址做裸指针解引用；
/// 调用者必须保证页表所在物理页在内核虚拟空间中是恒久映射的。
///
/// 调用示例：
/// ```ignore
/// let root = KERNEL_PAGE_TABLE.get().unwrap().root_paddr();
/// unsafe { dump_page_table::<PageTableEntry, PagingConsts>(root, 0..0x8000_0000, &mut serial) };
/// ```
pub unsafe fn dump_page_table<
    E: PageTableEntryTrait,
    C: PagingConstsTrait,
    R: core::ops::RangeBounds<Vaddr>,
>(
    root_paddr: Paddr,
    range: R,
) {
    // 转成绝对起止；闭区间左闭右开
    let start = match range.start_bound() {
        core::ops::Bound::Included(&v) | core::ops::Bound::Excluded(&v) => v,
        core::ops::Bound::Unbounded => 0,
    };
    let end = match range.end_bound() {
        core::ops::Bound::Included(&v) => v.saturating_add(1),
        core::ops::Bound::Excluded(&v) => v,
        core::ops::Bound::Unbounded => usize::MAX,
    };

    early_print!("[dump_page_table] start={:#x}, end={:#x}, root_paddr={:#x}\n", start, end, root_paddr);

    // 借鉴 `page_walk`，但遍历整张表而非单点查询
    unsafe fn dfs<E: PageTableEntryTrait, C: PagingConstsTrait>(
        pt_pa: Paddr,
        level: PagingLevel,
        va_base: Vaddr,
        start: Vaddr,
        end: Vaddr,
        indent: usize,
    ) {
        let pt_va = paddr_to_vaddr(pt_pa) as *const E;
        let stride = page_size::<C>(level);
        let entries = nr_subpage_per_huge::<C>();

        for idx in 0..entries {
            let mut child_va_base = va_base + idx * stride;
            if level == C::NR_LEVELS {
                // 对齐不同架构的“规范地址”规则：
                // - x86_64 / riscv64：需要对高半区做符号扩展；
                // - loongarch64：低半区不应做符号扩展（按原始 48bit 处理）。
                #[cfg(not(target_arch = "loongarch64"))]
                {
                    // va_base is initially 0, so for canonical address spaces, we need to sign-extend
                    // the address if it's in the upper half.
                    // We assume 12 bits for page offset and 9 bits per level.
                    const PAGE_BITS: u32 = 12;
                    let va_bits = PAGE_BITS + 9 * C::NR_LEVELS as u32;
                    if va_bits < 64 {
                        let sign_bit = 1 << (va_bits - 1);
                        if (child_va_base & sign_bit) != 0 {
                            tracing::info!("sign_bit: {:#x}", sign_bit);
                            child_va_base |= !((1_usize << va_bits) - 1);
                        }
                    }
                }
            }
            // crate::prelude::println!("child_va_base: {:#x}", child_va_base);
            // 与关注区间无交集则跳过，避免无意义的输出
            if child_va_base >= end || child_va_base.checked_add(stride).unwrap_or(usize::MAX) <= start {
                continue;
            }

            let pte = pt_va.add(idx).read();

            if !pte.is_present() {
                // continue;
            }

            // 基础行：层次、索引、VA 片段、目标 PA、属性
            let _ = crate::prelude::println!(
                "{:indent$}L{}[{:03}] VA={:#016x} → PA={:#016x} {:?}",
                "",
                level,
                idx,
                child_va_base,
                pte.paddr(),
                pte.prop(),
                indent = indent
            );

            // 非叶子则递归
            if !pte.is_last(level) {
                dfs::<E, C>(
                    pte.paddr(),
                    level - 1,
                    child_va_base,
                    start,
                    end,
                    indent + 2,
                );
            }
        }
    }

    dfs::<E, C>(root_paddr, C::NR_LEVELS, 0, start, end, 0);
}
