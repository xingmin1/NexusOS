// kernel/libs/block-dev/src/lib.rs
// SPDX-License-Identifier: MPL-2.0

#![no_std]

extern crate alloc;

use core::fmt;

/// 逻辑扇区尺寸：VirtIO‑Blk 规格固定 512 字节。
pub const SECTOR_SIZE: usize = 512;

/// 块设备错误类型（可按需增补）。
#[derive(Debug)]
pub enum BlockError {
    InvalidParam,
    IoError,
    Unsupported,
}

impl fmt::Display for BlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockError::InvalidParam => write!(f, "invalid parameter"),
            BlockError::IoError      => write!(f, "device I/O error"),
            BlockError::Unsupported  => write!(f, "operation unsupported"),
        }
    }
}

/// **同步**块设备接口 —— 先满足 ext4。
///
/// * 所有 API 在返回前必须完成请求，确保数据已到达内存或落盘。  
/// * 如果后续要做异步，只需在此 trait 上再包一层 future/queue。
pub trait BlockDevice: Send + Sync + 'static {
    /// 设备可用的扇区总数。
    fn sectors(&self) -> u64;

    /// 读 `buf.len()/SECTOR_SIZE` 个扇区；`lba` 以扇区为单位。
    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError>;

    /// 写 `buf.len()/SECTOR_SIZE` 个扇区；`lba` 以扇区为单位。
    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), BlockError>;

    /// flush ‑ 若设备支持缓存，需要保证数据持久化。
    fn flush(&self) -> Result<(), BlockError> {
        Ok(())
    }
}
