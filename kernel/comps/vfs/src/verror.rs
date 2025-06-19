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
pub(crate) use nexus_error::Errno;

pub type VfsResult<T> = ErrorStackResult<T, KernelError>;
pub type KernelError = nexus_error::Error;

/// 创建一个表示不支持的操作的错误
#[macro_export]
macro_rules! vfs_err_unsupported {
    ($operation:expr) => {
        nexus_error::error_stack::Report::new(
            nexus_error::Error::with_message(
                nexus_error::Errno::ENOSYS,
                $operation
            )
        )
    };
}

/// 创建一个表示无效参数的错
#[macro_export]
macro_rules! vfs_err_invalid_argument {
    ($operation:expr) => {
        nexus_error::error_stack::Report::new(
            nexus_error::Error::new(nexus_error::Errno::EINVAL)
        ).attach_printable($operation)
    };
    ($operation:expr, $reason:expr) => {
        nexus_error::error_stack::Report::new(
            nexus_error::Error::new(nexus_error::Errno::EINVAL)
        ).attach_printable(alloc::format!("{}: {}", $operation, $reason))
    };
}

/// 创建一个表示资源未找到的错误
#[macro_export]
macro_rules! vfs_err_not_found {
    ($desc:expr) => {
        nexus_error::error_stack::Report::new(
            nexus_error::Error::with_message(
                nexus_error::Errno::ENOENT,
                "Resource not found"
            )
        ).attach_printable(alloc::format!("{}", $desc))
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_not_found!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示I/O错误的错误
#[macro_export]
macro_rules! vfs_err_io_error {
    ($context:expr) => {
        {
            nexus_error::error_stack::Report::new(
                nexus_error::Error::with_message(
                    nexus_error::Errno::EIO,
                    "I/O Error"
                )
            ).attach_printable(alloc::format!("{}", $context))
        }
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_io_error!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示路径不是目录的错误
#[macro_export]
macro_rules! vfs_err_not_dir {
    ($path:expr) => {
        nexus_error::error_stack::Report::new(
            nexus_error::Error::with_message(
                nexus_error::Errno::ENOTDIR,
                "Not a directory"
            )
        ).attach_printable(alloc::format!("{}", $path))
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_not_dir!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示未实现功能的错误
#[macro_export]
macro_rules! vfs_err_not_implemented {
    ($desc:expr) => {
        nexus_error::error_stack::Report::new(
            nexus_error::Error::with_message(
                nexus_error::Errno::ENOSYS,
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
        nexus_error::error_stack::Report::new(
            nexus_error::Error::with_message(
                nexus_error::Errno::EISDIR,
                "Is a directory"
            )
        ).attach_printable(alloc::format!("{}", $path))
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_is_dir!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示已存在的错误
#[macro_export]
macro_rules! vfs_err_already_exists {
    ($path:expr) => {
        nexus_error::error_stack::Report::new(
            nexus_error::Error::with_message(
                nexus_error::Errno::EEXIST,
                "Already exists"
            )
        ).attach_printable(alloc::format!("{}", $path))
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_already_exists!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示目录非空的错误
#[macro_export]
macro_rules! vfs_err_not_empty {
    ($path:expr) => {
        nexus_error::error_stack::Report::new(
            nexus_error::Error::with_message(
                nexus_error::Errno::ENOTEMPTY,
                "Directory not empty"
            )
        ).attach_printable(alloc::format!("{}", $path))
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_not_empty!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示名称太长的错误
#[macro_export]
macro_rules! vfs_err_name_too_long {
    ($name:expr) => {
        nexus_error::error_stack::Report::new(
            nexus_error::Error::with_message(
                nexus_error::Errno::ENAMETOOLONG,
                "Name too long"
            )
        ).attach_printable(alloc::format!("{}", $name))
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_name_too_long!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示空间不足的错误
#[macro_export]
macro_rules! vfs_err_no_space {
    ($device:expr) => {
        nexus_error::error_stack::Report::new(
            nexus_error::Error::with_message(
                nexus_error::Errno::ENOSPC,
                "No space left on device"
            )
        ).attach_printable(alloc::format!("{}", $device))
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_no_space!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示权限拒绝的错误
#[macro_export]
macro_rules! vfs_err_permission_denied {
    ($op:expr) => {
        nexus_error::error_stack::Report::new(
            nexus_error::Error::with_message(
                nexus_error::Errno::EPERM,
                "Permission denied"
            )
        ).attach_printable(alloc::format!("{}", $op))
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_permission_denied!(alloc::format!($fmt, $($arg)*))
    };
}

/// 创建一个表示无效路径的错误
#[macro_export]
macro_rules! vfs_err_invalid_path {
    ($path:expr) => {
        nexus_error::error_stack::Report::new(
            nexus_error::Error::with_message(
                nexus_error::Errno::EINVAL,
                "Invalid path"
            )
        ).attach_printable(alloc::format!("{}", $path))
    };
    ($fmt:expr, $($arg:tt)*) => {
        vfs_err_invalid_path!(alloc::format!($fmt, $($arg)*))
    };
}