#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

pub use vfs::impls::ext4_fs::provider::get_ext4_provider;
