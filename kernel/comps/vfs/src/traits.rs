use core::future::Future;

use alloc::{boxed::Box, sync::Arc};
use async_trait::async_trait;

use crate::{
    path::PathBuf,
    types::*,
    verror::VfsResult,
};

/// 块设备
#[async_trait]
pub trait AsyncBlockDevice: Send + Sync + 'static {
    fn device_id(&self) -> u64;
    fn block_size_bytes(&self) -> VfsResult<u32>;
    fn total_blocks(&self) -> VfsResult<u64>;
    async fn read_blocks(&self, start: u64, buf: &mut [u8]) -> VfsResult<()>;
    async fn write_blocks(&self, start: u64, buf: &[u8]) -> VfsResult<()>;
    async fn flush(&self) -> VfsResult<()>;
}

/// 文件系统提供者
pub trait FileSystemProvider: Send + Sync + 'static {
    type FS: FileSystem;

    fn fs_type_name(&self) -> &'static str;

    fn mount(
        &self,
        dev: Option<Arc<dyn AsyncBlockDevice + Send + Sync>>,
        opts: &FsOptions,
        mount_id: MountId,
        fs_id: FilesystemId,
    ) -> impl Future<Output = VfsResult<Arc<Self::FS>>> + Send;
}

/// 文件系统实例
pub trait FileSystem: Send + Sync + 'static {
    type Vnode: Vnode<FS = Self>;

    fn id(&self) -> FilesystemId;
    fn mount_id(&self) -> MountId;
    fn options(&self) -> &FsOptions;

    fn root_vnode(self: Arc<Self>) -> impl Future<Output = VfsResult<Arc<Self::Vnode>>> + Send;
    fn statfs(&self) -> impl Future<Output = VfsResult<FilesystemStats>> + Send;
    fn sync(&self) -> impl Future<Output = VfsResult<()>> + Send;
    fn prepare_unmount(&self) -> impl Future<Output = VfsResult<()>> + Send;
    fn reclaim_vnode(&self, id: VnodeId) -> impl Future<Output = VfsResult<bool>> + Send;
    fn fs_type_name(&self) -> &'static str;
    fn is_readonly(&self) -> bool;
}

/// Vnode —— 最小公共能力
pub trait Vnode: Send + Sync + 'static {
    type FS: FileSystem<Vnode = Self>;

    fn id(&self) -> VnodeId;
    fn filesystem(&self) -> &Self::FS;

    fn metadata(&self) -> impl Future<Output = VfsResult<VnodeMetadata>> + Send;
    fn set_metadata(&self, ch: VnodeMetadataChanges) -> impl Future<Output = VfsResult<()>> + Send;

    fn cap_type(&self) -> VnodeType;

    // /* ---------- 可选能力检测 ---------- */
    // /// 运行时判断该节点是否实现扩展能力（读写/目录/链接）。  
    // fn has<T: ?Sized + VnodeCapability>(&self) -> bool
    // where Arc<Self>: Into<Arc<T>> { T::is_capable(self) }
}

/// 扩展能力 Trait
/// 所有能力扩展都实现此空标记，用于运行时检测。
pub trait VnodeCapability: Send + Sync + 'static {
    type FS: FileSystem<Vnode = Self::Vnode>;
    type Vnode: Vnode<FS = Self::FS>;

    // fn is_capable(node: &dyn Vnode<FS = Self::FS>) -> bool {
    //     // 默认：无法转换
    //     false
    // }
}

/// 读写文件能力
pub trait FileCap: VnodeCapability {
    /// 打开文件后产生的句柄类型
    type Handle: FileHandle<Vnode = Self>;

    fn open(self: Arc<Self>, flags: FileOpen) -> impl Future<Output = VfsResult<Arc<Self::Handle>>> + Send;
}

/// 文件句柄接口
pub trait FileHandle: Send + Sync + 'static {
    type Vnode: Vnode;

    fn flags(&self) -> FileOpen;
    fn vnode(&self) -> &Arc<Self::Vnode>;

    fn read_at(&self, off: u64, buf: &mut [u8]) -> impl Future<Output = VfsResult<usize>> + Send;
    fn write_at(&self, off: u64, buf: &[u8]) -> impl Future<Output = VfsResult<usize>> + Send;

    /// Scatter / Gather I/O —— 参见 readv/writev 设计  
    fn read_vectored_at(&self, off: u64, bufs: &mut [&mut [u8]])
        -> impl Future<Output = VfsResult<usize>> + Send;
    fn write_vectored_at(&self, off: u64, bufs: &[&[u8]])
        -> impl Future<Output = VfsResult<usize>> + Send;

    fn seek(&self, pos: SeekFrom) -> impl Future<Output = VfsResult<u64>> + Send;
    fn flush(&self) -> impl Future<Output = VfsResult<()>> + Send;
    fn close(&self) -> impl Future<Output = VfsResult<()>> + Send;
}

/// 目录能力
pub trait DirCap: VnodeCapability {
    type DirHandle: DirHandle<Vnode = Self>;

    fn open_dir(self: Arc<Self>) -> impl Future<Output = VfsResult<Arc<Self::DirHandle>>> + Send;
    fn lookup(&self, name: &OsStr) -> impl Future<Output = VfsResult<Arc<Self>>> + Send;
    fn create(
        &self,
        name: &OsStr,
        kind: VnodeType,
        perm: FileMode,
        rdev: Option<u64>,
    ) -> impl Future<Output = VfsResult<Arc<Self>>> + Send;
    fn rename(
        &self,
        old_name: &OsStr,
        new_parent: &Self,
        new_name: &OsStr,
    ) -> impl Future<Output = VfsResult<()>> + Send;

    
    /// 删除普通文件或符号链接；若目标是目录返回 `EISDIR`
    fn unlink(&self, name: &OsStr) -> impl Future<Output = VfsResult<()>> + Send;

    /// 删除空目录；若目标非目录返回 `ENOTDIR`
    fn rmdir(&self, name: &OsStr) -> impl Future<Output = VfsResult<()>> + Send;

    /// 创建硬链接：把 `target` 追加到 `new_parent/new_name`
    fn link(
        &self,                 // old_parent
        target_name: &OsStr,   // old_name
        new_parent: &Self,
        new_name: &OsStr,
    ) -> impl Future<Output = VfsResult<()>> + Send;
}

/// 目录句柄
pub trait DirHandle: Send + Sync + 'static {
    type Vnode: Vnode;

    fn vnode(&self) -> &Arc<Self::Vnode>;

    /// 读取最多 `buf.len()` 条目录项，返回实际写入条目数  
    fn read_dir_chunk(&self, buf: &mut [DirectoryEntry]) -> impl Future<Output = VfsResult<usize>> + Send;
    fn seek_dir(&self, offset: u64) -> impl Future<Output = VfsResult<()>> + Send;
    fn close(&self) -> impl Future<Output = VfsResult<()>> + Send;
}

/// 符号链接能力
pub trait SymlinkCap: VnodeCapability {
    fn readlink(&self) -> impl Future<Output = VfsResult<PathBuf>> + Send;
}
