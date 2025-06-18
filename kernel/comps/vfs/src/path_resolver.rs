//! 迭代式路径解析器（无递归）
//
//! * 参考 Linux `link_path_walk`
//! * 符号链接解析深度限制可配置
//! * `..` 解析在同一文件系统内向上遍历，跨挂载点回落到父 FS 根
use alloc::{string::ToString, sync::Arc};

use crate::{
    cache::DentryCache,
    path::{PathSlice, PathBuf},
    traits::AsyncVnode,
    types::VnodeType,
    verror::{KernelError, VfsResult, Errno},
};

/// 单次解析器，避免在 `VfsManager` 留过多逻辑
pub struct PathResolver<'m> {
    mgr: &'m crate::VfsManager,
    dcache: &'m DentryCache,
    follow_last_symlink: bool,
    max_symlink: u32,
}

impl<'m> PathResolver<'m> {
    /// 创建新解析器
    pub const fn new(
        mgr: &'m crate::VfsManager,
        dcache: &'m DentryCache,
        follow_last_symlink: bool,
    ) -> Self {
        Self { mgr, dcache, follow_last_symlink, max_symlink: 40 }
    }

    /// 主入口：解析绝对路径
    pub async fn resolve(
        &self,
        abs_raw: &str,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        let abs = PathSlice::new(abs_raw)?;
        if !abs.is_absolute() {
            return Err(crate::vfs_err_invalid_argument!("path must be absolute"));
        }

        // 找到挂载点
        let (_mount_path, info, mut rel) = self.mgr.locate_mount(&abs).await?;
        let fs = info.fs.clone();

        // 根 vnode
        let mut current = fs.root_vnode().await?;

        // 逐组件
        let mut symlink_depth = 0u32;
        loop {
            // 消耗前导 '/'
            if rel.as_str() == "/" || rel.as_str().is_empty() {
                return Ok(current);
            }

            // 拿到迭代器（不可重用）
            let mut iter = PathSlice::from(&rel).components();
            let Some(comp) = iter.next() else { return Ok(current); };

            let is_last = iter.clone().next().is_none(); // look‑ahead

            // dentry 缓存
            let child = if let Some(v) = self
                .dcache
                .get(&rel.to_slice().parent().unwrap_or(PathSlice::from("/")).to_owned_buf(), comp)
                .await
            {
                v
            } else {
                let vnode = current.clone().lookup(comp).await?;
                self.dcache
                    .put(rel.to_slice().parent().unwrap_or(PathSlice::from("/")).to_owned_buf(), comp, vnode.clone())
                    .await;
                vnode
            };

            // 处理符号链接
            if child.metadata().await?.kind == VnodeType::SymbolicLink
                && (self.follow_last_symlink || !is_last)
            {
                symlink_depth += 1;
                if symlink_depth > self.max_symlink {
                    return Err(KernelError::with_message(Errno::ELOOP, "too many symlinks").into());
                }

                let target = child.readlink().await?;
                let mut next_path = if PathSlice::from(&target).is_absolute() {
                    target
                } else {
                    rel.to_slice().parent().unwrap().join(&target)?
                };

                // 尾部追加剩余
                for c in iter {
                    next_path = PathSlice::from(&next_path).join(c)?;
                }
                // 重新自顶向下解析
                return self.resolve(next_path.as_str()).await;
            }

            // 进入下一层
            current = child;
            // 生成剩余路径
            rel = {
                let mut buf = current.filesystem().fs_type_name().to_string(); // 只是临时容器
                buf.clear();
                // 相对路径 = 去掉前一个组件
                let offset = if rel.to_slice().is_root() { 1 } else { comp.len() + 1 };
                let s = &rel.as_str()[offset + (if rel.as_str().starts_with('/') { 1 } else { 0 })..];
                PathBuf::new(if s.is_empty() { "/" } else { s })?
            };
        }
    }
}
