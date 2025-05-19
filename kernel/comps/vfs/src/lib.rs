#![cfg_attr(not(test), no_std)]
#![feature(str_as_str)] // 允许使用不稳定的 str.as_str() 方法，尝试解决 path.rs 中的 lint

// 引入 alloc crate，用于动态分配，例如 Vec, String, Box
extern crate alloc;

// NexusOS 虚拟文件系统 (VFS) 核心组件。
//
// 本 crate 定义了 VFS 的核心抽象，包括路径操作、文件系统、Vnode（虚拟节点）
// 以及文件句柄的 traits，还有相关的类型和错误定义。
// 目标是提供一个统一的接口，供不同的文件系统实现插入，并为内核其他部分
// 提供标准的文件操作API。

/// 路径操作模块 (`VfsPath`, `VfsPathBuf`)。
///
/// 提供了处理和规范化文件系统路径的类型和函数。
pub mod path;

/// VFS 核心 Traits 模块 (`Filesystem`, `Vnode`, `FileHandle`)。
///
/// 定义了文件系统实现需要满足的接口，以及 VFS 对象的行为。
pub mod traits;

/// VFS 通用类型定义模块。
///
/// 包含如 `VnodeType`, `OpenFlags`, `Metadata` 等 VFS 中广泛使用的类型。
pub mod types;

/// VFS 错误类型定义模块 (`VfsError`, `VfsResult`)。
///
/// 定义了 VFS 操作可能产生的标准错误枚举和结果类型。
pub mod verror;

// [[TODO]]: 考虑是否需要 pub use 一些常用类型到 crate 根级别，以方便使用。
// 例如:
// pub use path::{VfsPath, VfsPathBuf};
// pub use traits::{FileHandle, Filesystem, Vnode};
// pub use types::{OpenFlags, VnodeType, VnodeMetadata};
// pub use verror::{VfsError, VfsResult};
