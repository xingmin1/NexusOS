use alloc::sync::Arc;

use alloc::vec;
use alloc::vec::Vec;
use ostd::sync::GuardRwArc;

use crate::thread::ThreadSharedInfo;
use crate::thread::id;

/// 线程组 —— 等价于 Linux 概念里的 *process*。
pub struct ThreadGroup {
    /// 线程组 id == 组长线程的 tid
    id: u64,
    /// 成员列表
    members: GuardRwArc<Vec<Arc<ThreadSharedInfo>>>,
}

impl ThreadGroup {
    /// 创建新线程组，并把 `leader` 放进去。
    pub fn new_leader(leader: Arc<ThreadSharedInfo>) -> Arc<Self> {
        let id = leader.tid; // Linux: tgid = leader tid
        Arc::new(Self {
            id,
            members: GuardRwArc::new(vec![leader]),
        })
    }

    /// 加入现有线程组
    pub fn attach(&self, thr: Arc<ThreadSharedInfo>) {
        self.members.write().push(thr);
    }

    pub fn id(&self) -> u64 {
        self.id
    }
}

/// 为 *fork*（非 CLONE_THREAD）预留：生成新 thread‑group id。
/// 当前实现沿用 tid 分配器，简化 id 管理。
pub fn alloc_tgid() -> u64 {
    id::alloc()
}
