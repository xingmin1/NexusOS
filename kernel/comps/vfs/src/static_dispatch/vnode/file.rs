use alloc::sync::Arc;
use crate::impls::dev_fs::{DevCharHandle, DevVnode};
use crate::impls::ext4_fs::vnode::{Ext4FileHandle, Ext4Vnode};
use crate::types::{SeekFrom, VnodeId, VnodeMetadataChanges};
use crate::{FileHandle, FileOpen, VfsResult, Vnode, VnodeMetadata};
use crate::traits::FileCap;

#[derive(Clone)]
pub enum SFile {
    Ext4(Arc<Ext4Vnode>),
    Dev(Arc<DevVnode>),
}

#[derive(Clone)]
pub enum SFileHandle {
    Ext4(Arc<Ext4FileHandle>),
    Dev(Arc<DevCharHandle>),
}

/// Vnode —— 最小公共能力
impl SFile {
    pub fn id(&self) -> VnodeId {
        match self {
            SFile::Ext4(v) => v.id(),
            SFile::Dev(v) => v.id(),
        }
    }

    pub async fn metadata(&self) -> VfsResult<VnodeMetadata> {
        match self {
            SFile::Ext4(v) => v.metadata().await,
            SFile::Dev(v) => v.metadata().await,
        }
    }

    pub async fn set_metadata(&self, ch: VnodeMetadataChanges) -> VfsResult<()> {
        match self {
            SFile::Ext4(v) => v.set_metadata(ch).await,
            SFile::Dev(v) => v.set_metadata(ch).await,
        }
    }
}

// FileCap
impl SFile {
    pub async fn open(self, flags: FileOpen) -> VfsResult<SFileHandle> {
        match self {
            SFile::Ext4(v) => {
                let handle = v.open(flags).await?;
                Ok(SFileHandle::Ext4(handle))
            }
            SFile::Dev(v) => {
                let handle = v.open(flags).await?;
                Ok(SFileHandle::Dev(handle))
            }
        }
    }
}

// FileHandle
impl SFileHandle {
    pub fn flags(&self) -> FileOpen {
        match self {
            SFileHandle::Ext4(h) => h.flags(),
            SFileHandle::Dev(h) => h.flags(),
        }
    }
    pub fn vnode(&self) -> SFile {
        match self {
            SFileHandle::Ext4(h) => SFile::Ext4(h.vnode().clone()),
            SFileHandle::Dev(h) => SFile::Dev(h.vnode().clone()),
        }
    }

    pub async fn read_at(&self, off: u64, buf: &mut [u8]) -> VfsResult<usize> {
        match self {
            SFileHandle::Ext4(h) => h.read_at(off, buf).await,
            SFileHandle::Dev(h) => h.read_at(off, buf).await,
        }
    }
    pub async fn write_at(&self, off: u64, buf: &[u8]) -> VfsResult<usize> {
        match self {
            SFileHandle::Ext4(h) => h.write_at(off, buf).await,
            SFileHandle::Dev(h) => h.write_at(off, buf).await,
        }
    }

    /// Scatter / Gather I/O —— 参见 readv/writev 设计
    pub async fn read_vectored_at(&self, off: u64, bufs: &mut [&mut [u8]])
                              -> VfsResult<usize> {
        match self {
            SFileHandle::Ext4(h) => h.read_vectored_at(off, bufs).await,
            SFileHandle::Dev(h) => h.read_vectored_at(off, bufs).await,
        }
    }
    pub async fn write_vectored_at(&self, off: u64, bufs: &[&[u8]])
                               -> VfsResult<usize> {
        match self {
            SFileHandle::Ext4(h) => h.write_vectored_at(off, bufs).await,
            SFileHandle::Dev(h) => h.write_vectored_at(off, bufs).await,
        }
    }

    pub async fn seek(&self, pos: SeekFrom) -> VfsResult<u64> {
        match self {
            SFileHandle::Ext4(h) => h.seek(pos).await,
            SFileHandle::Dev(h) => h.seek(pos).await,
        }
    }
    pub async fn flush(&self) -> VfsResult<()> {
        match self {
            SFileHandle::Ext4(h) => h.flush().await,
            SFileHandle::Dev(h) => h.flush().await,
        }
    }
    pub async fn close(&self) -> VfsResult<()> {
        match self {
            SFileHandle::Ext4(h) => h.close().await,
            SFileHandle::Dev(h) => h.close().await,
        }
    }
}