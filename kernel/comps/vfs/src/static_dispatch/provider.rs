
/* ---------- FileSystemProvider 静态派发 ---------- */
use alloc::sync::Arc;
use crate::{AsyncBlockDevice, FileSystemProvider, FsOptions, VfsResult};
use crate::impls::ext4_fs::provider::Ext4Provider;
use crate::static_dispatch::filesystem::SFileSystem;
use crate::types::{FilesystemId, MountId};

#[derive(Clone)]
pub enum SProvider {
    Ext4(Arc<Ext4Provider>),
}

impl SProvider {
    pub fn fs_type_name(&self) -> &'static str {
        match self {
            SProvider::Ext4(p) => p.fs_type_name(),
        }
    }

    pub async fn mount(
        &self,
        dev: Option<Arc<dyn AsyncBlockDevice + Send + Sync>>,
        opts: &FsOptions,
        mount_id: MountId,
        fs_id: FilesystemId,
    ) -> VfsResult<SFileSystem> {
        match self {
            SProvider::Ext4(p) => {
                let fs = p.mount(dev, opts, mount_id, fs_id).await?;
                Ok(SFileSystem::Ext4(fs))
            }
        }
    }
}

impl From<Arc<Ext4Provider>> for SProvider {
    fn from(p: Arc<Ext4Provider>) -> Self {
        SProvider::Ext4(p)
    }
}