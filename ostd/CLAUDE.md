# CLAUDE.md

本文件为Claude Code (claude.ai/code)在ostd（操作系统标准库）模块中工作时提供指导。

## 概述

ostd是NexusOS框架内核架构的核心基础，作为可信计算基础(TCB)提供所有底层抽象和安全API。它是整个系统中唯一允许使用unsafe Rust的模块，通过精心设计的安全封装为上层内核服务提供零开销的硬件抽象。

## 关键命令

```bash
# 构建ostd
cd ostd && cargo osdk build

# 运行ostd测试
cd ostd && cargo osdk test

# 生成文档
cd ostd && cargo osdk doc

# 代码检查
cd ostd && cargo osdk clippy
```

## 架构设计

### 多架构支持系统

#### `src/arch/` - 真实的三架构支持
ostd提供完整的多架构硬件抽象层：

**x86_64架构** (`arch/x86/`):
- **boot/**: 多启动协议支持（Multiboot2、Linux EFI Handover64）
- **cpu/**: x2APIC/XAPIC管理、TDX机密计算支持
- **iommu/**: 完整的Intel VT-d IOMMU实现，包含DMA重映射和中断重映射
- **kernel/**: ACPI解析、高精度计时器(HPET)、TSC校准
- **trap/**: IDT中断描述符表、GDT全局描述符表、系统调用处理

**RISC-V架构** (`arch/riscv/`):
- **boot/**: SBI(Supervisor Binary Interface)集成、设备树解析
- **cpu/**: Hart管理、CPU本地状态
- **plic/**: 平台级中断控制器，支持多Hart中断分发
- **trap/**: 异常和中断处理、系统调用入口

**LoongArch架构** (`arch/loongarch/`):
- **boot/**: 龙芯特定的启动序列
- **cpu/**: 龙芯处理器特性支持
- **device/**: 龙芯平台设备驱动
- **timer/**: 龙芯定时器支持

### 内存管理系统

#### `src/mm/` - 完整的内存管理实现

**物理内存管理** (`mm/frame/`):
```rust
// 示例：使用buddy分配器分配物理页面
let frames = FrameAllocator::alloc(order)?;
let paddr = frames.start_paddr();
```

- **allocator.rs**: Buddy分配器实现，支持2^0到2^10页面分配
- **segment.rs**: 内存段管理，支持NUMA架构
- **meta.rs**: 页面元数据管理，包含引用计数和状态
- **unique.rs**: 唯一页面所有权，防止多重释放

**虚拟内存管理** (`mm/page_table/`):
```rust
// 示例：页表操作
let mut cursor = page_table.cursor_mut(&vaddr_range)?;
cursor.map(paddr, PAGE_SIZE, flags)?;
```

- **cursor.rs**: 游标式页表操作，避免递归调用
- **boot_pt.rs**: 启动时页表，处理物理到虚拟地址转换
- **node/**: 多级页表节点管理

**内核空间管理** (`mm/kspace/`):
- 统一的内核虚拟地址布局
- **HUGE_MARKER位**: 创新设计，用于区分巨页和普通页面
- 内核VMO(虚拟内存对象)管理

**DMA支持** (`mm/dma/`):
- **dma_coherent.rs**: 一致性DMA内存分配
- **dma_stream.rs**: 流式DMA操作
- 支持IOMMU的设备内存访问

### 异步任务系统

#### `src/task/` - 基于maitake的零开销异步

**调度器集成** (`task/scheduler/`):
```rust
// 示例：异步任务创建
spawn(async {
    // 异步任务逻辑
    device.read_async().await?;
}, None);
```

- 与maitake运行时深度集成
- 支持任务优先级和亲和性
- 零开销的协作式调度

**抢占控制** (`task/preempt/`):
```rust
// 示例：抢占保护
let guard = disable_preempt();
// 临界区代码
// guard自动析构时恢复抢占
```

- **cpu_local.rs**: 每CPU的抢占状态管理
- **guard.rs**: RAII抢占守护，自动恢复抢占状态
- 与中断处理的协调

### 同步原语系统

#### `src/sync/` - 丰富的并发控制机制

**多种守护类型** (`sync/guard_*`):
```rust
// 示例：不同类型的锁守护
let spin_guard = SpinLock::lock();
let rw_guard = RwLock::read();
let arc_guard = RwArc::read();
```

- **guard_spin.rs**: 自旋锁守护，适用于短临界区
- **guard_rwlock.rs**: 读写锁守护，支持多读单写
- **guard_rwarc.rs**: 原子引用计数读写锁

**RCU实现** (`sync/rcu/`):
```rust
// 示例：RCU的读-拷贝-更新模式
let rcu_guard = rcu_read_lock();
let data = rcu_dereference(shared_data);
// 使用data进行只读访问
```

- **monitor.rs**: RCU监控器，管理宽限期
- **owner_ptr.rs**: RCU保护的指针类型
- 低延迟的并发读取优化

### 设备驱动框架

#### `src/drivers/` - 统一的设备抽象

**VirtIO驱动** (`drivers/virtio/`):
```rust
// 示例：VirtIO块设备操作
let mut request = BlockRequest::new();
request.read_async(block_id, buffer).await?;
```

- **block.rs**: VirtIO块设备驱动
- **hal.rs**: 硬件抽象层，与ostd-HAL集成
- 支持块设备、网络设备、控制台设备

**总线管理** (`src/bus/`):
- **PCI总线**: PCI配置空间访问、设备枚举
- **MMIO总线**: 内存映射I/O设备管理
- 统一的设备发现和初始化流程

### 关键库集成

#### `libs/maitake/` - 定制异步运行时
```rust
// 示例：异步任务和定时器
use maitake::time::sleep;

async fn delayed_task() {
    sleep(Duration::from_millis(100)).await;
    // 延时执行的逻辑
}
```

- **无栈协程**: 基于状态机的异步实现
- **定时器系统**: 高精度的异步定时器
- **内存效率**: 最小化运行时开销

#### `libs/virtio-drivers/` - VirtIO设备支持
- 完整的VirtIO 1.1规范实现
- 支持块设备、网络、控制台、输入设备
- 与ostd DMA系统集成

### 安全设计策略

#### 内存安全封装
```rust
// 示例：安全的MMIO访问
pub struct MmioDevice {
    base: NonNull<u8>,
    size: usize,
}

impl MmioDevice {
    pub fn read_u32(&self, offset: usize) -> u32 {
        // 边界检查和安全访问
        assert!(offset + 4 <= self.size);
        unsafe { self.base.as_ptr().add(offset).cast::<u32>().read_volatile() }
    }
}
```

#### 类型安全抽象
```rust
// 示例：物理地址和虚拟地址的类型区分
pub struct PhysAddr(u64);
pub struct VirtAddr(u64);

// 编译时防止地址类型混淆
impl From<PhysAddr> for VirtAddr {
    fn from(paddr: PhysAddr) -> Self {
        // 只有通过特定函数才能转换
        phys_to_virt(paddr)
    }
}
```

### 初始化序列

#### 系统启动流程
ostd的初始化遵循严格的顺序：

1. **架构初始化**: 启用CPU特性，设置串口
2. **内存管理初始化**: 堆分配器、页表管理器
3. **任务系统初始化**: maitake运行时启动
4. **设备枚举**: PCI/MMIO设备发现
5. **中断启用**: 开启全局中断

```rust
unsafe fn init() {
    arch::enable_cpu_features();
    arch::serial::init();
    logger::init();
    mm::heap_allocator::init();
    mm::frame::allocator::init();
    // ... 其他初始化步骤
}
```

## 开发指导原则

### unsafe代码规范
1. **最小化使用**: 只在必要的硬件访问时使用unsafe
2. **安全封装**: 将unsafe操作封装在安全API中
3. **边界检查**: 进行充分的参数验证
4. **文档说明**: 详细注释unsafe块的安全性保证

### 性能优化策略
```rust
// 示例：零开销抽象
#[inline(always)]
pub fn read_register<T: Copy>(addr: VirtAddr) -> T {
    unsafe { addr.as_ptr::<T>().read_volatile() }
}
```

1. **编译时优化**: 大量使用泛型和内联
2. **避免动态分发**: 使用静态分发和单态化
3. **缓存友好**: 考虑数据局部性和缓存行对齐
4. **架构特定优化**: 利用特定架构的指令优化

### 测试策略

#### 单元测试
```bash
# 主机环境测试（可直接运行的组件）
cd ostd && cargo test

# 内核环境测试（需要硬件支持的组件）
cd ostd && cargo osdk test
```

#### 集成测试
```bash
# 多架构验证
for arch in x86_64 riscv64 loongarch64; do
    ARCH=$arch cargo osdk test
done
```

### 扩展指南

#### 添加新架构支持
1. **创建架构目录**: `src/arch/new_arch/`
2. **实现必需trait**: `Arch`, `Cpu`, `Interrupt`等
3. **启动代码**: 汇编启动代码和早期C代码
4. **设备支持**: 架构特定的设备驱动
5. **测试验证**: 确保所有功能在新架构上正常工作

#### 添加新设备驱动
1. **定义设备接口**: 在`src/drivers/`中定义trait
2. **实现具体驱动**: 继承设备接口并实现
3. **集成到框架**: 在设备枚举中添加支持
4. **测试覆盖**: 编写单元测试和集成测试

ostd作为NexusOS的可信基础，承担着整个系统安全性的重责。通过精心设计的安全抽象和零开销的性能优化，它为上层内核服务提供了强大而可靠的基础平台。每一行代码都需要经过严格的安全审查和性能验证，确保framekernel架构的核心优势得以实现。