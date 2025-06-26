use alloc::collections::BTreeMap;
use alloc::string::ToString;
use alloc::sync::Arc;
use ostd::sync::RwLock;
use crate::{FileSystem, FsOptions, VfsResult, VnodeMetadata, VnodeType};
use crate::impls::dev_fs::driver::AsyncCharDevice;
use crate::impls::dev_fs::vnode::{CharData, DevVnode, DirData};
use crate::types::{FileMode, FilesystemId, MountId, VnodeId};


pub struct DevFs {
    id: FilesystemId,
    mount_id: MountId,
    options: FsOptions,
    root: Arc<DevVnode>,
}

impl DevFs {
    pub(super) fn new(mount_id: MountId, fs_id: FilesystemId) -> Arc<Self> {
        let root_meta = VnodeMetadata {
            vnode_id: 1,
            fs_id,
            kind: VnodeType::Directory,
            size: 0,
            permissions: FileMode::OWNER_RWE | FileMode::GROUP_RWE | FileMode::OTHER_READ | FileMode::OTHER_WRITE,
            timestamps: crate::types::Timestamps::now(),
            uid: 0,
            gid: 0,
            nlinks: 1,
            rdev: None,
        };
        Arc::new_cyclic(|me| {
            let root = Arc::new(DevVnode::Directory(DirData {
                fs: me.clone(),
                id: 1,
                children: RwLock::new(BTreeMap::new()),
                metadata: RwLock::new(root_meta),
            }));
            DevFs {
                id: fs_id,
                mount_id,
                options: FsOptions::default(),
                root,
            }
        })
    }

    /// 注册字符设备到 /dev/<name>
    pub async fn register_char_device(
        self: &Arc<Self>,
        name: &str,
        dev: Arc<dyn AsyncCharDevice>,
        perm: FileMode,
    ) -> VfsResult<()> {
        let mut children = self.root.dir_data().unwrap().children.write().await;
        if children.contains_key(name) {
            return Err(crate::vfs_err_already_exists!(name));
        }

        let vnode_id = children.len() as VnodeId + 2;
        let meta = VnodeMetadata {
            vnode_id,
            fs_id: self.id,
            kind: VnodeType::CharDevice,
            size: 0,
            permissions: perm,
            timestamps: crate::types::Timestamps::now(),
            uid: 0,
            gid: 0,
            nlinks: 1,
            rdev: None,
        };
        let v = Arc::new(DevVnode::CharDevice(CharData {
            fs: Arc::downgrade(self),
            id: vnode_id,
            dev,
            metadata: RwLock::new(meta),
        }));
        children.insert(name.to_string(), v);
        Ok(())
    }
}


impl FileSystem for DevFs {
    type Vnode = DevVnode;

    fn id(&self) -> FilesystemId { self.id }
    fn mount_id(&self) -> MountId { self.mount_id }
    fn options(&self) -> &FsOptions { &self.options }

    async fn root_vnode(self: Arc<Self>) -> VfsResult<Arc<Self::Vnode>> {
        Ok(self.root.clone())
    }
    async fn statfs(&self) -> VfsResult<crate::types::FilesystemStats> {
        Ok(crate::types::FilesystemStats {
            fs_id: self.id,
            fs_type_name: "devfs".into(),
            block_size: 1,
            total_blocks: 0,
            free_blocks: 0,
            avail_blocks: 0,
            total_inodes: 0,
            free_inodes: 0,
            name_max_len: 255,
            optimal_io_size: None,
        })
    }
    async fn sync(&self) -> VfsResult<()> { Ok(()) }
    async fn prepare_unmount(&self) -> VfsResult<()> { Ok(()) }
    async fn reclaim_vnode(&self, _id: VnodeId) -> VfsResult<bool> { Ok(false) }
    fn fs_type_name(&self) -> &'static str { "devfs" }
    fn is_readonly(&self) -> bool { false }
}
