mod elf_image;
mod elf_mapper;
mod elf_file;

use alloc::{ffi::CString, vec::Vec};
pub use elf_image::ElfImage;
pub use elf_mapper::{ElfMapper, ElfLoadInfo};
use nexus_error::Result;

use crate::vm::ProcessVm;

pub async fn load_elf_to_vm(vm: &ProcessVm, path: &str, argv: Vec<CString>, envp: Vec<CString>) -> Result<ElfLoadInfo> {
    let image = ElfImage::from_path(path).await?;
    let mapper = ElfMapper { image: &image, vm };
    mapper.map(argv, envp).await
}
