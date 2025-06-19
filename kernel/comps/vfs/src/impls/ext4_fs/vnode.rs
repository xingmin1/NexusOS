//! `Ext4Vnode` 及文件/目录句柄实现。
//!
//! * **Vnode**：只提供元数据相关方法，读写目录能力由扩展 trait (`FileCap`/`DirCap`) 提供。  
//! * **FileHandle/DirHandle**：直接对应 VFS 新接口，支持向量 I/O。

#![allow(clippy::needless_lifetimes)]

use alloc::{string::String, sync::Arc, vec::Vec};

use another_ext4::{Ext4Error, FileAttr, FileType, InodeMode};
use nexus_error::{error_stack::{Report, ResultExt}, Errno, Error};
use ostd::sync::Mutex;
use crate::{traits::{DirCap, DirHandle, FileCap, FileHandle, VnodeCapability}, FileSystem};
use crate::impls::ext4_fs::filesystem::Ext4Fs;
use crate::{vfs_err_invalid_argument, vfs_err_not_found, vfs_err_unsupported, DirectoryEntry, FileOpen, VfsResult, Vnode, VnodeMetadata, VnodeType};
use crate::types::{FileMode, OsStr, SeekFrom, Timestamps, VnodeMetadataChanges};
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

#[inline]
fn convert_err(e: &Ext4Error) -> Errno {
    (e.code() as i32).try_into().unwrap()
}

/// 将 another_ext4 错误映射为 VFS 错误
fn map_err_with_msg(e: Ext4Error, msg: Option<&'static str>) -> Report<crate::verror::KernelError> {
    let err = Error::with_message(convert_err(&e), msg.unwrap_or("ext4 error"));
    Err::<(), _>(e)
        .change_context_lazy(|| err)
        .unwrap_err()
}

fn map_err(msg: Option<&'static str>) -> impl Fn(Ext4Error) -> Report<crate::verror::KernelError> {
    move |e| map_err_with_msg(e, msg)
}

/// 将 Ext4 `FileAttr` 转为 VFS `VnodeMetadata`
fn attr2meta(fs: &Ext4Fs, a: &FileAttr) -> VnodeMetadata {
    VnodeMetadata {
        vnode_id: a.ino as u64,
        fs_id: fs.id(),
        kind: convert_ftype(a.ftype),
        size: a.size,
        permissions: FileMode::from_bits_truncate(a.perm.bits()),
        timestamps: Timestamps::now(), // ext4 不含纳秒，简化处理
        uid: a.uid,
        gid: a.gid,
        nlinks: a.links as u64,
        rdev: None,
    }
}

/* ---------- Ext4Vnode ---------- */

pub struct Ext4Vnode {
    inode: u32,
    fs: Arc<Ext4Fs>,
    kind: VnodeType,
}

impl Ext4Vnode {
    pub fn new(inode: u32, kind: VnodeType, fs: Arc<Ext4Fs>) -> Arc<Self> {
        Arc::new(Self { inode, kind, fs })
    }

    pub fn new_root(fs: Arc<Ext4Fs>) -> Arc<Self> {
        Self::new(2, VnodeType::Directory, fs) // 根目录 inode = 2
    }

    #[inline]
    fn as_fs(&self) -> &Ext4Fs {
        &self.fs
    }

    /// 返回节点类型，避免在静态派发转换时额外查询元数据。
    pub fn kind(&self) -> VnodeType {
        self.kind
    }
}

/* ---------- Vnode ---------- */

// #[async_trait]
impl Vnode for Ext4Vnode {
    type FS = Ext4Fs;

    fn id(&self) -> u64 {
        self.inode as u64
    }

    fn filesystem(&self) -> &Self::FS {
        &self.fs
    }

    async fn metadata(&self) -> VfsResult<VnodeMetadata> {
        let attr = self
            .as_fs()
            .inner
            .lock()
            .await
            .getattr(self.inode)
            .map_err(map_err(Some("getattr error in Ext4Vnode::metadata")))?;
        Ok(attr2meta(&self.fs, &attr))
    }

    async fn set_metadata(&self, ch: VnodeMetadataChanges) -> VfsResult<()> {
        let mode = ch.permissions.map(|p| InodeMode::from_bits_truncate(p.bits()));
        self.as_fs()
            .inner
            .lock()
            .await
            .setattr(
                self.inode,
                mode,
                None,
                None,
                ch.size,
                None,
                None,
                None,
                None,
            )
            .map_err(map_err(Some("setattr error in Ext4Vnode::set_metadata")))?;
        Ok(())
    }

    fn cap_type(&self) -> VnodeType {
        self.kind
    }
}

/* ---------- 文件句柄 ---------- */

pub struct Ext4FileHandle {
    vnode: Arc<Ext4Vnode>,
    flags: FileOpen,
    offset: Mutex<u64>,
}

// #[async_trait]
impl FileHandle for Ext4FileHandle {
    type Vnode = Ext4Vnode;

    fn flags(&self) -> FileOpen {
        self.flags
    }

    fn vnode(&self) -> &Arc<Self::Vnode> {
        &self.vnode
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
            .await
            .read(self.vnode.inode, off as usize, buf)
            .map_err(map_err(Some("read error in Ext4FileHandle::read_at")))?;
        Ok(sz)
    }

    async fn write_at(&self, off: u64, buf: &[u8]) -> VfsResult<usize> {
        if !self.flags.is_writable() {
            return Err(vfs_err_invalid_argument!("handle not writable"));
        }
        let sz = self
            .vnode
            .as_fs()
            .inner
            .lock()
            .await
            .write(self.vnode.inode, off as usize, buf)
            .map_err(map_err(Some("write error in Ext4FileHandle::write_at")))?;
        Ok(sz)
    }

    async fn read_vectored_at(
        &self,
        off: u64,
        bufs: &mut [&mut [u8]],
    ) -> VfsResult<usize> {
        let mut offset = off;
        let mut total = 0;
        for buf in bufs {
            let n = self.read_at(offset, buf).await?;
            if n == 0 {
                break;
            }
            offset += n as u64;
            total += n;
            if n < buf.len() {
                break; // EOF
            }
        }
        Ok(total)
    }

    async fn write_vectored_at(&self, off: u64, bufs: &[&[u8]]) -> VfsResult<usize> {
        let mut offset = off;
        let mut total = 0;
        for buf in bufs {
            let n = self.write_at(offset, buf).await?;
            offset += n as u64;
            total += n;
        }
        Ok(total)
    }

    async fn seek(&self, pos: SeekFrom) -> VfsResult<u64> {
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
        Ok(())
    }
}

/* ---------- 目录句柄 ---------- */

pub struct Ext4DirHandle {
    vnode: Arc<Ext4Vnode>,
    idx: Mutex<usize>,
    entries: Vec<DirectoryEntry>,
}

impl DirHandle for Ext4DirHandle {
    type Vnode = Ext4Vnode;

    fn vnode(&self) -> &Arc<Self::Vnode> {
        &self.vnode
    }

    async fn read_dir_chunk(
        &self,
        buf: &mut [DirectoryEntry],
    ) -> VfsResult<usize> {
        let mut i = self.idx.lock().await;
        let remain = self.entries.len().saturating_sub(*i);
        let n = buf.len().min(remain);
        buf[..n].clone_from_slice(&self.entries[*i..*i + n]);
        *i += n;
        Ok(n)
    }

    async fn seek_dir(&self, offset: u64) -> VfsResult<()> {
        *self.idx.lock().await = offset as usize;
        Ok(())
    }

    async fn close(&self) -> VfsResult<()> {
        Ok(())
    }
}

/* ---------- 扩展能力实现 ---------- */
impl VnodeCapability for Ext4Vnode {
    type FS = Ext4Fs;
    type Vnode = Self;
}

// #[async_trait]
impl FileCap for Ext4Vnode {
    type Handle = Ext4FileHandle;

    async fn open(self: Arc<Self>, flags: FileOpen) -> VfsResult<Arc<Self::Handle>> {
        if self.kind != VnodeType::File {
            return Err(vfs_err_invalid_argument!("open on non‑file"));
        }
        Ok(Arc::new(Ext4FileHandle {
            vnode: self,
            flags,
            offset: Mutex::new(0),
        }))
    }
}

impl DirCap for Ext4Vnode {
    type DirHandle = Ext4DirHandle;

    async fn open_dir(self: Arc<Self>) -> VfsResult<Arc<Self::DirHandle>> {
        if self.kind != VnodeType::Directory {
            return Err(vfs_err_invalid_argument!("open_dir on non‑directory"));
        }

        // 预读取目录项
        let list = self
            .as_fs()
            .inner
            .lock()
            .await
            .listdir(self.inode)
            .change_context_lazy(||                 
                nexus_error::Error::with_message(
                    nexus_error::Errno::EIO,
                    "I/O Error"
                )
            ).attach_printable("listdir")?;
        let mut entries = Vec::with_capacity(list.len());
        for e in list {
            let name = String::from(e.name());
            let ino = e.inode();
            if let Ok(attr) = self.as_fs().inner.lock().await.getattr(ino) {
                entries.push(DirectoryEntry {
                    name,
                    vnode_id: ino as u64,
                    kind: convert_ftype(attr.ftype),
                });
            }
        }
        Ok(Arc::new(Ext4DirHandle {
            vnode: self,
            idx: Mutex::new(0),
            entries,
        }))
    }

    async fn lookup(&self, name: &OsStr) -> VfsResult<Arc<Self>> {
        let ino = self
            .as_fs()
            .inner
            .lock()
            .await
            .lookup(self.inode, name)
            .map_err(|_| vfs_err_not_found!(name))?;
        let attr = self.as_fs().inner.lock().await.getattr(ino).map_err(map_err(Some("getattr error in Ext4Vnode::lookup")))?;
        Ok(Ext4Vnode::new(ino, convert_ftype(attr.ftype), self.fs.clone()))
    }

    async fn create(
        &self,
        name: &OsStr,
        kind: VnodeType,
        perm: FileMode,
        _rdev: Option<u64>,
    ) -> VfsResult<Arc<Self>> {
        let mode_bits = perm.bits();
        let inode_mode = match kind {
            VnodeType::File => InodeMode::FILE,
            VnodeType::Directory => InodeMode::DIRECTORY,
            VnodeType::SymbolicLink => InodeMode::SOFTLINK,
            _ => return Err(vfs_err_unsupported!("create: unsupported type")),
        } | InodeMode::from_bits_truncate(mode_bits);

        let ino = self
            .as_fs()
            .inner
            .lock()
            .await
            .create(self.inode, name, inode_mode)
            .map_err(map_err(Some("create error in Ext4Vnode::create")))?;
        Ok(Ext4Vnode::new(ino, kind, self.fs.clone()))
    }

    async fn rename(
        &self,
        old_name: &OsStr,
        new_parent: &Self,
        new_name: &OsStr,
    ) -> VfsResult<()> {
        self.as_fs()
            .inner
            .lock()
            .await
            .rename(self.inode, old_name, new_parent.id() as u32, new_name)
            .map_err(map_err(Some("rename error in Ext4Vnode::rename")))
    }

    async fn unlink(&self, name: &OsStr) -> VfsResult<()> {
        self.as_fs()
            .inner
            .lock()
            .await
            .unlink(self.inode, name)
            .map_err(map_err(Some("unlink error in Ext4Vnode::unlink")))
    }

    async fn rmdir(&self, name: &OsStr) -> VfsResult<()> {
        self.as_fs()
            .inner
            .lock()
            .await
            .rmdir(self.inode, name)
            .map_err(map_err(Some("rmdir error in Ext4Vnode::rmdir")))
    }

    async fn link(
        &self,
        target_name: &OsStr,
        new_parent: &Self,
        new_name: &OsStr,
    ) -> VfsResult<()> {
        let fs = self.as_fs().inner.lock().await;
        let target = fs.lookup(self.inode, target_name).map_err(map_err(Some("lookup error in Ext4Vnode::link")))?;

        if let Ok(_) = fs.lookup(new_parent.id() as u32, new_name) {
            return Err(Error::with_message(Errno::EEXIST, "link: target already exists").into());
        }

        fs.link(target, new_parent.id() as u32, new_name)
            .map_err(map_err(Some("link error in Ext4Vnode::link")))
    }
}

/* ---------- 符号链接能力 ---------- */
use crate::traits::SymlinkCap;

impl SymlinkCap for Ext4Vnode {
    async fn readlink(&self) -> VfsResult<crate::path::PathBuf> {
        // another_ext4 当前未实现读取符号链接，故返回未支持错误。
        Err(vfs_err_unsupported!("ext4: readlink not supported"))
    }
}
