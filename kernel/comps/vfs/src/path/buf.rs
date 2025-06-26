//! 拥有型路径缓冲

use alloc::{borrow::Cow, string::ToString};
use alloc::string::String;
use core::ops::{Deref, DerefMut};

use crate::path::PathSlice;

use super::normalize;

/// `String` 包装，始终保持规范化
#[derive(Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct PathBuf(String);

impl PathBuf {
    /// 校验 + 规范化
    pub fn new<S: Into<String>>(s: S) -> crate::VfsResult<Self> {
        match normalize(&s.into())? {
            Cow::Borrowed(b) => Ok(Self(b.to_string())),
            Cow::Owned(o) => Ok(Self(o)),
        }
    }

    /// 内部字符串视图
    #[inline] pub fn as_str(&self) -> &str { &self.0 }

    /// **内部使用**：跳过校验直接构建
    #[inline] pub(crate) fn from_str_unchecked(s: String) -> Self { Self(s) }

    pub fn as_slice(&self) -> PathSlice { PathSlice::from(self) }
}

impl core::fmt::Display for PathBuf {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}
impl core::fmt::Debug for PathBuf {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "PathBuf({})", self.0)
    }
}
impl Deref for PathBuf {
    type Target = str;
    #[inline] fn deref(&self) -> &Self::Target { &self.0 }
}
impl DerefMut for PathBuf {
    #[inline] fn deref_mut(&mut self) -> &mut Self::Target { &mut self.0 }
}
