// SPDX-License-Identifier: MPL-2.0

//! Definitions of page mapping properties.

use core::fmt::Debug;

use bitflags::bitflags;

/// The property of a mapped virtual memory page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageProperty {
    /// The flags associated with the page,
    pub flags: PageFlags,
    /// The cache policy for the page.
    pub cache: CachePolicy,
    pub(crate) priv_flags: PrivilegedPageFlags,
}

impl PageProperty {
    /// Creates a new `PageProperty` with the given flags and cache policy for the user.
    pub fn new(flags: PageFlags, cache: CachePolicy, priv_flags: PrivilegedPageFlags) -> Self {
        Self {
            flags,
            cache,
            priv_flags,
        }
    }
    /// Creates a page property that implies an invalid page without mappings.
    pub fn new_absent() -> Self {
        Self {
            flags: PageFlags::empty(),
            cache: CachePolicy::Writeback,
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
    /// Page protection permissions and access status.
    pub struct PageFlags: u8 {
        /// Readable.
        const R = 0b00000001;
        /// Writable.
        const W = 0b00000010;
        /// Executable.
        const X = 0b00000100;
        /// Readable + writable.
        const RW = Self::R.bits | Self::W.bits;
        /// Readable + executable.
        const RX = Self::R.bits | Self::X.bits;
        /// Readable + writable + executable.
        const RWX = Self::R.bits | Self::W.bits | Self::X.bits;

        /// Has the memory page been read or written.
        const ACCESSED  = 0b00001000;
        /// Has the memory page been written.
        const DIRTY     = 0b00010000;

        /// The first bit available for software use.
        const AVAIL1    = 0b01000000;
        /// The second bit available for software use.
        const AVAIL2    = 0b10000000;
    }
}

bitflags! {
    /// Page property that are only accessible in OSTD.
    pub struct PrivilegedPageFlags: u8 {
        /// Accessible from kernel mode.
        #[cfg(target_arch = "riscv64")]
        const KERNEL    = 0b00000000;
        /// Accessible from user mode.
        const USER      = 0b00000001;
        /// Global page that won't be evicted from TLB with normal TLB flush.
        /// # 在 RISC-V 中，
        /// 1. 当执行 rs2=x0 的 SFENCE.VMA 指令时，不会从本地地址转换缓存中刷新那些 Global 被设置的映射。
        /// 2. 由于 User(第4位，从0开始) 不被设置时为 KERNEL，所以当是 Global(第5位，从0开始) 时，也是 KERNEL 的，即在这里 PrivilegedPageFlags::Global 其实也有 KERNEL 的含义。
        const GLOBAL    = 0b00000010;

        /// (TEE only) If the page is shared with the host.
        /// Otherwise the page is ensured confidential and not visible outside the guest.
        #[cfg(all(target_arch = "x86_64", feature = "cvm_guest"))]
        const SHARED    = 0b10000000;
    }
}
