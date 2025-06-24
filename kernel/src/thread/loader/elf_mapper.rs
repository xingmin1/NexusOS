//! 将一个 `ElfBinary` 映射到目标 `ProcessVm`，并生成入口点 + 用户栈信息。

use crate::{
    error::Result, thread::{init_stack::{AuxKey, AuxVec}, loader::ElfImage}, vm::{perms::VmPerms, vmo::VmoOptions, ProcessVm}
};
use aster_rights::Rights;
use elf_loader::{
    abi::{PF_R, PF_W, PF_X, PT_LOAD},
    arch::{ElfPhdr},
    mmap::MmapImpl,
    Loader,
};
use nexus_error::{error_stack::Report, ostd_error_to_errno, Errno};
use ostd::mm::{Vaddr, VmIo, PAGE_SIZE};
use alloc::{ffi::CString, format, vec::Vec};

pub struct ElfLoadInfo {
    pub entry: Vaddr,
    pub user_sp: Vaddr,
}

pub struct ElfMapper<'a> {
    pub image: &'a ElfImage,
    pub vm:    &'a ProcessVm,
}

impl<'a> ElfMapper<'a> {
    pub async fn map(self, argv: Vec<CString>, envp: Vec<CString>) -> Result<ElfLoadInfo> {
        let mut loader = Loader::<MmapImpl>::new();

        let mut binary = self.image.as_binary();

        // 解析 ELF Header / PHDR
        let ehdr  = loader.read_ehdr(&mut binary).map_err(elf_loader_error_to_errno)?;
        let phdrs = loader.read_phdr(&mut binary, &ehdr).map_err(elf_loader_error_to_errno)?;

        for ph in phdrs.iter().filter(|p| p.p_type == PT_LOAD) {
            self.map_segment(ph).await?;
        }

        // 构建并写入初始栈
        let mut aux = AuxVec::new();
        aux.set(AuxKey::AT_PAGESZ, PAGE_SIZE as _)?;
        aux.set(AuxKey::AT_PHDR  , phdrs.as_ptr() as u64)?;
        aux.set(AuxKey::AT_PHNUM , ehdr.e_phnum as _)?;
        aux.set(AuxKey::AT_PHENT , ehdr.e_phentsize as _)?;
        aux.set(AuxKey::AT_ENTRY , ehdr.e_entry as _)?;

        self.vm.map_and_write_init_stack(argv, envp, aux).await?;

        Ok(ElfLoadInfo {
            entry:   ehdr.e_entry as _,
            user_sp: self.vm.user_stack_top(),
        })
    }

    async fn map_segment(&self, ph: &ElfPhdr) -> Result<()> {
        let file_off = ph.p_offset as usize;
        let virt     = ph.p_vaddr as usize;

        // 创建 VMO 并复制文件内容
        let vmo = {
            let opt = VmoOptions::<Rights>::new(ph.p_memsz as usize);
            let v   = opt.alloc()?;
            v.write_slice(0, &self.image.as_slice()[file_off .. file_off + ph.p_filesz as usize]).map_err(ostd_error_to_errno)?;
            v
        };

        // 权限解析
        let mut perms = VmPerms::empty();
        if ph.p_flags & PF_R != 0 { perms |= VmPerms::READ;  }
        if ph.p_flags & PF_W != 0 { perms |= VmPerms::WRITE; }
        if ph.p_flags & PF_X != 0 { perms |= VmPerms::EXEC;  }

        // 计算映射范围
        let map_size  = (ph.p_memsz as usize + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let vm_offset = virt & !(PAGE_SIZE - 1);

        self.vm.root_vmar()
            .new_map(map_size, perms)?
            .offset(vm_offset)
            .vmo(vmo)
            .build()
            .await?;
        Ok(())
    }
}

fn elf_loader_error_to_errno(error: elf_loader::Error) -> Report<nexus_error::Error> {
    let errno = match error {
        elf_loader::Error::MmapError { msg: _ } => Errno::ENOMEM,
        elf_loader::Error::RelocateError { msg: _, custom_err: _ } => Errno::ENOMEM,
        elf_loader::Error::ParseDynamicError { msg: _ } => Errno::ENOMEM,
        elf_loader::Error::ParseEhdrError { msg: _ } => Errno::ENOMEM,
        elf_loader::Error::ParsePhdrError { msg: _, custom_err: _ } => Errno::ENOMEM,
    };
    Report::new(nexus_error::Error::new(errno)).attach_printable(format!("elf-loader error: {:?}", error))
}
