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
    pub permissions: u16,        // 权限位 (例如，类似POSIX的 mode_t 的低位)
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
    pub permissions: Option<u16>,             // 新的权限位
    pub timestamps: Option<TimestampsToSet>,  // 要更新的时间戳 (通常是访问和修改时间)
    pub uid: Option<u32>,                     // 新的用户ID
    pub gid: Option<u32>,                     // 新的组ID
    // 注意：
    // - `nlinks` (硬链接数) 通常由 `link` 和 `unlink` 操作管理。
    // - `rdev` (设备号) 通常在创建设备文件时设定，之后不常更改。
    // - `kind` (类型) 和 `fs_id` (文件系统ID) 在Vnode创建后通常是不可变的。
    // - `changed` (ctime) 时间戳会在元数据或内容发生更改时由系统自动更新。
}

bitflags::bitflags! {
    /// 定义文件打开标志的位域。
    ///
    /// 这些标志控制 `open` 操作的行为以及返回的文件句柄的属性。
    /// 类似于 POSIX `open()` 的 `flags` 参数。
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct OpenFlags: u32 {
        const READ        = 1 << 0;  // 以只读方式打开
        const WRITE       = 1 << 1;  // 以只写方式打开
        const APPEND      = 1 << 2;  // 以追加方式写入 (意味着 WRITE)
        const CREATE      = 1 << 3;  // 如果文件不存在则创建它
        const EXCLUSIVE   = 1 << 4;  // (O_EXCL) 与 CREATE 一起使用，确保原子创建
        const TRUNCATE    = 1 << 5;  // 如果文件存在并且是普通文件，并且以写方式打开，则将其长度截断为0
        const DIRECTORY   = 1 << 6;  // (O_DIRECTORY) 如果路径不是目录，则打开失败
        const NOFOLLOW    = 1 << 7;  // (O_NOFOLLOW) 如果路径是符号链接，则不解引用它
        const DSYNC       = 1 << 8;  // (O_DSYNC) 要求文件数据同步完成才返回
        const SYNC        = 1 << 9;  // (O_SYNC) 要求文件数据和元数据同步完成才返回
        const DIRECT_IO   = 1 << 10; // (O_DIRECT) 尝试最小化或避免缓存效果 (直接I/O)
        
        // 预定义组合标志
        const CREATE_NEW  = Self::CREATE.bits() | Self::EXCLUSIVE.bits(); // 等同于 O_CREAT | O_EXCL
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
