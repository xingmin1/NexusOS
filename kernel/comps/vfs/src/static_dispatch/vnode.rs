pub mod file;
pub mod dir;
pub mod symlink;

use crate::impls::ext4_fs::vnode::Ext4Vnode;
use crate::static_dispatch::vnode::dir::SDir;
use crate::static_dispatch::vnode::file::SFile;
use crate::static_dispatch::vnode::symlink::SSymlink;
use crate::types::{VnodeId, VnodeMetadataChanges};
use crate::{VfsResult, VnodeMetadata};
use alloc::sync::Arc;

/// 静态派发后的统一 Vnode 类型。
///
/// 目前仅支持 Ext4，后续可按需扩展。
#[derive(Clone)]
pub enum SVnode {
    File(SFile),
    Dir(SDir),
    Symlink(SSymlink),
}

/* ---------- 顶层 SVnode 接口 ---------- */

impl SVnode {
    /// 获取唯一 ID
    pub fn id(&self) -> VnodeId {
        match self {
            SVnode::File(f) => f.id(),
            SVnode::Dir(d) => d.id(),
            SVnode::Symlink(s) => s.id(),
        }
    }

    /// 获取元数据
    pub async fn metadata(&self) -> VfsResult<VnodeMetadata> {
        match self {
            SVnode::File(f) => f.metadata().await,
            SVnode::Dir(d) => d.metadata().await,
            SVnode::Symlink(s) => s.metadata().await,
        }
    }

    /// 设置元数据
    pub async fn set_metadata(&self, ch: VnodeMetadataChanges) -> VfsResult<()> {
        match self {
            SVnode::File(f) => f.set_metadata(ch).await,
            SVnode::Dir(d) => d.set_metadata(ch).await,
            SVnode::Symlink(s) => s.set_metadata(ch).await,
        }
    }

    pub fn to_file(&self) -> Option<&SFile> {
        match self {
            SVnode::File(f) => Some(f),
            _ => None,
        }
    }

    pub fn to_dir(&self) -> Option<&SDir> {
        match self {
            SVnode::Dir(d) => Some(d),
            _ => None,
        }
    }

    pub fn to_symlink(&self) -> Option<&SSymlink> {
        match self {
            SVnode::Symlink(s) => Some(s),
            _ => None,
        }
    }
}

/* ---------- From / Into 转换 ---------- */

impl From<SFile> for SVnode { fn from(v: SFile) -> Self { SVnode::File(v) } }
impl From<SDir> for SVnode { fn from(v: SDir) -> Self { SVnode::Dir(v) } }
impl From<SSymlink> for SVnode { fn from(v: SSymlink) -> Self { SVnode::Symlink(v) } }

impl From<Arc<Ext4Vnode>> for SVnode {
    fn from(v: Arc<Ext4Vnode>) -> Self {
        match v.kind() {
            crate::types::VnodeType::Directory => SVnode::Dir(SDir::Ext4(v)),
            crate::types::VnodeType::SymbolicLink => SVnode::Symlink(SSymlink::Ext4(v)),
            _ => SVnode::File(SFile::Ext4(v)),
        }
    }
}