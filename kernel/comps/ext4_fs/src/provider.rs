//! 将上述文件系统注册为 VFS Provider

use alloc::sync::Arc;
use async_trait::async_trait;
use alloc::boxed::Box;
use tracing::info;

use vfs::{types::{FilesystemId, MountId}, AsyncBlockDevice, AsyncFileSystem, AsyncFileSystemProvider, FsOptions, VfsResult};
use ostd::sync::Mutex;

use virtio_drivers::device::blk::VirtIOBlk;
use ostd::drivers::virtio::block::get_block_device;

use crate::{block_dev::VirtioBlockDevice, filesystem::Ext4Fs};

pub struct Ext4Provider;

pub fn get_ext4_provider() -> Arc<dyn AsyncFileSystemProvider + Send + Sync> {
    Arc::new(Ext4Provider)
}

#[async_trait]
impl AsyncFileSystemProvider for Ext4Provider {
    fn fs_type_name(&self) -> &'static str { "ext4" }

    async fn mount(
        &self,
        source_device: Option<Arc<dyn AsyncBlockDevice + Send + Sync>>,
        options: &FsOptions,
        mount_id: MountId,
        fs_id: FilesystemId,
    ) -> VfsResult<Arc<dyn AsyncFileSystem + Send + Sync>> {
        // 若 VFS 层未传入块设备，从 ostd 设备树中获取名为 "block_device"
        let blk = if let Some(dev) = source_device {
            dev
        } else {
            let vblk: Arc<Mutex<VirtIOBlk<_, _>>> =
                get_block_device("block_device").expect("No virtio block!");
            Arc::new(VirtioBlockDevice::new(vblk))
        };

        info!("Mounting ext4 with readonly = {}", options.read_only);
        Ok(Arc::new(Ext4Fs::new(
            mount_id as u64,
            fs_id as u64,
            options.clone(),
            blk,
        )))
    }
}
