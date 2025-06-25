#![no_std]
#![feature(str_as_str)]
#![feature(async_drop)]
#![feature(impl_trait_in_assoc_type)]

// 引入 alloc crate，用于动态分配，例如 Vec, String, Box
extern crate alloc;

// NexusOS 虚拟文件系统 (VFS) 核心组件。
//
// 本 crate 定义了 VFS 的核心抽象，包括路径操作、文件系统、Vnode（虚拟节点）
// 以及文件句柄的 traits，还有相关的类型和错误定义。
// 目标是提供一个统一的接口，供不同的文件系统实现插入，并为内核其他部分
// 提供标准的文件操作API。

/// 路径操作模块 (`PathSlice`, `PathBuf`)。
///
/// 提供了处理和规范化文件系统路径的类型和函数。
mod path;

/// VFS 核心 Traits 模块 (`Filesystem`, `Vnode`, `FileHandle`)。
///
/// 定义了文件系统实现需要满足的接口，以及 VFS 对象的行为。
mod traits;

/// VFS 通用类型定义模块。
///
/// 包含如 `VnodeType`, `OpenFlags`, `Metadata` 等 VFS 中广泛使用的类型。
mod types;

/// VFS 错误类型定义模块 (`VfsError`, `VfsResult`)。
///
/// 定义了 VFS 操作可能产生的标准错误枚举和结果类型。
mod verror;

/// VFS 缓存模块 (`VnodeCache`, `DentryCache`)。
///
/// 提供VFS性能优化所需的缓存机制。
mod cache;

/// VFS 管理器模块 (`VfsManager`)。
///
/// 提供VFS的核心管理功能，包括文件系统挂载、路径解析等。
mod manager;

/// VFS 路径解析模块 (内部)。
///
/// 实现路径解析的核心逻辑，被VfsManager使用。
mod path_resolver;

mod static_dispatch;

/// 文件系统具体实现模块。
///
/// 包含不同文件系统的实现，如内存文件系统等。
pub mod impls;

use alloc::sync::Arc;
use ostd::sync::spin::InitOnce;
/// VFS 测试模块
// #[cfg(ktest)]
// pub mod tests;

// 从各个模块中导出常用类型，方便使用
pub use path::{PathSlice, PathBuf};
pub use traits::{FileSystem, Vnode, FileHandle, DirHandle, FileSystemProvider, AsyncBlockDevice};
pub use types::{FileOpen, VnodeType, VnodeMetadata, DirectoryEntry, FsOptions, FilesystemStats, OpenStatus, AccessMode, FileOpenBuilder};
pub use verror::{VfsResult};
pub use manager::{VfsManager, VfsManagerBuilder};
pub use cache::{VnodeCache, DentryCache};
pub use static_dispatch::{vnode::{SVnode, file::{SFile, SFileHandle}, dir::{SDir, SDirHandle}, symlink::SSymlink}, filesystem::SFileSystem, provider::SProvider,};
pub use impls::ext4_fs::{get_ext4_provider, Ext4Provider, Ext4Fs, Ext4Vnode, Ext4FileHandle, Ext4DirHandle};

use crate::path_resolver::PathResolver;
// pub use impls::memfs::{InMemoryFsProvider, get_memfs_provider};

pub static VFS_MANAGER: InitOnce<Arc<VfsManager>> = InitOnce::uninitialized();

pub async fn init_vfs() {
    let vfs_manager = VfsManager::builder()
        .provider(get_ext4_provider().into())
        .build();
    vfs_manager.mount(None, "/", "ext4", Default::default()).await.unwrap();
    VFS_MANAGER.init(vfs_manager);
}

pub fn get_path_resolver<'a>() -> PathResolver<'a> {
    let vfs_manager = VFS_MANAGER.get();
    PathResolver::new(&vfs_manager, &vfs_manager.dentry_cache, true)
}