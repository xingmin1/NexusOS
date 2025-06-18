//! 规范化 + 组件迭代

#![allow(clippy::needless_return)]
use alloc::{borrow::Cow, vec::Vec};

use crate::{vfs_err_invalid_argument, VfsResult};

/// 将任意 UTF‑8 字符串规范化为 POSIX 风格
///
/// * 删除重复 `/`  
/// * 解析 `.` / `..`  
/// * 保留绝对/相对语义，不解析符号链接
pub fn normalize(s: &str) -> VfsResult<Cow<'_, str>> {
    if s.is_empty() { return Err(vfs_err_invalid_argument!("empty")); }
    if s.contains('\0') { return Err(vfs_err_invalid_argument!("NUL")); }

    if s == "/" { return Ok(Cow::Borrowed("/")); }

    // fast‑path：无需修改
    let fast = !s.contains("//") && !s.ends_with('/') && !s.contains("/./") &&
               !s.contains("/../") && !s.starts_with("./") && !s.starts_with("../");
    if fast { return Ok(Cow::Borrowed(s)); }

    // slow‑path：一遍扫描
    let mut comps: Vec<&str> = Vec::new();
    let abs = s.starts_with('/');

    for raw in s.split('/') {
        match raw {
            "" | "." => {}
            ".." => { if comps.pop().is_none() && !abs { comps.push(".."); } }
            seg => comps.push(seg),
        }
    }

    if comps.is_empty() && abs {
        return Ok(Cow::Borrowed("/"));
    }
    let mut out = alloc::string::String::new();
    if abs { out.push('/'); }
    out.push_str(&comps.join("/"));
    Ok(Cow::Owned(out))
}

/// 只读组件迭代器
#[derive(Clone)]
pub struct Components<'a> {
    inner: core::str::Split<'a, char>,
}
impl<'a> Components<'a> {
    #[inline] pub(crate) fn new(s: &'a str) -> Self { Self { inner: s.split('/') } }
}
impl<'a> Iterator for Components<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        for seg in &mut self.inner {
            if seg.is_empty() || seg == "." { continue; }
            return Some(seg);
        }
        None
    }
}
