//! 将 VirtIOBlk 封装为 VFS 与 another_ext4 都能使用的块设备。

use alloc::{boxed::Box, sync::Arc};
use core::sync::atomic::{AtomicU64, Ordering};
use async_trait::async_trait;
use ostd::{sync::Mutex, task::scheduler::blocking_future::BlockingFuture};
use virtio_drivers::device::blk::VirtIOBlk;
use crate::{AsyncBlockDevice, VfsResult};

/// 4 KiB 逻辑块大小，与 ext4 superblock 缺省一致。
const LOGICAL_BLOCK_SIZE: u32 = 4096;
const SECTOR_SIZE: usize = virtio_drivers::device::blk::SECTOR_SIZE;

/// 包装结构
pub struct VirtioBlockDevice<H, T>
where
    H: virtio_drivers::Hal + Send + Sync,
    T: virtio_drivers::transport::Transport + Send + Sync,
{
    inner: Arc<Mutex<VirtIOBlk<H, T>>>,
    id: u64,
}

impl<H, T> VirtioBlockDevice<H, T>
where
    H: virtio_drivers::Hal + Send + Sync,
    T: virtio_drivers::transport::Transport + Send + Sync,
{
    pub fn new(inner: Arc<Mutex<VirtIOBlk<H, T>>>) -> Self {
        // 简单生成一个设备 id
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            inner,
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// 将逻辑块号转换为扇区号（512B）
    #[inline]
    fn blk2sec(blk: u64) -> usize {
        (blk * LOGICAL_BLOCK_SIZE as u64 / SECTOR_SIZE as u64) as usize
    }
}

/* ---------- 实现 another_ext4::BlockDevice ---------- */

// impl<H, T> BlockDevice for VirtioBlockDevice<H, T>
// where
//     H: virtio_drivers::Hal + Send + Sync + 'static,
//     T: virtio_drivers::transport::Transport + Send + Sync + 'static,
// {
//     fn read_block(&self, block_id: u64) -> Block {
//         let mut buf = [0u8; SECTOR_SIZE];
//         self.inner
//             .lock()
//             .block()
//             .read_blocks(block_id, &mut buf)
//             .unwrap();
//         Block::new(block_id, &buf)
//     }

//     fn write_block(&self, block: &Block) {
//         self.inner
//             .lock()
//             .block()
//             .write_blocks(block.id, &block.data)
//             .unwrap();
//     }
// }


/* ---------- 实现 VFS::AsyncBlockDevice ---------- */

#[async_trait]
impl<
    H: virtio_drivers::Hal + Send + Sync + 'static,
    T: virtio_drivers::transport::Transport + Send + Sync + 'static,
> AsyncBlockDevice for VirtioBlockDevice<H, T>
{
    fn device_id(&self) -> u64 {
        self.id
    }

    fn block_size_bytes(&self) -> VfsResult<u32> {
        Ok(LOGICAL_BLOCK_SIZE)
    }

    fn total_blocks(&self) -> VfsResult<u64> {
        let sectors = self.inner.lock().block().capacity();
        let total = sectors * SECTOR_SIZE as u64 / LOGICAL_BLOCK_SIZE as u64;
        Ok(total)
    }

    async fn read_blocks(&self, start_block: u64, buf: &mut [u8]) -> VfsResult<()> {
        // debug!(start = start_block, len = buf.len(), "read_blocks");
        let sector = Self::blk2sec(start_block);
        self.inner.lock().await.read_blocks(sector, buf).unwrap();
        Ok(())
    }

    async fn write_blocks(&self, start_block: u64, buf: &[u8]) -> VfsResult<()> {
        // debug!(start = start_block, len = buf.len(), "write_blocks");
        let sector = Self::blk2sec(start_block);
        self.inner.lock().await.write_blocks(sector, buf).unwrap();
        Ok(())
    }

    async fn flush(&self) -> VfsResult<()> {
        self.inner.lock().await.flush().unwrap();
        Ok(())
    }
}

// #[async_trait]
// impl<
//     H: virtio_drivers::Hal + Send + Sync + 'static,
//     T: virtio_drivers::transport::Transport + Send + Sync + 'static,
// > AsyncBlockDevice for Arc<VirtioBlockDevice<H, T>>
// {
//     fn device_id(&self) -> u64 {
//         self.device_id()
//     }

//     fn block_size_bytes(&self) -> VfsResult<u32> {
//         self.block_size_bytes()
//     }

//     fn total_blocks(&self) -> VfsResult<u64> {
//         self.total_blocks()
//     }

//     async fn read_blocks(&self, start_block: u64, buf: &mut [u8]) -> VfsResult<()> {
//         self.read_blocks(start_block, buf).await
//     }

//     async fn write_blocks(&self, start_block: u64, buf: &[u8]) -> VfsResult<()> {
//         self.write_blocks(start_block, buf).await
//     }

//     async fn flush(&self) -> VfsResult<()> {
//         self.flush().await
//     }
// }