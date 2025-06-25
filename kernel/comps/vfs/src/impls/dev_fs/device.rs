use crate::{impls::dev_fs::AsyncCharDevice, VfsResult};
use alloc::boxed::Box;
use ostd::early_print;

pub struct StdOutDevice;

#[async_trait::async_trait]
impl AsyncCharDevice for StdOutDevice {
    async fn write(&self, _off: u64, buf: &[u8]) -> VfsResult<usize> {
        early_print!("{}", core::str::from_utf8(buf).unwrap());
        Ok(buf.len())
    }
}