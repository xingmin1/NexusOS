# NexusOS 项目开发指南

本文件为Claude Code (claude.ai/code) 提供在此代码库中工作的详细指南。

## 项目概述

NexusOS是一个基于Rust的类Unix操作系统，采用创新的**framekernel架构**，结合了微内核的安全性与宏内核的性能优势。项目构建在Asterinas框架之上，采用多核异步设计，支持三种主流架构。

### Framekernel架构优势

framekernel架构将系统分为两个核心组件：

- **OS Framework (ostd)**：可信计算基础(TCB)，包含少量unsafe Rust代码，为上层提供安全的API接口
- **OS Services (kernel)**：大型代码库，完全使用safe Rust实现系统调用、文件系统、驱动程序等服务

这种架构实现了：
- **安全性**：最小化unsafe代码暴露面，将安全关键代码隔离在ostd中
- **性能**：通过静态分发和编译时优化，避免传统微内核的IPC开销
- **可维护性**：清晰的模块边界，便于开发和调试

### 多架构支持

NexusOS提供真正的多架构支持：
- **RISC-V (riscv64)**：主要开发平台，完整功能支持
- **x86_64**：完整支持，包括Intel TDX技术
- **LoongArch (loongarch64)**：实验性支持，正在完善中

## 核心构建命令

### 系统构建
```bash
# 构建内核（默认RISC-V）
make build

# 指定架构构建
make build ARCH=x86_64
make build ARCH=riscv64
make build ARCH=loongarch64

# 发布版本构建
make build RELEASE=1
make build RELEASE_LTO=1  # 链接时优化
```

### 运行与测试
```bash
# 在QEMU中运行内核
make run

# 使用特定配置方案运行
make run SCHEME="oscomp-riscv"
make run SCHEME="oscomp-loongarch"

# 运行系统调用测试
make run AUTO_TEST=syscall

# 运行通用测试
make run AUTO_TEST=test

# 单元测试
make test      # 非OSDK crate的主机测试
make ktest     # QEMU中的内核测试
```

### 开发工具
```bash
# 安装OSDK开发套件
make install_osdk

# 代码格式化
make format

# 代码检查（格式化+linting+clippy）
make check

# 生成文档
make docs

# GDB调试
make gdb_server  # 在一个终端中运行
make gdb_client  # 在另一个终端中运行
```

### 直接使用OSDK
```bash
# 使用OSDK构建内核
cd kernel && cargo osdk build

# 运行内核
cd kernel && cargo osdk run

# 运行测试
cd kernel && cargo osdk test

# 创建新项目
cargo osdk new --kernel my-kernel

# OSDK项目管理
cargo osdk info        # 显示项目信息
cargo osdk check       # 检查项目配置
cargo osdk debug       # 调试模式运行
```

## 详细架构分析

### 目录结构与功能模块

#### ostd/ - OS标准库（framekernel基础）
作为framekernel架构的可信基础，ostd提供：

**架构支持** (`src/arch/`)：
- 真实的三架构支持（x86_64、RISC-V、LoongArch）
- 架构特定的底层抽象
- 启动引导和硬件初始化

**内存管理** (`src/mm/`)：
- 完整的物理内存管理系统
- 多级页表管理
- VMO/VMAR虚拟内存抽象
- DMA缓冲区管理
- 堆分配器（buddy allocator）

**异步任务系统** (`src/task/`)：
- 基于maitake的异步运行时
- 抢占式多任务调度
- 内核级异步执行器
- 任务优先级管理

**同步原语** (`src/sync/`)：
- 丰富的同步机制
- 无锁数据结构
- 读写锁、互斥锁等

**设备驱动框架** (`src/drivers/`)：
- VirtIO设备驱动
- 设备抽象层
- 中断处理机制

#### kernel/ - 主内核实现
实现完整的操作系统服务：

**系统调用支持**：
- 32个主要系统调用实现
- 支持glibc和musl C库
- 异步系统调用设计
- Linux兼容性

**安全模型**：
- 基于aster-rights的能力安全模型
- 细粒度权限控制
- 资源访问控制

**进程与线程**：
- 用户态进程管理
- 线程调度
- 信号处理

#### kernel/comps/ - 内核组件
采用清晰的组件化架构：

**VFS组件** - 虚拟文件系统：
- 先进的异步架构设计
- 静态分发性能优化
- 能力导向的接口设计
- 高效的路径处理系统
- 智能缓存机制
- 作为系统的中心协调点

**ext4组件** - 文件系统实现：
- 完全no_std实现
- 基于ext4_rs项目并经过Metis验证
- 支持现代ext4特性
- 与ostd深度集成
- 完整的日志和恢复机制

#### osdk/ - OS开发套件
提供完整的开发工具链：

**项目管理**：
- 项目创建和配置
- 依赖管理
- 构建系统集成

**构建系统**：
- 多架构交叉编译
- 链接时优化
- 增量构建支持

**测试框架**：
- 单元测试执行
- 集成测试支持
- 性能基准测试

**调试工具**：
- QEMU集成
- GDB调试支持
- 性能分析工具

### 核心技术特性

#### 异步设计
NexusOS在整个系统中采用async/await模式：
- **maitake运行时**：定制的no_std异步运行时
- **ostd::task**：支持异步的抢占式多任务
- **VFS**：完全异步的文件系统操作
- **设备驱动**：异步I/O操作
- **系统调用**：异步系统调用处理

#### 内存管理系统
- **帧分配器**：高效的物理内存管理
- **页表管理**：多级页表，支持大页
- **虚拟内存**：VMO/VMAR抽象，支持内存映射
- **DMA支持**：设备直接内存访问
- **堆管理**：内核堆，支持动态分配

#### 安全机制
- **能力系统**：基于aster-rights的细粒度权限控制
- **类型标志**：编译时配置和验证
- **内存安全**：Rust语言保证 + framekernel隔离
- **权限分离**：最小权限原则

## 测试策略

### 单元测试
```bash
# 主机环境库测试
make test

# 内核模式单元测试
make ktest

# 特定组件测试
cd kernel/comps/ext4 && cargo test
cd ostd && cargo test
```

### 集成测试
```bash
# Linux系统调用兼容性测试
make run AUTO_TEST=syscall

# 通用功能测试
make run AUTO_TEST=test

# 启动序列测试
make run AUTO_TEST=boot

# 文件系统测试
make run AUTO_TEST=fs
```

### 性能基准测试
```bash
# LMbench性能基准
make run BENCHMARK=lmbench

# 系统基准测试
make run BENCHMARK=sysbench

# 内存管理性能测试
make run BENCHMARK=memory

# VFS性能测试
make run BENCHMARK=vfs
```

### 多架构测试
```bash
# RISC-V架构测试
make run ARCH=riscv64 AUTO_TEST=syscall

# x86_64架构测试
make run ARCH=x86_64 AUTO_TEST=syscall

# LoongArch架构测试
make run ARCH=loongarch64 AUTO_TEST=syscall
```

## 开发工作流

### 日常开发流程

1. **本地开发**：
   ```bash
   # 快速迭代开发
   make build && make run
   
   # 代码检查
   make check
   ```

2. **功能开发**：
   ```bash
   # 新功能分支
   git checkout -b feature/new-feature
   
   # 增量构建测试
   make build
   make test
   make ktest
   ```

3. **多架构验证**：
   ```bash
   # 在所有支持的架构上测试
   for arch in riscv64 x86_64 loongarch64; do
     make build ARCH=$arch
     make run ARCH=$arch AUTO_TEST=syscall
   done
   ```

4. **性能优化**：
   ```bash
   # 发布版本性能测试
   make build RELEASE=1
   make run RELEASE=1 BENCHMARK=lmbench
   ```

### 代码提交规范

1. **提交前检查**：
   ```bash
   # 必须通过的检查
   make check      # 代码格式和语法检查
   make test       # 单元测试
   make ktest      # 内核测试
   ```

2. **测试覆盖**：
   - 在debug和release模式下都要测试
   - VFS和内核组件需要集成测试
   - 内存管理变更需要跨架构测试

3. **性能验证**：
   - 关键路径变更需要性能基准测试
   - 内存使用分析
   - 启动时间测试

### 调试指南

#### GDB调试
```bash
# 启动GDB服务器
make gdb_server

# 在另一个终端连接GDB
make gdb_client

# 或者手动连接
gdb kernel/target/riscv64gc-unknown-none-elf/debug/kernel
(gdb) target remote localhost:1234
```

#### 日志调试
```bash
# 启用详细日志
make run LOG_LEVEL=debug

# 特定模块日志
make run LOG_FILTER="vfs,ext4"
```

#### 性能分析
```bash
# 性能分析模式运行
cargo osdk run --profile

# 内存使用分析
make run MEMORY_PROFILE=1
```

## 配置文件说明

### 关键配置文件

- **`Makefile`**：顶层构建系统配置，定义各种构建目标
- **`OSDK.toml`**：OSDK特定的构建和运行配置，包含架构方案
- **`Cargo.toml`**：Rust工作空间配置，包含架构特定依赖
- **`rust-toolchain.toml`**：Rust工具链版本规范

### 环境变量配置

- **`ARCH`**：目标架构（riscv64/x86_64/loongarch64）
- **`SCHEME`**：运行方案（oscomp-riscv/oscomp-loongarch等）
- **`RELEASE`**：发布版本构建标志
- **`AUTO_TEST`**：自动化测试模式
- **`BENCHMARK`**：性能基准测试模式

## 故障排除

### 常见问题

1. **构建失败**：
   - 检查Rust工具链版本
   - 确保OSDK正确安装
   - 验证依赖项是否完整

2. **运行时错误**：
   - 检查QEMU版本兼容性
   - 验证架构配置正确性
   - 查看内核日志输出

3. **测试失败**：
   - 确保在正确的架构上运行测试
   - 检查测试环境设置
   - 查看详细的测试输出

### 性能优化建议

1. **编译优化**：
   - 使用`RELEASE=1`进行发布版本构建
   - 启用LTO：`RELEASE_LTO=1`
   - 针对特定架构优化

2. **运行时优化**：
   - 调整内存分配策略
   - 优化异步任务调度
   - 使用性能分析工具识别瓶颈

## 贡献指南

### 代码风格
- 严格遵循Rust官方代码风格
- 使用`make format`保持一致性
- 提供充分的文档注释

### 测试要求
- 新功能必须包含单元测试
- 系统调用变更需要集成测试
- 性能敏感代码需要基准测试

### 架构兼容性
- 新功能应支持所有目标架构
- 架构特定代码应适当抽象
- 跨架构测试验证

NexusOS项目致力于构建一个高性能、安全、可维护的现代操作系统。通过framekernel架构和Rust语言的强大特性，我们正在重新定义操作系统设计的可能性。