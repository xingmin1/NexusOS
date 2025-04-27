// SPDX-License-Identifier: MPL-2.0

//! CPU local storage.
//!
//! This module provides a mechanism to define CPU-local objects, by the macro
//! [`crate::cpu_local!`].
//!
//! Such a mechanism exploits the fact that constant values of non-[`Copy`]
//! types can be bitwise copied. For example, a [`Option<T>`] object, though
//! being not [`Copy`], have a constant constructor [`Option::None`] that
//! produces a value that can be bitwise copied to create a new instance.
//! [`alloc::sync::Arc`] however, don't have such a constructor, and thus cannot
//! be directly used as a CPU-local object. Wrapping it in a type that has a
//! constant constructor, like [`Option<T>`], can make it CPU-local.
//!
//! # Implementation
//!
//! These APIs are implemented by placing the CPU-local objects in a special
//! section `.cpu_local`. The bootstrap processor (BSP) uses the objects linked
//! in this section, and these objects are copied to dynamically allocated
//! local storage of each application processors (AP) during the initialization
//! process.

// This module also, provide CPU-local cell objects that have inner mutability.
//
// The difference between CPU-local objects (defined by [`crate::cpu_local!`])
// and CPU-local cell objects (defined by [`crate::cpu_local_cell!`]) is that
// the CPU-local objects can be shared across CPUs. While through a CPU-local
// cell object you can only access the value on the current CPU, therefore
// enabling inner mutability without locks.

mod cell;
mod cpu_local;

pub(crate) mod single_instr;

use alloc::{vec, vec::Vec};

use align_ext::AlignExt;
pub use cell::CpuLocalCell;
pub use cpu_local::{CpuLocal, CpuLocalDerefGuard};
use spin::Once;

use crate::{
    arch,
    mm::{frame::Segment, kspace::KernelMeta, paddr_to_vaddr, FrameAllocOptions, PAGE_SIZE},
};

// These symbols are provided by the linker script.
extern "C" {
    fn __cpu_local_start();
    fn __cpu_local_end();
}

/// Sets the base address of the CPU-local storage for the bootstrap processor.
///
/// It should be called early to let [`crate::task::disable_preempt`] work,
/// which needs to update a CPU-local preemption info. Otherwise it may
/// panic when calling [`crate::task::disable_preempt`]. It is needed since
/// heap allocations need to disable preemption, which would happen in the
/// very early stage of the kernel.
///
/// # Safety
///
/// It should be called only once and only on the BSP.
pub(crate) unsafe fn early_init_bsp_local_base() {
    let start_base_va = __cpu_local_start as usize as u64;

    // SAFETY: The base to be set is the start of the `.cpu_local` section,
    // where accessing the CPU-local objects have defined behaviors.
    unsafe {
        arch::cpu::local::set_base(start_base_va);
    }
}

/// 存储为每个应用处理器(AP)分配的CPU本地数据段，索引为AP的hart ID
pub(crate) static AP_CPU_LOCAL_AREAS: Once<Vec<Option<Segment<KernelMeta>>>> = Once::new();

/// 初始化引导处理器(BSP)的CPU本地数据，并为AP分配CPU本地存储区域
///
/// # 安全性
///
/// 本函数只能在BSP上调用且只能调用一次
///
/// 必须确保BSP在调用本函数前不会访问本地数据，否则复制非常量值会导致严重的未定义行为
pub unsafe fn init_on_bsp() {
    let bsp_base_va = __cpu_local_start as usize;
    let bsp_end_va = __cpu_local_end as usize;

    let num_cpus = super::num_cpus();
    let bsp_hart_id = crate::cpu::CpuId::bsp().as_usize();

    let mut ap_local_areas = vec![None; num_cpus];

    for ap_id in 0..num_cpus {
        if ap_id == bsp_hart_id {
            continue;
        }

        let ap_pages = {
            let nbytes = (bsp_end_va - bsp_base_va).align_up(PAGE_SIZE);
            FrameAllocOptions::new()
                .zeroed(false)
                .alloc_segment_with(nbytes / PAGE_SIZE, |_| KernelMeta)
                .unwrap()
        };
        let ap_pages_ptr = paddr_to_vaddr(ap_pages.start_paddr()) as *mut u8;

        // 从BSP的.cpu_local段复制初始数据
        // 安全性：BSP尚未初始化CPU本地区域，因此.cpu_local段中的对象可以按位批量复制到AP的本地存储
        // 目标内存已分配，因此写入是有效的
        unsafe {
            core::ptr::copy_nonoverlapping(
                bsp_base_va as *const u8,
                ap_pages_ptr,
                bsp_end_va - bsp_base_va,
            );
        }

        ap_local_areas[ap_id] = Some(ap_pages);
    }

    // 存储映射关系供AP后续查找
    AP_CPU_LOCAL_AREAS.call_once(|| ap_local_areas);

    // 设置BSP自身的基地址（仍使用链接器脚本分配的段）
    arch::cpu::local::set_base(bsp_base_va as u64);

    has_init::set_true();
}

/// 初始化应用处理器(AP)的CPU本地数据
///
/// # 安全性
///
/// 本函数只能在AP上调用
pub unsafe fn init_on_ap(cpu_id: u32) {
    // 查找预分配的本AP对应的段
    let ap_areas_map = AP_CPU_LOCAL_AREAS
        .get()
        .expect("AP_CPU_LOCAL_AREAS is not initialized before init_on_ap");

    let ap_pages_segment = ap_areas_map[cpu_id as usize]
        .as_ref()
        .unwrap_or_else(|| panic!("can't find the CPU local storage area for AP {}", cpu_id));

    let ap_base_ptr = paddr_to_vaddr(ap_pages_segment.start_paddr());

    // 安全性：该内存是为本AP分配的，且当前在AP上执行
    unsafe {
        arch::cpu::local::set_base(ap_base_ptr as u64);
    }

    crate::task::reset_preempt_info();
}

mod has_init {
    //! This module is used to detect the programming error of using the CPU-local
    //! mechanism before it is initialized. Such bugs have been found before and we
    //! do not want to repeat this error again. This module is only incurs runtime
    //! overhead if debug assertions are enabled.
    cfg_if::cfg_if! {
        if #[cfg(debug_assertions)] {
            use core::sync::atomic::{AtomicBool, Ordering};

            static IS_INITIALIZED: AtomicBool = AtomicBool::new(false);

            pub fn assert_true() {
                debug_assert!(IS_INITIALIZED.load(Ordering::Relaxed));
            }

            pub fn set_true() {
                IS_INITIALIZED.store(true, Ordering::Relaxed);
            }
        } else {
            pub fn assert_true() {}

            pub fn set_true() {}
        }
    }
}

#[cfg(ktest)]
mod test {
    use core::cell::RefCell;

    use ostd_macros::ktest;

    #[ktest]
    fn test_cpu_local() {
        crate::cpu_local! {
            static FOO: RefCell<usize> = RefCell::new(1);
        }
        let irq_guard = crate::trap::disable_local();
        let foo_guard = FOO.get_with(&irq_guard);
        assert_eq!(*foo_guard.borrow(), 1);
        *foo_guard.borrow_mut() = 2;
        assert_eq!(*foo_guard.borrow(), 2);
        drop(foo_guard);
    }

    #[ktest]
    fn test_cpu_local_cell() {
        crate::cpu_local_cell! {
            static BAR: usize = 3;
        }
        let _guard = crate::trap::disable_local();
        assert_eq!(BAR.load(), 3);
        BAR.store(4);
        assert_eq!(BAR.load(), 4);
    }
}
