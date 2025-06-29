use ostd::Pod;

#[repr(C)]
#[derive(Default, Copy, Clone, Pod, Debug)]
pub struct Tms {
    /// 用户态 CPU 时间（嘀嗒）
    pub tms_utime:  u64,
    /// 内核态 CPU 时间（嘀嗒）
    pub tms_stime:  u64,
    /// 已终结子进程用户态时间累计
    pub tms_cutime: u64,
    /// 已终结子进程内核态时间累计
    pub tms_cstime: u64,
}
