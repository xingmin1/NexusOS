use alloc::sync::Arc;

use crate::{
    path::PathBuf,
    types::*,
    verror::VfsResult,
};

/// 块设备
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

    async fn mount(
        &self,
        dev: Option<Arc<dyn AsyncBlockDevice>>,
        opts: &FsOptions,
        mount_id: MountId,
        fs_id: FilesystemId,
    ) -> VfsResult<Arc<Self::FS>>;
}

/// 文件系统实例
pub trait FileSystem: Send + Sync + 'static {
    type Vnode: Vnode<FS = Self>;

    fn id(&self) -> FilesystemId;
    fn mount_id(&self) -> MountId;
    fn options(&self) -> &FsOptions;

    async fn root_vnode(&self) -> VfsResult<Arc<Self::Vnode>>;
    async fn statfs(&self) -> VfsResult<FilesystemStats>;
    async fn sync(&self) -> VfsResult<()>;
    async fn prepare_unmount(&self) -> VfsResult<()>;
    async fn reclaim_vnode(&self, id: VnodeId) -> VfsResult<bool>;
    fn fs_type_name(&self) -> &'static str;
    fn is_readonly(&self) -> bool;
}

/// Vnode —— 最小公共能力
pub trait Vnode: Send + Sync + 'static {
    type FS: FileSystem<Vnode = Self>;

    fn id(&self) -> VnodeId;
    fn filesystem(&self) -> &Self::FS;

    async fn metadata(&self) -> VfsResult<VnodeMetadata>;
    async fn set_metadata(&self, ch: VnodeMetadataChanges) -> VfsResult<()>;

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

    async fn open(self: Arc<Self>, flags: FileOpen) -> VfsResult<Arc<Self::Handle>>;
}

/// 文件句柄接口
pub trait FileHandle: Send + Sync + 'static {
    type Vnode: Vnode;

    fn flags(&self) -> FileOpen;
    fn vnode(&self) -> &Self::Vnode;

    async fn read_at(&self, off: u64, buf: &mut [u8]) -> VfsResult<usize>;
    async fn write_at(&self, off: u64, buf: &[u8]) -> VfsResult<usize>;

    /// Scatter / Gather I/O —— 参见 readv/writev 设计  
    async fn read_vectored_at(&self, off: u64, bufs: &mut [&mut [u8]])
        -> VfsResult<usize>;
    async fn write_vectored_at(&self, off: u64, bufs: &[&[u8]])
        -> VfsResult<usize>;

    async fn seek(&self, pos: SeekFrom) -> VfsResult<u64>;
    async fn flush(&self) -> VfsResult<()>;
    async fn close(&self) -> VfsResult<()>;
}

/// 目录能力
pub trait DirCap: VnodeCapability {
    type DirHandle: DirHandle<Vnode = Self>;

    async fn open_dir(self: Arc<Self>) -> VfsResult<Arc<Self::DirHandle>>;
    async fn lookup(&self, name: &OsStr) -> VfsResult<Arc<Self>>;
    async fn create(
        &self,
        name: &OsStr,
        kind: VnodeType,
        perm: FileMode,
        rdev: Option<u64>,
    ) -> VfsResult<Arc<Self>>;
    async fn rename(
        &self,
        old_name: &OsStr,
        new_parent: Arc<Self>,
        new_name: &OsStr,
    ) -> VfsResult<()>;
}

/// 目录句柄
pub trait DirHandle: Send + Sync + 'static {
    type Vnode: Vnode;

    fn vnode(&self) -> &Self::Vnode;

    /// 读取最多 `buf.len()` 条目录项，返回实际写入条目数  
    async fn read_dir_chunk(&self, buf: &mut [DirectoryEntry]) -> VfsResult<usize>;
    async fn seek_dir(&self, offset: u64) -> VfsResult<()>;
    async fn close(&self) -> VfsResult<()>;
}

/// 符号链接能力
pub trait SymlinkCap: VnodeCapability {
    async fn readlink(&self) -> VfsResult<PathBuf>;
}
