//! Ext4Fs ‑ VFS::FileSystem 的实现，内部委托给 another_ext4。

use alloc::sync::Arc;
use ostd::sync::Mutex;

use another_ext4::{self as ext4, BlockDevice};
use crate::{vfs_err_unsupported, FileSystem, FilesystemStats, FsOptions, VfsResult};
use crate::impls::ext4_fs::vnode::Ext4Vnode;
use crate::types::{FilesystemId, MountId, VnodeId};

pub struct Ext4Fs {
    mount_id: u64,
    fs_id:    u64,
    options:  FsOptions,
    block:    Arc<dyn BlockDevice>,
    pub inner:    Mutex<ext4::Ext4>, // 同步 ext4 实例
}

impl Ext4Fs {
    pub fn new(mount_id: u64, fs_id: u64, options: FsOptions, block: Arc<dyn BlockDevice>) -> Self {
        let inner = ext4::Ext4::load(block.clone()).expect("ext4 load failed");
        Self { mount_id, fs_id, options, block, inner: Mutex::new(inner) }
    }

    pub fn root_vnode_arc(self: Arc<Self>) -> Arc<Ext4Vnode> {
        Ext4Vnode::new_root(self)
    }
}

/* ---------- FileSystem ---------- */
impl FileSystem for Ext4Fs {
    type Vnode = Ext4Vnode;
    
    fn id(&self) -> FilesystemId { self.fs_id as FilesystemId }

    fn mount_id(&self) -> MountId { self.mount_id as MountId }

    fn options(&self) -> &FsOptions { &self.options }

    async fn root_vnode(self: Arc<Self>) -> VfsResult<Arc<Self::Vnode>> {
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

    async fn prepare_unmount(&self) -> VfsResult<()> {
        self.sync().await
    }

    async fn reclaim_vnode(&self, _id: VnodeId) -> VfsResult<bool> {
        Ok(true)
    }

    fn fs_type_name(&self) -> &'static str { "ext4" }

    fn is_readonly(&self) -> bool { self.options.read_only }
}
