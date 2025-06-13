//! 路径解析模块
//!
//! 实现VFS的路径解析逻辑，包括查找挂载点、解析符号链接、组件遍历等。
//! 本模块主要提供VfsManager的路径解析功能的支持方法。

use alloc::{boxed::Box, format, string::ToString, sync::Arc};

use nexus_error::error_stack::{bail, report, ResultExt};

use crate::{
    manager::VfsManager,
    path::{VfsPath, VfsPathBuf},
    traits::AsyncVnode,
    types::VnodeType,
    verror::{Errno, KernelError, VfsResult},
};

impl VfsManager {
    /// 为给定的绝对路径查找对应的挂载点
    ///
    /// # 参数
    /// - `self_arc`: 对VfsManager的强引用
    /// - `absolute_path`: 要查找的绝对路径
    ///
    /// # 返回
    /// 如果找到匹配的挂载点，返回(文件系统实例, 剩余路径, 挂载点路径)；否则返回None
    pub(crate) async fn find_mount_point_details_for_path(
        self_arc: Arc<Self>,
        absolute_path: &VfsPath<'_>,
    ) -> VfsResult<
        Option<(
            Arc<dyn AsyncFileSystem + Send + Sync>, // 文件系统实例
            VfsPathBuf,                             // 剩余路径（相对于挂载点）
            VfsPathBuf,                             // 挂载点路径
        )>,
    > {
        // 验证路径是绝对路径
        if !absolute_path.is_absolute() {
            bail!(KernelError::with_message(
                Errno::EINVAL,
                "路径必须是绝对路径"
            ));
        }

        // 获取挂载表
        let mount_table = self_arc.mount_table.read().await;

        // 查找最长匹配的挂载点
        // [TODO]: 性能优化点，当前遍历HashMap
        let mut best_match: Option<(VfsPathBuf, MountEntry)> = None;
        let mut best_match_len = 0;

        for (mount_point, entry) in mount_table.iter() {
            if absolute_path.starts_with(mount_point.into()) {
                let mount_point_str = mount_point.as_str();
                let len = mount_point_str.len();

                if len > best_match_len {
                    best_match = Some((mount_point.clone(), entry.clone()));
                    best_match_len = len;
                }
            }
        }

        // 如果找到匹配的挂载点
        if let Some((mount_point, entry)) = best_match {
            // 计算剩余路径（相对于挂载根）
            let remaining_path = if mount_point.as_str() == "/" {
                // 根挂载点特殊处理
                absolute_path.as_str().to_string()
            } else {
                // 移除挂载点前缀
                // 这里假设VfsPathBuf没有as_path方法，用as_str代替
                let stripped = absolute_path.strip_prefix((&mount_point).into()).ok_or_else(|| {
                    report!(KernelError::with_message(Errno::EINVAL, "无法分离路径前缀"))
                })?;

                // 确保路径以"/"开头
                if stripped.as_str().is_empty() {
                    "/".to_string()
                } else {
                    format!("/{}", stripped.as_str())
                }
            };

            // 转换为VfsPathBuf
            let remaining_path_buf = VfsPathBuf::new(remaining_path.to_string())?;

            return Ok(Some((entry.fs_instance, remaining_path_buf, mount_point)));
        }

        // 未找到匹配的挂载点
        Ok(None)
    }

    /// 在文件系统内解析路径段
    ///
    /// # 参数
    /// - `self_arc`: 对VfsManager的强引用
    /// - `current_vnode`: 当前节点（起始点）
    /// - `parent_of_current_for_symlink_resolution`: 当前节点的父节点（用于符号链接解析）
    /// - `relative_path`: 要解析的相对路径
    /// - `follow_last_symlink`: 是否跟随最后一个符号链接
    /// - `symlink_depth`: 符号链接递归深度计数
    ///
    /// # 返回
    /// 解析到的最终Vnode
    /// [TODO]: 递归转迭代，以去掉Box::pin()分配
    pub(crate) async fn resolve_path_segments_within_fs(
        self_arc: Arc<Self>,
        mut current_vnode: Arc<dyn AsyncVnode + Send + Sync>,
        parent_of_current_for_symlink_resolution: Arc<dyn AsyncVnode + Send + Sync>,
        relative_path: &VfsPath<'_>,
        follow_last_symlink: bool,
        symlink_depth: &mut u32,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        // 限制符号链接递归深度
        const MAX_SYMLINK_DEPTH: u32 = 8;
        if *symlink_depth > MAX_SYMLINK_DEPTH {
            bail!(KernelError::with_message(Errno::ELOOP, "符号链接递归过深"));
        }

        // 如果路径为空或只是"/"，返回当前节点
        let rel_path_str = relative_path.as_str();
        if rel_path_str.is_empty() || rel_path_str == "/" {
            return Ok(current_vnode);
        }

        // 获取路径组件迭代器
        let mut components = relative_path.components();

        // 处理每个路径组件
        while let Some(component) = components.next() {
            // 跳过空组件和"."
            if component.is_empty() || component == "." {
                continue;
            }

            // 处理".."（上级目录）
            if component == ".." {
                // 初期简化，暂不处理".."
                // [TODO]: 在未来实现中正确处理".."
                bail!(KernelError::with_message(
                    Errno::ENOENT,
                    "暂不支持'..'组件解析"
                ));
            }

            // 检查是否是最后一个组件
            let next_component = components.next();
            let is_last_component = next_component.is_none();
            // 如果有下一个元素，我们需要将其放回去
            if let Some(_nc) = next_component {
                // 这里假设有一个方法可以将元素插入到迭代器前面
                // 在实际代码中需要确保这个方法存在
                // [TODO]: 实现正确的迭代器操作
            }

            // 在缓存中查找目录项
            let component_vnode = if let Some(cached_vnode) = self_arc
                .dentry_cache
                .get(
                    // 使用new方法代替不存在的from_path，并使用fs_type_name替代to_string
                    &VfsPathBuf::new(current_vnode.filesystem().fs_type_name().to_string())?,
                    component,
                )
                .await
            {
                cached_vnode
            } else {
                // 缓存未命中，在当前目录中查找组件
                let vnode = current_vnode.clone().lookup(component).await.map_err(|_| {
                    report!(KernelError::new(Errno::ENOENT)).attach_printable(format!(
                        "在{}中查找组件{}失败",
                        current_vnode.filesystem().fs_type_name(),
                        component
                    ))
                })?;

                // 将结果添加到缓存
                // [TODO]: 实现缓存添加逻辑

                vnode
            };

            // 获取组件节点的元数据
            let metadata = component_vnode
                .metadata()
                .await
                .map_err(|_| KernelError::with_message(Errno::EIO, "获取节点元数据失败"))?;

            // 处理符号链接
            if metadata.kind == VnodeType::SymbolicLink
                && (follow_last_symlink || !is_last_component)
            {
                // 增加符号链接递归深度计数
                *symlink_depth += 1;

                // 读取符号链接目标
                let link_target = component_vnode
                    .clone()
                    .readlink()
                    .await
                    .map_err(|_| KernelError::with_message(Errno::EIO, "读取符号链接失败"))?;

                // 处理符号链接目标
                let link_target_path: VfsPath<'_> = (&link_target).into();
                if link_target_path.is_absolute() {
                    // 如果是绝对路径，从全局路径解析
                    return self_arc
                        .get_vnode_from_global_path_internal(
                            &link_target_path,
                            follow_last_symlink,
                            symlink_depth,
                        )
                        .await
                        .change_context_lazy(|| KernelError::with_message(Errno::EIO, "解析符号链接失败"));
                } else {
                    // 如果是相对路径，从当前目录解析
                    // 构建剩余路径
                    let mut remaining_path = link_target;

                    for comp in components {
                        remaining_path = VfsPath::from(&remaining_path).join(comp)?;
                    }

                    // 在当前目录上下文中解析相对路径
                    let remaining_path_vfs: VfsPath<'_> = (&remaining_path).into();
                    return Box::pin(Self::resolve_path_segments_within_fs(
                        self_arc,
                        parent_of_current_for_symlink_resolution.clone(),
                        parent_of_current_for_symlink_resolution,
                        &remaining_path_vfs,
                        follow_last_symlink,
                        symlink_depth,
                    ))
                    .await
                    .change_context_lazy(|| KernelError::with_message(Errno::EIO, "解析路径失败"));
                }
            }

            // 如果不是符号链接或者是最后一个组件且不需要跟随符号链接
            current_vnode = component_vnode;
        }

        // 返回解析到的最终节点
        Ok(current_vnode)
    }

    /// 从全局路径获取Vnode的内部实现
    ///
    /// # 参数
    /// - `self`: 对VfsManager的强引用
    /// - `path`: 要解析的路径
    /// - `follow_last_symlink`: 是否跟随最后一个符号链接
    /// - `symlink_depth`: 符号链接递归深度计数
    ///
    /// # 返回
    /// 解析到的Vnode
    pub(crate) async fn get_vnode_from_global_path_internal(
        self: Arc<Self>,
        path: &VfsPath<'_>,
        follow_last_symlink: bool,
        symlink_depth: &mut u32,
    ) -> VfsResult<Arc<dyn AsyncVnode + Send + Sync>> {
        // 查找对应的挂载点
        let (fs_instance, remaining_path, _) =
            Self::find_mount_point_details_for_path(self.clone(), path)
                .await
                .change_context_lazy(|| KernelError::with_message(Errno::ENOENT, "查找挂载点失败"))?
                .ok_or_else(|| {
                    report!(KernelError::new(Errno::ENOENT,))
                        .attach_printable(format!("找不到路径'{}'的挂载点", path.as_str()))
                })?;

        // 获取文件系统根Vnode
        let root_vnode = fs_instance
            .root_vnode()
            .await
            .change_context_lazy(|| KernelError::with_message(Errno::EIO, "获取文件系统根节点失败"))?;

        // 在文件系统内解析剩余路径
        let remaining_path_vfs: VfsPath<'_> = (&remaining_path).into();
        Box::pin(Self::resolve_path_segments_within_fs(
            self.clone(),
            root_vnode.clone(),
            root_vnode,
            &remaining_path_vfs,
            follow_last_symlink,
            symlink_depth,
        ))
        .await
        .change_context_lazy(|| KernelError::with_message(Errno::EIO, "解析路径失败"))
    }
}

/// MountEntry类型（用于导入）
use crate::manager::MountEntry;
/// 用于VfsManager的路径解析工具
use crate::traits::AsyncFileSystem;
