//! VFS 核心管理器
//!
//! * 只关心 “**资源目录**” 的维护；不再直接解析路径。
//! * 各子组件职责单一，可独立测试。
use core::{future::AsyncDrop, pin::Pin};

use alloc::{
    collections::btree_map::BTreeMap as Map, string::String as AString, sync::{Arc, Weak}
};

use nexus_error::{error_stack::ResultExt, with_pos};
use ostd::sync::{Mutex, RwLock};

use crate::{
    cache::{DentryCache, VnodeCache}, path::{PathBuf, PathSlice}, static_dispatch::{filesystem::SFileSystem, provider::SProvider}, traits::AsyncBlockDevice, types::{FsOptions, MountId}, verror::{Errno, KernelError, VfsResult}
};

/// **文件系统提供者注册表** —— 写少读多，单锁即可
struct ProviderRegistry {
    inner: RwLock<Map<AString, SProvider>>,
}
impl ProviderRegistry {
    fn new(initial: Map<AString, SProvider>) -> Self {
        Self { inner: RwLock::new(initial) }
    }
    async fn get(&self, ty: &str) -> Option<SProvider> {
        self.inner.read().await.get(ty).cloned()
    }
}

/// **挂载点信息**
#[derive(Clone)]
pub struct MountInfo {
    pub id: MountId,
    pub fs: SFileSystem,
}

/// **挂载点注册表** —— 需要最长前缀匹配
struct MountRegistry {
    table: RwLock<Map<PathBuf, MountInfo>>,
}
impl MountRegistry {
    fn new() -> Self { Self { table: RwLock::new(Map::new()) } }

    /// 长前缀匹配（只读快路径）
    async fn longest_match(&self, abs: PathSlice<'_>) -> Option<(PathBuf, MountInfo)> {
        self.table.read().await.iter()
               .filter(|(p, _)| abs.starts_with(PathSlice::from(*p)))
               .max_by_key(|(p, _)| p.as_str().len())
               .map(|(p, m)| (p.clone(), m.clone()))
    }
    async fn insert(&self, path: PathBuf, info: MountInfo) { self.table.write().await.insert(path, info); }
    async fn remove_by_id(&self, id: MountId) -> Option<(PathBuf, MountInfo)> {
        let mut m = self.table.write().await;
        let key = m.iter().find(|(_, v)| v.id == id).map(|(k, _)| k.clone())?;
        m.remove_entry(&key)
    }
}

/// **ID池（RAII 保护）**
struct IdPool {
    inner: Mutex<id_alloc::IdAlloc>,
}
impl IdPool {
    fn new(cap: usize) -> Self { Self { inner: Mutex::new(id_alloc::IdAlloc::with_capacity(cap)) } }

    async fn alloc(&self) -> VfsResult<IdGuard<'_>> {
        let mut g = self.inner.lock().await;
        let id = g.alloc().ok_or_else(|| KernelError::with_message(Errno::ENOMEM, "id pool"))?;
        Ok(IdGuard { pool: self, id })
    }
    async fn free(&self, id: usize) { self.inner.lock().await.free(id); }
}
struct IdGuard<'p> {
    pool: &'p IdPool,
    id: usize,
}
impl AsyncDrop for IdGuard<'_> {
    type Dropper<'a> = impl core::future::Future<Output = ()> 
    where Self: 'a;
    fn async_drop(self: Pin<&mut Self>) -> Self::Dropper<'_>{
        self.pool.free(self.id)
    }
}
impl core::ops::Deref for IdGuard<'_> { type Target = usize; fn deref(&self) -> &Self::Target { &self.id } }

/// **VfsManager：协调各 Registry / Cache**
pub struct VfsManager {
    providers: ProviderRegistry,
    mounts: MountRegistry,
    mount_id_pool: IdPool,
    fs_id_pool: IdPool,
    pub vnode_cache: Arc<VnodeCache>,
    pub dentry_cache: Arc<DentryCache>,
    self_arc: Weak<Self>,
}

impl VfsManager {
    /// 构造器
    pub fn builder() -> VfsManagerBuilder { VfsManagerBuilder::default() }

    /// 根据绝对路径获得 `(挂载点, MountInfo, 余下路径)`；只读，无锁递归
    pub async fn locate_mount(&self, abs: PathSlice<'_>) -> VfsResult<(PathBuf, MountInfo, PathBuf)> {
        let (mpath, minfo) = self.mounts.longest_match(abs)
            .await
            .ok_or_else(|| KernelError::with_message(Errno::ENOENT, "no mount"))?;
        let rest = abs.strip_prefix(PathSlice::from(&mpath))
                      .unwrap_or(PathSlice::from("."))
                      .to_owned_buf();
        Ok((mpath, minfo, rest))
    }

    /* ---------- 挂载 / 卸载 ---------- */

    /// 挂载文件系统
    pub async fn mount(
        &self,
        dev: Option<Arc<dyn AsyncBlockDevice + Send + Sync>>,
        target: &str,
        ty: &str,
        opts: FsOptions,
    ) -> VfsResult<MountId> {
        // 0. 路径校验
        let target_path = PathBuf::new(target)?;
        if !PathSlice::from(&target_path).is_absolute() {
            return Err(KernelError::with_message(Errno::EINVAL, "mount path must be absolute").into());
        }
        if self.mounts.longest_match(PathSlice::from(&target_path)).await.map_or(false, |(p, _)| p == target_path) {
            return Err(KernelError::with_message(Errno::EINVAL, "already mounted").into());
        }

        // 1. provider
        let prov = self.providers.get(ty)
            .await
            .ok_or_else(|| KernelError::with_message(Errno::ENOENT, "provider not found"))?;

        // 2. 预分配 ID，使用 RAII 失败即回滚
        let mount_id = self.mount_id_pool.alloc().await?;
        let fs_id    = self.fs_id_pool.alloc().await?;

        // 3. 调用 provider.mount
        let fs = prov.mount(dev, &opts, *mount_id, *fs_id).await
            .attach_printable_lazy(|| with_pos!("fs mount failed"))?;

        // 4. 更新表；到此 IDGuard 转移所有权，防止提前释放
        self.mounts.insert(target_path.clone(), MountInfo { id: *mount_id, fs }).await;

        // 5. 释放 Guard → 不释放 ID
        core::mem::forget(mount_id);
        core::mem::forget(fs_id);
        Ok(self.mounts.longest_match(PathSlice::from(&target_path)).await.unwrap().1.id)
    }

    /// 卸载
    pub async fn unmount(&self, id: MountId) -> VfsResult<()> {
        let (_, info) = self.mounts.remove_by_id(id)
            .await
            .ok_or_else(|| KernelError::with_message(Errno::EINVAL, "mount id invalid"))?;
        info.fs.prepare_unmount().await?;
        self.mount_id_pool.free(id).await;
        self.fs_id_pool.free(info.fs.id()).await;
        Ok(())
    }

    /* ---------- 辅助 ---------- */

    /// 获取自身 `Arc`
    pub fn self_arc(&self) -> VfsResult<Arc<Self>> {
        self.self_arc.upgrade().ok_or_else(|| KernelError::with_message(Errno::EINVAL, "manager gone").into())
    }
}

/* ---------- Builder ---------- */

#[derive(Default)]
pub struct VfsManagerBuilder {
    providers: Map<AString, SProvider>,
    vnode_cap: usize,
    dentry_cap: usize,
}
impl VfsManagerBuilder {
    pub fn provider(mut self, p: SProvider) -> Self {
        self.providers.insert(p.fs_type_name().into(), p); self
    }
    pub fn vnode_cache(mut self, cap: usize) -> Self { self.vnode_cap = cap; self }
    pub fn dentry_cache(mut self, cap: usize) -> Self { self.dentry_cap = cap; self }
    pub fn build(self) -> Arc<VfsManager> {
        const DFLT_VNODE: usize = 1024;
        const DFLT_DENTRY: usize = 4096;
        Arc::new_cyclic(|weak| VfsManager {
            providers: ProviderRegistry::new(self.providers),
            mounts: MountRegistry::new(),
            mount_id_pool: IdPool::new(1024),
            fs_id_pool: IdPool::new(1024),
            vnode_cache: Arc::new(VnodeCache::new(if self.vnode_cap == 0 { DFLT_VNODE } else { self.vnode_cap })),
            dentry_cache: Arc::new(DentryCache::new(if self.dentry_cap == 0 { DFLT_DENTRY } else { self.dentry_cap })),
            self_arc: weak.clone(),
        })
    }
}
