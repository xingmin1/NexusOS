use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use nexus_error::Errno;
use ostd::sync::{Mutex, RwLock};
use crate::impls::dev_fs::driver::AsyncCharDevice;
use crate::impls::dev_fs::filesystem::DevFs;
use crate::types::{FileMode, VnodeId, VnodeMetadataChanges};
use crate::{DirectoryEntry, FileOpen, VfsResult, Vnode, VnodeMetadata, VnodeType};
use crate::impls::dev_fs::handle::{DevCharHandle, DevDirHandle};
use crate::traits::{DirCap, FileCap, VnodeCapability};
use crate::verror::KernelError;

pub enum DevVnode {
    Directory(DirData),
    CharDevice(CharData),
}

impl DevVnode {
    fn id(&self) -> VnodeId {
        match self {
            Self::Directory(d) => d.id,
            Self::CharDevice(c) => c.id,
        }
    }
    pub(super) fn dir_data(&self) -> Option<&DirData> {
        match self {
            Self::Directory(d) => Some(d),
            _ => None,
        }
    }
    pub(super) fn char_data(&self) -> Option<&CharData> {
        match self {
            Self::CharDevice(c) => Some(c),
            _ => None,
        }
    }
}

pub struct DirData {
    pub(super) fs: Weak<DevFs>,
    pub(super) id: VnodeId,
    pub(super) children: RwLock<BTreeMap<String, Arc<DevVnode>>>,
    pub(super) metadata: RwLock<VnodeMetadata>,
}

pub struct CharData {
    pub(super) fs: Weak<DevFs>,
    pub(super) id: VnodeId,
    pub(super) dev: Arc<dyn AsyncCharDevice>,
    pub(super) metadata: RwLock<VnodeMetadata>,
}

impl Vnode for DevVnode {
    type FS = DevFs;

    fn id(&self) -> VnodeId { self.id() }
    fn filesystem(&self) -> Arc<Self::FS> {
        match self {
            Self::Directory(d) => d.fs.upgrade().unwrap(),
            Self::CharDevice(c) => c.fs.upgrade().unwrap(),
        }
    }

    async fn metadata(&self) -> VfsResult<VnodeMetadata> {
        match self {
            Self::Directory(d) => Ok(d.metadata.read().await.clone()),
            Self::CharDevice(c) => Ok(c.metadata.read().await.clone()),
        }
    }

    async fn set_metadata(&self, ch: VnodeMetadataChanges) -> VfsResult<()> {
        match self {
            Self::Directory(d) => d.apply_meta(ch).await,
            Self::CharDevice(c) => c.apply_meta(ch).await,
        }
    }

    fn cap_type(&self) -> VnodeType {
        match self {
            Self::Directory(_) => VnodeType::Directory,
            Self::CharDevice(_) => VnodeType::CharDevice,
        }
    }
}

impl DirData {
    async fn apply_meta(&self, ch: VnodeMetadataChanges) -> VfsResult<()> {
        let mut md = self.metadata.write().await;
        if let Some(perm) = ch.permissions { md.permissions = perm; }
        Ok(())
    }
}

impl VnodeCapability for DevVnode {
    type FS = DevFs;
    type Vnode = Self;
}

impl DirCap for DevVnode {
    type DirHandle = DevDirHandle;

    async fn open_dir(self: Arc<Self>) -> VfsResult<Arc<Self::DirHandle>> {
        let list = {
            let data = self.dir_data().ok_or_else(|| KernelError::new(Errno::ENOTDIR))?;
            let guard = data.children.read().await;
            guard
                .iter()
                .map(|(n, v)| DirectoryEntry {
                    name: n.clone(),
                    vnode_id: v.id(),
                    kind: v.cap_type(),
                })
                .collect()
        };
        Ok(Arc::new(DevDirHandle {
            vnode: self,
            cursor: Mutex::new(0),
            snapshot: list,
        }))
    }

    async fn lookup(&self, name: &crate::types::OsStr) -> VfsResult<Arc<Self>> {
        let data = self.dir_data().ok_or_else(|| KernelError::new(Errno::ENOTDIR))?;
        let guard = data.children.read().await;
        guard
            .get(name)
            .cloned()
            .ok_or_else(|| KernelError::with_message(Errno::ENOENT, "lookup").into())
    }

    async fn create(
        &self,
        _name: &crate::types::OsStr,
        _kind: VnodeType,
        _perm: FileMode,
        _rdev: Option<u64>,
    ) -> VfsResult<Arc<Self>> {
        Err(crate::vfs_err_unsupported!("devfs create"))
    }

    async fn rename(
        &self,
        _old_name: &crate::types::OsStr,
        _new_parent: &Self,
        _new_name: &crate::types::OsStr,
    ) -> VfsResult<()> {
        Err(crate::vfs_err_unsupported!("devfs rename"))
    }

    async fn unlink(&self, _name: &crate::types::OsStr) -> VfsResult<()> {
        Err(crate::vfs_err_unsupported!("devfs unlink"))
    }

    async fn rmdir(&self, _name: &crate::types::OsStr) -> VfsResult<()> {
        Err(crate::vfs_err_unsupported!("devfs rmdir"))
    }

    async fn link(
        &self,
        _target_name: &crate::types::OsStr,
        _new_parent: &Self,
        _new_name: &crate::types::OsStr,
    ) -> VfsResult<()> {
        Err(crate::vfs_err_unsupported!("devfs link"))
    }
}

impl CharData {
    async fn apply_meta(&self, ch: VnodeMetadataChanges) -> VfsResult<()> {
        let mut md = self.metadata.write().await;
        if let Some(perm) = ch.permissions { md.permissions = perm; }
        Ok(())
    }
}

impl FileCap for DevVnode {
    type Handle = DevCharHandle;

    async fn open(self: Arc<Self>, flags: FileOpen) -> VfsResult<Arc<Self::Handle>> {
        if self.char_data().is_none() {
            return Err(crate::vfs_err_not_dir!("not char device"));
        }
        Ok(Arc::new(DevCharHandle { vnode: self, flags }))
    }
}
