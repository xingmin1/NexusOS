// kernel/drivers/virtio-blk/src/queue.rs
#![allow(dead_code)]

use core::{
    mem::size_of,
    ptr::{read_volatile, write_volatile},
};

use log::error;
use ostd::{
    mm::{DmaStreamSlice, FrameAllocOptions},
    offset_of,
    sync::SpinLock,
};

use crate::device::DMAPage; // 若放在同一 crate，请自行调整路径

/// 队列错误
#[derive(Debug)]
pub enum QueueError {
    NoDesc,
    Invalid,
}

/// 与 virtio spec 一致的描述符
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Desc {
    addr: u64,
    len:  u32,
    flags: u16,
    next:  u16,
}

const VIRTQ_DESC_F_NEXT  : u16 = 1;
const VIRTQ_DESC_F_WRITE : u16 = 2;

/// avail ring 头
#[repr(C)]
struct AvailRingHeader {
    flags: u16,
    idx:   u16,
}

/// used ring elem
#[repr(C)]
struct UsedElem {
    id:  u32,
    len: u32,
}

/// used ring 头
#[repr(C)]
struct UsedRingHeader {
    flags: u16,
    idx:   u16,
}

/// **非常**简单的 Virtqueue，只够同步块 I/O 使用
pub struct SimpleVirtQueue {
    size: u16, // 64
    /// DMA 区 (desc ‑ avail ‑ used) 连续布局
    dma: SpinLock<DMAPage>,
    last_used_idx: u16,
}

impl SimpleVirtQueue {
    pub fn new(index: u16, size: u16, transport: &mut dyn crate::device::VirtioTransport)
        -> Result<Self, QueueError>
    {
        assert_eq!(index, 0);
        assert!(size.is_power_of_two());

        // 申请一页足够：64 desc(16*64=1K) + avail(4*64≈260) + used(8*64=512) < 4K
        let seg = FrameAllocOptions::new().alloc_segment(1).unwrap();
        let dma = DMAPage::new(seg);

        // 配置设备
        transport.set_queue(index, size, &dma.desc_ptr(index), &dma.avail_ptr(), &dma.used_ptr())
            .map_err(|_| QueueError::Invalid)?;

        Ok(Self { size, dma: SpinLock::new(dma), last_used_idx: 0 })
    }

    /// 把 header+data+status 三 slice 组成一条链放入 avail，返回 token(desc idx)
    pub fn add_dma_buf(
        &mut self,
        inputs: &[&DmaStreamSlice],
        outputs: &[&DmaStreamSlice],
    ) -> Result<u16, QueueError> {
        let mut dma = self.dma.lock();
        let free_head = dma.alloc_desc().ok_or(QueueError::NoDesc)?;
        let mut cur = free_head;

        // Inputs
        for (i, slice) in inputs.iter().enumerate() {
            dma.init_desc(cur, slice, false);
            if i != inputs.len() - 1 || !outputs.is_empty() {
                let next = dma.alloc_desc().ok_or(QueueError::NoDesc)?;
                dma.set_next(cur, next, false);
                cur = next;
            }
        }
        // Outputs (device write‑back)
        for (i, slice) in outputs.iter().enumerate() {
            dma.init_desc(cur, slice, true);
            if i != outputs.len() - 1 {
                let next = dma.alloc_desc().ok_or(QueueError::NoDesc)?;
                dma.set_next(cur, next, true);
                cur = next;
            }
        }

        dma.push_avail(free_head);
        Ok(free_head)
    }

    /// 是否有已完成
    pub fn can_pop(&self) -> bool {
        let used_idx = unsafe { read_volatile(self.dma.lock().used_hdr().idx.as_ptr()) };
        used_idx != self.last_used_idx
    }

    /// 弹出 used
    pub fn pop_used(&mut self) -> Result<(u16, u32), QueueError> {
        let mut dma = self.dma.lock();
        let used_idx_ptr = &dma.used_hdr().idx as *const u16;
        let cur_idx = unsafe { read_volatile(used_idx_ptr) };
        if cur_idx == self.last_used_idx {
            return Err(QueueError::Invalid);
        }
        let slot = self.last_used_idx & (self.size - 1);
        let elem_ptr = dma.used_elem_ptr(slot as usize);
        let id = unsafe { read_volatile(&(*elem_ptr).id) } as u16;
        let len = unsafe { read_volatile(&(*elem_ptr).len) };
        dma.free_chain(id);
        self.last_used_idx = self.last_used_idx.wrapping_add(1);
        Ok((id, len))
    }

    pub fn should_notify(&self) -> bool {
        true // 为简化直接每次都通知
    }

    pub fn notify(&self) {
        // 此队列依赖外层 device 调用 transport.notify(); 已在调用方处理
    }
}

/* ---------- DMAPage 工具 ---------- */

/// 一页持续内存的 helper，负责 desc 链管理
pub struct DMAPage {
    stream: ostd::mm::DmaStream,
    free_head: u16,
    size: u16,
}

impl DMAPage {
    pub fn new(seg: ostd::mm::DmaSegment) -> Self {
        let stream = ostd::mm::DmaStream::map(seg.into(), ostd::mm::DmaDirection::Bidirectional, false).unwrap();
        let size = 64;
        // 初始化空闲链
        for i in 0..size {
            let desc_ptr = Self::desc_ptr_raw(&stream, i);
            unsafe { (*desc_ptr).flags = VIRTQ_DESC_F_NEXT };
            unsafe { (*desc_ptr).next  = (i + 1) % size };
        }
        Self { stream, free_head: 0, size }
    }

    #[inline]
    fn desc_ptr_raw(stream: &ostd::mm::DmaStream, idx: u16) -> *mut Desc {
        let base = stream.daddr();
        (base as *mut u8).wrapping_add(idx as usize * size_of::<Desc>()) as *mut Desc
    }

    fn desc_ptr(&self, idx: u16) -> *mut Desc {
        Self::desc_ptr_raw(&self.stream, idx)
    }

    pub fn desc_ptr(&self) -> ostd::mm::SafePtr<Desc, &ostd::mm::DmaStream> {
        unsafe { ostd::mm::SafePtr::new(&self.stream, 0).cast() }
    }

    pub fn avail_ptr(&self) -> ostd::mm::SafePtr<AvailRingHeader, &ostd::mm::DmaStream> {
        let off = size_of::<Desc>() * self.size as usize;
        unsafe { ostd::mm::SafePtr::new(&self.stream, off) }
    }

    pub fn used_ptr(&self) -> ostd::mm::SafePtr<UsedRingHeader, &ostd::mm::DmaStream> {
        let off = size_of::<Desc>() * self.size as usize +
                  size_of::<AvailRingHeader>() + 2 /*flags/idx*/ +
                  self.size as usize * size_of::<u16>();
        unsafe { ostd::mm::SafePtr::new(&self.stream, off) }
    }

    /* freelist 操作 ------------------------------------------------------ */

    fn alloc_desc(&mut self) -> Option<u16> {
        if self.free_head == 0xFFFF { return None; }
        let idx = self.free_head;
        let next = unsafe { (*self.desc_ptr(idx)).next };
        if next == idx { // only one left means empty after alloc
            self.free_head = 0xFFFF;
        } else {
            self.free_head = next;
        }
        Some(idx)
    }

    fn free_desc(&mut self, idx: u16) {
        unsafe { (*self.desc_ptr(idx)).flags = VIRTQ_DESC_F_NEXT };
        unsafe { (*self.desc_ptr(idx)).next  = self.free_head };
        self.free_head = idx;
    }

    fn free_chain(&mut self, mut head: u16) {
        loop {
            let flags = unsafe { (*self.desc_ptr(head)).flags };
            let next  = unsafe { (*self.desc_ptr(head)).next };
            self.free_desc(head);
            if flags & VIRTQ_DESC_F_NEXT == 0 { break; }
            head = next;
        }
    }

    /* desc 内容设置 ------------------------------------------------------ */

    fn init_desc(&self, idx: u16, slice: &DmaStreamSlice, write: bool) {
        let d = self.desc_ptr(idx);
        unsafe {
            (*d).addr = slice.daddr() as u64;
            (*d).len  = slice.nbytes() as u32;
            (*d).flags = if write { VIRTQ_DESC_F_WRITE } else { 0 };
            (*d).next  = 0;
        }
    }

    fn set_next(&self, idx: u16, next: u16, write_next: bool) {
        let d = self.desc_ptr(idx);
        unsafe {
            (*d).flags |= VIRTQ_DESC_F_NEXT;
            if write_next { (*d).flags |= VIRTQ_DESC_F_WRITE; }
            (*d).next = next;
        }
    }

    /* avail / used ------------------------------------------------------- */

    fn avail_hdr(&self) -> &mut AvailRingHeader {
        unsafe { &mut *self.avail_ptr().as_ptr() }
    }
    fn used_hdr(&self) -> &mut UsedRingHeader {
        unsafe { &mut *self.used_ptr().as_ptr() }
    }

    fn push_avail(&mut self, head: u16) {
        let hdr = self.avail_hdr();
        let ring_base = hdr as *mut _ as *mut u16; // flags, idx, then ring[0]
        let idx = hdr.idx;
        let slot = (idx & (self.size - 1)) as isize;
        unsafe {
            write_volatile(ring_base.offset(2 + slot), head); // 2 u16 offset for flags+idx
            hdr.idx = idx.wrapping_add(1);
        }
    }

    fn used_elem_ptr(&self, slot: usize) -> *mut UsedElem {
        let hdr = self.used_hdr() as *mut _ as *mut u8;
        unsafe { hdr.add(size_of::<UsedRingHeader>() + slot * size_of::<UsedElem>()) as *mut UsedElem }
    }
}
