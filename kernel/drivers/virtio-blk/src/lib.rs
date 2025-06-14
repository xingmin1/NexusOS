// kernel/drivers/virtio-blk/src/mod.rs
#![no_std]

mod device;
mod queue;

pub use device::VirtIOBlkDevice;
