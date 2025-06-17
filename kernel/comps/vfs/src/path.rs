use alloc::{
    borrow::Cow,
    format,
    string::{String, ToString},
    vec::Vec,
};
use nexus_error::error_stack::Report;
use core::{
    cmp::Ordering,
    fmt::{self, Debug, Display},
    hash::{Hash, Hasher}
};

use crate::{verror::KernelError, vfs_err_invalid_argument, VfsResult};

/// 借用型虚拟路径切片。
#[derive(Copy, Clone)]
pub struct VfsPath<'a>(pub(crate) &'a str);

impl<'a> VfsPath<'a> {
    /* ---------- 构造 ---------- */

    pub fn new(s: &'a str) -> VfsResult<Self> {
        match normalize(s)? {
            Cow::Borrowed(b) => Ok(Self(b)),
            Cow::Owned(_) => unreachable!("Borrowed variant guaranteed if unchanged"),
        }
    }

    /* ---------- 只读查询 ---------- */

    #[inline]
    pub fn as_str(self) -> &'a str {
        self.0
    }
    #[inline]
    pub fn is_absolute(self) -> bool {
        self.0.starts_with('/')
    }
    #[inline]
    pub fn is_root(self) -> bool {
        self.0 == "/"
    }
    #[inline]
    pub fn components(self) -> Components<'a> {
        Components {
            inner: self.0.split('/'),
            skip_empty: true,
        }
    }

    pub fn parent(self) -> Option<Self> {
        if self.is_root() {
            return None;
        }
        self.0.rfind('/').map(|idx| {
            if idx == 0 {
                Self("/")
            } else {
                Self(&self.0[..idx])
            }
        })
    }

    pub fn file_name(self) -> Option<&'a str> {
        if self.is_root() {
            None
        } else {
            Some(self.0.rsplit('/').next().unwrap())
        }
    }

    pub fn starts_with(self, base: Self) -> bool {
        if base.is_root() {
            return self.is_absolute();
        }
        self.0.starts_with(base.0)
            && (self.0.len() == base.0.len() || self.0.as_bytes()[base.0.len()] == b'/')
    }

    pub fn strip_prefix(self, base: Self) -> Option<Self> {
        if !self.starts_with(base) {
            return None;
        }
        if self.0.len() == base.0.len() {
            Some(Self(".")) // 相对空路径用 "." 表示
        } else {
            Some(Self(&self.0[base.0.len() + 1..]))
        }
    }

    /* ---------- 产生拥有缓冲区 ---------- */

    pub fn to_owned_buf(self) -> VfsPathBuf {
        VfsPathBuf(self.0.to_string())
    }

    pub fn join(self, comp: &str) -> VfsResult<VfsPathBuf> {
        if comp.is_empty() || comp.contains('/') {
            return Err(vfs_err_invalid_argument!("bad component"));
        }
        if self.is_root() {
            Ok(VfsPathBuf(format!("/{}", comp)))
        } else {
            Ok(VfsPathBuf(format!("{}/{}", self.0, comp)))
        }
    }
}

impl <'a> From<&'a str> for VfsPath<'a> {
    fn from(s: &'a str) -> Self {
        Self(s)
    }
}

/* ===== 拥有型缓冲区 ===== */

#[derive(Clone, PartialEq, Eq)]
pub struct VfsPathBuf(pub(crate) String);

impl VfsPathBuf {
    pub fn new<S: Into<String>>(s: S) -> VfsResult<Self> {
        match normalize(&s.into())? {
            Cow::Borrowed(b) => Ok(Self(b.to_string())),
            Cow::Owned(o) => Ok(Self(o)),
        }
    }
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'a> From<&'a VfsPathBuf> for VfsPath<'a> {
    fn from(b: &'a VfsPathBuf) -> Self {
        Self(&b.0)
    }
}
impl<'a> TryFrom<&'a str> for VfsPathBuf {
    type Error = Report<KernelError>;
    fn try_from(s: &'a str) -> VfsResult<Self> {
        Self::new(s)
    }
}

/* ===== 迭代器 ===== */

pub struct Components<'a> {
    inner: core::str::Split<'a, char>,
    skip_empty: bool,
}
impl<'a> Iterator for Components<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        for seg in &mut self.inner {
            if seg.is_empty() && self.skip_empty {
                continue;
            }
            return Some(seg);
        }
        None
    }
}

/* ===== Normalize 实现 ===== */

fn normalize(s: &str) -> VfsResult<Cow<'_, str>> {
    if s.is_empty() {
        return Err(vfs_err_invalid_argument!("empty"));
    }
    if s.contains('\0') {
        return Err(vfs_err_invalid_argument!("NUL"));
    }

    if s == "/" {
        return Ok(Cow::Borrowed("/"));
    }

    // 预先判断：快速路径——已规范？
    let fast = !s.contains("//")
        && !s.contains("./")
        && !s.ends_with('/')
        && !s.contains("/../")
        && !s.starts_with("./")
        && !s.starts_with("../");
    if fast {
        return Ok(Cow::Borrowed(s));
    }

    // 慢路径：解析并重组
    let mut comps: Vec<&str> = Vec::new();
    let abs = s.starts_with('/');

    for raw in s.split('/') {
        match raw {
            "" | "." => {}
            ".." => {
                if comps.pop().is_none() && !abs {
                    comps.push("..");
                }
            }
            seg => comps.push(seg),
        }
    }

    if comps.is_empty() && abs {
        return Ok(Cow::Borrowed("/"));
    }
    let mut out = String::with_capacity(s.len());
    if abs {
        out.push('/');
    }
    out.push_str(&comps.join("/"));
    Ok(Cow::Owned(out))
}

/* ===== Debug / Display / Hash / Eq impls ===== */

impl<'a> Debug for VfsPath<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VfsPath({})", self.0)
    }
}
impl<'a> Display for VfsPath<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl<'a> PartialEq for VfsPath<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<'a> Eq for VfsPath<'a> {}
impl<'a> PartialOrd for VfsPath<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.partial_cmp(other.0)
    }
}
impl<'a> Ord for VfsPath<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(other.0)
    }
}
impl<'a> Hash for VfsPath<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state)
    }
}

impl Debug for VfsPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VfsPathBuf({})", self.0)
    }
}
impl Display for VfsPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Hash for VfsPathBuf {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state)
    }
}
impl PartialOrd for VfsPathBuf {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}
impl Ord for VfsPathBuf {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}
