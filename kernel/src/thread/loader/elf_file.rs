//! ext4 上的文件读取器；封装 vnode→handle→bytes 流程。

use crate::error::Result;
use alloc::{sync::Arc, vec::Vec, vec};
use vfs::{get_path_resolver, FileOpenBuilder, PathBuf, SFileHandle};

pub struct ExtFile {
    vnode_handle: Arc<SFileHandle>,
    len: usize,
}

impl ExtFile {
    pub async fn open(abs_path: &str) -> Result<(PathBuf, Self)> {
        debug_assert!(abs_path.starts_with('/'));
        let mut abs_path = PathBuf::new(abs_path)?;
        let vnode = get_path_resolver().resolve(&mut abs_path).await?;
        let len   = vnode.metadata().await?.size;
        let handle = vnode.as_file().unwrap().clone().open(FileOpenBuilder::new().read_only().build().unwrap()).await?;
        Ok((abs_path, Self { vnode_handle: Arc::new(handle), len: len as usize }))
    }

    pub(super) async fn read_all(&self) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; self.len];
        self.vnode_handle.read_at(0, &mut buf).await?;
        Ok(buf)
    }
}
