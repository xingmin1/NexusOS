//! 将上述文件系统注册为 VFS Provider

use alloc::{boxed::Box, sync::Arc};

use another_ext4::{Block, BlockDevice};
use async_trait::async_trait;
use ostd::{drivers::virtio::block::get_block_device, sync::Mutex, task::scheduler::blocking_future::BlockingFuture};
use tracing::info;
use vfs::{
    types::{FilesystemId, MountId}, AsyncBlockDevice, AsyncFileSystem, AsyncFileSystemProvider, FsOptions,
    VfsResult,
};
use virtio_drivers::device::blk::VirtIOBlk;

use crate::{block_dev::VirtioBlockDevice, filesystem::Ext4Fs};

pub struct Ext4Provider;

pub fn get_ext4_provider() -> Arc<dyn AsyncFileSystemProvider + Send + Sync> {
    Arc::new(Ext4Provider)
}

#[async_trait]
impl AsyncFileSystemProvider for Ext4Provider {
    fn fs_type_name(&self) -> &'static str {
        "ext4"
    }

    async fn mount(
        &self,
        source_device: Option<Arc<dyn AsyncBlockDevice + Send + Sync>>,
        options: &FsOptions,
        mount_id: MountId,
        fs_id: FilesystemId,
    ) -> VfsResult<Arc<dyn AsyncFileSystem + Send + Sync>> {
        /// 本地 new-type，内部保存任意异步块设备对象
        pub struct Adapt<D: ?Sized + AsyncBlockDevice + Send + Sync>(pub Arc<D>);

        impl<D> another_ext4::BlockDevice for Adapt<D>
        where
            D: ?Sized + AsyncBlockDevice + Send + Sync + 'static,
        {
            fn read_block(&self, block_id: u64) -> Block {
                let mut block = Block::default();
                self.0.read_blocks(block_id, &mut block.data).block().unwrap();
                block.id = block_id;
                block
            }

            fn write_block(&self, block: &Block) {
                self.0.write_blocks(block.id, &block.data).block().unwrap();
            }
        }

        // 若 VFS 层未传入块设备，从 ostd 设备树中获取名为 "block_device"
        let blk: Arc<dyn another_ext4::BlockDevice> = if let Some(dev) = source_device {
            Arc::new(Adapt(dev))
        } else {
            let vblk: Arc<Mutex<VirtIOBlk<_, _>>> = get_block_device("block_device")
                .await
                .expect("No virtio block!");
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
