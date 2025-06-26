// VFS 核心类型定义模块
//
// 本文件包含 NexusOS 虚拟文件系统 (VFS) 使用的各种核心数据结构和类型别名。
// 这些类型用于表示文件系统对象、元数据、操作标志等。

use alloc::collections::BTreeMap;
use alloc::fmt;
use alloc::string::String;
use nexus_error::error_stack::{Context, Result, ResultExt};
use ostd::timer::Jiffies as SystemTime;
use bitflags::bitflags;

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

bitflags! {
    /// POSIX 文件模式 (mode_t, underlying u16)
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
    pub struct FileMode: u16 {
        /* ---------- special bits ---------- */
        const SETUID  = 0o4000; // 执行时临时以文件所有者 UID 运行
        const SETGID  = 0o2000; // 以文件所属 GID 运行，若目录则继承组
        const STICKY  = 0o1000; // 仅所有者/根可删除目录下文件

        /* ---------- Owner ---------- */
        const OWNER_READ    = 0o0400;
        const OWNER_WRITE   = 0o0200;
        const OWNER_EXECUTE = 0o0100;

        /* ---------- Group ---------- */
        const GROUP_READ    = 0o0040;
        const GROUP_WRITE   = 0o0020;
        const GROUP_EXECUTE = 0o0010;

        /* ---------- Others ---------- */
        const OTHER_READ    = 0o0004;
        const OTHER_WRITE   = 0o0002;
        const OTHER_EXECUTE = 0o0001;

        /* ---------- helper masks ---------- */
        const OWNER_RWE = Self::OWNER_READ.bits() | Self::OWNER_WRITE.bits() | Self::OWNER_EXECUTE.bits();
        const GROUP_RWE = Self::GROUP_READ.bits() | Self::GROUP_WRITE.bits() | Self::GROUP_EXECUTE.bits();
        const OTHER_RWE = Self::OTHER_READ.bits() | Self::OTHER_WRITE.bits() | Self::OTHER_EXECUTE.bits();

        const ALL_PERMISSIONS = Self::OWNER_RWE.bits() | Self::GROUP_RWE.bits() | Self::OTHER_RWE.bits();
    }
}

/// 仅三种互斥访问模式，底层数值对齐 open(2) 最低两位
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessMode {
    ReadOnly  = 0, // O_RDONLY
    WriteOnly = 1, // O_WRONLY
    ReadWrite = 2, // O_RDWR
}

impl AccessMode {
    #[inline] pub const fn is_readable(self) -> bool { matches!(self, Self::ReadOnly | Self::ReadWrite) }
    #[inline] pub const fn is_writable(self) -> bool { matches!(self, Self::WriteOnly | Self::ReadWrite) }
}

bitflags! {
    /// Linux/POSIX open(2) 状态位（不含最低两位的访问模式）
    #[derive(Debug, Copy, Clone, Default)]
    pub struct OpenStatus: u32 {
        /* ---------- create / existence ---------- */
        const CREATE   = 0o0100;      // O_CREAT  : 若不存在则创建
        const EXCL     = 0o0200;      // O_EXCL   : CREATE+EXCL => 排他创建
        const TRUNC    = 0o1000;      // O_TRUNC  : 打开时截断文件
        const TMPFILE  = 0o20000000;  // O_TMPFILE: 匿名临时文件（Linux 特有）

        /* ---------- status ---------- */
        const APPEND   = 0o2000;      // 每次写前自动 seek EOF
        const NONBLOCK = 0o4000;      // 非阻塞 I/O
        const DSYNC    = 0o10000;     // 仅同步数据
        const ASYNC    = 0o20000;     // SIGIO 异步通知
        const DIRECT   = 0o40000;     // 直接 I/O，绕过页缓存
        const SYNC     = 0o4010000;   // 同步数据+元数据（含 O_FSYNC）

        /* ---------- path & atime ---------- */
        const DIRECTORY = 0o200000;   // 要求目标为目录
        const NOFOLLOW  = 0o400000;   // 最后一段符号链接不跟随
        const LARGEFILE = 0o100000;   // 允许 >2 GiB (古早 32bit)
        const NOATIME   = 0o1000000;  // 不更新 atime
        const CLOEXEC   = 0o2000000;  // execve 时自动关闭
        const PATH      = 0o10000000; // 仅获取路径句柄 (Linux)
    }
}

/// 文件打开参数，解析并保存原始 flags
#[derive(Clone, Copy, Debug)]
pub struct FileOpen {
    raw: u32,
    access: AccessMode,
    status: OpenStatus,
}

/// 非法访问模式错误
#[derive(Debug, Clone, Copy)]
pub struct InvalidFlags;

impl fmt::Display for InvalidFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InvalidFlags")
    }
}

impl Context for InvalidFlags {}

impl FileOpen {
    /// 从用户态 flags 构造，若访问模式非法则返回 `Err`
    pub fn new(raw_flags: u32) -> Result<Self, InvalidFlags> {
        let access_val = raw_flags & 0b11; // 低两位
        let access = match access_val {
            0 => AccessMode::ReadOnly,
            1 => AccessMode::WriteOnly,
            2 => AccessMode::ReadWrite,
            _ => return Err(InvalidFlags.into()), // 理论上 3 保留
        };

        let status_bits = raw_flags & !0b11;
        let status = OpenStatus::from_bits_truncate(status_bits);

        Ok(Self { raw: raw_flags, access, status })
    }

    #[inline] pub const fn bits(&self) -> u32 { self.raw }
    #[inline] pub const fn access(&self) -> AccessMode { self.access }
    #[inline] pub const fn status(&self) -> OpenStatus { self.status }

    /* ---------- 常用便捷判断 ---------- */
    #[inline] pub const fn is_readable(&self) -> bool { self.access.is_readable() }
    #[inline] pub const fn is_writable(&self) -> bool { self.access.is_writable() }
    #[inline] pub const fn should_create(&self) -> bool { self.status.contains(OpenStatus::CREATE) }
    #[inline] pub const fn should_truncate(&self) -> bool { self.status.contains(OpenStatus::TRUNC) }
    #[inline] pub const fn is_append(&self) -> bool { self.status.contains(OpenStatus::APPEND) }
    #[inline] pub const fn is_exclusive(&self) -> bool { self.status.contains(OpenStatus::EXCL) }
    #[inline] pub const fn is_cloexec(&self) -> bool { self.status.contains(OpenStatus::CLOEXEC) }
    #[inline] pub const fn is_directory(&self) -> bool { self.status.contains(OpenStatus::DIRECTORY) }

    pub fn cloexec(mut self) -> Self {
        self.status.insert(OpenStatus::CLOEXEC);
        self
    }
}

/// 构建 FileOpen 的链式 Builder
#[derive(Clone, Copy, Debug, Default)]
pub struct FileOpenBuilder {
    access: Option<AccessMode>,  // 必须显式设置
    status: OpenStatus,          // 默认为空
}

#[derive(Debug)]
pub enum BuildError {
    MissingAccess,               // 未指定读/写模式
    InvalidFlags(InvalidFlags),  // 理论上永不触发
}

impl Context for BuildError {}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingAccess => write!(f, "MissingAccess"),
            Self::InvalidFlags(e) => write!(f, "InvalidFlags: {}", e),
        }
    }
}

impl FileOpenBuilder {
    /// 入口：FileOpen::options() 会调用它
    pub const fn new() -> Self {
        Self { access: None, status: OpenStatus::empty() }
    }

    /* ---------- 访问模式（必选其一） ---------- */
    pub const fn read_only(mut self) -> Self {
        self.access = Some(AccessMode::ReadOnly); self
    }
    pub const fn write_only(mut self) -> Self {
        self.access = Some(AccessMode::WriteOnly); self
    }
    pub const fn read_write(mut self) -> Self {
        self.access = Some(AccessMode::ReadWrite); self
    }

    /* ---------- 状态位（可选连调） ---------- */
    pub fn create(mut self) -> Self       { self.status |= OpenStatus::CREATE;   self }
    pub fn excl(mut self) -> Self         { self.status |= OpenStatus::EXCL;     self }
    pub fn truncate(mut self) -> Self     { self.status |= OpenStatus::TRUNC;    self }
    pub fn append(mut self) -> Self       { self.status |= OpenStatus::APPEND;   self }
    pub fn cloexec(mut self) -> Self      { self.status |= OpenStatus::CLOEXEC;  self }
    pub fn nofollow(mut self) -> Self     { self.status |= OpenStatus::NOFOLLOW; self }
    // ……可按需继续补充 setuid 等

    /// 最终构造 FileOpen；编译期确保必填字段，运行期再次验证位组合
    pub fn build(self) -> Result<FileOpen, BuildError> {
        let access = self.access.ok_or(BuildError::MissingAccess)?;
        // 低两位来自访问模式，其他位来自 status
        let raw = (access as u32) | self.status.bits();
        FileOpen::new(raw).change_context_lazy(|| BuildError::InvalidFlags(InvalidFlags))
    }
}

/* ---------- FileOpen 对应便捷构造入口 ---------- */
impl FileOpen {
    /// 开启链式构造
    pub const fn options() -> FileOpenBuilder { FileOpenBuilder::new() }
}

/* ---------- ktest ---------- */
#[cfg(ktest)]
mod tests {
    use super::*;
    use ostd::prelude::ktest;

    #[ktest]
    fn parse_and_check() {
        /* O_RDWR | O_CREAT | O_TRUNC | O_CLOEXEC */
        let raw = 0b10 | OpenStatus::CREATE.bits() | OpenStatus::TRUNC.bits() | OpenStatus::CLOEXEC.bits();
        let fo = FileOpen::new(raw).unwrap();
        assert_eq!(fo.access(), AccessMode::ReadWrite);
        assert!(fo.should_create() && fo.should_truncate() && fo.is_cloexec());
        assert!(fo.is_readable() && fo.is_writable());
    }

    #[ktest]
    fn builder_roundtrip() {
        let fo = FileOpen::options()
            .read_write()
            .create()
            .truncate()
            .cloexec()
            .build()
            .unwrap();

        assert_eq!(fo.access(), AccessMode::ReadWrite);
        assert!(fo.should_create() && fo.should_truncate() && fo.is_cloexec());
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
#[allow(unused)]
pub struct FsOptionsBuilder {
    options: BTreeMap<AllocOsString, AllocOsString>, // 存储选项的 BTreeMap
    read_only: Option<bool>,                     // 只读标志，None 表示使用默认值 (false)
}

#[allow(unused)]
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
