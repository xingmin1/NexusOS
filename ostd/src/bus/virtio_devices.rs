// SPDX-License-Identifier: MPL-2.0

//! VirtIO 设备容器与中断确认统一入口。

use alloc::sync::Arc;
use smallvec::SmallVec;
use crate::sync::SpinMutex;
use virtio_drivers::{
    device::{
        blk::VirtIOBlk,
        console::VirtIOConsole,
        gpu::VirtIOGpu,
        input::VirtIOInput,
        net::VirtIONet,
        rng::VirtIORng,
        socket::VirtIOSocket,
        sound::VirtIOSound,
    },
    transport::SomeTransport,
};

use crate::{drivers::virtio::hal::RiscvHal, io_mem::IoMem};
use crate::trap::IrqLine;

/// VirtIO 设备的统一枚举类型。
/// VirtIO 设备枚举，封装所有支持的设备类型。
/// 
/// 每个变体都使用 Arc<Mutex<T>> 包装，保证线程安全访问。
pub enum VirtioDevice {
    /// 块存储设备 (virtio-blk)
    Block(Arc<SpinMutex<VirtIOBlk<RiscvHal, SomeTransport<'static>>>>),
    /// 网络设备 (virtio-net)，使用 32 个队列
    Network(Arc<SpinMutex<VirtIONet<RiscvHal, SomeTransport<'static>, 32>>>),
    /// 控制台设备 (virtio-console)
    Console(Arc<SpinMutex<VirtIOConsole<RiscvHal, SomeTransport<'static>>>>),
    /// GPU 设备 (virtio-gpu)
    Gpu(Arc<SpinMutex<VirtIOGpu<RiscvHal, SomeTransport<'static>>>>),
    /// 输入设备 (virtio-input)
    Input(Arc<SpinMutex<VirtIOInput<RiscvHal, SomeTransport<'static>>>>),
    /// 随机数生成器 (virtio-rng)
    Rng(Arc<SpinMutex<VirtIORng<RiscvHal, SomeTransport<'static>>>>),
    /// 音频设备 (virtio-sound)
    Sound(Arc<SpinMutex<VirtIOSound<RiscvHal, SomeTransport<'static>>>>),
    /// 套接字设备 (virtio-socket)，使用 512 字节缓冲区
    Socket(Arc<SpinMutex<VirtIOSocket<RiscvHal, SomeTransport<'static>, 512>>>),
}

impl VirtioDevice {
    /// 确认并清除设备中断。
    pub fn ack_interrupt(&self) -> bool {
        match self {
            Self::Block(d) => d.lock().ack_interrupt(),
            Self::Network(d) => d.lock().ack_interrupt(),
            Self::Console(d) => d.lock().ack_interrupt().unwrap_or(false),
            Self::Gpu(d) => d.lock().ack_interrupt(),
            Self::Input(d) => d.lock().ack_interrupt(),
            Self::Rng(d) => d.lock().ack_interrupt(),
            Self::Sound(d) => d.lock().ack_interrupt(),
            Self::Socket(_) => false,
        }
    }

    /// 人类可读的设备名。
    pub fn name(&self) -> &'static str {
        match self {
            Self::Block(_) => "virtio-blk",
            Self::Network(_) => "virtio-net",
            Self::Console(_) => "virtio-console",
            Self::Gpu(_) => "virtio-gpu",
            Self::Input(_) => "virtio-input",
            Self::Rng(_) => "virtio-rng",
            Self::Sound(_) => "virtio-sound",
            Self::Socket(_) => "virtio-socket",
        }
    }
}

/// 设备元信息（便于登记到设备管理器）。
#[derive(Clone)]
pub struct DeviceInfo {
    /// 设备名
    pub name: &'static str,
    /// 设备中断线
    pub irq: Option<IrqLine>,
    /// 对于 MMIO 设备，保存其 IoMem 以保持映射有效。
    pub io_mem: SmallVec<[IoMem; 1]>,
}
