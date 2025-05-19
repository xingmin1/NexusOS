//! VFS 内部错误处理模块
//!
//! 本模块基于 `error-stack` crate 实现，为 VFS 操作提供结构化的错误处理能力。
//! 它提供了常用的错误创建函数，如 `vfs_err_unsupported` 和 `vfs_err_invalid_argument`，
//! 这些函数会创建包含适当错误码和描述性消息的 `error_stack::Report`。
//!
//! 所有错误都使用 `nexus_error::Error` 作为错误类型，并通过 `VfsResult` 类型别名
//! 提供了一致的返回类型。错误信息会通过 `attach_printable` 附加到报告中，
//! 以便在错误链中提供更多上下文信息。

// 使用 extern crate alloc; 如果是在 no_std 环境且 alloc 不是全局可用的
// extern crate alloc;

use nexus_error::error_stack::{Result as ErrorStackResult};
pub use nexus_error::{Errno};
use alloc::format;
use alloc::string::ToString;
pub use nexus_error::error_stack::*;

pub type VfsResult<T> = ErrorStackResult<T, KernelError>;
pub type KernelError = nexus_error::Error;

// 使用内联函数，是为了错误的代码的文件和行号指到调用的位置，而不是本函数的位置的定义位置.
#[inline(always)]
pub fn vfs_err_unsupported(operation: &'static str) -> Report<nexus_error::Error> {
    Report::new(
        nexus_error::Error::with_message(Errno::ENOSYS, operation)
    )
}

#[inline(always)]
pub fn vfs_err_invalid_argument(operation: impl AsRef<str>, reason: impl AsRef<str>) -> Report<nexus_error::Error> {
    Report::new(
        nexus_error::Error::new(Errno::EINVAL)
    ).attach_printable(format!("{}: {}", operation.as_ref(), reason.as_ref()))
}

#[inline(always)]
pub fn vfs_err_not_found(resource_description: impl AsRef<str>) -> Report<KernelError> {
    Report::new(
        KernelError::with_message(Errno::ENOENT, "Resource not found")
    ).attach_printable(resource_description.as_ref().to_string())
}

#[inline(always)]
pub fn vfs_err_io_error(context: impl AsRef<str>) -> Report<KernelError> {
    Report::new(
        KernelError::with_message(Errno::EIO, "I/O Error")
    ).attach_printable(context.as_ref().to_string())
}

#[inline(always)]
pub fn vfs_err_not_dir<S: AsRef<str>>(path_description: S) -> Report<KernelError> {
    Report::new(
        KernelError::with_message(Errno::ENOTDIR, "Not a directory")
    ).attach_printable(path_description.as_ref().to_string())
}

/// 创建一个表示未实现功能的错误
#[inline(always)]
pub fn vfs_err_not_implemented<S: AsRef<str>>(operation_description: S) -> Report<KernelError> {
    Report::new(
        KernelError::with_message(Errno::ENOSYS, "Operation not implemented")
    ).attach_printable(operation_description.as_ref().to_string())
}

/// 为未实现的功能创建错误的宏
#[macro_export]
macro_rules! vfs_err_not_implemented {
    ($($arg:tt)*) => {
        $crate::verror::vfs_err_not_implemented(format!($($arg)*))
    };
}

#[inline(always)]
pub fn vfs_err_is_dir(path_description: impl AsRef<str>) -> Report<KernelError> {
    Report::new(
        KernelError::with_message(Errno::EISDIR, "Is a directory")
    ).attach_printable(path_description.as_ref().to_string())
}

#[inline(always)]
pub fn vfs_err_already_exists(path_description: impl AsRef<str>) -> Report<KernelError> {
    Report::new(
        KernelError::with_message(Errno::EEXIST, "Already exists")
    ).attach_printable(path_description.as_ref().to_string())
}

#[inline(always)]
pub fn vfs_err_not_empty(path_description: impl AsRef<str>) -> Report<KernelError> {
    Report::new(
        KernelError::with_message(Errno::ENOTEMPTY, "Directory not empty")
    ).attach_printable(path_description.as_ref().to_string())
}

#[inline(always)]
pub fn vfs_err_name_too_long(filename: impl AsRef<str>) -> Report<KernelError> {
    Report::new(
        KernelError::with_message(Errno::ENAMETOOLONG, "Name too long")
    ).attach_printable(filename.as_ref().to_string())
}

#[inline(always)]
pub fn vfs_err_no_space(device_description: impl AsRef<str>) -> Report<KernelError> {
    Report::new(
        KernelError::with_message(Errno::ENOSPC, "No space left on device")
    ).attach_printable(device_description.as_ref().to_string())
}

#[inline(always)]
pub fn vfs_err_permission_denied(operation_description: impl AsRef<str>) -> Report<KernelError> {
    Report::new(
        KernelError::with_message(Errno::EPERM, "Permission denied") // Or EACCES
    ).attach_printable(operation_description.as_ref().to_string())
}

#[inline(always)]
pub fn vfs_err_invalid_path(path_description: impl AsRef<str>) -> Report<KernelError> {
    Report::new(
        KernelError::with_message(Errno::EINVAL, "Invalid path")
    ).attach_printable(path_description.as_ref().to_string())
}