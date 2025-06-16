//! Ext4Vnode / Ext4FileHandle / Ext4DirHandle：VFS 对应对象
//!
//! 该实现通过锁住 `Ext4Fs.inner`（`Mutex<another_ext4::Ext4>`）
//! 调用 another_ext4 提供的同步 API，并将错误包装为 `VfsResult`。

#![allow(clippy::needless_lifetimes)]

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use async_trait::async_trait;
use ostd::sync::Mutex;
use tracing::{debug, error, trace};

use vfs::{
    vfs_err_io_error, vfs_err_invalid_argument, vfs_err_not_found, vfs_err_unsupported,
    AsyncDirHandle, AsyncFileHandle, AsyncFileSystem, AsyncVnode, DirectoryEntry, FileMode,
    OpenFlags, SeekFrom, VfsResult, VnodeId, VnodeMetadata, VnodeMetadataChanges, VnodeType,
};

use another_ext4::{
    error::Ext4Error, ext4_defs::inode::InodeMode, FileAttr, FileType, InodeMode as E4Mode,
};

use crate::filesystem::Ext4Fs;

/* ---------- 内部工具 ---------- */

#[inline]
fn convert_ftype(ft: FileType) -> VnodeType {
    match ft {
        FileType::RegularFile => VnodeType::File,
        FileType::Directory => VnodeType::Directory,
        FileType::SymLink => VnodeType::SymbolicLink,
        FileType::BlockDev => VnodeType::BlockDevice,
        FileType::CharacterDev => VnodeType::CharDevice,
        FileType::Fifo => VnodeType::Fifo,
        FileType::Socket => VnodeType::Socket,
        FileType::Unknown => VnodeType::File,
    }
}

/// 将 `another_ext4::Ext4Error` 转为 VFS 错误报告。
fn map_err(e: Ext4Error) -> vfs::verror::Report {
    vfs_err_io_error!(format!("ext4 error {:?}", e.code()))
}

/// 将 ext4 的 `FileAttr` 转为 VFS `VnodeMetadata`
fn attr2meta(fs: &dyn AsyncFileSystem, a: &FileAttr) -> VnodeMetadata {
    VnodeMetadata {
        vnode_id: a.ino as VnodeId,
        fs_id: fs.id(),
        kind: convert_ftype(a.ftype),
        size: a.size,
        permissions: FileMode::from_bits_truncate(a.perm.bits()),
        timestamps: vfs::types::Timestamps::now(), // ext4 无纳秒，此处简化
        uid: a.uid,
        gid: a.gid,
        nlinks: a.links as u64,
        rdev: None,
    }
}

/* ---------- Ext4Vnode ---------- */

pub struct Ext4Vnode<Fs: AsyncFileSystem> {
    inode: u32,
    fs:    Arc<Fs>,
    kind:  VnodeType,
}

impl<Fs: AsyncFileSystem> Ext4Vnode<Fs> {
    pub fn new(inode: u32, kind: VnodeType, fs: Arc<Fs>) -> Arc<Self> {
        Arc::new(Self { inode, kind, fs })
    }
    pub fn new_root(fs: Arc<Fs>) -> Arc<Self> {
        Self::new(2, VnodeType::Directory, fs) // ext4 root inode = 2
    }

    #[inline]
    fn as_fs(&self) -> &Ext4Fs<impl vfs::AsyncBlockDevice> {
        // SAFETY: 在 crate::filesystem::Ext4Fs 中创建时保证类型
        unsafe { &*(self.fs.as_ref() as *const _ as *const Ext4Fs<impl vfs::AsyncBlockDevice>) }
    }
}

/* ---------- AsyncVnode ---------- */

#[async_trait]
impl<Fs: AsyncFileSystem + 'static> AsyncVnode for Ext4Vnode<Fs> {
    fn id(&self) -> VnodeId { self.inode as VnodeId }

    fn filesystem(&self) -> Arc<dyn AsyncFileSystem + Send + Sync> { self.fs.clone() }

    async fn metadata(&self) -> VfsResult<VnodeMetadata> {
        let attr = self
            .as_fs()
            .inner
            .lock()
            .getattr(self.inode)
            .map_err(map_err)?;
        Ok(attr2meta(self.fs.as_ref(), &attr))
    }

    async fn set_metadata(&self, changes: VnodeMetadataChanges) -> VfsResult<()> {
        let mut mode: Option<E4Mode> = None;
        if let Some(perm) = changes.permissions {
            mode = Some(E4Mode::from_bits_truncate(perm.bits()));
        }
        let size = changes.size;
        self.as_fs()
            .inner
            .lock()
            .setattr(self.inode, mode, None, None, size, None, None, None, None)
            .map_err(map_err)?;
        Ok(())
    }

    async fn lookup(
        self: Arc<Self>,
        name: &vfs::types::OsStr,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        let name_str = name;
        let child = self
            .as_fs()
            .inner
            .lock()
            .lookup(self.inode, name_str)
            .map_err(|_| vfs_err_not_found!(name_str))?;
        let attr = self.as_fs().inner.lock().getattr(child).map_err(map_err)?;
        Ok(Ext4Vnode::new(child, convert_ftype(attr.ftype), self.fs.clone()))
    }

    async fn create_node(
        self: Arc<Self>,
        name: &vfs::types::OsStr,
        kind: VnodeType,
        perm: FileMode,
        _rdev: Option<u64>,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        let mode_bits = perm.bits();
        let inode_mode = match kind {
            VnodeType::File => InodeMode::FILE,
            VnodeType::Directory => InodeMode::DIRECTORY,
            VnodeType::SymbolicLink => InodeMode::SOFT_LINK,
            _ => return Err(vfs_err_unsupported!("create_node other type")),
        } | InodeMode::from_bits_truncate(mode_bits);

        let ino = self
            .as_fs()
            .inner
            .lock()
            .create(self.inode, name, inode_mode)
            .map_err(map_err)?;
        Ok(Ext4Vnode::new(ino, kind, self.fs.clone()))
    }

    async fn mkdir(
        self: Arc<Self>,
        name: &vfs::types::OsStr,
        perm: FileMode,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        self.clone()
            .create_node(name, VnodeType::Directory, perm, None)
            .await
    }

    async fn symlink_node(
        self: Arc<Self>,
        name: &vfs::types::OsStr,
        target: &vfs::types::OsStr,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        let mode = InodeMode::SOFT_LINK | InodeMode::ALL_RWX;
        let ino = self
            .as_fs()
            .inner
            .lock()
            .symlink(self.inode, name, target)
            .map_err(map_err)?;
        Ok(Ext4Vnode::new(
            ino,
            VnodeType::SymbolicLink,
            self.fs.clone(),
        ))
    }

    async fn unlink(self: Arc<Self>, name: &vfs::types::OsStr) -> VfsResult<()> {
        self.as_fs()
            .inner
            .lock()
            .unlink(self.inode, name)
            .map_err(map_err)
    }

    async fn rmdir(self: Arc<Self>, name: &vfs::types::OsStr) -> VfsResult<()> {
        self.as_fs()
            .inner
            .lock()
            .rmdir(self.inode, name)
            .map_err(map_err)
    }

    async fn rename(
        self: Arc<Self>,
        old_name: &vfs::types::OsStr,
        new_name: &vfs::types::OsStr,
    ) -> VfsResult<()> {
        // 仅支持同目录重命名
        self.as_fs()
            .inner
            .lock()
            .rename(self.inode, old_name, self.inode, new_name)
            .map_err(map_err)
    }

    async fn open_file_handle(
        self: Arc<Self>,
        flags: OpenFlags,
    ) -> VfsResult<Arc<dyn AsyncFileHandle + Send + Sync>> {
        if self.kind != VnodeType::File {
            return Err(vfs_err_invalid_argument!("open_file_handle on non‑file"));
        }
        Ok(Arc::new(Ext4FileHandle {
            vnode: self.clone(),
            flags,
            offset: Mutex::new(0),
        }))
    }

    async fn open_dir_handle(self: Arc<Self>) -> VfsResult<Arc<dyn AsyncDirHandle + Send + Sync>> {
        if self.kind != VnodeType::Directory {
            return Err(vfs_err_invalid_argument!("open_dir_handle on non‑dir"));
        }
        // 预取目录条目
        let list = self
            .as_fs()
            .inner
            .lock()
            .listdir(self.inode)
            .map_err(map_err)?;
        let entries = list
            .into_iter()
            .filter_map(|e| {
                let name = String::from(e.name());
                let ino = e.inode();
                let attr = self.as_fs().inner.lock().getattr(ino).ok()?;
                Some(DirectoryEntry {
                    name,
                    vnode_id: ino as VnodeId,
                    kind: convert_ftype(attr.ftype),
                })
            })
            .collect::<Vec<_>>();
        Ok(Arc::new(Ext4DirHandle {
            vnode: self,
            idx: Mutex::new(0),
            entries,
        }))
    }

    async fn readlink(self: Arc<Self>) -> VfsResult<String> {
        if self.kind != VnodeType::SymbolicLink {
            return Err(vfs_err_invalid_argument!("readlink on non‑symlink"));
        }
        let target = self
            .as_fs()
            .inner
            .lock()
            .readlink(self.inode)
            .map_err(map_err)?;
        Ok(target)
    }
}

/* ---------- 文件句柄 ---------- */

pub struct Ext4FileHandle<Fs: AsyncFileSystem> {
    vnode: Arc<Ext4Vnode<Fs>>,
    flags: OpenFlags,
    offset: Mutex<u64>,
}

#[async_trait]
impl<Fs: AsyncFileSystem + 'static> AsyncFileHandle for Ext4FileHandle<Fs> {
    async fn write_at(&self, off: u64, buf: &[u8]) -> VfsResult<usize> {
        if !self.flags.is_writable() {
            return Err(vfs_err_invalid_argument!("handle not writable"));
        }
        let sz = self
            .vnode
            .as_fs()
            .inner
            .lock()
            .write(self.vnode.inode, off as usize, buf)
            .map_err(map_err)?;
        Ok(sz)
    }

    async fn read_at(&self, off: u64, buf: &mut [u8]) -> VfsResult<usize> {
        if !self.flags.is_readable() {
            return Err(vfs_err_invalid_argument!("handle not readable"));
        }
        let sz = self
            .vnode
            .as_fs()
            .inner
            .lock()
            .read(self.vnode.inode, off as usize, buf)
            .map_err(map_err)?;
        Ok(sz)
    }

    async fn seek(&self, pos: SeekFrom) -> VfsResult<u64> {
        let mut off = self.offset.lock();
        match pos {
            SeekFrom::Start(o) => *off = o as u64,
            SeekFrom::End(n) => {
                let meta = self.vnode.metadata().await?;
                *off = (meta.size as isize + n) as u64;
            }
            SeekFrom::Current(n) => *off = (*off as isize + n) as u64,
        }
        Ok(*off)
    }

    async fn flush(&self) -> VfsResult<()> {
        self.vnode.as_fs().inner.lock().flush_all();
        Ok(())
    }

    async fn close(&self) -> VfsResult<()> {
        // nothing to do
        Ok(())
    }

    fn vnode(&self) -> Arc<dyn AsyncVnode + Send + Sync> { self.vnode.clone() }

    fn flags(&self) -> OpenFlags { self.flags }
}

/* ---------- 目录句柄 ---------- */

pub struct Ext4DirHandle<Fs: AsyncFileSystem> {
    vnode: Arc<Ext4Vnode<Fs>>,
    idx: Mutex<usize>,
    entries: Vec<DirectoryEntry>,
}

#[async_trait]
impl<Fs: AsyncFileSystem + 'static> AsyncDirHandle for Ext4DirHandle<Fs> {
    fn vnode(&self) -> Arc<dyn AsyncVnode + Send + Sync> { self.vnode.clone() }

    fn flags(&self) -> OpenFlags { OpenFlags::RDONLY }

    async fn readdir(self: Arc<Self>) -> VfsResult<Option<DirectoryEntry>> {
        let mut i = self.idx.lock();
        if *i >= self.entries.len() {
            return Ok(None);
        }
        let entry = self.entries[*i].clone();
        *i += 1;
        Ok(Some(entry))
    }

    async fn seek_dir(self: Arc<Self>, o: u64) -> VfsResult<()> {
        *self.idx.lock() = o as usize;
        Ok(())
    }

    async fn close(&self) -> VfsResult<()> { Ok(()) }
}
