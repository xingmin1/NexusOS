// SPDX-License-Identifier: MPL-2.0

//! VirtIO driver implementations leveraging the `virtio-drivers` crate.
//!
//! 驱动总览：
//! - 块设备：通过 `DEVICE_MANAGER` 获取（见 `block`）。
//! - 控制台：由 `DEVICE_MANAGER.console_device()` 获取，`ostd::console::print/println` 自动使用。
//! - 网络：提供基本设备访问；后续加入 `VirtIONetRaw` 的 smoltcp 适配封装。

pub mod hal;
pub mod block;
pub mod net;
