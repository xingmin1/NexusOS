// SPDX-License-Identifier: MPL-2.0

//! 物理帧的元数据管理。
//!
//! 你可以将其想象为一个全局共享的、静态的、巨大的元数据数组，
//! 为每个物理帧初始化。数组中的一个条目称为 [`MetaSlot`]，
//! 其中包含了一个物理帧的引用计数。
//!
//! # 实现
//!
//! 这些槽位被放置在映射到内核空间特定虚拟地址的元数据页中。
//! 因此查找物理帧的元数据通常没有额外开销，因为地址转换是一个简单的算术运算。

pub(crate) mod mapping {
    //! 每个物理页的元数据被线性映射到 [`FRAME_METADATA_RANGE`] 中的固定虚拟地址。

    use core::mem::size_of;

    use super::MetaSlot;
    use crate::{
        mm::{kspace::FRAME_METADATA_RANGE, PAGE_SIZE},
        prelude::{Paddr, Vaddr},
    };

    /// 将基础物理帧的物理地址转换为元数据槽的虚拟地址。
    pub(crate) const fn frame_to_meta(paddr: Paddr) -> Vaddr {
        let base = FRAME_METADATA_RANGE.start;
        let offset = paddr / PAGE_SIZE;
        base + offset * size_of::<MetaSlot>()
    }

    /// 将元数据槽的虚拟地址转换为物理帧的物理地址。
    pub(crate) const fn meta_to_frame(vaddr: Vaddr) -> Paddr {
        let base = FRAME_METADATA_RANGE.start;
        let offset = (vaddr - base) / size_of::<MetaSlot>();
        offset * PAGE_SIZE
    }
}

use core::sync::atomic::{AtomicU32, Ordering};

use align_ext::AlignExt;

use super::{allocator, Segment, Typed};
use crate::{
    const_assert,
    mm::{
        kspace::{paddr_to_vaddr, LINEAR_MAPPING_BASE_VADDR},
        page_prop::{PageFlags, PageProperty, PrivilegedPageFlags},
        page_table::boot_pt,
        PagingConsts, PAGE_SIZE,
    },
    prelude::{Paddr, Vaddr},
};

// 调整元数据槽大小为简单的原子引用计数大小，考虑对齐
pub const META_SLOT_SIZE: usize = 4; // 只需要存储AtomicU32

#[repr(C)]
pub(crate) struct MetaSlot {
    /// 页面的引用计数。
    ///
    /// 具体来说，引用计数具有以下含义：
    ///  - `REF_COUNT_UNUSED`：页面未被使用。
    ///  - `0`：页面正在构建（[`Frame::from_unused`]）
    ///    或正在析构（[`drop_last_in_place`]）。
    ///  - `1..REF_COUNT_MAX`：页面正在使用中。
    ///  - `REF_COUNT_MAX..REF_COUNT_UNUSED`：非法值，
    ///    用于防止引用计数溢出。否则，引用计数溢出将导致健全性问题。
    ///
    /// [`Frame::from_unused`]: super::Frame::from_unused
    pub(super) ref_count: AtomicU32,
}

pub(super) const REF_COUNT_UNUSED: u32 = u32::MAX;
const REF_COUNT_MAX: u32 = i32::MAX as u32;

const_assert!(PAGE_SIZE % META_SLOT_SIZE == 0);
const_assert!(size_of::<MetaSlot>() == META_SLOT_SIZE);

impl MetaSlot {
    /// 将帧引用计数增加一。
    ///
    /// # 安全性
    ///
    /// 调用者必须已经持有对该帧的引用。
    pub(super) unsafe fn inc_ref_count(&self) {
        let last_ref_cnt = self.ref_count.fetch_add(1, Ordering::Relaxed);
        debug_assert!(last_ref_cnt != 0 && last_ref_cnt != REF_COUNT_UNUSED);

        if last_ref_cnt >= REF_COUNT_MAX {
            // 这遵循与 `Arc::clone` 实现相同的原则，以防止引用计数溢出。
            // 参见 <https://doc.rust-lang.org/std/sync/struct.Arc.html#method.clone>。
            panic!("frame reference count overflow");
        }
    }
}

/// 在析构实现中的内部例程。
///
/// # 安全性
///
/// 调用者应确保指针指向帧的元数据槽。该帧应该有且仅有最后一个句柄，并且该帧即将被丢弃。
pub(super) unsafe fn drop_last_in_place(ptr: *mut MetaSlot) {
    // 安全性：`ptr` 指向一个有效的 `MetaSlot`，它永远不会被可变借用，
    // 所以对它取一个不可变引用总是安全的。
    let slot = unsafe { &*ptr };

    // 这应该作为安全要求得到保证。
    debug_assert_eq!(slot.ref_count.load(Ordering::Relaxed), 0);

    let paddr = mapping::meta_to_frame::<PagingConsts>(ptr as Vaddr);

    // 由于现在帧仅有引用计数状态，使用Relaxed内存序即可
    // 原子操作本身已经保证了引用计数的一致性
    slot.ref_count.store(REF_COUNT_UNUSED, Ordering::Relaxed);

    // 释放帧。
    allocator::FRAME_ALLOCATOR
        .get()
        .unwrap()
        .lock()
        .dealloc(paddr / PAGE_SIZE, 1);
}

/// 持有帧元数据的帧的元数据。
#[derive(Debug, Default)]
pub struct MetaPageMeta {}

/// 初始化所有物理帧的元数据。
///
/// 该函数返回包含元数据的 `Frame` 列表。
pub(crate) fn init() -> Segment<Typed> {
    todo!()
    // let max_paddr = {
    //     let regions = &crate::boot::EARLY_INFO.get().unwrap().memory_regions;
    //     regions.iter().map(|r| r.base() + r.len()).max().unwrap()
    // };

    // info!("为物理内存初始化帧元数据，物理内存上限为 {:x}", max_paddr);

    // add_temp_linear_mapping(max_paddr);

    // super::MAX_PADDR.store(max_paddr, Ordering::Relaxed);

    // let tot_nr_frames = max_paddr / page_size::<PagingConsts>(1);
    // let (nr_meta_pages, meta_pages) = alloc_meta_frames(tot_nr_frames);

    // // 映射元数据帧。
    // boot_pt::with_borrow(|boot_pt| {
    //     for i in 0..nr_meta_pages {
    //         let frame_paddr = meta_pages + i * PAGE_SIZE;
    //         let vaddr = mapping::frame_to_meta::<PagingConsts>(0) + i * PAGE_SIZE;
    //         let prop = PageProperty {
    //             flags: PageFlags::RW,
    //             priv_flags: PrivilegedPageFlags::GLOBAL,
    //         };
    //         // 安全性：我们正在为内核进行元数据映射。
    //         unsafe { boot_pt.map_base_page(vaddr, frame_paddr / PAGE_SIZE, prop) };
    //     }
    // })
    // .unwrap();

    // // 现在元数据帧已经映射，我们可以初始化元数据。
    // Segment::from_unused(meta_pages..meta_pages + nr_meta_pages * PAGE_SIZE, |_| {
    //     MetaPageMeta {}
    // })
}

fn alloc_meta_frames(tot_nr_frames: usize) -> (usize, Paddr) {
    let nr_meta_pages = tot_nr_frames
        .checked_mul(size_of::<MetaSlot>())
        .unwrap()
        .div_ceil(PAGE_SIZE);
    let start_paddr = allocator::FRAME_ALLOCATOR
        .get()
        .unwrap()
        .lock()
        .alloc(nr_meta_pages)
        .unwrap()
        * PAGE_SIZE;

    let slots = paddr_to_vaddr(start_paddr) as *mut MetaSlot;

    // 用 `REF_COUNT_UNUSED` 的字节模式填充元数据帧。
    debug_assert_eq!(REF_COUNT_UNUSED.to_ne_bytes(), [0xff, 0xff, 0xff, 0xff]);
    // 安全性：`slots` 和长度是元数据帧的有效区域，
    // 这些区域将被视为元数据槽。字节模式对于引用计数的初始值是有效的。
    unsafe {
        core::ptr::write_bytes(
            slots as *mut u8,
            0xff,
            tot_nr_frames * size_of::<MetaSlot>(),
        );
    }

    (nr_meta_pages, start_paddr)
}

/// 为元数据帧添加临时线性映射。
///
/// 我们只假设引导页表包含 4G 线性映射。因此，如果物理内存很大，
/// 我们最终会耗尽用于初始化元数据的线性虚拟内存。
fn add_temp_linear_mapping(max_paddr: Paddr) {
    todo!("适配 riscv sv39 的线性映射");

    const PADDR4G: Paddr = 0x1_0000_0000;

    if max_paddr <= PADDR4G {
        return;
    }

    // TODO: 我们不知道分配器是否会从低到高分配。
    // 所以我们在引导页表中准备所有线性映射。希望这不会大幅拖慢引导性能。
    let end_paddr = max_paddr.align_up(PAGE_SIZE);
    let prange = PADDR4G..end_paddr;
    let prop = PageProperty {
        flags: PageFlags::RW,
        priv_flags: PrivilegedPageFlags::GLOBAL,
    };

    // 安全性：我们正在为内核进行线性映射。
    unsafe {
        boot_pt::with_borrow(|boot_pt| {
            for paddr in prange.step_by(PAGE_SIZE) {
                let vaddr = LINEAR_MAPPING_BASE_VADDR + paddr;
                boot_pt.map_base_page(vaddr, paddr / PAGE_SIZE, prop);
            }
        })
        .unwrap();
    }
}
