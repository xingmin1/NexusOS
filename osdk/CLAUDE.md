# CLAUDE.md

本文件为Claude Code (claude.ai/code) 在使用OSDK (操作系统开发工具包) 时提供详细指导。

## 概述

OSDK是NexusOS的操作系统开发工具包，是一个专门为frame-kernel架构的操作系统开发而设计的综合性开发工具链。它不仅类似于Rust生态系统中的Cargo包管理器，更集成了完整的项目管理、构建、测试、调试和性能分析功能，为现代操作系统开发提供了一站式解决方案。

OSDK通过统一的配置文件(OSDK.toml)和灵活的scheme系统，支持多种开发场景和目标架构，从实验性的内核开发到生产环境的部署都能够提供强有力的支持。

## 核心功能与命令详解

### 项目管理命令

#### 项目创建
```bash
# 创建新的内核项目（带有完整的运行环境配置）
cargo osdk new --kernel my-kernel
cargo osdk new --type kernel my-kernel

# 创建新的库项目（用于开发内核模块或依赖库）
cargo osdk new --lib my-lib
cargo osdk new --library my-lib
cargo osdk new --type library my-lib

# 项目自动包含：
# - 适当的Cargo.toml配置
# - OSDK.toml配置文件
# - rust-toolchain.toml工具链配置
# - 架构特定的链接脚本模板
# - 基础的源码模板
```

#### 构建系统
```bash
# 基础构建（开发模式）
cargo osdk build

# 发布构建（启用优化）
cargo osdk build --release
cargo osdk build --profile release

# 指定目标架构构建
cargo osdk build --target-arch x86_64
cargo osdk build --target-arch riscv64
cargo osdk build --target-arch loongarch64

# 自定义特性构建
cargo osdk build --features "cvm_guest,debug_symbols"
cargo osdk build --no-default-features --features "minimal"

# 测试构建（启用ktest配置）
cargo osdk build --for-test

# 指定输出目录
cargo osdk build --output ./custom_build_output

# 启用ELF文件剥离（减小文件大小）
cargo osdk build --strip-elf
```

### 运行与测试命令

#### 内核运行
```bash
# 基础运行（使用默认scheme配置）
cargo osdk run

# 使用特定配置scheme运行
cargo osdk run --scheme microvm      # 微虚拟机优化
cargo osdk run --scheme tdx          # Intel TDX机密计算
cargo osdk run --scheme oscomp-riscv # RISC-V竞赛环境
cargo osdk run --scheme iommu        # IOMMU支持

# 启用GDB调试服务器
cargo osdk run --gdb-server                    # 使用默认配置
cargo osdk run --gdb-server addr=localhost:1234 # 自定义地址
cargo osdk run --gdb-server wait-client        # 等待调试器连接
cargo osdk run --gdb-server vscode            # 生成VS Code调试配置

# 自定义内核命令行参数
cargo osdk run --kcmd-args="console=ttyS0,115200 debug"

# 自定义init进程参数
cargo osdk run --init-args="sh -c 'echo Hello from init'"

# 指定initramfs
cargo osdk run --initramfs ./custom_initramfs.cpio.gz

# 自定义QEMU参数
cargo osdk run --qemu-args="-m 4G -smp 2"
```

#### 测试执行
```bash
# 运行所有内核单元测试
cargo osdk test

# 运行特定测试
cargo osdk test basic_functionality
cargo osdk test --test vfs_tests

# 运行集成测试
cargo osdk test --scheme test-env
```

#### 调试功能
```bash
# 启动GDB调试会话
cargo osdk debug

# 连接到特定地址的调试目标
cargo osdk debug --remote localhost:1234
cargo osdk debug --remote .osdk-gdb-socket

# 调试会话会自动：
# - 加载内核符号信息
# - 连接到QEMU GDB stub
# - 设置适当的GDB配置
```

#### 性能分析
```bash
# 基础性能分析（生成火焰图）
cargo osdk profile

# 指定采样次数和间隔
cargo osdk profile --samples 500 --interval 0.05

# 选择输出格式
cargo osdk profile --format flame-graph  # SVG火焰图
cargo osdk profile --format folded      # 折叠的堆栈跟踪
cargo osdk profile --format json        # 原始JSON数据

# 指定输出文件
cargo osdk profile --output kernel-profile.svg

# 解析已有的性能数据
cargo osdk profile --parse ./existing-profile.json --format flame-graph

# 指定CPU掩码（多核系统）
cargo osdk profile --cpu-mask 0xFF  # 分析前8个CPU核心
```

### 代码质量命令

```bash
# 代码检查（检查编译错误）
cargo osdk check

# Clippy代码质量检查
cargo osdk clippy

# 生成文档
cargo osdk doc

# 这些命令会：
# - 自动设置正确的目标架构
# - 启用内核特定的配置
# - 处理多个crate的检查
```

## 架构设计深入解析

### 主要源码模块结构

#### 1. `src/main.rs` & `src/cli.rs` - 命令行界面
- **基于clap的现代CLI**: 提供类型安全的参数解析和验证
- **分层命令结构**: 支持子命令、全局选项和特定选项的组合
- **环境变量集成**: 支持通过环境变量覆盖配置选项
- **智能默认值**: 根据项目环境自动推断合理的默认配置

#### 2. `src/commands/` - 命令实现模块
- **build/**: 构建系统核心
  - `mod.rs`: 构建流程协调和缓存管理
  - `bin.rs`: ELF文件处理和内核二进制生成
  - `grub.rs`: GRUB启动器集成和ISO镜像生成
  - `qcow2.rs`: QCOW2虚拟磁盘镜像转换
- **new/**: 项目模板生成
  - 内核项目和库项目的模板文件
  - 自动化依赖配置和工作空间设置
- **run.rs**: 虚拟机运行管理
  - QEMU参数生成和进程管理
  - GDB服务器配置和VS Code集成
  - 启动方法适配和错误处理
- **debug.rs**: 调试会话管理
  - GDB自动配置和符号加载
  - 远程调试支持
- **profile.rs**: 性能分析工具
  - 基于GDB的堆栈采样
  - 多种输出格式支持（火焰图、JSON等）
- **test.rs**: 测试执行框架

#### 3. `src/config/` - 配置管理系统
- **manifest.rs**: OSDK.toml解析和验证
  - TOML配置文件的完整解析
  - 项目类型识别和验证
  - 工作空间和单独项目的配置合并
- **scheme/**: 配置scheme系统
  - `action.rs`: 构建和运行动作配置
  - `boot.rs`: 启动方法和内核参数配置
  - `grub.rs`: GRUB启动器特定配置
  - `qemu.rs`: QEMU虚拟机配置和参数验证
- **unix_args.rs**: Unix风格参数处理工具

#### 4. `src/bundle/` - 打包和分发系统
- **mod.rs**: Bundle生命周期管理
  - 构建产物的打包和版本管理
  - 配置一致性验证和缓存机制
- **vm_image.rs**: 虚拟机镜像生成
  - ISO镜像和QCOW2磁盘镜像创建
  - 多种启动方法的镜像格式支持
- **file.rs**: 文件系统打包
  - Initramfs处理和文件捆绑
- **bin.rs**: 内核二进制文件处理
  - ELF文件元数据分析和验证
  - 多架构二进制文件支持

#### 5. `src/arch.rs` - 多架构支持
- 架构抽象和目标三元组管理
- 架构特定的工具链和QEMU配置
- 交叉编译支持

### 配置系统 (OSDK.toml) 详解

OSDK的配置系统是其核心特性之一，提供了灵活而强大的项目配置能力。

#### 基础配置结构
```toml
# 项目类型声明（可选，可通过项目结构推断）
project_type = "kernel"  # 或 "library"

# 支持的架构列表（用于scheme验证）
supported_archs = ["x86_64", "riscv64"]

# 通用启动配置
[boot]
method = "grub-rescue-iso"  # 或 "grub-qcow2", "qemu-direct"
kcmd_args = ["console=ttyS0", "debug"]
init_args = ["sh", "-l"]
initramfs = "path/to/initramfs.cpio.gz"

# GRUB启动器配置
[grub]
protocol = "multiboot2"  # 或 "linux", "multiboot"
display_grub_menu = false
grub_mkrescue = "/usr/bin/grub-mkrescue"

# QEMU虚拟机配置
[qemu]
args = '''
    -machine q35
    -cpu host
    -m 8G
    -smp 4
    -nographic
    -serial stdio
'''
path = "/usr/bin/qemu-system-x86_64"
bootdev_append_options = ",if=virtio"

# 构建配置
[build]
profile = "dev"  # 或 "release"
features = ["debug_symbols", "smp"]
no_default_features = false
strip_elf = false
linux_x86_legacy_boot = false
encoding = "raw"  # 或 "gzip", "bzip2", "lzma", "lz4"

# 运行时特殊配置（继承上述默认配置）
[run]
# 运行时可以覆盖任何默认配置

[run.boot]
kcmd_args = ["console=ttyS0", "TERM=xterm-256color"]

[run.qemu]
args = "$(./tools/qemu_args.sh normal)"  # 支持shell命令求值

# 测试特殊配置
[test]
[test.boot]
method = "qemu-direct"
[test.qemu]
args = "$(./tools/qemu_args.sh test)"
```

#### Scheme系统详解
Scheme系统是OSDK的高级配置功能，允许预定义多套完整的配置方案：

**微虚拟机优化scheme**:
```toml
[scheme."microvm"]
boot.method = "qemu-direct"
build.strip_elf = true
build.profile = "release"
qemu.args = '''
    -machine microvm,pic=on,pit=on
    -cpu host
    -m 1G
    -nodefaults
    -no-user-config
    -nographic
    -serial stdio
    -kernel-irqchip=off
'''
```

**Intel TDX机密计算scheme**:
```toml
[scheme."tdx"]
supported_archs = ["x86_64"]
build.features = ["cvm_guest"]
boot.method = "grub-qcow2"
grub.boot_protocol = "linux"
qemu.args = '''
    -name process=tdxvm,debug-threads=on
    -machine q35,kernel_irqchip=split,confidential-guest-support=tdx
    -object tdx-guest,sept-ve-disable=on,id=tdx
    -cpu host,-kvm-steal-time,pmu=off
    -bios /usr/share/qemu/OVMF.fd
    -m 8G -smp 1 -nographic
    -device virtio-net-pci,netdev=mynet0
    -netdev user,id=mynet0
'''
```

**RISC-V竞赛环境scheme**:
```toml
[scheme."oscomp-riscv"]
boot.method = "qemu-direct"
build.strip_elf = false
qemu.args = '''
    -machine virt
    -cpu rv64,zba=true,zbb=true
    -m 1G -smp 1 -nographic
    -bios default
    -drive file=./sdcard-rv.img,if=none,format=raw,id=x0
    -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
    -device virtio-net-device,netdev=net
    -netdev user,id=net
    -no-reboot -rtc base=utc
'''
```

**IOMMU测试scheme**:
```toml
[scheme."iommu"]
supported_archs = ["x86_64"]
qemu.args = '''
    -machine q35,kernel-irqchip=split
    -device intel-iommu,intremap=on,device-iotlb=on
    -cpu Icelake-Server,+x2apic
    -m 8G -smp 4 -nographic
'''
```

### 多架构支持系统

OSDK提供全面的多架构支持，不仅覆盖主流架构，还为每种架构提供了专门优化的配置。

#### 支持的架构详解

**x86_64架构**:
- **目标三元组**: `x86_64-unknown-none`
- **QEMU模拟器**: `qemu-system-x86_64`
- **特殊特性**:
  - Intel TDX机密计算支持
  - IOMMU虚拟化支持
  - 多种启动协议（Multiboot, Multiboot2, Linux, EFI）
  - 高性能特性优化（ERMSB指令集）
  - 完整的调试符号支持

**RISC-V 64位架构**:
- **目标三元组**: `riscv64gc-unknown-none-elf`
- **QEMU模拟器**: `qemu-system-riscv64`
- **特殊特性**:
  - 完整的RV64GC指令集支持
  - Zba、Zbb扩展指令优化
  - SBI固件集成
  - 竞赛环境专门配置
  - PCI和MMIO设备支持

**LoongArch 64位架构**:
- **目标三元组**: `loongarch64-unknown-none`
- **QEMU模拟器**: `qemu-system-loongarch64`
- **特殊特性**:
  - 实验性支持
  - panic=abort模式强制启用
  - 龙芯专用虚拟化特性

**AArch64架构** (规划中):
- **目标三元组**: `aarch64-unknown-none`
- **QEMU模拟器**: `qemu-system-aarch64`

#### 架构检测和选择机制
```rust
// OSDK会自动检测主机架构并选择对应的默认目标
pub fn get_default_arch() -> Arch {
    // 通过rustc -vV获取主机信息
    // 智能映射到支持的目标架构
}
```

### 构建系统集成

#### 高级Cargo集成
OSDK不仅仅是Cargo的包装器，而是深度集成了Cargo的构建系统：

**自动配置管理**:
- 目标架构的自动设置和验证
- 工具链版本一致性检查
- 依赖关系解析和冲突检测
- 工作空间级别的配置继承

**内核专用编译选项**:
```rust
let rustflags = vec![
    "-C relocation-model=static",  // 禁用位置无关代码
    "-C relro-level=off",          // 禁用RELRO保护
    "-C no-redzone=y",             // 禁用红区优化
    "-C panic=unwind",             // 启用panic展开（除LoongArch外）
    "--check-cfg cfg(ktest)",      // 内核测试配置检查
];
```

**链接脚本管理**:
- 架构特定的链接脚本自动选择
- 自定义链接器参数注入
- 符号表和调试信息优化

#### 启动方法详解

**GRUB Rescue ISO (`grub-rescue-iso`)**:
- 生成可启动的ISO镜像
- 支持Multiboot和Multiboot2协议
- 自动生成GRUB配置文件
- 适用于物理机和完整虚拟化环境
- 支持Legacy BIOS和UEFI启动

**GRUB QCOW2 (`grub-qcow2`)**:
- 基于QCOW2格式的虚拟磁盘
- 支持快照和增量存储
- 适用于机密计算环境（如TDX）
- 更好的存储效率和性能

**QEMU直接启动 (`qemu-direct`)**:
- 绕过传统启动加载器
- 最快的启动速度
- 适用于开发和测试环境
- 支持直接内核参数传递

### QEMU集成系统

#### 智能参数管理
OSDK的QEMU集成不仅仅是简单的参数传递，而是包含了完整的配置验证和优化系统：

**参数分类和验证**:
```rust
// 多值参数（可以重复多次）
const MULTI_VALUE_KEYS: &[&str] = &[
    "-device", "-chardev", "-object", "-netdev", "-drive", "-cdrom",
];

// 单值参数（只能设置一次）
const SINGLE_VALUE_KEYS: &[&str] = &[
    "-cpu", "-machine", "-m", "-serial", "-monitor", "-display"
];

// 无值参数（标志位）
const NO_VALUE_KEYS: &[&str] = &[
    "--no-reboot", "-nographic", "-enable-kvm"
];

// 禁止设置的参数（由OSDK管理）
const NOT_ALLOWED_TO_SET_KEYS: &[&str] = &[
    "-kernel", "-append", "-initrd"
];
```

**动态参数求值**:
```toml
# 支持Shell命令求值，运行时动态生成参数
[qemu]
args = "$(./tools/qemu_args.sh $(uname -m))"

# 支持环境变量替换
args = "-m ${MEM:-8G} -smp ${SMP:-4}"
```

#### 调试集成

**GDB服务器自动配置**:
```bash
# 启用Unix域套接字调试
cargo osdk run --gdb-server addr=.osdk-gdb-socket,wait-client

# 启用TCP调试
cargo osdk run --gdb-server addr=localhost:1234

# VS Code集成
cargo osdk run --gdb-server vscode
```

**VS Code调试配置自动生成**:
OSDK能够自动生成并管理VS Code的`launch.json`配置文件：
```json
{
    "name": "Debug Kernel (OSDK)",
    "type": "cppdbg",
    "request": "launch",
    "program": "${workspaceFolder}/target/osdk/kernel-name/kernel-name",
    "miDebuggerServerAddress": "localhost:1234",
    "miDebuggerPath": "gdb",
    "setupCommands": [
        {"text": "set arch i386:x86-64"},
        {"text": "target remote localhost:1234"}
    ]
}
```

#### 性能优化和监控

**多种性能分析模式**:
```bash
# CPU火焰图分析
cargo osdk profile --format flame-graph --samples 1000

# 内存分配分析
cargo osdk profile --format json --cpu-mask 0x1 --output memory-profile.json

# 实时性能监控
cargo osdk profile --interval 0.01 --samples 10000
```

### 项目模板系统

#### 内核项目模板结构
```
my-kernel/
├── Cargo.toml                 # Rust包配置
├── OSDK.toml                  # OSDK项目配置
├── rust-toolchain.toml        # 工具链版本固定
├── src/
│   └── lib.rs                 # 内核入口点
├── linker-scripts/            # 架构特定链接脚本
│   ├── x86_64.ld
│   ├── riscv64.ld
│   └── loongarch64.ld
└── .vscode/                   # VS Code配置（可选）
    └── launch.json
```

**Cargo.toml自动配置**:
```toml
[package]
name = "my-kernel"
version = "0.1.0"
edition = "2021"

[dependencies]
ostd = { git = "https://github.com/asterinas/asterinas", package = "ostd" }

[[bin]]
name = "my-kernel"
path = "src/lib.rs"

[profile.dev]
panic = "unwind"

[profile.release]
panic = "unwind"
lto = true
```

**内核模板代码**:
```rust
#![no_std]
#![deny(unsafe_code)]

use ostd::prelude::*;

#[ostd::main]
fn kernel_main() {
    println!("Hello world from guest kernel!");
    
    // 内核初始化代码
    ostd::mm::init();
    ostd::task::init();
    
    // 用户空间启动
    let init_process = ostd::process::spawn("/bin/init", &[]);
    init_process.wait();
}
```

#### 库项目模板
```
my-lib/
├── Cargo.toml
├── OSDK.toml
├── src/
│   ├── lib.rs
│   └── tests.rs
├── tests/
│   └── integration_test.rs
└── examples/
    └── basic_usage.rs
```

### 开发工作流集成

#### 持续集成支持
OSDK提供了完整的CI/CD集成能力：

**多架构自动化测试**:
```yaml
# .github/workflows/osdk-ci.yml
name: OSDK CI
on: [push, pull_request]

jobs:
  test-multi-arch:
    strategy:
      matrix:
        arch: [x86_64, riscv64, loongarch64]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - name: Install OSDK
        run: cargo install cargo-osdk
      - name: Build for ${{ matrix.arch }}
        run: cargo osdk build --target-arch ${{ matrix.arch }}
      - name: Run tests
        run: cargo osdk test --target-arch ${{ matrix.arch }}
```

**性能回归检测**:
```bash
# 性能基准测试
cargo osdk profile --format json --output baseline.json
# CI中比较性能数据
python3 tools/compare-performance.py baseline.json current.json
```

#### IDE和编辑器支持

**Rust Analyzer配置**:
OSDK自动生成适当的`.vscode/settings.json`:
```json
{
    "rust-analyzer.cargo.target": "x86_64-unknown-none",
    "rust-analyzer.checkOnSave.allTargets": false,
    "rust-analyzer.cargo.features": ["ostd/smp", "ostd/intel_tdx"]
}
```

**调试器集成**:
- GDB配置文件自动生成
- 符号路径自动设置
- 源码级调试支持
- 内核数据结构可视化

### 测试框架

#### 内核单元测试支持
```rust
#[cfg(ktest)]
mod tests {
    use super::*;
    
    #[ktest]
    fn test_memory_allocation() {
        let ptr = ostd::mm::alloc_pages(1);
        assert!(!ptr.is_null());
        ostd::mm::free_pages(ptr, 1);
    }
    
    #[ktest]
    fn test_process_creation() {
        let pid = ostd::task::spawn_kernel_task(test_task);
        assert!(pid > 0);
    }
}
```

#### 集成测试框架
```bash
# 系统调用兼容性测试
cargo osdk test --test syscall_compat

# 文件系统功能测试
cargo osdk test --test vfs_functionality

# 网络栈测试
cargo osdk test --test network_stack

# 性能基准测试
cargo osdk test --test performance_benchmarks
```

### 性能分析工具

#### 高级性能分析功能
**CPU分析**:
```bash
# 生成详细的CPU使用火焰图
cargo osdk profile --samples 5000 --format flame-graph --output cpu-profile.svg

# 分析特定CPU核心
cargo osdk profile --cpu-mask 0x0F --samples 1000

# 高频采样模式
cargo osdk profile --interval 0.001 --samples 100000
```

**内存分析**:
```rust
// 内置内存分析支持
#[ostd::profile_memory]
fn memory_intensive_function() {
    // 函数执行时的内存使用会被自动跟踪
}
```

**I/O性能分析**:
```bash
# 磁盘I/O分析
cargo osdk profile --trace-disk-io --output disk-analysis.json

# 网络I/O分析
cargo osdk profile --trace-network-io --format folded
```

### 错误处理和诊断

#### 智能错误诊断
OSDK提供了完整的错误诊断和建议系统：

**构建错误分析**:
```
错误: 链接器错误 - undefined symbol: __aster_boot_info
建议: 
1. 检查是否正确配置了启动协议
2. 确认链接脚本是否包含必要的段
3. 验证架构特定的启动代码是否存在
```

**运行时错误跟踪**:
```bash
# 自动解析内核panic堆栈跟踪
cargo osdk run 2>&1 | tools/symbolize-panic.py

# 实时内核日志分析
tail -f qemu.log | cargo osdk analyze-log
```

### 扩展性和定制化

#### 插件系统设计
OSDK采用模块化设计，支持第三方扩展：

```rust
// 自定义命令扩展接口
pub trait OsdkCommand {
    fn name(&self) -> &str;
    fn execute(&self, args: &[String]) -> Result<()>;
}

// 自定义架构支持
pub trait ArchSupport {
    fn target_triple(&self) -> &str;
    fn linker_script(&self) -> &str;
    fn rustflags(&self) -> Vec<&str>;
}
```

#### 配置扩展机制
```toml
# OSDK.toml支持自定义字段
[custom]
my_extension = "custom_value"

[extensions."my-plugin"]
enabled = true
config_file = "plugin-config.toml"
```

### 最佳实践和开发指导

#### 项目组织建议
```
workspace-root/
├── OSDK.toml                 # 工作空间级配置
├── kernel/                   # 主内核代码
│   ├── OSDK.toml            # 内核特定配置
│   └── src/
├── drivers/                  # 设备驱动模块
│   ├── block/
│   ├── network/
│   └── graphics/
├── libs/                     # 共享库
│   ├── memory/
│   ├── sync/
│   └── collections/
└── tools/                    # 开发工具
    ├── qemu_args.sh
    ├── debug_scripts/
    └── benchmarks/
```

#### 性能优化建议
1. **编译优化**:
   - 生产环境使用`cargo osdk build --release --strip-elf`
   - 启用LTO: `profile.release.lto = true`
   - 目标CPU优化: `RUSTFLAGS="-C target-cpu=native"`

2. **运行时优化**:
   - 使用microvm scheme减少启动时间
   - 根据工作负载调整内存和CPU配置
   - 启用IOMMU提高I/O性能

3. **调试优化**:
   - 开发时保留调试符号: `build.strip_elf = false`
   - 使用专门的调试scheme配置
   - 启用详细的内核日志

#### 安全性考虑
1. **机密计算**:
   - 使用TDX scheme进行机密计算开发
   - 启用内存加密和attestation
   - 注意机密数据的处理流程

2. **内存安全**:
   - 启用所有Rust安全检查
   - 使用`#![deny(unsafe_code)]`限制unsafe代码
   - 定期进行内存泄漏检测

3. **权限管理**:
   - 实施最小权限原则
   - 使用能力系统限制组件权限
   - 启用用户空间隔离

## 总结

OSDK作为NexusOS的核心开发工具包，不仅仅是一个构建工具，更是一个完整的操作系统开发平台。它通过统一的配置系统、灵活的架构支持、强大的调试能力和完整的性能分析工具，极大地简化了现代操作系统的开发流程。

无论是学术研究、产品开发还是教学实验，OSDK都能够提供适合的工具和配置，帮助开发者专注于操作系统本身的逻辑实现，而不需要在开发工具链上投入过多精力。随着项目的持续发展，OSDK将继续演进，为操作系统开发社区提供更加先进和易用的开发体验。