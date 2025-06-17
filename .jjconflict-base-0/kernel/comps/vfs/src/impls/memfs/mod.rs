// 导出内存文件系统组件
mod fs;
mod provider;
mod vnode;
mod handle;

pub use fs::InMemoryFileSystem;
pub use provider::InMemoryFsProvider;
pub use vnode::InMemoryVnode;
pub use handle::{InMemoryFileHandle, InMemoryDirHandle};

// 导出内存文件系统提供者
pub fn get_memfs_provider() -> InMemoryFsProvider {
    InMemoryFsProvider::new()
}
