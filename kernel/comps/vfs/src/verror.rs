//! VFS 内部错误处理模块
//!
//! 本模块基于 `error-stack` crate 实现，为 VFS 操作提供结构化的错误处理能力。
//! 它提供了常用的错误创建宏，如 `vfs_err_unsupported!` 和 `vfs_err_invalid_argument!`，
//! 这些宏会创建包含适当错误码和描述性消息的 `error_stack::Report`。
//!
//! 所有错误都使用 `nexus_error::Error` 作为错误类型，并通过 `VfsResult` 类型别名
//! 提供了一致的返回类型。错误信息会通过 `attach_printable` 附加到报告中，
//! 以便在错误链中提供更多上下文信息。

use nexus_error::error_stack::Result as ErrorStackResult;
pub use nexus_error::Errno;
pub use nexus_error::error_stack::*;

pub type VfsResult<T> = ErrorStackResult<T, KernelError>;
pub type KernelError = nexus_error::Error;

/// 创建一个表示不支持的操作的错误
#[macro_export]
macro_rules! vfs_err_unsupported {
    ($operation:expr) => {
        $crate::verror::Report::new(
            $crate::verror::KernelError::with_message(
                $crate::verror::Errno::ENOSYS,
                $operation
            )
        )
    };
}

/// 创建一个表示无效参数的错
#[macro_export]
macro_rules! vfs_err_invalid_argument {
    ($operation:expr) => {
        $crate::verror::Report::new(
            $crate::verror::KernelError::new($crate::verror::Errno::EINVAL)
        ).attach_printable($operation)
    };
    ($operation:expr, $reason:expr) => {
        $crate::verror::Report::new(
            $crate::verror::KernelError::new($crate::verror::Errno::EINVAL)
        ).attach_printable(alloc::format!("{}: {}", $operation, $reason))
    };
}

/// 创建一个表示资源未找到的错误
#[macro_export]
macro_rules! vfs_err_not_found {
    ($desc:expr) => {
        $crate::verror::Report::new(
            $crate::verror::KernelError::with_message(
                $crate::verror::Errno::ENOENT,
                "Resource not found"
            )
        ).attach_printable($desc.to_string())
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_not_found!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示I/O错误的错误
#[macro_export]
macro_rules! vfs_err_io_error {
    ($context:expr) => {
        use alloc::string::ToString;
        $crate::verror::Report::new(
            $crate::verror::KernelError::with_message(
                $crate::verror::Errno::EIO,
                "I/O Error"
            )
        ).attach_printable($context.to_string())
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_io_error!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示路径不是目录的错误
#[macro_export]
macro_rules! vfs_err_not_dir {
    ($path:expr) => {
        $crate::verror::Report::new(
            $crate::verror::KernelError::with_message(
                $crate::verror::Errno::ENOTDIR,
                "Not a directory"
            )
        ).attach_printable($path.to_string())
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_not_dir!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示未实现功能的错误
#[macro_export]
macro_rules! vfs_err_not_implemented {
    ($desc:expr) => {
        $crate::verror::Report::new(
            $crate::verror::KernelError::with_message(
                $crate::verror::Errno::ENOSYS,
                "Operation not implemented"
            )
        ).attach_printable(alloc::format!("{}", $desc))
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_not_implemented!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示路径是目录的错误
#[macro_export]
macro_rules! vfs_err_is_dir {
    ($path:expr) => {
        $crate::verror::Report::new(
            $crate::verror::KernelError::with_message(
                $crate::verror::Errno::EISDIR,
                "Is a directory"
            )
        ).attach_printable($path.to_string())
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_is_dir!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示已存在的错误
#[macro_export]
macro_rules! vfs_err_already_exists {
    ($path:expr) => {
        $crate::verror::Report::new(
            $crate::verror::KernelError::with_message(
                $crate::verror::Errno::EEXIST,
                "Already exists"
            )
        ).attach_printable($path.to_string())
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_already_exists!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示目录非空的错误
#[macro_export]
macro_rules! vfs_err_not_empty {
    ($path:expr) => {
        $crate::verror::Report::new(
            $crate::verror::KernelError::with_message(
                $crate::verror::Errno::ENOTEMPTY,
                "Directory not empty"
            )
        ).attach_printable($path.to_string())
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_not_empty!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示名称太长的错误
#[macro_export]
macro_rules! vfs_err_name_too_long {
    ($name:expr) => {
        $crate::verror::Report::new(
            $crate::verror::KernelError::with_message(
                $crate::verror::Errno::ENAMETOOLONG,
                "Name too long"
            )
        ).attach_printable($name.to_string())
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_name_too_long!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示空间不足的错误
#[macro_export]
macro_rules! vfs_err_no_space {
    ($device:expr) => {
        $crate::verror::Report::new(
            $crate::verror::KernelError::with_message(
                $crate::verror::Errno::ENOSPC,
                "No space left on device"
            )
        ).attach_printable($device.to_string())
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_no_space!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示权限拒绝的错误
#[macro_export]
macro_rules! vfs_err_permission_denied {
    ($op:expr) => {
        $crate::verror::Report::new(
            $crate::verror::KernelError::with_message(
                $crate::verror::Errno::EPERM,
                "Permission denied"
            )
        ).attach_printable($op.to_string())
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_permission_denied!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示无效路径的错误
#[macro_export]
macro_rules! vfs_err_invalid_path {
    ($path:expr) => {
        $crate::verror::Report::new(
            $crate::verror::KernelError::with_message(
                $crate::verror::Errno::EINVAL,
                "Invalid path"
            )
        ).attach_printable($path.to_string())
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_invalid_path!(alloc::format!($fmt, $($arg)*))
    };
}