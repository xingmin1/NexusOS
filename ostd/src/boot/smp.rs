// SPDX-License-Identifier: MPL-2.0

//! Symmetric multiprocessing (SMP) boot support.

use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicBool, Ordering};

use spin::Once;

use crate::{
    arch::boot::smp::{bringup_all_aps, get_num_processors},
    cpu,
    mm::{frame::Segment, kspace::KernelMeta, paddr_to_vaddr, FrameAllocOptions, PAGE_SIZE},
};

pub(crate) static AP_BOOT_INFO: Once<ApBootInfo> = Once::new();

const AP_BOOT_STACK_SIZE: usize = PAGE_SIZE * 64;

pub(crate) struct ApBootInfo {
    /// It holds the boot stack top pointers used by all APs.
    pub(crate) boot_stack_array: Segment<KernelMeta>,
    /// `per_ap_info` maps each AP's ID to its associated boot information.
    per_ap_info: BTreeMap<u32, PerApInfo>,
}

struct PerApInfo {
    is_started: AtomicBool,
    // TODO: When the AP starts up and begins executing tasks, the boot stack will
    // no longer be used, and the `Segment` can be deallocated (this problem also
    // exists in the boot processor, but the memory it occupies should be returned
    // to the frame allocator).
    #[expect(dead_code)]
    boot_stack_pages: Segment<KernelMeta>,
}

static AP_LATE_ENTRY: Once<fn() -> !> = Once::new();

/// Boot all application processors.
///
/// This function should be called late in the system startup. The system must at
/// least ensure that the scheduler, ACPI table, memory allocation, and IPI module
/// have been initialized.
///
/// However, the function need to be called before any `cpu_local!` variables are
/// accessed, including the APIC instance.
///
/// `bsp_hart_id` is the ID of the calling processor (BSP).
pub fn boot_all_aps(bsp_hart_id: u32) {
    // TODO: support boot protocols without ACPI tables, e.g., Multiboot
    let Some(num_cpus) = get_num_processors() else {
        log::warn!("No processor information found. The kernel operates with a single processor.");
        return;
    };
    log::info!("Found {} processors.", num_cpus);

    // We currently assumes that bootstrap processor (BSP) have always the
    // processor ID 0. And the processor ID starts from 0 to `num_cpus - 1`.
    // ^^^ THIS ASSUMPTION IS NO LONGER VALID FOR RISC-V ^^^ We need bsp_hart_id.

    AP_BOOT_INFO.call_once(|| {
        let mut per_ap_info = BTreeMap::new();
        // Use two pages to place stack pointers of all APs, thus support up to 1024 APs.
        let boot_stack_array = FrameAllocOptions::new()
            .zeroed(true)
            .alloc_segment_with(2, |_| KernelMeta)
            .unwrap();
        assert!(num_cpus < 1024);

        // Iterate through all potential hart IDs up to num_cpus
        for ap_id in 0..num_cpus {
            // Skip the BSP itself
            if ap_id == bsp_hart_id {
                continue;
            }

            let boot_stack_pages = FrameAllocOptions::new()
                .zeroed(false)
                .alloc_segment_with(AP_BOOT_STACK_SIZE / PAGE_SIZE, |_| KernelMeta)
                .unwrap();
            let boot_stack_ptr = paddr_to_vaddr(boot_stack_pages.end_paddr());
            let stack_array_ptr = paddr_to_vaddr(boot_stack_array.start_paddr()) as *mut u64;
            // SAFETY: The `stack_array_ptr` is valid and aligned.
            unsafe {
                stack_array_ptr
                    .add(ap_id as usize)
                    .write_volatile(boot_stack_ptr as u64);
            }
            per_ap_info.insert(
                ap_id,
                PerApInfo {
                    is_started: AtomicBool::new(false),
                    boot_stack_pages,
                },
            );
        }

        ApBootInfo {
            boot_stack_array,
            per_ap_info,
        }
    });

    log::info!(
        "Booting all application processors (except BSP {})...",
        bsp_hart_id
    );
    // Pass BSP ID to architecture-specific bringup function
    bringup_all_aps(bsp_hart_id);

    wait_for_all_aps_started();

    log::info!("All application processors started. The BSP continues to run.");
}

/// Register the entry function for the application processor.
///
/// Once the entry function is registered, all the application processors
/// will jump to the entry function immediately.
pub fn register_ap_entry(entry: fn() -> !) {
    AP_LATE_ENTRY.call_once(|| entry);
}

#[no_mangle]
#[allow(unreachable_code)] // 允许函数包含不可达代码
pub(crate) fn ap_early_entry(ap_hart_id: u32) -> ! {
    // SAFETY: 在初始化`sscratch`和`stvec`之后，不会在手动修改这些寄存器
    unsafe {
        crate::arch::init_on_ap(ap_hart_id);
    }

    // SAFETY: 我们正在AP上，并且只会使用正确的CPU ID调用一次
    unsafe {
        cpu::local::init_on_ap(ap_hart_id);
        cpu::set_this_cpu_id(ap_hart_id);
        crate::mm::kspace::activate_kernel_page_table();
    }

    // Mark the AP as started.
    let ap_boot_info = AP_BOOT_INFO.get().unwrap();
    ap_boot_info
        .per_ap_info
        .get(&ap_hart_id)
        .unwrap()
        .is_started
        .store(true, Ordering::Release);

    log::info!("Processor {} started. Spinning for tasks.", ap_hart_id);

    let ap_late_entry = AP_LATE_ENTRY.wait();
    ap_late_entry();

    unreachable!("`ap_late_entry` should not return");
}

fn wait_for_all_aps_started() {
    fn is_all_aps_started() -> bool {
        let ap_boot_info = AP_BOOT_INFO.get().unwrap();
        // 检查启动的AP数量是否与预期数量匹配
        let started_count = ap_boot_info
            .per_ap_info
            .values()
            .filter(|info| info.is_started.load(Ordering::Acquire))
            .count();
        // 预期数量是总CPU数量减去1（BSP）
        let expected_ap_count = ap_boot_info.per_ap_info.len();
        started_count == expected_ap_count
    }

    log::info!("Waiting for all APs to start...");
    while !is_all_aps_started() {
        core::hint::spin_loop();
    }
    log::info!("All APs confirmed started.");
}
