#![allow(unused)] // TODO: 真实实现

pub mod times;
pub mod tms;
pub mod gettimeofday;

use core::{sync::atomic::{AtomicU64, Ordering}, time::Duration};
use ostd::{timer::Jiffies, Pod};



/// 每秒嘀嗒数；定时中断频率。
pub const CLOCK_TICK_HZ: u64 = ostd::arch::timer::TIMER_FREQ;

/// 自系统启动以来经过的嘀嗒数。
#[inline]
pub fn ticks_since_boot() -> u64 {
    Jiffies::elapsed().as_u64()
}

pub fn duration_since_boot() -> Duration {
    Jiffies::elapsed().as_duration()
}


/// 内核态 / 用户态嘀嗒计数
#[derive(Default)]
pub struct CpuTimes {
    pub utime:  AtomicU64,
    pub stime:  AtomicU64,
    pub cutime: AtomicU64,
    pub cstime: AtomicU64,
}

impl CpuTimes {
    #[inline]
    pub fn account_user(&self, ticks: u64) {
        self.utime.fetch_add(ticks, Ordering::Relaxed);
    }
    #[inline]
    pub fn account_sys(&self, ticks: u64) {
        self.stime.fetch_add(ticks, Ordering::Relaxed);
    }
}

impl Clone for CpuTimes {
    fn clone(&self) -> Self {
        Self {
            utime: AtomicU64::new(self.utime.load(Ordering::Relaxed)),
            stime: AtomicU64::new(self.stime.load(Ordering::Relaxed)),
            cutime: AtomicU64::new(self.cutime.load(Ordering::Relaxed)),
            cstime: AtomicU64::new(self.cstime.load(Ordering::Relaxed)),
        }
    }
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Pod)]
pub struct Timespec {
    pub sec: i64,
    pub usec: i64,
}

impl From<Duration> for Timespec {
    fn from(duration: Duration) -> Timespec {
        let sec = duration.as_secs() as i64;
        let usec = duration.subsec_micros() as i64;
        Timespec { sec, usec }
    }
}

impl From<Timespec> for Duration {
    fn from(timespec: Timespec) -> Duration {
        Duration::from_secs(timespec.sec as u64) + Duration::from_micros(timespec.usec as u64)
    }
}