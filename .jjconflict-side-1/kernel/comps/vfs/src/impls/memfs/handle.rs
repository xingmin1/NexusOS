use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use async_trait::async_trait;
use ostd::sync::RwLock;

use crate::{vfs_err_not_implemented, VfsResult};
use crate::{
    traits::{AsyncDirHandle, AsyncFileHandle, AsyncVnode},
    types::{DirectoryEntry, OpenFlags, SeekFrom},
};

use super::vnode::InMemoryVnode;

/// 内存文件系统中的文件句柄
#[derive(Debug)]
pub struct InMemoryFileHandle {
    /// 指向关联的 Vnode
    vnode: Arc<InMemoryVnode>,
    /// 打开标志
    flags: OpenFlags,
    /// 当前文件指针位置，需要Arc<RwLock<>>允许多个句柄共享和修改
    pos: Arc<RwLock<u64>>,
}

impl InMemoryFileHandle {
    /// 创建新的文件句柄
    pub fn new(vnode: Arc<InMemoryVnode>, flags: OpenFlags) -> Self {
        Self {
            vnode,
            flags,
            pos: Arc::new(RwLock::new(0)),
        }
    }
}

#[async_trait]
impl AsyncFileHandle for InMemoryFileHandle {
    fn flags(&self) -> OpenFlags {
        self.flags
    }

    async fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        if let Some(lock) = self.vnode.file_content_lock() {
            let content = lock.read().await;
            let start = core::cmp::min(offset as usize, content.len());
            let end = core::cmp::min(start + buf.len(), content.len());
            let slice = &content[start..end];
            buf[..slice.len()].copy_from_slice(slice);
            // 更新文件指针
            let mut pos = self.pos.write().await;
            *pos = (offset as usize + slice.len()) as u64;
            Ok(slice.len())
        } else {
            Err(crate::vfs_err_invalid_argument!("read_at on non-file vnode"))
        }
    }

    async fn write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        if let Some(lock) = self.vnode.file_content_lock() {
            let mut content = lock.write().await;
            let start = offset as usize;
            if content.len() < start {
                content.resize(start, 0);
            }
            if content.len() < start + buf.len() {
                content.resize(start + buf.len(), 0);
            }
            content[start..start + buf.len()].copy_from_slice(buf);
            // 更新元数据大小
            {
                let mut meta = self.vnode.metadata_lock().write().await;
                meta.size = content.len() as u64;
            }
            // 更新文件指针
            let mut pos = self.pos.write().await;
            *pos = (offset as usize + buf.len()) as u64;
            Ok(buf.len())
        } else {
            Err(crate::vfs_err_invalid_argument!("write_at on non-file vnode"))
        }
    }

    async fn seek(self: Arc<Self>, pos: SeekFrom) -> VfsResult<u64> {
        let new_pos = match pos {
            SeekFrom::Start(off) => off as u64,
            SeekFrom::Current(delta) => {
                let cur = *self.pos.read().await;
                if delta >= 0 {
                    cur + delta as u64
                } else {
                    cur.checked_sub((-delta) as u64).ok_or_else(|| crate::vfs_err_invalid_argument!("seek before start"))?
                }
            }
            SeekFrom::End(delta) => {
                let size = self.vnode.metadata_lock().read().await.size as i64;
                let target = size + delta as i64;
                if target < 0 {
                    return Err(crate::vfs_err_invalid_argument!("seek before start"));
                }
                target as u64
            }
        };
        *self.pos.write().await = new_pos;
        Ok(new_pos)
    }

    async fn flush(&self) -> VfsResult<()> {
        // 内存文件系统，flush 通常是无操作
        Ok(())
    }

    fn vnode(&self) -> Arc<dyn AsyncVnode + Send + Sync> {
        self.vnode.clone() as Arc<dyn AsyncVnode + Send + Sync>
    }

    async fn close(&self) -> VfsResult<()> {
        // 内存文件句柄关闭操作，基本为无操作
        Ok(())
    }
}

/// 内存文件系统中的目录句柄
#[derive(Debug)]
pub struct InMemoryDirHandle {
    /// 指向关联的目录 Vnode
    vnode: Arc<InMemoryVnode>,
    /// 缓存的目录项列表
    entries: Arc<RwLock<Vec<DirectoryEntry>>>,
    /// 当前读取到的目录项索引，用 AtomicU64 因为 readdir 可能被并发调用
    current_idx: AtomicU64,
}

impl InMemoryDirHandle {
    /// 创建新的目录句柄
    pub fn new(vnode: Arc<InMemoryVnode>, entries: Vec<DirectoryEntry>) -> Self {
        Self {
            vnode,
            entries: Arc::new(RwLock::new(entries)),
            current_idx: AtomicU64::new(0),
        }
    }
}

#[async_trait]
impl AsyncDirHandle for InMemoryDirHandle {
    fn flags(&self) -> OpenFlags {
        // 目录的默认打开标志
        OpenFlags::empty()
    }

    async fn readdir(self: Arc<Self>) -> VfsResult<Option<DirectoryEntry>> {
        let idx = self.current_idx.fetch_add(1, Ordering::SeqCst) as usize;
        let guard = self.entries.read().await;
        if idx >= guard.len() {
            Ok(None)
        } else {
            Ok(Some(guard[idx].clone()))
        }
    }

    async fn seek_dir(self: Arc<Self>, offset: u64) -> VfsResult<()> {
        self.current_idx.store(offset, Ordering::SeqCst);
        Ok(())
    }

    async fn close(&self) -> VfsResult<()> {
        // 目录句柄关闭操作，基本为无操作
        Ok(())
    }

    fn vnode(&self) -> Arc<dyn AsyncVnode + Send + Sync> {
        self.vnode.clone() as Arc<dyn AsyncVnode + Send + Sync>
    }
}
