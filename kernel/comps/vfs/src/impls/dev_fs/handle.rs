use alloc::sync::Arc;
use alloc::vec::Vec;
use ostd::sync::Mutex;
use crate::{DirHandle, DirectoryEntry, FileHandle, FileOpen, VfsResult};
use crate::impls::dev_fs::vnode::DevVnode;
use crate::types::SeekFrom;

pub struct DevDirHandle {
    pub(super) vnode: Arc<DevVnode>,
    pub(super) cursor: Mutex<usize>,
    pub(super) snapshot: Vec<DirectoryEntry>,
}

impl DirHandle for DevDirHandle {
    type Vnode = DevVnode;

    fn vnode(&self) -> &Arc<Self::Vnode> { &self.vnode }

    async fn read_dir_chunk(&self, len: Option<usize>) -> VfsResult<&[DirectoryEntry]> {
        let l = len.unwrap_or(self.snapshot.len());
        let mut cur = self.cursor.lock().await;
        let start = *cur;
        let end = usize::min(start + l, self.snapshot.len());
        *cur = end;
        Ok(&self.snapshot[start..end])
    }

    async fn seek_dir(&self, offset: u64) -> VfsResult<()> {
        let mut cur = self.cursor.lock().await;
        *cur = offset as usize;
        Ok(())
    }

    async fn close(&self) -> VfsResult<()> { Ok(()) }
}

pub struct DevCharHandle {
    pub(super) vnode: Arc<DevVnode>,
    pub(super) flags: FileOpen,
}
impl FileHandle for DevCharHandle {
    type Vnode = DevVnode;

    fn flags(&self) -> FileOpen { self.flags }
    fn vnode(&self) -> &Arc<Self::Vnode> { &self.vnode }

    async fn read_at(&self, off: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let dev = self.vnode.char_data().unwrap().dev.clone();
        dev.read(off, buf).await
    }
    async fn write_at(&self, off: u64, buf: &[u8]) -> VfsResult<usize> {
        let dev = self.vnode.char_data().unwrap().dev.clone();
        dev.write(off, buf).await
    }

    async fn read_vectored_at(
        &self,
        off: u64,
        bufs: &mut [&mut [u8]],
    ) -> VfsResult<usize> {
        let mut total = 0;
        for b in bufs {
            total += self.read_at(off + total as u64, *b).await?;
        }
        Ok(total)
    }
    async fn write_vectored_at(&self, off: u64, bufs: &[&[u8]]) -> VfsResult<usize> {
        let mut total = 0;
        for b in bufs {
            total += self.write_at(off + total as u64, *b).await?;
        }
        Ok(total)
    }

    async fn seek(&self, _pos: SeekFrom) -> VfsResult<u64> {
        Err(crate::vfs_err_unsupported!("seek on char device"))
    }
    async fn flush(&self) -> VfsResult<()> { Ok(()) }
    async fn close(&self) -> VfsResult<()> { Ok(()) }
}