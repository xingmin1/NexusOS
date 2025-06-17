// VFS 核心类型定义模块
//
// 本文件包含 NexusOS 虚拟文件系统 (VFS) 使用的各种核心数据结构和类型别名。
// 这些类型用于表示文件系统对象、元数据、操作标志等。

use alloc::collections::BTreeMap;
use alloc::string::String;
use ostd::timer::Jiffies as SystemTime;

/// Vnode (虚拟节点) 的类型枚举。
///
/// 代表文件系统中的一个条目可以是哪种类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VnodeType {
    File,          // 普通文件
    Directory,     // 目录
    SymbolicLink,  // 符号链接
    BlockDevice,   // 块设备文件
    CharDevice,    // 字符设备文件
    Fifo,          // 命名管道 (FIFO)
    Socket,        // 套接字
}

pub type OsStr = str;

pub type OsString = String;

pub type AllocOsString = OsString;

type Id = usize;

/// 文件系统ID的类型别名，通常是一个唯一的标识符。
pub type FilesystemId = Id;

/// 挂载点ID的类型别名。
pub type MountId = Id;

/// Vnode ID的类型别名，用于唯一标识一个文件系统内的Vnode。
/// 使用 `u64` 以提供足够的空间，类似于inode号。
pub type VnodeId = u64;

#[derive(Copy, PartialEq, Eq, Clone, Debug)]
pub enum SeekFrom {
    Start(usize), // 从文件开始位置
    End(isize),   // 从文件末尾位置
    Current(isize), // 从当前文件位置
}

/// 文件或目录的时间戳信息。
///
/// 包含访问时间 (atime)、修改时间 (mtime)、创建时间 (birthtime/crtime)
/// 和元数据更改时间 (ctime)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamps {
    pub accessed: SystemTime, // 最后访问时间
    pub modified: SystemTime, // 最后内容修改时间
    pub created: SystemTime,  // 创建时间
    pub changed: SystemTime,  // 最后元数据更改时间 (POSIX ctime)
}

impl Timestamps {
    /// 创建一个所有时间戳都设置为当前时间的 `Timestamps` 实例。
    pub fn now() -> Self {
        let t = SystemTime::elapsed();
        Self { accessed: t, modified: t, created: t, changed: t }
    }
}

/// Vnode 的元数据。
///
/// 包含了关于文件系统对象（如文件、目录）的详细信息。
#[derive(Debug, Clone)]
pub struct VnodeMetadata {
    pub vnode_id: VnodeId,       // 此Vnode的唯一ID
    pub fs_id: FilesystemId,     // 此Vnode所属的文件系统的ID
    pub kind: VnodeType,         // Vnode的类型 (文件、目录等)
    pub size: u64,               // Vnode的大小 (字节，对于文件和符号链接)
    pub permissions: FileMode,   // 权限位 (例如，类似POSIX的 mode_t 的低位)
    pub timestamps: Timestamps,    // 时间戳信息 (访问、修改、创建、更改)
    pub uid: u32,                // 用户ID (所有者)
    pub gid: u32,                // 组ID (所有者)
    pub nlinks: u64,             // 硬链接数量
    pub rdev: Option<u64>,       // 设备号 (如果Vnode是块设备或字符设备)
}

/// 用于指定要更新的时间戳。
///
/// 并非所有时间戳都可以由用户直接设置（例如ctime）。
/// `None` 表示不更改对应的时间戳。
#[derive(Debug, Clone, Default)]
pub struct TimestampsToSet {
    pub accessed: Option<SystemTime>, // 新的访问时间，或 `None` 表示不更改
    pub modified: Option<SystemTime>, // 新的修改时间，或 `None` 表示不更改
    // ctime 由系统自动更新，不由用户直接设置
}

/// 用于指定要修改的 Vnode 元数据字段。
///
/// `None` 值表示对应的字段不应被修改。
#[derive(Debug, Clone, Default)]
pub struct VnodeMetadataChanges {
    pub size: Option<u64>,                    // 新的文件大小 (用于 truncate)
    pub permissions: Option<FileMode>,             // 新的权限位
    pub timestamps: Option<TimestampsToSet>,  // 要更新的时间戳 (通常是访问和修改时间)
    pub uid: Option<u32>,                     // 新的用户ID
    pub gid: Option<u32>,                     // 新的组ID
    // 注意：
    // - `nlinks` (硬链接数) 通常由 `link` 和 `unlink` 操作管理。
    // - `rdev` (设备号) 通常在创建设备文件时设定，之后不常更改。
    // - `kind` (类型) 和 `fs_id` (文件系统ID) 在Vnode创建后通常是不可变的。
    // - `changed` (ctime) 时间戳会在元数据或内容发生更改时由系统自动更新。
}


use bitflags::bitflags;

bitflags! {
    /// POSIX 文件模式标志 (mode_t)，底层 u16
    /// 包含 setuid/setgid/sticky 以及 owner/group/other 的 rwx 位
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct FileMode: u16 {
        /// Set-user-ID 位 (S_ISUID)
        const SETUID        = 0o4000;
        /// Set-group-ID 位 (S_ISGID)
        const SETGID        = 0o2000;
        /// Sticky 位 (S_ISVTX)
        const STICKY        = 0o1000;

        /// Owner 可读 (S_IRUSR)
        const OWNER_READ    = 0o0400;
        /// Owner 可写 (S_IWUSR)
        const OWNER_WRITE   = 0o0200;
        /// Owner 可执行 (S_IXUSR)
        const OWNER_EXECUTE = 0o0100;

        /// Group 可读 (S_IRGRP)
        const GROUP_READ    = 0o0040;
        /// Group 可写 (S_IWGRP)
        const GROUP_WRITE   = 0o0020;
        /// Group 可执行 (S_IXGRP)
        const GROUP_EXECUTE = 0o0010;

        /// Others 可读 (S_IROTH)
        const OTHER_READ    = 0o0004;
        /// Others 可写 (S_IWOTH)
        const OTHER_WRITE   = 0o0002;
        /// Others 可执行 (S_IXOTH)
        const OTHER_EXECUTE = 0o0001;

        /// 所有权限
        const ALL_PERMISSIONS = Self::OWNER_READ.bits() | Self::OWNER_WRITE.bits() | Self::OWNER_EXECUTE.bits() |
            Self::GROUP_READ.bits() | Self::GROUP_WRITE.bits() | Self::GROUP_EXECUTE.bits() |
            Self::OTHER_READ.bits() | Self::OTHER_WRITE.bits() | Self::OTHER_EXECUTE.bits();

        /// Owner 读写执行权限
        const OWNER_RWE = Self::OWNER_READ.bits() | Self::OWNER_WRITE.bits() | Self::OWNER_EXECUTE.bits();
        /// Group 读写执行权限
        const GROUP_RWE = Self::GROUP_READ.bits() | Self::GROUP_WRITE.bits() | Self::GROUP_EXECUTE.bits();
        /// Others 读写执行权限
        const OTHER_RWE = Self::OTHER_READ.bits() | Self::OTHER_WRITE.bits() | Self::OTHER_EXECUTE.bits();

        /// Owner 读执行权限
        const OWNER_RE = Self::OWNER_READ.bits() | Self::OWNER_EXECUTE.bits();
        /// Group 读执行权限
        const GROUP_RE = Self::GROUP_READ.bits() | Self::GROUP_EXECUTE.bits();
        /// Others 读执行权限
        const OTHER_RE = Self::OTHER_READ.bits() | Self::OTHER_EXECUTE.bits();

        /// Owner 读写权限
        const OWNER_RW = Self::OWNER_READ.bits() | Self::OWNER_WRITE.bits();
        /// Group 读写权限
        const GROUP_RW = Self::GROUP_READ.bits() | Self::GROUP_WRITE.bits();
        /// Others 读写权限
        const OTHER_RW = Self::OTHER_READ.bits() | Self::OTHER_WRITE.bits();    
    }

    /// POSIX / Linux <fcntl.h> 中定义的 open(2) 标志位
    #[derive(Debug, Clone, Copy)]
    pub struct OpenFlags: u32 {
        // ————— 最低两位：访问模式掩码 —————
        /// 访问模式掩码（最低两位）
        const ACCMODE   = 0o3;
        /// 只读（O_RDONLY）
        const RDONLY    = 0o0;
        /// 只写（O_WRONLY）
        const WRONLY    = 0o1;
        /// 读写（O_RDWR）
        const RDWR      = 0o2;

        // ————— 文件创建标志 —————
        /// 如果不存在则创建（O_CREAT）
        const CREATE    = 0o100;
        /// 文件存在则报错，仅与 O_CREAT 一起生效（O_EXCL）
        const EXCL      = 0o200;
        /// 不成为控制终端（O_NOCTTY）
        const NOCTTY    = 0o400;
        /// 截断文件为零长度（O_TRUNC）
        const TRUNC     = 0o1000;
        /// 创建匿名临时文件，仅供内部使用（O_TMPFILE）
        const TMPFILE   = 0o20000000;

        // ————— 文件状态标志 —————
        /// 追加写入（O_APPEND）
        const APPEND    = 0o2000;
        /// 非阻塞 I/O（O_NONBLOCK，即 O_NDELAY）
        const NONBLOCK  = 0o4000;
        /// 同步写入数据及元数据（O_SYNC）
        const SYNC      = 0o4010000;
        /// 只同步写入数据，不保证元数据（O_DSYNC）
        const DSYNC     = 0o10000;
        /// 异步 I/O 通知（O_ASYNC）
        const ASYNC     = 0o20000;
        /// 直接 I/O，绕过缓存（O_DIRECT）
        const DIRECT    = 0o40000;

        // ————— 其他 Linux/POSIX.1-2008 标志 —————
        /// 必须为目录（O_DIRECTORY）
        const DIRECTORY = 0o200000;
        /// 不跟随最后一个符号链接（O_NOFOLLOW）
        const NOFOLLOW  = 0o400000;
        /// 打开时不更新访问时间（O_NOATIME）
        const NOATIME   = 0o1000000;
        /// 执行新程序时自动关闭（O_CLOEXEC）
        const CLOEXEC   = 0o2000000;

        // —————（Linux 特有，但 POSIX.1-2008 可选）—————
        /// 获取文件描述符用于路径操作（O_PATH）
        const PATH      = 0o10000000;
        /// 支持大文件（O_LARGEFILE）
        const LARGEFILE = 0o100000;
    }
}

/// 文件访问模式枚举，仅三种互斥模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

/// 解析用户态传入的 flags 并提供清晰的接口
#[derive(Debug, Clone, Copy)]
pub struct FileOpen {
    /// 原始 flags，便于透传或反向获取
    raw: u32,
    /// 访问模式
    access: AccessMode,
    /// 除访问模式外的其他标志位
    flags: OpenFlags,
}

impl FileOpen {
    /// 从原始 flags 创建实例
    pub fn new(raw_flags: u32) -> Self {
        // 提取访问模式位（最低两位）
        let mode_bits = raw_flags & OpenFlags::ACCMODE.bits();
        let access = match mode_bits {
            b if b == OpenFlags::RDONLY.bits() => AccessMode::ReadOnly,
            b if b == OpenFlags::WRONLY.bits() => AccessMode::WriteOnly,
            b if b == OpenFlags::RDWR.bits()   => AccessMode::ReadWrite,
            _ => AccessMode::ReadOnly, // 可替换为错误处理
        };
        // 安全解析已知标志位，忽略未知位
        let flags = OpenFlags::from_bits_truncate(raw_flags & !OpenFlags::ACCMODE.bits());
        Self { raw: raw_flags, access, flags }
    }

    /// 获取原始 flags
    #[inline]
    pub fn bits(&self) -> u32 {
        self.raw
    }

    /// 获取访问模式
    #[inline]
    pub fn access_mode(&self) -> AccessMode {
        self.access
    }

    /// 是否可读
    #[inline]
    pub fn is_readable(&self) -> bool {
        matches!(self.access, AccessMode::ReadOnly | AccessMode::ReadWrite)
    }

    /// 是否可写
    #[inline]
    pub fn is_writable(&self) -> bool {
        matches!(self.access, AccessMode::WriteOnly | AccessMode::ReadWrite)
    }

    /// 是否包含指定标志
    #[inline]
    pub fn has(&self, flag: OpenFlags) -> bool {
        self.flags.contains(flag)
    }

    /// 是否需要创建文件
    #[inline]
    pub fn should_create(&self) -> bool {
        self.has(OpenFlags::CREATE)
    }

    /// 是否为追加模式
    #[inline]
    pub fn is_append(&self) -> bool {
        self.has(OpenFlags::APPEND)
    }

    /// 是否需要截断文件
    #[inline]
    pub fn should_truncate(&self) -> bool {
        self.has(OpenFlags::TRUNC)
    }

    /// 是否为排他创建
    #[inline]
    pub fn is_exclusive(&self) -> bool {
        self.has(OpenFlags::EXCL)
    }

    /// 是否设置 CLOEXEC
    #[inline]
    pub fn is_cloexec(&self) -> bool {
        self.has(OpenFlags::CLOEXEC)
    }
}

// 单元测试示例（需 ktest 支持）
#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn test_file_open() {
        // 模拟：O_RDWR | O_CREAT | O_TRUNC | O_CLOEXEC
        let raw = OpenFlags::RDWR.bits()
                | OpenFlags::CREATE.bits()
                | OpenFlags::TRUNC.bits()
                | OpenFlags::CLOEXEC.bits();
        let fo = FileOpen::new(raw);
        assert_eq!(fo.access_mode(), AccessMode::ReadWrite);
        assert!(fo.should_create());
        assert!(fo.should_truncate());
        assert!(fo.is_cloexec());
        assert!(fo.is_writable());
        assert!(fo.is_readable());
    }
}

/// 表示目录中一个条目的信息。
///
/// 用于 `read_dir` 等操作返回目录内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    // [TODO]: 未来应考虑支持 OsString 或 Vec<u8> 以便更好地处理非UTF-8编码的文件名。
    // 当前为简化实现，假设VFS内部文件名是UTF-8编码的字符串。
    pub name: AllocOsString,     // 条目名称 (文件名或目录名)
    pub vnode_id: VnodeId,     // 此条目指向的 Vnode 的 ID
    pub kind: VnodeType,       // 此条目指向的 Vnode 的类型
}

/// 文件系统的统计信息。
///
/// 类似于 POSIX `statfs()` 或 `statvfs()` 返回的信息。
#[derive(Debug, Clone)]
pub struct FilesystemStats {
    pub fs_id: FilesystemId,           // 文件系统ID
    pub fs_type_name: AllocOsString,     // 文件系统类型名称 (例如, "memfs", "fat32")
    pub block_size: u64,               // 文件系统块大小 (字节)
    pub total_blocks: u64,             // 文件系统总块数
    pub free_blocks: u64,              // 可供非特权用户使用的空闲块数
    pub avail_blocks: u64,             // 可供特权用户使用的空闲块数 (通常与 free_blocks 相同或稍多)
    pub total_inodes: u64,             // 文件系统总 inode 数 (如果适用)
    pub free_inodes: u64,              // 文件系统空闲 inode 数 (如果适用)
    pub name_max_len: u32,             // 文件名组件的最大长度
    pub optimal_io_size: Option<u64>,  // 最佳 I/O 传输大小 (如果适用)
}

/// 文件系统挂载选项。
///
/// 存储为键值对字符串，并提供一个特定的 `read_only` 标志。
#[derive(Clone, Debug, Default)]
pub struct FsOptions {
    inner: BTreeMap<AllocOsString, AllocOsString>, // 内部存储挂载选项的 BTreeMap
    pub read_only: bool,                       // 是否以只读模式挂载
}

impl FsOptions {
    /// 根据键获取挂载选项的值。
    pub fn get(&self, key: &str) -> Option<&str> {
        self.inner.get(key).map(|s| s.as_str())
    }

    /// 根据键获取布尔类型的挂载选项值。
    /// 如果键不存在或无法解析为布尔值，则返回 `default_val`。
    pub fn get_bool(&self, key: &str, default_val: bool) -> bool {
        self.get(key).and_then(|s| s.parse().ok()).unwrap_or(default_val)
    }
    // 可以根据需要添加更多类型化的获取器，例如 get_u64 等。
}

/// 用于构建 `FsOptions` 实例的构建器模式。
#[derive(Default, Debug)]
pub struct FsOptionsBuilder {
    options: BTreeMap<AllocOsString, AllocOsString>, // 存储选项的 BTreeMap
    read_only: Option<bool>,                     // 只读标志，None 表示使用默认值 (false)
}

impl FsOptionsBuilder {
    /// 创建一个新的 `FsOptionsBuilder` 实例。
    pub fn new() -> Self {
        Default::default()
    }

    /// 添加一个键值对挂载选项。
    pub fn option(mut self, key: impl Into<AllocOsString>, value: impl Into<AllocOsString>) -> Self {
        self.options.insert(key.into(), value.into());
        self
    }

    /// 设置只读挂载标志。
    pub fn read_only(mut self, val: bool) -> Self {
        self.read_only = Some(val);
        self
    }

    /// 构建 `FsOptions` 实例。
    pub fn build(self) -> FsOptions {
        FsOptions {
            inner: self.options,
            read_only: self.read_only.unwrap_or(false),
        }
    }
}
