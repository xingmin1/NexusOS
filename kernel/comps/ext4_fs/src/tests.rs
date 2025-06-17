mod mem_blk;

use ostd::task::scheduler::blocking_future::BlockingFuture;

fn block_on<F: core::future::Future>(f: F) -> F::Output {
    f.block()
}
