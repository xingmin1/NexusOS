use crate::{impls::dev_fs::AsyncCharDevice, vfs_err_invalid_argument, VfsResult};
use alloc::boxed::Box;
use ostd::early_print;
use tracing::trace;

pub struct StdOutDevice;

#[async_trait::async_trait]
impl AsyncCharDevice for StdOutDevice {
    async fn write(&self, _off: u64, buf: &[u8]) -> VfsResult<usize> {
        let s = core::str::from_utf8(buf).map_err(|_| vfs_err_invalid_argument!("invalid utf-8"))?;
        trace!("StdOutDevice write: {:?}", s);
        early_print!("{}", s);
        Ok(buf.len())
    }
}