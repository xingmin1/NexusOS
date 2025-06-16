//! Ext4Fs ‑ VFS::AsyncFileSystem 的实现，内部委托给 another_ext4。

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use async_trait::async_trait;
use ostd::sync::Mutex;
use tracing::{debug, error};

use vfs::{
    AsyncBlockDevice, AsyncFileSystem, AsyncVnode, DirectoryEntry, FilesystemStats, FsOptions,
    VfsResult, VnodeId, VnodeMetadata, VnodeMetadataChanges, VnodeType,
};
use another_ext4::{self as ext4};

use crate::{block_dev::VirtioBlockDevice, vnode::Ext4Vnode};

pub struct Ext4Fs<D: AsyncBlockDevice> {
    mount_id: u64,
    fs_id:    u64,
    options:  FsOptions,
    block:    Arc<D>,
    pub(crate) inner:    Mutex<ext4::Ext4>, // 同步 ext4 实例
}

impl<D: AsyncBlockDevice> Ext4Fs<D> {
    pub fn new(mount_id: u64, fs_id: u64, options: FsOptions, block: Arc<D>) -> Self {
        // another_ext4::Ext4::new 需要 Arc<dyn BlockDevice>
        let dev_for_ext4 = Arc::new(block.clone()) as Arc<dyn ext4::BlockDevice>;
        let inner = ext4::Ext4::load(dev_for_ext4).expect("ext4 load failed");
        Self { mount_id, fs_id, options, block, inner: Mutex::new(inner) }
    }

    pub fn root_vnode_arc(self: &Arc<Self>) -> Arc<dyn AsyncVnode + Send + Sync> {
        Ext4Vnode::new_root(self.clone())
    }
}

/* ---------- AsyncFileSystem ---------- */

#[async_trait]
impl<D: AsyncBlockDevice + 'static> AsyncFileSystem for Ext4Fs<D> {
    fn id(&self) -> u64 { self.fs_id }

    fn mount_id(&self) -> u64 { self.mount_id }

    fn fs_type_name(&self) -> &'static str { "ext4" }

    fn options(&self) -> &FsOptions { &self.options }

    fn is_readonly(&self) -> bool { self.options.read_only }

    async fn root_vnode(&self) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        Ok(self.root_vnode_arc())
    }

    async fn statfs(&self) -> VfsResult<FilesystemStats> {
        // 简易实现
        Ok(FilesystemStats {
            block_size: self.block.block_size_bytes()?,
            total_blocks: self.block.total_blocks()?,
            free_blocks: 0,
            avail_blocks: 0,
            total_inodes: 0,
            free_inodes: 0,
            name_max_len: 255,
            optimal_io_size: Some(self.block.block_size_bytes()? as u64),
            fs_id: self.fs_id,
            fs_type_name: self.fs_type_name().to_string(),
        })
    }

    async fn sync(&self) -> VfsResult<()> {
        // another_ext4 flush
        self.inner.lock().flush_all();
        self.block.flush().await
    }

    async fn unmount_prepare(&self) -> VfsResult<()> {
        self.sync().await
    }

    async fn gc_vnode(&self, _vnode_id: VnodeId) -> VfsResult<bool> {
        // ext4 有内部缓存，示例直接返回 true 表示 GC 成功
        Ok(true)
    }
}
