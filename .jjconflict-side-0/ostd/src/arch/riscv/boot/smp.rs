// SPDX-License-Identifier: MPL-2.0

//! Multiprocessor Boot Support

use crate::{arch::boot::DEVICE_TREE, boot::smp::AP_BOOT_INFO, mm::kspace::kernel_loaded_offset};

pub(crate) fn get_num_processors() -> Option<u32> {
    // 从 BSP 初始化时存储的 DEVICE_TREE 获取 Fdt 对象
    let fdt = DEVICE_TREE.get()?;

    // 查找 /cpus 节点
    let cpus_node = fdt.find_node("/cpus")?;

    // 计算 /cpus 节点下类型为 "cpu" 的子节点数量
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
        // 如果找不到 cpu 子节点，可能 DTB 结构不同，或者只有 BSP
        // 尝试直接读取 /cpus 下的 #size-cells 和 #address-cells？或者返回 1？
        // 暂时返回 1 作为保底，表示至少有 BSP。
        log::warn!("Could not determine number of CPUs from DTB, assuming 1 (BSP only).");
        Some(1) // 保底返回 1
    }
}

// Linker symbol for the entry point
extern "C" {
    fn _start_ap();
}

// Accepts the BSP hart ID to avoid trying to start it.
pub(crate) fn bringup_all_aps(bsp_hart_id: u32) {
    let Some(num_processors) = get_num_processors() else {
        log::warn!("Cannot get number of processors, skipping AP bringup.");
        return;
    };

    let ap_count = num_processors - 1;
    if ap_count == 0 {
        log::info!(
            "Only one processor found (BSP {}), no APs to bring up.",
            bsp_hart_id
        );
        return;
    }

    // 获取 _start 的物理地址。
    // 具体详见 osdk/src/base_crate/riscv64.ld.template
    let start_addr_phys = _start_ap as usize - kernel_loaded_offset();

    log::info!(
        "Attempting to bring up {} AP(s) starting at P:{:#x}...",
        ap_count,
        start_addr_phys
    );

    // Get the AP boot info which contains the stack pointers
    let ap_boot_info = AP_BOOT_INFO
        .get()
        .expect("AP_BOOT_INFO not initialized before bringup_all_aps");
    let stack_array_ptr =
        crate::mm::paddr_to_vaddr(ap_boot_info.boot_stack_array.start_paddr()) as *const u64;

    // Iterate through all potential hart IDs.
    for hart_id in 0..num_processors {
        // Skip the BSP itself.
        if hart_id == bsp_hart_id {
            continue;
        }

        // Read the pre-allocated stack pointer for this AP from the array
        // SAFETY: The pointer is valid, aligned, and initialized by boot_all_aps.
        // We assume hart_id < 1024 based on the array size in boot_all_aps.
        let ap_stack_pointer = unsafe { stack_array_ptr.add(hart_id as usize).read_volatile() };

        if ap_stack_pointer == 0 {
            log::error!("Stack pointer for hart {} is zero! Skipping.", hart_id);
            continue;
        }

        log::debug!(
            "AP {}: Read stack pointer {:#x} from array index {}",
            hart_id,
            ap_stack_pointer,
            hart_id
        );

        // 使用 SBI 调用唤醒 AP，将栈顶指针作为 opaque 参数传递
        log::debug!(
            "Sending SBI hart_start to hart {} at P:{:#x} with SP:{:#x}",
            hart_id,
            start_addr_phys,
            ap_stack_pointer
        );
        let ret = sbi_rt::hart_start(hart_id as usize, start_addr_phys, ap_stack_pointer as usize);

        if ret.is_err() {
            log::error!(
                "Failed to start hart {} with physical address {:#x} and SP {:#x}. SBI Error: {:?}",
                hart_id,
                start_addr_phys,
                ap_stack_pointer,
                ret.err().unwrap()
            );
        } else {
            log::debug!(
                "SBI hart_start call seemingly succeeded for hart {}.",
                hart_id
            );
        }
    }

    // 注意：sbi_rt::hart_start 只是发送启动请求，AP 可能不会立即启动。
    // BSP 会在稍后调用 wait_for_all_aps_started() (在通用 smp 模块中) 来确认 AP 是否已成功初始化。
}
