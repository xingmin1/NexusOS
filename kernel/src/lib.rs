// SPDX-License-Identifier: MPL-2.0

#![no_std]
#![deny(unsafe_code)]
#![feature(btree_cursors)]
#![feature(let_chains)]
#![feature(step_trait)]
#![expect(incomplete_features)]
// FIXME: the feature `specialization` is incomplete and may not be safe to use and/or cause compiler crashes
#![feature(specialization)]

mod error;
mod thread;
mod vm;

extern crate alloc;

use ostd::{
    cpu::{CpuSet, PinCurrentCpu},
    smp::inter_processor_call,
    task::{disable_preempt, scheduler::{blocking_future::BlockingFuture, spawn}},
};
use thread::ThreadBuilder;
use tracing::{debug, info, trace_span, warn};
#[allow(unused_imports)]
use nexus_error::{return_errno, return_errno_with_message};

/// The kernel's boot and initialization process is managed by OSTD.
/// After the process is done, the kernel's execution environment
/// (e.g., stack, heap, tasks) will be ready for use and the entry function
/// labeled as `#[ostd::main]` will be called.
#[ostd::main]
pub fn main() {
    let _main_span = trace_span!("kernel_main", stage = "initialization").entered();

    info!("开始执行内核主函数");
    debug!("追踪系统已初始化");

    spawn(async {
        vfs::init_vfs().await;
        let thread_span = trace_span!("thread_spawn").entered();
        ThreadBuilder::new().path("/musl/basic/exit").spawn().block().inspect_err(|e| {
            warn!("ThreadBuilder::spawn 失败: {:?}", e);
        });
    }, None);

    info!("内核主函数完成设置，BSP 进入空闲循环");

    debug!("注册 AP 入口函数");
    ostd::boot::smp::register_ap_entry(ap_main);

    let cpus = CpuSet::new_full();
    inter_processor_call(&cpus, || {
        let cpu_id = disable_preempt().current_cpu().as_usize();
        info!(cpu_id, "CPU 运行 inter_processor_call");
    });

    let mut core = ostd::task::scheduler::Core::new();
    core.run();
}

fn ap_main() -> ! {
    let cpu_id;
    {
        let disable_preempt = disable_preempt();
        cpu_id = disable_preempt.current_cpu().as_usize();
        info!(cpu_id, "AP 进入 ap_main 函数，准备进入空闲循环");
    }

    let mut core = ostd::task::scheduler::Core::new();
    core.run();
    unreachable!()
}
