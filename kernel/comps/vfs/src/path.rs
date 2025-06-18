//! 路径类型（无文件系统状态）

extern crate alloc;

mod slice;
mod buf;
mod normalize;
pub use slice::PathSlice;
pub use buf::PathBuf;
pub use normalize::{normalize, Components};
