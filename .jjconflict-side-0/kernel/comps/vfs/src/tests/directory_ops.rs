//! VFS 目录操作测试
//!
//! 本测试文件验证 VFS 的目录相关操作

use alloc::{
    format,
    sync::Arc,
    vec::Vec,
    string::String,
};

use nexus_os_vfs::{
    VfsManager, VfsManagerBuilder, get_memfs_provider,
    types::{OpenFlags, VnodeType, DirectoryEntry},
    path::VfsPathBuf,
};
use maitake::scheduler::Scheduler;
use nexus_error::Errno;

/// 目录创建与删除测试
#[test]
fn test_directory_create_delete() {
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
    
    // 创建多层嵌套目录
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            vfs.mkdir("/parent", 0o755).await.unwrap();
            vfs.mkdir("/parent/child", 0o755).await.unwrap();
            vfs.mkdir("/parent/child/grandchild", 0o755).await.unwrap();
        }
    });
    
    // 执行目录创建任务
    run_to_completion(&scheduler);
    
    // 验证目录结构
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            // 验证根目录
            let entries = vfs.readdir("/").await.unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].name, "parent");
            assert_eq!(entries[0].kind, VnodeType::Directory);
            
            // 验证父目录
            let entries = vfs.readdir("/parent").await.unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].name, "child");
            assert_eq!(entries[0].kind, VnodeType::Directory);
            
            // 验证子目录
            let entries = vfs.readdir("/parent/child").await.unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].name, "grandchild");
            assert_eq!(entries[0].kind, VnodeType::Directory);
            
            // 验证孙目录
            let entries = vfs.readdir("/parent/child/grandchild").await.unwrap();
            assert_eq!(entries.len(), 0); // 空目录
        }
    });
    
    // 执行目录验证任务
    run_to_completion(&scheduler);
    
    // 在目录中创建文件
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            let file_handle = vfs.open("/parent/child/file.txt", OpenFlags::CREATE | OpenFlags::WRITE).await.unwrap();
            file_handle.write_at(0, b"文件内容").await.unwrap();
            file_handle.close().await.unwrap();
        }
    });
    
    // 执行文件创建任务
    run_to_completion(&scheduler);
    
    // 测试不能删除非空目录
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            let result = vfs.rmdir("/parent/child").await;
            assert!(result.is_err()); // 应该失败，因为目录非空
        }
    });
    
    // 执行删除非空目录测试
    run_to_completion(&scheduler);
    
    // 删除文件后再删除目录
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            // 先删除文件
            vfs.unlink("/parent/child/file.txt").await.unwrap();
            
            // 自下而上删除目录
            vfs.rmdir("/parent/child/grandchild").await.unwrap();
            vfs.rmdir("/parent/child").await.unwrap();
            vfs.rmdir("/parent").await.unwrap();
        }
    });
    
    // 执行删除任务
    run_to_completion(&scheduler);
    
    // 验证根目录为空
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            let entries = vfs.readdir("/").await.unwrap();
            assert_eq!(entries.len(), 0);
        }
    });
    
    // 执行最终验证
    run_to_completion(&scheduler);
}

/// 测试目录句柄操作
#[test]
fn test_directory_handle() {
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
    
    // 创建目录和文件
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            vfs.mkdir("/test_dir", 0o755).await.unwrap();
            
            // 创建多个文件用于目录项遍历测试
            for i in 0..5 {
                let file_path = format!("/test_dir/file_{}.txt", i);
                let file_handle = vfs.open(&file_path, OpenFlags::CREATE | OpenFlags::WRITE).await.unwrap();
                file_handle.close().await.unwrap();
            }
        }
    });
    
    // 执行准备任务
    run_to_completion(&scheduler);
    
    // 测试目录读取
    scheduler.spawn({
        let vfs = vfs.clone();
        async move {
            // 使用 readdir API 读取目录
            let entries = vfs.readdir("/test_dir").await.unwrap();
            
            // 应该有5个文件
            assert_eq!(entries.len(), 5);
            
            // 验证所有文件名前缀一致
            for entry in &entries {
                assert!(entry.name.starts_with("file_"));
                assert!(entry.name.ends_with(".txt"));
                assert_eq!(entry.kind, VnodeType::File);
            }
        }
    });
    
    // 执行目录读取测试
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
