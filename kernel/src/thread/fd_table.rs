//! 进程级文件描述符表
//! 
//! * fd 取值 `0‥=OPEN_MAX-1`，默认 `OPEN_MAX = 1<<20`；超限返回 `EMFILE/ENFILE`。

#![allow(unused)]

use alloc::{collections::BTreeMap, sync::Arc};
use nexus_error::{errno_with_message, return_errno_with_message, Errno, Result};
use core::{
    sync::atomic::{AtomicU32, Ordering},
};

use bitflags::bitflags;
use ostd::sync::RwLock;

use vfs::{SDirHandle, SFileHandle};

bitflags! {
    #[derive(Default, Clone, Copy, Debug)]
    pub struct FdFlags: u8 {
        /// `FD_CLOEXEC` —— exec 成功后自动关闭
        const CLOEXEC = 0b0000_0001;
    }
}

#[derive(Clone)]
pub enum FdObject {
    File(Arc<SFileHandle>),
    Dir (Arc<SDirHandle>),
}

impl FdObject {
    /// 简易类型判定，方便 syscall 分派
    pub fn is_file(&self) -> bool {
        matches!(self, Self::File(_))
    }
}

/// 表项：对象 + 描述符级 flag
#[derive(Clone)]
pub struct FdEntry {
    pub obj:   FdObject,
    pub flags: FdFlags,
}

impl FdEntry {
    pub fn new(obj: FdObject, flags: FdFlags) -> Self {
        Self { obj, flags }
    }
}

/// 默认最大同时打开文件数（软上限）
const OPEN_MAX: usize = 1 << 20; // 约 1 M，后续可由 RLIMIT_NOFILE 替代

pub struct FdTable {
    map:      RwLock<BTreeMap<u32, FdEntry>>,
    next:     AtomicU32,
    capacity: usize,
}

impl FdTable {
    /// capacity==0 取 OPEN_MAX
    pub const fn new(capacity: usize) -> Self {
        Self {
            map:      RwLock::new(BTreeMap::new()),
            next:     AtomicU32::new(0),
            capacity: if capacity == 0 { OPEN_MAX } else { capacity },
        }
    }

    /// 为 `entry` 分配描述符；`min_fd` 为搜索起点
    pub async fn alloc(
        &self,
        entry: FdEntry,
        min_fd: u32,
    ) -> Result<u32> {
        let mut tbl = self.map.write().await;

        if tbl.len() >= self.capacity {
            return_errno_with_message!(Errno::EMFILE, "fd table full");
        }

        // 从 max(min_fd, next) 开始扫空洞
        let mut fd = self.next.load(Ordering::Relaxed).max(min_fd);
        while tbl.contains_key(&fd) {
            fd = fd.checked_add(1)
                   .ok_or_else(|| errno_with_message(Errno::EMFILE, "fd overflow"))?;
            if (fd as usize) >= self.capacity {
                return_errno_with_message!(Errno::EMFILE, "fd table full");
            }
        }
        tbl.insert(fd, entry);
        self.next.store(fd + 1, Ordering::Relaxed);
        Ok(fd)
    }

    /// 只读获取；克隆表项以避免持锁过久
    pub async fn get(&self, fd: u32) -> Result<FdEntry> {
        let tbl = self.map.read().await;
        tbl.get(&fd)
            .cloned()
            .ok_or_else(|| errno_with_message(Errno::EBADF, "bad fd"))
    }

    /// 可写获取（需独占锁）
    pub async fn get_entry_mut<F, R>(&self, fd: u32, f: F) -> Result<R>
    where
        F: FnOnce(&mut FdEntry) -> R,
    {
        let mut tbl = self.map.write().await;
        let ent = tbl
            .get_mut(&fd)
            .ok_or_else(|| errno_with_message(Errno::EBADF, "bad fd"))?;
        Ok(f(ent))
    }

    /// 关闭并移除条目
    pub async fn close(&self, fd: u32) -> Result<()> {
        let mut tbl = self.map.write().await;
        tbl.remove(&fd)
            .map(|_| ())
            .ok_or_else(|| errno_with_message(Errno::EBADF, "bad fd"))
    }

    /// dup/dup2/dup3 语义合并实现  
    /// * `min_fd` : `dup()` 传 `0`；`dup2` 传 `newfd`；`dup3` 同时带 `cloexec`
    pub async fn dup(
        &self,
        oldfd: u32,
        min_fd: u32,
        set_cloexec: bool,
    ) -> Result<u32> {
        let entry = self.get(oldfd).await?; // 克隆
        let mut cloned = entry.clone();
        if set_cloexec {
            cloned.flags.insert(FdFlags::CLOEXEC);
        }
        // dup2 语义：若 old==new 直接返回
        if oldfd == min_fd {
            self.get_entry_mut(oldfd, |_| ()).await?;
            // 更新 flags（dup3 可改变 cloexec）
            if set_cloexec {
                self.get_entry_mut(oldfd, |e| e.flags.insert(FdFlags::CLOEXEC)).await?;
            }
            return Ok(oldfd);
        }

        // 若 newfd 已存在，需要先关闭
        if self.map.read().await.contains_key(&min_fd) {
            self.close(min_fd).await?;
        }
        self.alloc(cloned, min_fd).await
    }

    /// 复制 FD 表
    /// 深拷贝：复制条目，fd 计数独立
    pub async fn dup_table(&self) -> Arc<Self> {
        let snapshot = self.map.read().await.clone();
        Arc::new(Self {
            map:      RwLock::new(snapshot),
            next:     AtomicU32::new(self.next.load(Ordering::Relaxed)),
            capacity: self.capacity,
        })
    }

    /// 更新 flags
    pub async fn set_flags(&self, fd: u32, flags: FdFlags) -> Result<()> {
        self.get_entry_mut(fd, |e| e.flags = flags).await
    }

    /// execve 前调用：批量关闭带 FD_CLOEXEC 的描述符
    pub async fn clear_cloexec_on_exec(&self) {
        let mut tbl = self.map.write().await;
        tbl.retain(|_, ent| {
            let keep = !ent.flags.contains(FdFlags::CLOEXEC);
            keep
        });
        // 重置 next 以便快速复用低位 fd
        if let Some((&min_fd, _)) = tbl.first_key_value() {
            self.next.store(min_fd, Ordering::Relaxed);
        } else {
            self.next.store(0, Ordering::Relaxed);
        }
    }
}
