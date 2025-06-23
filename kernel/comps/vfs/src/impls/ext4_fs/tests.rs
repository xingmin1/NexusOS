// mod mem_blk;

use nexus_error::Errno;
// use ostd::task::scheduler::blocking_future::BlockingFuture;

// fn block_on<F: core::future::Future>(f: F) -> F::Output {
//     f.block()
// }

// kernel/comps/ext4_fs/src/tests.rs
//
// 真实 virtio‑blk 环境下的集成测试（由易到难仅 1 例即可覆盖核心路径）
// `cargo ktest` → QEMU 参数已在 workspace root 指定。
//
// * 依赖：ext4_fs + vfs + ostd(runtime)
// * 测试流程：mount → mkdir → create_file → write/read → rename → unlink → rmdir

extern crate alloc;

use alloc::{format, sync::Arc};
use crate::impls::ext4_fs::get_ext4_provider;
use crate::static_dispatch::provider::SProvider;
use crate::types::FileMode;
use crate::verror::KernelError;
use crate::{FileOpen, OpenStatus, VfsManager, VfsResult, VnodeType};
use core::sync::atomic::{AtomicBool, Ordering};
use nexus_error::error_stack::ResultExt;
use ostd::prelude::ktest;
use ostd::task::{scheduler, scheduler::spawn};
use tracing::{debug, error, info};


/// 业务逻辑放在单个 async 任务里，便于与调度器对接
async fn test_basic() -> VfsResult<()> {
    // --- 1. 挂载 ---
    let provider: SProvider = get_ext4_provider().into();
    let vfs_manager = VfsManager::builder().provider(provider).build();
    vfs_manager.mount(None, "/", "ext4", Default::default())
        .await
        .attach_printable("mount ext4")?;

    let (_, mount_info, _) = vfs_manager.locate_mount("/".into()).await?;
    let fs = mount_info.fs;
    let root_vnode = fs.root_vnode().await?;
    let root_vnode = root_vnode.to_dir().unwrap();

    info!("root mounted");

    // --- 2. mkdir ---
    let d_node = root_vnode
        .create("ktest".as_ref(), VnodeType::Directory, FileMode::OWNER_RWE, None)
        .await
        .attach_printable("mkdir ktest")?;
    let d_node = d_node.to_dir().unwrap();
    debug!("mkdir ok");

    // --- 3. create file & write ---
    let fnode = d_node
        .create("hello.txt".as_ref(), VnodeType::File, FileMode::OWNER_RWE, None)
        .await?;
    let fnode_handle = fnode
        .to_file()
        .unwrap()
        .clone()
        .open(FileOpen::new(2 | OpenStatus::CREATE.bits()).change_context_lazy(|| KernelError::new(Errno::EINVAL))?)
        .await
        .attach_printable("create file")?;

    let data = b"hello ext4";
    fnode_handle.write_at(0, data).await.attach_printable_lazy(|| {
        format!("write data to file failed: 当前位置：{}:{}:{}\n", file!(), line!(), column!())
    })?;
    fnode_handle.flush().await?;
    debug!("write/flush ok");

    // --- 4. read back ---
    let mut buf = [0u8; 16];
    let n = fnode_handle.read_at(0, &mut buf).await?;
    assert_eq!(&buf[..n], data);
    debug!("read ok => {:?}", &buf[..n]);

    // --- 5. rename ---
    d_node
        .rename("hello.txt".as_ref(), &d_node, "greet.txt".as_ref())
        .await
        .attach_printable("rename file")?;
    info!("rename ok");

    // --- 6. metadata sanity ---
    let meta = d_node.metadata().await?;
    assert_eq!(meta.kind, VnodeType::Directory);

    // --- 7. unlink & rmdir ---
    d_node.unlink("greet.txt".as_ref()).await?;
    root_vnode.rmdir("ktest".as_ref()).await?;
    info!("unlink + rmdir ok");

    // --- 8. list root ---
    let dir_handle = root_vnode.clone().open_dir().await?;
    let list = dir_handle.read_dir_chunk(None).await?;
    info!("list root: {:?}", &list[..]);
    info!("list root ok");

    Ok(())
}

/// 顶层 ktest：启动任务并驱动调度器
#[ktest]
fn test_core() {
    let worker = spawn(test_basic(), None);
    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();

    // 当 worker 完成后清零标志，Core 跳出循环
    spawn(
        async move {
            worker
                .await
                .inspect_err(|e| error!("run abort: {:?}", e))
                .unwrap()
                .inspect_err(|e| error!("worker failed: {:?}", e))
                .unwrap();
            flag.store(false, Ordering::Relaxed);
        },
        None,
    );

    let mut core = scheduler::Core::new();
    core.run_while(|| running.load(Ordering::Relaxed));
}
