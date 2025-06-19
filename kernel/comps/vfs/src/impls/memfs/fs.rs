use alloc::boxed::Box;
use alloc::format;
use alloc::sync::Arc;
use async_trait::async_trait;
use ostd::sync::RwLock;

use crate::{vfs_err_not_implemented, VfsResult};
use crate::{
    traits::FileSystem,
    types::{FilesystemId, FilesystemStats, FsOptions, MountId, VnodeId},
};

use super::vnode::InMemoryVnode;

/// 内存文件系统
#[derive(Debug)]
pub struct InMemoryFileSystem {
    /// 文件系统 ID
    id: FilesystemId,
    /// 挂载点 ID
    mount_id: MountId,
    /// 文件系统根节点
    root: RwLock<Option<Arc<InMemoryVnode>>>,
    /// 挂载选项
    options: FsOptions,
}

impl InMemoryFileSystem {
    /// 创建一个新的内存文件系统实例
    pub fn new(id: FilesystemId, mount_id: MountId, options: FsOptions) -> Self {
        Self {
            id,
            mount_id,
            root: RwLock::new(None),
            options,
        }
    }
    
    /// 初始化根节点
    pub(crate) async fn init_root_node(&self, root_node: Arc<InMemoryVnode>) -> VfsResult<()> {
        let mut root_guard = self.root.write().await;
        *root_guard = Some(root_node);
        Ok(())
    }
}

#[async_trait]
impl FileSystem for InMemoryFileSystem {
    /// 获取文件系统类型名称
    fn fs_type_name(&self) -> &'static str {
        "inmemoryfs"
    }
    
    /// 获取文件系统 ID
    fn id(&self) -> FilesystemId {
        self.id
    }

    /// 获取挂载点 ID
    fn mount_id(&self) -> MountId {
        self.mount_id
    }
    
    /// 获取挂载选项
    fn options(&self) -> &FsOptions {
        &self.options
    }
    
    /// 检查文件系统是否为只读
    fn is_readonly(&self) -> bool {
        self.options.read_only
    }

    /// 获取根节点
    async fn root_vnode(&self) -> VfsResult<Arc<dyn crate::traits::Vnode + Send + Sync>> {
        if let Some(root) = self.root.read().await.as_ref() {
            Ok(root.clone() as Arc<dyn crate::traits::Vnode + Send + Sync>)
        } else {
            Err(vfs_err_not_implemented!("InMemoryFileSystem::root_vnode - 根节点未初始化"))
        }
    }

    /// 获取文件系统统计信息
    async fn statfs(&self) -> VfsResult<FilesystemStats> {
        Err(vfs_err_not_implemented!("InMemoryFileSystem::statfs"))
    }
    
    /// 同步文件系统缓存到存储设备
    async fn sync(&self) -> VfsResult<()> {
        // 内存文件系统不需要同步到设备
        Ok(())
    }
    
    /// 准备卸载文件系统
    async fn unmount_prepare(&self) -> VfsResult<()> {
        // 内存文件系统没有特殊的卸载准备工作
        Ok(())
    }
    
    /// 垃圾回收未使用的 vnode
    async fn gc_vnode(&self, _vnode_id: VnodeId) -> VfsResult<bool> {
        // 简单实现，不做实际回收
        // 返回 false 表示未回收该 vnode
        Ok(false)
    }
}
