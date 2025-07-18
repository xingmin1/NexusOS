// SPDX-License-Identifier: MPL-2.0

//! VirtIO-Block MMIO 驱动，封装为 [`MmioDriver`] 供总线自动探测。

use alloc::{collections::btree_map::BTreeMap, string::{String, ToString}, sync::Arc};
use maitake::sync::Mutex;
use ostd_macros::ktest;
use core::ptr::NonNull;

use crate::{
    bus::{mmio::{bus::{MmioDevice, MmioDriver}, common_device::MmioCommonDevice}, BusProbeError}, io_mem::IoMem, task::scheduler::blocking_future::BlockingFuture, trap::IrqLine
};

use virtio_drivers::{device::blk::VirtIOBlk, transport::{mmio::{MmioTransport, VirtIOHeader}, DeviceType, Transport}};

use super::hal::RiscvHal;

static BLOCK_DEVICE_TABLE: Mutex<BTreeMap<String, Arc<Mutex<VirtIOBlk<RiscvHal, MmioTransport<'static>>>>>> = Mutex::new(BTreeMap::new());

/// 目前仅支持 VirtIO-Block（device_id == 2）。
#[derive(Debug)]
pub struct VirtioBlkDriver;

impl VirtioBlkDriver {
    /// Creates a new driver instance.
    pub const fn new() -> Self {
        Self
    }
}

impl MmioDriver for VirtioBlkDriver {
    fn probe(&self, common: MmioCommonDevice) -> core::result::Result<Arc<dyn MmioDevice>, (BusProbeError, MmioCommonDevice)> {
        let device_id = match common.read_device_id() {
            Ok(id) => id,
            Err(_) => return Err((BusProbeError::ConfigurationSpaceError, common)),
        };

        // Clone IoMem & Irq to move into our own struct.
        let io_mem: IoMem = common.io_mem().clone();
        let irq_line: IrqLine = common.irq().clone();
        let paddr = io_mem.paddr();

        // SAFETY: 已确认这是 virtio-mmio header 区域，长度固定 0x200。
        let header_ptr = unsafe { NonNull::new_unchecked(crate::mm::paddr_to_vaddr(paddr) as *mut VirtIOHeader) };
        let transport = match unsafe { MmioTransport::new(header_ptr, io_mem.length()) } {
            Ok(t) => t,
            Err(_) => return Err((BusProbeError::ConfigurationSpaceError, common)),
        };

        // 再次确认类型
        if transport.device_type() != DeviceType::Block {
            // 不匹配则返回错误
            return Err((BusProbeError::DeviceNotMatch, common));
        }

        // 构造 VirtIO-Blk 设备
        let mut blk = match VirtIOBlk::<RiscvHal, _>::new(transport) {
            Ok(b) => b,
            Err(_) => return Err((BusProbeError::ConfigurationSpaceError, common)),
        };

        // 注册设备
        let mut block_device_table = BLOCK_DEVICE_TABLE.lock().block();
        let mut device_name = [0; 20];
        let len = blk.device_id(&mut device_name).unwrap();
        let device_name = if len > 0 {
            String::from_utf8_lossy(&device_name[..len]).to_string()
        } else {
            "block_device".to_string()
        };
        let blk = Arc::new(Mutex::new(blk));
        // [TODO]: 如果有多个设备，且都没有名称，会覆盖
        block_device_table.insert(device_name, blk.clone());

        // 使用 spin::Mutex 包装，便于中断回调访问
        let device_inner = Arc::new(VirtioBlkDevice {
            _blk: blk.clone(),
            device_id,
            _io_mem: io_mem.clone(),
            _irq: irq_line.clone(),
        });

        // 中断回调：收到 IRQ 后 ack && 触发 blk 驱动内部处理
        let weak_dev = Arc::downgrade(&blk);
        let mut irq_line_mut = irq_line.clone();
        irq_line_mut.on_active(move |_| {
            if let Some(dev) = weak_dev.upgrade() {
                let mut guard = dev.lock().block();
                let _ = guard.ack_interrupt();
            }
        });

        // 返回作为 MmioDevice
        Ok(device_inner)
    }
}

/// 获取 VirtIO-Blk 设备
pub async fn get_block_device(device_name: &str) -> Option<Arc<Mutex<VirtIOBlk<RiscvHal, MmioTransport<'static>>>>> {
    let block_device_table = BLOCK_DEVICE_TABLE.lock().await;
    block_device_table.get(device_name).cloned()
}

/// 已初始化并可供系统使用的 VirtIO-Blk 设备实现。
struct VirtioBlkDevice {
    _blk: Arc<Mutex<VirtIOBlk<RiscvHal, MmioTransport<'static>>>>,
    device_id: u32,
    // 在结构体里持有这些对象，确保生命周期及映射有效
    _io_mem: IoMem,
    _irq: IrqLine,
}

impl MmioDevice for VirtioBlkDevice {
    fn device_id(&self) -> u32 {
        self.device_id
    }
}

// [TODO]: 未来可在 `VirtioBlkDevice` 里缓存 `DmaStreamSlice` 用于请求描述符，
//         按 `DmaDirection` 决定同步策略（ToDevice/FromDevice/Bidirectional）。 

#[cfg(ktest)]
mod test {
    use crate::task::scheduler::blocking_future::BlockingFuture;

    use super::*;

    #[ktest]
    fn test_list_block_device() {
        let _span = tracing::info_span!("list_block_device");
        let block_device_table = BLOCK_DEVICE_TABLE.lock().block();
        tracing::info!("block_device count: {}", block_device_table.len());
        for (name, blk) in block_device_table.iter() {
            tracing::info!("{}", name);
            
            // 测试读写
            let mut blk = blk.lock().block();
            let mut raw_buf = [0; 512];
            blk.read_blocks(0, &mut raw_buf).unwrap();
            tracing::info!("read: {:?}", raw_buf);

            let write_buf = [1; 512];
            blk.write_blocks(0, &write_buf).unwrap();
            tracing::info!("write: {:?}", write_buf);
            blk.flush().unwrap();
            let mut read_buf = [0; 512];
            blk.read_blocks(0, &mut read_buf).unwrap();
            assert_eq!(read_buf, write_buf);
            
            // 还原
            blk.write_blocks(0, &raw_buf).unwrap();
            blk.flush().unwrap();
        }
    }
}
