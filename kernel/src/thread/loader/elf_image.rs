//! 将“文件系统路径”转换成可解析的 ELF 字节切片。

use alloc::vec::Vec;

use elf_loader::object::ElfBinary;
use vfs::PathBuf;

use crate::{error::Result, thread::loader::elf_file::ExtFile};

/// `ElfImage` 在加载完成后持有字节缓冲区，生命周期与自身一致。
pub struct ElfImage {
    bytes: Vec<u8>,
    path: PathBuf,
}

impl ElfImage {
    /// 从 ext4 打开并读取文件。
    pub async fn from_path(path: &str) -> Result<Self> {
        let file = ExtFile::open(path).await?;
        let (path, file) = file;
        Ok(Self {
            bytes: file.read_all().await?,
            path,
        })
    }

    /// 提供给 `ElfMapper` 解析。  
    pub fn as_binary(&self) -> ElfBinary<'_> {
        ElfBinary::new(
            self.path.to_slice().file_name().unwrap_or(&self.path),
            &self.bytes,
        )
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
}
