// SPDX-License-Identifier: MPL-2.0

//! VirtIO-Block 驱动包装：通过全局 `DEVICE_MANAGER` 获取已创建设备。

use alloc::sync::Arc;
use spin::Mutex as SpinMutex;
use virtio_drivers::device::blk::VirtIOBlk;

use crate::{bus::device_manager::DEVICE_MANAGER, drivers::virtio::hal::RiscvHal};

/// 返回当前系统中第一个块设备（若存在）。
pub fn first_block_device() -> Option<Arc<SpinMutex<VirtIOBlk<RiscvHal, virtio_drivers::transport::SomeTransport<'static>>>>> {
    let mgr = DEVICE_MANAGER.lock();
    mgr.block_devices().into_iter().next()
}
