// SPDX-License-Identifier: MPL-2.0

//! The timer support.

use core::sync::atomic::{AtomicU64, Ordering};
use loongArch64::register::{tcfg, ticlr};

use crate::{timer::INTERRUPT_CALLBACKS, trap::disable_local};

/// The timer frequency (Hz). Here we choose 1000Hz since 1000Hz is easier for unit conversion and
/// convenient for timer. What's more, the frequency cannot be set too high or too low, 1000Hz is
/// a modest choice.
///
/// For system performance reasons, this rate cannot be set too high, otherwise most of the time
/// is spent executing timer code.
pub const TIMER_FREQ: u64 = 1000;

pub(crate) static TIMEBASE_FREQ: AtomicU64 = AtomicU64::new(1);

pub(super) fn init() {
    use loongArch64::cpu::CPUCFG;

    let cc_freq = CPUCFG::read(0x4).get_bits(0, 31) as u64;
    let cc_mul = CPUCFG::read(0x5).get_bits(0, 15) as u64;
    let cc_div = CPUCFG::read(0x5).get_bits(16, 31) as u64;

    let timer_freq = cc_freq * cc_mul / cc_div;

    TIMEBASE_FREQ.store(timer_freq, Ordering::Relaxed);

    let tick_duration = 1_000_000_000 / timer_freq;
    let clock = maitake::time::Clock::new(
        maitake::time::Duration::from_nanos(tick_duration),
        read_time64,
    );
    crate::task::scheduler::init(clock);
    set_next_timer();
}

/// 从LoongArch定时器读取64位时间值
/// 
/// 使用rdtime.d指令读取时间计数器，该指令返回64位时间值，
/// 在32位模式下分为高32位和低32位两部分返回
fn read_time64() -> u64 {
    let (high, low): (u32, u32);
    unsafe {
        core::arch::asm!(
            "rdtime.d {low}, {high}",  // 读取时间计数器
            high = out(reg) high,      // 高32位
            low = out(reg) low,        // 低32位
        );
    }
    ((high as u64) << 32) | (low as u64)  // 合并高低位为64位值
}

/// 设置并启用周期性定时器中断
///
/// 在 LoongArch 架构上，定时器可以被配置为周期性的，
/// 因此该函数只需在初始化阶段调用一次。
pub(crate) fn set_next_timer() {
    let interval = TIMEBASE_FREQ.load(Ordering::Relaxed) / TIMER_FREQ;
    // LoongArch 的定时器是一个递减计数器。当计数器减到零时，会触发一个中断。
    // 在周期模式下，它会自动重新加载初始值。
    tcfg::set_init_val(interval as usize);
    // 清除任何挂起的时钟中断。
    ticlr::clear_timer_interrupt();
    // 使能定时器并将其设置为周期模式。
    tcfg::set_en(true);
    tcfg::set_periodic(true);
}

/// 处理时钟中断。
///
/// 当时钟中断发生时，该函数会由陷阱处理程序调用。
pub(crate) fn time_interrupt_handler() {
    // 对于周期性定时器，我们只需要清除中断标志位。
    // 定时器会自动重新加载并继续计数。
    ticlr::clear_timer_interrupt();

    crate::timer::jiffies::ELAPSED.fetch_add(1, Ordering::SeqCst);

    let irq_guard = disable_local();
    let callbacks_guard = INTERRUPT_CALLBACKS.get_with(&irq_guard);
    for callback in callbacks_guard.borrow().iter() {
        (callback)();
    }
}
