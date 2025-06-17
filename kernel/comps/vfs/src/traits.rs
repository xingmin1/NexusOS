//! VFS 核心 Trait 定义模块
//!
//! 本文件定义了 VFS 的核心抽象接口，包括文件系统、Vnode、文件句柄等。

use crate::{path::{VfsPath, VfsPathBuf}, types::FileMode, vfs_err_unsupported};
use crate::types::{DirectoryEntry, FileOpen, FilesystemId, FilesystemStats, FsOptions, MountId, SeekFrom, VnodeId, VnodeMetadata, VnodeMetadataChanges, VnodeType};
use crate::verror::{VfsResult}; // 采用 error_stack 错误链方案，所有错误返回值均为 VfsResult
use alloc::boxed::Box;
use alloc::sync::Arc;
use crate::types::OsStr;

/// 文件系统提供者 Trait。
///
/// 用于创建特定类型的文件系统实例。
#[async_trait::async_trait]
pub trait AsyncFileSystemProvider: Send + Sync + 'static {
    /// 返回此提供者支持的文件系统类型名称。
    fn fs_type_name(&self) -> &'static str;

    /// 挂载一个文件系统。
    ///
    /// # 参数
    /// - `source_device`: 可选的块设备，用于存储文件系统数据
    /// - `options`: 挂载选项
    /// - `mount_id`: 挂载点ID
    /// - `fs_id`: 文件系统ID
    async fn mount(
        &self,
        source_device: Option<Arc<dyn AsyncBlockDevice + Send + Sync>>,
        options: &FsOptions,
        mount_id: MountId,
        fs_id: FilesystemId,
    ) -> VfsResult<Arc<dyn AsyncFileSystem + Send + Sync>>;
}

/// 异步文件系统 Trait。
///
/// 定义了文件系统实例必须实现的接口。
#[async_trait::async_trait]
pub trait AsyncFileSystem: Send + Sync + 'static {
    /// 返回此文件系统的ID。
    fn id(&self) -> FilesystemId;

    /// 返回此文件系统挂载的挂载点ID。
    fn mount_id(&self) -> MountId;

    /// 返回此文件系统的类型名称。
    fn fs_type_name(&self) -> &'static str;

    /// 返回此文件系统的挂载选项。
    fn options(&self) -> &FsOptions;

    /// 返回此文件系统是否以只读方式挂载。
    fn is_readonly(&self) -> bool;

    /// 返回文件系统的根Vnode。
    async fn root_vnode(self: Arc<Self>) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>>;

    /// 获取文件系统的统计信息。
    async fn statfs(&self) -> VfsResult<FilesystemStats>;

    /// 同步文件系统的所有挂起更改到存储设备。
    async fn sync(&self) -> VfsResult<()>;

    /// 准备卸载文件系统。
    ///
    /// 在此阶段，文件系统应拒绝任何新的操作，并确保所有挂起的更改都已刷新。
    async fn unmount_prepare(&self) -> VfsResult<()>;

    /// 垃圾回收指定的Vnode。
    ///
    /// 当VFS检测到某个Vnode不再被引用时，会调用此方法。
    /// 返回 `true` 表示Vnode已被回收，`false` 表示Vnode仍在使用中。
    async fn gc_vnode(&self, vnode_id: VnodeId) -> VfsResult<bool>;

    // 未来可能添加的方法：
    // async fn sync(&self) -> VfsResult<()>; // 同步整个文件系统
    // async fn get_vnode(&self, id: VnodeId) -> VfsResult<Arc<dyn Vnode>>; // 通过ID获取Vnode
}

/// 异步Vnode Trait。
///
/// 代表文件系统中的一个节点，可以是文件、目录、符号链接等。
#[async_trait::async_trait]
pub trait AsyncVnode: Send + Sync + 'static {
    /// 返回此Vnode的ID。
    fn id(&self) -> VnodeId;

    /// 返回此Vnode所属的文件系统。
    fn filesystem(&self) -> Arc<dyn AsyncFileSystem + Send + Sync>;

    /// 获取Vnode的元数据。
    async fn metadata(&self) -> VfsResult<VnodeMetadata>;

    /// 检查 Vnode 是否是符号链接
    /// 默认实现返回 false，具体文件系统应覆盖此方法
    async fn is_symlink(&self) -> VfsResult<bool> {
        Ok(false)
    }

    /// 设置Vnode的元数据。
    async fn set_metadata(&self, changes: VnodeMetadataChanges) -> VfsResult<()>;

    /// 在目录中查找指定名称的Vnode。
    async fn lookup(self: Arc<Self>, name: &OsStr) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>>;

    /// 在目录中创建一个新的节点。
    async fn create_node(
        self: Arc<Self>,
        name: &OsStr,
        kind: VnodeType,
        permissions: FileMode,
        rdev: Option<u64>,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>>;

    /// 在目录中创建一个子目录。
    async fn mkdir(
        self: Arc<Self>,
        name: &OsStr,
        permissions: FileMode,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>>;

    /// 在目录中创建一个符号链接。
    async fn symlink_node(
        self: Arc<Self>,
        name: &OsStr,
        target: &VfsPath,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>>;

    /// 删除一个文件或符号链接。
    async fn unlink(self: Arc<Self>, name: &OsStr) -> VfsResult<()>;

    /// 删除一个空目录。
    async fn rmdir(self: Arc<Self>, name: &OsStr) -> VfsResult<()>;

    /// 重命名或移动一个文件或目录。
    async fn rename(
        self: Arc<Self>,
        old_name: &OsStr,
        new_parent: Arc<dyn AsyncVnode + Send + Sync>,
        new_name: &OsStr,
    ) -> VfsResult<()>;

    /// 打开文件并返回文件句柄。
    async fn open_file_handle(
        self: Arc<Self>,
        flags: FileOpen,
    ) -> VfsResult<Arc<dyn AsyncFileHandle + Send + Sync>>;

    /// 打开目录并返回目录句柄。
    async fn open_dir_handle(
        self: Arc<Self>,
        flags: FileOpen,
    ) -> VfsResult<Arc<dyn AsyncDirHandle + Send + Sync>>;

    /// 读取符号链接的目标路径。
    async fn readlink(self: Arc<Self>) -> VfsResult<VfsPathBuf>;
}

/// 异步文件句柄 Trait。
///
/// 代表一个打开的文件，用于执行I/O操作。
#[async_trait::async_trait]
pub trait AsyncFileHandle: Send + Sync + 'static {
    /// 返回与此句柄关联的Vnode。
    fn vnode(&self) -> Arc<dyn AsyncVnode + Send + Sync>;

    /// 返回打开文件时使用的标志。
    fn flags(&self) -> FileOpen;

    /// 从指定偏移量读取数据到缓冲区。
    async fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize>;

    /// 将缓冲区中的数据写入到指定偏移量。
    async fn write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize>;

    /// 移动文件指针。
    async fn seek(self: Arc<Self>, pos: SeekFrom) -> VfsResult<u64>;

    /// 刷新所有挂起的写入操作。
    async fn flush(&self) -> VfsResult<()>;

    /// 关闭文件句柄。
    ///
    /// 这是一个幂等操作。
    async fn close(&self) -> VfsResult<()>;

    /// 执行设备特定的I/O控制操作。
    ///
    /// 默认实现返回“不支持的操作”错误，支持 error_stack 错误链和上下文。
    ///
    /// 注意：使用内存地址而不是裸指针，以确保在异步上下文中的 Send 特性。
    async fn ioctl(&self, _command: u32, _argp_addr: usize) -> VfsResult<i32> {
        // 具体实现可以在需要时将地址转换回裸指针。
        // 例如：let argp = _argp_addr as *mut u8;
        // 但在异步上下文中要特别注意指针安全性。
        Err(vfs_err_unsupported!("ioctl").attach_printable("ioctl 命令未实现"))
    }
}

// 为所有实现 AsyncFileHandle 的类型提供默认实现
#[async_trait::async_trait]
impl<T: AsyncFileHandle + ?Sized> AsyncFileHandleUtil for T {}

/// 提供 AsyncFileHandle 的实用方法
#[async_trait::async_trait]
pub trait AsyncFileHandleUtil: AsyncFileHandle {
    // 可以在这里添加通用实现
}

/// 异步目录句柄 Trait。
///
/// 代表一个打开的目录，用于迭代目录条目。
#[async_trait::async_trait]
pub trait AsyncDirHandle: Send + Sync + 'static {
    /// 返回与此句柄关联的Vnode。
    fn vnode(&self) -> Arc<dyn AsyncVnode + Send + Sync>;

    /// 返回打开目录时使用的标志。
    fn flags(&self) -> FileOpen;

    /// 读取下一个目录条目。
    async fn readdir(self: Arc<Self>) -> VfsResult<Option<DirectoryEntry>>;

    /// 定位到指定的目录偏移量。
    async fn seek_dir(self: Arc<Self>, offset: u64) -> VfsResult<()>;

    /// 关闭目录句柄。
    ///
    /// 这是一个幂等操作。
    async fn close(&self) -> VfsResult<()>;
}

/// 异步块设备 Trait。
///
/// 用于支持块设备操作。
#[async_trait::async_trait]
pub trait AsyncBlockDevice: Send + Sync + 'static {
    /// 返回设备的唯一标识符。
    fn device_id(&self) -> u64;

    /// 返回块大小（以字节为单位）。
    fn block_size_bytes(&self) -> VfsResult<u32>;

    /// 返回设备的总块数。
    fn total_blocks(&self) -> VfsResult<u64>;

    /// 从设备读取一个或多个块。
    async fn read_blocks(&self, start_block: u64, buf: &mut [u8]) -> VfsResult<()>;

    /// 向设备写入一个或多个块。
    async fn write_blocks(&self, start_block: u64, buf: &[u8]) -> VfsResult<()>;

    /// 刷新所有挂起的写入操作。
    async fn flush(&self) -> VfsResult<()>;
}
