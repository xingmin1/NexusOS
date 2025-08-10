// SPDX-License-Identifier: MPL-2.0

//! 设备管理器：封装 VirtIO 设备创建与中断分发。

use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use core::mem;
use smallvec::SmallVec;

use spin::Mutex;
use virtio_drivers::{
    device::{
        blk::VirtIOBlk, console::VirtIOConsole, gpu::VirtIOGpu, input::VirtIOInput, net::VirtIONet,
        rng::VirtIORng, socket::VirtIOSocket, sound::VirtIOSound,
    },
    transport::{
        mmio::{MmioTransport, VirtIOHeader},
        pci::PciTransport,
        DeviceType, SomeTransport, Transport,
    },
};

use crate::{
    bus::virtio_devices::{DeviceInfo, VirtioDevice},
    drivers::virtio::hal::RiscvHal,
    io_mem::IoMem,
    mm::page_prop::{CachePolicy, PageFlags},
    trap::IrqLine,
};

/// 设备管理器，持有已创建设备与 IRQ 映射。
pub struct DeviceManager {
    devices: Vec<(DeviceInfo, VirtioDevice)>,
    irq_map: BTreeMap<u16, usize>,
}

impl DeviceManager {
    /// 创建空管理器。
    pub const fn new() -> Self {
        Self {
            devices: Vec::new(),
            irq_map: BTreeMap::new(),
        }
    }

    /// 基于 MMIO 物理区创建设备。
    pub fn create_mmio_device(&mut self, paddr: usize, size: usize, irq: Option<IrqLine>) {
        let io_mem =
            unsafe { IoMem::new(paddr..paddr + size, PageFlags::RW, CachePolicy::Uncacheable) };
        let header = unsafe { io_mem.as_non_null::<VirtIOHeader>(0) };
        let transport = match unsafe { MmioTransport::new(header, size) } {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("MMIO transport init failed: {:?}", e);
                return;
            }
        };
        // 将生命周期提升到 'static：io_mem 持有映射，内核生命周期内不释放
        let some: SomeTransport<'static> =
            unsafe { mem::transmute(SomeTransport::from(transport)) };
        self.create_device_with_iomems(some, irq, SmallVec::from([io_mem]));
    }

    /// 基于 PCI 传输创建设备。
    pub fn create_pci_device(&mut self, transport: PciTransport, irq: Option<IrqLine>, iomems: SmallVec<[IoMem; 1]>) {
        let some = SomeTransport::from(transport);
        self.create_device_with_iomems(some, irq, iomems);
    }

    #[no_mangle]
    pub fn create_device_with_iomems(
        &mut self,
        transport: SomeTransport<'static>,
        irq: Option<IrqLine>,
        iomems: SmallVec<[IoMem; 1]>,
    ) {
        let transport = transport;
        let ty = transport.device_type();
        let dev = match ty {
            DeviceType::Block => match VirtIOBlk::<RiscvHal, _>::new(transport) {
                Ok(v) => VirtioDevice::Block(Arc::new(Mutex::new(v))),
                Err(e) => {
                    tracing::error!("virtio-blk init failed: {:?}", e);
                    return;
                }
            },
            DeviceType::Network => match VirtIONet::<RiscvHal, _, 32>::new(transport, 1536) {
                Ok(v) => VirtioDevice::Network(Arc::new(Mutex::new(v))),
                Err(e) => {
                    tracing::error!("virtio-net init failed: {:?}", e);
                    return;
                }
            },
            DeviceType::Console => match VirtIOConsole::<RiscvHal, _>::new(transport) {
                Ok(v) => VirtioDevice::Console(Arc::new(Mutex::new(v))),
                Err(e) => {
                    tracing::error!("virtio-console init failed: {:?}", e);
                    return;
                }
            },
            DeviceType::GPU => match VirtIOGpu::<RiscvHal, _>::new(transport) {
                Ok(v) => VirtioDevice::Gpu(Arc::new(Mutex::new(v))),
                Err(e) => {
                    tracing::error!("virtio-gpu init failed: {:?}", e);
                    return;
                }
            },
            DeviceType::Input => match VirtIOInput::<RiscvHal, _>::new(transport) {
                Ok(v) => VirtioDevice::Input(Arc::new(Mutex::new(v))),
                Err(e) => {
                    tracing::error!("virtio-input init failed: {:?}", e);
                    return;
                }
            },
            DeviceType::EntropySource => match VirtIORng::<RiscvHal, _>::new(transport) {
                Ok(v) => VirtioDevice::Rng(Arc::new(Mutex::new(v))),
                Err(e) => {
                    tracing::error!("virtio-rng init failed: {:?}", e);
                    return;
                }
            },
            DeviceType::Sound => match VirtIOSound::<RiscvHal, _>::new(transport) {
                Ok(v) => VirtioDevice::Sound(Arc::new(Mutex::new(v))),
                Err(e) => {
                    tracing::error!("virtio-sound init failed: {:?}", e);
                    return;
                }
            },
            DeviceType::Socket => match VirtIOSocket::<RiscvHal, _, 512>::new(transport) {
                Ok(v) => VirtioDevice::Socket(Arc::new(Mutex::new(v))),
                Err(e) => {
                    tracing::error!("virtio-socket init failed: {:?}", e);
                    return;
                }
            },
            _ => {
                tracing::warn!("unsupported virtio device: {:?}", ty);
                return;
            }
        };

        let name = dev.name();
        let idx = self.devices.len();
        self.devices.push((
            DeviceInfo {
                name,
                irq: irq.clone(),
                io_mem: iomems,
            },
            dev,
        ));
        if let Some(irq) = &irq {
            self.irq_map.insert(irq.num(), idx);
        }
        tracing::info!("registered device: {} (idx {})", name, idx);

        // 注册 IRQ 回调 -> 分发到 DEVICE_MANAGER.handle_irq
        if let Some(line) = &irq {
            let mut line_mut = line.clone();
            let irq_num = line.num();
            line_mut.on_active(move |_| {
                crate::bus::device_manager::DEVICE_MANAGER
                    .lock()
                    .handle_irq(irq_num);
            });
        }
    }

    /// 分发中断。
    pub fn handle_irq(&self, irq_num: u16) {
        if let Some(&idx) = self.irq_map.get(&irq_num) {
            if let Some((info, dev)) = self.devices.get(idx) {
                tracing::trace!("irq {} -> {}", irq_num, info.name);
                let _ = dev.ack_interrupt();
            }
        }
    }

    /// 获取所有块设备。
    pub fn block_devices(&self) -> Vec<Arc<Mutex<VirtIOBlk<RiscvHal, SomeTransport<'static>>>>> {
        self.devices
            .iter()
            .filter_map(|(_, d)| match d {
                VirtioDevice::Block(x) => Some(x.clone()),
                _ => None,
            })
            .collect()
    }

    /// 获取所有网络设备。
    pub fn network_devices(
        &self,
    ) -> Vec<Arc<Mutex<VirtIONet<RiscvHal, SomeTransport<'static>, 32>>>> {
        self.devices
            .iter()
            .filter_map(|(_, d)| match d {
                VirtioDevice::Network(x) => Some(x.clone()),
                _ => None,
            })
            .collect()
    }

    /// 获取一个控制台设备（若存在）。
    pub fn console_device(
        &self,
    ) -> Option<Arc<Mutex<VirtIOConsole<RiscvHal, SomeTransport<'static>>>>> {
        self.devices.iter().find_map(|(_, d)| match d {
            VirtioDevice::Console(x) => Some(x.clone()),
            _ => None,
        })
    }
}

/// 全局设备管理器。
pub static DEVICE_MANAGER: Mutex<DeviceManager> = Mutex::new(DeviceManager::new());
