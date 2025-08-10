// SPDX-License-Identifier: MPL-2.0

#![expect(dead_code)]

//! Virtio over MMIO

pub mod bus;
pub mod common_device;

use alloc::vec::Vec;

use cfg_if::cfg_if;

use self::bus::MmioBus;
use crate::{sync::GuardSpinLock, trap::IrqLine};

#[cfg(any(target_arch = "riscv64", target_arch = "loongarch64"))]
use crate::arch::boot::DEVICE_TREE;

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
    // 迁移说明：
    // - RISC-V / LoongArch 的 MMIO 设备扫描已迁移至 `crate::bus::init::init()`。
    // - 这里保留 x86_64 环境下的历史探测逻辑，避免破坏原有平台；当前项目不再调用。
    #[cfg(target_arch = "x86_64")]
    {
        #[cfg(all(feature = "cvm_guest"))]
        if tdx_is_enabled() {
            unsafe { tdx_guest::unprotect_gpa_range(0xFEB0_0000, 4).unwrap(); }
        }
        iter_range(0xFEB0_0000..0xFEB0_4000);
    }
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

#[cfg(any(target_arch = "riscv64", target_arch = "loongarch64"))]
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
                        tracing::trace!("paddr: {:#x}", paddr);
                        // 解析 IRQ
                        let irq_id = if let Some(prop) = node.property("interrupts-extended") {
                            use ostd_pod::Pod;
                            tracing::trace!("interrupts-extended: {:?}", prop);
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
                            tracing::trace!("interrupts: {:?}", prop);
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
