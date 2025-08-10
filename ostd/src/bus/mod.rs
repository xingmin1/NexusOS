// SPDX-License-Identifier: MPL-2.0

//! 设备总线：负责发现并初始化 VirtIO 设备（MMIO / PCI）。

pub mod mmio;
pub mod pci;
pub mod virtio_devices;
pub mod device_manager;
pub mod init;

/// An error that occurs during bus probing.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BusProbeError {
    /// The device does not match the expected criteria.
    DeviceNotMatch,
    /// An error in accessing the configuration space of the device.
    ConfigurationSpaceError,
}

/// 初始化设备总线（RISC-V / LoongArch 从设备树发现 MMIO / PCI VirtIO 设备）
pub(crate) fn init() {
    #[cfg(any(target_arch = "riscv64", target_arch = "loongarch64"))]
    {
        crate::bus::init::init();
    }
}
