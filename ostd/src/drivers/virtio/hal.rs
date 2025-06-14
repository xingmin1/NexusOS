// SPDX-License-Identifier: MPL-2.0

//! HAL implementation required by `virtio-drivers`.
//!
//! 目前仅在 RISC-V 平台测试，通过直接物理恒等映射实现。
//! 后续可根据不同架构增加 IOMMU、缓存一致性等处理。

use core::ptr::NonNull;
use virtio_drivers::{Hal, PhysAddr};
use virtio_drivers::BufferDirection;

use crate::mm::HasPaddr;
// 使用内核现有的 DMA 抽象，避免内存泄漏并简化缓存/一致性处理。
use crate::mm::{DmaStream, DmaDirection, paddr_to_vaddr, frame::allocator::FrameAllocOptions};

use alloc::collections::BTreeMap;
use crate::sync::GuardSpinLock;
use spin::Once;

// -----------------------------------------------------------------------------
//  全局表：追踪 `dma_alloc` 创建的 DMA 映射。
// -----------------------------------------------------------------------------

// [TODO]: 若支持多核并发更高，可改用更细粒度锁或 lock-free 结构。
static DMA_ALLOCS: Once<GuardSpinLock<BTreeMap<PhysAddr, DmaStream>>> = Once::new();

/// 简易 HAL。
///
/// - DMA 分配：直接向物理内存分配连续页，零填充。
/// - Dealloc: 当前实现直接忽略（泄漏），后续可补充记账释放。
/// - 地址转换：假设恒等映射。
pub struct HalImpl;

unsafe impl Hal for HalImpl {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        // SAFETY: `FrameAllocOptions` 已保证返回的段物理连续且页对齐。
        let pages = pages.max(1);

        // 1) 申请物理页段。
        let seg = FrameAllocOptions::new()
            .alloc_segment(pages)
            .expect("DMA frame alloc failed");

        // 2) 创建 streaming DMA 映射，记录方向。
        let direction = match _direction {
            BufferDirection::DriverToDevice => DmaDirection::ToDevice,
            BufferDirection::DeviceToDriver => DmaDirection::FromDevice,
            BufferDirection::Both => DmaDirection::Bidirectional,
        };

        // RISC-V virt 平台暂不支持硬件缓存一致性 => is_cache_coherent = false
        let dma = DmaStream::map(seg.into(), direction, /*is_cache_coherent=*/ false)
            .expect("DmaStream map failed");

        let paddr = dma.paddr() as PhysAddr;
        let vaddr = paddr_to_vaddr(paddr as usize) as *mut u8;

        // 3) 将映射存入全局表，供后续 `dma_dealloc` 释放。
        DMA_ALLOCS.call_once(|| GuardSpinLock::new(BTreeMap::new()));
        DMA_ALLOCS
            .get()
            .unwrap()
            .lock()
            .insert(paddr, dma);

        (paddr, NonNull::new(vaddr).unwrap())
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, _vaddr: NonNull<u8>, _pages: usize) -> i32 {
        // 从全局表移除，对象 Drop 时自动撤销 DMA 映射并归还物理页。
        if let Some(_stream) = DMA_ALLOCS
            .get()
            .expect("DMA_ALLOCS not initialized")
            .lock()
            .remove(&paddr)
        {
            0 // success
        } else {
            // [TODO]: 打印警告或记录调试信息
            -1 // not found
        }
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        NonNull::new(paddr_to_vaddr(paddr as usize) as *mut u8).unwrap()
    }

    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        buffer.as_ptr() as *mut u8 as usize        
    }

    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {
        // Nothing to do, as the host already has access to all memory and we didn't copy the buffer
        // anywhere else.
    }
}
