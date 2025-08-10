// SPDX-License-Identifier: MPL-2.0

//! PCI BAR 分配器。
//!
//! - 从设备树 `ranges` 提取一组可用的“CPU 物理地址窗口”，每个窗口描述一段可分配的 PCI 内存空间；
//! - 关注 Memory 类型（32/64-bit）与 `prefetchable` 属性，尽量按请求匹配；
//! - 按请求大小做 2 的幂对齐分配，简单的 first-fit 策略即可满足 VirtIO 的需要；
//! - 返回用于写入 BAR 的“CPU 物理地址”（非 bus 地址）。

use alloc::vec::Vec;

use fdt::node::FdtNode;

/// PCI BAR 地址宽度
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AddressWidth {
    /// 32-bit
    Width32,
    /// 64-bit
    Width64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum PciRangeType {
    ConfigurationSpace,
    IoSpace,
    Memory32,
    Memory64,
}

impl From<u32> for PciRangeType {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::ConfigurationSpace,
            1 => Self::IoSpace,
            2 => Self::Memory32,
            3 => Self::Memory64,
            _ => Self::ConfigurationSpace,
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct BarWindow {
    cpu_base: u64,
    size: u64,
    next: u64,
    prefetchable: bool,
    width: AddressWidth,
}

impl BarWindow {
    fn new(cpu_base: u64, size: u64, prefetchable: bool, width: AddressWidth) -> Self {
        Self {
            cpu_base,
            size,
            next: cpu_base,
            prefetchable,
            width,
        }
    }

    #[allow(unused)]
    fn remaining(&self) -> u64 {
        self.cpu_base + self.size - self.next
    }

    fn allocate(&mut self, size: u64) -> Option<u64> {
        let size = next_power_of_two(size);
        let alloc = align_up(self.next, size);
        if alloc.checked_add(size)? > self.cpu_base + self.size {
            return None;
        }
        self.next = alloc + size;
        Some(alloc)
    }
}

/// BAR 分配器：维护多段窗口，根据属性进行匹配与分配。
pub struct BarAllocator {
    windows: Vec<BarWindow>,
}

impl BarAllocator {
    /// 从设备树 `ranges` 解析窗口。
    pub fn from_fdt_ranges(pci_node: &FdtNode) -> Option<Self> {
        let prop = pci_node.property("ranges")?;
        let bytes = prop.value;
        if bytes.len() % (7 * 4) != 0 {
            return None;
        }

        let mut windows = Vec::new();
        for chunk in bytes.chunks_exact(28) {
            // 7 u32 cells
            let w = |i: usize| -> u32 {
                u32::from_be_bytes([chunk[i], chunk[i + 1], chunk[i + 2], chunk[i + 3]])
            };
            let child_hi = w(0);
            let child_mid = w(4);
            let child_lo = w(8);
            let parent_hi = w(12);
            let parent_lo = w(16);
            let size_hi = w(20);
            let size_lo = w(24);

            let prefetchable = (child_hi & 0x4000_0000) != 0;
            let range_type = PciRangeType::from((child_hi & 0x0300_0000) >> 24);
            let bus_address = ((child_mid as u64) << 32) | (child_lo as u64);
            let cpu_physical = ((parent_hi as u64) << 32) | (parent_lo as u64);
            let size = ((size_hi as u64) << 32) | (size_lo as u64);

            // 仅接受 Memory 区域
            let width = match range_type {
                PciRangeType::Memory32 => AddressWidth::Width32,
                PciRangeType::Memory64 => AddressWidth::Width64,
                _ => continue,
            };

            // 出于简化：要求 bus 与 cpu 物理地址基址一致（常见平台满足），否则仍可使用，
            // 因为我们写入的是 CPU 物理地址，host bridge 负责翻译。
            // 这里仅记录日志，不强制要求 1:1。
            if bus_address != cpu_physical {
                log::debug!(
                    "non-identity PCI range: bus {:#x} != cpu {:#x}",
                    bus_address,
                    cpu_physical
                );
            }

            if size == 0 {
                continue;
            }

            windows.push(BarWindow::new(cpu_physical, size, prefetchable, width));
        }

        if windows.is_empty() {
            None
        } else {
            Some(Self { windows })
        }
    }

    /// 创建空的分配器（不会成功分配）。
    pub fn new_empty() -> Self {
        Self {
            windows: Vec::new(),
        }
    }

    /// 由固定窗口创建分配器（用于缺少 DT ranges 的平台回退）。
    pub fn from_fixed_window(
        start: u64,
        size: u64,
        prefetchable: bool,
        width: AddressWidth,
    ) -> Self {
        Self {
            windows: alloc::vec![BarWindow::new(start, size, prefetchable, width)],
        }
    }

    /// 分配一个 BAR：可指定宽度与是否希望 prefetchable（None 表示不关心）。
    pub fn allocate(
        &mut self,
        size: u64,
        width: AddressWidth,
        prefetch: Option<bool>,
    ) -> Option<u64> {
        // 1) 严格匹配：width + prefetchable
        for window in self.windows.iter_mut() {
            if window.width == width && prefetch.map_or(true, |p| p == window.prefetchable) {
                if let Some(addr) = window.allocate(size) {
                    return Some(addr);
                }
            }
        }
        // 2) 宽松匹配：仅匹配 width
        for window in self.windows.iter_mut() {
            if window.width == width {
                if let Some(addr) = window.allocate(size) {
                    return Some(addr);
                }
            }
        }
        // 3) 最宽松：忽略 width（例如 64‑bit BAR 也可放在 32‑bit 窗口内，只要地址写入有效）
        for window in self.windows.iter_mut() {
            if let Some(addr) = window.allocate(size) {
                return Some(addr);
            }
        }
        None
    }
}

const fn align_up(value: u64, alignment: u64) -> u64 {
    ((value - 1) | (alignment - 1)) + 1
}

const fn next_power_of_two(mut x: u64) -> u64 {
    if x == 0 {
        return 1;
    }
    x -= 1;
    x |= x >> 1;
    x |= x >> 2;
    x |= x >> 4;
    x |= x >> 8;
    x |= x >> 16;
    x |= x >> 32;
    x + 1
}
