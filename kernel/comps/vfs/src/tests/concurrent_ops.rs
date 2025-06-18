//! VFS 并发操作测试
//!
//! 本测试文件验证 VFS 在并发环境下的操作正确性
//! 
use alloc::{
    format,
    sync::Arc,
    vec::Vec,
    string::String,
};

use nexus_os_vfs::{
    VfsManager, VfsManagerBuilder, get_memfs_provider,
    types::{OpenFlags, VnodeType, DirectoryEntry},
    path::PathBuf,
};
use maitake::scheduler::Scheduler;
use nexus_error::Errno;

// 任务数量
const TASKS: usize = if cfg!(miri) { 2 } else { 10 };

/// 测试并发文件创建
#[test]
fn test_concurrent_file_creation() {
    // 创建调度器
    let scheduler = Scheduler::new();
    
    // 创建 VFS 管理器
    let vfs = Arc::new(
        VfsManagerBuilder::new()
            .provider(get_memfs_provider())
            .vnode_cache_capacity(100)
            .dentry_cache_capacity(100)
            .build()
    );
    
    // 挂载根文件系统
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            vfs.mount(None, "/", "memfs", Default::default()).await.unwrap();
        }
    });
    
    // 创建测试目录
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            vfs.mkdir("/concurrent", 0o755).await.unwrap();
        }
    });
    
    // 执行初始化任务
    run_to_completion(&scheduler);
    
    // 并发创建多个文件
    for i in 0..TASKS {
        scheduler.spawn({
            let vfs = vfs.clone();
            let i = i;
            async move {
                let path = format!("/concurrent/file_{}.txt", i);
                let file_handle = vfs.open(&path, OpenFlags::CREATE | OpenFlags::WRITE).await.unwrap();
                let content = format!("文件 {} 的内容", i);
                file_handle.write_at(0, content.as_bytes()).await.unwrap();
                file_handle.close().await.unwrap();
            }
        });
    }
    
    // 执行并发文件创建任务
    run_to_completion(&scheduler);
    
    // 验证所有文件都已创建
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            let entries = vfs.readdir("/concurrent").await.unwrap();
            assert_eq!(entries.len(), TASKS);
            
            // 验证文件内容
            for i in 0..TASKS {
                let path = format!("/concurrent/file_{}.txt", i);
                let file_handle = vfs.open(&path, OpenFlags::READ).await.unwrap();
                let mut buf = alloc::vec![0u8; 100];
                let read_len = file_handle.read_at(0, &mut buf).await.unwrap();
                let content = core::str::from_utf8(&buf[0..read_len]).unwrap();
                let expected = format!("文件 {} 的内容", i);
                assert_eq!(content, expected);
                file_handle.close().await.unwrap();
            }
        }
    });
    
    // 执行验证任务
    run_to_completion(&scheduler);
}

/// 测试并发读写同一文件
#[test]
fn test_concurrent_file_access() {
    // 创建调度器
    let scheduler = Scheduler::new();
    
    // 创建 VFS 管理器
    let vfs = Arc::new(
        VfsManagerBuilder::new()
            .provider(get_memfs_provider())
            .build()
    );
    
    // 挂载根文件系统
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            vfs.mount(None, "/", "memfs", Default::default()).await.unwrap();
        }
    });
    
    // 创建测试文件
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            let file_handle = vfs.open("/shared.txt", OpenFlags::CREATE | OpenFlags::WRITE).await.unwrap();
            let data = "0".as_bytes(); // 初始值
            file_handle.write_at(0, data).await.unwrap();
            file_handle.close().await.unwrap();
        }
    });
    
    // 执行初始化任务
    run_to_completion(&scheduler);
    
    // 并发读取和递增文件内容
    for _ in 0..TASKS {
        scheduler.spawn({
            let vfs = vfs.clone();
            async move {
                let file_handle = vfs.open("/shared.txt", OpenFlags::READ | OpenFlags::WRITE).await.unwrap();
                
                // 读取当前值
                let mut buf = alloc::vec![0u8; 10];
                let read_len = file_handle.read_at(0, &mut buf).await.unwrap();
                let content = core::str::from_utf8(&buf[0..read_len]).unwrap();
                
                // 转换为数字并递增
                let value = content.parse::<i32>().unwrap();
                let new_value = value + 1;
                let new_content = new_value.to_string();
                
                // 写回递增后的值
                file_handle.write_at(0, new_content.as_bytes()).await.unwrap();
                file_handle.close().await.unwrap();
            }
        });
    }
    
    // 执行并发访问任务
    run_to_completion(&scheduler);
    
    // 验证最终结果
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            let file_handle = vfs.open("/shared.txt", OpenFlags::READ).await.unwrap();
            let mut buf = alloc::vec![0u8; 10];
            let read_len = file_handle.read_at(0, &mut buf).await.unwrap();
            let content = core::str::from_utf8(&buf[0..read_len]).unwrap();
            
            // 最终值应该是初始值(0) + TASKS
            let value = content.parse::<i32>().unwrap();
            assert_eq!(value, TASKS as i32);
            
            file_handle.close().await.unwrap();
        }
    });
    
    // 执行验证任务
    run_to_completion(&scheduler);
}

/// 测试并发文件和目录操作
#[test]
fn test_mixed_concurrent_operations() {
    // 创建调度器
    let scheduler = Scheduler::new();
    
    // 创建 VFS 管理器
    let vfs = Arc::new(
        VfsManagerBuilder::new()
            .provider(get_memfs_provider())
            .build()
    );
    
    // 挂载根文件系统
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            vfs.mount(None, "/", "memfs", Default::default()).await.unwrap();
        }
    });
    
    // 执行初始化任务
    run_to_completion(&scheduler);
    
    // 并发创建目录和文件的混合操作
    for i in 0..TASKS {
        // 创建目录
        scheduler.spawn({
            let vfs = vfs.clone();
            let i = i;
            async move {
                let dir_path = format!("/dir_{}", i);
                vfs.mkdir(&dir_path, 0o755).await.unwrap();
            }
        });
        
        // 在目录中创建文件
        scheduler.spawn({
            let vfs = vfs.clone();
            let i = i;
            async move {
                // 等待目录创建完成
                let dir_path = format!("/dir_{}", i);
                
                // 尝试等待目录就绪
                let mut retries = 0;
                while retries < 10 {
                    if let Ok(_) = vfs.stat(&dir_path, true).await {
                        break;
                    }
                    // 简单的重试逻辑
                    retries += 1;
                }
                
                // 在目录中创建文件
                let file_path = format!("{}/file.txt", dir_path);
                let file_handle = vfs.open(&file_path, OpenFlags::CREATE | OpenFlags::WRITE).await.unwrap();
                let content = format!("目录 {} 中的文件", i);
                file_handle.write_at(0, content.as_bytes()).await.unwrap();
                file_handle.close().await.unwrap();
            }
        });
    }
    
    // 执行混合操作任务
    run_to_completion(&scheduler);
    
    // 验证所有操作结果
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            // 验证根目录下有TASKS个目录
            let entries = vfs.readdir("/").await.unwrap();
            assert_eq!(entries.len(), TASKS);
            
            // 验证每个目录中都有一个文件
            for i in 0..TASKS {
                let dir_path = format!("/dir_{}", i);
                let entries = vfs.readdir(&dir_path).await.unwrap();
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].name, "file.txt");
                
                // 验证文件内容
                let file_path = format!("{}/file.txt", dir_path);
                let file_handle = vfs.open(&file_path, OpenFlags::READ).await.unwrap();
                let mut buf = alloc::vec![0u8; 100];
                let read_len = file_handle.read_at(0, &mut buf).await.unwrap();
                let content = core::str::from_utf8(&buf[0..read_len]).unwrap();
                let expected = format!("目录 {} 中的文件", i);
                assert_eq!(content, expected);
                file_handle.close().await.unwrap();
            }
        }
    });
    
    // 执行验证任务
    run_to_completion(&scheduler);
}

/// 辅助函数：运行调度器直到所有任务完成
fn run_to_completion(scheduler: &Scheduler) {
    // 执行任务直到全部完成
    let mut total_ticks = 0;
    loop {
        let tick = scheduler.tick();
        if tick.did_work() {
            total_ticks += 1;
        } else {
            // 如果没有进行中的任务了，就退出循环
            if scheduler.tasks_count() == 0 {
                break;
            }
        }
        
        // 安全阀，避免无限循环
        assert!(total_ticks < 10000, "似乎陷入了死循环，任务未能完成");
    }
}
