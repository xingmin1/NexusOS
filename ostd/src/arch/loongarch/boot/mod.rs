// SPDX-License-Identifier: MPL-2.0

//! The LoongArch boot module defines the entrypoints of Asterinas.

pub mod smp;

use core::arch::global_asm;

use fdt::Fdt;
use spin::Once;

use crate::{
    boot::{
        memory_region::{MemoryRegion, MemoryRegionArray, MemoryRegionType},
        BootloaderAcpiArg, BootloaderFramebufferArg,
    },
    mm::paddr_to_vaddr,
};

global_asm!(include_str!("boot.S"));

static FDT_DATA: &[u8] = include_bytes!("loongarch64-qemu.dtb");
/// The Flattened Device Tree of the platform.
pub static DEVICE_TREE: Once<Fdt> = Once::new();

/// Stores the CPU ID of the bootstrap processor (BSP).
static BSP_CPU_ID: Once<usize> = Once::new();

/// Returns the CPU ID of the bootstrap processor (BSP).
/// Panics if called before the BSP ID is initialised during early boot.
#[inline]
#[allow(dead_code)]
pub(crate) fn bsp_hart_id() -> u32 {
    (*BSP_CPU_ID
        .get()
        .expect("BSP_CPU_ID accessed before initialization"))
        .try_into()
        .unwrap()
}

fn parse_bootloader_name() -> &'static str {
    "unknown"
}

fn parse_kernel_commandline() -> &'static str {
    DEVICE_TREE.get().unwrap().chosen().bootargs().unwrap_or("")
}

fn parse_initramfs() -> Option<&'static [u8]> {
    let Some((start, end)) = parse_initramfs_range() else {
        return None;
    };

    let base_va = paddr_to_vaddr(start);
    let length = end - start;
    Some(unsafe { core::slice::from_raw_parts(base_va as *const u8, length) })
}

fn parse_acpi_arg() -> BootloaderAcpiArg {
    BootloaderAcpiArg::NotProvided
}

fn parse_framebuffer_info() -> Option<BootloaderFramebufferArg> {
    None
}

fn parse_memory_regions() -> MemoryRegionArray {
    let mut regions = MemoryRegionArray::new();

    let fdt = DEVICE_TREE
        .get()
        .expect("DEVICE_TREE not initialized in parse_memory_regions");

    // Add usable memory regions
    for region in fdt.memory().regions() {
        if region.size.unwrap_or(0) > 0 {
            regions
                .push(MemoryRegion::new(
                    region.starting_address as usize,
                    region.size.unwrap(),
                    MemoryRegionType::Usable,
                ))
                .expect("Failed to push memory region");
        }
    }

    // Reserved memory regions
    if let Some(node) = fdt.find_node("/reserved-memory") {
        for child in node.children() {
            if let Some(reg_iter) = child.reg() {
                for region in reg_iter {
                    regions
                        .push(MemoryRegion::new(
                            region.starting_address as usize,
                            region.size.unwrap(),
                            MemoryRegionType::Reserved,
                        ))
                        .expect("Failed to push reserved memory region");
                }
            }
        }
    }

    // Kernel region
    regions
        .push(MemoryRegion::kernel())
        .expect("Failed to push kernel region");

    // Initramfs region (if any)
    if let Some((start, end)) = parse_initramfs_range() {
        regions
            .push(MemoryRegion::new(
                start,
                end - start,
                MemoryRegionType::Module,
            ))
            .expect("Failed to push initramfs region");
    }

    regions.into_non_overlapping()
}

fn parse_initramfs_range() -> Option<(usize, usize)> {
    let fdt = DEVICE_TREE
        .get()
        .expect("DEVICE_TREE not initialized in parse_initramfs_range");
    let chosen = fdt.find_node("/chosen")?;
    let initrd_start = chosen.property("linux,initrd-start")?.as_usize()?;
    let initrd_end = chosen.property("linux,initrd-end")?.as_usize()?;
    Some((initrd_start, initrd_end))
}

/// The entry point of the Rust code portion of Asterinas.
#[no_mangle]
pub extern "C" fn loongarch_boot(cpu_id: usize, _system_table_paddr: usize) -> ! {
    // The first CPU arriving becomes BSP
    let bsp_cpu = *BSP_CPU_ID.call_once(|| cpu_id);

    if cpu_id == bsp_cpu {
        // BSP path
        // [TODO] 这里需要修改，FDT 需要从 _system_table_paddr 中获取
        let fdt = Fdt::new(FDT_DATA).unwrap();
        DEVICE_TREE.call_once(|| fdt);

        use crate::boot::{call_ostd_main, EarlyBootInfo, EARLY_INFO};

        EARLY_INFO.call_once(|| EarlyBootInfo {
            bootloader_name: parse_bootloader_name(),
            kernel_cmdline: parse_kernel_commandline(),
            initramfs: parse_initramfs(),
            acpi_arg: parse_acpi_arg(),
            framebuffer_arg: parse_framebuffer_info(),
            memory_regions: parse_memory_regions(),
        });

        call_ostd_main();
    } else {
        // Application Processor (AP) path – jump to generic early entry
        crate::boot::smp::ap_early_entry(cpu_id as u32);
    }
}
