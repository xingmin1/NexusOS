// kernel/drivers/virtio-blk/src/device.rs
// SPDX-License-Identifier: MPL-2.0

use alloc::boxed::Box;
use core::{hint::spin_loop, mem::size_of};

use block_dev::{BlockDevice, BlockError, SECTOR_SIZE};
use int_to_c_enum::TryFromInt;
use log::{debug, info};
use ostd::{
    mm::{DmaDirection, DmaStream, DmaStreamSlice, FrameAllocOptions},
    offset_of,
    sync::SpinLock,
};
/// VirtIO 传输层（MMIO/PCI）抽象。
use virtio_transport::{DeviceStatus, VirtioTransport};

use crate::queue::{QueueError, SimpleVirtQueue};

/// VirtIO‑Blk 请求类型
#[repr(u32)]
#[derive(Debug, Copy, Clone, TryFromInt)]
enum ReqType {
    In = 0,
    Out = 1,
    Flush = 4,
    GetID = 8,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct BlkReqHeader {
    req_type: u32,
    _reserved: u32,
    sector: u64,
}

const REQ_HEADER_SIZE: usize = size_of::<BlkReqHeader>();
const REQ_STATUS_SIZE: usize = 1; // status 字节
const DMA_PAGE_ORDER: usize = 1; // 2 pages => 足够 4K ext4 块

/// **同步** VirtIO‑Blk 设备
pub struct VirtIOBlkDevice {
    transport: SpinLock<Box<dyn VirtioTransport>>,
    queue: SpinLock<SimpleVirtQueue>,
    capacity: u64,         // 以扇区计
    dma_region: DmaStream, // 一个 2*PAGE 大小的 bounce buffer
}

impl VirtIOBlkDevice {
    /// 探测 + 初始化（只处理 mmio‑virt 设备 0）
    pub fn try_init(mut transport: Box<dyn VirtioTransport>) -> Result<Self, BlockError> {
        // 1. 复位 & ACK
        transport
            .write_device_status(DeviceStatus::empty())
            .unwrap();
        transport
            .write_device_status(DeviceStatus::ACKNOWLEDGE | DeviceStatus::DRIVER)
            .unwrap();

        // 2. feature 协商（只保留我们支持的低 24 bit）
        let features = transport.read_device_features();
        // 不支持 MQ、discard、flush（先留空），直接全关：
        transport.write_driver_features(features & 0).unwrap();

        // 3. 设置队列（固定队列 0，64 desc）
        let queue =
            SimpleVirtQueue::new(0, 64, transport.as_mut()).map_err(|_| BlockError::IoError)?;

        // 4. capacity 从 config space 读取（legacy 低 32 + 高 32）
        let cap_lo: u32 = transport.read_config_legacy(0).unwrap(); // 假设 helper
        let cap_hi: u32 = transport.read_config_legacy(4).unwrap();
        let capacity = ((cap_hi as u64) << 32) | cap_lo as u64;

        info!(
            "virtio‑blk: capacity {} sectors (~{} MiB)",
            capacity,
            capacity * 512 / 1024 / 1024
        );

        // 5. 准备 DMA 区
        let seg = FrameAllocOptions::new()
            .alloc_segment(DMA_PAGE_ORDER)
            .unwrap();
        let dma = DmaStream::map(seg.into(), DmaDirection::Bidirectional, false).unwrap();

        // 6. DRIVER_OK
        transport
            .write_device_status(
                DeviceStatus::ACKNOWLEDGE | DeviceStatus::DRIVER | DeviceStatus::DRIVER_OK,
            )
            .unwrap();

        Ok(Self {
            transport: SpinLock::new(transport),
            queue: SpinLock::new(queue),
            capacity,
            dma_region: dma,
        })
    }

    /// 向 virtqueue 提交一次 **同步** 请求
    fn do_rw(&self, lba: u64, data: &mut [u8], req: ReqType) -> Result<(), BlockError> {
        assert_eq!(data.len() % SECTOR_SIZE, 0);

        let hdr_slice = DmaStreamSlice::new(&self.dma_region, 0, REQ_HEADER_SIZE);
        let data_slice = DmaStreamSlice::new(&self.dma_region, REQ_HEADER_SIZE, data.len());
        let stat_slice = DmaStreamSlice::new(
            &self.dma_region,
            REQ_HEADER_SIZE + data.len(),
            REQ_STATUS_SIZE,
        );

        // 写 header
        let header = BlkReqHeader {
            req_type: req as u32,
            _reserved: 0,
            sector: lba,
        };
        hdr_slice.write_val(0, &header).unwrap();
        hdr_slice.sync().unwrap();

        // 如果是写，把 payload 拷进去
        if matches!(req, ReqType::Out) {
            data_slice.write_bytes(0, data).unwrap();
        }
        data_slice.sync().unwrap();

        // status 置 0
        stat_slice.write_val(0, &0u8).unwrap();

        // 组成 DMA buf 列表
        let (inputs, outputs): (Vec<&DmaStreamSlice>, Vec<&DmaStreamSlice>) = match req {
            ReqType::In => (vec![&hdr_slice], vec![&data_slice, &stat_slice]),
            ReqType::Out => (vec![&hdr_slice, &data_slice], vec![&stat_slice]),
            _ => return Err(BlockError::Unsupported),
        };

        // 进入队列
        let token = self
            .queue
            .lock()
            .add_dma_buf(&inputs, &outputs)
            .map_err(|_| BlockError::IoError)?;

        // 通知
        if self.queue.lock().should_notify() {
            self.queue.lock().notify();
        }

        // busy‑wait 直到 used
        while !self.queue.lock().can_pop() {
            spin_loop();
        }
        let (_tok, _len) = self
            .queue
            .lock()
            .pop_used()
            .map_err(|_| BlockError::IoError)?;
        assert_eq!(token, _tok);

        // 检查 status
        stat_slice.sync().unwrap();
        let status: u8 = stat_slice.read_val(0).unwrap();
        if status != 0 {
            return Err(BlockError::IoError);
        }

        // 如果是读，把数据搬到 caller buf
        if matches!(req, ReqType::In) {
            data_slice.read_bytes(0, data).unwrap();
        }
        Ok(())
    }
}

impl BlockDevice for VirtIOBlkDevice {
    fn sectors(&self) -> u64 {
        self.capacity
    }

    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        self.do_rw(lba, buf, ReqType::In)
    }

    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), BlockError> {
        // 对齐检查
        let mut bounce = [0u8; 4096]; // 临时；大于 8K 时可循环
        assert!(buf.len() <= bounce.len());
        bounce[..buf.len()].copy_from_slice(buf);
        self.do_rw(lba, &mut bounce[..buf.len()], ReqType::Out)
    }
}
