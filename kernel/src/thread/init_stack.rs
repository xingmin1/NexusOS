use alloc::{collections::btree_map::BTreeMap, ffi::CString, sync::Arc, vec::Vec};
use nexus_error::ostd_error_to_errno;
use core::{
    mem,
    sync::atomic::{AtomicUsize, Ordering},
};

use align_ext::AlignExt;
use aster_rights::Full;
use ostd::mm::{Vaddr, VmIo, MAX_USERSPACE_VADDR, PAGE_SIZE};

use crate::{
    error::{Errno, Result},
    return_errno_with_message,
    vm::{
        perms::VmPerms,
        vmar::Vmar,
        vmo::{Vmo, VmoOptions, VmoRightsOp},
    },
};

/// The initial portion of the main stack of a process.
pub struct InitStack {
    /// The initial highest address.
    /// The stack grows down from this address
    initial_top: Vaddr,
    /// The max allowed stack size
    max_size: usize,
    /// The current stack pointer.
    /// Before initialized, `pos` points to the `initial_top`,
    /// After initialized, `pos` points to the user stack pointer(rsp)
    /// of the process.
    pos: Arc<AtomicUsize>,
}

/// Set the initial stack size to 8 megabytes, following the default Linux stack size limit.
pub const INIT_STACK_SIZE: usize = 8 * 1024 * 1024; // 8 MB

impl InitStack {
    pub fn new() -> Self {
        let nr_pages_padding = {
            // We do not want the stack top too close to MAX_USERSPACE_VADDR.
            // So we add this fixed padding. Any small value greater than zero will do.
            const NR_FIXED_PADDING_PAGES: usize = 7;

            // TODO: Some random padding pages are added as a simple measure to
            // make the stack values of a buggy user program harder
            // to be exploited by attackers.

            NR_FIXED_PADDING_PAGES
        };
        let initial_top = MAX_USERSPACE_VADDR - PAGE_SIZE * nr_pages_padding;
        let max_size = INIT_STACK_SIZE;

        Self {
            initial_top,
            max_size,
            pos: Arc::new(AtomicUsize::new(initial_top)),
        }
    }

    /// Maps the VMO of the init stack and constructs a writer to initialize its content.
    pub async fn map_and_write(
        &self,
        root_vmar: &Vmar<Full>,
        argv: Vec<CString>,
        envp: Vec<CString>,
        auxvec: AuxVec,
    ) -> Result<()> {
        self.set_uninitialized();

        let vmo = {
            let vmo_options = VmoOptions::<Full>::new(self.max_size);
            vmo_options.alloc()?
        };
        let vmar_map_options = {
            let perms = VmPerms::READ | VmPerms::WRITE;
            let map_addr = self.initial_top - self.max_size;
            debug_assert!(map_addr % PAGE_SIZE == 0);
            root_vmar
                .new_map(self.max_size, perms)?
                .offset(map_addr)
                .vmo(vmo.dup().to_dyn())
        };
        vmar_map_options.build().await?;

        let writer = InitStackWriter {
            pos: self.pos.clone(),
            vmo,
            argv,
            envp,
            auxvec,
            map_addr: self.initial_top - self.max_size,
        };
        writer.write()
    }

    fn set_uninitialized(&self) {
        self.pos.store(self.initial_top, Ordering::Relaxed);
    }

    /// Returns the user stack top(highest address), used to setup rsp.
    ///
    /// This method should only be called after the stack is initialized.
    pub fn user_stack_top(&self) -> Vaddr {
        let stack_top = self.pos();
        debug_assert!(self.is_initialized());

        stack_top
    }

    fn pos(&self) -> Vaddr {
        self.pos.load(Ordering::Relaxed)
    }

    fn is_initialized(&self) -> bool {
        self.pos() != self.initial_top
    }
}

/// A writer to initialize the content of an `InitStack`.
struct InitStackWriter {
    pos: Arc<AtomicUsize>,
    vmo: Vmo<Full>,
    argv: Vec<CString>,
    envp: Vec<CString>,
    auxvec: AuxVec,
    /// The mapping address of the `InitStack`.
    map_addr: usize,
}

impl InitStackWriter {
    fn write(mut self) -> Result<()> {
        // FIXME: Some OSes may put the first page of executable file here
        // for interpreting elf headers.

        let argc = self.argv.len() as u64;

        // Write envp string
        let envp_pointers = self.write_envp_strings()?;
        // Write argv string
        let argv_pointers = self.write_argv_strings()?;
        // Generate random values for auxvec
        let random_value_pointer = {
            // let random_value = generate_random_for_aux_vec();
            let random_value = [0; 16];
            self.write_bytes(&random_value)?
        };
        self.auxvec.set(AuxKey::AT_RANDOM, random_value_pointer)?;

        self.adjust_stack_alignment(&envp_pointers, &argv_pointers)?;
        self.write_aux_vec()?;
        self.write_envp_pointers(envp_pointers)?;
        self.write_argv_pointers(argv_pointers)?;

        // write argc
        self.write_u64(argc)?;

        // Ensure stack top is 16-bytes aligned
        debug_assert_eq!(self.pos() & !0xf, self.pos());

        Ok(())
    }

    fn write_envp_strings(&self) -> Result<Vec<u64>> {
        let mut envp_pointers = Vec::with_capacity(self.envp.len());
        for envp in self.envp.iter() {
            let pointer = self.write_cstring(envp)?;
            envp_pointers.push(pointer);
        }
        Ok(envp_pointers)
    }

    fn write_argv_strings(&self) -> Result<Vec<u64>> {
        let mut argv_pointers = Vec::with_capacity(self.argv.len());
        for argv in self.argv.iter().rev() {
            let pointer = self.write_cstring(argv)?;
            // debug!("argv address = 0x{:x}", pointer);
            argv_pointers.push(pointer);
        }
        argv_pointers.reverse();
        Ok(argv_pointers)
    }

    /// Libc ABI requires 16-byte alignment of the stack entrypoint.
    /// Current position of the stack is 8-byte aligned already, insert 8 byte
    /// to meet the requirement if necessary.
    fn adjust_stack_alignment(&self, envp_pointers: &[u64], argv_pointers: &[u64]) -> Result<()> {
        // Ensure 8-byte alignment
        self.write_u64(0)?;
        let auxvec_size = (self.auxvec.table().len() + 1) * (mem::size_of::<u64>() * 2);
        let envp_pointers_size = (envp_pointers.len() + 1) * mem::size_of::<u64>();
        let argv_pointers_size = (argv_pointers.len() + 1) * mem::size_of::<u64>();
        let argc_size = mem::size_of::<u64>();
        let to_write_size = auxvec_size + envp_pointers_size + argv_pointers_size + argc_size;
        if (self.pos() - to_write_size) % 16 != 0 {
            self.write_u64(0)?;
        }
        Ok(())
    }

    fn write_aux_vec(&self) -> Result<()> {
        // Write NULL auxiliary
        self.write_u64(0)?;
        self.write_u64(AuxKey::AT_NULL as u64)?;
        // Write Auxiliary vectors
        let aux_vec: Vec<_> = self
            .auxvec
            .table()
            .iter()
            .map(|(aux_key, aux_value)| (*aux_key, *aux_value))
            .collect();
        for (aux_key, aux_value) in aux_vec.iter() {
            self.write_u64(*aux_value)?;
            self.write_u64(*aux_key as u64)?;
        }
        Ok(())
    }

    fn write_envp_pointers(&self, mut envp_pointers: Vec<u64>) -> Result<()> {
        // write NULL pointer
        self.write_u64(0)?;
        // write envp pointers
        envp_pointers.reverse();
        for envp_pointer in envp_pointers {
            self.write_u64(envp_pointer)?;
        }
        Ok(())
    }

    fn write_argv_pointers(&self, mut argv_pointers: Vec<u64>) -> Result<()> {
        // write 0
        self.write_u64(0)?;
        // write argv pointers
        argv_pointers.reverse();
        for argv_pointer in argv_pointers {
            self.write_u64(argv_pointer)?;
        }
        Ok(())
    }

    /// Writes u64 to the stack.
    /// Returns the writing address
    fn write_u64(&self, val: u64) -> Result<u64> {
        let start_address = (self.pos() - 8).align_down(8);
        self.pos.store(start_address, Ordering::Relaxed);
        self.vmo.write_val(start_address - self.map_addr, &val).map_err(ostd_error_to_errno)?;
        Ok(self.pos() as u64)
    }

    /// Writes a CString including the ending null byte to the stack.
    /// Returns the writing address
    fn write_cstring(&self, val: &CString) -> Result<u64> {
        let bytes = val.as_bytes_with_nul();
        self.write_bytes(bytes)
    }

    /// Writes u64 to the stack.
    /// Returns the writing address.
    fn write_bytes(&self, bytes: &[u8]) -> Result<u64> {
        let len = bytes.len();
        self.pos.fetch_sub(len, Ordering::Relaxed);
        let pos = self.pos();
        self.vmo.write_bytes(pos - self.map_addr, bytes).map_err(ostd_error_to_errno)?;
        Ok(pos as u64)
    }

    fn pos(&self) -> Vaddr {
        self.pos.load(Ordering::Relaxed)
    }
}

/// Auxiliary Vector.
///
/// # What is Auxiliary Vector?
///
/// Here is a concise description of Auxiliary Vector from GNU's manual:
///
///  > When a program is executed, it receives information from the operating system
///  > about the environment in which it is operating. The form of this information
///  > is a table of key-value pairs, where the keys are from the set of ‘AT_’
///  > values in elf.h.
#[expect(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum AuxKey {
    AT_NULL = 0,      /* end of vector */
    AT_IGNORE = 1,    /* entry should be ignored */
    AT_EXECFD = 2,    /* file descriptor of program */
    AT_PHDR = 3,      /* program headers for program */
    AT_PHENT = 4,     /* size of program header entry */
    AT_PHNUM = 5,     /* number of program headers */
    AT_PAGESZ = 6,    /* system page size */
    AT_BASE = 7,      /* base address of interpreter */
    AT_FLAGS = 8,     /* flags */
    AT_ENTRY = 9,     /* entry point of program */
    AT_NOTELF = 10,   /* program is not ELF */
    AT_UID = 11,      /* real uid */
    AT_EUID = 12,     /* effective uid */
    AT_GID = 13,      /* real gid */
    AT_EGID = 14,     /* effective gid */
    AT_PLATFORM = 15, /* string identifying CPU for optimizations */
    AT_HWCAP = 16,    /* arch dependent hints at CPU capabilities */
    AT_CLKTCK = 17,   /* frequency at which times() increments */

    /* 18...22 not used */
    AT_SECURE = 23, /* secure mode boolean */
    AT_BASE_PLATFORM = 24, /* string identifying real platform, may
                     * differ from AT_PLATFORM. */
    AT_RANDOM = 25, /* address of 16 random bytes */
    AT_HWCAP2 = 26, /* extension of AT_HWCAP */

    /* 28...30 not used */
    AT_EXECFN = 31, /* filename of program */
    AT_SYSINFO = 32,
    AT_SYSINFO_EHDR = 33, /* the start address of the page containing the VDSO */
}

impl AuxKey {
    pub fn as_u64(&self) -> u64 {
        *self as u64
    }
}

#[derive(Clone, Default, Debug)]
pub struct AuxVec {
    table: BTreeMap<AuxKey, u64>,
}

impl AuxVec {
    pub const fn new() -> AuxVec {
        AuxVec {
            table: BTreeMap::new(),
        }
    }
}

impl AuxVec {
    pub fn set(&mut self, key: AuxKey, val: u64) -> Result<()> {
        if key == AuxKey::AT_NULL || key == AuxKey::AT_IGNORE {
            return_errno_with_message!(Errno::EINVAL, "Illegal key");
        }
        self.table
            .entry(key)
            .and_modify(|val_mut| *val_mut = val)
            .or_insert(val);
        Ok(())
    }

    pub fn get(&self, key: AuxKey) -> Option<u64> {
        self.table.get(&key).copied()
    }

    pub fn del(&mut self, key: AuxKey) -> Option<u64> {
        self.table.remove(&key)
    }

    pub fn table(&self) -> &BTreeMap<AuxKey, u64> {
        &self.table
    }
}
