# NexusOS

基于 Rust 的多核、异步、**框内核 (Framekernel)** 架构的操作系统。

## 项目结构

ostd                  # 操作系统标准库 (OS Standard Library)，提供底层抽象和核心服务
├── libs              # 依赖的第三方库或辅助工具库
│   ├── align_ext     # 内存地址对齐工具
│   ├── id-alloc      # 通用唯一 ID 分配器 (如 PID, FD)
│   ├── int-to-c-enum # 整数到 C 风格枚举的安全转换
│   ├── linux-bzimage # Linux `bzImage` 内核格式解析库
│   ├── maitake       # 用于 `no_std` 的轻量级异步运行时
│   ├── maitake-sync  # `maitake` 的同步原语
│   ├── ostd-macros   # `ostd` 的过程宏，用于简化代码
│   ├── ostd-test     # 内核环境测试框架
│   └── virtio-drivers# VirtIO 半虚拟化 I/O 驱动，用于在虚拟机中高效运行
└── src               # 核心源码
    ├── arch          # 体系结构相关代码 (e.g., RISC-V, x86_64)
    │   ├── riscv     # RISC-V 架构的特有实现
    │   └── x86       # x86-64 架构的特有实现
    ├── boot          # 系统引导加载程序，完成到内核的过渡
    ├── bus           # 系统总线驱动 (e.g., PCI, MMIO)
    ├── collections   # 内核环境下的特供数据结构
    ├── cpu           # CPU 核心功能封装 (e.g., Per-CPU, 上下文切换)
    ├── drivers       # 设备驱动程序 (e.g., Console, Serial, Block Device)
    ├── mm            # 内存管理 (物理/虚拟内存、页表、VMO)
    ├── sync          # 内核并发控制的同步原语 (Mutex, Spinlock)
    ├── task          # 任务、线程、进程的抽象和管理
    ├── timer         # 硬件时钟和定时器管理
    └── trap          # 陷入处理 (中断、异常、系统调用)

kernel                # 内核主程序
├── comps             # 内核组件，大型、独立的模块
│   ├── another_ext4  # ext4 文件系统实现
│   └── vfs           # 虚拟文件系统 (Virtual File System) 抽象层
├── libs              # 内核专用的依赖库
│   ├── aster-rights        # 基于能力 (Capability) 的权限管理系统
│   ├── aster-rights-proc   # `aster-rights` 的过程宏
│   ├── block-dev           # 块设备抽象
│   ├── nexus-error         # NexusOS 自定义错误处理
│   ├── typeflags           # 使用类型作为编译期标志的工具
│   └── typeflags-util      # `typeflags` 的辅助库
└── src               # 内核核心功能实现
    ├── syscall       # 系统调用接口层
    ├── thread        # 线程与文件描述符 (FD) 管理
    ├── time          # 内核时间管理
    └── vm            # 虚拟内存管理

## 项目文档

[初赛文档](docs/初赛/doc.md)
[初赛文档](docs/初赛/NexusOS%20初赛文档.pdf)
[初赛幻灯片](docs/初赛/slide.qmd)
[初赛幻灯片](docs/初赛/slide.pptx)



## 许可证 (License)

NexusOS 项目的主要源代码和文档基于 **Mozilla Public License Version 2.0 (MPL 2.0)** 发布。完整的许可证文本请参见项目根目录下的 [LICENSE-MPL](LICENSE-MPL) 文件。

本项目是在 [Asterinas project](https://github.com/asterinas/asterinas) (同样基于 MPL 2.0) 的基础上进行修改和开发的。

此外，NexusOS 包含了一些来自第三方项目的组件，这些组件可能使用不同的许可证：

*   `ostd/libs/maitake/` 目录下的代码：源自 [mycelium/maitake](https://github.com/hawkw/mycelium/tree/main/maitake)，基于 **MIT License** 发布。其许可证文件位于 [ostd/libs/maitake/LICENSE](ostd/libs/maitake/LICENSE)。
*   `ostd/libs/maitake-sync/` 目录下的代码：源自 [mycelium/maitake-sync](https://github.com/hawkw/mycelium/tree/main/maitake-sync)，基于 **MIT License** 发布。其许可证文件位于 [ostd/libs/maitake-sync/LICENSE](ostd/libs/maitake-sync/LICENSE)。
*   `kernel/comps/another_ext4/` 目录下的代码：源自 [ext4_rs](https://github.com/yuoo655/ext4_rs)，基于 **MIT License** 发布。其许可证文件位于 [kernel/comps/another_ext4/LICENSE](kernel/comps/another_ext4/LICENSE)。
*   `ostd/libs/virtio-drivers/` 目录下的代码：源自 [rcore-os/virtio-drivers](https://github.com/rcore-os/virtio-drivers)，基于 **MIT License** 发布。其许可证文件位于 [ostd/libs/virtio-drivers/LICENSE](ostd/libs/virtio-drivers/LICENSE)。
*   以下组件同样源自 [Asterinas project](https://github.com/asterinas/asterinas)，并基于 **MPL 2.0** 许可证：
    *   `kernel/libs/aster-rights`
    *   `kernel/libs/aster-rights-proc`
    *   `kernel/libs/typeflags`
    *   `kernel/libs/typeflags-util`
    *   `ostd/libs/align_ext`
    *   `ostd/libs/id-alloc`
    *   `ostd/libs/int-to-c-enum`
    *   `ostd/libs/linux-bzimage`
    *   `ostd/libs/ostd-macros`
    *   `ostd/libs/ostd-test`

还有，本项目的 ostd 部分和 kernel 中的 vm 部分的绝大部分内容也都源自 [Asterinas project](https://github.com/asterinas/asterinas)，并基于 **MPL 2.0** 许可证。

项目中各部分的版权归其各自的贡献者所有。使用或分发 NexusOS 时，请确保遵守 MPL 2.0 许可证以及所包含组件的原始许可证条款。

本项目参考借鉴了：
[Asterinas project](https://github.com/asterinas/asterinas)
[umi](https://github.com/js2xxx/umi)

演示视频：[初赛.mkv](https://pan.baidu.com/s/1M2gC5rop7vnsnN8tYxCGhA?pwd=sxda)