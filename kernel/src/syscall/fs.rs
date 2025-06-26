#![allow(non_upper_case_globals)]

use core::ops::ControlFlow;
use alloc::{string::String, vec};
use nexus_error::{
    errno_with_message, error_stack::ResultExt, ostd_error_to_errno, ostd_tuple_to_errno, return_errno_with_message, Errno, Error, Result
};
use ostd::{
    cpu::UserContext, mm::{FallibleVmRead, VmReader, VmWriter}, user::UserContextApi, Pod
};
use vfs::{
    self, get_path_resolver, FileMode, FileOpen, PathBuf, PathSlice, SVnode, VnodeType
};
use crate::{
    thread::{
        fd_table::{FdEntry, FdObject},
        ThreadState,
    },
    vm::ProcessVm,
};

pub const AT_FDCWD: i32 = -100; // 相对当前工作目录

/// 从用户地址复制 NUL 结尾字符串到内核。
fn copy_cstr_from_user(vm: &ProcessVm, uaddr: usize) -> Result<String> {
    vm.read_cstring(uaddr, 4096)?
        .into_string()
        .map_err(|_| errno_with_message(Errno::EINVAL, "path is not a valid utf-8 string"))
}

/// 成功返回新分配的进程级 fd
pub async fn do_openat(
    state: &mut ThreadState,
    cx: &mut UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    let [dirfd, path_ptr, flags, _mode, ..] = cx.syscall_arguments();
    
    let fo = FileOpen::new(flags as u32)
        .change_context_lazy(|| Error::with_message(Errno::EINVAL, "invalid open flags"))?;

    let raw = copy_cstr_from_user(&state.process_vm, path_ptr)?;
    // 绝对路径直接返回
    let (vnode, path, dir) = if raw.starts_with('/') {
        let mut path = PathBuf::new(&raw)?;
        (get_path_resolver().resolve(&mut path).await, Some(path), None)
    } else if dirfd == AT_FDCWD as _ || raw.starts_with("./") || raw == "." {
        let mut path = state.cwd.as_slice().join(&raw)?;
        (get_path_resolver().resolve(&mut path).await, Some(path), None)
    } else {
        let entry = state.fd_table.get(dirfd as u32).await?;
        let dir = entry
            .obj
            .as_dir()
            .ok_or_else(|| errno_with_message(Errno::ENOTDIR, "dirfd not a directory"))?;
        
        (dir.vnode().lookup(raw.as_str()).await, None, Some(dir.clone()))
    };

    let vnode = if let Err(e) = vnode {
        if e.downcast_ref::<Error>().map(|e| e.error() as _).unwrap_or(-1) == Errno::ENOENT as i32 && fo.should_create() {
            if let Some(path) = path {
                let dir = get_path_resolver().resolve(&mut path.as_slice().strip_suffix().unwrap_or(PathSlice::from("/")).to_owned_buf()).await?;
                let dir = dir.as_dir().ok_or_else(|| errno_with_message(Errno::ENOTDIR, "dirfd not a directory"))?;
                dir.create(raw.as_str(), VnodeType::File, FileMode::from_bits_truncate(0o666), None).await?
            } else if let Some(dir) = dir {
                dir.vnode().create(raw.as_str(), VnodeType::File, FileMode::from_bits_truncate(0o666), None).await?
            } else {
                return Err(e);
            }
        } else {
            return Err(e);
        }
    } else {
        vnode.unwrap()
    };

    let fd = if vnode.is_dir() || fo.is_directory() {
        state.fd_table.alloc(
            FdEntry::new_dir(
                vnode.to_dir().unwrap().open_dir().await?, 
                fo.into()
            ), 0
        ).await?
    } else {
        state.fd_table.alloc(
            FdEntry::new_file(
                vnode.to_file().unwrap().open(fo.into()).await?,
                fo.into()
            ), 0
        ).await?
    };
    Ok(ControlFlow::Continue(Some(fd as isize)))
}

pub async fn do_close(state: &ThreadState, cx: &mut UserContext) -> Result<ControlFlow<i32, Option<isize>>> {
    let [fd, ..] = cx.syscall_arguments();
    state.fd_table.close(fd as u32).await?;
    Ok(ControlFlow::Continue(Some(0)))
}

pub async fn do_read(
    state: &ThreadState,
    cx: &mut UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    let [fd, buf_ptr, len, ..] = cx.syscall_arguments();
    let entry = state.fd_table.get(fd as u32).await?;
    let file = match entry.obj {
        FdObject::File(ref fh) => fh.clone(),
        _ => return_errno_with_message!(Errno::EINVAL, "fd not file"),
    };
    // 从用户空间拿出可写缓冲区（简单读取到临时内核 vec，然后再写回）
    let mut kbuf = vec![0u8; len];
    let n = file.read_at(0, &mut kbuf).await?;
    state
        .process_vm
        .write_bytes(buf_ptr, &mut VmReader::from(&kbuf[..n]))?;
    Ok(ControlFlow::Continue(Some(n as isize)))
}

pub async fn do_write(
    state: &ThreadState,
    cx: &mut UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    let [fd, buf_ptr, len, ..] = cx.syscall_arguments();
    let entry = state.fd_table.get(fd as u32).await?;
    let file = match entry.obj {
        FdObject::File(ref fh) => fh.clone(),
        _ => return_errno_with_message!(Errno::EINVAL, "fd not file"),
    };
    let mut kbuf = vec![0u8; len];
    state
        .process_vm
        .root_vmar()
        .vm_space()
        .reader(buf_ptr, len)
        .map_err(ostd_error_to_errno)?
        .read_fallible(&mut VmWriter::from(kbuf.as_mut_slice()))
        .map_err(ostd_tuple_to_errno)?;
        // .read_bytes(buf_ptr, &mut VmWriter::from(kbuf.as_mut_slice()))?;
    let n = file.write_at(0, &kbuf).await?;
    Ok(ControlFlow::Continue(Some(n as isize)))
}

pub async fn do_getdents64(
    state: &ThreadState,
    cx: &mut UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    let [fd, buf_ptr, len, ..] = cx.syscall_arguments();
    let entry = state.fd_table.get(fd as u32).await?;
    let dir_handle = entry
        .obj
        .as_dir()
        .ok_or_else(|| errno_with_message(Errno::ENOTDIR, "fd not dir"))?;
    let chunk = dir_handle.read_dir_chunk(None).await?;
    // 逐条编码 linux_dirent64
    let mut offset: usize = 0;
    for d in chunk {
        let name_bytes = d.name.as_bytes();
        let reclen = core::mem::size_of::<u64>()   // d_ino
            + core::mem::size_of::<i64>()          // d_off
            + 2                                    // d_reclen
            + 1                                    // d_type
            + name_bytes.len() + 1; // d_name + '\0'
        if offset + reclen > len {
            break;
        }
        // 写入
        let mut writer = |data: &[u8]| -> Result<()> {
            state
                .process_vm
                .write_bytes(buf_ptr + offset, &mut VmReader::from(data))?;
            offset += data.len();
            Ok(())
        };
        writer(&(d.vnode_id.to_le_bytes()))?;
        writer(&(0i64.to_le_bytes()))?; // d_off 未使用
        writer(&(reclen as u16).to_le_bytes())?;
        writer(&[d.kind as u8])?;
        writer(name_bytes)?;
        writer(&[0u8])?; // NUL
    }
    Ok(ControlFlow::Continue(Some(offset as isize)))
}

pub async fn do_linkat(
    state: &ThreadState,
    cx: &mut UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    let [olddirfd, old_ptr, newdirfd, new_ptr, _flags, ..] = cx.syscall_arguments();
    let fd_table = &state.fd_table;
    let old_entry = fd_table.get(olddirfd as u32).await?;
    let old_vnode = old_entry
        .obj
        .as_dir()
        .ok_or_else(|| errno_with_message(Errno::ENOTDIR, "olddirfd not a directory"))?
        .vnode();
    let old_name = copy_cstr_from_user(&state.process_vm, old_ptr)?;

    let new_entry = fd_table.get(newdirfd as u32).await?;
    let new_vnode = new_entry
        .obj
        .as_dir()
        .ok_or_else(|| errno_with_message(Errno::ENOTDIR, "newdirfd not a directory"))?
        .vnode();
    let new_name = copy_cstr_from_user(&state.process_vm, new_ptr)?;

    old_vnode.link(&old_name, &new_vnode, &new_name).await?;
    Ok(ControlFlow::Continue(Some(0)))
}

pub async fn do_unlinkat(
    state: &ThreadState,
    cx: &mut UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    let [dirfd, path_ptr, _flags, ..] = cx.syscall_arguments();
    let fd_table = &state.fd_table;
    let entry = fd_table.get(dirfd as u32).await?;
    let dir = entry
        .obj
        .as_dir()
        .ok_or_else(|| errno_with_message(Errno::ENOTDIR, "dirfd not a directory"))?
        .vnode();
    let name = copy_cstr_from_user(&state.process_vm, path_ptr)?;
    dir.unlink(&name).await?;
    Ok(ControlFlow::Continue(Some(0)))
}

pub async fn do_mkdirat(
    state: &mut ThreadState,
    cx: &mut UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    let [dirfd, path_ptr, mode, ..] = cx.syscall_arguments();
    let dir = if dirfd == AT_FDCWD as _ {
        get_path_resolver().resolve(&mut state.cwd).await?.to_dir().ok_or_else(|| {
            errno_with_message(Errno::ENOTDIR, "AT_FDCWD not a directory")
        })?
    } else {
        let entry = state.fd_table.get(dirfd as u32).await?;
        entry
            .obj
            .as_dir()
            .ok_or_else(|| errno_with_message(Errno::ENOTDIR, "dirfd not a directory"))?
            .vnode()
    };
    
    let name = copy_cstr_from_user(&state.process_vm, path_ptr)?;
    let ret = dir
        .create(
            &name,
            VnodeType::Directory,
            FileMode::from_bits_truncate(mode as _),
            None,
        )
        .await
        .map(|_| 0)
        .unwrap_or(-1) as isize;
    Ok(ControlFlow::Continue(Some(ret)))
}

#[allow(unused_variables)]
pub async fn do_mount(
    state: &ThreadState,
    cx: &mut UserContext,
) -> Result<ControlFlow<i32, Option<isize>>> {
    let [special_ptr, dir_ptr, fstype_ptr, flags, data_ptr, ..] = cx.syscall_arguments();
    // 读取三个字符串；可为 NULL
    let read_opt = |ptr| {
        if ptr == 0 {
            Err(errno_with_message(Errno::EINVAL, "null pointer"))
        } else {
            copy_cstr_from_user(&state.process_vm, ptr)
        }
    };
    let special = read_opt(special_ptr)?;
    let dir = read_opt(dir_ptr)?;
    let fstype = read_opt(fstype_ptr)?;
    let data = read_opt(data_ptr).unwrap_or_default();

    let dir = if dir.starts_with("./") {
        state.cwd.as_slice().join(&dir)?.to_string()
    } else {
        dir
    };

    let vfs_manager = vfs::VFS_MANAGER.get();

    if fstype == "vfat" {
        // FAKE IMPLEMENTATION
        let vfs_manager = vfs::VFS_MANAGER.get();
        let ret = vfs_manager
            .mount(None, &dir, "ext4", Default::default())
            .await?;
        Ok(ControlFlow::Continue(Some(0 as isize)))
    } else {
        Err(errno_with_message(
            Errno::EINVAL,
            "unsupported filesystem type",
        ))
    }
}

pub async fn do_umount2(state: &ThreadState, cx: &mut UserContext) -> Result<ControlFlow<i32, Option<isize>>> {
    let [target_ptr, _flags, ..] = cx.syscall_arguments();
    if target_ptr == 0 {
        return Err(errno_with_message(Errno::EINVAL, "null pointer"));
    }
    let target = copy_cstr_from_user(&state.process_vm, target_ptr)?;
    let path = if target.starts_with("./") {
        state.cwd.as_slice().join(&target)?
    } else {
        PathBuf::new(target)?
    };
    let vfs_manager = vfs::VFS_MANAGER.get();
    let (_, mount_info, _) = vfs_manager.locate_mount(path.as_slice()).await?;
    vfs_manager.unmount(mount_info.id).await?;
    Ok(ControlFlow::Continue(Some(0)))
}

pub async fn do_fstat(state: &ThreadState, cx: &mut UserContext) -> Result<ControlFlow<i32, Option<isize>>> {
    let [fd, kstat_ptr, ..] = cx.syscall_arguments();
    let entry = state.fd_table.get(fd as u32).await?;
    let vnode = entry.obj.vnode();
    let meta = vnode.metadata().await?;
    // 填充 linux `struct kstat`
    #[derive(Pod, Copy, Clone)]
    #[repr(C)]
    struct KStat {
        st_dev: u64,
        st_ino: u64,
        st_mode: u32,
        __pad1: u32,
        st_nlink: u64,
        st_uid: u32,
        st_gid: u32,
        st_rdev: u64,
        __pad2: u64,
        st_size: i64,
        st_blksize: i32,
        __pad3: i32,
        st_blocks: i64,
        times: [i64; 6], // atime/mtime/ctime sec+nsec
        __unused: [u32; 2],
    }
    let ks = KStat {
        st_dev: meta.fs_id as _,
        st_ino: meta.vnode_id,
        st_mode: meta.permissions.bits() as _,
        __pad1: 0,
        st_nlink: meta.nlinks,
        st_uid: meta.uid,
        st_gid: meta.gid,
        st_rdev: meta.rdev.unwrap_or(0),
        __pad2: 0,
        st_size: meta.size as _,
        st_blksize: 4096,
        __pad3: 0,
        st_blocks: ((meta.size + 511) / 512) as _,
        times: [
            meta.timestamps.accessed.as_u64() as _,
            0,
            meta.timestamps.modified.as_u64() as _,
            0,
            meta.timestamps.changed.as_u64() as _,
            0,
        ],
        __unused: [0; 2],
    };
    // 写回用户态
    state
        .process_vm
        .write_bytes(kstat_ptr, &mut VmReader::from(ks.as_bytes()))?;
    Ok(ControlFlow::Continue(Some(0)))
}
