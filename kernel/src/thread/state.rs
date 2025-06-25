use core::{
    sync::atomic::{AtomicI32, AtomicU8, Ordering},
};
use ostd::sync::WaitQueue;

/// 线程可观察到的生命周期状态
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LifeState {
    Running = 0,
    Zombie  = 1, // 等待被 wait4 收尸
}

/// 线程生命周期
pub struct Lifecycle {
    state:           AtomicU8,     // LifeState
    exit_code:       AtomicI32,    // 正常退出码或信号编码
    exit_wait_queue: WaitQueue,    // 供父线程 wait4
}

impl Lifecycle {
    pub fn new() -> Self {
        Self {
            state:     AtomicU8::new(LifeState::Running as u8),
            exit_code: AtomicI32::new(0),
            exit_wait_queue: WaitQueue::new(),
        }
    }

    /// 主动退出：设置退出码并广播
    pub fn exit(&self, code: i32) {
        self.exit_code.store(code, Ordering::Release);
        self.state.store(LifeState::Zombie as u8, Ordering::Release);
        self.exit_wait_queue.wake_all();
    }

    /// 阻塞等待子线程退出；返回退出码
    pub async fn wait(&self) -> i32 {
        if self.state.load(Ordering::Acquire) == LifeState::Zombie as u8 {
            return self.exit_code.load(Ordering::Acquire);
        }
        let _ = self.exit_wait_queue.wait().await;
        self.exit_code.load(Ordering::Acquire)
    }

    pub fn is_zombie(&self) -> bool {
        self.state.load(Ordering::Acquire) == LifeState::Zombie as u8
    }
}

impl Clone for Lifecycle {
    fn clone(&self) -> Self {
        let state = self.state.load(Ordering::Acquire);
        let exit_code = self.exit_code.load(Ordering::Acquire);
        Self {
            state:     AtomicU8::new(state),
            exit_code: AtomicI32::new(exit_code),
            exit_wait_queue: WaitQueue::new(),
        }
    }
}

