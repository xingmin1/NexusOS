use alloc::sync::Arc;
use ostd::sync::spin::Lazy;
use crate::{FileSystemProvider, FsOptions, VfsResult};
use crate::impls::dev_fs::filesystem::DevFs;
use crate::types::{FilesystemId, MountId};

pub struct DevFsProvider;

static PROVIDER: Lazy<Arc<DevFsProvider>> = Lazy::new(|| Arc::new(DevFsProvider));
pub fn get_devfs_provider() -> Arc<DevFsProvider> {
    PROVIDER.get().clone()
}

impl FileSystemProvider for DevFsProvider {
    type FS = DevFs;

    fn fs_type_name(&self) -> &'static str { "devfs" }

    async fn mount(
        &self,
        _dev: Option<Arc<dyn crate::AsyncBlockDevice + Send + Sync>>,
        _opts: &FsOptions,
        mount_id: MountId,
        fs_id: FilesystemId,
    ) -> VfsResult<Arc<Self::FS>> {
        Ok(DevFs::new(mount_id, fs_id))
    }
}
