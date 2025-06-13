//! VFS 管理器模块
//!
//! 本模块实现了虚拟文件系统的核心管理功能，包括文件系统的注册、挂载、路径解析等。
//! VfsManager 作为VFS的中央组件，负责协调不同的文件系统实例和路径操作。

use alloc::{
    collections::btree_map::BTreeMap as HashMap,
    format,
    string::{String as AllocString, ToString},
    sync::{Arc, Weak},
    vec::Vec,
};

use id_alloc::IdAlloc;
use nexus_error::{
    error_stack::{bail, report, ResultExt},
    Errno,
};
use ostd::sync::{Mutex, RwLock};

use crate::{
    cache::{DentryCache, VnodeCache},
    path::{VfsPath, VfsPathBuf},
    traits::{AsyncBlockDevice, AsyncFileSystem, AsyncFileSystemProvider, AsyncVnode},
    types::{FileMode, FilesystemId, FsOptions, MountId},
    verror::{KernelError, VfsResult}, vfs_err_invalid_argument,
};

/// VFS管理器结构体
///
/// 负责管理文件系统提供者、挂载点、路径解析等核心功能。
pub struct VfsManager {
    /// 挂载表：路径到挂载点的映射
    pub(crate) mount_table: Arc<RwLock<HashMap<VfsPathBuf, MountEntry>>>,
    /// 通过ID查找挂载点路径
    mount_points_by_id: Arc<RwLock<HashMap<MountId, VfsPathBuf>>>,
    /// 注册的文件系统提供者
    fs_providers: Arc<RwLock<HashMap<AllocString, Arc<dyn AsyncFileSystemProvider + Send + Sync>>>>,
    /// 挂载ID分配器
    mount_id_allocator: Arc<Mutex<IdAlloc>>,
    /// 文件系统ID分配器
    fs_id_allocator: Arc<Mutex<IdAlloc>>,
    /// Vnode缓存
    pub(crate) vnode_cache: Arc<VnodeCache>,
    /// 目录项缓存
    pub(crate) dentry_cache: Arc<DentryCache>,
    /// 对自身的弱引用，用于在异步方法中获取完整的Arc<VfsManager>
    self_weak_ref: Weak<VfsManager>,
}

/// 挂载点条目
#[derive(Clone)]
pub(crate) struct MountEntry {
    /// 挂载点ID
    pub(crate) mount_id: MountId,
    /// 文件系统实例
    pub(crate) fs_instance: Arc<dyn AsyncFileSystem + Send + Sync>,
}

/// VfsManager构建器
///
/// 用于创建和配置VfsManager实例。
pub struct VfsManagerBuilder {
    /// 初始文件系统提供者
    initial_providers: HashMap<AllocString, Arc<dyn AsyncFileSystemProvider + Send + Sync>>,
    /// Vnode缓存容量
    vnode_cache_capacity: Option<usize>,
    /// 目录项缓存容量
    dentry_cache_capacity: Option<usize>,
}

impl VfsManagerBuilder {
    /// 创建一个新的VfsManagerBuilder实例
    pub fn new() -> Self {
        Self {
            initial_providers: HashMap::new(),
            vnode_cache_capacity: None,
            dentry_cache_capacity: None,
        }
    }

    /// 添加一个文件系统提供者
    pub fn provider(mut self, provider: Arc<dyn AsyncFileSystemProvider + Send + Sync>) -> Self {
        self.initial_providers
            .insert(provider.fs_type_name().into(), provider);
        self
    }

    /// 设置Vnode缓存容量
    pub fn vnode_cache_capacity(mut self, capacity: usize) -> Self {
        self.vnode_cache_capacity = Some(capacity);
        self
    }

    /// 设置目录项缓存容量
    pub fn dentry_cache_capacity(mut self, capacity: usize) -> Self {
        self.dentry_cache_capacity = Some(capacity);
        self
    }

    /// 构建VfsManager实例
    pub fn build(self) -> Arc<VfsManager> {
        const DEFAULT_VNODE_CACHE_CAP: usize = 1024; // 默认Vnode缓存大小
        const DEFAULT_DENTRY_CACHE_CAP: usize = 4096; // 默认目录项缓存大小

        let vnode_cache_cap = self.vnode_cache_capacity.unwrap_or(DEFAULT_VNODE_CACHE_CAP);
        let dentry_cache_cap = self
            .dentry_cache_capacity
            .unwrap_or(DEFAULT_DENTRY_CACHE_CAP);

        let vnode_cache = Arc::new(VnodeCache::new(vnode_cache_cap));
        let dentry_cache = Arc::new(DentryCache::new(dentry_cache_cap));

        // 使用Arc::new_cyclic创建带有自引用的Arc
        Arc::new_cyclic(|self_weak: &Weak<VfsManager>| VfsManager {
            mount_table: Arc::new(RwLock::new(HashMap::new())),
            mount_points_by_id: Arc::new(RwLock::new(HashMap::new())),
            fs_providers: Arc::new(RwLock::new(self.initial_providers)),
            mount_id_allocator: Arc::new(Mutex::new(IdAlloc::with_capacity(1024))),
            fs_id_allocator: Arc::new(Mutex::new(IdAlloc::with_capacity(1024))),
            vnode_cache,
            dentry_cache,
            self_weak_ref: self_weak.clone(),
        })
    }
}

impl VfsManager {
    /// 获取对自身的Arc引用
    ///
    /// 这在异步方法中很有用，因为它们需要使用Arc<Self>。
    pub fn self_arc(&self) -> VfsResult<Arc<VfsManager>> {
        self.self_weak_ref.upgrade().ok_or_else(|| {
            report!(KernelError::with_message(
                Errno::EINVAL,
                "VfsManager实例已被销毁"
            ))
        })
    }

    /// 注册文件系统提供者
    ///
    /// # 参数
    /// - `provider`: 要注册的文件系统提供者
    ///
    /// # 返回
    /// 成功则返回Ok(())，已存在则返回错误
    pub async fn register_filesystem_provider(
        &self,
        provider: Arc<dyn AsyncFileSystemProvider + Send + Sync>,
    ) -> VfsResult<()> {
        let provider_name = provider.fs_type_name().to_string();
        let mut providers = self.fs_providers.write().await;

        if providers.contains_key(&provider_name) {
            return Err(vfs_err_invalid_argument!(
                "register_filesystem_provider",
                format!("文件系统提供者'{}'已注册", provider_name)
            ));
        }

        providers.insert(provider_name, provider);
        Ok(())
    }

    /// 注销文件系统提供者
    ///
    /// # 参数
    /// - `fs_type_name`: 要注销的文件系统类型名称
    ///
    /// # 返回
    /// 成功则返回Ok(())，不存在则返回错误
    pub async fn unregister_filesystem_provider(&self, fs_type_name: &str) -> VfsResult<()> {
        let mut providers = self.fs_providers.write().await;

        if !providers.contains_key(fs_type_name) {
            return Err(vfs_err_invalid_argument!(
                "unregister_filesystem_provider",
                format!("文件系统提供者'{}'未注册", fs_type_name)
            ));
        }

        providers.remove(fs_type_name);
        Ok(())
    }

    /// 挂载一个文件系统
    ///
    /// # 参数
    /// - `source_device`: 可选的块设备，用于存储文件系统数据
    /// - `target_path_str`: 要挂载到的目标路径
    /// - `fs_type_name`: 文件系统类型名称
    /// - `options`: 挂载选项
    ///
    /// # 返回
    /// 成功则返回挂载点ID，失败则返回错误
    pub async fn mount(
        &self,
        source_device: Option<Arc<dyn AsyncBlockDevice + Send + Sync>>,
        target_path_str: &str,
        fs_type_name: &str,
        options: FsOptions,
    ) -> VfsResult<MountId> {
        // 解析并规范化目标路径
        let target_path = VfsPathBuf::new(target_path_str.to_string())?;
        // 前面执行了新建，已经规范化了，这里实际不需要normalize

        // 验证路径是否为绝对路径
        if !VfsPath::from(&target_path).is_absolute() {
            return Err(vfs_err_invalid_argument!("mount", "挂载点必须是绝对路径"));
        }

        // 检查挂载点冲突
        {
            let mount_table = self.mount_table.read().await;
            if mount_table.contains_key(&target_path) {
                return Err(vfs_err_invalid_argument!(
                    "mount",
                    format!("路径'{}'已挂载", target_path)
                ));
            }
        }

        // 获取文件系统提供者
        let provider = {
            let providers = self.fs_providers.read().await;
            providers.get(fs_type_name).cloned().ok_or_else(|| {
                report!(KernelError::new(Errno::ENOENT,))
                    .attach_printable(format!("文件系统提供者'{}'未注册", fs_type_name))
            })
        }?;

        // 分配ID
        let mount_id = {
            let mut allocator_guard = self.mount_id_allocator.lock().await;
            allocator_guard.alloc().ok_or_else(|| {
                report!(KernelError::with_message(
                    Errno::ENOMEM,
                    "无法分配挂载点ID: 内存不足"
                ))
            })?
        };
        let fs_id = {
            let mut allocator_guard = self.fs_id_allocator.lock().await;
            allocator_guard.alloc().ok_or_else(|| {
                report!(KernelError::with_message(
                    Errno::ENOMEM,
                    "无法分配文件系统ID: 内存不足"
                ))
            })?
        };

        // 调用提供者挂载文件系统
        let mount_result = provider
            .mount(source_device, &options, mount_id, fs_id)
            .await;
        let fs_instance = match mount_result {
            Ok(instance) => instance,
            Err(e) => {
                // 释放已分配的ID (尽力而为)
                {
                    let mut allocator = self.mount_id_allocator.lock().await;
                    allocator.free(mount_id);
                    // 不再需要判断锁获取是否成功，因为OSTD的锁在获取失败时会直接panic
                }

                {
                    let mut allocator = self.fs_id_allocator.lock().await;
                    allocator.free(fs_id);
                }
                return Err(report!(KernelError::new(Errno::EIO,))
                    .attach_printable(format!("文件系统挂载失败: {:?}", e)));
            }
        };

        // 更新挂载表
        {
            // 从 fs_instance 获取根 VNode
            let fs_root_vnode = match fs_instance.root_vnode().await {
                // 假设 AsyncFileSystem 有 async fn root(&self) -> error::Result<Arc<dyn AsyncVnode + Send + Sync>> 方法
                Ok(vnode) => vnode,
                Err(e) => {
                    // 文件系统实例已成功创建，但获取其根节点失败。
                    // 此时通常不应尝试回滚ID分配，因为文件系统提供者可能已经使用了它们。
                    // 记录详细错误并返回。
                    // log::error!("VFS: 成功挂载文件系统 (mount_id: {:?}, fs_id: {:?}) 但无法获取其根 VNode: {:?}", mount_id, fs_id, e);
                    return Err(report!(KernelError::new(Errno::EIO,))
                        .attach_printable(format!("获取挂载文件系统的根节点失败: {:?}", e)));
                }
            };

            let mut mount_table = self.mount_table.write().await;
            // 使用不同的变量名以避免在 map_err 闭包中可能出现的捕获问题
            let mut mount_points_map = self.mount_points_by_id.write().await;

            mount_table.insert(
                target_path.clone(),
                MountEntry {
                    mount_id,
                    fs_instance: Arc::clone(&fs_instance), // 使用正确获取的 fs_instance
                },
            );
            mount_points_map.insert(mount_id, target_path.clone());
        }

        Ok(mount_id)
    }
    /// 卸载文件系统
    ///
    /// # 参数
    /// - `mount_id`: 要卸载的挂载点ID
    ///
    /// # 返回
    /// 成功则返回Ok(())，失败则返回错误
    pub async fn unmount(&self, mount_id: MountId) -> VfsResult<()> {
        // 返回类型改为 error::Result
        // 查找挂载点条目 (fs_instance 和 mount_id 都包含在 MountEntry 中)
        // 我们需要先从 mount_points_by_id 获得 VNode，然后再遍历 mount_table 找到对应的 MountEntry
        // 更高效的方式是，如果 MountEntry 中存储了 mount_path，或者 mount_points_by_id 直接存储 MountEntry 的 Arc
        // 但基于当前结构，我们需要 mount_id 来找到 fs_instance (通过 mount_table) 和 mount_path (如果它存储在某处)
        // MountEntry 包含 fs_instance 和 mount_id。我们需要从 mount_table 移除。
        // mount_points_by_id 存储 mount_id -> VNode。也需要移除。

        // 首先，通过 mount_id 从 mount_points_by_id 获取对应的 VNode (如果存在)
        // 然后，找到 mount_table 中与该 mount_id 关联的 MountEntry
        // 这似乎有点迂回。MountEntry 本身就应该可以直接通过 mount_id 访问。

        // 假设 MountEntry 中没有直接存储 mount_path，我们将需要它来从 mount_table 中移除。
        // 而 mount_points_by_id 也不直接存储 mount_path。
        // 当前 unmount 的逻辑是先通过 mount_id 找到 mount_path，再通过 mount_path 找到 MountEntry。
        // 这暗示 mount_points_by_id 应该存储 mount_id -> mount_path。
        // 但 grep 的结果显示 MountEntry { mount_id, fs_instance }，
        // 且之前的 mount 方法插入的是 mount_points_by_id.insert(mount_id, Arc::clone(&fs_root_vnode));
        // 这说明 mount_points_by_id 是 MountId -> Arc<AsyncVnode>

        // 因此，正确的逻辑应该是：
        // 1. 从 mount_table 中找到 mount_id 对应的 MountEntry。mount_table 是 VfsPath -> MountEntry。
        //    这意味着我们必须遍历 mount_table 来找到具有特定 mount_id 的条目。
        // 2. 获取该 MountEntry 中的 fs_instance。
        // 3. 调用 fs_instance.unmount()。
        // 4. 如果成功，则从 mount_table 和 mount_points_by_id 中移除相关条目。

        let mut mount_table = self.mount_table.write().await;
        let mut mount_points_by_id_map = self.mount_points_by_id.write().await;

        // 找到 mount_id 对应的 mount_path 和 fs_instance
        let mut mount_path_to_remove: Option<VfsPathBuf> = None;
        let mut fs_instance_to_unmount: Option<Arc<dyn AsyncFileSystem + Send + Sync>> = None;
        let mut fs_id_to_free: Option<FilesystemId> = None; // 我们需要 FsId 来释放它

        for (path, entry) in mount_table.iter() {
            if entry.mount_id == mount_id {
                mount_path_to_remove = Some(path.clone());
                fs_instance_to_unmount = Some(Arc::clone(&entry.fs_instance));
                // 从 MountEntry 定义来看，它不包含 FsId，但卸载时最好能释放 FsId。
                // 这意味着 MountEntry 可能需要存储 FsId，或者 VfsManager 需要一个 MountId -> FsId 的映射。
                // 目前的 MountEntry { mount_id, fs_instance }。FsId 是在 mount 时分配并传递给 provider.mount 的。
                // Provider 的 unmount 方法可能需要它，或者 manager 需要它来释放。
                // 假设 provider 的 unmount 不需要 fs_id，但 manager 需要释放它。
                // 我们需要一个方法从 mount_id 找到 fs_id。也许 FsId 应该在 MountEntry 中。
                // 暂时我们无法获取 fs_id 用于释放。
                break;
            }
        }

        let (mount_path, fs_instance) = match (mount_path_to_remove, fs_instance_to_unmount) {
            (Some(path), Some(instance)) => (path, instance),
            _ => {
                return Err(report!(KernelError::new(Errno::EINVAL,))
                    .attach_printable(format!("未找到与挂载ID '{0}' 关联的挂载点", mount_id)));
            }
        };

        // 准备卸载文件系统
        fs_instance.unmount_prepare().await.map_err(|e| {
            // 即便卸载准备失败，也应该尝试记录，但不一定需要回滚表的修改，因为文件系统可能已处于不一致状态
            // 根据具体策略，这里也可以选择直接返回错误，不继续清理 manager 内部状态
            report!(KernelError::new(Errno::EBUSY,))
                .attach_printable(format!("文件系统卸载准备失败: {:?}", e))
        })?;

        // 从挂载表和挂载点ID映射中移除条目
        // mount_table 和 mount_points_by_id_map 已经从函数开始时就获取了写锁
        if mount_table.remove(&mount_path).is_none() {
            // 如果mount_path不存在于mount_table中，这可能表示状态不一致，但我们仍继续尝试清理
            // 可选: tracing::warn!("[unmount] mount_path {:?} 未在 mount_table 中找到，但仍继续卸载流程 (mount_id: {})", mount_path, mount_id.0);
        }
        if mount_points_by_id_map.remove(&mount_id).is_none() {
            // 如果mount_id不存在于mount_points_by_id_map中，也可能表示状态不一致
            // 可选: tracing::warn!("[unmount] mount_id {} 未在 mount_points_by_id_map 中找到，但仍继续卸载流程", mount_id.0);
        }

        // 获取 FsId 以便释放
        let fs_id_to_free = fs_instance.id(); // fs_instance.id() 返回 FilesystemId, 假设可直接用于 FsId 的分配器

        // 释放挂载ID
        {
            let mut allocator = self.mount_id_allocator.lock().await;
            allocator.free(mount_id);
        }

        // 释放文件系统ID
        {
            let mut allocator = self.fs_id_allocator.lock().await;
            allocator.free(fs_id_to_free); // 使用从 fs_instance 获取的 fs_id
        }

        Ok(())
    }

    /// 从路径获取Vnode
    ///
    /// # 参数
    /// - `path_str`: 路径字符串
    /// - `follow_last_symlink`: 是否跟随最后一个符号链接
    ///
    /// # 返回
    /// 成功则返回Vnode，失败则返回错误
    pub async fn get_vnode(
        &self,
        path_str: &str,
        follow_last_symlink: bool,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        // 解析并规范化路径
        let path = VfsPathBuf::new(path_str.to_string())?;

        // 初始化符号链接递归深度计数
        let mut symlink_depth = 0;

        // 获取对自身的Arc引用
        let self_arc = self.self_arc().change_context_lazy(|| {
            KernelError::with_message(Errno::EINVAL, "获取VfsManager的Arc引用失败")
        })?;

        // 解析路径并返回vnode
        self.resolve_path(self_arc, path, follow_last_symlink, &mut symlink_depth)
            .await
            .change_context_lazy(|| KernelError::with_message(Errno::EINVAL, "解析路径失败"))
    }

    /// 解析全局路径并返回vnode（内部实现）
    ///
    /// # 参数
    /// - `self_arc`: 对自身的Arc引用
    /// - `path`: 要解析的路径
    /// - `follow_last_symlink`: 是否跟随最后一个符号链接
    /// - `symlink_depth`: 符号链接递归跟随的深度计数
    ///
    /// # 返回
    /// 成功则返回Vnode，失败则返回错误
    async fn resolve_path(
        &self,
        _self_arc: Arc<VfsManager>,
        path: VfsPathBuf,
        _follow_last_symlink: bool,
        symlink_depth: &mut u32,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        // 检查符号链接递归深度
        if *symlink_depth > 40 {
            bail!(KernelError::with_message(Errno::ELOOP, "符号链接递归过深"));
        }

        // 当前路径相关的挂载点
        let mount_point = self
            .find_mount_point_for_path(&VfsPath::from(&path))
            .await
            .change_context_lazy(|| KernelError::with_message(Errno::EINVAL, "查找挂载点失败"))?;

        // 得到这个挂载点的文件系统和相对路径
        let (fs_instance, relative_path) = {
            let mount_table = self.mount_table.read().await;
            let mount_entry = mount_table.get(&mount_point).ok_or_else(|| {
                report!(KernelError::new(Errno::ENOENT,))
                    .attach_printable(format!("挂载点 '{}' 不存在", mount_point))
            })?;

            // 计算相对路径
            let relative_path = if mount_point.as_str() == "/" {
                // 根目录特殊处理
                path.clone()
            } else {
                // 删除挂载点前缀得到相对路径
                let mount_prefix_len = mount_point.as_str().len();
                let relative_str = &path.as_str()[mount_prefix_len..];

                // 确保相对路径以/开头
                if relative_str.is_empty() {
                    VfsPathBuf::new("/".to_string())?
                } else {
                    VfsPathBuf::new(relative_str.to_string())?
                }
            };

            (mount_entry.fs_instance.clone(), relative_path)
        };

        // 获取根目录vnode
        let root_vnode = fs_instance.root_vnode().await.map_err(|e| {
            report!(KernelError::new(Errno::EIO,))
                .attach_printable(format!("获取根目录vnode失败: {:?}", e))
        })?;

        // 如果路径就是根目录，直接返回
        if relative_path.as_str() == "/" {
            return Ok(root_vnode);
        }

        // 在文件系统内部解析路径段
        let segments = VfsPath::from(&relative_path).components();
        let mut current = root_vnode.clone();

        // 遍历路径段
        for segment in segments {
            if segment == "." {
                continue; // 当前目录，不做操作
            } else if segment == ".." {
                // 跳到上级目录
                // [TODO]: 实现上层目录的处理
                continue;
            }

            // 查找子项
            let child = current.lookup(segment).await.map_err(|e| {
                report!(KernelError::new(Errno::ENOENT,))
                    .attach_printable(format!("查找 '{}' 失败: {:?}", segment, e))
            })?;

            // 检查是否为符号链接
            if child.is_symlink().await? {
                // [TODO]: 实现符号链接解析
                *symlink_depth += 1;
            }

            current = child;
        }

        Ok(current)
    }

    /// 列出所有挂载点
    ///
    /// # 返回
    /// 挂载点列表，每个条目包含：(挂载ID，挂载路径，文件系统类型)
    pub async fn list_mount_points(&self) -> VfsResult<Vec<(MountId, VfsPathBuf, AllocString)>> {
        let mount_table = self.mount_table.read().await;
        let mut result = Vec::with_capacity(mount_table.len());

        for (path, entry) in mount_table.iter() {
            result.push((
                entry.mount_id,
                path.clone(),
                entry.fs_instance.fs_type_name().into(),
            ));
        }

        Ok(result)
    }

    /// 为给定路径查找最匹配的挂载点
    ///
    /// 例如，如果有挂载点 "/mnt/data" 和 "/mnt"，并且路径是 "/mnt/data/file.txt"，
    /// 则应该返回 "/mnt/data"
    ///
    /// # 参数
    /// - `path`: 要查找挂载点的路径
    ///
    /// # 返回
    /// 最匹配的挂载点路径，如果没有匹配的挂载点，则返回根目录 "/"
    async fn find_mount_point_for_path(&self, path: &VfsPath<'_>) -> VfsResult<VfsPathBuf> {
        // 获取挂载表读锁
        let mount_table = self.mount_table.read().await;

        // 检查路径为空或不以/开头的特殊情况
        let path_str = path.as_str();
        if path_str.is_empty() || !path_str.starts_with('/') {
            return Err(report!(KernelError::new(Errno::EINVAL,))
                .attach_printable(format!("无效路径: '{}'", path_str)));
        }

        // 如果挂载表为空，只返回根目录
        if mount_table.is_empty() {
            return Ok(VfsPathBuf::new("/".to_string())?);
        }

        // 存储最长匹配前缀
        let mut best_match = VfsPathBuf::new("/".to_string())?;
        let mut best_match_len = 0;

        // 遍历所有挂载点，查找最长匹配
        for (mount_path, _) in mount_table.iter() {
            let mount_str = mount_path.as_str();

            // 检查是否前缀匹配，并且是完整路径段
            // 如/mnt/data是/mnt/data/file.txt的前缀，但/mnt/d不是
            if path_str.starts_with(mount_str)
                && (mount_str == "/"
                    || path_str.len() == mount_str.len()
                    || path_str.as_bytes()[mount_str.len()] == b'/')
            {
                // 如果这是更长的前缀，则更新最佳匹配
                if mount_str.len() > best_match_len {
                    best_match = mount_path.clone();
                    best_match_len = mount_str.len();
                }
            }
        }

        Ok(best_match)
    }

    /// 创建一个目录
    ///
    /// # 参数
    /// - `path_str`: 要创建的目录路径
    /// - `permissions`: 权限位
    ///
    /// # 返回
    /// 成功则返回Ok(())，失败则返回错误
    pub async fn mkdir(&self, path_str: &str, permissions: FileMode) -> VfsResult<()> {
        // 解析并规范化路径
        let path = VfsPathBuf::new(path_str.to_string())?;

        // 获取父目录路径
        let parent_path = VfsPath::from(&path).parent().ok_or_else(|| {
            report!(KernelError::new(Errno::EINVAL,))
                .attach_printable(format!("路径 '{}' 没有父目录", path_str))
        })?;

        // 获取目录名
        let dir_name = VfsPath::from(&path).file_name().ok_or_else(|| {
            report!(KernelError::new(Errno::EINVAL,))
                .attach_printable(format!("路径 '{}' 没有有效的目录名", path_str))
        })?;

        // 获取父目录的Vnode
        let parent_vnode = self
            .get_vnode(parent_path.as_str(), true)
            .await
            .change_context_lazy(|| {
                KernelError::with_message(Errno::EINVAL, "获取父目录Vnode失败")
            })?;

        // 在父目录中创建新目录
        parent_vnode
            .clone()
            .mkdir(dir_name, permissions)
            .await
            .change_context_lazy(|| KernelError::new(Errno::EIO))
            .attach_printable(format!("创建目录失败"))?;

        Ok(())
    }

    /// 打开文件并返回文件句柄
    ///
    /// # 参数
    /// - `path_str`: 要打开的文件路径
    /// - `flags`: 打开标志，指定打开模式（如读、写、追加等）
    ///
    /// # 返回
    /// 成功则返回文件句柄，失败则返回错误
    pub async fn open(
        &self,
        path_str: &str,
        flags: crate::types::OpenFlags,
    ) -> VfsResult<Arc<dyn crate::traits::AsyncFileHandle + Send + Sync>> {
        // 尝试直接获取 vnode
        let vnode_res = self.get_vnode(path_str, true).await;

        let vnode = match vnode_res {
            Ok(v) => v,
            Err(e) => {
                // 如果失败且包含 CREATE，则尝试创建
                if flags.contains(crate::types::OpenFlags::CREATE) {
                    // 解析父目录和文件名
                    let path = crate::path::VfsPathBuf::new(path_str.to_string())?;
                    let parent_path = VfsPath::from(&path).parent().ok_or_else(|| {
                        report!(KernelError::new(Errno::EINVAL,)).attach_printable("路径没有父目录")
                    })?;
                    let file_name = VfsPath::from(&path).file_name().ok_or_else(|| {
                        report!(KernelError::new(Errno::EINVAL,)).attach_printable("无效文件名")
                    })?;

                    // 获取父 vnode
                    let parent_vnode = self.get_vnode(parent_path.as_str(), true).await?;
                    // 创建文件节点
                    let new_vnode = parent_vnode
                        .clone()
                        .create_node(file_name, crate::types::VnodeType::File, crate::types::FileMode::OWNER_RW | crate::types::FileMode::GROUP_RW | crate::types::FileMode::OTHER_RW, None)
                        .await?;
                    new_vnode
                } else {
                    return Err(e);
                }
            }
        };

        // 打开文件句柄
        let handle = vnode
            .clone()
            .open_file_handle(flags)
            .await
            .change_context_lazy(|| {
                KernelError::with_message(Errno::EIO, "打开文件句柄失败")
            })
            .attach_printable(format!("无法为 '{}' 打开文件句柄", path_str))?;

        Ok(handle)
    }

    /// 获取文件的元数据信息
    ///
    /// # 参数
    /// - `path_str`: 要获取元数据的文件路径
    /// - `follow_symlink`: 如果路径指向符号链接，是否获取目标文件而非链接本身的元数据
    ///
    /// # 返回
    /// 成功则返回文件元数据，失败则返回错误
    pub async fn stat(
        &self,
        path_str: &str,
        follow_symlink: bool,
    ) -> VfsResult<crate::types::VnodeMetadata> {
        // 获取路径对应的Vnode
        let vnode = self.get_vnode(path_str, follow_symlink).await?;

        // 获取Vnode的元数据
        let metadata = vnode
            .metadata()
            .await
            .change_context_lazy(|| {
                KernelError::with_message(Errno::EIO, "获取文件元数据失败")
            })
            .attach_printable(format!("无法获取 '{}' 的元数据", path_str))?;

        Ok(metadata)
    }

    /// 删除目录
    ///
    /// # 参数
    /// - `path_str`: 要删除的目录路径
    ///
    /// # 返回
    /// 成功则返回Ok(())，失败则返回错误
    pub async fn rmdir(&self, path_str: &str) -> VfsResult<()> {
        // 解析并规范化路径
        let path = VfsPathBuf::new(path_str.to_string())?;

        // 获取父目录路径
        let parent_path = VfsPath::from(&path).parent().ok_or_else(|| {
            report!(KernelError::new(Errno::EINVAL,))
                .attach_printable(format!("路径 '{}' 没有父目录", path_str))
        })?;

        // 获取要删除的目录名
        let dir_name = VfsPath::from(&path).file_name().ok_or_else(|| {
            report!(KernelError::new(Errno::EINVAL,))
                .attach_printable(format!("路径 '{}' 没有有效的目录名", path_str))
        })?;

        // 获取父目录的Vnode
        let parent_vnode = self
            .get_vnode(parent_path.as_str(), true)
            .await
            .change_context_lazy(|| {
                KernelError::with_message(Errno::EINVAL, "获取父目录Vnode失败")
            })?;

        // 执行删除操作
        parent_vnode
            .clone()
            .rmdir(dir_name)
            .await
            .change_context_lazy(|| KernelError::new(Errno::EIO))
            .attach_printable(format!("删除目录 '{}' 失败", path_str))?;

        // 如果有缓存，这里应该对Dentry缓存进行失效处理
        // TODO: 可能需要类似 invalidate_dentry 的方法

        Ok(())
    }

    /// 删除文件
    ///
    /// # 参数
    /// - `path_str`: 要删除的文件路径
    ///
    /// # 返回
    /// 成功则返回Ok(())，失败则返回错误
    pub async fn unlink(&self, path_str: &str) -> VfsResult<()> {
        // 解析并规范化路径
        let path = VfsPathBuf::new(path_str.to_string())?;

        // 获取父目录路径
        let parent_path = VfsPath::from(&path).parent().ok_or_else(|| {
            report!(KernelError::new(Errno::EINVAL,))
                .attach_printable(format!("路径 '{}' 没有父目录", path_str))
        })?;

        // 获取要删除的文件名
        let file_name = VfsPath::from(&path).file_name().ok_or_else(|| {
            report!(KernelError::new(Errno::EINVAL,))
                .attach_printable(format!("路径 '{}' 没有有效的文件名", path_str))
        })?;

        // 获取父目录的Vnode
        let parent_vnode = self
            .get_vnode(parent_path.as_str(), true)
            .await
            .change_context_lazy(|| {
                KernelError::with_message(Errno::EINVAL, "获取父目录Vnode失败")
            })?;

        // 执行删除操作 
        // unlink操作会删除文件或符号链接，但不会删除目录（应该使用rmdir）
        parent_vnode
            .clone()
            .unlink(file_name)
            .await
            .change_context_lazy(|| KernelError::new(Errno::EIO))
            .attach_printable(format!("删除文件 '{}' 失败", path_str))?;

        // 如果有缓存，这里应该对Dentry缓存进行失效处理
        // TODO: 可能需要类似 invalidate_dentry 的方法

        Ok(())
    }

    /// 重命名文件或目录
    ///
    /// # 参数
    /// - `old_path_str`: 原文件或目录路径
    /// - `new_path_str`: 新文件或目录路径
    ///
    /// # 返回
    /// 成功则返回Ok(())，失败则返回错误
    pub async fn rename(&self, old_path_str: &str, new_path_str: &str) -> VfsResult<()> {
        // 解析并规范化路径
        let old_path = VfsPathBuf::new(old_path_str.to_string())?;
        let new_path = VfsPathBuf::new(new_path_str.to_string())?;

        // 获取原路径和新路径的父目录路径
        let old_parent_path = VfsPath::from(&old_path).parent().ok_or_else(|| {
            report!(KernelError::new(Errno::EINVAL,))
                .attach_printable(format!("原路径 '{}' 没有父目录", old_path_str))
        })?;

        let new_parent_path = VfsPath::from(&new_path).parent().ok_or_else(|| {
            report!(KernelError::new(Errno::EINVAL,))
                .attach_printable(format!("新路径 '{}' 没有父目录", new_path_str))
        })?;

        // 获取原文件名和新文件名
        let old_name = VfsPath::from(&old_path).file_name().ok_or_else(|| {
            report!(KernelError::new(Errno::EINVAL,))
                .attach_printable(format!("原路径 '{}' 没有有效的文件名", old_path_str))
        })?;

        let new_name = VfsPath::from(&new_path).file_name().ok_or_else(|| {
            report!(KernelError::new(Errno::EINVAL,))
                .attach_printable(format!("新路径 '{}' 没有有效的文件名", new_path_str))
        })?;

        // 获取原路径和新路径的挂载点
        let old_mount_point = self.find_mount_point_for_path(&VfsPath::from(&old_path)).await?;
        let new_mount_point = self.find_mount_point_for_path(&VfsPath::from(&new_path)).await?;

        // 获取原目录和新目录的Vnode
        let old_parent_vnode = self
            .get_vnode(old_parent_path.as_str(), true)
            .await
            .change_context_lazy(|| {
                KernelError::with_message(Errno::EINVAL, "获取原父目录Vnode失败")
            })?;

        let new_parent_vnode = self
            .get_vnode(new_parent_path.as_str(), true)
            .await
            .change_context_lazy(|| {
                KernelError::with_message(Errno::EINVAL, "获取新父目录Vnode失败")
            })?;

        // 检查是否跨文件系统重命名
        // 如果原目录和新目录所属的文件系统实例不同，那么是跨文件系统操作
        if old_mount_point.as_str() != new_mount_point.as_str() {
            // 注意：跨文件系统重命名需要特殊处理
            return Err(report!(KernelError::new(Errno::EXDEV,))
                .attach_printable("不支持跨文件系统重命名"));
            // TODO: 实现跨文件系统重命名，通常需要复制文件内容并删除原文件
        }

        // 执行重命名操作
        old_parent_vnode
            .clone()
            .rename(old_name, new_parent_vnode.clone(), new_name)
            .await
            .change_context_lazy(|| KernelError::new(Errno::EIO))
            .attach_printable(format!("重命名 '{}' 到 '{}' 失败", old_path_str, new_path_str))?;

        // 如果有缓存，这里应该对相关的Dentry缓存进行失效处理
        // TODO: 实现缓存失效逻辑

        Ok(())
    }

    /// 创建符号链接
    ///
    /// # 参数
    /// - `path_str`: 符号链接路径
    /// - `target_str`: 符号链接目标路径
    ///
    /// # 返回
    /// 成功则返回Ok(())，失败则返回错误
    pub async fn symlink(&self, target_str: &str, link_path_str: &str) -> VfsResult<()> {
        // 解析并规范化符号链接路径
        let link_path = VfsPathBuf::new(link_path_str.to_string())?;

        // 将目标路径转换为VfsPathBuf
        let target_path = VfsPathBuf::new(target_str.to_string())?;

        // 获取符号链接的父目录路径
        let parent_path = VfsPath::from(&link_path).parent().ok_or_else(|| {
            report!(KernelError::new(Errno::EINVAL,))
                .attach_printable(format!("路径 '{}' 没有父目录", link_path_str))
        })?;

        // 获取符号链接的文件名
        let link_name = VfsPath::from(&link_path).file_name().ok_or_else(|| {
            report!(KernelError::new(Errno::EINVAL,))
                .attach_printable(format!("路径 '{}' 没有有效的文件名", link_path_str))
        })?;

        // 获取父目录的Vnode
        let parent_vnode = self
            .get_vnode(parent_path.as_str(), true)
            .await
            .change_context_lazy(|| {
                KernelError::with_message(Errno::EINVAL, "获取父目录Vnode失败")
            })?;

        // 执行创建符号链接操作
        parent_vnode
            .clone()
            .symlink_node(link_name, &VfsPath::from(&target_path))
            .await
            .change_context_lazy(|| KernelError::new(Errno::EIO))
            .attach_printable(format!("创建符号链接 '{}' 指向 '{}' 失败", link_path_str, target_str))?;

        // 如果有缓存，这里应该对父目录的Dentry缓存进行更新
        // TODO: 实现缓存更新逻辑

        Ok(())
    }

    /// 读取符号链接目标路径
    ///
    /// # 参数
    /// - `path_str`: 符号链接路径
    ///
    /// # 返回
    /// 成功则返回符号链接目标路径，失败则返回错误
    pub async fn readlink(&self, path_str: &str) -> VfsResult<VfsPathBuf> {
        // 获取符号链接路径对应的Vnode
        // 注意：指定不跟随符号链接，因为我们要获取符号链接本身
        let symlink_vnode = self.get_vnode(path_str, false).await?;

        // 检查该节点是否为符号链接
        let metadata = symlink_vnode.metadata().await
            .change_context_lazy(|| KernelError::new(Errno::EIO))
            .attach_printable(format!("获取 '{}' 的元数据失败", path_str))?;

        if metadata.kind != crate::types::VnodeType::SymbolicLink {
            return Err(report!(KernelError::new(Errno::EINVAL,))
                .attach_printable(format!("'{}' 不是符号链接", path_str)));
        }

        // 读取符号链接目标
        symlink_vnode
            .clone()
            .readlink()
            .await
            .change_context_lazy(|| KernelError::new(Errno::EIO))
            .attach_printable(format!("读取符号链接 '{}' 失败", path_str))
    }

    /// 设置文件元数据
    ///
    /// # 参数
    /// - `path_str`: 要设置元数据的文件路径
    /// - `changes`: 要应用的元数据更改
    /// - `follow_symlink`: 是否跟随符号链接
    ///
    /// # 返回
    /// 成功则返回Ok(())，失败则返回错误
    pub async fn set_metadata(
        &self,
        path_str: &str,
        changes: crate::types::VnodeMetadataChanges,
        follow_symlink: bool,
    ) -> VfsResult<()> {
        // 获取路径对应的Vnode
        let vnode = self.get_vnode(path_str, follow_symlink).await?;

        // 设置元数据
        vnode
            .clone()
            .set_metadata(changes)
            .await
            .change_context_lazy(|| KernelError::new(Errno::EIO))
            .attach_printable(format!("设置 '{}' 的元数据失败", path_str))?;

        // 如果有缓存，这里应该对Vnode缓存进行更新
        // TODO: 实现缓存更新逻辑

        Ok(())
    }

    /// 读取目录内容
    ///
    /// # 参数
    /// - `path_str`: 目录路径
    ///
    /// # 返回
    /// 成功则返回目录条目列表，失败则返回错误
    pub async fn readdir(&self, path_str: &str) -> VfsResult<Vec<crate::types::DirectoryEntry>> {
        // 获取目录路径对应的Vnode
        let dir_vnode = self.get_vnode(path_str, true).await?;

        // 检查该节点是否为目录
        let metadata = dir_vnode.metadata().await
            .change_context_lazy(|| KernelError::new(Errno::EIO))
            .attach_printable(format!("获取 '{}' 的元数据失败", path_str))?;

        if metadata.kind != crate::types::VnodeType::Directory {
            return Err(report!(KernelError::new(Errno::ENOTDIR,))
                .attach_printable(format!("'{}' 不是目录", path_str)));
        }

        // 打开目录句柄
        let dir_handle = dir_vnode
            .clone()
            .open_dir_handle(crate::types::OpenFlags::RDONLY)
            .await
            .change_context_lazy(|| KernelError::new(Errno::EIO))
            .attach_printable(format!("打开目录 '{}' 失败", path_str))?;

        // 读取目录内容
        let mut entries = Vec::new();
        let mut entry_opt = dir_handle.clone().readdir().await
            .change_context_lazy(|| KernelError::new(Errno::EIO))
            .attach_printable(format!("读取目录 '{}' 内容失败", path_str))?;

        // 收集所有目录条目
        while let Some(entry) = entry_opt {
            entries.push(entry.clone());
            entry_opt = dir_handle.clone().readdir().await
                .change_context_lazy(|| KernelError::new(Errno::EIO))
                .attach_printable(format!("读取目录 '{}' 内容失败", path_str))?;
        }

        // 关闭目录句柄
        dir_handle.close().await
            .change_context_lazy(|| KernelError::new(Errno::EIO))
            .attach_printable(format!("关闭目录 '{}' 失败", path_str))?;

        Ok(entries)
    }
}
