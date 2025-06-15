// SPDX-License-Identifier: MPL-2.0

//! The RISC-V boot module defines the entrypoints of Asterinas.

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

/// The Flattened Device Tree of the platform.
pub static DEVICE_TREE: Once<Fdt> = Once::new();

/// 存储引导处理器（BSP）的 Hart ID
/// 由第一个进入 `riscv_boot` 的 hart 初始化
static BSP_HART_ID: Once<usize> = Once::new();

/// 返回引导处理器（BSP）的 Hart ID
/// 在引导过程中 BSP ID 确定前调用会触发 panic
#[inline]
pub(crate) fn bsp_hart_id() -> u32 {
    (*BSP_HART_ID
        .get()
        .expect("BSP_HART_ID accessed before initialization"))
    .try_into()
    .unwrap()
}

fn parse_bootloader_name() -> &'static str {
    "Unknown"
}

fn parse_kernel_commandline() -> &'static str {
    DEVICE_TREE
        .get()
        .expect("DEVICE_TREE not initialized in parse_kernel_commandline")
        .chosen()
        .bootargs()
        .unwrap_or("")
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

    // 在 BSP 上下文中，DEVICE_TREE.call_once 后必须已初始化
    let fdt = DEVICE_TREE
        .get()
        .expect("DEVICE_TREE not initialized in parse_memory_regions");
    for region in fdt.memory().regions() {
        if region.size.unwrap_or(0) > 0 {
            regions.push(MemoryRegion::new(
                region.starting_address as usize,
                region.size.unwrap(),
                MemoryRegionType::Usable,
            )).expect("Failed to push memory region");
        }
    }

    // 从 DTB 解析保留内存区域
    if let Some(node) = fdt.find_node("/reserved-memory") {
        for child in node.children() {
            if let Some(reg_iter) = child.reg() {
                for region in reg_iter {
                    regions.push(MemoryRegion::new(
                        region.starting_address as usize,
                        region.size.unwrap(),
                        MemoryRegionType::Reserved,
                    )).expect("Failed to push reserved memory region");
                }
            }
        }
    }

    // 添加内核区域
    regions
        .push(MemoryRegion::kernel())
        .expect("Failed to push kernel region");

    // 添加 initramfs 区域
    if let Some((start, end)) = parse_initramfs_range() {
        regions.push(MemoryRegion::new(
            start,
            end - start,
            MemoryRegionType::Module,
        )).expect("Failed to push initramfs region");
    }

    regions.into_non_overlapping()
}

fn parse_initramfs_range() -> Option<(usize, usize)> {
    // 在 BSP 上下文中，DEVICE_TREE.call_once 后必须已初始化
    let fdt = DEVICE_TREE
        .get()
        .expect("DEVICE_TREE not initialized in parse_initramfs_range");
    let chosen = fdt.find_node("/chosen")?;
    let initrd_start = chosen.property("linux,initrd-start")?.as_usize()?;
    let initrd_end = chosen.property("linux,initrd-end")?.as_usize()?;
    Some((initrd_start, initrd_end))
}

/// Asterinas 的 Rust 代码入口点
#[no_mangle]
pub extern "C" fn riscv_boot(hart_id: usize, device_tree_paddr: usize) -> ! {
    crate::early_println!(
        "riscv_boot: hart_id = {}, device_tree_paddr = {}",
        hart_id,
        device_tree_paddr
    );
    // 第一个执行此函数的 hart 将成为 BSP
    let bsp_hart_id = *BSP_HART_ID.call_once(|| hart_id);

    if hart_id == bsp_hart_id {
        let device_tree_ptr = paddr_to_vaddr(device_tree_paddr) as *const u8;
        // 安全性：DTB 指针由引导程序/SBI 提供给第一个 hart（BSP）
        let fdt = unsafe { fdt::Fdt::from_ptr(device_tree_ptr).unwrap() };

        // 由于 BSP 是第一个调用者，这应该会成功
        DEVICE_TREE.call_once(|| fdt);

        // 初始化全局 PLIC
        unsafe { crate::arch::riscv::plic::init_global(device_tree_paddr); }

        use crate::boot::{call_ostd_main, EarlyBootInfo, EARLY_INFO};

        // 安全性：此函数仅由 BSP 调用一次
        EARLY_INFO.call_once(|| {
            EarlyBootInfo {
                bootloader_name: parse_bootloader_name(),
                kernel_cmdline: parse_kernel_commandline(),
                initramfs: parse_initramfs(),
                acpi_arg: parse_acpi_arg(),
                framebuffer_arg: parse_framebuffer_info(),
                memory_regions: parse_memory_regions(),
            }
        });
        
        call_ostd_main();
    } else {
        // 当前 hart 是 AP
        crate::boot::smp::ap_early_entry(hart_id as u32);
    }
}
