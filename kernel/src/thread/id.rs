use core::sync::atomic::{AtomicU64, Ordering::Relaxed};

const FIRST_ID: u64 = 2;                 // 0‒1 预留给内核
static NEXT_ID: AtomicU64 = AtomicU64::new(FIRST_ID);

#[inline]
pub fn alloc() -> u64 {
    let id = NEXT_ID.fetch_add(1, Relaxed);
    if id == u64::MAX { panic!("ID space exhausted"); }
    id
}
