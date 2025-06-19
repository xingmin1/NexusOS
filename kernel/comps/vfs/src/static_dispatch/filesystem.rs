
/* ---------- FileSystem 静态派发 ---------- */
use alloc::sync::Arc;
use crate::{FileSystem, FsOptions, VfsResult};
use crate::impls::ext4_fs::filesystem::Ext4Fs;
use crate::static_dispatch::vnode::SVnode;
use crate::types::{FilesystemId, MountId, VnodeId};

#[derive(Clone)]
pub enum SFileSystem {
    Ext4(Arc<Ext4Fs>),
}

impl SFileSystem {
    pub fn id(&self) -> FilesystemId {
        match self {
            SFileSystem::Ext4(fs) => fs.id(),
        }
    }

    pub fn mount_id(&self) -> MountId {
        match self {
            SFileSystem::Ext4(fs) => fs.mount_id(),
        }
    }

    pub fn options(&self) -> &FsOptions {
        match self {
            SFileSystem::Ext4(fs) => fs.options(),
        }
    }

    pub fn is_readonly(&self) -> bool {
        match self {
            SFileSystem::Ext4(fs) => fs.is_readonly(),
        }
    }

    pub async fn root_vnode(&self) -> VfsResult<SVnode> {
        match self {
            SFileSystem::Ext4(fs) => {
                let v = fs.clone().root_vnode().await?;
                Ok(SVnode::from(v))
            }
        }
    }

    pub async fn statfs(&self) -> VfsResult<crate::types::FilesystemStats> {
        match self {
            SFileSystem::Ext4(fs) => fs.statfs().await,
        }
    }

    pub async fn sync(&self) -> VfsResult<()> {
        match self {
            SFileSystem::Ext4(fs) => fs.sync().await,
        }
    }

    pub async fn prepare_unmount(&self) -> VfsResult<()> {
        match self {
            SFileSystem::Ext4(fs) => fs.prepare_unmount().await,
        }
    }

    pub async fn reclaim_vnode(&self, id: VnodeId) -> VfsResult<bool> {
        match self {
            SFileSystem::Ext4(fs) => fs.reclaim_vnode(id).await,
        }
    }

    pub fn fs_type_name(&self) -> &'static str {
        match self {
            SFileSystem::Ext4(fs) => fs.fs_type_name(),
        }
    }
}

impl From<Arc<Ext4Fs>> for SFileSystem {
    fn from(fs: Arc<Ext4Fs>) -> Self {
        SFileSystem::Ext4(fs)
    }
}