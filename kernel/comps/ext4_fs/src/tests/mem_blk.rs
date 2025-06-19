//! ext4_fs — 内核端综合测试集
//! 组织：
//! 1. 内存块设备 `MemBlockDev`（带随机错注入、trace 打点）
//! 2. 通用 `TestCtx`：挂载 ext4 → 返回 (manager, root)
//! 3. 分层用例（L0~L6），详见函数注释

// extern crate alloc;

use alloc::{boxed::Box, string::{String, ToString}, sync::Arc, vec::Vec, vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use nexus_error::error_stack::ResultExt;
use ostd::{prelude::ktest, sync::Mutex, task::scheduler::blocking_future::BlockingFuture};
use tracing::{debug, info, trace};

use crate::{get_ext4_provider, tests::block_on};
use vfs::{
    types::FileMode, AsyncBlockDevice, FileSystemProvider, DirectoryEntry, OpenFlags,
    VfsManager, VfsManagerBuilder, VfsResult, VnodeType,
};

use another_ext4::{Block, BlockDevice};
use vfs::types::OsStr;

/// —— 1. 内存块设备实现 ——————————————————————————————————————
const BLOCK_SIZE: usize = 4096;
const TOTAL_BLOCKS: usize = 1024; // 4 MiB

struct MemBlockDev {
    data: Mutex<Box<[u8]>>,
    dev_id: u64,
    modified: AtomicU64,
}

impl MemBlockDev {
    fn new(init_img: &[u8]) -> Arc<Self> {
        assert_eq!(init_img.len(), TOTAL_BLOCKS * BLOCK_SIZE);
        let mut v = vec![0u8; init_img.len()].into_boxed_slice();
        v.copy_from_slice(init_img);
        Arc::new(Self {
            data: Mutex::new(v),
            dev_id: 1,
            modified: AtomicU64::new(0),
        })
    }
    /// 随机注入写错误（这里简单地在偶数次写时报错）
    fn maybe_fail(&self) -> VfsResult<()> {
        let n = self.modified.fetch_add(1, Ordering::Relaxed);
        if n % 17 == 0 {
            Err(vfs::vfs_err_io_error!("fault‑inject"))
        } else {
            Ok(())
        }
    }
}

// impl BlockDevice for MemBlockDev {
//     fn read_block(&self, block_id: u64) -> Block {
//         let mut block = Block::default();
//         let off = block_id as usize * BLOCK_SIZE;
//         block.data.copy_from_slice(&self.data.lock().block()[off..off + BLOCK_SIZE]);
//         block.id = block_id;
//         block
//     }
//     fn write_block(&self, block: &Block) {
//         let off = block.id as usize * BLOCK_SIZE;
//         self.data.lock().block()[off..off + block.data.len()].copy_from_slice(block.data.as_ref());
//     }
// }

#[async_trait::async_trait]
impl AsyncBlockDevice for MemBlockDev {
    fn device_id(&self) -> u64 {
        self.dev_id
    }
    fn block_size_bytes(&self) -> VfsResult<u32> {
        Ok(BLOCK_SIZE as u32)
    }
    fn total_blocks(&self) -> VfsResult<u64> {
        Ok(TOTAL_BLOCKS as u64)
    }
    async fn read_blocks(&self, start: u64, buf: &mut [u8]) -> VfsResult<()> {
        let off = start as usize * BLOCK_SIZE;
        trace!(blk = start, len = buf.len(), "mem‑read");
        buf.copy_from_slice(&self.data.lock().await[off..off + buf.len()]);
        Ok(())
    }
    async fn write_blocks(&self, start: u64, buf: &[u8]) -> VfsResult<()> {
        self.maybe_fail().attach_printable("write_blocks")?;
        let off = start as usize * BLOCK_SIZE;
        trace!(blk = start, len = buf.len(), "mem‑write");
        self.data.lock().await[off..off + buf.len()].copy_from_slice(buf);
        Ok(())
    }
    async fn flush(&self) -> VfsResult<()> {
        Ok(())
    }
}

/// —— 2. 公共上下文 ——
/// 返回已挂载的 VFS 管理器与根 vnode。
struct TestCtx {
    vfs: Arc<VfsManager>,
    root: Arc<dyn vfs::Vnode + Send + Sync>,
}

impl TestCtx {
    fn new() -> Self {
        // 加载内置只读镜像；真实项目可替换为外部文件。
        // const IMG: &[u8] = include_bytes!("../../../../../sdcard-rv.img");
        const IMG: &[u8] = &[0u8; TOTAL_BLOCKS * BLOCK_SIZE]; // 使用全零填充的内存块设备

        let blk = MemBlockDev::new(IMG);
        let provider = get_ext4_provider();

        let mut vfs = VfsManagerBuilder::new()
            .provider(provider)
            .build();

        block_on(async {
            vfs.mount(Some(blk), "/", "ext4", Default::default())
                .await
                .expect("mount fs");
        });

        let root = block_on(vfs.get_vnode("/", false)).expect("root vnode");

        Self { vfs, root }
    }
}

/* ============================================================== */
/* =========================  TESTS  ============================ */
/* ============================================================== */

#[ktest]
fn test_mount_statfs() {
    let t = TestCtx::new();
    let stats = t.vfs.stat("/", false).block().unwrap();
    info!(?stats, "statfs");
}

#[ktest]
fn test_lookup_read() {
    let t = TestCtx::new();
    let etc = t.root.lookup("etc".as_ref()) .block().unwrap();
    assert_eq!(etc.metadata().block().unwrap().kind, VnodeType::Directory);
    // 读取一个已知文件
    let f = etc.lookup("passwd".as_ref()).block().unwrap();
    let h = f.open_file_handle(OpenFlags::RDONLY).block().unwrap();
    let mut buf = [0u8; 16];
    let n = h.read_at(0, &mut buf).block().unwrap();
    debug!("read passwd {} B: {:?}", n, &buf[..n]);
    assert!(n > 0);
}

#[ktest]
fn test_create_write_read() {
    let t = TestCtx::new();
    let tmp = t.root.mkdir("tmp".as_ref(), FileMode::OWNER_RWE).block().unwrap();
    let file = tmp.create_node("foo.txt".as_ref(), VnodeType::File, FileMode::OWNER_RW, None).block().unwrap();

    let h = file.open_file_handle(OpenFlags::RDWR).block().unwrap();

    let data = b"hello-ext4";
    h.write_at(0, data).block().unwrap();
    h.flush().block().unwrap();

    let mut readback = [0u8; 16];
    let n = h.read_at(0, &mut readback).block().unwrap();
    assert_eq!(&readback[..n], data);
}

#[ktest]
fn test_mkdir_readdir_rmdir() {
    let t = TestCtx::new();
    let dir = t.root.mkdir("a".as_ref(), FileMode::OWNER_RWE).block().unwrap();
    dir.clone().mkdir("b".as_ref(), FileMode::OWNER_RWE).block().unwrap();
    let open_flag = OpenFlags::RDONLY;
    let dh = dir.clone().open_dir_handle(open_flag).block().unwrap();
    let mut names = Vec::<String>::new();
    let mut d = dh.clone();
    while let Some(e) = d.clone().readdir().block().unwrap() {
        names.push(e.name);
    }
    assert!(names.contains(&"b".to_string()));
    dir.rmdir("b".as_ref()).block().unwrap();
}

#[ktest]
fn test_symlink_readlink() {
    let t = TestCtx::new();
    let link = t.root.symlink_node("sym".as_ref(), &"etc/passwd".into()).block().unwrap();
    let target = link.readlink().block().unwrap();
    assert_eq!(target.as_str(), "etc/passwd");
}

#[ktest]
fn test_rename_unlink() {
    let t = TestCtx::new();
    let tmp = t.root.clone().mkdir("rename".as_ref(), FileMode::OWNER_RWE).block().unwrap();
    let _f = tmp.clone().create_node("old".as_ref(), VnodeType::File, FileMode::OWNER_RW, None).block().unwrap();
    tmp.clone().rename("old".as_ref(), t.root, "new".as_ref()).block().unwrap();
    tmp.unlink("new".as_ref()).block().unwrap();
}

#[ktest]
fn test_parallel_rw() {
    use ostd::task::{scheduler::spawn, yield_now};
    let t = TestCtx::new();
    let tmp = t.root.mkdir("p".as_ref(), FileMode::OWNER_RWE).block().unwrap();

    // 两个并发 writer
    let writer = move |idx: u32| {
        let tmp = tmp.clone();
        async move {
            let name = alloc::format!("f{}.bin", idx);
            let v = tmp.clone().create_node(name.as_ref(), VnodeType::File, FileMode::OWNER_RW, None).block().unwrap();
            let h = v.open_file_handle(OpenFlags::RDWR).block().unwrap();
            let buf = [idx as u8; 512];
            for round in 0..16 {
                h.write_at((round * 512) as u64, &buf).block().unwrap();
                yield_now().block();
            }
        }
    };
    let mut core = ostd::task::scheduler::Core::new();
    let condition = Arc::new(AtomicBool::new(true));
    let condition_clone = condition.clone();
    spawn(async move {
        let t1 = spawn(writer(1), None);
        let t2 = spawn(writer(2), None);
        t1.await.unwrap();
        t2.await.unwrap();
        condition_clone.store(false, Ordering::Relaxed);
    }, None);
    core.run_while(|| condition.load(Ordering::Relaxed));
}
