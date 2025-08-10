// SPDX-License-Identifier: MPL-2.0

//! VirtIO-Net 适配包装：提供获取设备与基础收发封装。

use alloc::sync::Arc;
use spin::Mutex as SpinMutex;
use virtio_drivers::device::net::VirtIONet;

use crate::{bus::device_manager::DEVICE_MANAGER, drivers::virtio::hal::RiscvHal};

/// 返回第一个网络设备（若存在）。
pub fn first_net_device() -> Option<Arc<SpinMutex<VirtIONet<RiscvHal, virtio_drivers::transport::SomeTransport<'static>, 32>>>> {
    DEVICE_MANAGER.lock().network_devices().into_iter().next()
}


