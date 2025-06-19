//! 迭代式路径解析器：零递归、零 Box。
//!
//! 算法：外层 loop 维护“尚未解析完的绝对路径”；
//!       walk_one_mount() 负责在当前挂载内遍历组件；
//!       一旦遇到需跟随的符号链接，立即返回新绝对路径，外层 loop 重启。
//!
//! 性能：单次遍历 O(|path|)；遇到 n 个链接最多循环 n+1 次。
//! 锁顺序：dcache → vnode

use crate::{
    cache::DentryCache, path::{PathBuf, PathSlice}, static_dispatch::vnode::SVnode, types::VnodeType, verror::{Errno, KernelError, VfsResult}
};

/// 每次 walk 的结果
enum Step {
    /// 到达最终 Vnode
    Done(SVnode),
    /// 解析过程中遇到符号链接，需要以新绝对路径重启
    Restart(PathBuf),
}

/// 单次解析器（不可跨线程共享）
pub struct PathResolver<'m> {
    mgr:        &'m crate::VfsManager,
    dcache:     &'m DentryCache,
    follow_last_symlink: bool,
    max_symlink: u32,
}

impl<'m> PathResolver<'m> {
    /// 构造
    pub const fn new(
        mgr: &'m crate::VfsManager,
        dcache: &'m DentryCache,
        follow_last_symlink: bool,
    ) -> Self {
        Self { mgr, dcache, follow_last_symlink, max_symlink: 40 }
    }

    /// 公开接口：解析绝对路径
    ///
    /// # 参数
    /// * `abs_raw` —— **必须**以 `/` 开头的 UTF‑8 字符串
    pub async fn resolve(
        &self,
        abs_raw: &str,
    ) -> VfsResult<SVnode> {
        // 预规范化
        let mut todo = PathBuf::new(abs_raw)?;
        if !PathSlice::from(&todo).is_absolute() {
            return Err(crate::vfs_err_invalid_argument!("path must be absolute"));
        }

        let mut depth = 0;
        loop {
            if depth > self.max_symlink {
                return Err(KernelError::with_message(Errno::ELOOP, "too many symlinks").into());
            }

            match self.walk_one_mount(&todo).await? {
                Step::Done(v)      => return Ok(v),
                Step::Restart(p) => { todo = p; depth += 1; }
            }
        }
    }

    /// 在 **单一挂载** 内遍历组件；遇到需跟随的符号链接即返回 Restart。
    async fn walk_one_mount(&self, abs: &PathBuf) -> VfsResult<Step> {
        // 1. 锁定挂载信息
        let (_mnt_path, mnt_info, rel) = self.mgr.locate_mount(&abs.into()).await?;
        let fs = mnt_info.fs.clone();
        let mut current = fs.root_vnode().await?;

        // 空 / 根路径直接返回
        if rel.as_str() == "/" || rel.as_str().is_empty() {
            return Ok(Step::Done(current));
        }

        // 2. 遍历组件
        let mut path_prefix = alloc::string::String::from(_mnt_path.as_str());   // 用于构造新绝对路径
        if !path_prefix.ends_with('/') { path_prefix.push('/'); }

        let mut comps = PathSlice::from(&rel).components().peekable();
        while let Some(seg) = comps.next() {
            let is_last = comps.peek().is_none();

            // --- dentry 缓存 ---
            let dir_key = PathSlice::from(&PathBuf::from_str_unchecked(path_prefix.clone()))
                .parent()
                .unwrap_or(PathSlice::from("/"))
                .to_owned_buf();

            let child = if let Some(v) = self.dcache.get(&dir_key, seg).await {
                v
            } else {
                let v = current.to_dir().unwrap().lookup(seg).await?;
                self.dcache.put(dir_key, seg, v.clone()).await;
                v
            };

            // --- 符号链接处理 ---
            if child.metadata().await?.kind == VnodeType::SymbolicLink
                && (self.follow_last_symlink || !is_last)
            {
                let target = child.to_symlink().unwrap().readlink().await?; // 可能是绝对或相对
                // 1. 绝对：直接替换
                // 2. 相对：基于 path_prefix 构造
                let mut new_abs = if PathSlice::from(&target).is_absolute() {
                    target
                } else {
                    let mut s = path_prefix.clone();           // 已含到当前目录
                    s.push_str(target.as_str());
                    PathBuf::new(s)?
                };

                // 将 comps 剩余部分附在尾部
                for rest in comps {
                    new_abs = PathSlice::from(&new_abs).join(rest)?;
                }
                return Ok(Step::Restart(new_abs));
            }

            // 前进
            current = child;
            if !path_prefix.ends_with('/') { path_prefix.push('/'); }
            path_prefix.push_str(seg);
        }
        Ok(Step::Done(current))
    }
}
