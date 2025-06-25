// SPDX-License-Identifier: MPL-2.0

//! Virtual memory (VM).
//!
//! There are two primary VM abstractions:
//!  * Virtual Memory Address Regions (VMARs) a type of capability that manages
//!    user address spaces.
//!  * Virtual Memory Objects (VMOs) are are a type of capability that
//!    represents a set of memory pages.
//!
//! The concepts of VMARs and VMOs are originally introduced by
//! [Zircon](https://fuchsia.dev/fuchsia-src/reference/kernel_objects/vm_object).
//! As capabilities, the two abstractions are aligned with our goal
//! of everything-is-a-capability, although their specifications and
//! implementations in C/C++ cannot apply directly to Asterinas.
//! In Asterinas, VMARs and VMOs, as well as other capabilities, are implemented
//! as zero-cost capabilities.

use alloc::{ffi::CString, vec::Vec};

use aster_rights::Full;
use ostd::mm::Vaddr;
use vmar::Vmar;

use crate::{
    error::Result,
    thread::init_stack::{AuxVec, InitStack},
};

pub mod page_fault_handler;
pub mod perms;
pub mod util;
pub mod vmar;
pub mod vmo;

// The process user space virtual memory
pub struct ProcessVm {
    root_vmar: Vmar<Full>,
    init_stack: InitStack,
    // heap: Heap,
}

impl ProcessVm {
    /// Allocates a new `ProcessVm`
    pub fn alloc() -> Self {
        let root_vmar = Vmar::<Full>::new_root();
        let init_stack = InitStack::new();
        // let heap = Heap::new();
        // heap.alloc_and_map_vm(&root_vmar).unwrap();
        Self {
            root_vmar,
            // heap,
            init_stack,
        }
    }

    /// Forks a `ProcessVm` from `other`.
    ///
    /// The returned `ProcessVm` will have a forked `Vmar`.
    pub async fn fork_from(other: &ProcessVm) -> Result<Self> {
        let root_vmar = Vmar::<Full>::fork_from(&other.root_vmar).await?;
        Ok(Self {
            root_vmar,
            // heap: other.heap.clone(),
            init_stack: other.init_stack.clone(),
        })
    }

    pub async fn map_and_write_init_stack(
        &self,
        argv: Vec<CString>,
        envp: Vec<CString>,
        aux_vec: AuxVec,
    ) -> Result<()> {
        self.init_stack
            .map_and_write(&self.root_vmar, argv, envp, aux_vec)
            .await
    }

    /// Returns the top address of the user stack.
    pub fn user_stack_top(&self) -> Vaddr {
        self.init_stack.user_stack_top()
    }

    pub fn root_vmar(&self) -> &Vmar<Full> {
        &self.root_vmar
    }
}
