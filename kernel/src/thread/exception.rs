use alloc::sync::Arc;

use aster_rights::Full;
use ostd::{
    cpu::{CpuException, CpuExceptionInfo},
    mm::{Vaddr, VmSpace},
    task::Task,
};
use tracing::warn;

use crate::{
    thread::GetThreadLocalData,
    vm::{page_fault_handler::PageFaultHandler, perms::VmPerms, vmar::Vmar},
};

/// Page fault information converted from [`CpuExceptionInfo`].
///
/// `From<CpuExceptionInfo>` should be implemented for this struct.
/// If `CpuExceptionInfo` is a page fault, `try_from` should return `Ok(PageFaultInfo)`,
/// or `Err(())` (no error information) otherwise.
#[derive(Debug)]
pub struct PageFaultInfo {
    /// The virtual address where a page fault occurred.
    pub address: Vaddr,

    /// The [`VmPerms`] required by the memory operation that causes page fault.
    /// For example, a "store" operation may require `VmPerms::WRITE`.
    pub required_perms: VmPerms,
}

impl TryFrom<&CpuExceptionInfo> for PageFaultInfo {
    type Error = ();

    fn try_from(value: &CpuExceptionInfo) -> Result<Self, Self::Error> {
        let vm_perms = match value.code {
            CpuException::InstructionPageFault => VmPerms::EXEC | VmPerms::READ,
            CpuException::LoadPageFault => VmPerms::READ,
            CpuException::StorePageFault => VmPerms::WRITE | VmPerms::READ,
            _ => {
                warn!("其他异常类型: {:?}", value.code);
                VmPerms::empty()
            }
        };
        Ok(PageFaultInfo {
            address: value.stval,
            required_perms: vm_perms,
        })
    }
}

/// Handles the page fault occurs in the input `VmSpace`.
pub(crate) async fn handle_page_fault_from_vm_space(
    vm_space: &VmSpace,
    page_fault_info: &PageFaultInfo,
) -> core::result::Result<(), ()> {
    let current = Task::current().expect("current task is not found");
    let root_vmar = current
        .get_thread_local_data()
        .expect("thread local data is not found")
        .process_vm
        .root_vmar();

    // If page is not present or due to write access, we should ask the vmar try to commit this page
    debug_assert_eq!(
        Arc::as_ptr(root_vmar.vm_space()),
        vm_space as *const VmSpace
    );

    handle_page_fault_from_vmar(root_vmar, page_fault_info).await
}

/// Handles the page fault occurs in the input `Vmar`.
pub(crate) async fn handle_page_fault_from_vmar(
    root_vmar: &Vmar<Full>,
    page_fault_info: &PageFaultInfo,
) -> core::result::Result<(), ()> {
    if let Err(e) = root_vmar.handle_page_fault(page_fault_info).await {
        // warn!(
        //     "page fault handler failed: addr: 0x{:x}, err: {:?}",
        //     page_fault_info.address, e
        // );
        warn!(
            "page fault handler failed: addr: 0x{:x}, err: {:?}",
            page_fault_info.address, e
        );
        return Err(());
    }
    Ok(())
}
