use alloc::{collections::VecDeque, sync::Arc};
use ostd::sync::{Mutex, WaitCell};

use crate::VfsResult;

pub struct RingPipe {
    /// 内部循环缓冲区
    pub buf: Mutex<VecDeque<u8>>,    
    /// 有数据写入时用于唤醒阻塞读取者
    notify: WaitCell,
}

impl RingPipe {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            buf: Mutex::new(VecDeque::with_capacity(4096)),
            notify: WaitCell::new(),
        })
    }

    /// 判断是否还有写端引用
    fn has_writer(this: &Arc<Self>) -> bool {
        // 约定：存在至少一个 `PipeWriter` 时 strong_count 大于 1；
        // 虽不十分精确（多读端也会增加计数），但在仅 fork 产生的常见场景下可以工作。
        Arc::strong_count(this) > 1
    }
}

pub struct PipeReader(pub Arc<RingPipe>);
pub struct PipeWriter(pub Arc<RingPipe>);

impl PipeReader {
    pub async fn read_at(&self, _ofs: u64, dst: &mut [u8]) -> VfsResult<usize> {
        loop {
            // 先尝试快速路径：直接拿锁查看是否有数据
            {
                let mut b = self.0.buf.lock().await;
                if !b.is_empty() {
                    let n = dst.len().min(b.len());
                    for i in 0..n {
                        dst[i] = b.pop_front().unwrap();
                    }
                    return Ok(n);
                }

                // 若无数据且也无写端，返回 EOF
                if !RingPipe::has_writer(&self.0) {
                    return Ok(0);
                }
            }

            // 等待写端唤醒
            // 如果 wait 过程中被关闭（此处不会发生），视为 EOF
            let _ = self.0.notify.wait().await.ok();
        }
    }
}

impl PipeWriter {
    pub async fn write_at(&self, _ofs: u64, src: &[u8]) -> VfsResult<usize> {
        {
            let mut b = self.0.buf.lock().await;
            for &c in src {
                b.push_back(c);
            }
        }
        // 唤醒可能阻塞的读端
        self.0.notify.wake();
        Ok(src.len())
    }
}

impl Drop for PipeWriter {
    fn drop(&mut self) {
        // 写端计数减少，可能已有读端在等待；
        // 释放锁同步不必等待，因此直接唤醒一次。
        // 此时唤醒时，还会继续运行当前的线程（在当前调度器和调度策略下），因此其他线程看到的计数会减少。
        self.0.notify.wake();
    }
}
