// SPDX-License-Identifier: MPL-2.0
//
// Copyright (c) 2024 The NexusOS Project Contributors
//
// This file originates from `maitake/src/scheduler.rs` in the `mycelium` project,
// Copyright (c) 2022 Eliza Weisman, which is licensed under the MIT License.
// A copy of the MIT License for the original code can be found in the
// `ostd/libs/maitake/LICENSE` file.
//
// This file has been modified by The NexusOS Project Contributors.
// As part of NexusOS, this file is licensed under the Mozilla Public License v2.0.
// You may obtain a copy of MPL-2.0 at https://mozilla.org/MPL/2.0/.

//! 内核任务调度器模块。
//!
//! 该模块实现了基于 `maitake` 库的异步任务调度器。
//! 主要包含以下功能：
//! - `Core` 结构体：代表单个 CPU 核心的运行时，负责管理该核心的任务调度和执行。
//! - `Runtime` 结构体：管理所有核心的调度器实例和全局任务注入器。
//! - 任务生成 (`spawn`) 和执行 (`run`, `tick`)。
//! - 工作窃取 (`try_steal`) 机制以实现负载均衡。
//! - 任务抢占 (`might_preempt`) 和让出 (`yield_now`)。
//! - 全局定时器 (`TIMER`) 初始化和管理。
use alloc::sync::Arc;
use core::{
    cell::Cell,
    cmp,
    fmt::Debug,
    future::Future,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering::*},
};

use log::{debug, info};
use maitake::{
    scheduler::{self, StaticScheduler, Stealer},
    sync::spin::InitOnce,
    task::JoinHandle,
};
use rand::Rng;
use rand_xoshiro::rand_core::SeedableRng;

use super::{disable_preempt, preempt::cpu_local, Task};
use crate::{
    cpu::{CpuId, PinCurrentCpu},
    cpu_local,
};

/// 单核内核运行时。
pub struct Core {
    /// 此核心的任务调度器。
    scheduler: &'static StaticScheduler,

    /// 此核心的ID。
    ///
    /// ID 0 是系统启动时启动的第一个 CPU 核心。
    id: usize,

    /// 如果此核心应关闭，则设置为 `false`。
    // running: AtomicBool,

    /// 用于选择下一个要窃取工作的核心的索引。
    ///
    /// 在工作窃取时选择一个随机的核心索引有助于确保不会出现
    /// 所有空闲核心都从第一个可用工作者窃取的情况，
    /// 导致其他核心最终拥有庞大的空闲任务队列，而第一个核心的队列始终为空。
    ///
    /// 这*不是*一个密码学安全的随机数生成器，因为
    /// 此值的随机性并非安全所需。相反，它只是
    /// 有助于确保良好的负载分布。因此，我们使用一个快速的、
    /// 非密码学的随机数生成器（RNG）。
    rng: rand_xoshiro::Xoroshiro128PlusPlus,
}

struct Runtime {
    cores: [InitOnce<StaticScheduler>; MAX_CORES],

    /// 用于在任何 `Core` 实例上生成任务的全局注入器队列。
    injector: scheduler::Injector<&'static StaticScheduler>,
    initialized: AtomicUsize,
}

/// 最多支持 512 个 CPU 核心，应该够用了吧...
pub const MAX_CORES: usize = 512;

static TIMER: InitOnce<maitake::time::Timer> = InitOnce::uninitialized();

static RUNTIME: Runtime = {
    const UNINIT_SCHEDULER: InitOnce<StaticScheduler> = InitOnce::uninitialized();

    Runtime {
        cores: [UNINIT_SCHEDULER; MAX_CORES],
        initialized: AtomicUsize::new(0),
        injector: {
            static STUB_TASK: scheduler::TaskStub = scheduler::TaskStub::new();
            unsafe { scheduler::Injector::new_with_static_stub(&STUB_TASK) }
        },
    }
};

/// 在 Mycelium 的全局运行时上生成一个任务。
pub fn spawn<F>(future: F, ostd_task_ptr: Option<usize>) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    SCHEDULER.with(|scheduler| {
        if let Some(scheduler) = scheduler.get() {
            let task_builder = scheduler.build_task();
            if let Some(ostd_task_ptr) = ostd_task_ptr {
                task_builder.ostd_task_ptr(ostd_task_ptr).spawn(future)
            } else {
                task_builder.spawn(future)
            }
        } else {
            // 此核心上没有调度器在运行。
            RUNTIME.injector.spawn(future, ostd_task_ptr)
        }
    })
}

/// 初始化内核运行时。
pub fn init(clock: maitake::time::Clock) {
    info!(
        "clock = {:?}, clock_max_duration = {:?}, initializing kernel runtime...",
        clock.name(),
        clock.max_duration(),
    );
    let timer = TIMER.init(maitake::time::Timer::new(clock));
    maitake::time::set_global_timer(timer).expect("`rt::init` should only be called once!");

    info!("kernel runtime initialized");
}

cpu_local! {
    static SCHEDULER: Cell<Option<&'static StaticScheduler>> = Cell::new(None);
}

cpu_local! {
    static RUNNING: AtomicBool = AtomicBool::new(false);
}

/// 如果此核心当前正在运行，则停止它。
///
/// # 返回
///
/// - 如果此核心正在运行并且现在正在停止，则返回 `true`
/// - 如果此核心未运行，则返回 `false`。
pub fn stop_running() -> bool {
    RUNNING.with(|running| {
        let was_running = running
        .compare_exchange(
            true,
            false,
            core::sync::atomic::Ordering::AcqRel,
            core::sync::atomic::Ordering::Acquire,
        )
        .is_ok();
        was_running
    })
}

impl Core {
    /// 创建一个新的核心。
    #[must_use]
    pub fn new() -> Self {
        let (id, scheduler) = RUNTIME.new_scheduler();
        info!("core {} initialized task scheduler", id);
        Self {
            scheduler,
            id,
            rng: rand_xoshiro::Xoroshiro128PlusPlus::seed_from_u64(id as u64),
            // running: AtomicBool::new(false),
        }
    }

    /// 在此核心上运行内核主循环的一个 tick。
    ///
    /// 如果此核心有更多工作要做，则返回 `true`，否则返回 `false`。
    pub fn tick(&mut self) -> bool {
        // 驱动任务调度器
        let tick = self.scheduler.tick();

        // 如果计时器轮最近没有转动并且没有其他核心持有锁，则转动它，
        // 确保消耗所有挂起的计时器滴答。
        TIMER.get().turn();

        // 如果还有剩余的任务要轮询，则继续执行而不进行窃取。
        if tick.has_remaining {
            return true;
        }

        // 如果此核心的运行队列中没有剩余任务，尝试从分发器队列窃取新任务。
        let stolen = self.try_steal();
        if stolen > 0 {
            debug!("core {} stole {} tasks", self.id, stolen);
            // 如果我们窃取了任务，我们需要继续 tick
            return true;
        }

        // 如果我们没有剩余的已唤醒任务，并且没有窃取任何新任务，
        // 则此核心可以休眠直到发生中断。
        false
    }

    /// 如果此核心当前正在运行，则返回 `true`。
    #[inline]
    pub fn is_running(&self) -> bool {
        RUNNING.with(|running| {
            running.load(core::sync::atomic::Ordering::Acquire)
        })
    }

    // /// 如果此核心当前正在运行，则停止它。
    // ///
    // /// # 返回
    // ///
    // /// - 如果此核心正在运行并且现在正在停止，则返回 `true`
    // /// - 如果此核心未运行，则返回 `false`。
    // pub fn stop(&self) -> bool {
    //     let was_running = self
    //         .running
    //         .compare_exchange(
    //             true,
    //             false,
    //             core::sync::atomic::Ordering::AcqRel,
    //             core::sync::atomic::Ordering::Acquire,
    //         )
    //         .is_ok();
    //     log::info!("stopping core={}, was_running={}", self.id, was_running);
    //     was_running
    // }

    /// 运行此核心直到 [`Core::stop`] 被调用。
    pub fn run(&mut self) {
        struct CoreGuard;
        impl Drop for CoreGuard {
            fn drop(&mut self) {
                SCHEDULER.with(|scheduler| scheduler.set(None));
            }
        }

        if RUNNING.with(|running| {
            running.compare_exchange(false, true, AcqRel, Acquire).is_err()
        }) {
            log::error!("this core is already running!");
            return;
        }

        SCHEDULER.with(|scheduler| scheduler.set(Some(self.scheduler)));
        let _unset = CoreGuard;

        log::info!("started kernel main loop");

        loop {
            // 持续 tick 调度器，直到它表明没有任务可运行。
            if self.tick() {
                continue;
            }

            // 检查此核心是否应该关闭。
            if !self.is_running() {
                log::info!("stop signal received, shutting down core={}", self.id);
                return;
            }

            // 如果我们没有任务可运行，可以休眠直到发生中断。
            crate::arch::wait_for_interrupt();
        }
    }

    /// 在条件闭包为真时运行此核心。
    pub fn run_while<F>(&mut self, condition: F)
    where
        F: Fn() -> bool,
    {
        struct CoreGuard;
        impl Drop for CoreGuard {
            fn drop(&mut self) {
                SCHEDULER.with(|scheduler| scheduler.set(None));
            }
        }

        if RUNNING.with(|running| {
            running.compare_exchange(false, true, AcqRel, Acquire).is_err()
        }) {
            log::error!("this core is already running!");
            return;
        }

        SCHEDULER.with(|scheduler| scheduler.set(Some(self.scheduler)));
        let _unset = CoreGuard;

        log::info!("started kernel main loop");

        loop {
            if !condition() {
                log::info!("condition is false, shutting down core={}", self.id);
                return;
            }

            // 持续 tick 调度器，直到它表明没有任务可运行。
            if self.tick() {
                continue;
            }

            // 检查此核心是否应该关闭。
            if !self.is_running() {
                log::info!("stop signal received, shutting down core={}", self.id);
                return;
            }

            // 如果我们没有任务可运行，可以休眠直到发生中断。
            crate::arch::wait_for_interrupt();
        }
    }

    fn try_steal(&mut self) -> usize {
        // 如果所有潜在受害者核心的队列都为空或繁忙，不要无限次尝试窃取工作。
        const MAX_STEAL_ATTEMPTS: usize = 16;
        // 任意选择的值！
        const MAX_STOLEN_PER_TICK: usize = 256;

        // 首先，尝试从注入器队列窃取。
        if let Ok(injector) = RUNTIME.injector.try_steal() {
            return injector.spawn_n(&self.scheduler, MAX_STOLEN_PER_TICK);
        }

        // 如果注入器队列为空或有其他核心正在从中窃取，
        // 尝试寻找另一个工作者核心进行窃取。
        let mut attempts = 0;
        while attempts < MAX_STEAL_ATTEMPTS {
            let active_cores = RUNTIME.active_cores();

            // 如果窃取核心是唯一活动的核心，则没有其他核心可供窃取，退出。
            if active_cores <= 1 {
                break;
            }

            // 随机选择一个潜在的受害者核心进行窃取。
            let victim_idx = self.rng.random_range(0..active_cores);

            // 我们不能从自己这里窃取任务。
            if victim_idx == self.id {
                continue;
            }

            // 找到了一个可以窃取的核心
            if let Some(victim) = RUNTIME.try_steal_from(victim_idx) {
                let num_steal = cmp::min(victim.initial_task_count() / 2, MAX_STOLEN_PER_TICK);
                return victim.spawn_n(&self.scheduler, num_steal);
            } else {
                attempts += 1;
            }
        }

        // 如果找不到其他可窃取的核心，再次尝试注入器队列
        if let Ok(injector) = RUNTIME.injector.try_steal() {
            injector.spawn_n(&self.scheduler, MAX_STOLEN_PER_TICK)
        } else {
            0
        }
    }
}

impl Default for Core {
    fn default() -> Self {
        Self::new()
    }
}

// === impl Runtime ===

impl Runtime {
    fn active_cores(&self) -> usize {
        self.initialized.load(core::sync::atomic::Ordering::Acquire)
    }

    fn new_scheduler(&self) -> (usize, &StaticScheduler) {
        let next = self.initialized.fetch_add(1, AcqRel);
        assert!(next < MAX_CORES);
        let scheduler = self.cores[next].init(StaticScheduler::new());
        (next, scheduler)
    }

    fn try_steal_from(
        &'static self,
        idx: usize,
    ) -> Option<Stealer<'static, &'static StaticScheduler>> {
        self.cores[idx].try_get()?.try_steal().ok()
    }
}

impl Debug for Runtime {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let cores = self.active_cores();
        f.debug_struct("Runtime")
            .field("active_cores", &cores)
            .field("cores", &&self.cores[..cores])
            .field("injector", &self.injector)
            .finish()
    }
}

/// 抢占当前任务。
pub(crate) async fn might_preempt() {
    if !cpu_local::should_preempt() {
        return;
    }
    yield_now().await;
}

/// 主动让出执行权。
pub async fn yield_now() {
    super::atomic_mode::might_sleep();

    // 安全性: RCU 读侧临界区会禁用抢占。执行到这里时，
    // 我们已经检查过抢占是启用的。
    unsafe {
        crate::sync::finish_grace_period();
    }
    cpu_local::clear_need_preempt();
    maitake::future::yield_now().await;
}

/// 将一个新构建的任务入队。
///
/// 注意，新任务不保证立即运行。
#[track_caller]
pub(super) fn spawn_user_task<Fut>(runnable: Arc<Task>, future: Fut) -> JoinHandle<Fut::Output>
where
    Fut: Future + Send + 'static,
    Fut::Output: Send + 'static,
{
    let runnable_ptr = Arc::as_ptr(&runnable) as usize;
    let future = async move {
       future.await
    };
    spawn(future, Some(runnable_ptr))
}

#[allow(unused)]
fn set_need_preempt(cpu_id: CpuId) {
    let preempt_guard = disable_preempt();

    if preempt_guard.current_cpu() == cpu_id {
        cpu_local::set_need_preempt();
    } else {
        // TODO: 发送 IPI (核间中断) 来设置远程 CPU 的 `need_preempt` 标志
    }
}

pub(crate) fn get_current_task_ptr() -> Option<usize> {
    let mut current_task = None;
    SCHEDULER.with(|scheduler| {
        current_task = scheduler
            .get()
            .expect("There is no scheduler running.")
            .current_task();
    });
    current_task
        .expect("There are no tasks running in the scheduler.")
        .ostd_task_ptr()
}

/// 一个用于将异步任务转换为阻塞任务的模块。
///
/// 这个模块提供了一个 `BlockingFuture` trait，
/// 它允许将异步任务转换为阻塞任务，以便在需要时使用。
///
/// 主要功能包括：
/// - 将异步任务转换为阻塞任务
pub mod blocking_future {

    use core::{future::*, hint::spin_loop, pin::pin, task::*};

    /// 一个用于将异步任务转换为阻塞任务的 trait。
    ///
    /// 这个 trait 允许将异步任务转换为阻塞任务，以便在需要时使用。
    ///
    /// 主要功能包括：
    /// - 将异步任务转换为阻塞任务
    pub trait BlockingFuture: Future + Sized {
        /// 将异步任务转换为阻塞任务。
        fn block(self) -> <Self as Future>::Output {
            let mut pinned = pin!(self);
            let mut ctx = Context::from_waker(maitake::task::Waker::noop());
            loop {
                match pinned.as_mut().poll(&mut ctx) {
                    Poll::Ready(x) => {
                        return x;
                    }
                    Poll::Pending => {
                        spin_loop();
                        // crate::prelude::println!("block pending");
                    }
                }
            }
        }
    }

    impl<F: Future + Sized> BlockingFuture for F {}
}
