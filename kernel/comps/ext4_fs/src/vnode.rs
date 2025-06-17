//! Ext4Vnode / Ext4FileHandle / Ext4DirHandle：VFS 对应对象
//!
//! 该实现通过锁住 `Ext4Fs.inner`（`Mutex<another_ext4::Ext4>`）
//! 调用 another_ext4 提供的同步 API，并将错误包装为 `VfsResult`。

#![allow(clippy::needless_lifetimes)]

use alloc::{sync::Arc, vec::Vec};

use another_ext4::{Ext4Error, FileAttr, FileType, InodeMode, InodeMode as E4Mode};
use async_trait::async_trait;
use ostd::sync::Mutex;
use vfs::{
    types::{FileMode, OsStr, SeekFrom, VnodeMetadataChanges}, verror::KernelError, vfs_err_invalid_argument, vfs_err_io_error, vfs_err_not_found, vfs_err_unsupported,
    AsyncDirHandle, AsyncFileHandle, AsyncFileSystem, AsyncVnode, DirectoryEntry,
    OpenFlags,
    VfsPath,
    VfsPathBuf, VfsResult, VnodeMetadata, VnodeType,
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
fn map_err(e: Ext4Error) -> vfs::verror::Report<KernelError> {
    vfs_err_io_error!("ext4 error {:?}", e.code())
}

/// 将 ext4 的 `FileAttr` 转为 VFS `VnodeMetadata`
fn attr2meta(fs: &dyn AsyncFileSystem, a: &FileAttr) -> VnodeMetadata {
    VnodeMetadata {
        vnode_id: a.ino as u64,
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

pub struct Ext4Vnode {
    inode: u32,
    fs: Arc<Ext4Fs>, // 具体的 ext4 文件系统
    kind: VnodeType,
}

impl Ext4Vnode {
    pub fn new(inode: u32, kind: VnodeType, fs: Arc<Ext4Fs>) -> Arc<Self> {
        Arc::new(Self { inode, kind, fs })
    }

    pub fn new_root(fs: Arc<Ext4Fs>) -> Arc<Self> {
        Self::new(2, VnodeType::Directory, fs) // ext4 根 inode=2
    }

    #[inline]
    fn as_fs(&self) -> &Ext4Fs {
        &self.fs
    }
}

/* ---------- AsyncVnode ---------- */

#[async_trait]
impl AsyncVnode for Ext4Vnode {
    fn id(&self) -> u64 {
        self.inode as u64
    }

    fn filesystem(&self) -> Arc<dyn AsyncFileSystem + Send + Sync> {
        self.fs.clone()
    }

    async fn metadata(&self) -> VfsResult<VnodeMetadata> {
        let attr = self
            .as_fs()
            .inner
            .lock()
            .await
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
            .await
            .setattr(self.inode, mode, None, None, size, None, None, None, None)
            .map_err(map_err)?;
        Ok(())
    }

    async fn lookup(self: Arc<Self>, name: &OsStr) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        let name_str = name;
        let child = self
            .as_fs()
            .inner
            .lock()
            .await
            .lookup(self.inode, name_str)
            .map_err(|_| vfs_err_not_found!(name_str))?;
        let attr = self
            .as_fs()
            .inner
            .lock()
            .await
            .getattr(child)
            .map_err(map_err)?;
        Ok(Ext4Vnode::new(
            child,
            convert_ftype(attr.ftype),
            self.fs.clone(),
        ))
    }

    async fn create_node(
        self: Arc<Self>,
        name: &OsStr,
        kind: VnodeType,
        perm: FileMode,
        _rdev: Option<u64>,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        let mode_bits = perm.bits();
        let inode_mode = match kind {
            VnodeType::File => InodeMode::FILE,
            VnodeType::Directory => InodeMode::DIRECTORY,
            VnodeType::SymbolicLink => InodeMode::SOFTLINK,
            _ => return Err(vfs_err_unsupported!("create_node other type")),
        } | InodeMode::from_bits_truncate(mode_bits);

        let ino = self
            .as_fs()
            .inner
            .lock()
            .await
            .create(self.inode, name, inode_mode)
            .map_err(map_err)?;
        Ok(Ext4Vnode::new(ino, kind, self.fs.clone()))
    }

    async fn mkdir(
        self: Arc<Self>,
        name: &OsStr,
        perm: FileMode,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        self.clone()
            .create_node(name, VnodeType::Directory, perm, None)
            .await
    }

    async fn symlink_node(
        self: Arc<Self>,
        _name: &OsStr,
        _target: &VfsPath,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        Err(vfs_err_unsupported!("symlink_node not supported"))
        // let mode = InodeMode::SOFTLINK | InodeMode::ALL_RWX;
        // let ino = self
        //     .as_fs()
        //     .inner
        //     .lock()
        //     .await
        //     .symlink(self.inode, name, target)
        //     .map_err(map_err)?;
        // Ok(Ext4Vnode::new(
        //     ino,
        //     VnodeType::SymbolicLink,
        //     self.fs.clone(),
        // ))
    }

    async fn unlink(self: Arc<Self>, name: &OsStr) -> VfsResult<()> {
        self.as_fs()
            .inner
            .lock()
            .await
            .unlink(self.inode, name)
            .map_err(map_err)
    }

    async fn rmdir(self: Arc<Self>, name: &OsStr) -> VfsResult<()> {
        self.as_fs()
            .inner
            .lock()
            .await
            .rmdir(self.inode, name)
            .map_err(map_err)
    }

    async fn rename(
        self: Arc<Self>,
        old_name: &OsStr,
        new_parent: Arc<dyn AsyncVnode + Send + Sync>,
        new_name: &OsStr,
    ) -> VfsResult<()> {
        // 仅支持同目录重命名
        self.as_fs()
            .inner
            .lock()
            .await
            .rename(self.inode, old_name, new_parent.id() as u32, new_name)
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

    async fn open_dir_handle(
        self: Arc<Self>,
        flags: OpenFlags,
    ) -> VfsResult<Arc<dyn AsyncDirHandle + Send + Sync>> {
        if self.kind != VnodeType::Directory {
            return Err(vfs_err_invalid_argument!("open_dir_handle on non-dir"));
        }
        todo!("use flags: {:?}", flags);
        // 预取目录条目
        // let list = self
        //     .as_fs()
        //     .inner
        //     .lock()
        //     .await
        //     .listdir(self.inode)
        //     .map_err(map_err)?;
        // let mut entries = Vec::new();
        // for e in list {
        //     let name = String::from(e.name());
        //     let ino = e.inode();
        //     if let Ok(attr) = self.as_fs().inner.lock().await.getattr(ino) {
        //         entries.push(DirectoryEntry {
        //             name,
        //             vnode_id: ino as u64,
        //             kind: convert_ftype(attr.ftype),
        //         });
        //     }
        // }
        // Ok(Arc::new(Ext4DirHandle {
        //     vnode: self,
        //     flags,
        //     idx: Mutex::new(0),
        //     entries,
        // }))
    }

    async fn readlink(self: Arc<Self>) -> VfsResult<VfsPathBuf> {
        if self.kind != VnodeType::SymbolicLink {
            return Err(vfs_err_invalid_argument!("readlink on non‑symlink"));
        }
        Err(vfs_err_unsupported!("readlink not supported"))
        // let target = self
        //     .as_fs()
        //     .inner
        //     .lock()
        //     .await
        //     .readlink(self.inode)
        //     .map_err(map_err)?;
        // Ok(target)
    }
}

/* ---------- 文件句柄 ---------- */

pub struct Ext4FileHandle {
    vnode: Arc<Ext4Vnode>,
    flags: OpenFlags,
    offset: Mutex<u64>,
}

#[async_trait]
impl AsyncFileHandle for Ext4FileHandle {
    async fn write_at(&self, off: u64, buf: &[u8]) -> VfsResult<usize> {
        if !self.flags.contains(OpenFlags::WRONLY | OpenFlags::RDWR) {
            return Err(vfs_err_invalid_argument!("handle not writable"));
        }
        let sz = self
            .vnode
            .as_fs()
            .inner
            .lock()
            .await
            .write(self.vnode.inode, off as usize, buf)
            .map_err(map_err)?;
        Ok(sz)
    }

    async fn read_at(&self, off: u64, buf: &mut [u8]) -> VfsResult<usize> {
        if !self.flags.contains(OpenFlags::RDONLY | OpenFlags::RDWR) {
            return Err(vfs_err_invalid_argument!("handle not readable"));
        }
        let sz = self
            .vnode
            .as_fs()
            .inner
            .lock()
            .await
            .read(self.vnode.inode, off as usize, buf)
            .map_err(map_err)?;
        Ok(sz)
    }

    async fn seek(self: Arc<Self>, pos: SeekFrom) -> VfsResult<u64> {
        let mut off = self.offset.lock().await;
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
        self.vnode.as_fs().inner.lock().await.flush_all();
        Ok(())
    }

    async fn close(&self) -> VfsResult<()> {
        // nothing to do
        Ok(())
    }

    fn vnode(&self) -> Arc<dyn AsyncVnode + Send + Sync> {
        self.vnode.clone()
    }

    fn flags(&self) -> OpenFlags {
        self.flags
    }
}

/* ---------- 目录句柄 ---------- */

pub struct Ext4DirHandle {
    vnode: Arc<Ext4Vnode>,
    flags: OpenFlags,
    idx: Mutex<usize>,
    entries: Vec<DirectoryEntry>,
}

#[async_trait]
impl AsyncDirHandle for Ext4DirHandle {
    fn vnode(&self) -> Arc<dyn AsyncVnode + Send + Sync> {
        self.vnode.clone()
    }

    fn flags(&self) -> OpenFlags {
        self.flags
    }

    async fn readdir(self: Arc<Self>) -> VfsResult<Option<DirectoryEntry>> {
        let mut i = self.idx.lock().await;
        if *i >= self.entries.len() {
            return Ok(None);
        }
        let entry = self.entries[*i].clone();
        *i += 1;
        Ok(Some(entry))
    }

    async fn seek_dir(self: Arc<Self>, o: u64) -> VfsResult<()> {
        *self.idx.lock().await = o as usize;
        Ok(())
    }

    async fn close(&self) -> VfsResult<()> {
        Ok(())
    }
}
