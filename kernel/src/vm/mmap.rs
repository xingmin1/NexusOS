use alloc::{format, vec};
use core::ops::ControlFlow;

use bitflags::bitflags;
use aster_rights::Rights;
use nexus_error::{return_errno, return_errno_with_message, Errno, Result};
use ostd::user::UserContextApi;
use tracing::warn;
use ostd::mm::VmIo;
use crate::{
    thread::ThreadState,
    vm::{
        perms::VmPerms,
        vmar::{is_userspace_vaddr, VmarMapOptions},
        vmo::{VmoOptions, VmoRightsOp},
    },
};
use crate::error::error_stack::{Report, ResultExt};

const PAGE_SIZE: usize = ostd::mm::PAGE_SIZE;

/// 低 4bit 表示映射类型
const MAP_TYPE_MASK: u32 = 0xf;

#[derive(Copy, Clone, PartialEq, Debug)]
#[repr(u8)]
enum MMapType {
    File = 0,
    Shared = 1,
    Private = 2,
    SharedValidate = 3,
}

bitflags! {
    struct MMapFlags: u32 {
        const MAP_FIXED           = 0x10;
        const MAP_ANONYMOUS       = 0x20;
        const MAP_32BIT           = 0x40;
        const MAP_FIXED_NOREPLACE = 0x100000;
    }
}

struct MMapOpts {
    typ:  MMapType,
    flags: MMapFlags,
}

impl TryFrom<u32> for MMapOpts {
    type Error = Report<nexus_error::Error>;
    fn try_from(raw: u32) -> core::prelude::rust_2024::Result<Self, Self::Error> {
        let typ = match (raw & MAP_TYPE_MASK) as u8 {
            0 => MMapType::File,
            1 => MMapType::Shared,
            2 => MMapType::Private,
            3 => MMapType::SharedValidate,
            _ => return_errno_with_message!(Errno::EINVAL, "unknown map type"),
        };
        let Some(flags) = MMapFlags::from_bits(raw & !MAP_TYPE_MASK) else {
            return_errno_with_message!(Errno::EINVAL, "unknown mmap flags");
        };
        Ok(Self { typ, flags })
    }
}

pub async fn do_mmap(
    state: &ThreadState,
    cx: &mut ostd::cpu::UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    let [addr, len, prot, flags, fd, offset] = cx.syscall_arguments();

    if len == 0 {
        return_errno_with_message!(Errno::EINVAL, "len cannot be zero");
    }
    let len = len.next_multiple_of(PAGE_SIZE);
    let addr = addr;
    let prot = VmPerms::from_bits_truncate(prot as u32);
    let mut opts = MMapOpts::try_from(flags as u32)?;
    if opts.flags.contains(MMapFlags::MAP_FIXED_NOREPLACE) {
        opts.flags |= MMapFlags::MAP_FIXED;
    }
    if opts.flags.contains(MMapFlags::MAP_FIXED) && !is_userspace_vaddr(addr) {
        return_errno_with_message!(Errno::EINVAL, "MAP_FIXED addr invalid");
    }
    if offset % PAGE_SIZE != 0 {
        return_errno_with_message!(Errno::EINVAL, "offset must be page‑aligned");
    }

    let root = state.process_vm.root_vmar();
    let mut map_opt = root.new_map(len, prot)?;
    if opts.flags.contains(MMapFlags::MAP_FIXED) {
        map_opt = map_opt.offset(addr).can_overwrite(!opts.flags.contains(MMapFlags::MAP_FIXED_NOREPLACE));
    } else if opts.flags.contains(MMapFlags::MAP_32BIT) {
        warn!("MAP_32BIT 未实现，退化为默认策略");
    }

    match (opts.flags.contains(MMapFlags::MAP_ANONYMOUS), opts.typ) {
        (true, _) => anonymous_mapping(len, prot, opts.typ, map_opt, offset).await,
        (false, _) => file_mapping(state, len, fd as i32, prot, opts.typ, map_opt, offset).await,
    }
    .map(|addr| ControlFlow::Continue(Some(addr as isize)))
}

/// 匿名映射
async fn anonymous_mapping<R>(
    len: usize,
    _prot: VmPerms,
    typ: MMapType,
    mut map_opt: VmarMapOptions<R, Rights>,
    offset: usize,
) -> Result<usize> {
    if offset != 0 {
        return_errno_with_message!(Errno::EINVAL, "anonymous offset must be 0");
    }

    // 共享匿名映射应共用同一 VMO
    if matches!(typ, MMapType::Shared) {
        let vmo = VmoOptions::<Rights>::new(len).alloc()?.to_dyn();
        map_opt = map_opt.is_shared(true).vmo(vmo);
    }
    map_opt.build().await
}

/// 文件映射
async fn file_mapping<R>(
    state: &ThreadState,
    len: usize,
    fd: i32,
    prot: VmPerms,
    typ: MMapType,
    mut map_opt: VmarMapOptions<R, Rights>,
    offset: usize,
) -> Result<usize> {
    let entry = state.fd_table.get(fd as u32).await?;
    let inode = entry.obj.as_file().ok_or_else(|| {
        nexus_error::Error::new(Errno::EBADF)
    }).attach_printable_lazy(|| format!("fd {} is not a file", fd))?;
    let mode = inode.flags().access();

    // 权限检查，写共享必须文件可写
    if prot.contains(VmPerms::READ) && !mode.is_readable() {
        return_errno !(Errno::EACCES);
    }
    if matches!(typ, MMapType::Shared) && prot.contains(VmPerms::WRITE) && !mode.is_writable() {
        return_errno!(Errno::EACCES);
    }
    let mut file = vec![0; len];
    inode.read_at(offset as u64, &mut file).await.map_err(|e| {
        warn!("failed to read file: {}", e);
        nexus_error::Error::new(Errno::EIO)
    })?;
    let vmo = VmoOptions::<Rights>::new(file.len()).alloc()?.to_dyn();
    vmo.write_slice(0, &file).change_context_lazy(|| {
        nexus_error::Error::new(Errno::EIO)
    })?;
    map_opt = map_opt
        .vmo(vmo)
        .vmo_offset(offset)
        .handle_page_faults_around();
    if matches!(typ, MMapType::Shared) {
        map_opt = map_opt.is_shared(true);
    }
    map_opt.build().await
}
