use alloc::boxed::Box;
use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::sync::{Arc, Weak};
use async_trait::async_trait;
use ostd::sync::RwLock;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::{types::{FileMode, Timestamps}, vfs_err_already_exists, vfs_err_invalid_argument, vfs_err_not_dir, vfs_err_not_implemented, FileSystem, VfsResult};
use crate::{
    path::{PathSlice, PathBuf},
    traits::Vnode,
    types::{
        OpenFlags, OsStr, VnodeId, VnodeMetadata, 
        VnodeMetadataChanges, VnodeType
    },
};

use super::fs::InMemoryFileSystem;

/// 内存文件系统中的节点类型数据
#[derive(Debug)]
pub enum InMemoryVnodeKindData {
    /// 文件，包含文件内容
    File(RwLock<Vec<u8>>),
    /// 目录，包含目录项映射表
    Directory(RwLock<alloc::collections::BTreeMap<alloc::string::String, Arc<InMemoryVnode>>>),
    /// 符号链接，包含目标路径
    SymbolicLink(PathBuf),
}

/// 内存文件系统中的节点
#[derive(Debug)]
pub struct InMemoryVnode {
    /// 指向所属文件系统的弱引用
    fs: Weak<InMemoryFileSystem>,
    /// 节点ID
    id: VnodeId,
    /// 节点类型数据
    kind: InMemoryVnodeKindData,
    /// 节点元数据
    metadata: RwLock<VnodeMetadata>,
}

/// 为 InMemory 文件系统生成唯一 vnode id（根节点占用 1）
static NEXT_VNODE_ID: AtomicU64 = AtomicU64::new(2);

impl InMemoryVnode {
    /// 创建新的内存文件系统节点
    pub fn new(
        fs: Weak<InMemoryFileSystem>,
        id: VnodeId,
        kind: InMemoryVnodeKindData,
        metadata: VnodeMetadata,
    ) -> Self {
        Self {
            fs,
            id,
            kind,
            metadata: RwLock::new(metadata),
        }
    }
    
    /// 创建新的文件节点
    pub fn new_file(
        fs: Weak<InMemoryFileSystem>,
        id: VnodeId,
        metadata: VnodeMetadata,
    ) -> Self {
        Self::new(
            fs,
            id,
            InMemoryVnodeKindData::File(RwLock::new(Vec::new())),
            metadata,
        )
    }
    
    /// 创建新的目录节点
    pub fn new_directory(
        fs: Weak<InMemoryFileSystem>,
        id: VnodeId,
        metadata: VnodeMetadata,
    ) -> Self {
        Self::new(
            fs,
            id,
            InMemoryVnodeKindData::Directory(RwLock::new(alloc::collections::BTreeMap::new())),
            metadata,
        )
    }
    
    /// 创建新的符号链接节点
    pub fn new_symlink(
        fs: Weak<InMemoryFileSystem>,
        id: VnodeId,
        target: PathBuf,
        metadata: VnodeMetadata,
    ) -> Self {
        Self::new(
            fs,
            id,
            InMemoryVnodeKindData::SymbolicLink(target),
            metadata,
        )
    }

    /// 分配新的 vnode id
    fn alloc_vnode_id() -> VnodeId {
        NEXT_VNODE_ID.fetch_add(1, Ordering::Relaxed)
    }

    /// 如果此节点是文件，返回其内容锁
    pub(crate) fn file_content_lock(&self) -> Option<&RwLock<Vec<u8>>> {
        match &self.kind {
            InMemoryVnodeKindData::File(lock) => Some(lock),
            _ => None,
        }
    }

    /// 公开元数据锁，供同 crate 使用
    pub(crate) fn metadata_lock(&self) -> &RwLock<VnodeMetadata> {
        &self.metadata
    }
}

#[async_trait]
impl Vnode for InMemoryVnode {
    fn id(&self) -> VnodeId {
        self.id
    }
    
    fn filesystem(&self) -> Arc<dyn crate::traits::FileSystem + Send + Sync> {
        self.fs.upgrade().unwrap()
    }
    
    async fn metadata(&self) -> VfsResult<VnodeMetadata> {
        Ok(self.metadata.read().await.clone())
    }
    
    async fn set_metadata(&self, changes: VnodeMetadataChanges) -> VfsResult<()> {
        let mut meta = self.metadata.write().await;
        if let Some(sz) = changes.size {
            meta.size = sz;
        }
        if let Some(perm) = changes.permissions {
            meta.permissions = perm;
        }
        if let Some(ts) = changes.timestamps {
            if let Some(atime) = ts.accessed {
                meta.timestamps.accessed = atime;
            }
            if let Some(mtime) = ts.modified {
                meta.timestamps.modified = mtime;
            }
        }
        if let Some(uid) = changes.uid {
            meta.uid = uid;
        }
        if let Some(gid) = changes.gid {
            meta.gid = gid;
        }
        Ok(())
    }
    
    async fn lookup(self: Arc<Self>, name: &OsStr) -> VfsResult<Arc<dyn Vnode + Send + Sync>> {
        let entry_name = name.as_str();
        match &self.kind {
            InMemoryVnodeKindData::Directory(entries) => {
                let guard = entries.read().await;
                guard.get(entry_name).cloned().ok_or_else(|| {
                    crate::vfs_err_not_found!(format!("Entry '{}' not found in dir {}", entry_name, self.id))
                }).map(|v| v as Arc<dyn Vnode + Send + Sync>)
            }
            _ => Err(crate::vfs_err_not_dir!(format!("Vnode {} is not directory", self.id))),
        }
    }
    
    async fn create_node(
        self: Arc<Self>,
        name: &OsStr,
        kind: VnodeType,
        permissions: FileMode,
        _rdev: Option<u64>,
    ) -> VfsResult<Arc<dyn Vnode + Send + Sync>> {
        let filename = name.as_str();
        let InMemoryVnodeKindData::Directory(entries) = &self.kind else {
            return Err(crate::vfs_err_not_dir!("create_node on non-directory"));
        };
        let mut guard = entries.write().await;
        if guard.contains_key(filename) {
            return Err(vfs_err_already_exists!(format!("{} already exists", filename)));
        }
        let fs = self.fs.upgrade().ok_or_else(|| crate::vfs_err_invalid_argument!("filesystem dropped"))?;
        let now = Timestamps::now();
        let new_meta = VnodeMetadata {
            vnode_id: Self::alloc_vnode_id(),
            fs_id: fs.id(),
            kind,
            size: 0,
            permissions,
            timestamps: now,
            uid: 0,
            gid: 0,
            nlinks: 1,
            rdev: None,
        };
        let new_node = match kind {
            VnodeType::File => Arc::new(InMemoryVnode::new_file(Arc::downgrade(&fs), new_meta.vnode_id, new_meta.clone())),
            VnodeType::Directory => Arc::new(InMemoryVnode::new_directory(Arc::downgrade(&fs), new_meta.vnode_id, new_meta.clone())),
            VnodeType::SymbolicLink => return Err(crate::vfs_err_not_implemented!("symlink create via create_node")),
            _ => return Err(crate::vfs_err_not_implemented!("unsupported vnode type in memfs create_node")),
        };
        guard.insert(filename.to_string(), new_node.clone());
        Ok(new_node as Arc<dyn Vnode + Send + Sync>)
    }
    
    async fn unlink(self: Arc<Self>, name: &OsStr) -> VfsResult<()> {
        let InMemoryVnodeKindData::Directory(entries) = &self.kind else {
            return Err(vfs_err_not_dir!("unlink on non-directory"));
        };
        let mut guard = entries.write().await;
        let filename = name.as_str();
        let Some(node) = guard.get(filename) else {
            return Err(crate::vfs_err_not_found!(format!("{} not found", filename)));
        };
        let meta = node.metadata().await?;
        if meta.kind == VnodeType::Directory {
            return Err(crate::vfs_err_is_dir!(filename));
        }
        guard.remove(filename);
        Ok(())
    }

    async fn rmdir(self: Arc<Self>, name: &OsStr) -> VfsResult<()> {
        let InMemoryVnodeKindData::Directory(entries) = &self.kind else {
            return Err(vfs_err_not_dir!("rmdir on non-directory"));
        };
        let mut guard = entries.write().await;
        let dirname = name.as_str();
        let Some(node) = guard.get(dirname) else {
            return Err(crate::vfs_err_not_found!(format!("{} not found", dirname)));
        };
        let node_meta = node.metadata().await?;
        if node_meta.kind != VnodeType::Directory {
            return Err(crate::vfs_err_not_dir!("rmdir target is not directory"));
        }
        // Ensure directory empty
        if let InMemoryVnodeKindData::Directory(child_entries) = &node.kind {
            if !child_entries.read().await.is_empty() {
                return Err(crate::vfs_err_not_empty!(dirname));
            }
        }
        guard.remove(dirname);
        Ok(())
    }

    async fn rename(
        self: Arc<Self>,
        _old_name: &OsStr,
        _new_parent: Arc<dyn Vnode + Send + Sync>,
        _new_name: &OsStr,
    ) -> VfsResult<()> {
        Err(vfs_err_not_implemented!("InMemoryVnode::rename "))
    }

    async fn open_file_handle(
        self: Arc<Self>,
        flags: OpenFlags,
    ) -> VfsResult<Arc<dyn crate::traits::AsyncFileHandle + Send + Sync>> {
        match &self.kind {
            InMemoryVnodeKindData::File(_) => {
                Ok(Arc::new(super::handle::InMemoryFileHandle::new(self.clone(), flags)) as Arc<_>)
            }
            _ => Err(crate::vfs_err_not_implemented!("open_file_handle on non-file")),
        }
    }

    async fn open_dir_handle(
        self: Arc<Self>,
        _flags: OpenFlags,
    ) -> VfsResult<Arc<dyn crate::traits::AsyncDirHandle + Send + Sync>> {
        match &self.kind {
            InMemoryVnodeKindData::Directory(entries) => {
                let guard = entries.read().await;
                let mut list = Vec::with_capacity(guard.len());
                for (name, vnode) in guard.iter() {
                    let meta = vnode.metadata().await?;
                    list.push(crate::types::DirectoryEntry {
                        name: name.clone(),
                        vnode_id: meta.vnode_id,
                        kind: meta.kind,
                    });
                }
                Ok(Arc::new(super::handle::InMemoryDirHandle::new(self.clone(), list)) as Arc<_>)
            }
            _ => Err(crate::vfs_err_not_dir!("open_dir_handle on non-directory")),
        }
    }

    async fn readlink(self: Arc<Self>) -> VfsResult<PathBuf> {
        match &self.kind {
            InMemoryVnodeKindData::SymbolicLink(target) => Ok(target.clone()),
            _ => Err(crate::vfs_err_invalid_argument!("readlink on non-symlink")),
        }
    }
    
    async fn mkdir(
        self: Arc<Self>,
        name: &OsStr,
        permissions: FileMode,
    ) -> VfsResult<Arc<dyn Vnode + Send + Sync>> {
        use crate::types::VnodeType;
        use crate::vfs_err_already_exists;

        // 1. 检查当前节点是否为目录
        let InMemoryVnodeKindData::Directory(entries) = &self.kind else {
            return Err(vfs_err_not_implemented!("mkdir on non-directory node"));
        };

        // 2. 检查目录是否已存在
        let name_str = name.as_str();
        let mut entries_guard = entries.write().await;
        
        if entries_guard.contains_key(name_str) {
            return Err(vfs_err_already_exists!("Directory already exists"));
        }

        // 3. 获取文件系统引用
        let fs = self.fs.upgrade().ok_or_else(|| vfs_err_invalid_argument!("Filesystem no longer exists"))?;
        
        // 4. 创建新目录的元数据
        let now = ostd::timer::Jiffies::elapsed();
        let metadata = VnodeMetadata {
            vnode_id: 0, // 临时值，将在分配后更新
            fs_id: fs.id(),
            kind: VnodeType::Directory,
            size: 0,
            permissions: permissions & FileMode::all(), // 只保留权限位
            timestamps: Timestamps {
                accessed: now,
                modified: now,
                created: now,
                changed: now,
            },
            uid: 0, // root
            gid: 0, // root
            nlinks: 2, // . 和 ..
            rdev: None,
        };

        // 5. 创建新目录节点
        let new_dir = Arc::new(InMemoryVnode::new_directory(
            Arc::downgrade(&fs),
            Self::alloc_vnode_id(),
            metadata,
        ));

        // 6. 将新目录添加到当前目录
        entries_guard.insert(name_str.to_string(), new_dir.clone());

        // 7. 更新父目录的修改时间
        // 注意：这里简化处理，实际应该更新 mtime 和 ctime
        
        // 8. 返回新创建的目录节点
        Ok(new_dir as Arc<dyn Vnode + Send + Sync>)
    }
    
    async fn symlink_node(
        self: Arc<Self>,
        _name: &OsStr,
        _target: &PathSlice,
    ) -> VfsResult<Arc<dyn Vnode + Send + Sync>> {
        Err(vfs_err_not_implemented!("InMemoryVnode::symlink_node "))
    }

    async fn is_symlink(&self) -> VfsResult<bool> {
        Ok(matches!(self.kind, InMemoryVnodeKindData::SymbolicLink(_)))
    }
}
