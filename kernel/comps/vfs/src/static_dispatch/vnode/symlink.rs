use alloc::sync::Arc;
use crate::impls::ext4_fs::vnode::Ext4Vnode;
use crate::types::{VnodeId, VnodeMetadataChanges};
use crate::{PathBuf, VfsResult, Vnode, VnodeMetadata};
use crate::traits::SymlinkCap;

#[derive(Clone)]
pub enum SSymlink {
    Ext4(Arc<Ext4Vnode>),
}

/// Symlink 基础能力
impl SSymlink {
    pub fn id(&self) -> VnodeId {
        match self {
            SSymlink::Ext4(v) => v.id(),
        }
    }

    pub async fn metadata(&self) -> VfsResult<VnodeMetadata> {
        match self {
            SSymlink::Ext4(v) => v.metadata().await,
        }
    }

    pub async fn set_metadata(&self, ch: VnodeMetadataChanges) -> VfsResult<()> {
        match self {
            SSymlink::Ext4(v) => v.set_metadata(ch).await,
        }
    }
}
// SymlinkCap
impl SSymlink {
    pub async fn readlink(&self) -> VfsResult<PathBuf> {
        match self {
            SSymlink::Ext4(v) => v.readlink().await,
        }
    }
}
