//! VFS 基础功能测试
//!
//! 本测试文件使用 maitake 调度器来测试 VFS 的基本功能

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
    ostd::task::scheduler::spawn(test_basic_file_operations(), None);
    let mut core = ostd::task::scheduler::Core::new();
    core.run();
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

// /// 测试符号链接
// #[test]
// fn test_symlink() {
//     // 创建调度器
//     let scheduler = Scheduler::new();

//     // 创建VFS管理器
//     let vfs = Arc::new(
//         VfsManagerBuilder::new()
//             .provider(Arc::new(get_memfs_provider()))
//             .build()
//     );

//     // 挂载根文件系统
//     scheduler.spawn({
//         let vfs = vfs.clone();
//         async move {
//             vfs.mount(None, "/", "memfs", Default::default()).await.unwrap();
//         }
//     });

//     // 创建目录和文件
//     scheduler.spawn({
//         let vfs = vfs.clone();
//         async move {
//             vfs.mkdir("/test_dir", 0o755).await.unwrap();

//             let file_handle = vfs.open("/test_dir/target.txt", OpenFlags::CREATE | OpenFlags::WRITE).await.unwrap();
//             let data = "这是目标文件内容".as_bytes();
//             file_handle.write_at(0, data).await.unwrap();
//             file_handle.close().await.unwrap();
//         }
//     });

//     // 创建符号链接
//     scheduler.spawn({
//         let vfs = vfs.clone();
//         async move {
//             vfs.symlink("/test_dir/target.txt", "/test_link").await.unwrap();
//         }
//     });

//     // 执行准备任务
//     run_to_completion(&scheduler);

//     // 测试读取符号链接
//     scheduler.spawn({
//         let vfs = vfs.clone();
//         async move {
//             let target = vfs.readlink("/test_link").await.unwrap();
//             assert_eq!(target.as_str(), "/test_dir/target.txt");

//             // 通过符号链接访问文件
//             let file_handle = vfs.open("/test_link", OpenFlags::READ).await.unwrap();
//             let mut buf = alloc::vec![0u8; 100];
//             let read_len = file_handle.read_at(0, &mut buf).await.unwrap();
//             let content = core::str::from_utf8(&buf[0..read_len]).unwrap();
//             assert_eq!(content, "这是目标文件内容");
//             file_handle.close().await.unwrap();
//         }
//     });

//     // 执行符号链接测试
//     run_to_completion(&scheduler);
// }

// /// 测试重命名操作
// #[test]
// fn test_rename() {
//     // 创建调度器
//     let scheduler = Scheduler::new();

//     // 创建VFS管理器
//     let vfs = Arc::new(
//         VfsManagerBuilder::new()
//             .provider(Arc::new(get_memfs_provider()))
//             .build()
//     );

//     // 挂载根文件系统
//     scheduler.spawn({
//         let vfs = vfs.clone();
//         async move {
//             vfs.mount(None, "/", "memfs", Default::default()).await.unwrap();
//         }
//     });

//     // 创建测试文件
//     scheduler.spawn({
//         let vfs = vfs.clone();
//         async move {
//             let file_handle = vfs.open("/old_file.txt", OpenFlags::CREATE | OpenFlags::WRITE).await.unwrap();
//             let data = "测试文件内容".as_bytes();
//             file_handle.write_at(0, data).await.unwrap();
//             file_handle.close().await.unwrap();
//         }
//     });

//     // 执行准备任务
//     run_to_completion(&scheduler);

//     // 测试重命名
//     scheduler.spawn({
//         let vfs = vfs.clone();
//         async move {
//             vfs.rename("/old_file.txt", "/new_file.txt").await.unwrap();

//             // 验证旧文件不存在
//             let result = vfs.stat("/old_file.txt", true).await;
//             assert!(result.is_err());

//             // 验证新文件存在
//             let metadata = vfs.stat("/new_file.txt", true).await.unwrap();
//             assert_eq!(metadata.kind, VnodeType::File);

//             // 验证新文件内容
//             let file_handle = vfs.open("/new_file.txt", OpenFlags::READ).await.unwrap();
//             let mut buf = alloc::vec![0u8; 100];
//             let read_len = file_handle.read_at(0, &mut buf).await.unwrap();
//             let content = core::str::from_utf8(&buf[0..read_len]).unwrap();
//             assert_eq!(content, "测试文件内容");
//             file_handle.close().await.unwrap();
//         }
//     });

//     // 执行重命名测试
//     run_to_completion(&scheduler);
// }

// /// 辅助函数：运行调度器直到所有任务完成
// fn run_to_completion(scheduler: &Scheduler) {
//     // 执行任务直到全部完成
//     let mut total_ticks = 0;
//     loop {
//         let tick = scheduler.tick();
//         if tick.completed > 0 {
//             total_ticks += 1;
//         } else {
//             // 如果没有进行中的任务了，就退出循环
//             if !tick.has_remaining {
//                 break;
//             }
//         }

//         // 安全阀，避免无限循环
//         assert!(total_ticks < 10000, "似乎陷入了死循环，任务未能完成");
//     }
// }
