use alloc::boxed::Box;
use alloc::sync::Arc;
use async_trait::async_trait;

use crate::{types::FileMode, AsyncBlockDevice, AsyncFileSystem, VfsResult};
use crate::{
    traits::AsyncFileSystemProvider,
    types::{FilesystemId, FsOptions, MountId, VnodeMetadata, VnodeType, Timestamps},
};

use super::{fs::InMemoryFileSystem, vnode::InMemoryVnode};

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
        _source_device: Option<Arc<dyn AsyncBlockDevice + Send + Sync>>,
        options: &FsOptions,
        mount_id: MountId,
        fs_id: FilesystemId,
    ) -> VfsResult<Arc<dyn AsyncFileSystem + Send + Sync>> {
        // 创建一个新的内存文件系统实例
        let fs = Arc::new(InMemoryFileSystem::new(fs_id, mount_id, options.clone()));
        
        // 创建根目录的元数据
        let now = Timestamps::now();
        let root_metadata = VnodeMetadata {
            vnode_id: 1,
            fs_id,
            kind: VnodeType::Directory,
            permissions: FileMode::OWNER_RWE | FileMode::GROUP_RE | FileMode::OTHER_RE,
            timestamps: now,
            nlinks: 2,
            rdev: None,
            size: 0,
            uid: 0,
            gid: 0,
        };
        
        // 创建根目录节点
        let root_node = Arc::new(InMemoryVnode::new_directory(
            Arc::downgrade(&fs),
            1,  // 根节点ID为1
            root_metadata,
        ));
        
        // 初始化文件系统的根节点
        fs.init_root_node(root_node).await?;
        
        Ok(fs as Arc<dyn AsyncFileSystem + Send + Sync>)
    }
}
