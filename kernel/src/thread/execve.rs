use core::ops::ControlFlow;

use alloc::{ffi::CString, vec::Vec};
use vfs::PathSlice;
use crate::{
    thread::{init_stack::{MAX_ARGV_NUMBER, MAX_ARG_LEN, MAX_ENVP_NUMBER, MAX_ENV_LEN}, loader::load_elf_to_vm, ThreadState}, vm::ProcessVm,
};
use nexus_error::{errno_with_message, return_errno_with_message, Errno, Result};
use ostd::{mm::Vaddr, user::UserContextApi};

pub async fn do_execve(state: &mut ThreadState, uc: &mut ostd::cpu::UserContext) -> Result<ControlFlow<i32, Option<isize>>> {
    let [path_ptr, argv_ptr_ptr, envp_ptr_ptr, ..] = uc.syscall_arguments();
    let mut new_path = state.process_vm
        .read_cstring(path_ptr, 4096)?
        .into_string()
        .map_err(|_| errno_with_message(Errno::EINVAL, "path is not a valid C string"))?;
    let argv = read_cstring_vec(argv_ptr_ptr, MAX_ARGV_NUMBER, MAX_ARG_LEN, &state.process_vm)?;
    let envp = read_cstring_vec(envp_ptr_ptr, MAX_ENVP_NUMBER, MAX_ENV_LEN, &state.process_vm)?;

    let path = PathSlice::new(&new_path)?;
    if !path.is_absolute() {
        new_path = state.cwd.as_slice().join(&new_path)?.to_string();
    }

    let elf_info = load_elf_to_vm(&state.process_vm, &new_path, argv, envp).await?;

    // 切换用户上下文
    uc.set_instruction_pointer(elf_info.entry as _);
    uc.set_stack_pointer(elf_info.user_sp as _);

    // FD_CLOEXEC 处理
    state.fd_table.clear_cloexec_on_exec().await;

    Ok(ControlFlow::Continue(None))
}

fn read_cstring_vec(
    array_ptr: Vaddr,
    max_string_number: usize,
    max_string_len: usize,
    vm: &ProcessVm,
) -> Result<Vec<CString>> {
    let mut res = Vec::new();
    // On Linux, argv pointer and envp pointer can be specified as NULL.
    if array_ptr == 0 {
        return Ok(res);
    }
    let mut read_addr = array_ptr;
    let mut find_null = false;
    for _ in 0..max_string_number {
        let cstring_ptr = vm.read_val::<usize>(read_addr)?;
        read_addr += 8;
        // read a null pointer
        if cstring_ptr == 0 {
            find_null = true;
            break;
        }
        let cstring = vm.read_cstring(cstring_ptr, max_string_len)?;
        res.push(cstring);
    }
    if !find_null {
        return_errno_with_message!(Errno::E2BIG, "Cannot find null pointer in vector");
    }
    Ok(res)
}