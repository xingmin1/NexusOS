use alloc::sync::Arc;
use nexus_error::Errno;
use crate::impls::dev_fs::{DevDirHandle, DevVnode};
use crate::impls::ext4_fs::vnode::{Ext4DirHandle, Ext4Vnode};
use crate::types::{DirectoryEntry, VnodeId, VnodeMetadataChanges};
use crate::verror::KernelError;
use crate::{DirHandle, VfsResult, Vnode, VnodeMetadata};
use crate::static_dispatch::vnode::SVnode;
use crate::traits::DirCap;

#[derive(Clone)]
pub enum SDir {
    Ext4(Arc<Ext4Vnode>),
    Dev(Arc<DevVnode>),
}

#[derive(Clone)]
pub enum SDirHandle {
    Ext4(Arc<Ext4DirHandle>),
    Dev(Arc<DevDirHandle>),
}

/// Vnode —— 最小公共能力 for Dir
impl SDir {
    pub fn id(&self) -> VnodeId {
        match self {
            SDir::Ext4(v) => v.id(),
            SDir::Dev(v) => v.id(),
        }
    }

    pub async fn metadata(&self) -> VfsResult<VnodeMetadata> {
        match self {
            SDir::Ext4(v) => v.metadata().await,
            SDir::Dev(v) => v.metadata().await,
        }
    }

    pub async fn set_metadata(&self, ch: VnodeMetadataChanges) -> VfsResult<()> {
        match self {
            SDir::Ext4(v) => v.set_metadata(ch).await,
            SDir::Dev(v) => v.set_metadata(ch).await,
        }
    }
}

// DirCap
impl SDir {
    pub async fn open_dir(self) -> VfsResult<SDirHandle> {
        match self {
            SDir::Ext4(v) => Ok(SDirHandle::Ext4(v.open_dir().await?)),
            SDir::Dev(v) => Ok(SDirHandle::Dev(v.open_dir().await?)),
        }
    }

    pub async fn lookup(&self, name: &crate::types::OsStr) -> VfsResult<SVnode> {
        match self {
            SDir::Ext4(v) => {
                let child = v.lookup(name).await?;
                Ok(SVnode::from(child))
            }
            SDir::Dev(v) => {
                let child = v.lookup(name).await?;
                Ok(SVnode::from(child))
            }
        }
    }

    pub async fn create(
        &self,
        name: &crate::types::OsStr,
        kind: crate::types::VnodeType,
        perm: crate::types::FileMode,
        rdev: Option<u64>,
    ) -> VfsResult<SVnode> {
        match self {
            SDir::Ext4(v) => {
                let node = v.create(name, kind, perm, rdev).await?;
                Ok(SVnode::from(node))
            }
            SDir::Dev(v) => {
                let node = v.create(name, kind, perm, rdev).await?;
                Ok(SVnode::from(node))
            }
        }
    }

    pub async fn rename(
        &self,
        old_name: &crate::types::OsStr,
        new_parent: &Self,
        new_name: &crate::types::OsStr,
    ) -> VfsResult<()> {
        match (self, new_parent) {
            (SDir::Ext4(old), SDir::Ext4(new_p)) => old.rename(old_name, new_p, new_name).await,
            (SDir::Dev(old), SDir::Dev(new_p)) => old.rename(old_name, new_p, new_name).await,
            _ => Err(KernelError::with_message(Errno::EINVAL, "rename").into()),
        }
    }

    /// 删除普通文件或符号链接
    pub async fn unlink(&self, name: &crate::types::OsStr) -> VfsResult<()> {
        match self {
            SDir::Ext4(dir) => dir.unlink(name).await,
            SDir::Dev(dir) => dir.unlink(name).await,
        }
    }

    /// 删除空目录
    pub async fn rmdir(&self, name: &crate::types::OsStr) -> VfsResult<()> {
        match self {
            SDir::Ext4(dir) => dir.rmdir(name).await,
            SDir::Dev(dir) => dir.rmdir(name).await,
        }
    }

    /// 创建硬链接
    pub async fn link(
        &self,
        target_name: &crate::types::OsStr,
        new_parent: &Self,
        new_name: &crate::types::OsStr,
    ) -> VfsResult<()> {
        match (self, new_parent) {
            (SDir::Ext4(old_p), SDir::Ext4(new_p)) => {
                old_p.link(target_name, new_p, new_name).await
            }
            (SDir::Dev(old_p), SDir::Dev(new_p)) => {
                old_p.link(target_name, new_p, new_name).await
            }
            _ => Err(KernelError::with_message(Errno::EINVAL, "link").into()),
        }
    }
}


// DirHandle
impl SDirHandle {
    pub fn vnode(&self) -> SDir {
        match self {
            SDirHandle::Ext4(h) => SDir::Ext4(h.vnode().clone()),
            SDirHandle::Dev(h) => SDir::Dev(h.vnode().clone()),
        }
    }

    pub async fn read_dir_chunk(&self, len: Option<usize>) -> VfsResult<&[DirectoryEntry]> {
        match self {
            SDirHandle::Ext4(h) => h.read_dir_chunk(len).await,
            SDirHandle::Dev(h) => h.read_dir_chunk(len).await,
        }
    }

    pub async fn seek_dir(&self, offset: u64) -> VfsResult<()> {
        match self {
            SDirHandle::Ext4(h) => h.seek_dir(offset).await,
            SDirHandle::Dev(h) => h.seek_dir(offset).await,
        }
    }

    pub async fn close(&self) -> VfsResult<()> {
        match self {
            SDirHandle::Ext4(h) => h.close().await,
            SDirHandle::Dev(h) => h.close().await,
        }
    }
}