use alloc::boxed::Box;
use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::sync::{Arc, Weak};
use async_trait::async_trait;
use ostd::sync::RwLock;

use crate::{types::{FileMode, Timestamps}, vfs_err_already_exists, vfs_err_invalid_argument, vfs_err_no_space, vfs_err_not_dir, vfs_err_not_implemented, AsyncFileSystem, VfsResult};
use crate::{
    path::{VfsPath, VfsPathBuf},
    traits::AsyncVnode,
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
    SymbolicLink(VfsPathBuf),
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
        target: VfsPathBuf,
        metadata: VnodeMetadata,
    ) -> Self {
        Self::new(
            fs,
            id,
            InMemoryVnodeKindData::SymbolicLink(target),
            metadata,
        )
    }
}

#[async_trait]
impl AsyncVnode for InMemoryVnode {
    fn id(&self) -> VnodeId {
        self.id
    }
    
    fn filesystem(&self) -> Arc<dyn crate::traits::AsyncFileSystem + Send + Sync> {
        self.fs.upgrade().unwrap()
    }
    
    async fn metadata(&self) -> VfsResult<VnodeMetadata> {
        Err(vfs_err_not_implemented!("InMemoryVnode::metadata "))
    }
    
    async fn set_metadata(&self, _changes: VnodeMetadataChanges) -> VfsResult<()> {
        Err(vfs_err_not_implemented!("InMemoryVnode::set_metadata "))
    }
    
    async fn lookup(self: Arc<Self>, _name: &OsStr) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        Err(vfs_err_not_implemented!("InMemoryVnode::lookup "))
    }
    
    async fn create_node(
        self: Arc<Self>,
        name: &OsStr,
        _kind: VnodeType,
        _permissions: FileMode,
        _rdev: Option<u64>,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        // 先尝试转换为字符串，便于处理
        let filename_str = name.to_string();
        
        // 检查是否是目录类型
        match &self.kind {
            InMemoryVnodeKindData::Directory(entries) => {
                let entries_guard = entries.write().await;
                
                // 检查是否已存在
                if entries_guard.contains_key(&filename_str.to_string()) {
                    return Err(vfs_err_already_exists!(format!("Entry '{}' already exists in directory {}", filename_str, self.id)));
                }
                
                // 创建新节点并返回
                Err(vfs_err_no_space!("InMemoryFs out of Inode numbers "))
            }
            _ => Err(vfs_err_not_dir!(format!("Cannot create entry in Vnode {} because it is not a directory", self.id))),
        }
    }
    
    async fn unlink(self: Arc<Self>, _name: &OsStr) -> VfsResult<()> {
        Err(vfs_err_not_implemented!("InMemoryVnode::unlink "))
    }

    async fn rmdir(self: Arc<Self>, _name: &OsStr) -> VfsResult<()> {
        Err(vfs_err_no_space!("InMemoryFs out of Inode numbers "))
    }

    async fn rename(
        self: Arc<Self>,
        _old_name: &OsStr,
        _new_parent: Arc<dyn AsyncVnode + Send + Sync>,
        _new_name: &OsStr,
    ) -> VfsResult<()> {
        Err(vfs_err_not_implemented!("InMemoryVnode::rename "))
    }

    async fn open_file_handle(
        self: Arc<Self>,
        _flags: OpenFlags,
    ) -> VfsResult<Arc<dyn crate::traits::AsyncFileHandle + Send + Sync>> {
        Err(vfs_err_not_implemented!("InMemoryVnode::open_file_handle "))
    }

    async fn open_dir_handle(
        self: Arc<Self>,
        _flags: OpenFlags,
    ) -> VfsResult<Arc<dyn crate::traits::AsyncDirHandle + Send + Sync>> {
        Err(vfs_err_not_implemented!("InMemoryVnode::open_dir_handle "))
    }

    async fn readlink(self: Arc<Self>) -> VfsResult<VfsPathBuf> {
        Err(vfs_err_not_implemented!("InMemoryVnode::readlink "))
    }
    
    async fn mkdir(
        self: Arc<Self>,
        name: &OsStr,
        permissions: FileMode,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
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
            0, // 临时ID
            metadata,
        ));

        // 6. 将新目录添加到当前目录
        entries_guard.insert(name_str.to_string(), new_dir.clone());

        // 7. 更新父目录的修改时间
        // 注意：这里简化处理，实际应该更新 mtime 和 ctime
        
        // 8. 返回新创建的目录节点
        Ok(new_dir as Arc<dyn AsyncVnode + Send + Sync>)
    }
    
    async fn symlink_node(
        self: Arc<Self>,
        _name: &OsStr,
        _target: &VfsPath,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        Err(vfs_err_not_implemented!("InMemoryVnode::symlink_node "))
    }
}
