//! VFS 内部错误处理模块
//!
//! 本模块基于 `error-stack` crate 实现，为 VFS 操作提供结构化、信息丰富的错误处理能力。
//! 它定义了 VFS 特定的错误上下文 (`Context`) 类型，一个用于将这些上下文映射到
//! 内核标准错误码 (`nexus_error::Errno`) 的 trait (`VfsErrorProvider`)，以及
//! 在 VFS API 边界将内部 `error_stack::Report` 转换为 `nexus_error::Error` 的工具函数。

// 使用 extern crate alloc; 如果是在 no_std 环境且 alloc 不是全局可用的
// extern crate alloc;

use nexus_error::error_stack::{Context, Report, Result as ErrorStackResult};
use nexus_error::{Error as KernelError, Errno};
use alloc::string::String as AllocString;
use core::fmt;
use crate::path::VfsPathBuf;
use crate::alloc::string::ToString;

// --- 3.1. VfsErrorProvider Trait ---

/// 为VFS内部错误上下文提供一个建议的 `nexus_error::Errno`。
///
/// 实现此 trait 的 Context 类型可以被 `vfs_internal_report_to_kernel_error` 函数处理，
/// 以便将 `error_stack::Report` 转换为 `nexus_error::Error`。
pub trait VfsErrorProvider: Send + Sync {
    /// 返回此 VFS Context 对应的内核 `Errno`。
    fn os_errno(&self) -> Errno;
}

// --- 3.2. VFS特定的 Context 类型 ---

/// 辅助宏 `define_vfs_context!`：用于简化 `Context` 结构体的定义。
///
/// 此宏会自动派生 `Debug`，并实现 `core::fmt::Display`、`error_stack::Context`
/// 和 `VfsErrorProvider`。
///
/// # 参数
/// - `$struct_name`: 要定义的结构体名称。
/// - `$errno_val`: 此 Context 对应的内核 `Errno` 值 (一个 `nexus_error::Errno` 枚举成员)。
/// - `$display_fmt`: 用于 `core::fmt::Display` 实现的格式化字符串。
/// - `$(, $($field:ident: $field_ty:ty),*)?`: 可选的字段定义列表，每个字段包含名称和类型。
///
/// # 注意
/// 源码位置信息 (loc) 将由 `error_stack::Report` 或 `Frame` 自动捕获，
/// `Context` 结构体本身不需要显式存储 'loc' 字段。
macro_rules! define_vfs_context {
    ($struct_name:ident, $errno_val:expr, $display_fmt:literal $(, $($field:ident: $field_ty:ty),*)? ) => {
        #[derive(Debug)]
        pub struct $struct_name {
            $( $(pub $field: $field_ty,)* )?
        }

        impl fmt::Display for $struct_name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                // 确保字段在格式化字符串中的占位符 {} 与字段定义的顺序和数量一致
                write!(f, $display_fmt , $($(self.$field,)*)?)
            }
        }

        impl Context for $struct_name {} // 标记为 error-stack 的 Context

        // 使用 $crate::verror::VfsErrorProvider 来确保宏在不同模块调用时能正确找到 VfsErrorProvider trait。
        // 如果此宏仅在 verror.rs 内部使用，则可以直接用 VfsErrorProvider。
        // 根据计划，这里使用 $crate 形式。
        impl $crate::verror::VfsErrorProvider for $struct_name {
            fn os_errno(&self) -> Errno {
                $errno_val
            }
        }
    };
}

// 在模块内部导出宏以供本模块内的其他部分（如果需要）或通过 `crate::verror::define_vfs_context` 从外部使用。
// 如果只在本文件使用，pub(crate) use define_vfs_context; 也可以。
// 根据计划，是 pub(crate)
pub(crate) use define_vfs_context;

// 示例 `Context` 定义:
// 注意：确保 `VfsPathBuf` 和 `AllocString` 已经被正确导入。
// `KernelError` 已经在顶部导入。

define_vfs_context!(PathNotFoundContext, Errno::ENOENT, "Path not found: {}", path: VfsPathBuf);
define_vfs_context!(PermissionDeniedContext, Errno::EACCES, "Permission denied for operation '{}' on path '{}'", operation: AllocString, path: VfsPathBuf);
define_vfs_context!(IoOperationContext, Errno::EIO, "I/O error during VFS operation: {}", operation_description: AllocString);
define_vfs_context!(AlreadyExistsContext, Errno::EEXIST, "Entry already exists: {}", path: VfsPathBuf);
define_vfs_context!(NotADirectoryContext, Errno::ENOTDIR, "Not a directory: {}", path: VfsPathBuf);
define_vfs_context!(IsADirectoryContext, Errno::EISDIR, "Is a directory: {}", path: VfsPathBuf);
define_vfs_context!(NotEmptyContext, Errno::ENOTEMPTY, "Directory not empty: {}", path: VfsPathBuf);
define_vfs_context!(InvalidArgumentContext, Errno::EINVAL, "Invalid argument for '{}': {}", operation: AllocString, reason: AllocString);
define_vfs_context!(FilesystemSpecificErrorContext, Errno::EIO, "Filesystem specific error in '{}': (code: {}) {}", fs_type: &'static str, code: i32, message: AllocString);
define_vfs_context!(TooManyLinksContext, Errno::EMLINK, "Too many symbolic links encountered while resolving path: {}", path: VfsPathBuf);
define_vfs_context!(CrossDeviceLinkContext, Errno::EXDEV, "Cross-device link or rename attempted from '{}' to '{}'", source_path: VfsPathBuf, target_path: VfsPathBuf);
define_vfs_context!(UnsupportedOperationContext, Errno::ENOSYS, "Unsupported operation: {}", operation: AllocString);

/// 用于包装从底层（如AsyncBlockDevice或kernel其他组件）返回的 `nexus_error::Error`。
/// 这种错误源于VFS外部，但VFS操作因其失败。
#[derive(Debug)]
pub struct WrappedKernelErrorContext {
    /// 原始的 `nexus_error::Error`。
    pub source: KernelError,
    /// 描述导致此错误的VFS操作。
    pub operation_description: AllocString,
}

impl fmt::Display for WrappedKernelErrorContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Underlying kernel error during VFS operation '{}'. Source Error (Errno: {:?}): {}",
            self.operation_description,
            self.source.error(), // 获取 KernelError 中的 Errno
            self.source // KernelError 应该实现了 Display
        )
    }
}

impl Context for WrappedKernelErrorContext {}

impl VfsErrorProvider for WrappedKernelErrorContext {
    fn os_errno(&self) -> Errno {
        self.source.error() // 直接使用源错误的Errno
    }
}

// 默认的、通用的错误上下文，当没有更具体的Context适用时使用。
// 鼓励使用更具体的错误类型，此类型作为后备。
define_vfs_context!(GenericVfsOperationContext, Errno::EIO, "VFS operation failed: {}", details: AllocString);

// --- 3.3. VFS内部 Result 类型别名 ---

/// VFS 内部操作使用的 `Result` 类型。
///
/// 错误类型是 `error_stack::Report<C>`，其中 `C` 是一个实现了 `Context` 和 `VfsErrorProvider` 的类型。
/// 默认的 `Context` 是 `GenericVfsOperationContext`，表示一个通用的VFS操作错误。
/// 在具体的VFS函数签名中，可以指定更具体的 `Context` 类型 `C` 来提供更精确的错误信息。
/// 例如: `fn specific_op() -> VfsResult<SuccessType, SpecificOperationContext> { ... }`
pub type VfsResult<T> = ErrorStackResult<T, nexus_error::Error>;

// --- 3.4. 转换函数: VfsInternalReport 到 nexus_error::Error ---

/// 将VFS内部的 `error_stack::Report<C>` 转换为外部统一的 `nexus_error::Error`。
///
/// 在转换前，会使用 `tracing::error!` 记录完整的错误报告，包括所有上下文帧和附件。
/// `error_stack::Report` 实现了 `fmt::Debug`，使用 `{:#?}` 会打印详细的堆栈信息。
///
/// # 类型参数
/// - `C`: 实现了 `error_stack::Context` 和 `core::fmt::Debug` 的类型。
///        `error_stack::Context` trait 本身要求 `Debug + Display + Send + Sync + 'static`。
///
/// # 参数
/// - `report`: 要转换的 `error_stack::Report<C>`。
///
/// # 返回
/// 转换后的 `nexus_error::Error`。
pub fn vfs_internal_report_to_kernel_error<C: Context + 'static>(
    report: Report<C>,
) -> KernelError {
    // 1. 使用tracing记录完整的错误报告。
    // {:#?} 会利用 error_stack 的 Debug 实现打印结构化的错误链。
    tracing::error!("VFS Operation Failed. Internal Report:\n{:#?}", report);

    // 2. 从Report中提取最合适的Errno。
    let mut final_errno: Option<Errno> = None;

    // 尝试从当前（最顶层）上下文获取 Errno
    if let Some(&provider) = report.downcast_ref::<&dyn VfsErrorProvider>() {
        final_errno = Some(provider.os_errno());
    }

    let resolved_errno = final_errno.unwrap_or_else(|| {
        tracing::warn!(
            "No VfsErrorProvider found in error report stack for Report type <{}>, or top context is not a VFS context. Using EIO as default Errno.",
            core::any::type_name::<C>() // 记录未能提供Errno的Report的泛型类型C
        );
        Errno::EIO // 如果整个错误链都没有VFS定义的Context提供Errno，则使用通用I/O错误
    });

    // 3. 创建并返回 nexus_error::Error
    // 假设 nexus_error::Error 有一个类似 new(Errno) 的构造函数
    // 和一个可选的设置消息的方法。
    // KernelError::new(resolved_errno) // 如果 KernelError 只有 Errno
    // 或者，如果 KernelError 可以包含消息：
    KernelError::new(resolved_errno)
}

// macro_rules! vfs_bail {
//     ($context: expr) => {
//         return Err($context.into());
//     };
// }

pub fn vfs_err_unsupported(operation: &str) -> nexus_error::error_stack::Report<nexus_error::Error> {
    Report::new(vfs_internal_report_to_kernel_error(
        Report::new(
            UnsupportedOperationContext { operation: operation.to_string() }
        )
    ))
}

pub fn vfs_err_invalid_argument(operation: &str, reason: &str) -> nexus_error::error_stack::Report<nexus_error::Error> {
    Report::new(vfs_internal_report_to_kernel_error(
        Report::new(
            InvalidArgumentContext { operation: operation.to_string(), reason: reason.to_string() }
        )
    ))
}