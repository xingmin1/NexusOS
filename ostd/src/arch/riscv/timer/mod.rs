// SPDX-License-Identifier: MPL-2.0

//! The timer support.

use core::sync::atomic::{AtomicU64, Ordering};

use spin::Once;

use crate::{
    arch::boot::DEVICE_TREE, io_mem::IoMem, timer::INTERRUPT_CALLBACKS, trap::disable_local,
};

/// The timer frequency (Hz). Here we choose 1000Hz since 1000Hz is easier for unit conversion and
/// convenient for timer. What's more, the frequency cannot be set too high or too low, 200Hz is
/// a modest choice.
///
/// For system performance reasons, this rate cannot be set too high, otherwise most of the time
/// is spent executing timer code.
pub const TIMER_FREQ: u64 = 200;

// pub(crate) static TIMEBASE_FREQ: AtomicU64 = AtomicU64::new(1);
pub(crate) const TIMEBASE_FREQ: u64 = 10000000;

/// [`IoMem`] of goldfish RTC, which will be used by `aster-time`.
pub static GOLDFISH_IO_MEM: Once<IoMem> = Once::new();

pub(super) fn init() {
    let timer_freq = DEVICE_TREE
        .get()
        .expect("DTB not initialized in timer::init")
        .cpus()
        .next()
        .expect("No CPU node found in DTB for timebase frequency")
        .timebase_frequency() as u64;
    // TIMEBASE_FREQ.store(timer_freq, Ordering::Relaxed);
    // crate::prelude::println!("timer_freq: {}", timer_freq);

    let fdt = DEVICE_TREE
        .get()
        .expect("DTB not initialized in timer::init");

    if let Some(rtc_node) = fdt.find_node("/soc/rtc")
        && let Some(compatible) = rtc_node.compatible()
        && compatible.all().any(|c| c == "google,goldfish-rtc")
    {
        let region = rtc_node
            .reg()
            .expect("RTC node has no reg property")
            .next()
            .expect("RTC reg property has no region");
        let start_addr = region.starting_address as usize;
        let size = region.size.expect("RTC region has no size");
        let io_mem = unsafe {
            IoMem::new(
                start_addr..(start_addr + size),
                crate::mm::PageFlags::empty(),
                crate::mm::CachePolicy::Uncacheable,
            )
        };
        GOLDFISH_IO_MEM.call_once(|| io_mem);
    }
    // 每个tick的纳秒数 = 每秒的纳秒数 / 每秒的tick数
    let tick_duration = 1_000_000_000 / timer_freq;
    let clock = maitake::time::Clock::new(
        maitake::time::Duration::from_nanos(tick_duration),
        riscv::register::time::read64,
    );
    crate::task::scheduler::init(clock);
    set_next_timer();
}

/// 根据当前时间戳和定时器频率设置下一个定时器
pub(crate) fn set_next_timer() {
    sbi_rt::set_timer(
        riscv::register::time::read64()
            // .wrapping_add(TIMEBASE_FREQ.load(Ordering::Relaxed) / TIMER_FREQ),
            .wrapping_add(TIMEBASE_FREQ / TIMER_FREQ),
    );
}

pub(crate) fn time_interrupt_handler() {
    crate::timer::jiffies::ELAPSED.fetch_add(1, Ordering::SeqCst);

    // crate::prelude::print!("1");

    let irq_guard = disable_local();
    let callbacks_guard = INTERRUPT_CALLBACKS.get_with(&irq_guard);
    for callback in callbacks_guard.borrow().iter() {
        (callback)();
    }
    drop(callbacks_guard);
    set_next_timer();
}
