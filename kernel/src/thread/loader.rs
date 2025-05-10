use alloc::{ffi::CString, vec, vec::Vec};

use align_ext::AlignExt;
use aster_rights::{Full, Rights};
use elf_loader::{
    abi::{PF_R, PF_W, PF_X, PT_LOAD},
    arch::ElfPhdr,
    mmap::MmapImpl,
    object::ElfBinary,
    Loader,
};
use ostd::mm::{Vaddr, VmIo, PAGE_SIZE};
use tracing::{error, trace};

use super::init_stack::{AuxKey, AuxVec};
use crate::{
    error::Result,
    vm::{perms::VmPerms, util::duplicate_frame, vmar::Vmar, vmo::VmoOptions, ProcessVm},
};

/// 将 elf 加载到进程虚拟内存 (vm) 中。
///
/// 这个函数会映射 elf 段 (segments) 并且
/// 初始化进程的初始栈。
pub async fn load_elf_to_vm(
    process_vm: &ProcessVm, // 输入：目标进程的虚拟内存管理器
    // file_header: &[u8],     // 输入：ELF 文件的头部字节（用于快速解析）
    // elf_file: Dentry,       // 输入：代表 ELF 文件的目录项（用于读取完整内容）
    // fs_resolver: &FsResolver, // 输入：文件系统路径解析器（用于查找 ld.so）
    argv: Vec<CString>, // 输入：程序的命令行参数列表
    envp: Vec<CString>, // 输入：程序的环境变量列表
) -> Result<ElfLoadInfo> {
    // 输出：成功时返回 ELF 加载信息，失败时返回错误
    // 1. 解析 ELF 文件头
    //    使用提供的 `file_header` 来解析 ELF 文件的基本结构。
    //    `Elf::parse_elf` 会检查文件格式是否正确，并提取出程序头（Program Headers）等信息。
    // let parsed_elf = Elf::parse_elf(file_header)?; // `?` 表示如果解析失败，函数会提前返回错误
    // 解析ELF文件
    let program_binary = include_bytes!("../../hello");
    let mut elf = ElfBinary::new("hello", program_binary);

    // 2. 查找并解析动态链接器 (ld.so)
    //    检查 `parsed_elf` 是否需要一个动态链接器。如果是，就使用 `fs_resolver`
    //    根据 ELF 文件中指定的路径查找 ld.so 文件，并解析它。
    //    `ldso` 会是 `Option<(Dentry, Elf)>` 类型，表示可能找到也可能没找到。
    // let ldso = lookup_and_parse_ldso(&parsed_elf, file_header, fs_resolver)?;

    // 3. 初始化并映射虚拟内存对象 (VMOs)
    //    这是核心步骤，调用 `init_and_map_vmos` 函数（这个函数不在这段代码里，但在别处定义）。
    //    这个函数负责：
    //    a. 为 ELF 文件和 ld.so (如果存在) 的 "LOAD" 段创建 VMO。
    //    b. 将这些 VMO 映射到 `process_vm` 的地址空间中。
    //    c. 初始化一个叫做 AuxVec (Auxiliary Vector) 的数据结构，它包含了内核传递给用户空间程序的重要信息（如入口点、页大小等）。
    //    d. 确定最终的程序入口点地址（可能是 ELF 自己的入口点，也可能是 ld.so 的入口点）。
    match init_and_map_vmos(process_vm, &mut elf, program_binary).await {
        // 4. 处理成功情况
        Ok((entry_point, aux_vec)) => {
            // 4.1 映射 VDSO (Virtual Dynamic Shared Object)
            //     VDSO 是内核提供的一个特殊共享对象，包含一些可以在用户空间直接调用的内核函数（优化系统调用）。
            //     这里尝试将 VDSO 映射到进程空间，并将其地址记录在 AuxVec 中。
            // if let Some(vdso_text_base) = map_vdso_to_vm(process_vm) {
            //     aux_vec
            //         .set(AuxKey::AT_SYSINFO_EHDR, vdso_text_base as u64)
            //         .unwrap(); // `unwrap` 在这里假设设置总能成功
            // }

            // 4.2 设置初始栈
            //     将命令行参数 (`argv`)、环境变量 (`envp`) 和 AuxVec (`aux_vec`)
            //     写入到进程的用户栈顶。这是程序启动时首先会访问的数据。
            process_vm
                .map_and_write_init_stack(argv, envp, aux_vec)
                .await
                .inspect_err(|e| {
                    error!("map_and_write_init_stack 失败: {:?}", e);
                })?;

            // 4.3 准备返回信息
            //     获取用户栈的最终栈顶地址。
            let user_stack_top = process_vm.user_stack_top();
            //     打包入口点地址和栈顶地址到 `ElfLoadInfo` 结构体中，并成功返回。
            Ok(ElfLoadInfo::new(entry_point, user_stack_top))
        }
        // 5. 处理错误情况
        Err(err) => {
            error!("in load_elf_to_vm 失败: {:?}", err);
            // 如果 `init_and_map_vmos` 失败了，说明进程的内存状态可能已经不一致或损坏了。
            // 进程无法安全地返回用户空间继续执行。

            // 5.1 清理内存
            //     调用 `process_vm.root_vmar().clear()` 来清除之前可能创建的所有内存映射，
            //     尝试恢复到一个干净的状态（虽然进程马上要退出）。
            //     FIXME 注释表明这里未来可能考虑发送一个错误信号给进程。
            process_vm.root_vmar().clear().await.unwrap(); // 假设清理总能成功

            // 5.2 退出进程组
            //     调用 `do_exit_group` 来终止整个进程组。
            //     FIXME 注释提到了在初始化进程（init process）中使用 `current` 宏可能引发 panic 的问题，
            //     以及如何确定正确的退出状态码的问题，这些都需要后续处理。
            // do_exit_group(TermStatus::Exited(1)); // 使用硬编码的退出码 1

            // 5.3 返回错误
            //     虽然进程已经开始退出了，但函数仍然需要返回一个错误结果。
            //     这个错误最终可能不会被实际使用，因为进程已经终止了。
            Err(err)
        }
    }
}

async fn init_and_map_vmos(
    process_vm: &ProcessVm,
    // ldso: Option<(Dentry, Elf)>,
    parsed_elf: &mut ElfBinary<'_>,
    elf_file: &'static [u8],
) -> Result<(Vaddr, AuxVec)> {
    let root_vmar = process_vm.root_vmar();

    // After we clear process vm, if any error happens, we must call exit_group instead of return to user space.
    // let ldso_load_info = if let Some((ldso_file, ldso_elf)) = ldso {
    //     Some(load_ldso(root_vmar, &ldso_file, &ldso_elf)?)
    // } else {
    //     None
    // };

    let elf_map_addr = map_segment_vmos(parsed_elf, root_vmar, elf_file)
        .await
        .inspect_err(|e| {
            error!("map_segment_vmos 失败: {:?}", e);
        })?;

    let aux_vec = {
        // let ldso_base = ldso_load_info
        //     .as_ref()
        //     .map(|load_info| load_info.base_addr());
        init_aux_vec(parsed_elf, elf_map_addr, None).inspect_err(|e| {
            error!("init_aux_vec 失败: {:?}", e);
        })?
    };

    // let entry_point = if let Some(ldso_load_info) = ldso_load_info {
    //     // Normal shared object
    //     ldso_load_info.entry_point()
    // } else if parsed_elf.is_shared_object() {
    //     // ldso itself
    //     parsed_elf.entry_point() + elf_map_addr
    // } else {
    //     // statically linked executable
    //     parsed_elf.entry_point()
    // };
    let mut loader = Loader::<MmapImpl>::new();
    let elf_header = loader.read_ehdr(&mut *parsed_elf).unwrap();
    let entry_point = elf_header.e_entry as usize;

    Ok((entry_point, aux_vec))
}
/// Inits VMO for each segment and then map segment to root vmar
pub async fn map_segment_vmos(
    elf: &mut ElfBinary<'_>,
    root_vmar: &Vmar<Full>,
    elf_file: &'static [u8],
) -> Result<Vaddr> {
    // all segments of the shared object must be mapped to a continuous vm range
    // to ensure the relative offset of each segment not changed.
    // let base_addr = if elf.is_shared_object() {
    //     base_map_addr(elf, root_vmar)?
    // } else {
    //     0
    // };
    let base_addr = 0;
    let elf_pheader = {
        let mut loader = Loader::<MmapImpl>::new();
        let elf_header = loader.read_ehdr(&mut *elf).unwrap();
        // 读取程序头，loader 在此代码块结束时被销毁
        loader.read_phdr(&mut *elf, &elf_header).unwrap().to_vec()
    };

    for program_header in elf_pheader {
        if program_header.p_type == PT_LOAD {
            // check_segment_align(program_header)?;
            map_segment_vmo(program_header, elf_file, root_vmar, base_addr)
                .await
                .inspect_err(|e| {
                    error!("map_segment_vmo 失败: {:?}", e);
                })?;
        }
    }
    Ok(base_addr)
}
/// Creates and map the corresponding segment VMO to `root_vmar`.
/// If needed, create additional anonymous mapping to represents .bss segment.
async fn map_segment_vmo(
    program_header: ElfPhdr,
    elf_file: &'static [u8],
    root_vmar: &Vmar<Full>,
    base_addr: Vaddr,
) -> Result<()> {
    // trace!(
    //     "mem range = 0x{:x} - 0x{:x}, mem_size = 0x{:x}",
    //     program_header.virtual_addr,
    //     program_header.virtual_addr + program_header.mem_size,
    //     program_header.mem_size
    // );
    // trace!(
    //     "file range = 0x{:x} - 0x{:x}, file_size = 0x{:x}",
    //     program_header.offset,
    //     program_header.offset + program_header.file_size,
    //     program_header.file_size
    // );

    let file_offset = program_header.p_offset as usize;
    let virtual_addr = program_header.p_vaddr as usize;
    debug_assert!(file_offset % PAGE_SIZE == virtual_addr % PAGE_SIZE);
    let segment_vmo = {
        // let inode = elf_file.inode();
        // inode
        //     .page_cache()
        //     .ok_or(Error::with_message(
        //         Errno::ENOENT,
        //         "executable has no page cache",
        //     ))?
        //     .to_dyn()
        //     .dup_independent()?)
        trace!("program_header.p_memsz = {:?}", program_header.p_memsz);
        trace!("program_header.p_filesz = {:?}", program_header.p_filesz);
        let vmo_options = VmoOptions::<Rights>::new(program_header.p_memsz as usize);
        let vmo = vmo_options.alloc().inspect_err(|e| {
            error!("vmo_options.alloc 失败: {:?}", e);
        })?;
        vmo.write_slice(
            file_offset % PAGE_SIZE,
            elf_file[file_offset..(file_offset + program_header.p_filesz as usize)].as_ref(),
        )
        .inspect_err(|e| {
            error!("vmo.write_slice 失败: {:?}", e);
        })?;
        vmo
    };

    let total_map_size = {
        let vmap_start = virtual_addr.align_down(PAGE_SIZE);
        let vmap_end = (virtual_addr + program_header.p_memsz as usize).align_up(PAGE_SIZE);
        vmap_end - vmap_start
    };

    let (segment_offset, segment_size) = {
        let start = file_offset.align_down(PAGE_SIZE);
        let end = (file_offset + program_header.p_filesz as usize).align_up(PAGE_SIZE);
        debug_assert!(total_map_size >= (program_header.p_filesz as usize).align_up(PAGE_SIZE));
        (start, end - start)
    };

    // Write zero as paddings. There are head padding and tail padding.
    // Head padding: if the segment's virtual address is not page-aligned,
    // then the bytes in first page from start to virtual address should be padded zeros.
    // Tail padding: If the segment's mem_size is larger than file size,
    // then the bytes that are not backed up by file content should be zeros.(usually .data/.bss sections).

    // Head padding.
    let page_offset = file_offset % PAGE_SIZE;
    if page_offset != 0 {
        let new_frame = {
            let head_frame = segment_vmo.commit_page(segment_offset).await?;
            let new_frame = duplicate_frame(&head_frame)?;

            let buffer = vec![0u8; page_offset];
            new_frame.write_bytes(0, &buffer).unwrap();
            new_frame
        };
        let head_idx = segment_offset / PAGE_SIZE;
        segment_vmo
            .replace(new_frame.into(), head_idx)
            .await
            .inspect_err(|e| {
                error!("segment_vmo.replace 失败: {:?}", e);
            })?;
    }

    // Tail padding.
    let tail_padding_offset = program_header.p_filesz as usize + page_offset;
    if segment_size > tail_padding_offset {
        let new_frame = {
            let tail_frame = segment_vmo
                .commit_page(segment_offset + tail_padding_offset)
                .await
                .inspect_err(|e| {
                    error!("segment_vmo.commit_page 失败: {:?}", e);
                })?;
            let new_frame = duplicate_frame(&tail_frame)?;

            let buffer = vec![0u8; (segment_size - tail_padding_offset) % PAGE_SIZE];
            new_frame
                .write_bytes(tail_padding_offset % PAGE_SIZE, &buffer)
                .unwrap();
            new_frame
        };

        let tail_idx = (segment_offset + tail_padding_offset) / PAGE_SIZE;
        segment_vmo
            .replace(new_frame.into(), tail_idx)
            .await
            .unwrap();
    }

    let perms = parse_segment_perm(program_header.p_flags);
    let offset = base_addr + (program_header.p_vaddr as Vaddr).align_down(PAGE_SIZE);
    if segment_size != 0 {
        let mut vm_map_options = root_vmar
            .new_map(segment_size, perms)?
            .vmo(segment_vmo)
            .vmo_offset(segment_offset)
            .vmo_limit(segment_offset + segment_size)
            .can_overwrite(true);
        vm_map_options = vm_map_options.offset(offset).handle_page_faults_around();
        vm_map_options.build().await.inspect_err(|e| {
            error!("vm_map_options.build 失败: {:?}", e);
        })?;
    }

    let anonymous_map_size: usize = total_map_size.saturating_sub(segment_size);

    if anonymous_map_size > 0 {
        let mut anonymous_map_options = root_vmar
            .new_map(anonymous_map_size, perms)?
            .can_overwrite(true);
        anonymous_map_options = anonymous_map_options.offset(offset + segment_size);
        anonymous_map_options.build().await.inspect_err(|e| {
            error!("anonymous_map_options.build 失败: {:?}", e);
        })?;
    }
    Ok(())
}

fn parse_segment_perm(flags: u32) -> VmPerms {
    // 确定段的权限
    let mut vm_perm = VmPerms::empty();

    // 根据ELF段标志设置页面权限
    if flags & PF_R != 0 {
        vm_perm |= VmPerms::READ;
    }
    if flags & PF_W != 0 {
        vm_perm |= VmPerms::WRITE;
    }
    if flags & PF_X != 0 {
        vm_perm |= VmPerms::EXEC;
    }
    vm_perm
}

pub fn init_aux_vec(
    elf: &mut ElfBinary,
    _elf_map_addr: Vaddr,
    ldso_base: Option<Vaddr>,
) -> Result<AuxVec> {
    let mut loader = Loader::<MmapImpl>::new();
    let elf_header = loader.read_ehdr(&mut *elf).unwrap();

    let mut aux_vec = AuxVec::new();
    aux_vec.set(AuxKey::AT_PAGESZ, PAGE_SIZE as _)?;
    // let ph_addr = if elf.is_shared_object() {
    //     elf_header.e_phoff as usize + elf_map_addr
    // } else {
    //     elf_header.e_phoff as usize
    // };
    let ph_addr = elf_header.e_phoff as usize;
    aux_vec.set(AuxKey::AT_PHDR, ph_addr as u64)?;
    aux_vec.set(AuxKey::AT_PHNUM, elf_header.e_phnum as u64)?;
    aux_vec.set(AuxKey::AT_PHENT, elf_header.e_phentsize as u64)?;
    // let elf_entry = if elf.is_shared_object() {
    //     let base_load_offset = elf.base_load_address_offset();
    //     elf_header.e_entry as usize + elf_map_addr - base_load_offset as usize
    // } else {
    //     elf_header.e_entry as usize
    // };
    let elf_entry = elf_header.e_entry as usize;
    aux_vec.set(AuxKey::AT_ENTRY, elf_entry as u64)?;

    if let Some(ldso_base) = ldso_base {
        aux_vec.set(AuxKey::AT_BASE, ldso_base as u64)?;
    }
    Ok(aux_vec)
}

pub struct ElfLoadInfo {
    entry_point: Vaddr,
    user_stack_top: Vaddr,
}

impl ElfLoadInfo {
    pub fn new(entry_point: Vaddr, user_stack_top: Vaddr) -> Self {
        Self {
            entry_point,
            user_stack_top,
        }
    }

    pub fn entry_point(&self) -> Vaddr {
        self.entry_point
    }

    pub fn user_stack_top(&self) -> Vaddr {
        self.user_stack_top
    }
}
