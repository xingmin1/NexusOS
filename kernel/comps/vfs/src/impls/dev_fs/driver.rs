use alloc::boxed::Box;

use nexus_error::{return_errno_with_message, Errno};

use crate::VfsResult;


#[allow(unused_variables)]
#[async_trait::async_trait]
pub trait AsyncCharDevice: Send + Sync + 'static {
    async fn read(&self, off: u64, buf: &mut [u8]) -> VfsResult<usize> {
        return_errno_with_message!(Errno::ENODEV, "read not supported")
    }
    async fn write(&self, off: u64, buf: &[u8]) -> VfsResult<usize> {
        return_errno_with_message!(Errno::ENODEV, "write not supported")
    }
}
