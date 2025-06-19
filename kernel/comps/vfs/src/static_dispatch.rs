use alloc::sync::Arc;

use crate::{
    impls::ext4_fs::{filesystem::Ext4Fs, provider::Ext4Provider, vnode::{Ext4DirHandle, Ext4FileHandle, Ext4Vnode}}, path::PathBuf, traits::{AsyncBlockDevice, DirCap, DirHandle, FileCap, FileHandle, SymlinkCap}, types::{FilesystemId, FsOptions, MountId, SeekFrom, VnodeId, VnodeMetadataChanges}, FileOpen, FileSystem, FileSystemProvider, VfsResult, Vnode, VnodeMetadata
};

/// 静态派发后的统一 Vnode 类型。
///
/// 目前仅支持 Ext4，后续可按需扩展。
#[derive(Clone)]
pub enum SVnode {
    File(SFile),
    Dir(SDir),
    Symlink(SSymlink),
}

#[derive(Clone)]
pub enum SFile {
    Ext4(Arc<Ext4Vnode>),
}

#[derive(Clone)]
pub enum SDir {
    Ext4(Arc<Ext4Vnode>),
}

#[derive(Clone)]
pub enum SSymlink {
    Ext4(Arc<Ext4Vnode>),
}

#[derive(Clone)]
pub enum SFileHandle {
    Ext4(Arc<Ext4FileHandle>),
}

#[derive(Clone)]
pub enum SDirHandle {
    Ext4(Arc<Ext4DirHandle>),
}

/// Vnode —— 最小公共能力
impl SFile {
    fn id(&self) -> VnodeId {
        match self {
            SFile::Ext4(v) => v.id(),
        }
    }

    async fn metadata(&self) -> VfsResult<VnodeMetadata> {
        match self {
            SFile::Ext4(v) => v.metadata().await,
        }
    }

    async fn set_metadata(&self, ch: VnodeMetadataChanges) -> VfsResult<()> {
        match self {
            SFile::Ext4(v) => v.set_metadata(ch).await,
        }
    }
}

/// Vnode —— 最小公共能力 for Dir
impl SDir {
    fn id(&self) -> VnodeId {
        match self {
            SDir::Ext4(v) => v.id(),
        }
    }

    async fn metadata(&self) -> VfsResult<VnodeMetadata> {
        match self {
            SDir::Ext4(v) => v.metadata().await,
        }
    }

    async fn set_metadata(&self, ch: VnodeMetadataChanges) -> VfsResult<()> {
        match self {
            SDir::Ext4(v) => v.set_metadata(ch).await,
        }
    }
}

/// Symlink 基础能力
impl SSymlink {
    fn id(&self) -> VnodeId {
        match self {
            SSymlink::Ext4(v) => v.id(),
        }
    }

    async fn metadata(&self) -> VfsResult<VnodeMetadata> {
        match self {
            SSymlink::Ext4(v) => v.metadata().await,
        }
    }

    async fn set_metadata(&self, ch: VnodeMetadataChanges) -> VfsResult<()> {
        match self {
            SSymlink::Ext4(v) => v.set_metadata(ch).await,
        }
    }
}

// FileCap
impl SFile {
    async fn open(self, flags: FileOpen) -> VfsResult<SFileHandle> {
        match self {
            SFile::Ext4(v) => {
                let handle = v.open(flags).await?;
                Ok(SFileHandle::Ext4(handle))
            }
        }
    }
}

// DirCap
impl SDir {
    async fn open_dir(self) -> VfsResult<SDirHandle> {
        match self {
            SDir::Ext4(v) => Ok(SDirHandle::Ext4(v.open_dir().await?)),
        }
    }

    async fn lookup(&self, name: &crate::types::OsStr) -> VfsResult<SVnode> {
        match self {
            SDir::Ext4(v) => {
                let child = v.lookup(name).await?;
                Ok(SVnode::from(child))
            }
        }
    }

    async fn create(
        &self,
        name: &crate::types::OsStr,
        kind: crate::types::VnodeType,
        perm: crate::types::FileMode,
        rdev: Option<u64>,
    ) -> VfsResult<SVnode> {
        match self {
            SDir::Ext4(v) => {
                let node = v.create(name, kind, perm, rdev).await?;
                Ok(SVnode::from(node))
            }
        }
    }

    async fn rename(
        &self,
        old_name: &crate::types::OsStr,
        new_parent: &Self,
        new_name: &crate::types::OsStr,
    ) -> VfsResult<()> {
        match (self, new_parent) {
            (SDir::Ext4(old), SDir::Ext4(new_p)) => old.rename(old_name, new_p, new_name).await,
        }
    }
}

// SymlinkCap
impl SSymlink {
    async fn readlink(&self) -> VfsResult<PathBuf> {
        match self {
            SSymlink::Ext4(v) => v.readlink().await,
        }
    }
}

// FileHandle
impl SFileHandle {
    fn flags(&self) -> FileOpen {
        match self {
            SFileHandle::Ext4(h) => h.flags(),
        }
    }
    fn vnode(&self) -> SFile {
        match self {
            SFileHandle::Ext4(h) => SFile::Ext4(h.vnode().clone()),
        }
    }

    async fn read_at(&self, off: u64, buf: &mut [u8]) -> VfsResult<usize> {
        match self {
            SFileHandle::Ext4(h) => h.read_at(off, buf).await,
        }
    }
    async fn write_at(&self, off: u64, buf: &[u8]) -> VfsResult<usize> {
        match self {
            SFileHandle::Ext4(h) => h.write_at(off, buf).await,
        }
    }

    /// Scatter / Gather I/O —— 参见 readv/writev 设计  
    async fn read_vectored_at(&self, off: u64, bufs: &mut [&mut [u8]])
        -> VfsResult<usize> {
        match self {
            SFileHandle::Ext4(h) => h.read_vectored_at(off, bufs).await,
        }
    }
    async fn write_vectored_at(&self, off: u64, bufs: &[&[u8]])
        -> VfsResult<usize> {
        match self {
            SFileHandle::Ext4(h) => h.write_vectored_at(off, bufs).await,
        }
    }

    async fn seek(&self, pos: SeekFrom) -> VfsResult<u64> {
        match self {
            SFileHandle::Ext4(h) => h.seek(pos).await,
        }
    }
    async fn flush(&self) -> VfsResult<()> {
        match self {
            SFileHandle::Ext4(h) => h.flush().await,
        }
    }
    async fn close(&self) -> VfsResult<()> {
        match self {
            SFileHandle::Ext4(h) => h.close().await,
        }
    }
}

// DirHandle
impl SDirHandle {
    fn vnode(&self) -> SDir {
        match self {
            SDirHandle::Ext4(h) => SDir::Ext4(h.vnode().clone()),
        }
    }

    async fn read_dir_chunk(&self, buf: &mut [crate::types::DirectoryEntry]) -> VfsResult<usize> {
        match self {
            SDirHandle::Ext4(h) => h.read_dir_chunk(buf).await,
        }
    }

    async fn seek_dir(&self, offset: u64) -> VfsResult<()> {
        match self {
            SDirHandle::Ext4(h) => h.seek_dir(offset).await,
        }
    }

    async fn close(&self) -> VfsResult<()> {
        match self {
            SDirHandle::Ext4(h) => h.close().await,
        }
    }
}

/* ---------- 顶层 SVnode 接口 ---------- */

impl SVnode {
    /// 获取唯一 ID
    pub fn id(&self) -> VnodeId {
        match self {
            SVnode::File(f) => f.id(),
            SVnode::Dir(d) => d.id(),
            SVnode::Symlink(s) => s.id(),
        }
    }

    /// 获取元数据
    pub async fn metadata(&self) -> VfsResult<VnodeMetadata> {
        match self {
            SVnode::File(f) => f.metadata().await,
            SVnode::Dir(d) => d.metadata().await,
            SVnode::Symlink(s) => s.metadata().await,
        }
    }

    /// 设置元数据
    pub async fn set_metadata(&self, ch: VnodeMetadataChanges) -> VfsResult<()> {
        match self {
            SVnode::File(f) => f.set_metadata(ch).await,
            SVnode::Dir(d) => d.set_metadata(ch).await,
            SVnode::Symlink(s) => s.set_metadata(ch).await,
        }
    }

    /// 目录能力：lookup
    pub async fn lookup(&self, name: &crate::types::OsStr) -> VfsResult<SVnode> {
        match self {
            SVnode::Dir(d) => d.lookup(name).await,
            _ => Err(crate::vfs_err_invalid_argument!("lookup on non-directory")),
        }
    }

    /// 读取符号链接目标
    pub async fn readlink(&self) -> VfsResult<crate::path::PathBuf> {
        match self {
            SVnode::Symlink(s) => s.readlink().await,
            _ => Err(crate::vfs_err_invalid_argument!("readlink on non-symlink")),
        }
    }
}

/* ---------- From / Into 转换 ---------- */

impl From<SFile> for SVnode { fn from(v: SFile) -> Self { SVnode::File(v) } }
impl From<SDir> for SVnode { fn from(v: SDir) -> Self { SVnode::Dir(v) } }
impl From<SSymlink> for SVnode { fn from(v: SSymlink) -> Self { SVnode::Symlink(v) } }

impl From<Arc<Ext4Vnode>> for SVnode {
    fn from(v: Arc<Ext4Vnode>) -> Self {
        match v.kind() {
            crate::types::VnodeType::Directory => SVnode::Dir(SDir::Ext4(v)),
            crate::types::VnodeType::SymbolicLink => SVnode::Symlink(SSymlink::Ext4(v)),
            _ => SVnode::File(SFile::Ext4(v)),
        }
    }
}

/* ---------- FileSystem 静态派发 ---------- */

#[derive(Clone)]
pub enum SFileSystem {
    Ext4(Arc<Ext4Fs>),
}

impl SFileSystem {
    pub fn id(&self) -> FilesystemId {
        match self {
            SFileSystem::Ext4(fs) => fs.id(),
        }
    }

    pub fn mount_id(&self) -> MountId {
        match self {
            SFileSystem::Ext4(fs) => fs.mount_id(),
        }
    }

    pub fn options(&self) -> &FsOptions {
        match self {
            SFileSystem::Ext4(fs) => fs.options(),
        }
    }

    pub fn is_readonly(&self) -> bool {
        match self {
            SFileSystem::Ext4(fs) => fs.is_readonly(),
        }
    }

    pub async fn root_vnode(&self) -> VfsResult<SVnode> {
        match self {
            SFileSystem::Ext4(fs) => {
                let v = fs.clone().root_vnode().await?;
                Ok(SVnode::from(v))
            }
        }
    }

    pub async fn statfs(&self) -> VfsResult<crate::types::FilesystemStats> {
        match self {
            SFileSystem::Ext4(fs) => fs.statfs().await,
        }
    }

    pub async fn sync(&self) -> VfsResult<()> {
        match self {
            SFileSystem::Ext4(fs) => fs.sync().await,
        }
    }

    pub async fn prepare_unmount(&self) -> VfsResult<()> {
        match self {
            SFileSystem::Ext4(fs) => fs.prepare_unmount().await,
        }
    }

    pub async fn reclaim_vnode(&self, id: VnodeId) -> VfsResult<bool> {
        match self {
            SFileSystem::Ext4(fs) => fs.reclaim_vnode(id).await,
        }
    }

    pub fn fs_type_name(&self) -> &'static str {
        match self {
            SFileSystem::Ext4(fs) => fs.fs_type_name(),
        }
    }
}

/* ---------- FileSystemProvider 静态派发 ---------- */

#[derive(Clone)]
pub enum SProvider {
    Ext4(Arc<Ext4Provider>),
}

impl SProvider {
    pub fn fs_type_name(&self) -> &'static str {
        match self {
            SProvider::Ext4(p) => p.fs_type_name(),
        }
    }

    pub async fn mount(
        &self,
        dev: Option<Arc<dyn AsyncBlockDevice + Send + Sync>>,
        opts: &FsOptions,
        mount_id: MountId,
        fs_id: FilesystemId,
    ) -> VfsResult<SFileSystem> {
        match self {
            SProvider::Ext4(p) => {
                let fs = p.mount(dev, opts, mount_id, fs_id).await?;
                Ok(SFileSystem::Ext4(fs))
            }
        }
    }
}