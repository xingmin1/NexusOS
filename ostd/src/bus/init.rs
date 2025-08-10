// SPDX-License-Identifier: MPL-2.0

//! 设备总线初始化：基于设备树发现 VirtIO MMIO 与 PCI 设备。

use virtio_drivers::transport::pci::{
    bus::{BarInfo, Cam, Command, MemoryBarType, MmioCam, PciRoot},
    virtio_device_type, PciTransport,
};

#[cfg(any(target_arch = "riscv64", target_arch = "loongarch64"))]
use crate::arch::boot::DEVICE_TREE;
use crate::{
    bus::{
        device_manager::DEVICE_MANAGER,
        pci::bar_alloc::{AddressWidth, BarAllocator},
    },
    io_mem::IoMem,
    mm::{paddr_to_vaddr, page_prop::CachePolicy, PageFlags},
    trap::IrqLine,
};
// use fdt::node::FdtNode;

/// 初始化设备总线（RISC-V / LoongArch）。
pub fn init() {
    log::info!("Initializing VirtIO bus (DT based)");
    #[cfg(any(target_arch = "riscv64", target_arch = "loongarch64"))]
    {
        if let Some(fdt) = DEVICE_TREE.get() {
            scan_virtio_mmio_devices(fdt);
            scan_pci_devices_from_fdt(fdt);
        } else {
            log::warn!("No device tree, skip bus init");
        }
    }
    log::info!("VirtIO bus init complete");
}

#[cfg(any(target_arch = "riscv64", target_arch = "loongarch64"))]
fn scan_virtio_mmio_devices(fdt: &fdt::Fdt) {
    let mut count = 0u32;
    for node in fdt.all_nodes() {
        if let Some(compatible) = node.compatible() {
            if compatible.all().any(|s| s == "virtio,mmio") {
                if let Some(mut reg) = node.reg() {
                    if let Some(r) = reg.next() {
                        let paddr = r.starting_address as usize;
                        let size = r.size.unwrap_or(0x200);
                        // 中断
                        let irq = parse_fdt_interrupt(&node);
                        DEVICE_MANAGER.lock().create_mmio_device(paddr, size, irq);
                        count += 1;
                    }
                }
            }
        }
    }
    log::info!("virtio-mmio devices: {}", count);
}

#[cfg(any(target_arch = "riscv64", target_arch = "loongarch64"))]
fn parse_fdt_interrupt(node: &fdt::node::FdtNode) -> Option<IrqLine> {
    if let Some(prop) = node.property("interrupts-extended") {
        if prop.value.len() >= 8 {
            let id =
                u32::from_be_bytes([prop.value[4], prop.value[5], prop.value[6], prop.value[7]]);
            return IrqLine::alloc_specific(id as u16).ok();
        }
    }
    if let Some(prop) = node.property("interrupts") {
        if prop.value.len() >= 4 {
            let id =
                u32::from_be_bytes([prop.value[0], prop.value[1], prop.value[2], prop.value[3]]);
            return IrqLine::alloc_specific(id as u16).ok();
        }
    }
    None
}

#[cfg(any(target_arch = "riscv64", target_arch = "loongarch64"))]
fn scan_pci_devices_from_fdt(fdt: &fdt::Fdt) {
    if let Some(pci_node) = fdt.find_compatible(&["pci-host-ecam-generic"]) {
        let reg = pci_node.reg().and_then(|mut r| r.next()).unwrap();
        let base_paddr = reg.starting_address as usize;
        let _size = reg.size.unwrap();
        let base_vaddr = paddr_to_vaddr(base_paddr);

        let cam = unsafe { MmioCam::new(base_vaddr as *mut u8, Cam::Ecam) };
        let mut root = PciRoot::new(cam);

        // 基于设备树 ranges 的 BAR 分配器；失败则回退固定窗口
        let mut bar_allocator = BarAllocator::from_fdt_ranges(&pci_node).unwrap_or_else(|| {
            BarAllocator::from_fixed_window(0x4000_0000, 0x4000_0000, false, AddressWidth::Width32)
        });

        for (func, info) in root.enumerate_bus(0) {
            use smallvec::SmallVec;

            if virtio_device_type(&info).is_none() {
                continue;
            }
            let mut iomems = SmallVec::new();
            for (i, bar) in root.bars(func).unwrap().into_iter().enumerate() {
                if let Some(BarInfo::Memory {
                    address_type, size, ..
                }) = bar
                {
                    if size > 0 {
                        match address_type {
                            MemoryBarType::Width32 => {
                                if let Some(addr) =
                                    bar_allocator.allocate(size as u64, AddressWidth::Width32, None)
                                {
                                    let io_mem = unsafe {
                                        IoMem::new(
                                            addr as usize..addr as usize + size as usize,
                                            PageFlags::RW,
                                            CachePolicy::Uncacheable,
                                        )
                                    };
                                    iomems.push(io_mem);
                                    root.set_bar_32(func, i as u8, addr as u32);
                                    log::debug!(
                                        "  BAR{}: allocated {:#x} bytes at {:#x}",
                                        i,
                                        size,
                                        addr
                                    );
                                } else {
                                    log::warn!("  BAR{}: OOM for size {:#x}", i, size);
                                }
                            }
                            MemoryBarType::Width64 => {
                                if let Some(addr) =
                                    bar_allocator.allocate(size as u64, AddressWidth::Width64, None)
                                {
                                    let io_mem = unsafe {
                                        IoMem::new(
                                            addr as usize..addr as usize + size as usize,
                                            PageFlags::RW,
                                            CachePolicy::Uncacheable,
                                        )
                                    };
                                    iomems.push(io_mem);
                                    root.set_bar_64(func, i as u8, addr as u64);
                                    log::debug!(
                                        "  BAR{}: allocated {:#x} bytes at {:#x} (64-bit)",
                                        i,
                                        size,
                                        addr
                                    );
                                } else {
                                    log::warn!("  BAR{}: OOM for size {:#x}", i, size);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            root.set_command(
                func,
                Command::IO_SPACE | Command::MEMORY_SPACE | Command::BUS_MASTER,
            );
            match PciTransport::new::<crate::drivers::virtio::hal::RiscvHal, _>(&mut root, func) {
                Ok(t) => {
                    // TODO:暂未解析单独 IRQ，交由后续改造接入
                    DEVICE_MANAGER.lock().create_pci_device(t, None, iomems);
                }
                Err(e) => log::error!("create pci transport failed: {:?}", e),
            }
        }
    }
}
