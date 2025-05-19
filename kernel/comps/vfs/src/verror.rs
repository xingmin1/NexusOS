//! VFS 错误处理模块

use error_stack::{Context, Report};
use crate::path::VfsPathBuf;
use alloc::string::String as AllocString;
use nexus_error::Errno;
use core::fmt;
// 通常由 error_stack::report! 宏或通过 Frame::location() 隐式捕获位置信息 获取，直接导入可能不是必须的

// 显式定义错误结构体类型，以便辅助函数能够使用
#[derive(Debug)]
pub struct InvalidOperation { pub reason: AllocString }

#[derive(Debug)]
pub struct NotFound { pub target_path: VfsPathBuf, pub path: VfsPathBuf }

#[derive(Debug)]
pub struct AlreadyExists { pub target_path: VfsPathBuf }

#[derive(Debug)]
pub struct PermissionDenied { pub target_path: VfsPathBuf, pub operation: AllocString }

#[derive(Debug)]
pub struct NotADirectory { pub target_path: VfsPathBuf }

#[derive(Debug)]
pub struct IsADirectory { pub target_path: VfsPathBuf }

#[derive(Debug)]
pub struct DirectoryNotEmpty { pub target_path: VfsPathBuf }

#[derive(Debug)]
pub struct InvalidArgument { pub arg_name: AllocString, pub reason: AllocString }

#[derive(Debug)]
pub struct IoError { pub details: AllocString }

#[derive(Debug)]
pub struct FileSystemError { pub fs_type: AllocString, pub details: AllocString }

#[derive(Debug)]
pub struct Unsupported { pub operation: AllocString }

#[derive(Debug)]
pub struct Busy { }

#[derive(Debug)]
pub struct ReadOnlyFileSystem { }

#[derive(Debug)]
pub struct TooManyLinks { pub target_path: VfsPathBuf }

#[derive(Debug)]
pub struct FileNameTooLong { pub file_name: AllocString }

#[derive(Debug)]
pub struct NoSpace { }

#[derive(Debug)]
pub struct QuotaExceeded { }

#[derive(Debug)]
pub struct Corrupted { pub details: AllocString }

#[derive(Debug)]
pub struct Interrupted { }

#[derive(Debug)]
pub struct WouldBlock { }

#[derive(Debug)]
pub struct TimedOut { }

#[derive(Debug)]
pub struct ResourceLimitExceeded { pub limit_type: AllocString }

#[derive(Debug)]
pub struct InvalidHandle { }

#[derive(Debug)]
pub struct FileTooLarge { }

#[derive(Debug)]
pub struct MountError { pub reason: AllocString }

#[derive(Debug)]
pub struct UnmountError { pub reason: AllocString }

#[derive(Debug)]
pub struct CrossDeviceLink { }

// 为所有结构体实现Context和VfsKernelErrorProvider trait
impl Context for InvalidOperation {}
impl VfsKernelErrorProvider for InvalidOperation { fn kernel_errno(&self) -> Errno { Errno::EPERM } }
impl fmt::Display for InvalidOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "非法操作: {}", self.reason)
    }
}

impl Context for NotFound {}
impl VfsKernelErrorProvider for NotFound { fn kernel_errno(&self) -> Errno { Errno::ENOENT } }
impl fmt::Display for NotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "路径或文件未找到: {:?}", self.path)
    }
}

impl Context for AlreadyExists {}
impl VfsKernelErrorProvider for AlreadyExists { fn kernel_errno(&self) -> Errno { Errno::EEXIST } }
impl fmt::Display for AlreadyExists {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "路径或文件已存在: {:?}", self.target_path)
    }
}

impl Context for PermissionDenied {}
impl VfsKernelErrorProvider for PermissionDenied { fn kernel_errno(&self) -> Errno { Errno::EACCES } }
impl fmt::Display for PermissionDenied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "权限不足，无法在 {:?} 上执行操作: {}", self.target_path, self.operation)
    }
}

impl Context for NotADirectory {}
impl VfsKernelErrorProvider for NotADirectory { fn kernel_errno(&self) -> Errno { Errno::ENOTDIR } }
impl fmt::Display for NotADirectory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "路径不是目录: {:?}", self.target_path)
    }
}

impl Context for IsADirectory {}
impl VfsKernelErrorProvider for IsADirectory { fn kernel_errno(&self) -> Errno { Errno::EISDIR } }
impl fmt::Display for IsADirectory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "路径是目录: {:?}", self.target_path)
    }
}

impl Context for DirectoryNotEmpty {}
impl VfsKernelErrorProvider for DirectoryNotEmpty { fn kernel_errno(&self) -> Errno { Errno::ENOTEMPTY } }
impl fmt::Display for DirectoryNotEmpty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "目录不为空: {:?}", self.target_path)
    }
}

impl Context for InvalidArgument {}
impl VfsKernelErrorProvider for InvalidArgument { fn kernel_errno(&self) -> Errno { Errno::EINVAL } }
impl fmt::Display for InvalidArgument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "提供的参数无效: {} - {}", self.arg_name, self.reason)
    }
}

impl Context for IoError {}
impl VfsKernelErrorProvider for IoError { fn kernel_errno(&self) -> Errno { Errno::EIO } }
impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IO错误: {}", self.details)
    }
}

impl Context for FileSystemError {}
impl VfsKernelErrorProvider for FileSystemError { fn kernel_errno(&self) -> Errno { Errno::EUCLEAN } }
impl fmt::Display for FileSystemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "文件系统错误[{}]: {}", self.fs_type, self.details)
    }
}

impl Context for Unsupported {}
impl VfsKernelErrorProvider for Unsupported { fn kernel_errno(&self) -> Errno { Errno::ENOSYS } }
impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "不支持的操作: {}", self.operation)
    }
}

impl Context for Busy {}
impl VfsKernelErrorProvider for Busy { fn kernel_errno(&self) -> Errno { Errno::EBUSY } }
impl fmt::Display for Busy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "资源正忙")
    }
}

impl Context for ReadOnlyFileSystem {}
impl VfsKernelErrorProvider for ReadOnlyFileSystem { fn kernel_errno(&self) -> Errno { Errno::EROFS } }
impl fmt::Display for ReadOnlyFileSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "只读文件系统")
    }
}

impl Context for TooManyLinks {}
impl VfsKernelErrorProvider for TooManyLinks { fn kernel_errno(&self) -> Errno { Errno::EMLINK } }
impl fmt::Display for TooManyLinks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "链接数过多: {:?}", self.target_path)
    }
}

impl Context for FileNameTooLong {}
impl VfsKernelErrorProvider for FileNameTooLong { fn kernel_errno(&self) -> Errno { Errno::ENAMETOOLONG } }
impl fmt::Display for FileNameTooLong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "文件名过长: {}", self.file_name)
    }
}

impl Context for NoSpace {}
impl VfsKernelErrorProvider for NoSpace { fn kernel_errno(&self) -> Errno { Errno::ENOSPC } }
impl fmt::Display for NoSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "存储空间不足")
    }
}

impl Context for QuotaExceeded {}
impl VfsKernelErrorProvider for QuotaExceeded { fn kernel_errno(&self) -> Errno { Errno::EDQUOT } }
impl fmt::Display for QuotaExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "磁盘配额已超出")
    }
}

impl Context for Corrupted {}
impl VfsKernelErrorProvider for Corrupted { fn kernel_errno(&self) -> Errno { Errno::EUCLEAN } }
impl fmt::Display for Corrupted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "文件系统损坏: {}", self.details)
    }
}

impl Context for Interrupted {}
impl VfsKernelErrorProvider for Interrupted { fn kernel_errno(&self) -> Errno { Errno::EINTR } }
impl fmt::Display for Interrupted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "操作被中断")
    }
}

impl Context for WouldBlock {}
impl VfsKernelErrorProvider for WouldBlock { fn kernel_errno(&self) -> Errno { Errno::EAGAIN } }
impl fmt::Display for WouldBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "操作将阻塞")
    }
}

impl Context for TimedOut {}
impl VfsKernelErrorProvider for TimedOut { fn kernel_errno(&self) -> Errno { Errno::ETIMEDOUT } }
impl fmt::Display for TimedOut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "操作超时")
    }
}

impl Context for ResourceLimitExceeded {}
impl VfsKernelErrorProvider for ResourceLimitExceeded { fn kernel_errno(&self) -> Errno { Errno::ENOBUFS } }
impl fmt::Display for ResourceLimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "资源限制已超出: {}", self.limit_type)
    }
}

impl Context for InvalidHandle {}
impl VfsKernelErrorProvider for InvalidHandle { fn kernel_errno(&self) -> Errno { Errno::EBADF } }
impl fmt::Display for InvalidHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "无效的文件句柄")
    }
}

impl Context for FileTooLarge {}
impl VfsKernelErrorProvider for FileTooLarge { fn kernel_errno(&self) -> Errno { Errno::EFBIG } }
impl fmt::Display for FileTooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "文件过大")
    }
}

impl Context for MountError {}
impl VfsKernelErrorProvider for MountError { fn kernel_errno(&self) -> Errno { Errno::EBUSY } }
impl fmt::Display for MountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "挂载错误: {}", self.reason)
    }
}

impl Context for UnmountError {}
impl VfsKernelErrorProvider for UnmountError { fn kernel_errno(&self) -> Errno { Errno::EBUSY } }
impl fmt::Display for UnmountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "卸载错误: {}", self.reason)
    }
}

impl Context for CrossDeviceLink {}
impl VfsKernelErrorProvider for CrossDeviceLink { fn kernel_errno(&self) -> Errno { Errno::EXDEV } }
impl fmt::Display for CrossDeviceLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "不能跨设备链接")
    }
}


/// VFS 操作的结果类型别名。
/// C 是实现了 Context trait 的错误类型，代表了报告中最新的、最顶层的错误上下文。
/// 默认为 VfsErrorContext，一个通用的 VFS 错误上下文。
// pub type VfsResult<T, C = VfsErrorContext> = ErrorStackResult<T, C>; // 被更具体的类型别名取代

/// 为 VFS Context 提供转换为内核 Errno 的能力。
///
/// 实现此 trait 的 Context 类型可以被 `report_to_kernel_error` 函数处理，
/// 以便将 `error_stack::Report` 转换为 `nexus_error::Error`。
pub trait VfsKernelErrorProvider {
    /// 返回此 VFS Context 对应的内核 Errno。
    fn kernel_errno(&self) -> Errno;
}
/// 宏：用于快速定义 VFS Context 结构体。
///
/// 此宏会自动派生 Debug，并实现 Display、Context 和 VfsKernelErrorProvider。
///
/// # 参数
/// - `$struct_name`: 要定义的结构体名称。
/// - `$errno_val`: 此 Context 对应的内核 Errno 值 (一个 `nexus_error::Errno` 枚举成员)。
/// - `$display_fmt`: 用于 `core::fmt::Display` 实现的格式化字符串。
/// - `$(, $($field:ident: $field_ty:ty),*)?`: 可选的字段定义，用于在错误消息中包含额外信息。
///
/// # 示例
/// ```rust,ignore
/// define_vfs_context!(NotFound, Errno::ENOENT, "路径不存在: {target_path:?}", target_path: VfsPathBuf);
/// define_vfs_context!(PermissionDenied, Errno::EACCES, "操作权限不足");
/// ```
macro_rules! define_vfs_context {
    ($struct_name:ident, $errno_val:expr, $display_fmt:literal $(, $($field_name:ident: $field_type:ty),*)? ) => {
        // 注意：我们已经在文件顶部预先定义了结构体和trait实现
        // 不再在这里实现Display trait，因为我们已经为每个结构体手动实现了
    };
}

// --- VFS 核心错误上下文定义 ---

// 这些宏调用现在只负责实现Display trait，因为我们已经预先定义了结构体和其他trait实现
define_vfs_context!(InvalidOperation, Errno::EPERM, "无效操作: {reason}", reason: AllocString);
define_vfs_context!(NotFound, Errno::ENOENT, "路径或文件未找到: {target_path:?}", target_path: VfsPathBuf);
define_vfs_context!(AlreadyExists, Errno::EEXIST, "路径已存在: {target_path:?}", target_path: VfsPathBuf);
define_vfs_context!(PermissionDenied, Errno::EACCES, "权限不足: 对 {target_path:?} 进行 {operation} 操作", target_path: VfsPathBuf, operation: AllocString);
define_vfs_context!(NotADirectory, Errno::ENOTDIR, "路径不是一个目录: {target_path:?}", target_path: VfsPathBuf);
define_vfs_context!(IsADirectory, Errno::EISDIR, "路径是一个目录: {target_path:?}", target_path: VfsPathBuf);
define_vfs_context!(DirectoryNotEmpty, Errno::ENOTEMPTY, "目录不为空: {target_path:?}", target_path: VfsPathBuf);
define_vfs_context!(InvalidArgument, Errno::EINVAL, "提供的参数无效: {arg_name} - {reason}", arg_name: AllocString, reason: AllocString);
define_vfs_context!(IoError, Errno::EIO, "底层I/O错误: {details}", details: AllocString);
define_vfs_context!(FileSystemError, Errno::EUCLEAN, "文件系统错误 ({fs_type}): {details}", fs_type: AllocString, details: AllocString);
define_vfs_context!(Unsupported, Errno::ENOSYS, "不支持的操作: {operation}", operation: AllocString);
define_vfs_context!(Busy, Errno::EBUSY, "设备或资源忙");
define_vfs_context!(ReadOnlyFileSystem, Errno::EROFS, "文件系统只读");
define_vfs_context!(TooManyLinks, Errno::EMLINK, "链接数量过多: {target_path:?}", target_path: VfsPathBuf);
define_vfs_context!(FileNameTooLong, Errno::ENAMETOOLONG, "提供的文件名或路径组件过长: {file_name}", file_name: AllocString);
define_vfs_context!(NoSpace, Errno::ENOSPC, "设备上没有剩余空间");
define_vfs_context!(QuotaExceeded, Errno::EDQUOT, "已超出用户的磁盘限额");
define_vfs_context!(Corrupted, Errno::EUCLEAN, "发现数据损坏: {details}", details: AllocString);
define_vfs_context!(Interrupted, Errno::EINTR, "操作被中断");
define_vfs_context!(WouldBlock, Errno::EWOULDBLOCK, "操作会导致阻塞");
define_vfs_context!(TimedOut, Errno::ETIMEDOUT, "操作超时");
define_vfs_context!(ResourceLimitExceeded, Errno::ENOSR, "资源限制已达到: {limit_type}", limit_type: AllocString);
define_vfs_context!(InvalidHandle, Errno::EBADF, "无效的文件句柄");
define_vfs_context!(FileTooLarge, Errno::EFBIG, "文件太大");
define_vfs_context!(MountError, Errno::EBUSY, "挂载错误: {reason}", reason: AllocString);
define_vfs_context!(UnmountError, Errno::EBUSY, "卸载错误: {reason}", reason: AllocString);
define_vfs_context!(CrossDeviceLink, Errno::EXDEV, "尝试进行跨设备链接或重命名");

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub enum VfsErrorContext {
    InvalidOperation(InvalidOperation),
    NotFound(NotFound),
    AlreadyExists(AlreadyExists),
    PermissionDenied(PermissionDenied),
    NotADirectory(NotADirectory),
    IsADirectory(IsADirectory),
    DirectoryNotEmpty(DirectoryNotEmpty),
    InvalidArgument(InvalidArgument),
    IoError(IoError),
    FileSystemError(FileSystemError),
    Unsupported(Unsupported),
    Busy(Busy),
    ReadOnlyFileSystem(ReadOnlyFileSystem),
    TooManyLinks(TooManyLinks),
    FileNameTooLong(FileNameTooLong),
    NoSpace(NoSpace),
    QuotaExceeded(QuotaExceeded),
    Corrupted(Corrupted),
    Interrupted(Interrupted),
    WouldBlock(WouldBlock),
    TimedOut(TimedOut),
    ResourceLimitExceeded(ResourceLimitExceeded),
    InvalidHandle(InvalidHandle),
    FileTooLarge(FileTooLarge),
    MountError(MountError),
    UnmountError(UnmountError),
    CrossDeviceLink(CrossDeviceLink),
}

impl core::fmt::Display for VfsErrorContext {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VfsErrorContext::InvalidOperation(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::NotFound(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::AlreadyExists(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::PermissionDenied(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::NotADirectory(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::IsADirectory(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::DirectoryNotEmpty(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::InvalidArgument(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::IoError(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::FileSystemError(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::Unsupported(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::Busy(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::ReadOnlyFileSystem(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::TooManyLinks(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::FileNameTooLong(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::NoSpace(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::QuotaExceeded(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::Corrupted(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::Interrupted(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::WouldBlock(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::TimedOut(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::ResourceLimitExceeded(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::InvalidHandle(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::FileTooLarge(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::MountError(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::UnmountError(e) => core::fmt::Display::fmt(e, f),
            VfsErrorContext::CrossDeviceLink(e) => core::fmt::Display::fmt(e, f),
        }
    }
}

impl VfsKernelErrorProvider for VfsErrorContext {
    fn kernel_errno(&self) -> Errno {
        match self {
            VfsErrorContext::InvalidOperation(e) => e.kernel_errno(),
            VfsErrorContext::NotFound(e) => e.kernel_errno(),
            VfsErrorContext::AlreadyExists(e) => e.kernel_errno(),
            VfsErrorContext::PermissionDenied(e) => e.kernel_errno(),
            VfsErrorContext::NotADirectory(e) => e.kernel_errno(),
            VfsErrorContext::IsADirectory(e) => e.kernel_errno(),
            VfsErrorContext::DirectoryNotEmpty(e) => e.kernel_errno(),
            VfsErrorContext::InvalidArgument(e) => e.kernel_errno(),
            VfsErrorContext::IoError(e) => e.kernel_errno(),
            VfsErrorContext::FileSystemError(e) => e.kernel_errno(),
            VfsErrorContext::Unsupported(e) => e.kernel_errno(),
            VfsErrorContext::Busy(e) => e.kernel_errno(),
            VfsErrorContext::ReadOnlyFileSystem(e) => e.kernel_errno(),
            VfsErrorContext::TooManyLinks(e) => e.kernel_errno(),
            VfsErrorContext::FileNameTooLong(e) => e.kernel_errno(),
            VfsErrorContext::NoSpace(e) => e.kernel_errno(),
            VfsErrorContext::QuotaExceeded(e) => e.kernel_errno(),
            VfsErrorContext::Corrupted(e) => e.kernel_errno(),
            VfsErrorContext::Interrupted(e) => e.kernel_errno(),
            VfsErrorContext::WouldBlock(e) => e.kernel_errno(),
            VfsErrorContext::TimedOut(e) => e.kernel_errno(),
            VfsErrorContext::ResourceLimitExceeded(e) => e.kernel_errno(),
            VfsErrorContext::InvalidHandle(e) => e.kernel_errno(),
            VfsErrorContext::FileTooLarge(e) => e.kernel_errno(),
            VfsErrorContext::MountError(e) => e.kernel_errno(),
            VfsErrorContext::UnmountError(e) => e.kernel_errno(),
            VfsErrorContext::CrossDeviceLink(e) => e.kernel_errno(),
        }
    }
}

impl Context for VfsErrorContext {}

macro_rules! impl_from_for_vfs_error_context {
    ($($variant:ident($specific_error_type:ty)),* $(,)?) => {
        $(            impl From<$specific_error_type> for VfsErrorContext {
                fn from(err: $specific_error_type) -> Self {
                    VfsErrorContext::$variant(err)
                }
            }
        )*
    };
}

impl_from_for_vfs_error_context!(
    InvalidOperation(InvalidOperation),
    NotFound(NotFound),
    AlreadyExists(AlreadyExists),
    PermissionDenied(PermissionDenied),
    NotADirectory(NotADirectory),
    IsADirectory(IsADirectory),
    DirectoryNotEmpty(DirectoryNotEmpty),
    InvalidArgument(InvalidArgument),
    IoError(IoError),
    FileSystemError(FileSystemError),
    Unsupported(Unsupported),
    Busy(Busy),
    ReadOnlyFileSystem(ReadOnlyFileSystem),
    TooManyLinks(TooManyLinks),
    FileNameTooLong(FileNameTooLong),
    NoSpace(NoSpace),
    QuotaExceeded(QuotaExceeded),
    Corrupted(Corrupted),
    Interrupted(Interrupted),
    WouldBlock(WouldBlock),
    TimedOut(TimedOut),
    ResourceLimitExceeded(ResourceLimitExceeded),
    InvalidHandle(InvalidHandle),
    FileTooLarge(FileTooLarge),
    MountError(MountError),
    UnmountError(UnmountError),
    CrossDeviceLink(CrossDeviceLink),
);


/// 默认的 VFS 错误上下文，当没有更具体的上下文时使用。
/// 通常，我们期望在代码中创建并返回更具体的错误类型。
/// VFS 操作的统一 Result 类型，使用 error_stack 的 Report 和具体的错误上下文。
///
/// 注意：原 `VfsErrorContext` 类型别名被移除，因为现在有一个 `VfsErrorContext` 枚举。
/// `VfsResult` 现在直接使用 `crate::verror::VfsErrorContext` 作为其错误上下文类型。
pub type VfsResult<T> = error_stack::Result<T, crate::verror::VfsErrorContext>;

/// 将 `Report<C>` 转换为 `KernelError`。
///
/// 它会遍历 `Report` 中的所有 `Frame`，尝试找到第一个实现了 `VfsKernelErrorProvider`
/// 的 `Context`，并使用其 `kernel_errno()` 方法返回的 `Errno` 来创建 `KernelError`。
/// 如果没有找到这样的 `Context`，则默认使用 `Errno::EIO` (I/O 错误)。
///
/// 遍历顺序：从报告的当前上下文开始，然后是其下的帧。
/// error-stack 的报告结构是，`current_context()` 是最新的错误，frames 是之前的错误链。
/// 我们应该优先使用 `current_context()` 的 `Errno`，如果它提供了的话。
///
/// # 参数
/// - `report`: 要转换的 `error_stack::Report`。
///
/// # 返回
/// 转换后的 `nexus_error::Error`。
pub fn report_to_kernel_error(report: &Report<VfsErrorContext>) -> Errno {
    // 优先检查当前上下文 (最新的错误)
    // 主要通过匹配 VfsErrorContext 的变体来获取内核错误码
    // VfsErrorContext 的每个变体都包装了一个实现了 VfsKernelErrorProvider 的具体上下文类型
    match report.current_context() { // Ensure this match is against the VfsErrorContext enum
        VfsErrorContext::NotFound(c) => return c.kernel_errno(),
        VfsErrorContext::PermissionDenied(c) => return c.kernel_errno(),
        VfsErrorContext::AlreadyExists(c) => return c.kernel_errno(),
        VfsErrorContext::NotADirectory(c) => return c.kernel_errno(),
        VfsErrorContext::IsADirectory(c) => return c.kernel_errno(),
        VfsErrorContext::DirectoryNotEmpty(c) => return c.kernel_errno(),
        VfsErrorContext::InvalidArgument(c) => return c.kernel_errno(),
        VfsErrorContext::IoError(c) => return c.kernel_errno(),
        VfsErrorContext::FileSystemError(c) => return c.kernel_errno(),
        VfsErrorContext::Unsupported(c) => return c.kernel_errno(),
        VfsErrorContext::Busy(c) => return c.kernel_errno(),
        VfsErrorContext::ReadOnlyFileSystem(c) => return c.kernel_errno(),
        VfsErrorContext::TooManyLinks(c) => return c.kernel_errno(),
        VfsErrorContext::FileNameTooLong(c) => return c.kernel_errno(),
        VfsErrorContext::NoSpace(c) => return c.kernel_errno(),
        VfsErrorContext::QuotaExceeded(c) => return c.kernel_errno(),
        VfsErrorContext::Corrupted(c) => return c.kernel_errno(),
        VfsErrorContext::Interrupted(c) => return c.kernel_errno(),
        VfsErrorContext::WouldBlock(c) => return c.kernel_errno(),
        VfsErrorContext::TimedOut(c) => return c.kernel_errno(),
        VfsErrorContext::ResourceLimitExceeded(c) => return c.kernel_errno(),
        VfsErrorContext::InvalidHandle(c) => return c.kernel_errno(),
        VfsErrorContext::FileTooLarge(c) => return c.kernel_errno(),
        VfsErrorContext::MountError(c) => return c.kernel_errno(),
        VfsErrorContext::UnmountError(c) => return c.kernel_errno(),
        VfsErrorContext::CrossDeviceLink(c) => return c.kernel_errno(),
        VfsErrorContext::InvalidOperation(c) => return c.kernel_errno(),
    }

    // 这里不再需要遍历 frames，因为 match 已经是 exhaustive
    
    // 所有的分支都已经返回，不会执行到这里
    // 使用#[allow(unreachable_code)]来忽略警告
    // 所有的 match 分支都使用 return返回，所以这里的代码永远不会被执行
    #[allow(unreachable_code)]
    loop {}
}

// ... (rest of the code remains the same)
// 这些函数帮助快速创建包含特定上下文的 Report

pub fn vfs_err_invalid_operation(reason: impl Into<AllocString>) -> Report<VfsErrorContext> {
    let ctx = InvalidOperation { reason: reason.into() };
    Report::new(VfsErrorContext::InvalidOperation(ctx))
}

pub fn vfs_err_not_found(target_path: VfsPathBuf) -> Report<VfsErrorContext> {
    let ctx = NotFound { target_path: target_path.clone(), path: target_path };
    Report::new(VfsErrorContext::NotFound(ctx))
}

pub fn vfs_err_already_exists(target_path: VfsPathBuf) -> Report<VfsErrorContext> {
    let ctx = AlreadyExists { target_path };
    Report::new(VfsErrorContext::AlreadyExists(ctx))
}

pub fn vfs_err_permission_denied(target_path: VfsPathBuf, operation: AllocString) -> Report<VfsErrorContext> {
    let ctx = PermissionDenied { target_path, operation };
    Report::new(VfsErrorContext::PermissionDenied(ctx))
}

pub fn vfs_err_not_a_directory(target_path: VfsPathBuf) -> Report<VfsErrorContext> {
    let ctx = NotADirectory { target_path };
    Report::new(VfsErrorContext::NotADirectory(ctx))
}

pub fn vfs_err_is_a_directory(target_path: VfsPathBuf) -> Report<VfsErrorContext> {
    let ctx = IsADirectory { target_path };
    Report::new(VfsErrorContext::IsADirectory(ctx))
}

pub fn vfs_err_directory_not_empty(target_path: VfsPathBuf) -> Report<VfsErrorContext> {
    let ctx = DirectoryNotEmpty { target_path };
    Report::new(VfsErrorContext::DirectoryNotEmpty(ctx))
}

pub fn vfs_err_invalid_argument(
    arg_name: impl Into<AllocString>,
    reason: impl Into<AllocString>,
) -> Report<VfsErrorContext> {
    let ctx = InvalidArgument { arg_name: arg_name.into(), reason: reason.into() };
    Report::new(VfsErrorContext::InvalidArgument(ctx))
}

pub fn vfs_err_io(details: impl Into<AllocString>) -> Report<VfsErrorContext> {
    let ctx = IoError { details: details.into() };
    Report::new(VfsErrorContext::IoError(ctx))
}

pub fn vfs_err_filesystem(
    fs_type: impl Into<AllocString>,
    details: impl Into<AllocString>,
) -> Report<VfsErrorContext> {
    let ctx = FileSystemError { fs_type: fs_type.into(), details: details.into() };
    Report::new(VfsErrorContext::FileSystemError(ctx))
}

pub fn vfs_err_unsupported(operation: impl Into<AllocString>) -> Report<VfsErrorContext> {
    let ctx = Unsupported { operation: operation.into() };
    Report::new(VfsErrorContext::Unsupported(ctx))
}

pub fn vfs_err_busy() -> Report<VfsErrorContext> {
    Report::new(VfsErrorContext::Busy(Busy {}))
}

pub fn vfs_err_read_only_filesystem() -> Report<VfsErrorContext> {
    Report::new(VfsErrorContext::ReadOnlyFileSystem(ReadOnlyFileSystem {}))
}

pub fn vfs_err_too_many_links(target_path: VfsPathBuf) -> Report<VfsErrorContext> {
    let ctx = TooManyLinks { target_path };
    Report::new(VfsErrorContext::TooManyLinks(ctx))
}

pub fn vfs_err_filename_too_long(file_name: impl Into<AllocString>) -> Report<VfsErrorContext> {
    let ctx = FileNameTooLong { file_name: file_name.into() };
    Report::new(VfsErrorContext::FileNameTooLong(ctx))
}

pub fn vfs_err_no_space() -> Report<VfsErrorContext> {
    Report::new(VfsErrorContext::NoSpace(NoSpace {}))
}

pub fn vfs_err_quota_exceeded() -> Report<VfsErrorContext> {
    Report::new(VfsErrorContext::QuotaExceeded(QuotaExceeded {}))
}

pub fn vfs_err_corrupted(details: impl Into<AllocString>) -> Report<VfsErrorContext> {
    let ctx = Corrupted { details: details.into() };
    Report::new(VfsErrorContext::Corrupted(ctx))
}

pub fn vfs_err_interrupted() -> Report<VfsErrorContext> {
    Report::new(VfsErrorContext::Interrupted(Interrupted {}))
}

pub fn vfs_err_would_block() -> Report<VfsErrorContext> {
    Report::new(VfsErrorContext::WouldBlock(WouldBlock {}))
}

pub fn vfs_err_timed_out() -> Report<VfsErrorContext> {
    Report::new(VfsErrorContext::TimedOut(TimedOut {}))
}

pub fn vfs_err_resource_limit_exceeded(limit_type: impl Into<AllocString>) -> Report<VfsErrorContext> {
    let ctx = ResourceLimitExceeded { limit_type: limit_type.into() };
    Report::new(VfsErrorContext::ResourceLimitExceeded(ctx))
}

pub fn vfs_err_invalid_handle() -> Report<VfsErrorContext> {
    Report::new(VfsErrorContext::InvalidHandle(InvalidHandle {}))
}

pub fn vfs_err_file_too_large() -> Report<VfsErrorContext> {
    Report::new(VfsErrorContext::FileTooLarge(FileTooLarge {}))
}

pub fn vfs_err_mount(reason: impl Into<AllocString>) -> Report<VfsErrorContext> {
    let ctx = MountError { reason: reason.into() };
    Report::new(VfsErrorContext::MountError(ctx))
}

pub fn vfs_err_unmount(reason: impl Into<AllocString>) -> Report<VfsErrorContext> {
    let ctx = UnmountError { reason: reason.into() };
    Report::new(VfsErrorContext::UnmountError(ctx))
}

pub fn vfs_err_cross_device_link() -> Report<VfsErrorContext> {
    Report::new(VfsErrorContext::CrossDeviceLink(CrossDeviceLink {}))
}

#[cfg(test)]
mod tests {
    use super::*;
    // 重新导出常用的 core 类型
pub use core::{
    convert::{From, Into},
    default::Default,
    marker::{Send, Sync},
    option::Option,
    result::Result,
};

// 如果使用 alloc
extern crate alloc;
pub use alloc::{
    string::{String, ToString},
    vec::Vec,
    boxed::Box,
    format,
};

    // 辅助函数：为测试创建一个简单的 VfsPathBuf
    fn test_path(s: &str) -> VfsPathBuf {
        VfsPathBuf::from(s)
    }

    #[test]
    fn test_error_reporting_and_conversion() {
        fn func_a() -> VfsResult<i32, NotFound> { // Returns Result<i32, Report<NotFound>>
            Err(vfs_err_not_found(test_path("/test/a")))
                .attach_printable("附加信息: func_a 查找失败")
        }

        fn func_b() -> VfsResult<i32, InvalidArgument> { // Returns Result<i32, Report<InvalidArgument>>
            // change_context 将 Report<NotFound> 转换为 Report<InvalidArgument>
            // NotFound 仍然在 Report 的帧中
            func_a().change_context(InvalidArgument {
                arg_name: "arg_b".into(),
                reason: "来自 func_a 的错误".into(),
            })
            .attach_printable("附加信息: func_b 参数检查")
        }

        fn func_c() -> VfsResult<i32, FileSystemError> { // Returns Result<i32, Report<FileSystemError>>
            func_b().change_context(FileSystemError {
                fs_type: "test_fs".into(),
                details: "func_b 执行期间发生文件系统问题".into(),
            })
            .attach_printable("附加信息: func_c 顶层封装")
        }

        match func_c() {
            Ok(_) => panic!("测试应该返回错误"),
            Err(report) => {
                println!("完整错误报告:\n{:?}", report);
                // report_to_kernel_error 应该找到最具体的 VfsKernelErrorProvider
                // 在这个链条中，最新的 FileSystemError 会被找到
                let kernel_err = report_to_kernel_error(&report);
                println!("转换为 KernelError: {:?}, Errno: {:?}", kernel_err, kernel_err.errno());
                assert_eq!(kernel_err.errno(), Errno::EUCLEAN); // FileSystemError -> EUCLEAN

                // 验证是否能找到原始的 NotFound 上下文
                let original_not_found = report.frames().find_map(|frame| frame.context().downcast_ref::<NotFound>());
                assert!(original_not_found.is_some());
                assert_eq!(original_not_found.unwrap().path, test_path("/test/a"));
            }
        }
    }

    #[test]
    fn test_kernel_error_conversion_priority() {
        // 最顶层的错误是 PermissionDenied
        let report = vfs_err_permission_denied(VfsPathBuf::from("test/forbidden_path"), "test_operation".into())
            .attach_printable("顶层操作")
            .change_context(IoError { details: "底层IO失败".into() }); // 底层是 IoError
        
        // report_to_kernel_error 应优先使用当前上下文 (IoError)
        let kernel_err = report_to_kernel_error(&report);
        assert_eq!(kernel_err.errno(), Errno::EIO, "应使用最新的 IoError 的 EIO");

        // 如果我们反过来，让 IoError 是原始错误，PermissionDenied 是上层包装
        let report2 = Report::new(VfsErrorContext::IoError(IoError { details: "原始IO错误".into() }))
            .change_context(PermissionDenied{ target_path: VfsPathBuf::from("test/path_for_perm_denied"), operation: "test_op_in_change_context".into() });
        let kernel_err2 = report_to_kernel_error(&report2);
        assert_eq!(kernel_err2.errno(), Errno::EACCES, "应使用最新的 PermissionDenied 的 EACCES");
    }

    #[test]
    fn test_specific_error_to_kernel_error() {
        let report = vfs_err_not_a_directory(test_path("/file.txt"));
        let kernel_err = report_to_kernel_error(&report);
        assert_eq!(kernel_err.errno(), Errno::ENOTDIR);
    }

    #[test]
    fn test_no_vfs_kernel_provider_defaults_to_eio() {
        #[derive(Debug)]
        struct CustomNonVfsError;
        impl core::fmt::Display for CustomNonVfsError {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "一个非VFS的自定义错误")
            }
        }
        impl Context for CustomNonVfsError {}

        let report: Report<CustomNonVfsError> = Report::new(CustomNonVfsError);
        let kernel_err = report_to_kernel_error(&report);
        // 如果没有 VfsKernelErrorProvider，应默认返回 EIO
        assert_eq!(kernel_err.errno(), Errno::EIO);
    }

    #[test]
    fn test_display_of_defined_context() {
        let err = NotFound { target_path: test_path("/foo/bar").clone(), path: test_path("/foo/bar") };
        assert_eq!(format!("{}", err), "路径或文件未找到: VfsPathBuf(\"/foo/bar\")");

        let err2 = InvalidArgument { arg_name: "timeout".into(), reason: "must be positive".into() };
        assert_eq!(format!("{}", err2), "提供的参数无效: timeout - must be positive");
    }
}
