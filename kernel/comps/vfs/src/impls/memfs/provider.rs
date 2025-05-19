use alloc::boxed::Box;
use alloc::sync::Arc;
use async_trait::async_trait;

use crate::{VfsResult, AsyncFileSystem, AsyncBlockDevice};
use crate::{
    traits::AsyncFileSystemProvider,
    types::{FilesystemId, FsOptions, MountId},
};

use super::fs::InMemoryFileSystem;

/// 内存文件系统提供者
#[derive(Debug)]
pub struct InMemoryFsProvider {}

impl InMemoryFsProvider {
    /// 创建新的内存文件系统提供者
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl AsyncFileSystemProvider for InMemoryFsProvider {
    /// 获取文件系统类型名称
    fn fs_type_name(&self) -> &'static str {
        "inmemoryfs"
    }

    /// 挂载一个内存文件系统
    async fn mount(
        &self,
        source_device: Option<Arc<dyn AsyncBlockDevice + Send + Sync>>,
        options: &FsOptions,
        mount_id: MountId,
        fs_id: FilesystemId,
    ) -> VfsResult<Arc<dyn AsyncFileSystem + Send + Sync>> {
        // 创建一个新的内存文件系统实例
        let fs = InMemoryFileSystem::new(fs_id, mount_id, options.clone());
        
        // 返回包装在 Arc 中的文件系统实例
        Ok(Arc::new(fs))
    }
}
