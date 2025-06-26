//! 借用型路径切片

use alloc::{borrow::Cow, string::ToString};
use crate::path::PathBuf;

use super::normalize;

/// 一个已保证 *规范化* 的路径借用。
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct PathSlice<'a>(&'a str);

impl<'a> PathSlice<'a> {
    /// 创建新切片；传入字符串将被规范化
    #[inline]
    pub fn new(source: &'a str) -> crate::VfsResult<Self> {
        match normalize(source)? {
            Cow::Borrowed(s) => Ok(Self(s)),
            // [TODO]: 可以在 Err 中返回 Owned 的 PathBuf？
            Cow::Owned(_) => unreachable!("normalize() Borrowed guaranteed"),
        }
    }

    /// 原始字符串
    #[inline] pub const fn as_str(self) -> &'a str { self.0 }

    /// 是否以 `/` 开头
    #[inline] pub const fn is_absolute(self) -> bool { self.0.as_bytes()[0] == b'/' }

    /// 是否根路径 `/`
    #[inline] pub fn is_root(self) -> bool { self.0 == "/" }

    /// 路径组件迭代器 (`&str`)
    #[inline]
    pub fn components(self) -> super::Components<'a> {
        super::Components::new(self.0)
    }

    /// 父目录；`None` 表示已经是根
    pub fn parent(self) -> Option<Self> {
        if self.is_root() { return None; }
        let idx = self.0.rfind('/')?;
        // `/foo` → `/`; `/foo/bar` → `/foo`
        if idx == 0 { Some(Self("/")) } else { Some(Self(&self.0[..idx])) }
    }

    /// 文件名
    #[inline]
    pub fn file_name(self) -> Option<&'a str> {
        if self.is_root() { None } else { self.0.rsplit('/').next() }
    }

    /// 是否已规范化，以 `base` 为前缀
    #[inline]
    pub fn starts_with(self, base: Self) -> bool {
        if base.is_root() { return self.is_absolute(); }
        self.0.starts_with(base.0)
            && (self.0.len() == base.0.len() || self.0.as_bytes()[base.0.len()] == b'/')
    }

    /// 去前缀；成功返回相对路径（`.` 代表空）
    #[inline]
    pub fn strip_prefix(self, base: Self) -> Option<Self> {
        if !self.starts_with(base) { return None; }
        if self.0.len() == base.0.len() { Some(Self(".")) }
        else {
            // 若 base 为根路径 "/"，无需再跳过一个分隔符。
            // 示例：
            //   self = "/foo/bar", base = "/"   =>  "foo/bar"
            //   self = "/foo/bar", base = "/foo" =>  "bar"
            let start = if base.is_root() { base.0.len() } else { base.0.len() + 1 };
            Some(Self(&self.0[start..]))
        }
    }

    /// 去后缀；成功返回父目录
    pub fn strip_suffix(self) -> Option<Self> {
        let last = self.0.rfind('/')?;
        Some(Self(&self.0[..last + 1]))
    }

    /// 连接单个组件；组件已保证无 `/`
    pub fn join(self, comp: &str) -> crate::VfsResult<super::PathBuf> {
        if comp.is_empty() || comp.contains('/') {
            Err(crate::vfs_err_invalid_argument!("bad component"))
        } else if self.is_root() {
            Ok(super::PathBuf::from_str_unchecked(alloc::format!("/{}", comp)))
        } else {
            Ok(super::PathBuf::from_str_unchecked(alloc::format!("{}/{}", self.0, comp)))
        }
    }

    pub fn to_owned_buf(self) -> PathBuf {
        PathBuf::from_str_unchecked(self.0.to_string())
    }
}

/// 便捷转换
impl<'a> From<&'a str> for PathSlice<'a> {
    fn from(s: &'a str) -> Self { Self::new(s).expect("invalid path literal") }
}

impl<'a> From<&'a PathBuf> for PathSlice<'a> {
    fn from(s: &'a PathBuf) -> Self { Self(s.as_str()) }
}