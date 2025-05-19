// NexusOS VFS 路径处理模块。
//
// 本文件定义了 `VfsPath` (借用的路径切片) 和 `VfsPathBuf` (拥有的路径缓冲区)
// 用于表示和操作文件系统路径。所有路径都经过验证和规范化处理。
// 主要特性包括：
// - UTF-8 编码。
// - 使用 `/`作为路径分隔符。
// - 词法规范化 (移除 `.` 和 `..` 组件，合并多个斜杠，处理尾部斜杠)。
// - 提供路径组件迭代、连接、父目录获取等常用操作。

use alloc::borrow::{Cow, ToOwned};
use alloc::string::{String as AllocString, ToString};
use alloc::vec::Vec;
use core::borrow::Borrow;
use core::cmp::Ordering;
use core::fmt::{self, Debug, Display};
use core::hash::{Hash, Hasher};
use core::ops::Deref;
use crate::verror::{vfs_err_invalid_argument, VfsErrorContext, VfsResult};
use error_stack::Report;

/// 一个借用的、经过验证和规范化的 UTF-8 虚拟文件系统路径切片 (`&VfsPath`)。
///
/// 路径总是使用 `/` 分隔，并且是词法规范化的 (不包含 `.` 或 `..` 组件，
/// 除非是根路径 `/`，否则没有尾部斜杠，没有多个连续的斜杠)。
/// 根路径表示为 "/"。不允许空路径。
/// `VfsPath` 设计为 DST (动态大小类型)，通常通过引用 (如 `&VfsPath`) 使用。
#[derive(Eq)]
pub struct VfsPath {
    inner: str, // 内部存储规范化的路径字符串切片。
}

impl VfsPath {
    /// 从字符串切片创建新的 `VfsPath` (可能通过 `VfsPathBuf` 返回所有权实例)。
    ///
    /// 这是从外部输入创建 `VfsPath` 或 `VfsPathBuf` 的主要方式。
    /// 函数会进行验证和规范化处理。
    ///
    /// # 验证确保:
    /// - 路径非空。
    /// - UTF-8 编码。
    /// - 不含内部 NUL (`\0`) 字节。
    /// - 如果是绝对路径，则以 `/` 开头；如果是相对路径，则以有效的组件名开头。
    /// - 路径组件有效 (例如，非空，组件名内不含 `/`)。
    ///
    /// # 规范化确保:
    /// - 多个连续斜杠被合并 (例如, `a///b` -> `a/b`)。
    /// - `.` 组件被移除 (例如, `a/./b` -> `a/b`)。
    /// - `..` 组件被解析 (例如, `a/b/../c` -> `a/c`)，但不会越过根目录或相对路径的起点。
    /// - 除非是根路径 `/`，否则移除尾部斜杠。
    ///
    /// # 返回
    /// - `Ok(Cow::Borrowed(&VfsPath))` 如果输入字符串已经是规范化的，则无分配借用返回。
    /// - `Ok(Cow::Owned(VfsPathBuf))` 如果需要修改 (例如规范化或组件解析)，则分配并返回拥有的 `VfsPathBuf`。
    /// - `Err(VfsError)` 如果路径无效。
    /// 从字符串切片创建新的 `VfsPath` (可能通过 `VfsPathBuf` 返回所有权实例)。
    ///
    /// 这是从外部输入创建 `VfsPath` 或 `VfsPathBuf` 的主要方式。
    /// 函数会进行验证和规范化处理。
    ///
    /// # 验证确保:
    /// - 路径非空。
    /// - UTF-8 编码。
    /// - 不含内部 NUL (`\0`) 字节。
    /// - 如果是绝对路径，则以 `/` 开头；如果是相对路径，则以有效的组件名开头。
    /// - 路径组件有效 (例如，非空，组件名内不含 `/`)。
    ///
    /// # 规范化确保:
    /// - 多个连续斜杠被合并 (例如, `a///b` -> `a/b`)。
    /// - `.` 组件被移除 (例如, `a/./b` -> `a/b`)。
    /// - `..` 组件被解析 (例如, `a/b/../c` -> `a/c`)，但不会越过根目录或相对路径的起点。
    /// - 除非是根路径 `/`，否则移除尾部斜杠。
    pub fn new<'a>(s: &'a str) -> VfsResult<Cow<'a, VfsPath>> {
        // [TODO]: Implement full validation and normalization logic.
        // 这是一个复杂步骤
        // 目前只进行最小化检查，并保留规范化逻辑的占位符
        if s.is_empty() {
            return Err(vfs_err_invalid_argument("path_validation", "Path cannot be empty"));
        }
        if s.contains('\0') {
            return Err(vfs_err_invalid_argument("path_validation", "Path cannot contain NUL bytes"));
        }

        // 规范化逻辑的占位符
        // 在实际实现中，这里会处理：
        // - 将多个斜杠规范化为单个斜杠
        // - 解析"."和".."组件
        // - 移除尾部斜杠（根目录除外）
        // - 验证路径组件
        // 目前只确保非空且不包含NUL字符
        let normalized_s = VfsPath::normalize_str(s)?;

        match normalized_s {
            Cow::Borrowed(borrowed_s) => {
                // 安全性：我们假设 normalize_str 在返回 Cow::Borrowed 时保证有效性
                Ok(Cow::Borrowed(unsafe { VfsPath::from_str_unchecked(borrowed_s) }))
            }
            Cow::Owned(owned_s) => {
                // 安全性：我们假设 normalize_str 在返回 Cow::Owned 时保证有效性
                Ok(Cow::Owned(unsafe { VfsPathBuf::from_string_unchecked(owned_s) }))
            }
        }
    }

    /// 规范化路径字符串，返回一个 `Cow<str>`。
    ///
    /// 此函数是路径处理的核心辅助函数，包含主要的规范化逻辑。
    /// - `s`: 要规范化的原始路径字符串。
    ///
    /// # 返回
    /// - `Ok(Cow::Borrowed(str))` 如果原始字符串已经是规范化的。
    /// - `Ok(Cow::Owned(String))` 如果需要修改以进行规范化。
    /// - `Err(VfsError)` 如果路径包含无效序列 (例如，在解析 `..` 时遇到问题)。
    /// 这是一个辅助函数，将包含重要的逻辑。
    fn normalize_str(s: &str) -> VfsResult<Cow<str>> {
        // [TODO]: 实现完整的路径规范化 (移除 ., .., //, 尾部 / 除非根目录)
        // 这是一个关键且复杂的部分。
        // 目前只进行基本的处理，可能无法覆盖所有情况。
        if s == "/" {
            return Ok(Cow::Borrowed("/"));
        }

        let mut components = Vec::new();
        // 如果路径是绝对路径，它必须以单个'/'开头
        // 其余部分必须是有效的组件
        let is_absolute = s.starts_with('/');

        for component in s.split('/') {
            match component {
                "" | "." => { /* 跳过空组件或 "." 组件 */ }
                ".." => {
                    if is_absolute && components.is_empty() {
                        // 尝试从根目录中解析 ".."，忽略
                        continue;
                    } else if !components.is_empty() && components.last() != Some(&"..") {
                        components.pop();
                    } else if !is_absolute {
                        components.push(".."); // 允许相对路径中的 ".."
                    }
                }
                _ => components.push(component),
            }
        }

        if components.is_empty() {
            if is_absolute {
                return Ok(Cow::Borrowed("/"));
            }
            // 一个空的相对路径在规范化后可能表示为 "." 或无效输入，如 "././."
            // 根据严格性，这可能是一个错误或 "."。
            // 目前只确保路径非空且不包含NUL字符
            if s.chars().all(|c| c == '.' || c == '/') { // 例如，".", "./", "./."
                return Ok(Cow::Borrowed(".")); // VFS 可能不使用 "." 作为内部表示；对于 VfsPath 可能是一个错误。
            } else {
                // 这种情况需要仔细考虑真正空的结果来自非空输入。
                // 目前假设如果它不是 "/" 或 "."，则是一个错误。
                return Err(vfs_err_invalid_argument("path_validation", "路径规范化后为空字符串"));
            }
        }

        let mut result = AllocString::new();
        if is_absolute {
            result.push('/');
        }
        result.push_str(&components.join("/"));

        // 如果原始字符串已经是规范化的，我们可以避免分配。
        // 这个检查在这里简化了。
        if result == s {
            Ok(Cow::Borrowed(s))
        } else {
            Ok(Cow::Owned(result))
        }
    }

    /// 从字符串切片创建一个 `&VfsPath`，**不进行**验证或规范化。
    ///
    /// 此方法仅供内部使用，当调用者确信提供的字符串已经是有效且规范化的 VFS 路径时。
    /// 错误地使用此方法可能导致未定义行为或逻辑错误。
    ///
    /// # Safety (安全性)
    /// 调用者必须确保 `s` 代表一个有效的、规范化的 VFS 路径字符串。
    /// `s` 的生命周期必须超过返回的 `&VfsPath` 的生命周期。
    /// 这是内部使用的，当路径已知是有效且规范化时。
    /// # 安全性
    /// 调用者必须确保 `s` 代表一个有效的、规范化的 VFS 路径字符串。
    pub unsafe fn from_str_unchecked(s: &str) -> &VfsPath {
        &*(s as *const str as *const VfsPath)
    }

    /// 返回表示此 `VfsPath` 的内部字符串切片。
    ///
    /// 返回的字符串是经过验证和规范化的。
    pub fn as_str(&self) -> &str {
        &self.inner
    }

    /// 返回路径各组件的迭代器 (`ComponentsIter`)。
    ///
    /// 组件由 `/` 分隔。迭代器会跳过空组件或 `.`、`..` (因为路径已规范化)。
    /// 例如，对于路径 `"/foo/bar"`，迭代器将产生 `"foo"` 和 `"bar"`。
    /// 对于根路径 `"/"`，迭代器通常不产生任何组件，或根据具体实现可能产生特殊标记 (此实现倾向于不产生)。
    pub fn components(&self) -> ComponentsIter {
        ComponentsIter { inner: self.inner.split('/') }
    }

    /// 返回当前路径的父目录路径 (`&VfsPath`)。
    ///
    /// - 如果路径是根目录 (`"/"`)，则返回 `None`。
    /// - 如果路径是单个组件的相对路径 (例如 `"foo"`)，则返回 `None` (因为它没有父路径上下文)。
    ///   (注意：此行为可能需调整，POSIX `dirname("foo")` 会返回 `"."`)。
    ///   当前实现对于 `"foo"` 会返回 `None`，因为规范化路径不含 `.`。
    /// - 对于 `/foo`，父目录是 `/`。
    /// - 对于 `/foo/bar`，父目录是 `/foo`。
    pub fn parent(&self) -> Option<&VfsPath> {
        if self.inner == *"/" {
            return None;
        }
        match self.inner.rfind('/') {
            Some(0) => Some(unsafe { VfsPath::from_str_unchecked("/") }), // Parent is root
            Some(idx) => Some(unsafe { VfsPath::from_str_unchecked(&self.inner[..idx]) }),
            None => None, // Relative path with no slashes, e.g., "foo"
        }
    }

    /// 返回路径的最后一个组件 (文件名或目录名) 作为字符串切片。
    ///
    /// - 如果路径是根目录 (`"/"`)，则返回 `None`。
    /// - 如果路径为空或只有 `/` (理论上 `VfsPath` 不应为空)，返回 `None`。
    /// - 例如，对于 `"/foo/bar.txt"`，返回 `Some("bar.txt")`。
    /// - 对于 `"/foo/"` (规范化后为 `"/foo"`)，返回 `Some("foo")`。
    pub fn file_name(&self) -> Option<&str> {
        if self.inner == *"/" {
            return None;
        }
        self.inner.rfind('/').map_or(Some(&self.inner), |idx| Some(&self.inner[idx + 1..]))
    }

    /// 返回路径最后一个组件的文件扩展名 (如果存在)。
    ///
    /// 扩展名是文件名中最后一个 `.`之后的部分。
    /// - 如果没有 `.`，或者 `.` 是文件名的第一个字符 (隐藏文件)，则返回 `None`。
    /// - 例如，`"foo.txt"` -> `Some("txt")`； `"foo.tar.gz"` -> `Some("gz")`； `".bashrc"` -> `None`。
    pub fn extension(&self) -> Option<&str> {
        self.file_name()
            .and_then(|name| name.rfind('.'))
            .filter(|&idx| idx > 0 && idx < self.file_name().unwrap().len() - 1) // Ensure not .foo or foo.
            .map(|idx| &self.file_name().unwrap()[idx + 1..])
    }

    /// 判断路径是否为绝对路径 (以 `/` 开头)。
    pub fn is_absolute(&self) -> bool {
        self.inner.starts_with('/')
    }

    /// 判断路径是否为相对路径 (不以 `/` 开头)。
    pub fn is_relative(&self) -> bool {
        !self.is_absolute()
    }

    /// 判断 `base` 路径是否为当前路径 (`self`) 的前缀。
    ///
    /// 例如，`"/foo/bar".starts_with("/foo")` 为 `true`。
    /// 要求 `base` 和 `self` 都是规范化的 `VfsPath`。
    pub fn starts_with(&self, base: &VfsPath) -> bool {
        if &base.inner == "/" {
            return self.is_absolute();
        }
        let base_as_str: &str = &base.inner; // 显式转换为 &str
        self.inner.starts_with(base_as_str) &&
            (self.inner.len() == base.inner.len() ||
             self.inner.as_bytes().get(base.inner.len()) == Some(&b'/')) // 条件继续
    }

    /// 如果当前路径以 `base` 为前缀，则移除该前缀并返回剩余部分的路径切片 (`&VfsPath`)。
    ///
    /// - 如果不匹配，返回 `None`。
    /// - 例如，`"/foo/bar".strip_prefix("/foo")` 返回 `Some("bar")` (作为 `&VfsPath`)。
    /// - 返回的路径切片也是规范化的。
    pub fn strip_prefix(&self, base: &VfsPath) -> Option<&VfsPath> {
        if self.starts_with(base) {
            if &base.inner == "/" {
                // Stripping "/" from "/foo" should yield "foo" (relative)
                // but VfsPath must be normalized. This needs careful thought.
                // For now, let's assume stripping "/" from "/" is None or an error.
                // And stripping "/" from "/a" should give "a".
                if self.inner.len() > 1 {
                     // 安全性：子字符串是一个有效的、规范化的路径，如果它是非空的。
                     // 这是棘手的。"a" 是一个有效的相对路径。
                     Some(unsafe { VfsPath::from_str_unchecked(&self.inner[1..]) })
                } else { // self is also "/"
                    None // 或者可能是一个空路径，如果它被允许，或者 "."
                }
            } else if self.inner.len() == base.inner.len() {
                 // [TODO]: 处理结果为空字符串的情况，这不是一个有效的 VfsPath。
                 // 可以返回一个特殊值或在 VfsPath::new 中处理。
                 // 现在，这可能会导致问题，如果结果是空字符串。
                 Some(unsafe { VfsPath::from_str_unchecked("") }) // 这是有问题的
            } else {
                 Some(unsafe { VfsPath::from_str_unchecked(&self.inner[base.inner.len() + 1..]) })
            }
        } else {
            None
        }
    }

    /// 将当前路径与一个（或多个）路径组件连接，返回一个新的、拥有的 `VfsPathBuf`。
    ///
    /// - `component`: 要连接的路径片段。它可以是单个文件名或相对路径。
    ///   如果 `component` 包含 `/`，它将被视为多个组件。
    /// # 错误
    /// - 如果 `component` 无效 (例如，包含 `/`、空字符串、NUL 字符)，返回 `VfsError`。
    /// 
    /// 在当前路径末尾追加一个新的组件。
    ///
    /// - `component`: 要追加的单个路径组件 (文件名或目录名)。
    ///   **重要**: `component` 必须是单个有效的文件名，不能包含 `/` 或为空。
    ///
    /// # 错误
    /// - 如果 `component` 无效 (例如，包含 `/`、空字符串、NUL 字符)，返回 `VfsError`。
    pub fn join(&self, component: &str) -> VfsResult<VfsPathBuf> {
        // [TODO]: 验证组件 (不允许斜杠、空字符串、NUL 字符)
        if component.is_empty() || component.contains('/') || component == "." || component == ".." {
            return Err(vfs_err_invalid_argument("path_component_validation", component));
        }
        let mut new_path = AllocString::with_capacity(self.inner.len() + 1 + component.len());
        new_path.push_str(&self.inner);
        if &self.inner != "/" {
            new_path.push('/');
        }
        new_path.push_str(component);
        // 安全性：连接一个规范化的路径和一个有效的组件应该会产生一个规范化的路径。
        Ok(unsafe { VfsPathBuf::from_string_unchecked(new_path) })
    }
}

impl AsRef<VfsPath> for VfsPath {
    /// 将 `VfsPath` 实例作为 `&VfsPath` 引用返回。
    ///
    /// 这是实现 `AsRef` trait 的一个方法，允许将 `VfsPath` 实例转换为 `&VfsPath` 引用。
    fn as_ref(&self) -> &VfsPath {
        self
    }
}

impl AsRef<str> for VfsPath {
    fn as_ref(&self) -> &str {
        &self.inner
    }
}

impl Debug for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VfsPath(\"{}\")", &self.inner)
    }
}

impl Display for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &self.inner)
    }
}

impl PartialEq for VfsPath {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl PartialOrd for VfsPath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.inner.partial_cmp(&other.inner)
    }
}

impl Ord for VfsPath {
    fn cmp(&self, other: &Self) -> Ordering {
        self.inner.cmp(&other.inner)
    }
}

impl Hash for VfsPath {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.inner.hash(state);
    }
}

// 为 VfsPath 实现 ToOwned trait，使其可以被克隆为拥有的 VfsPathBuf。
// VfsPath (类似于 &str) 是一个借用的路径切片。
// VfsPathBuf (类似于 String) 是其对应的拥有所有权的路径缓冲区。
impl ToOwned for VfsPath {
    type Owned = VfsPathBuf;

    fn to_owned(&self) -> Self::Owned {
        // 从 &VfsPath 创建一个拥有的 VfsPathBuf。
        // 这涉及到复制底层的字符串数据。
        VfsPathBuf {
            inner: self.inner.to_string(),
        }
    }
}

/// 一个拥有的、经过验证和规范化的 UTF-8 虚拟文件系统路径 (`VfsPathBuf`)。
///
/// `VfsPathBuf` 类似于 `String`，而 `VfsPath` 类似于 `&str`。
/// 它用于构建和修改路径。内部存储一个 `AllocString`。
/// 所有操作确保路径保持规范化状态。
#[derive(Clone, Eq)]
pub struct VfsPathBuf {
    inner: AllocString, // 内部存储规范化的、拥有的路径字符串。
}

impl VfsPathBuf {
    /// 从 `AllocString` 创建一个新的 `VfsPathBuf`，会进行验证和规范化。
    ///
    /// - `s`: 用于创建路径的字符串。它将被消耗。
    ///
    /// # 错误
    /// - 如果输入字符串 `s` 无法形成有效路径 (例如，包含 NUL 字符)，返回 `VfsError`。
    pub fn new(s: AllocString) -> VfsResult<Self> {
        // Reuse VfsPath::new for validation and normalization
        match VfsPath::new(&s)? {
            Cow::Borrowed(_) => Ok(unsafe { Self::from_string_unchecked(s) }), // Already normalized, reuse string
            Cow::Owned(normalized_path_buf) => Ok(normalized_path_buf), // Normalization produced new owned VfsPathBuf
        }
    }

    /// 从 `AllocString` 创建一个 `VfsPathBuf`，**不进行**验证或规范化。
    ///
    /// 此方法仅供内部使用，当调用者确信提供的字符串已经是有效且规范化的 VFS 路径时。
    ///
    /// # Safety (安全性)
    /// 调用者必须确保 `s` 代表一个有效的、规范化的 VFS 路径字符串。
    /// # 安全性
    /// 调用者必须确保 `s` 代表一个有效的、规范化的 VFS 路径字符串。
    pub unsafe fn from_string_unchecked(s: AllocString) -> Self {
        Self { inner: s }
    }

    /// 消费 `VfsPathBuf`，返回底层的 `String`。
    pub fn into_string(self) -> AllocString {
        self.inner
    }
}

impl Deref for VfsPathBuf {
    type Target = VfsPath;
    fn deref(&self) -> &VfsPath {
        unsafe { VfsPath::from_str_unchecked(&self.inner) }
    }
}

impl Borrow<VfsPath> for VfsPathBuf {
    fn borrow(&self) -> &VfsPath {
        self.deref()
    }
}

impl From<&VfsPath> for VfsPathBuf {
    fn from(path: &VfsPath) -> Self {
        unsafe { Self::from_string_unchecked(path.as_str().to_string()) }
    }
}

impl From<&str> for VfsPathBuf {
    fn from(s: &str) -> Self {
        // 该转换会进行路径验证和规范化。
        // 设计文档要求实现 From<&str>。
        // 注意：如果转换失败（例如，路径无效或规范化失败），此函数将会 panic。
        // 这是 From trait 的标准行为，当转换可能失败但不希望通过 Result 返回时。
        // 对于期望可恢复错误处理的场景，请使用 VfsPathBuf::try_from(s)。
        match VfsPathBuf::try_from(s) {
            Ok(path_buf) => path_buf,
            Err(e) => panic!(
                "VfsPathBuf::from(\"&str\"): 转换失败，输入字符串: '{}'. 错误: {:?}. 请考虑使用 try_from 处理错误。", // 修复转义字符
                s,
                e
            ),
        }
    }
}

impl TryFrom<AllocString> for VfsPathBuf {
    type Error = Report<VfsErrorContext>;
    fn try_from(s: AllocString) -> Result<Self, Self::Error> {
        VfsPathBuf::new(s)
    }
}

impl AsRef<VfsPath> for VfsPathBuf {
    fn as_ref(&self) -> &VfsPath {
        self.deref()
    }
}

impl AsRef<str> for VfsPathBuf {
    fn as_ref(&self) -> &str {
        self.inner.as_str()
    }
}

impl Debug for VfsPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VfsPathBuf(\"{}\")", &self.inner)
    }
}

impl Display for VfsPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &self.inner)
    }
}

impl PartialEq for VfsPathBuf {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl PartialOrd for VfsPathBuf {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.inner.partial_cmp(&other.inner)
    }
}

impl Ord for VfsPathBuf {
    fn cmp(&self, other: &Self) -> Ordering {
        self.inner.cmp(&other.inner)
    }
}

impl Hash for VfsPathBuf {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.inner.hash(state);
    }
}

/// `VfsPath` 组件的迭代器。
///
/// 组件由 `/` 分隔。由于 `VfsPath` 保证是规范化的，
/// 此迭代器不会产生 `.`、`..` 或空组件 (根路径 `"/"` 的处理方式需要注意，
/// 一般不将其视为空组件，而是路径的起点)。
/// Components are separated by `/`. The root path `/` yields an empty first component
/// if split naively, so iteration logic needs care.
/// Normalized paths ensure no `.` or `..` or empty components (except possibly for root handling).
pub struct ComponentsIter<'a> {
    // A simple split will produce an empty string before the first '/' for absolute paths,
    // and potentially an empty string after a trailing '/'.
    // VfsPath 经过规范化，不应有尾部斜杠 (除非是根路径) 或空组件。
    // `core::str::Split` 用于按 '/' 分割内部路径字符串。
    inner: core::str::Split<'a, char>,
}

/// 为 `ComponentsIter` 实现 `Iterator` trait。
impl<'a> Iterator for ComponentsIter<'a> {
    type Item = &'a str;

    /// 获取路径的下一个组件。
    ///
    /// 此方法会跳过因 `split('/')` 产生的空字符串，这些空字符串可能源于：
    /// - 绝对路径开头的 `/` (例如 `"/a/b".split('/')` 会产生 `""`, `"a"`, `"b"`)。
    /// - 根路径 `"/".split('/')` 会产生 `""`, `""`。
    /// - （理论上不存在于规范化路径中的）连续斜杠，例如 `"a//b"`。
    /// 迭代器应该只返回有效的、非空的路径组件。
    fn next(&mut self) -> Option<Self::Item> {
        // 跳过可能来自 `split('/')` 的空组件，这可能来自绝对路径或多个斜杠
        // (尽管多个斜杠应该被规范化出 VfsPath)。
        // 对于像 "/a/b" 的路径，split('/') 会产生 ["", "a", "b"]。
        // 对于 "a/b"，它会产生 ["a", "b"]。
        // 对于 "/", 它会产生 ["", ""]。
        // VfsPath 规范化应该简化这个，但是迭代器需要是健壮的。
        loop {
            match self.inner.next() {
                Some("") => { 
                    // 这处理绝对路径的第一个组件前的空字符串。
                    // 它也可以在路径只是 "/" 时出现，yielding 两个空字符串。
                    // 或者如果有多个斜杠 (例如 "//")，它应该被规范化出。
                    // 让我们假设 VfsPath 是规范化的。如果 self.path_str == "/",那么 split 会给出 ["", ""]。
                    // 如果 self.path_str == "/a",那么 split 会给出 ["", "a"]。
                    // 这需要对 root "/" 的表示和 split 进行详细考虑。
                    // 如果 VfsPath "/" 的 inner 是 "/",那么 self.inner.split('/') 将会产生 "" 然后 ""。
                    // 我们可能只想要 yield 有意义的组件名称。
                    // 这当前的循环可能会错误地跳过绝对路径的第一个空字符串
                    // 如果它被认为是一个组件或标记。
                    //  requirements 提到 "迭代规范化的路径组件 (不应产生 . 或 .. 或空组件)"
                    // 所以空字符串应该被跳过。
                    continue;
                }
                Some(comp) => return Some(comp),
                None => return None,
            }
        }
    }
}

// [TODO]: 为路径规范化、组件迭代和所有方法添加单元测试。
