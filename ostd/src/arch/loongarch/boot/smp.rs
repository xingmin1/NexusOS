// SPDX-License-Identifier: MPL-2.0

//! Multiprocessor Boot Support

use crate::{
    arch::boot::DEVICE_TREE,
    boot::smp::AP_BOOT_INFO,
    mm::{kspace::kernel_loaded_offset, paddr_to_vaddr},
};

use loongArch64::ipi;

/// Send startup IPI to a secondary CPU (hart) on LoongArch.
///
/// `hartid`   – target CPU ID
/// `start_pa` – physical address where the CPU should start execution
/// `opaque`   – extra payload (e.g., stack pointer)
fn hart_start_loongarch(hartid: usize, start_pa: usize, opaque: usize) {
    // Map the physical address into the DMW1 window (0x9000_0000_0000_0000)
    let start_va = start_pa | 0x9000_0000_0000_0000;

    // Write startup address and send IPI
    ipi::csr_mail_send(start_va as u64, hartid, opaque);
    ipi::send_ipi_single(hartid, 1);
}

/// Return the total number of processors detected from the Device Tree.
pub(crate) fn get_num_processors() -> Option<u32> {
    let fdt = DEVICE_TREE.get()?;
    let cpus_node = fdt.find_node("/cpus")?;

    let num_cpus = cpus_node
        .children()
        .filter(|child| match child.property("device_type") {
            Some(prop) => prop.as_str().map_or(false, |s| s == "cpu"),
            None => false,
        })
        .count();

    if num_cpus > 0 {
        Some(num_cpus as u32)
    } else {
        log::warn!(
            "Could not determine number of CPUs from DTB, assuming 1 (BSP only)."
        );
        Some(1)
    }
}

// Linker symbol for the entry point of APs, if implemented in assembly.
#[allow(dead_code)]
extern "C" {
    fn _start_ap();
}

/// Bring up all application processors (APs). Currently unimplemented for LoongArch.
/// The function logs an informational message and returns immediately.
///
/// Once a standard interface (e.g. ACPI MADT / PSCI / SMC) is decided for
/// LoongArch in Asterinas, this function should be updated to start the APs and
/// pass the stack pointer (found in `AP_BOOT_INFO`) as the opaque parameter.
pub(crate) fn bringup_all_aps(bsp_cpu_id: u32) {
    let Some(num_processors) = get_num_processors() else {
        log::warn!("Cannot get number of processors, skipping AP bringup.");
        return;
    };

    let ap_count = num_processors - 1;
    if ap_count == 0 {
        log::info!(
            "Only one processor found (BSP {}), no APs to bring up.",
            bsp_cpu_id
        );
        return;
    }

    let start_addr_phys = _start_ap as usize - kernel_loaded_offset();

    log::info!(
        "Attempting to bring up {} AP(s) starting at P:{:#x}...",
        ap_count,
        start_addr_phys
    );

    let ap_boot_info = AP_BOOT_INFO
        .get()
        .expect("AP_BOOT_INFO not initialized before bringup_all_aps");
    let stack_array_ptr = paddr_to_vaddr(ap_boot_info.boot_stack_array.start_paddr()) as *const u64;

    for cpu_id in 0..num_processors {
        if cpu_id == bsp_cpu_id {
            continue;
        }

        let ap_stack_pointer = unsafe { stack_array_ptr.add(cpu_id as usize).read_volatile() };

        if ap_stack_pointer == 0 {
            log::error!("Stack pointer for CPU {} is zero! Skipping.", cpu_id);
            continue;
        }

        log::debug!(
            "CPU {}: stack pointer {:#x} retrieved from array index {}",
            cpu_id,
            ap_stack_pointer,
            cpu_id
        );

        log::debug!(
            "Sending startup IPI to CPU {} with SP:{:#x}",
            cpu_id,
            ap_stack_pointer
        );

        hart_start_loongarch(cpu_id as usize, start_addr_phys, ap_stack_pointer as usize);
    }
}

/// Compatibility alias for existing callers.
#[allow(dead_code)]
pub(crate) fn count_processors() -> Option<u32> {
    get_num_processors()
}