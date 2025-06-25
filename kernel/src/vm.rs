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
use nexus_error::{errno_with_message, ostd_error_to_errno, return_errno_with_message, Errno};
use ostd::{mm::{Fallible, Vaddr, VmReader}, Pod};
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

impl ProcessVm {
    /// Writes `val` to the user space of the current process.
    pub fn write_val<T: Pod>(&self, dest: Vaddr, val: &T) -> Result<()> {
        if core::mem::size_of::<T>() > 0 {
            check_vaddr(dest)?;
        }

        self.root_vmar
            .vm_space()
            .writer(dest, core::mem::size_of::<T>())
            .and_then(|mut user_writer| user_writer.write_val(val))
            .map_err(ostd_error_to_errno)
    }

    
    /// Reads a value typed `Pod` from the user space of the current process.
    pub fn read_val<T: Pod>(&self, src: Vaddr) -> Result<T> {
        if core::mem::size_of::<T>() > 0 {
            check_vaddr(src)?;
        }

        let mut user_reader = self.root_vmar
            .vm_space()
            .reader(src, core::mem::size_of::<T>())
            .map_err(ostd_error_to_errno)?;
        user_reader.read_val().map_err(ostd_error_to_errno)
    }

    /// Reads a C string from the user space of the current process.
    /// The length of the string should not exceed `max_len`,
    /// including the final `\0` byte.
    pub fn read_cstring(&self, vaddr: Vaddr, max_len: usize) -> Result<CString> {
        if max_len > 0 {
            check_vaddr(vaddr)?;
        }

        let mut user_reader = self.root_vmar
            .vm_space()
            .reader(vaddr, max_len)
            .map_err(ostd_error_to_errno)?;
        read_cstring(&mut user_reader)
    }
}

/// Checks if the user space pointer is below the lowest userspace address.
///
/// If a pointer is below the lowest userspace address, it is likely to be a
/// NULL pointer. Reading from or writing to a NULL pointer should trigger a
/// segmentation fault.
///
/// If it is not checked here, a kernel page fault will happen and we would
/// deny the access in the page fault handler either. It may save a page fault
/// in some occasions. More importantly, double page faults may not be handled
/// quite well on some platforms.
fn check_vaddr(va: Vaddr) -> Result<()> {
    if va < crate::vm::vmar::ROOT_VMAR_LOWEST_ADDR {
        Err(errno_with_message(
            Errno::EFAULT,
            "Bad user space pointer specified",
        ))
    } else {
        Ok(())
    }
}

fn read_cstring(reader: &mut VmReader<'_, Fallible>) -> Result<CString> {
    let max_len = reader.remain();
    let mut buffer: Vec<u8> = Vec::with_capacity(max_len);

    macro_rules! read_one_byte_at_a_time_while {
        ($cond:expr) => {
            while $cond {
                let byte = reader.read_val::<u8>().map_err(ostd_error_to_errno)?;
                buffer.push(byte);
                if byte == 0 {
                    return Ok(CString::from_vec_with_nul(buffer)
                        .expect("We provided 0 but no 0 is found"));
                }
            }
        };
    }

    // Handle the first few bytes to make `cur_addr` aligned with `size_of::<usize>`
    read_one_byte_at_a_time_while!(
        !is_addr_aligned(reader.cursor() as usize) && buffer.len() < max_len
    );

    // Handle the rest of the bytes in bulk
    while (buffer.len() + core::mem::size_of::<usize>()) <= max_len {
        let Ok(word) = reader.read_val::<usize>() else {
            break;
        };

        if has_zero(word) {
            for byte in word.to_ne_bytes() {
                buffer.push(byte);
                if byte == 0 {
                    return Ok(CString::from_vec_with_nul(buffer)
                        .expect("We provided 0 but no 0 is found"));
                }
            }
            unreachable!("The branch should never be reached unless `has_zero` has bugs.")
        }

        buffer.extend_from_slice(&word.to_ne_bytes());
    }

    // Handle the last few bytes that are not enough for a word
    read_one_byte_at_a_time_while!(buffer.len() < max_len);

    // Maximum length exceeded before finding the null terminator
    return_errno_with_message!(Errno::EFAULT, "Fails to read CString from user");
}

/// Determines whether the value contains a zero byte.
///
/// This magic algorithm is from the Linux `has_zero` function:
/// <https://elixir.bootlin.com/linux/v6.0.9/source/include/asm-generic/word-at-a-time.h#L93>
const fn has_zero(value: usize) -> bool {
    const ONE_BITS: usize = usize::from_le_bytes([0x01; core::mem::size_of::<usize>()]);
    const HIGH_BITS: usize = usize::from_le_bytes([0x80; core::mem::size_of::<usize>()]);

    value.wrapping_sub(ONE_BITS) & !value & HIGH_BITS != 0
}

/// Checks if the given address is aligned.
const fn is_addr_aligned(addr: usize) -> bool {
    (addr & (core::mem::size_of::<usize>() - 1)) == 0
}
