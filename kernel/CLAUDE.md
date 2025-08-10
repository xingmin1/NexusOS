# CLAUDE.md

本文件为Claude Code (claude.ai/code)在kernel（内核主模块）中工作时提供指导。

## 概述

kernel模块是NexusOS的主要内核实现，使用ostd提供的安全API构建完整的操作系统服务。它实现了32个主要系统调用，支持glibc和musl两种C库，采用异步系统调用设计，通过能力安全模型确保系统安全性。

## 关键命令

```bash
# 构建内核
cd kernel && cargo osdk build

# 运行内核测试
cd kernel && cargo osdk run --init-args="/test/run_general_test.sh"

# 运行系统调用测试
cd kernel && cargo osdk run --init-args="/opt/syscall_test/run_syscall_test.sh"

# GDB调试内核
cd kernel && cargo osdk run --gdb-server wait-client,vscode,addr=:12345

# 内核模式单元测试
cd kernel && cargo osdk test
```

## 架构设计

### 核心模块详析

#### `src/syscall/` - 系统调用接口层

NexusOS实现了32个主要系统调用，支持Linux兼容性：

```rust
// 示例：异步系统调用实现
pub async fn sys_read(fd: FileDesc, buf: UserPtr<u8>, count: usize) 
    -> ControlFlow<i32, Option<isize>> {
    
    let current_thread = current_thread();
    let file_handle = current_thread.file_table().get_file(fd)?;
    
    match file_handle.read(buf.as_slice_mut()?).await {
        Ok(bytes_read) => ControlFlow::Continue(Some(bytes_read as isize)),
        Err(e) => ControlFlow::Break(e.error_code()),
    }
}
```

**系统调用列表**：
```rust
static TASKS: [&str; 32] = [
    "brk", "chdir", "clone", "close", "dup2", "dup", "execve", "exit", 
    "fork", "fstat", "getcwd", "getdents", "getpid", "getppid", 
    "gettimeofday", "mkdir_", "mmap", "mount", "munmap", "openat", 
    "open", "pipe", "read", "sleep", "times", "umount", "uname", 
    "unlink", "wait", "waitpid", "write", "yield"
];
```

**关键特性**：
- **异步设计**: 所有系统调用返回`ControlFlow<i32, Option<isize>>`
- **错误处理**: 使用`nexus-error`提供Linux兼容的errno代码
- **C库支持**: 同时支持glibc和musl两种C运行时

#### `src/thread/` - 进程和线程管理系统

**核心生命周期管理**：

**1. 进程创建** (`clone.rs`):
```rust
pub async fn sys_clone(flags: CloneFlags, stack: Option<UserPtr<u8>>) 
    -> Result<ControlFlow<i32, Option<isize>>> {
    
    let current = current_thread();
    let new_thread = ThreadBuilder::new()
        .clone_flags(flags)
        .user_stack(stack)
        .build_from(current).await?;
    
    Ok(ControlFlow::Continue(Some(new_thread.tid() as isize)))
}
```

支持的CLONE标志：
- `CLONE_VM`: 共享虚拟内存空间
- `CLONE_FILES`: 共享文件描述符表
- `CLONE_THREAD`: 创建线程而非进程
- `CLONE_SIGHAND`: 共享信号处理器

**2. 程序执行** (`execve.rs`):
```rust
pub async fn sys_execve(
    pathname: UserPtr<c_char>,
    argv: UserPtr<UserPtr<c_char>>,
    envp: UserPtr<UserPtr<c_char>>
) -> Result<ControlFlow<i32, Option<isize>>> {
    
    let path = pathname.read_cstring()?;
    let loader = ElfLoader::new(&path).await?;
    
    // 加载ELF镜像到新的地址空间
    let entry_point = loader.load_and_map().await?;
    
    // 切换到用户态并执行
    switch_to_user(entry_point, user_stack).await
}
```

**3. ELF加载流程** (`loader/`):

ELF加载包含6个主要阶段：

```rust
// elf_file.rs - ELF文件解析
pub struct ElfFile {
    header: ElfHeader,
    program_headers: Vec<ProgramHeader>,
    sections: Vec<SectionHeader>,
}

// elf_image.rs - 内存布局规划
pub struct ElfImage {
    segments: Vec<ElfSegment>,
    entry_point: VirtAddr,
    interpreter: Option<String>,
}

// elf_mapper.rs - 内存映射执行
impl ElfMapper {
    pub async fn map_to_process(&self, process: &Process) -> Result<VirtAddr> {
        for segment in &self.image.segments {
            let vmo = Vmo::new(segment.size)?;
            let vmar = process.vmar().map(
                segment.vaddr, 
                vmo, 
                segment.permissions
            )?;
        }
        Ok(self.image.entry_point)
    }
}
```

**4. 文件描述符管理** (`fd_table.rs`):
```rust
pub struct FileTable {
    files: BTreeMap<FileDesc, Arc<dyn FileHandle>>,
    next_fd: AtomicU32,
}

impl FileTable {
    pub async fn open(&self, path: &str, flags: OpenFlags) 
        -> Result<FileDesc> {
        let path_resolver = get_path_resolver();
        let file = path_resolver.open(path, flags).await?;
        let fd = self.allocate_fd();
        self.files.insert(fd, file);
        Ok(fd)
    }
}
```

#### `src/vm/` - 虚拟内存管理

NexusOS使用VMO(虚拟内存对象)/VMAR(虚拟内存地址区域)抽象：

**1. 内存映射** (`mmap.rs`):
```rust
pub async fn sys_mmap(
    addr: Option<VirtAddr>,
    len: usize,
    prot: ProtFlags,
    flags: MapFlags,
    fd: Option<FileDesc>,
    offset: usize,
) -> Result<VirtAddr> {
    
    let process_vm = current_thread().process_vm();
    
    match flags {
        MapFlags::ANONYMOUS => {
            // 匿名映射
            let vmo = Vmo::new(len)?;
            process_vm.map_anonymous(addr, vmo, prot).await
        },
        MapFlags::SHARED => {
            // 文件映射
            let file = current_thread().file_table().get(fd.unwrap())?;
            let vmo = file.create_vmo(offset, len).await?;
            process_vm.map_shared(addr, vmo, prot).await
        },
        _ => Err(Error::EINVAL),
    }
}
```

**2. 能力安全的内存管理** (`vmar/`, `vmo/`):
```rust
// 使用aster-rights进行能力检查
#[aster_rights::require(Read)]
pub fn read_memory(vmar: &Vmar, addr: VirtAddr, buf: &mut [u8]) 
    -> Result<usize> {
    // 实现需要Read能力
    vmar.read_at(addr, buf)
}

#[aster_rights::require(Write)]  
pub fn write_memory(vmar: &Vmar, addr: VirtAddr, buf: &[u8]) 
    -> Result<usize> {
    // 实现需要Write能力
    vmar.write_at(addr, buf)
}
```

**3. 页错误处理** (`page_fault_handler.rs`):
```rust
pub async fn handle_page_fault(
    addr: VirtAddr, 
    error_code: PageFaultError
) -> Result<()> {
    let process_vm = current_thread().process_vm();
    
    match process_vm.find_mapping(addr) {
        Some(mapping) => {
            // 延迟分配或写时复制
            if error_code.is_write() && mapping.is_cow() {
                mapping.handle_cow_fault(addr).await?;
            } else {
                mapping.allocate_page(addr).await?;
            }
        },
        None => {
            // 访问无效地址，发送SIGSEGV信号
            send_signal(current_thread(), SIGSEGV);
        }
    }
    
    Ok(())
}
```

### 与ostd和VFS的集成

#### ostd框架集成
```rust
// 使用ostd的异步任务系统
use ostd::task::spawn;

pub async fn create_init_process() -> Result<ThreadRef> {
    spawn(async {
        let init_thread = ThreadBuilder::new()
            .path("/sbin/init")
            .spawn().await?;
        init_thread.wait().await
    }, None).await
}
```

#### VFS组件集成
```rust
use vfs::{VFS_MANAGER, get_path_resolver};

pub async fn sys_openat(
    dirfd: FileDesc,
    pathname: UserPtr<c_char>,
    flags: OpenFlags,
) -> Result<ControlFlow<i32, Option<isize>>> {
    
    let path = pathname.read_cstring()?;
    let resolver = get_path_resolver();
    
    // 通过VFS解析和打开文件
    let file_handle = if dirfd == AT_FDCWD {
        resolver.open(&path, flags).await?
    } else {
        let dir_handle = current_thread().file_table().get_dir(dirfd)?;
        resolver.openat(dir_handle, &path, flags).await?
    };
    
    let fd = current_thread().file_table().insert(file_handle);
    Ok(ControlFlow::Continue(Some(fd as isize)))
}
```

### 安全与权限系统

#### aster-rights能力系统
```rust
use aster_rights::{Require, Full, Read, Write};

// 示例：文件操作的能力检查
#[aster_rights::require(Read)]
pub fn read_file_content(file: &FileHandle<Read>) -> Result<Vec<u8>> {
    // 只能读取具有Read能力的文件
    file.read_to_end()
}

#[aster_rights::require(Write)]  
pub fn write_file_content(file: &FileHandle<Write>, data: &[u8]) -> Result<()> {
    // 只能写入具有Write能力的文件
    file.write_all(data)
}
```

#### 用户态权限检查
```rust
pub fn verify_user_pointer<T>(ptr: UserPtr<T>) -> Result<&mut T> {
    let current = current_thread();
    let process_vm = current.process_vm();
    
    // 检查地址是否在用户空间
    if !ptr.addr().is_user_space() {
        return Err(Error::EFAULT);
    }
    
    // 检查内存区域是否可访问
    process_vm.verify_access(ptr.addr(), size_of::<T>(), AccessFlags::READ)?;
    
    Ok(unsafe { ptr.as_ref_mut() })
}
```

### 错误处理模式

#### 统一错误处理
```rust
use nexus_error::{Error, Result};

// 系统调用错误映射
impl From<VfsError> for Error {
    fn from(e: VfsError) -> Self {
        match e {
            VfsError::NotFound => Error::ENOENT,
            VfsError::PermissionDenied => Error::EACCES,
            VfsError::InvalidArgument => Error::EINVAL,
            // ... 其他映射
        }
    }
}
```

#### 系统调用返回约定
```rust
// 所有系统调用遵循相同的返回模式
pub type SyscallResult = ControlFlow<i32, Option<isize>>;

// 成功返回值
ControlFlow::Continue(Some(result_value))
// 错误返回errno
ControlFlow::Break(errno)
// 特殊返回（如阻塞调用）
ControlFlow::Continue(None)
```

## 开发规范

### 系统调用实现规范
1. **异步设计**: 所有系统调用必须是异步的
2. **错误处理**: 使用统一的errno代码映射
3. **能力检查**: 在资源访问前验证权限
4. **参数验证**: 检查用户空间指针和参数有效性
5. **资源清理**: 确保错误路径上的资源正确释放

### 内存管理规范
1. **VMO/VMAR使用**: 所有内存操作通过ostd抽象进行
2. **引用计数**: 正确管理共享资源的生命周期
3. **页错误处理**: 实现按需分页和写时复制
4. **地址空间隔离**: 确保进程间内存隔离

### 进程管理规范
1. **状态一致性**: 维护线程状态的一致性
2. **资源限制**: 实施资源限制和配额
3. **信号处理**: 实现POSIX兼容的信号语义
4. **调度公平性**: 确保调度算法的公平性

### 测试策略

#### 单元测试
```bash
cd kernel && cargo osdk test
```

#### 集成测试
```bash
# 系统调用兼容性测试
make run AUTO_TEST=syscall

# 通用功能测试
make run AUTO_TEST=test
```

#### 性能测试
```bash
# 系统调用性能基准
make run BENCHMARK=syscall_bench

# 内存管理性能
make run BENCHMARK=memory_bench
```

### 当前实现状态

#### 已实现功能
- ✅ 32个主要系统调用
- ✅ ELF程序加载和执行
- ✅ 虚拟内存管理(mmap/munmap/brk)
- ✅ 文件描述符管理
- ✅ 进程创建和管理(clone/fork/wait)
- ✅ 基本文件I/O操作

#### 待完善功能
- ⏳ 完整的信号处理机制
- ⏳ 进程组和会话管理
- ⏳ 完整的POSIX时间接口
- ⏳ 扩展的内存保护机制
- ⏳ 更多系统调用的实现

### 关键代码路径

#### 系统调用分发
`src/syscall.rs` 中的分发逻辑将系统调用号映射到具体实现。

#### 进程启动序列
`src/lib.rs:main()` → VFS初始化 → 测试进程启动

#### 内存映射路径
`sys_mmap` → `ProcessVm` → `Vmar::map` → `ostd::mm`

kernel模块作为NexusOS的核心服务层，展示了如何在Rust中构建完整的类Unix内核。通过与ostd框架的深度集成和VFS组件的协同工作，它实现了高性能、安全、可维护的操作系统内核服务。