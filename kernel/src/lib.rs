// SPDX-License-Identifier: MPL-2.0

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

use alloc::{sync::Arc, vec};
use core::{future::Future, str};

use align_ext::AlignExt;
use elf_loader::{
    abi::{PF_R, PF_W, PF_X, PT_LOAD},
    mmap::MmapImpl,
    object::ElfBinary,
    Loader,
};
use ostd::{
    arch::qemu::{exit_qemu, QemuExitCode},
    cpu::{CpuSet, PinCurrentCpu, UserContext},
    mm::{
        CachePolicy, FallibleVmRead, FrameAllocOptions, PageFlags, PageProperty,
        PrivilegedPageFlags, VmIo, VmSpace, VmWriter, PAGE_SIZE,
    },
    prelude::*,
    smp::inter_processor_call,
    task::{disable_preempt, Task, TaskOptions},
    user::{ReturnReason, UserContextApi, UserMode, UserSpace},
};

/// The kernel's boot and initialization process is managed by OSTD.
/// After the process is done, the kernel's execution environment
/// (e.g., stack, heap, tasks) will be ready for use and the entry function
/// labeled as `#[ostd::main]` will be called.
#[ostd::main]
pub fn main() {
    println!("开始执行内核主函数");
    let program_binary = include_bytes!("../hello");
    println!("加载程序二进制文件，大小: {} 字节", program_binary.len());

    // 解析ELF文件以获取入口点
    let (entry_point, vm_space) = create_vm_space(program_binary);
    println!("ELF入口点地址: 0x{:x}", entry_point);

    let vm_space = Arc::new(vm_space);
    println!("创建虚拟内存空间完成");
    let (user_task_1, task_future_1) = create_user_task(vm_space.clone(), entry_point, 3);
    let (user_task_2, task_future_2) = create_user_task(vm_space.clone(), entry_point, 5);
    println!("创建用户任务完成，准备运行");
    let mut core = ostd::task::scheduler::Core::new();
    // TODO: 使run消耗Task而不是Arc<Task>，这里不需要Arc<Task>
    user_task_1.run(task_future_1);
    user_task_2.run(task_future_2);
    println!("内核主函数完成设置，BSP 进入空闲循环");

    println!("注册 AP 入口函数");
    ostd::boot::smp::register_ap_entry(ap_main);

    let cpus = CpuSet::new_full();
    inter_processor_call(&cpus, || {
        let cpu_id = disable_preempt().current_cpu().as_usize();
        println!("CPU {} 运行 inter_processor_call", cpu_id);
    });

    core.run();
}

fn ap_main() -> ! {
    let cpu_id;
    {
        let disable_preempt = disable_preempt();
        cpu_id = disable_preempt.current_cpu().as_usize();
        println!("AP {} 进入 ap_main 函数，准备进入空闲循环", cpu_id);
    }

    let mut core = ostd::task::scheduler::Core::new();
    core.run();
    unreachable!()
}

fn create_vm_space(program: &[u8]) -> (usize, VmSpace) {
    println!("开始创建虚拟内存空间");

    // 解析ELF文件
    let mut elf = ElfBinary::new("hello", program);
    let mut loader = Loader::<MmapImpl>::new();
    let elf_header = loader.read_ehdr(&mut elf).unwrap();
    let elf_pheader = loader.read_phdr(&mut elf, &elf_header).unwrap();

    let offset = 0x0_usize;
    // 获取入口点地址
    let entry_point = elf_header.e_entry as usize + offset;
    println!("ELF入口点地址: 0x{:x}", entry_point);

    // 创建虚拟内存空间
    let vm_space = VmSpace::new();
    println!("创建新的虚拟内存空间");

    // 遍历所有的程序头
    for phdr in elf_pheader {
        // 只处理可加载段
        if phdr.p_type != PT_LOAD {
            println!("跳过非可加载段: 类型={}", phdr.p_type);
            continue;
        }

        println!(
            "处理程序头: vaddr=0x{:x}, paddr=0x{:x}, filesz=0x{:x}, memsz=0x{:x}, flags=0x{:x}, p_offset=0x{:x}",
            phdr.p_vaddr, phdr.p_paddr, phdr.p_filesz, phdr.p_memsz, phdr.p_flags, phdr.p_offset
        );

        // 计算段所需的内存大小（需要页对齐）
        let vaddr_start = phdr.p_vaddr as usize + offset;
        let vaddr_end = vaddr_start + (phdr.p_memsz as usize) + offset;
        let aligned_end = vaddr_end.align_up(PAGE_SIZE);
        let seg_size = aligned_end - vaddr_start;

        println!("段大小(页对齐后): 0x{:x} 字节", seg_size);

        // 分配物理内存
        let segment = FrameAllocOptions::new()
            .alloc_segment(seg_size / PAGE_SIZE)
            .unwrap();
        println!("分配物理内存段成功，页数: {}", seg_size / PAGE_SIZE);

        // 确定段的权限
        let mut page_flags = PageFlags::empty();

        // 根据ELF段标志设置页面权限
        if phdr.p_flags & PF_R != 0 {
            page_flags |= PageFlags::R;
        }
        if phdr.p_flags & PF_W != 0 {
            page_flags |= PageFlags::W;
        }
        if phdr.p_flags & PF_X != 0 {
            page_flags |= PageFlags::X;
        }

        // 标记为用户页面
        let page_prop = PageProperty::new(
            page_flags,
            CachePolicy::Writeback,
            PrivilegedPageFlags::USER,
        );

        // 计算段在文件中的偏移和大小
        let file_offset = phdr.p_offset as usize;
        let file_size = phdr.p_filesz as usize;

        // 将段数据从文件复制到物理内存
        if file_size > 0 {
            let data_to_copy = if file_offset + file_size <= program.len() {
                &program[file_offset..(file_offset + file_size)]
            } else {
                println!("警告: 段数据超出文件范围");
                &program[file_offset..program.len()]
            };

            segment.write_bytes(0, data_to_copy).unwrap();
            println!(
                "写入段数据到物理内存成功, 大小: {} 字节",
                data_to_copy.len()
            );
        }

        // 映射物理内存到虚拟地址空间
        println!(
            "映射虚拟地址范围: 0x{:x} - 0x{:x}",
            vaddr_start, aligned_end
        );

        let mut cursor = vm_space.cursor_mut(&(vaddr_start..aligned_end)).unwrap();
        for frame in segment {
            cursor.map(frame.into(), page_prop);
        }
    }

    println!("虚拟内存空间创建完成");
    (entry_point, vm_space)
}

fn create_user_task(
    vm_space: Arc<VmSpace>,
    entry_point: usize,
    sleep_secs: u64,
) -> (Arc<Task>, impl Future + 'static + Send) {
    println!("开始创建用户任务");

    // 创建用户上下文，使用ELF的真实入口点
    let user_ctx = create_user_context(entry_point);
    println!(
        "创建用户上下文完成，入口点: 0x{:x}",
        user_ctx.instruction_pointer()
    );
    let user_space = Arc::new(UserSpace::new(Arc::clone(&vm_space), user_ctx));
    // 定义一个包含vm_space的结构体，用于任务数据
    struct TaskData {
        vm_space: Arc<VmSpace>,
    }

    // 创建任务闭包，避免捕获外部的vm_space

    // 创建任务数据
    let task_data = TaskData { vm_space };

    // Kernel tasks are managed by the Framework,
    // while scheduling algorithms for them can be
    // determined by the users of the Framework.
    println!("构建用户任务");
    let task = Arc::new(
        TaskOptions::new()
            .data(task_data)
            .user_space(Some(user_space))
            .build(),
    );

    let current = task.clone();
    let task_future = async move {
        {
            let disable_preempt = disable_preempt();
            let cpu_id = disable_preempt.current_cpu().as_usize();
            println!("用户任务开始在CPU {} 上执行", cpu_id);
        }
        let task_data = current.data().downcast_ref::<TaskData>().unwrap();

        // 测试sleep
        println!("测试并开始sleep");
        ostd::task::sleep(ostd::task::Duration::from_secs(sleep_secs)).await;
        println!("sleep完成");
        // vm_space_clone.print_page_table(false);
        let user_space = current.user_space().unwrap();
        let mut user_mode = UserMode::new(user_space);
        println!("创建用户模式完成，准备进入用户空间");

        loop {
            // The execute method returns when system
            // calls or CPU exceptions occur or some
            // events specified by the kernel occur.
            println!("准备切换到用户空间执行");
            let return_reason = user_mode.execute(|| false).await;
            println!("从用户空间返回，原因: {:?}", return_reason);

            // The CPU registers of the user space
            // can be accessed and manipulated via
            // the `UserContext` abstraction.
            let user_context = user_mode.context_mut();
            if ReturnReason::UserSyscall == return_reason {
                println!("处理系统调用，系统调用号: {}", user_context.a7());
                handle_syscall(user_context, &task_data.vm_space);
            }
        }
    };
    (task, task_future)
}

fn create_user_context(entry_point: usize) -> UserContext {
    println!("开始创建用户上下文");
    // The user-space CPU states can be initialized
    // to arbitrary values via the `UserContext`
    // abstraction.
    let mut user_ctx = UserContext::default();

    // 使用从ELF文件解析出的入口点地址
    user_ctx.set_instruction_pointer(entry_point);
    println!("设置用户上下文入口点: 0x{:x}", entry_point);
    user_ctx
}

fn handle_syscall(user_context: &mut UserContext, vm_space: &VmSpace) {
    const SYS_WRITE: usize = 64;
    const SYS_EXIT: usize = 93;

    // RISC-V的系统调用号放在a7寄存器中
    match user_context.a7() {
        SYS_WRITE => {
            // RISC-V，系统调用参数放在a0-a6寄存器中
            let (fd, buf_addr, buf_len) = (user_context.a0(), user_context.a1(), user_context.a2());
            println!(
                "处理write系统调用: fd={}, buf_addr=0x{:x}, buf_len={}",
                fd, buf_addr, buf_len
            );
            let buf = {
                let mut buf = vec![0u8; buf_len];
                // Copy data from the user space without
                // unsafe pointer dereferencing.
                let mut reader = vm_space.reader(buf_addr, buf_len).unwrap();
                reader
                    .read_fallible(&mut VmWriter::from(&mut buf as &mut [u8]))
                    .unwrap();
                println!("从用户空间读取数据成功，长度: {}", buf_len);
                buf
            };
            // Use the console for output safely.
            let content = str::from_utf8(&buf).unwrap();
            println!("用户程序输出: {}", content);
            // Manipulate the user-space CPU registers safely.
            user_context.set_a0(buf_len);
            println!("write系统调用处理完成，返回值: {}", buf_len);
        }
        SYS_EXIT => {
            let exit_code = user_context.a0();
            println!("处理exit系统调用，退出码: {}", exit_code);
            exit_qemu(QemuExitCode::Success);
        }
        syscall_num => {
            println!("未实现的系统调用: {}", syscall_num);
            unimplemented!();
        }
    }
}
