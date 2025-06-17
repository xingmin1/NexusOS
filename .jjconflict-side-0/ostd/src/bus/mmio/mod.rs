// SPDX-License-Identifier: MPL-2.0

#![expect(dead_code)]

//! Virtio over MMIO

pub mod bus;
pub mod common_device;

use alloc::vec::Vec;
use alloc::sync::Arc;

use cfg_if::cfg_if;

use self::bus::MmioBus;
use crate::{sync::GuardSpinLock, trap::IrqLine};
use crate::drivers::virtio::block::VirtioBlkDriver;

#[cfg(target_arch = "riscv64")]
use crate::arch::riscv::boot::DEVICE_TREE;

cfg_if! {
    if #[cfg(all(target_arch = "x86_64", feature = "cvm_guest"))] {
        use ::tdx_guest::tdx_is_enabled;
        use crate::arch::tdx_guest;
    }
}

const VIRTIO_MMIO_MAGIC: u32 = 0x74726976;

/// MMIO bus instance
pub static MMIO_BUS: GuardSpinLock<MmioBus> = GuardSpinLock::new(MmioBus::new());
static IRQS: GuardSpinLock<Vec<IrqLine>> = GuardSpinLock::new(Vec::new());

pub(crate) fn init() {
    // 注册 VirtIO 相关驱动，需在扫描设备之前完成。
    MMIO_BUS
        .lock()
        .register_driver(Arc::new(VirtioBlkDriver::new()));

    #[cfg(all(target_arch = "x86_64", feature = "cvm_guest"))]
    // SAFETY:
    // This is safe because we are ensuring that the address range 0xFEB0_0000 to 0xFEB0_4000 is valid before this operation.
    // The address range is page-aligned and falls within the MMIO range, which is a requirement for the `unprotect_gpa_range` function.
    // We are also ensuring that we are only unprotecting four pages.
    // Therefore, we are not causing any undefined behavior or violating any of the requirements of the `unprotect_gpa_range` function.
    if tdx_is_enabled() {
        unsafe {
            tdx_guest::unprotect_gpa_range(0xFEB0_0000, 4).unwrap();
        }
    }
    // FIXME: The address 0xFEB0_0000 is obtained from an instance of microvm, and it may not work in other architecture.
    #[cfg(target_arch = "x86_64")]
    iter_range(0xFEB0_0000..0xFEB0_4000);

    // [TODO]: 对 LoongArch 平台，需要解析 FDT 并绑定 GIC IRQ，再调用 iter_range 或等效扫描函数。

    #[cfg(target_arch = "riscv64")]
    tracing::info!("Initializing MMIO for BSP hart {}", crate::arch::boot::bsp_hart_id());
    iter_fdt_nodes();
    tracing::info!("Initialized MMIO for BSP hart {}", crate::arch::boot::bsp_hart_id());
}

#[cfg(target_arch = "x86_64")]
fn iter_range(range: Range<usize>) {
    debug!("[Virtio]: Iter MMIO range:{:x?}", range);
    let mut current = range.end;
    let mut lock = MMIO_BUS.lock();
    let io_apics = crate::arch::kernel::IO_APIC.get().unwrap();
    let is_ioapic2 = io_apics.len() == 2;
    let mut io_apic = if is_ioapic2 {
        io_apics.get(1).unwrap().lock()
    } else {
        io_apics.first().unwrap().lock()
    };
    let mut device_count = 0;
    while current > range.start {
        current -= 0x100;
        // SAFETY: It only read the value and judge if the magic value fit 0x74726976
        let magic = unsafe { core::ptr::read_volatile(paddr_to_vaddr(current) as *const u32) };
        if magic == VIRTIO_MMIO_MAGIC {
            // SAFETY: It only read the device id
            let device_id = unsafe { *(paddr_to_vaddr(current + 8) as *const u32) };
            device_count += 1;
            if device_id == 0 {
                continue;
            }
            let handle = IrqLine::alloc().unwrap();
            // If has two IOApic, then start: 24 (0 in IOApic2), end 47 (23 in IOApic2)
            // If one IOApic, then start: 16, end 23
            io_apic.enable(24 - device_count, handle.clone()).unwrap();
            let device = MmioCommonDevice::new(current, handle);
            lock.register_mmio_device(device);
        }
    }
}

#[cfg(target_arch = "riscv64")]
fn iter_fdt_nodes() {
    let Some(fdt) = DEVICE_TREE.get() else {
        log::warn!("[Virtio]: DEVICE_TREE not ready, skip MMIO scan");
        return;
    };

    let mut device_count = 0u32;
    for node in fdt.all_nodes() {
        if let Some(compatible) = node.compatible() {
            if compatible.all().any(|s| s == "virtio,mmio") {
                if let Some(mut reg_iter) = node.reg() {
                    if let Some(reg) = reg_iter.next() {
                        let paddr = reg.starting_address as usize;
                        tracing::error!("paddr: {:#x}", paddr);
                        // 解析 IRQ
                        let irq_id = if let Some(prop) = node.property("interrupts-extended") {
                            use ostd_pod::Pod;
                            tracing::error!("interrupts-extended: {:?}", prop);
                            let usizes = prop.as_usize().unwrap();
                            let bytes = usizes.as_bytes();
                            if bytes.len() >= 8 {
                                // 跳过 phandle (前 4 字节)，取第二个 cell
                                u32::from_be_bytes([
                                    bytes[4], bytes[5], bytes[6], bytes[7],
                                ])
                            } else {
                                0
                            }
                        } else if let Some(prop) = node.property("interrupts") {
                            tracing::error!("interrupts: {:?}", prop);
                            let bytes = prop.value;
                            if bytes.len() >= 4 {
                                u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                            } else {
                                0
                            }
                        } else {
                            0
                        };

                        if irq_id == 0 {
                            log::warn!("[Virtio]: node {} has no valid IRQ, skip", node.name);
                            continue;
                        }

                        // 分配 IRQ line 对应 handle
                        let handle = match IrqLine::alloc_specific(irq_id as u16) {
                            Ok(h) => h,
                            Err(_) => {
                                log::warn!("[Virtio]: IRQ {} already allocated", irq_id);
                                continue;
                            }
                        };

                        let device = crate::bus::mmio::common_device::MmioCommonDevice::new(paddr, handle);

                        MMIO_BUS.lock().register_mmio_device(device);

                        device_count += 1;
                    }
                }
            }
        }
    }
    log::info!("[Virtio]: FDT scan found {} device(s)", device_count);
}
