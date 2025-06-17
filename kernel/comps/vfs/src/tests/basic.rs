//! VFS 基础功能测试
//!
//! 本测试文件使用 maitake 调度器来测试 VFS 的基本功能
#![allow(unused)]
use alloc::{format, string::String, sync::Arc, vec::Vec};

// use maitake::scheduler::Scheduler;
use nexus_error::Errno;
use ostd::arch::qemu::{exit_qemu, QemuExitCode};

use crate::{
    get_memfs_provider,
    path::VfsPathBuf,
    types::{DirectoryEntry, FileMode, OpenFlags, TimestampsToSet, VnodeType},
    VfsManager, VfsManagerBuilder,
};

// 任务数量 (用于并发测试)
const TASKS: usize = if cfg!(miri) { 2 } else { 10 };

use tracing::*;

/// 基础文件操作测试
#[ostd::prelude::ktest]
fn test_core() {
    let handle1 = ostd::task::scheduler::spawn(test_basic_file_operations(), None);
    // ostd::task::scheduler::spawn(other_tests, None);
    let condition = Arc::new(AtomicBool::new(true));
    let condition_clone = condition.clone();
    ostd::task::scheduler::spawn(async move{
        handle1.await.unwrap();
        condition_clone.store(false, Ordering::Relaxed);
    }, None);
    let mut core = ostd::task::scheduler::Core::new();
    core.run_while(|| condition.load(Ordering::Relaxed));
}

async fn test_basic_file_operations() {
    info!("Starting VFS test");
    let span = span!(Level::DEBUG, "vfs_test");
    let _enter = span.enter();

    let mut join_handles = Vec::new();

    // 创建 VFS 管理器
    info!("Creating VFS manager");
    let vfs = Arc::new(
        VfsManagerBuilder::new()
            .provider(Arc::new(get_memfs_provider()))
            .vnode_cache_capacity(100)
            .dentry_cache_capacity(100)
            .build(),
    );
    debug!("VFS manager created successfully");

    // 挂载根文件系统
    info!("Mounting root filesystem");
    let mount_task = ostd::task::scheduler::spawn(
        {
            let vfs = vfs.clone();
            async move {
                debug!("Starting mount operation");
                vfs.mount(None, "/", "inmemoryfs", Default::default())
                    .await
                    .inspect_err(|e| {
                        error!("Mount root fs failed: {:?}", e);
                    })
                    .unwrap();
                info!("Root filesystem mounted successfully");
            }
        },
        None,
    );
    join_handles.push(mount_task);

    // 创建测试目录
    info!("Creating test directory");
    let mkdir_task = ostd::task::scheduler::spawn(
        {
            let vfs = vfs.clone();
            async move {
                debug!("Creating directory /test_dir");
                vfs.mkdir("/test_dir", FileMode::OWNER_RWE | FileMode::GROUP_RE | FileMode::OTHER_RE)
                    .await
                    .inspect_err(|e| error!("Mkdir failed: {:?}", e))
                    .unwrap();
                info!("Directory /test_dir created successfully");
            }
        },
        None,
    );
    join_handles.push(mkdir_task);

    // 创建并写入文件
    info!("Creating and writing test file");
    let write_task = ostd::task::scheduler::spawn(
        {
            let vfs = vfs.clone();
            async move {
                debug!("Opening file /test_dir/test_file.txt");
                let file_handle = vfs
                    .open(
                        "/test_dir/test_file.txt",
                        OpenFlags::CREATE | OpenFlags::WRONLY,
                    )
                    .await
                    .inspect_err(|e| error!("Open file failed: {:?}", e))
                    .unwrap();
                debug!("Writing data to file");
                let data = "Hello, VFS World!".as_bytes();
                file_handle.write_at(0, data).await.unwrap();
                file_handle.close().await.unwrap();
                info!("File written successfully");
            }
        },
        None,
    );
    join_handles.push(write_task);

    // 读取文件内容
    info!("Reading test file");
    let read_task = ostd::task::scheduler::spawn(
        {
            let vfs = vfs.clone();
            async move {
                debug!("Opening file for reading");
                let file_handle = vfs
                    .open("/test_dir/test_file.txt", OpenFlags::RDONLY)
                    .await
                    .unwrap();
                let mut buf = alloc::vec![0u8; 100];
                debug!("Reading file content");
                let read_len = file_handle.read_at(0, &mut buf).await.unwrap();
                let content = core::str::from_utf8(&buf[0..read_len]).unwrap();
                assert_eq!(content, "Hello, VFS World!");
                file_handle.close().await.unwrap();
                info!("File read successfully, content verified");
            }
        },
        None,
    );
    join_handles.push(read_task);

    // 查询文件元数据
    info!("Getting file metadata");
    let stat_task = ostd::task::scheduler::spawn(
        {
            let vfs = vfs.clone();
            async move {
                debug!("Querying metadata for /test_dir/test_file.txt");
                let metadata = vfs.stat("/test_dir/test_file.txt", true).await.unwrap();
                assert_eq!(metadata.kind, VnodeType::File);
                assert_eq!(metadata.size, "Hello, VFS World!".len() as u64);
                info!("Metadata verified successfully");
            }
        },
        None,
    );
    join_handles.push(stat_task);

    // 执行所有任务
    info!("Executing all tasks");
    while let Some(handle) = join_handles.pop() {
        debug!("Waiting for task completion");
        let _ = handle.await;
    }
    info!("All tasks completed");

    // 创建另一批任务来验证之前的操作结果
    info!("Starting verification tasks");

    // 验证目录内容
    info!("Verifying directory contents");
    let verify_dir_task = ostd::task::scheduler::spawn(
        {
            let vfs = vfs.clone();
            async move {
                debug!("Reading directory /test_dir");
                let entries = vfs.readdir("/test_dir").await.unwrap();
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].name, "test_file.txt");
                assert_eq!(entries[0].kind, VnodeType::File);
                info!("Directory contents verified successfully");
            }
        },
        None,
    );
    join_handles.push(verify_dir_task);

    // 执行验证任务
    info!("Executing verification tasks");
    while let Some(handle) = join_handles.pop() {
        debug!("Waiting for verification task completion");
        let _ = handle.await;
    }
    info!("All verification tasks completed");

    exit_qemu(QemuExitCode::Success);
}
