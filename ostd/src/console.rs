// SPDX-License-Identifier: MPL-2.0

//! Console output.

use core::fmt::{self, Arguments, Write};

use crate::bus::device_manager::DEVICE_MANAGER;

/// 早期控制台：直接走架构串口，适用于内存/总线初始化之前。
pub fn early_print(args: Arguments) {
    crate::arch::serial::print(args);
}

/// Prints to the console.
#[macro_export]
macro_rules! early_print {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::early_print(format_args!($fmt $(, $($arg)+)?))
    }
}

/// Prints to the console with a newline.
#[macro_export]
macro_rules! early_println {
    () => { $crate::early_print!("\n") };
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::early_print(format_args!(concat!($fmt, "\n") $(, $($arg)+)?))
    }
}

/// 通过 VirtIO Console 打印（若不存在则回退到 early_print）。
pub fn print(args: Arguments) {
    if let Some(console) = DEVICE_MANAGER.lock().console_device() {
        let mut guard = console.lock();
        let _ = core::fmt::Write::write_fmt(&mut *guard, args);
    } else {
        early_print(args);
    }
}

/// 打印一行（末尾自动添加换行）。
pub fn println(args: Arguments) {
    print(format_args!("{}\n", args));
}
