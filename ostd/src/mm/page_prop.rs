// SPDX-License-Identifier: MPL-2.0

//! 页面映射属性的定义。

use core::fmt::Debug;

use bitflags::bitflags;

/// 映射虚拟内存页的属性。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageProperty {
    /// 与页面关联的标志，
    pub flags: PageFlags,
    pub(crate) priv_flags: PrivilegedPageFlags,
}

impl PageProperty {
    /// 为用户创建具有给定标志的新 `PageProperty`。
    pub fn new(flags: PageFlags) -> Self {
        Self {
            flags,
            priv_flags: PrivilegedPageFlags::USER,
        }
    }

    /// 创建一个表示无效页面且没有映射的页面属性。
    pub fn new_absent() -> Self {
        Self {
            flags: PageFlags::empty(),
            priv_flags: PrivilegedPageFlags::empty(),
        }
    }
}

// TODO: Make it more abstract when supporting other architectures.
/// A type to control the cacheability of the main memory.
///
/// The type currently follows the definition as defined by the AMD64 manual.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CachePolicy {
    /// Uncacheable (UC).
    ///
    /// Reads from, and writes to, UC memory are not cacheable.
    /// Reads from UC memory cannot be speculative.
    /// Write-combining to UC memory is not allowed.
    /// Reads from or writes to UC memory cause the write buffers to be written to memory
    /// and be invalidated prior to the access to UC memory.
    ///
    /// The UC memory type is useful for memory-mapped I/O devices
    /// where strict ordering of reads and writes is important.
    Uncacheable,
    /// Write-Combining (WC).
    ///
    /// Reads from, and writes to, WC memory are not cacheable.
    /// Reads from WC memory can be speculative.
    ///
    /// Writes to this memory type can be combined internally by the processor
    /// and written to memory as a single write operation to reduce memory accesses.
    ///
    /// The WC memory type is useful for graphics-display memory buffers
    /// where the order of writes is not important.
    WriteCombining,
    /// Write-Protect (WP).
    ///
    /// Reads from WP memory are cacheable and allocate cache lines on a read miss.
    /// Reads from WP memory can be speculative.
    ///
    /// Writes to WP memory that hit in the cache do not update the cache.
    /// Instead, all writes update memory (write to memory),
    /// and writes that hit in the cache invalidate the cache line.
    /// Write buffering of WP memory is allowed.
    ///
    /// The WP memory type is useful for shadowed-ROM memory
    /// where updates must be immediately visible to all devices that read the shadow locations.
    WriteProtected,
    /// Writethrough (WT).
    ///
    /// Reads from WT memory are cacheable and allocate cache lines on a read miss.
    /// Reads from WT memory can be speculative.
    ///
    /// All writes to WT memory update main memory,
    /// and writes that hit in the cache update the cache line.
    /// Writes that miss the cache do not allocate a cache line.
    /// Write buffering of WT memory is allowed.
    Writethrough,
    /// Writeback (WB).
    ///
    /// The WB memory is the "normal" memory. See detailed descriptions in the manual.
    ///
    /// This type of memory provides the highest-possible performance
    /// and is useful for most software and data stored in system memory (DRAM).
    Writeback,
}

bitflags! {
    /// 页面保护权限和访问状态。
    pub struct PageFlags: u8 {
        /// 可读。
        const R = 0b00000001;
        /// 可写。
        const W = 0b00000010;
        /// 可执行。
        const X = 0b00000100;
        /// 可读 + 可写。
        const RW = Self::R.bits() | Self::W.bits();
        /// 可读 + 可执行。
        const RX = Self::R.bits() | Self::X.bits();
        /// 可读 + 可写 + 可执行。
        const RWX = Self::R.bits() | Self::W.bits() | Self::X.bits();
        /// 内存页是否已被读取或写入。
        const ACCESSED  = 0b00001000;
        /// 内存页是否已被写入。
        const DIRTY     = 0b00010000;

        /// 第一个可供软件使用的位。
        const AVAIL1    = 0b01000000;
        /// 第二个可供软件使用的位。
        const AVAIL2    = 0b10000000;
    }
}

bitflags! {
    /// 仅在 OSTD 中可访问的页面属性。
    pub struct PrivilegedPageFlags: u8 {
        /// 可从用户模式访问。
        const USER      = 0b00000001;
        /// 全局页面，在正常的 TLB 刷新时不会从 TLB 中被驱逐。
        const GLOBAL    = 0b00000010;

        /// (TEE only) If the page is shared with the host.
        /// Otherwise the page is ensured confidential and not visible outside the guest.
        #[cfg(all(target_arch = "x86_64", feature = "cvm_guest"))]
        const SHARED    = 0b10000000;
    }
}
