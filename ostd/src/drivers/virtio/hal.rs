// SPDX-License-Identifier: MPL-2.0
//! VirtIO‑drivers ↔ OSTD (RISC‑V) HAL ‑‑ *no‑IOMMU* version.

use alloc::{collections::BTreeMap};
use core::{
    ptr::{self, NonNull},
};

use spin::Mutex;
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

use crate::mm::{
    dma::{DmaDirection, DmaStream},
    frame::{allocator::FrameAllocOptions, Segment},
    kspace::{paddr_to_vaddr, LINEAR_MAPPING_BASE_VADDR},
    HasPaddr,
};

/// 内部状态：把 paddr 映射到“要不要做 bounce 以及附带数据”。
enum Mapping {
    /// 需要回写的 bounce‑buffer。
    Bounce {
        stream: DmaStream,
        orig_ptr: *mut u8,
        len: usize,
        dir: BufferDirection,
    },
    /// 直接映射，无需任何后续处理。
    Direct,
}

// 仅传递裸地址，受互斥锁保护，声明为线程安全
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

static MAP: Mutex<BTreeMap<PhysAddr, Mapping>> = Mutex::new(BTreeMap::new());

/// VirtIO-HAL 实现
pub struct RiscvHal;

/* -------------------------------------------------------------
 *  helper：检查地址是否落在内核线性映射并保持物理连续（一页内）
 * -----------------------------------------------------------*/
fn fast_path_candidate(v: usize, len: usize) -> Option<PhysAddr> {
    if !(LINEAR_MAPPING_BASE_VADDR..).contains(&v) {
        return None;
    }
    let offset = v & (PAGE_SIZE - 1);
    if offset + len > PAGE_SIZE {
        return None; // 跨页则可能不连续
    }
    Some(v - LINEAR_MAPPING_BASE_VADDR)
}

unsafe impl Hal for RiscvHal {
    /* ============== 1. 长期固定 DMA 区 ============== */
    fn dma_alloc(pages: usize, _dir: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let segment: Segment<()> = FrameAllocOptions::new()
            .alloc_segment(pages)
            .expect("DMA OOM");
        let paddr = segment.start_paddr();
        let vaddr = paddr_to_vaddr(paddr);
        // 让 Segment 在 HAL 内部就被忘掉：VirtIO 驱动负责在 `dma_dealloc`
        // 时归还，我们只需把它丢进 Box 以便 drop。
        MAP.lock().insert(
            paddr,
            Mapping::Bounce /*借用此 enum 因为也要在 dealloc 时释放*/{
                stream: {
                    // 借助 DmaStream 完成 cache 属性转换；方向无关紧要
                    let us = segment.into();
                    DmaStream::map(us, DmaDirection::Bidirectional, false).unwrap()
                },
                orig_ptr: core::ptr::null_mut(),
                len: pages * PAGE_SIZE,
                dir: BufferDirection::Both,
            },
        );
        (paddr, NonNull::new(vaddr as *mut u8).unwrap())
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, _vaddr: NonNull<u8>, _pages: usize) -> i32 {
        MAP.lock().remove(&paddr); // drop 即释放
        0
    }

    /* ============== 2. MMIO ============== */
    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        // 0x8_0000_0000.. 的 I/O 区已在线性映射中标为 Uncacheable
        NonNull::new(paddr_to_vaddr(paddr) as *mut u8).unwrap()
    }

    /* ============== 3. share / unshare ============== */
    unsafe fn share(buf: NonNull<[u8]>, dir: BufferDirection) -> PhysAddr {
        let vaddr = buf.as_ptr().addr();
        let len = buf.len();

        // Fast‑path：页面内 + 内核线性映射
        if let Some(paddr) = fast_path_candidate(vaddr, len) {
            MAP.lock().insert(paddr, Mapping::Direct);
            return paddr;
        }

        /* ---------- 走 bounce‑buffer ---------- */
        let pages = (len + PAGE_SIZE - 1) / PAGE_SIZE;
        let segment = FrameAllocOptions::new()
            .alloc_segment(pages)
            .expect("bounce OOM");
        let us = segment.into();
        let dma_dir = match dir {
            BufferDirection::DriverToDevice => DmaDirection::ToDevice,
            BufferDirection::DeviceToDriver => DmaDirection::FromDevice,
            BufferDirection::Both => DmaDirection::Bidirectional,
        };
        let stream = DmaStream::map(us, dma_dir, false).unwrap();
        let paddr = stream.paddr();

        // 如需写入设备，先拷贝原数据
        if matches!(dir, BufferDirection::DriverToDevice | BufferDirection::Both) {
            ptr::copy_nonoverlapping(vaddr as *const u8, paddr_to_vaddr(paddr) as *mut u8, len);
        }

        MAP.lock().insert(
            paddr,
            Mapping::Bounce {
                stream,
                orig_ptr: vaddr as *mut u8,
                len,
                dir,
            },
        );

        paddr
    }

    unsafe fn unshare(paddr: PhysAddr, _buffer: NonNull<[u8]>, _dir: BufferDirection) {
        if let Some(entry) = MAP.lock().remove(&paddr) {
            if let Mapping::Bounce {
                stream,
                orig_ptr,
                len,
                dir,
            } = entry
            {
                // 设备→CPU 方向需要回拷
                if matches!(dir, BufferDirection::DeviceToDriver | BufferDirection::Both) {
                    ptr::copy_nonoverlapping(
                        paddr_to_vaddr(stream.paddr()) as *const u8,
                        orig_ptr,
                        len,
                    );
                }
                drop(stream); // 自动撤 cache 属性并释放 frames
            }
        }
    }
}
