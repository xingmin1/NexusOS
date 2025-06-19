//! 将上述文件系统注册为 VFS Provider

use alloc::sync::Arc;
use another_ext4::{Block, BlockDevice};
use ostd::{drivers::virtio::block::get_block_device, sync::Mutex, task::scheduler::blocking_future::BlockingFuture};
use tracing::info;
use crate::impls::ext4_fs::filesystem::Ext4Fs;
use virtio_drivers::device::blk::VirtIOBlk;
use crate::{AsyncBlockDevice, FileSystemProvider, FsOptions, VfsResult};
use crate::impls::ext4_fs::block_dev::VirtioBlockDevice;
use crate::types::{FilesystemId, MountId};

pub struct Ext4Provider;

pub fn get_ext4_provider() -> Arc<impl FileSystemProvider> {
    Arc::new(Ext4Provider)
}

impl FileSystemProvider for Ext4Provider {
    type FS = Ext4Fs;

    fn fs_type_name(&self) -> &'static str {
        "ext4"
    }

    async fn mount(
        &self,
        source_device: Option<Arc<dyn AsyncBlockDevice + Send + Sync>>,
        options: &FsOptions,
        mount_id: MountId,
        fs_id: FilesystemId,
    ) -> VfsResult<Arc<Self::FS>> {
        /// 本地 new-type，内部保存任意异步块设备对象
        pub struct Adapt<D: ?Sized + AsyncBlockDevice + Send + Sync>(pub Arc<D>);

        impl<D> BlockDevice for Adapt<D>
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

        impl<H, T> BlockDevice for VirtioBlockDevice<H, T>
        where
            H: virtio_drivers::Hal + Send + Sync + 'static,
            T: virtio_drivers::transport::Transport + Send + Sync + 'static,
        {
            fn read_block(&self, block_id: u64) -> Block {
                let mut block = Block::default();
                self.read_blocks(block_id, &mut block.data).block().unwrap();
                block.id = block_id;
                block
            }
        
            fn write_block(&self, block: &Block) {
                self.write_blocks(block.id, &block.data).block().unwrap();
            }
        }

        // 若 VFS 层未传入块设备，从 ostd 设备树中获取名为 "block_device"
        let blk: Arc<dyn BlockDevice> = if let Some(dev) = source_device {
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
