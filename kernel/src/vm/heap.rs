// SPDX-License-Identifier: MPL-2.0

use core::sync::atomic::{AtomicUsize, Ordering};
use nexus_error::{return_errno_with_message, Errno, Result};
use align_ext::AlignExt;
use aster_rights::Full;
use ostd::mm::{Vaddr, PAGE_SIZE};
use crate::{
    vm::{perms::VmPerms, vmar::Vmar},
};

/// The base address of user heap
pub const USER_HEAP_BASE: Vaddr = 0x0000_0000_1000_0000;
/// The max allowed size of user heap
pub const USER_HEAP_SIZE_LIMIT: usize = 16 * 1024 * PAGE_SIZE; // 16 * 4MB

#[derive(Debug)]
pub struct Heap {
    /// The lowest address of the heap
    base: Vaddr,
    /// The heap size limit
    limit: usize,
    /// The current heap highest address
    current_heap_end: AtomicUsize,
}

impl Heap {
    pub const fn new() -> Self {
        Heap {
            base: USER_HEAP_BASE,
            limit: USER_HEAP_SIZE_LIMIT,
            current_heap_end: AtomicUsize::new(USER_HEAP_BASE),
        }
    }

    /// Initializes and maps the heap virtual memory.
    pub(super) async fn alloc_and_map_vm(&self, root_vmar: &Vmar<Full>) -> Result<()> {
        let vmar_map_options = {
            let perms = VmPerms::READ | VmPerms::WRITE;
            root_vmar
                .new_map(PAGE_SIZE, perms)
                .unwrap()
                .offset(self.base)
        };
        vmar_map_options.build().await?;

        // If we touch another mapped range when we are trying to expand the
        // heap, we fail.
        //
        // So a simple solution is to reserve enough space for the heap by
        // mapping without any permissions and allow it to be overwritten
        // later by `brk`. New mappings from `mmap` that overlaps this range
        // may be moved to another place.
        let vmar_reserve_options = {
            let perms = VmPerms::empty();
            root_vmar
                .new_map(USER_HEAP_SIZE_LIMIT - PAGE_SIZE, perms)
                .unwrap()
                .offset(self.base + PAGE_SIZE)
        };
        vmar_reserve_options.build().await?;

        self.set_uninitialized();
        Ok(())
    }

    pub async  fn brk(&self, new_heap_end: Option<Vaddr>, root_vmar: &Vmar<Full>) -> Result<Vaddr> {
        match new_heap_end {
            None => Ok(self.current_heap_end.load(Ordering::Relaxed)),
            Some(new_heap_end) => {
                if new_heap_end > self.base + self.limit {
                    return_errno_with_message!(Errno::ENOMEM, "heap size limit was met.");
                }
                let current_heap_end_raw = self.current_heap_end.load(Ordering::Acquire);

                // 如果申请的地址不高于当前堆顶，直接返回（暂不支持收缩）。
                if new_heap_end <= current_heap_end_raw {
                    return Ok(current_heap_end_raw);
                }

                // 仅用于映射操作的页对齐地址
                let current_heap_end_pg = current_heap_end_raw.align_up(PAGE_SIZE);
                let new_heap_end_pg = new_heap_end.align_up(PAGE_SIZE);

                // new_heap_end_pg 可能与 current_heap_end_pg 相等（在同一页内扩展），
                // 此时无需拆除映射，否则会向 VMAR 传入长度为 0 的区间触发断言。
                if new_heap_end_pg > current_heap_end_pg {
                    root_vmar
                        .remove_mapping(current_heap_end_pg..new_heap_end_pg)
                        .await?;

                    let old_size = current_heap_end_pg - self.base;
                    let new_size = new_heap_end_pg - self.base;
                    
                    root_vmar.resize_mapping(self.base, old_size, new_size).await?;
                }

                // 用未对齐的用户请求值更新堆顶，保持字节粒度可见性
                self.current_heap_end
                    .store(new_heap_end, Ordering::Release);
                Ok(new_heap_end)
            }
        }
    }

    pub(super) fn set_uninitialized(&self) {
        self.current_heap_end
            .store(self.base + PAGE_SIZE, Ordering::Relaxed);
    }
}

impl Clone for Heap {
    fn clone(&self) -> Self {
        let current_heap_end = self.current_heap_end.load(Ordering::Relaxed);
        Self {
            base: self.base,
            limit: self.limit,
            current_heap_end: AtomicUsize::new(current_heap_end),
        }
    }
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}
