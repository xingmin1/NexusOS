//! VFS 缓存模块
//!
//! 本模块实现了VFS的缓存机制，包括Vnode缓存和Dentry缓存。
//! 这些缓存可以提高文件系统性能，减少重复的文件系统操作。

use alloc::{collections::btree_map::BTreeMap as HashMap};

use ostd::sync::RwLock;

use crate::{path::PathBuf, static_dispatch::vnode::SVnode, types::VnodeId};

/// Vnode缓存，用于缓存常用的Vnode对象
///
/// 这个缓存可以避免多次从文件系统重新加载同一个Vnode，
/// 提高文件系统访问性能。
pub struct VnodeCache {
    // 使用RwLock保护内部缓存，允许多读单写并发访问
    cache: RwLock<HashMap<VnodeId, SVnode>>,
    capacity: usize, // 缓存容量上限
}

impl VnodeCache {
    /// 创建一个新的VnodeCache实例
    ///
    /// # 参数
    /// - `capacity`: 缓存的预期容量
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()), // BTreeMap没有with_capacity方法
            capacity,
        }
    }

    /// 获取缓存中的Vnode
    ///
    /// # 参数
    /// - `vnode_id`: 要查找的Vnode ID
    ///
    /// # 返回
    /// 如果找到，返回指向缓存Vnode的Arc指针；否则返回None
    pub async fn get(&self, vnode_id: VnodeId) -> Option<SVnode> {
        // 尝试从缓存中读取
        let cache = self.cache.read().await;
        cache.get(&vnode_id).cloned()
    }

    /// 向缓存中添加Vnode
    ///
    /// # 参数
    /// - `vnode`: 要缓存的Vnode
    ///
    /// # 返回
    /// 如果添加成功，返回克隆的Arc指针；否则返回原始Arc（未添加）
    pub async fn put(
        &self,
        vnode: SVnode,
    ) -> SVnode {
        let vnode_id = vnode.id();
        let mut cache = self.cache.write().await;

        // 如果缓存满了，可以实现一个淘汰策略，例如LRU
        // 目前为简化实现，如果容量超限，不添加新项
        if cache.len() >= self.capacity && !cache.contains_key(&vnode_id) {
            return vnode;
        }

        cache.insert(vnode_id, vnode.clone());
        vnode
    }

    /// 从缓存中移除Vnode
    ///
    /// # 参数
    /// - `vnode_id`: 要移除的Vnode ID
    pub async fn remove(&self, vnode_id: VnodeId) {
        let mut cache = self.cache.write().await;
        cache.remove(&vnode_id);
    }

    /// 清空整个缓存
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
}

/// 目录项缓存，用于缓存路径查找结果
///
/// 这个缓存可以加速路径解析过程，避免重复的目录查找操作。
pub struct DentryCache {
    // 键是目录路径和文件名的组合，值是对应的Vnode
    cache: RwLock<HashMap<(PathBuf, AllocString), SVnode>>,
    capacity: usize, // 缓存容量上限
}

use alloc::string::String as AllocString;

impl DentryCache {
    /// 创建一个新的DentryCache实例
    ///
    /// # 参数
    /// - `capacity`: 缓存的预期容量
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()), // BTreeMap没有with_capacity方法
            capacity,
        }
    }

    /// 获取缓存中的目录项
    ///
    /// # 参数
    /// - `dir_path`: 父目录路径
    /// - `name`: 文件或目录名
    ///
    /// # 返回
    /// 如果找到，返回缓存的Vnode；否则返回None
    pub async fn get(
        &self,
        dir_path: &PathBuf,
        name: &str,
    ) -> Option<SVnode> {
        let cache = self.cache.read().await;
        cache
            .get(&(dir_path.clone(), AllocString::from(name)))
            .cloned()
    }

    /// 向缓存中添加目录项
    ///
    /// # 参数
    /// - `dir_path`: 父目录路径
    /// - `name`: 文件或目录名
    /// - `vnode`: 对应的Vnode
    pub async fn put(
        &self,
        dir_path: PathBuf,
        name: &str,
        vnode: SVnode,
    ) {
        let mut cache = self.cache.write().await;

        // 如果缓存满了，可以实现一个淘汰策略
        // 目前为简化实现，如果容量超限，不添加新项
        if cache.len() >= self.capacity {
            return;
        }

        cache.insert((dir_path, AllocString::from(name)), vnode);
    }

    /// 清除与特定目录相关的所有缓存项
    ///
    /// 当目录内容变化时调用此方法
    ///
    /// # 参数
    /// - `dir_path`: 被修改的目录路径
    pub async fn invalidate_dir(&self, dir_path: &PathBuf) {
        let mut cache = self.cache.write().await;
        cache.retain(|(path, _), _| path != dir_path);
    }

    /// 清空整个缓存
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
}
