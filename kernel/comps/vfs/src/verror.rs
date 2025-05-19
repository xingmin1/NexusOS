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

use nexus_error::error_stack::{Report, Result as ErrorStackResult};
use nexus_error::{Errno};
use alloc::format;

pub type VfsResult<T> = ErrorStackResult<T, nexus_error::Error>;

pub fn vfs_err_unsupported(operation: &'static str) -> Report<nexus_error::Error> {
    Report::new(
        nexus_error::Error::with_message(Errno::ENOSYS, operation)
    )
}

pub fn vfs_err_invalid_argument(operation: impl AsRef<str>, reason: impl AsRef<str>) -> Report<nexus_error::Error> {
    Report::new(
        nexus_error::Error::new(Errno::EINVAL)
    ).attach_printable(format!("{}: {}", operation.as_ref(), reason.as_ref()))
}