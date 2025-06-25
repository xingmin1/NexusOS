//! 内存设备文件系统（devfs）
//
//! - 根目录仅在内存中维护子节点表
//! - 支持动态注册字符设备：/dev/<name>
//! - 仅依赖 `AsyncCharDevice` trait，避免与块层耦合
//!   （若需块设备，可额外引入 `AsyncBlockDevice`）

mod driver;
mod filesystem;
mod provider;
mod vnode;
mod handle;
mod device;

pub use driver::AsyncCharDevice;
pub use filesystem::DevFs;
pub use provider::{get_devfs_provider, DevFsProvider};
pub use vnode::DevVnode;
pub use handle::{DevDirHandle, DevCharHandle};
pub use device::StdOutDevice;