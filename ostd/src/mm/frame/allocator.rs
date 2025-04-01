// SPDX-License-Identifier: MPL-2.0

//! 物理内存分配器。

use align_ext::AlignExt;
use buddy_system_allocator::FrameAllocator;
use log::info;
use spin::Once;

use super::{segment::Segment, Frame, Untyped};
use crate::{
    mm::{boot::MemoryRegionType, kspace::paddr_to_vaddr, PAGE_SIZE},
    sync::SpinLock,
    Error, Result,
};

/// 分配物理内存帧的选项。
pub struct FrameAllocOptions {
    zeroed: bool,
}

impl Default for FrameAllocOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameAllocOptions {
    /// 创建用于分配指定数量帧的新选项。
    pub fn new() -> Self {
        Self { zeroed: true }
    }

    /// 设置分配的帧是否应该用零初始化。
    ///
    /// 如果 `zeroed` 为 `true`，分配的帧将填充零。
    /// 如果不是，分配的帧将包含敏感数据，调用者应该在与其他组件共享之前清除它们。
    ///
    /// 默认情况下，帧会被零初始化。
    pub fn zeroed(&mut self, zeroed: bool) -> &mut Self {
        self.zeroed = zeroed;
        self
    }

    /// 分配一个非类型化帧。
    pub fn alloc_frame(&self) -> Result<Frame<Untyped>> {
        let frame = FRAME_ALLOCATOR
            .get()
            .unwrap()
            .disable_irq()
            .lock()
            .alloc(1)
            .map(|idx| {
                let paddr = idx * PAGE_SIZE;
                Frame::from_unused(paddr)
            })
            .ok_or(Error::NoMemory)?;

        if self.zeroed {
            let addr = paddr_to_vaddr(frame.start_paddr()) as *mut u8;
            // SAFETY: The newly allocated frame is guaranteed to be valid.
            unsafe { core::ptr::write_bytes(addr, 0, PAGE_SIZE) }
        }

        Ok(frame)
    }

    /// 分配一个连续的非类型化帧范围。
    pub fn alloc_segment(&self, nframes: usize) -> Result<Segment<Untyped>> {
        self.alloc_segment_with(nframes, |_| ())
    }
}

// #[cfg(ktest)]
// #[ktest]
// fn test_alloc_dealloc() {
//     // 这里我们以随机顺序分配和释放帧来测试分配器。
//     // 如果底层实现出现异常，我们预期测试会失败。
//     let single_options = FrameAllocOptions::new();
//     let mut contiguous_options = FrameAllocOptions::new();
//     contiguous_options.zeroed(false);
//     let mut remember_vec = Vec::new();
//     for _ in 0..10 {
//         for i in 0..10 {
//             let single_frame = single_options.alloc_frame().unwrap();
//             if i % 3 == 0 {
//                 remember_vec.push(single_frame);
//             }
//         }
//         let contiguous_segment = contiguous_options.alloc_segment(10).unwrap();
//         drop(contiguous_segment);
//         remember_vec.pop();
//     }
// }

/// 带有已分配内存计数器的帧分配器
pub(crate) struct CountingFrameAllocator {
    allocator: FrameAllocator,
    total: usize,
    allocated: usize,
}

impl CountingFrameAllocator {
    pub fn new(allocator: FrameAllocator, total: usize) -> Self {
        CountingFrameAllocator {
            allocator,
            total,
            allocated: 0,
        }
    }

    pub fn alloc(&mut self, count: usize) -> Option<usize> {
        match self.allocator.alloc(count) {
            Some(value) => {
                self.allocated += count * PAGE_SIZE;
                Some(value)
            }
            None => None,
        }
    }

    // TODO: 这个方法应该被标记为 unsafe，因为无效的参数会破坏底层分配器。
    pub fn dealloc(&mut self, start_frame: usize, count: usize) {
        self.allocator.dealloc(start_frame, count);
        self.allocated -= count * PAGE_SIZE;
    }

    pub fn mem_total(&self) -> usize {
        self.total
    }

    pub fn mem_available(&self) -> usize {
        self.total - self.allocated
    }
}

pub(crate) static FRAME_ALLOCATOR: Once<SpinLock<CountingFrameAllocator>> = Once::new();

pub(crate) fn init() {
    let regions = &crate::boot::EARLY_INFO.get().unwrap().memory_regions;
    let mut total: usize = 0;
    let mut allocator = FrameAllocator::<32>::new();
    for region in regions.iter() {
        if region.typ() == MemoryRegionType::Usable {
            // 使内存区域页面对齐，如果太小则跳过。
            let start = region.base().align_up(PAGE_SIZE) / PAGE_SIZE;
            let region_end = region.base().checked_add(region.len()).unwrap();
            let end = region_end.align_down(PAGE_SIZE) / PAGE_SIZE;
            if end <= start {
                continue;
            }
            // 将全局空闲页面添加到帧分配器。
            allocator.add_frame(start, end);
            total += (end - start) * PAGE_SIZE;
            info!(
                "找到可用区域，起始:{:x}，结束:{:x}",
                region.base(),
                region.base() + region.len()
            );
        }
    }
    let counting_allocator = CountingFrameAllocator::new(allocator, total);
    FRAME_ALLOCATOR.init(|| SpinLock::new(counting_allocator));
}
