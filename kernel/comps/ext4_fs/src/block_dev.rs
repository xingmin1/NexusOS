//! 将 VirtIOBlk 封装为 VFS 与 another_ext4 都能使用的块设备。

use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use ostd::sync::Mutex;
use async_trait::async_trait;
use tracing::debug;

use vfs::{AsyncBlockDevice, VfsResult};
use another_ext4::BlockDevice;
use virtio_drivers::device::blk::VirtIOBlk;

/// 4 KiB 逻辑块大小，与 ext4 superblock 缺省一致。
const LOGICAL_BLOCK_SIZE: u32 = 4096;
const SECTOR_SIZE: usize = virtio_drivers::device::blk::SECTOR_SIZE;

/// 包装结构
pub struct VirtioBlockDevice<H, T> {
    inner: Arc<Mutex<VirtIOBlk<H, T>>>,
    id:    u64,
}

impl<H, T> VirtioBlockDevice<H, T> {
    pub fn new(inner: Arc<Mutex<VirtIOBlk<H, T>>>) -> Self {
        // 简单生成一个设备 id
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self { inner, id: NEXT_ID.fetch_add(1, Ordering::Relaxed) }
    }

    /// 将逻辑块号转换为扇区号（512B）
    #[inline]
    fn blk2sec(blk: u64) -> usize {
        (blk * LOGICAL_BLOCK_SIZE as u64 / SECTOR_SIZE as u64) as usize
    }
}

/* ---------- 实现 another_ext4::BlockDevice ---------- */

impl<H: virtio_drivers::Hal, T: virtio_drivers::transport::Transport> Ext4BlockDev
    for VirtioBlockDevice<H, T>
{
    fn read_offset(&self, offset: usize) -> Vec<u8> {
        let sector = offset / SECTOR_SIZE;
        let mut buf = alloc::vec![0u8; SECTOR_SIZE];
        self.inner.lock().read_blocks(sector, &mut buf).unwrap();
        buf
    }

    fn write_offset(&self, offset: usize, data: &[u8]) {
        let sector = offset / SECTOR_SIZE;
        self.inner.lock().write_blocks(sector, data).unwrap();
    }
}

/* ---------- 实现 VFS::AsyncBlockDevice ---------- */

#[async_trait]
impl<H: virtio_drivers::Hal + Send + Sync, T: virtio_drivers::transport::Transport + Send + Sync>
    AsyncBlockDevice for VirtioBlockDevice<H, T>
{
    fn device_id(&self) -> u64 { self.id }

    fn block_size_bytes(&self) -> VfsResult<u32> { Ok(LOGICAL_BLOCK_SIZE) }

    fn total_blocks(&self) -> VfsResult<u64> {
        let sectors = self.inner.lock().capacity();
        let total = sectors * SECTOR_SIZE as u64 / LOGICAL_BLOCK_SIZE as u64;
        Ok(total)
    }

    async fn read_blocks(&self, start_block: u64, buf: &mut [u8]) -> VfsResult<()> {
        debug!(start = start_block, len = buf.len(), "read_blocks");
        let sector = Self::blk2sec(start_block);
        self.inner.lock().read_blocks(sector, buf).unwrap();
        Ok(())
    }

    async fn write_blocks(&self, start_block: u64, buf: &[u8]) -> VfsResult<()> {
        debug!(start = start_block, len = buf.len(), "write_blocks");
        let sector = Self::blk2sec(start_block);
        self.inner.lock().write_blocks(sector, buf).unwrap();
        Ok(())
    }

    async fn flush(&self) -> VfsResult<()> {
        self.inner.lock().flush().unwrap();
        Ok(())
    }
}
