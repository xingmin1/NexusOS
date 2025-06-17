//! Ext4Fs ‑ VFS::AsyncFileSystem 的实现，内部委托给 another_ext4。

use alloc::{sync::Arc, boxed::Box};
use async_trait::async_trait;
use ostd::sync::Mutex;

use another_ext4::{self as ext4, BlockDevice};
use vfs::{
    types::{FilesystemId, MountId}, vfs_err_unsupported, AsyncFileSystem, AsyncVnode, FilesystemStats, FsOptions, VfsResult
};

use crate::vnode::Ext4Vnode;

pub struct Ext4Fs {
    mount_id: u64,
    fs_id:    u64,
    options:  FsOptions,
    block:    Arc<dyn BlockDevice>,
    pub(crate) inner:    Mutex<ext4::Ext4>, // 同步 ext4 实例
}

impl Ext4Fs {
    pub fn new(mount_id: u64, fs_id: u64, options: FsOptions, block: Arc<dyn BlockDevice>) -> Self {
        let inner = ext4::Ext4::load(block.clone()).expect("ext4 load failed");
        Self { mount_id, fs_id, options, block, inner: Mutex::new(inner) }
    }

    pub fn root_vnode_arc(self: &Arc<Self>) -> Arc<dyn AsyncVnode + Send + Sync> {
        Ext4Vnode::new_root(self.clone())
    }
}

/* ---------- AsyncFileSystem ---------- */

#[async_trait]
impl AsyncFileSystem for Ext4Fs {
    fn id(&self) -> FilesystemId { self.fs_id as FilesystemId }

    fn mount_id(&self) -> MountId { self.mount_id as MountId }

    fn fs_type_name(&self) -> &'static str { "ext4" }

    fn options(&self) -> &FsOptions { &self.options }

    fn is_readonly(&self) -> bool { self.options.read_only }

    async fn root_vnode(self: Arc<Self>) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        Ok(self.root_vnode_arc())
    }

    async fn statfs(&self) -> VfsResult<FilesystemStats> {
        // 简易实现
        Err(vfs_err_unsupported!("statfs not supported"))
        // Ok(FilesystemStats {
        //     block_size: BLOCK_SIZE,
        //     total_blocks: self.?,
        //     free_blocks: 0,
        //     avail_blocks: 0,
        //     total_inodes: 0,
        //     free_inodes: 0,
        //     name_max_len: 255,
        //     optimal_io_size: Some(self.block.block_size_bytes()? as u64),
        //     fs_id: self.fs_id as FilesystemId,
        //     fs_type_name: self.fs_type_name().to_string(),
        // })
    }

    async fn sync(&self) -> VfsResult<()> {
        // another_ext4 flush
        self.inner.lock().await.flush_all();
        Ok(())
    }

    async fn unmount_prepare(&self) -> VfsResult<()> {
        self.sync().await
    }

    async fn gc_vnode(&self, _vnode_id: u64) -> VfsResult<bool> {
        // ext4 有内部缓存，示例直接返回 true 表示 GC 成功
        Ok(true)
    }
}
