// SPDX-License-Identifier: MPL-2.0

#![no_std]

pub use error_stack;

use alloc::{fmt, format};

extern crate alloc;
pub type Result<T> = error_stack::Result<T, Error>;

/// Error number.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, num_enum::TryFromPrimitive)]
pub enum Errno {
    EPERM = 1,    /* Operation not permitted */
    ENOENT = 2,   /* No such file or directory */
    ESRCH = 3,    /* No such process */
    EINTR = 4,    /* Interrupted system call */
    EIO = 5,      /* I/O error */
    ENXIO = 6,    /* No such device or address */
    E2BIG = 7,    /* Argument list too long */
    ENOEXEC = 8,  /* Exec format error */
    EBADF = 9,    /* Bad file number */
    ECHILD = 10,  /* No child processes */
    EAGAIN = 11,  /* Try again */
    ENOMEM = 12,  /* Out of memory */
    EACCES = 13,  /* Permission denied */
    EFAULT = 14,  /* Bad address */
    ENOTBLK = 15, /* Block device required */
    EBUSY = 16,   /* Device or resource busy */
    EEXIST = 17,  /* File exists */
    EXDEV = 18,   /* Cross-device link */
    ENODEV = 19,  /* No such device */
    ENOTDIR = 20, /* Not a directory */
    EISDIR = 21,  /* Is a directory */
    EINVAL = 22,  /* Invalid argument */
    ENFILE = 23,  /* File table overflow */
    EMFILE = 24,  /* Too many open files */
    ENOTTY = 25,  /* Not a typewriter */
    ETXTBSY = 26, /* Text file busy */
    EFBIG = 27,   /* File too large */
    ENOSPC = 28,  /* No space left on device */
    ESPIPE = 29,  /* Illegal seek */
    EROFS = 30,   /* Read-only file system */
    EMLINK = 31,  /* Too many links */
    EPIPE = 32,   /* Broken pipe */
    EDOM = 33,    /* Math argument out of domain of func */
    ERANGE = 34,  /* Math result not representable */

    EDEADLK = 35,      /* Resource deadlock would occur */
    ENAMETOOLONG = 36, /* File name too long */
    ENOLCK = 37,       /* No record locks available */
    /*
     * This error code is special: arch syscall entry code will return
     * -ENOSYS if users try to call a syscall that doesn't exist.  To keep
     * failures of syscalls that really do exist distinguishable from
     * failures due to attempts to use a nonexistent syscall, syscall
     * implementations should refrain from returning -ENOSYS.
     */
    ENOSYS = 38,    /* Invalid system call number */
    ENOTEMPTY = 39, /* Directory not empty */
    ELOOP = 40,     /* Too many symbolic links encountered */
    // EWOULDBLOCK	EAGAIN	/* Operation would block */
    ENOMSG = 42,   /* No message of desired type */
    EIDRM = 43,    /* Identifier removed */
    ECHRNG = 44,   /* Channel number out of range */
    EL2NSYNC = 45, /* Level 2 not synchronized */
    EL3HLT = 46,   /* Level 3 halted */
    EL3RST = 47,   /* Level 3 reset */
    ELNRNG = 48,   /* Link number out of range */
    EUNATCH = 49,  /* Protocol driver not attached */
    ENOCSI = 50,   /* No CSI structure available */
    EL2HLT = 51,   /* Level 2 halted */
    EBADE = 52,    /* Invalid exchange */
    EBADR = 53,    /* Invalid request descriptor */
    EXFULL = 54,   /* Exchange full */
    ENOANO = 55,   /* No anode */
    EBADRQC = 56,  /* Invalid request code */
    EBADSLT = 57,  /* Invalid slot */
    // EDEADLOCK	EDEADLK
    EBFONT = 59,          /* Bad font file format */
    ENOSTR = 60,          /* Device not a stream */
    ENODATA = 61,         /* No data available */
    ETIME = 62,           /* Timer expired */
    ENOSR = 63,           /* Out of streams resources */
    ENONET = 64,          /* Machine is not on the network */
    ENOPKG = 65,          /* Package not installed */
    EREMOTE = 66,         /* Object is remote */
    ENOLINK = 67,         /* Link has been severed */
    EADV = 68,            /* Advertise error */
    ESRMNT = 69,          /* Srmount error */
    ECOMM = 70,           /* Communication error on send */
    EPROTO = 71,          /* Protocol error */
    EMULTIHOP = 72,       /* Multihop attempted */
    EDOTDOT = 73,         /* RFS specific error */
    EBADMSG = 74,         /* Not a data message */
    EOVERFLOW = 75,       /* Value too large for defined data type */
    ENOTUNIQ = 76,        /* Name not unique on network */
    EBADFD = 77,          /* File descriptor in bad state */
    EREMCHG = 78,         /* Remote address changed */
    ELIBACC = 79,         /* Can not access a needed shared library */
    ELIBBAD = 80,         /* Accessing a corrupted shared library */
    ELIBSCN = 81,         /* .lib section in a.out corrupted */
    ELIBMAX = 82,         /* Attempting to link in too many shared libraries */
    ELIBEXEC = 83,        /* Cannot exec a shared library directly */
    EILSEQ = 84,          /* Illegal byte sequence */
    ERESTART = 85,        /* Interrupted system call should be restarted */
    ESTRPIPE = 86,        /* Streams pipe error */
    EUSERS = 87,          /* Too many users */
    ENOTSOCK = 88,        /* Socket operation on non-socket */
    EDESTADDRREQ = 89,    /* Destination address required */
    EMSGSIZE = 90,        /* Message too long */
    EPROTOTYPE = 91,      /* Protocol wrong type for socket */
    ENOPROTOOPT = 92,     /* Protocol not available */
    EPROTONOSUPPORT = 93, /* Protocol not supported */
    ESOCKTNOSUPPORT = 94, /* Socket type not supported */
    EOPNOTSUPP = 95,      /* Operation not supported on transport endpoint */
    EPFNOSUPPORT = 96,    /* Protocol family not supported */
    EAFNOSUPPORT = 97,    /* Address family not supported by protocol */
    EADDRINUSE = 98,      /* Address already in use */
    EADDRNOTAVAIL = 99,   /* Cannot assign requested address */
    ENETDOWN = 100,       /* Network is down */
    ENETUNREACH = 101,    /* Network is unreachable */
    ENETRESET = 102,      /* Network dropped connection because of reset */
    ECONNABORTED = 103,   /* Software caused connection abort */
    ECONNRESET = 104,     /* Connection reset by peer */
    ENOBUFS = 105,        /* No buffer space available */
    EISCONN = 106,        /* Transport endpoint is already connected */
    ENOTCONN = 107,       /* Transport endpoint is not connected */
    ESHUTDOWN = 108,      /* Cannot send after transport endpoint shutdown */
    ETOOMANYREFS = 109,   /* Too many references: cannot splice */
    ETIMEDOUT = 110,      /* Connection timed out */
    ECONNREFUSED = 111,   /* Connection refused */
    EHOSTDOWN = 112,      /* Host is down */
    EHOSTUNREACH = 113,   /* No route to host */
    EALREADY = 114,       /* Operation already in progress */
    EINPROGRESS = 115,    /* Operation now in progress */
    ESTALE = 116,         /* Stale file handle */
    EUCLEAN = 117,        /* Structure needs cleaning */
    ENOTNAM = 118,        /* Not a XENIX named type file */
    ENAVAIL = 119,        /* No XENIX semaphores available */
    EISNAM = 120,         /* Is a named type file */
    EREMOTEIO = 121,      /* Remote I/O error */
    EDQUOT = 122,         /* Quota exceeded */
    ENOMEDIUM = 123,      /* No medium found */
    EMEDIUMTYPE = 124,    /* Wrong medium type */
    ECANCELED = 125,      /* Operation Canceled */
    ENOKEY = 126,         /* Required key not available */
    EKEYEXPIRED = 127,    /* Key has expired */
    EKEYREVOKED = 128,    /* Key has been revoked */
    EKEYREJECTED = 129,   /* Key was rejected by service */
    /* for robust mutexes */
    EOWNERDEAD = 130,      /* Owner died */
    ENOTRECOVERABLE = 131, /* State not recoverable */

    ERFKILL = 132, /* Operation not possible due to RF-kill */

    EHWPOISON = 133, /* Memory page has hardware error */

    ERESTARTSYS = 512, /* Restart of an interrupted system call. For kernel internal use only. */
}

impl fmt::Display for Errno {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let description = match self {
            Errno::EPERM => "Operation not permitted",
            Errno::ENOENT => "No such file or directory",
            Errno::ESRCH => "No such process",
            Errno::EINTR => "Interrupted system call",
            Errno::EIO => "I/O error",
            Errno::ENXIO => "No such device or address",
            Errno::E2BIG => "Argument list too long",
            Errno::ENOEXEC => "Exec format error",
            Errno::EBADF => "Bad file number",
            Errno::ECHILD => "No child processes",
            Errno::EAGAIN => "Try again",
            Errno::ENOMEM => "Out of memory",
            Errno::EACCES => "Permission denied",
            Errno::EFAULT => "Bad address",
            Errno::ENOTBLK => "Block device required",
            Errno::EBUSY => "Device or resource busy",
            Errno::EEXIST => "File exists",
            Errno::EXDEV => "Cross-device link",
            Errno::ENODEV => "No such device",
            Errno::ENOTDIR => "Not a directory",
            Errno::EISDIR => "Is a directory",
            Errno::EINVAL => "Invalid argument",
            Errno::ENFILE => "File table overflow",
            Errno::EMFILE => "Too many open files",
            Errno::ENOTTY => "Not a typewriter",
            Errno::ETXTBSY => "Text file busy",
            Errno::EFBIG => "File too large",
            Errno::ENOSPC => "No space left on device",
            Errno::ESPIPE => "Illegal seek",
            Errno::EROFS => "Read-only file system",
            Errno::EMLINK => "Too many links",
            Errno::EPIPE => "Broken pipe",
            Errno::EDOM => "Math argument out of domain of func",
            Errno::ERANGE => "Math result not representable",
            Errno::EDEADLK => "Resource deadlock would occur",
            Errno::ENAMETOOLONG => "File name too long",
            Errno::ENOLCK => "No record locks available",
            Errno::ENOSYS => "Invalid system call number",
            Errno::ENOTEMPTY => "Directory not empty",
            Errno::ELOOP => "Too many symbolic links encountered",
            Errno::ENOMSG => "No message of desired type",
            Errno::EIDRM => "Identifier removed",
            Errno::ECHRNG => "Channel number out of range",
            Errno::EL2NSYNC => "Level 2 not synchronized",
            Errno::EL3HLT => "Level 3 halted",
            Errno::EL3RST => "Level 3 reset",
            Errno::ELNRNG => "Link number out of range",
            Errno::EUNATCH => "Protocol driver not attached",
            Errno::ENOCSI => "No CSI structure available",
            Errno::EL2HLT => "Level 2 halted",
            Errno::EBADE => "Invalid exchange",
            Errno::EBADR => "Invalid request descriptor",
            Errno::EXFULL => "Exchange full",
            Errno::ENOANO => "No anode",
            Errno::EBADRQC => "Invalid request code",
            Errno::EBADSLT => "Invalid slot",
            Errno::EBFONT => "Bad font file format",
            Errno::ENOSTR => "Device not a stream",
            Errno::ENODATA => "No data available",
            Errno::ETIME => "Timer expired",
            Errno::ENOSR => "Out of streams resources",
            Errno::ENONET => "Machine is not on the network",
            Errno::ENOPKG => "Package not installed",
            Errno::EREMOTE => "Object is remote",
            Errno::ENOLINK => "Link has been severed",
            Errno::EADV => "Advertise error",
            Errno::ESRMNT => "Srmount error",
            Errno::ECOMM => "Communication error on send",
            Errno::EPROTO => "Protocol error",
            Errno::EMULTIHOP => "Multihop attempted",
            Errno::EDOTDOT => "RFS specific error",
            Errno::EBADMSG => "Not a data message",
            Errno::EOVERFLOW => "Value too large for defined data type",
            Errno::ENOTUNIQ => "Name not unique on network",
            Errno::EBADFD => "File descriptor in bad state",
            Errno::EREMCHG => "Remote address changed",
            Errno::ELIBACC => "Can not access a needed shared library",
            Errno::ELIBBAD => "Accessing a corrupted shared library",
            Errno::ELIBSCN => ".lib section in a.out corrupted",
            Errno::ELIBMAX => "Attempting to link in too many shared libraries",
            Errno::ELIBEXEC => "Cannot exec a shared library directly",
            Errno::EILSEQ => "Illegal byte sequence",
            Errno::ERESTART => "Interrupted system call should be restarted",
            Errno::ESTRPIPE => "Streams pipe error",
            Errno::EUSERS => "Too many users",
            Errno::ENOTSOCK => "Socket operation on non-socket",
            Errno::EDESTADDRREQ => "Destination address required",
            Errno::EMSGSIZE => "Message too long",
            Errno::EPROTOTYPE => "Protocol wrong type for socket",
            Errno::ENOPROTOOPT => "Protocol not available",
            Errno::EPROTONOSUPPORT => "Protocol not supported",
            Errno::ESOCKTNOSUPPORT => "Socket type not supported",
            Errno::EOPNOTSUPP => "Operation not supported on transport endpoint",
            Errno::EPFNOSUPPORT => "Protocol family not supported",
            Errno::EAFNOSUPPORT => "Address family not supported by protocol",
            Errno::EADDRINUSE => "Address already in use",
            Errno::EADDRNOTAVAIL => "Cannot assign requested address",
            Errno::ENETDOWN => "Network is down",
            Errno::ENETUNREACH => "Network is unreachable",
            Errno::ENETRESET => "Network dropped connection because of reset",
            Errno::ECONNABORTED => "Software caused connection abort",
            Errno::ECONNRESET => "Connection reset by peer",
            Errno::ENOBUFS => "No buffer space available",
            Errno::EISCONN => "Transport endpoint is already connected",
            Errno::ENOTCONN => "Transport endpoint is not connected",
            Errno::ESHUTDOWN => "Cannot send after transport endpoint shutdown",
            Errno::ETOOMANYREFS => "Too many references: cannot splice",
            Errno::ETIMEDOUT => "Connection timed out",
            Errno::ECONNREFUSED => "Connection refused",
            Errno::EHOSTDOWN => "Host is down",
            Errno::EHOSTUNREACH => "No route to host",
            Errno::EALREADY => "Operation already in progress",
            Errno::EINPROGRESS => "Operation now in progress",
            Errno::ESTALE => "Stale file handle",
            Errno::EUCLEAN => "Structure needs cleaning",
            Errno::ENOTNAM => "Not a XENIX named type file",
            Errno::ENAVAIL => "No XENIX semaphores available",
            Errno::EISNAM => "Is a named type file",
            Errno::EREMOTEIO => "Remote I/O error",
            Errno::EDQUOT => "Quota exceeded",
            Errno::ENOMEDIUM => "No medium found",
            Errno::EMEDIUMTYPE => "Wrong medium type",
            Errno::ECANCELED => "Operation Canceled",
            Errno::ENOKEY => "Required key not available",
            Errno::EKEYEXPIRED => "Key has expired",
            Errno::EKEYREVOKED => "Key has been revoked",
            Errno::EKEYREJECTED => "Key was rejected by service",
            Errno::EOWNERDEAD => "Owner died",
            Errno::ENOTRECOVERABLE => "State not recoverable",
            Errno::ERFKILL => "Operation not possible due to RF-kill",
            Errno::EHWPOISON => "Memory page has hardware error",
            Errno::ERESTARTSYS => "Restart of an interrupted system call. For kernel internal use only.",
        };
        // 使用 {:?} 来获取枚举成员的名称 (例如 "EPERM")
        // 使用 *self as i32 来获取其对应的整数值
        write!(f, "{:?} ({}) : {}", self, *self as i32, description)
    }
}

/// error used in this crate
#[derive(Debug, Clone, Copy)]
pub struct Error {
    errno: Errno,
    msg: Option<&'static str>,
}

impl Error {
    pub const fn new(errno: Errno) -> Self {
        Error { errno, msg: None }
    }

    pub const fn with_message(errno: Errno, msg: &'static str) -> Self {
        Error {
            errno,
            msg: Some(msg),
        }
    }

    pub const fn error(&self) -> Errno {
        self.errno
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(msg) = self.msg {
            write!(f, "{}: {}", self.errno, msg)
        } else {
            write!(f, "{}", self.errno)
        }
    }
}

impl From<Errno> for Error {
    fn from(errno: Errno) -> Self {
        Error::new(errno)
    }
}

impl AsRef<Error> for Error {
    fn as_ref(&self) -> &Error {
        self
    }
}

impl From<ostd::Error> for Error {
    fn from(ostd_error: ostd::Error) -> Self {
        match ostd_error {
            ostd::Error::AccessDenied => Error::new(Errno::EFAULT),
            ostd::Error::NoMemory => Error::new(Errno::ENOMEM),
            ostd::Error::InvalidArgs => Error::new(Errno::EINVAL),
            ostd::Error::IoError => Error::new(Errno::EIO),
            ostd::Error::NotEnoughResources => Error::new(Errno::EBUSY),
            ostd::Error::PageFault => Error::new(Errno::EFAULT),
            ostd::Error::Overflow => Error::new(Errno::EOVERFLOW),
            ostd::Error::MapAlreadyMappedVaddr => Error::new(Errno::EINVAL),
            ostd::Error::KVirtAreaAllocError => Error::new(Errno::ENOMEM),
        }
    }
}

#[track_caller]
pub fn ostd_error_to_errno(ostd_error: error_stack::Report<ostd::Error>) -> error_stack::Report<Error> {
    let error = ostd_error.downcast_ref::<ostd::Error>().expect("Can not convert ostd::Error to Error");
    let error = (*error).into();
    ostd_error.change_context(error)
}

#[track_caller]
pub fn errno_to_ostd_error(errno: error_stack::Report<Error>) -> error_stack::Report<ostd::Error> {
    let error = errno.downcast_ref::<Error>().expect("Can not convert Error to ostd::Error");
    let error = match error.errno {
        Errno::EFAULT => ostd::Error::AccessDenied,
        Errno::ENOMEM => ostd::Error::NoMemory,
        Errno::EINVAL => ostd::Error::InvalidArgs,
        Errno::EIO => ostd::Error::IoError,
        Errno::EBUSY => ostd::Error::NotEnoughResources,
        Errno::EOVERFLOW => ostd::Error::Overflow,
        _ => ostd::Error::InvalidArgs,
    };
    errno.change_context(error)
}

#[track_caller]
pub fn ostd_tuple_to_errno(ostd_tuple: (ostd::Error, usize)) -> error_stack::Report<Error> {
    let (ostd_error, _) = ostd_tuple;
    let error: Error = ostd_error.into();
    error_stack::Report::new(error).attach_printable(format!("ostd_tuple: {:?}", ostd_tuple))
}

impl From<(ostd::Error, usize)> for Error {
    // Used in fallible memory read/write API
    fn from(ostd_error: (ostd::Error, usize)) -> Self {
        Error::from(ostd_error.0)
    }
}

// impl From<aster_block::bio::BioEnqueueError> for Error {
//     fn from(error: aster_block::bio::BioEnqueueError) -> Self {
//         match error {
//             aster_block::bio::BioEnqueueError::IsFull => {
//                 Error::with_message(Errno::EBUSY, "The request queue is full")
//             }
//             aster_block::bio::BioEnqueueError::Refused => {
//                 Error::with_message(Errno::EBUSY, "Refuse to enqueue the bio")
//             }
//             aster_block::bio::BioEnqueueError::TooBig => {
//                 Error::with_message(Errno::EINVAL, "Bio is too big")
//             }
//         }
//     }
// }

// impl From<aster_block::bio::BioStatus> for Error {
//     fn from(err_status: aster_block::bio::BioStatus) -> Self {
//         match err_status {
//             aster_block::bio::BioStatus::NotSupported => {
//                 Error::with_message(Errno::EIO, "I/O operation is not supported")
//             }
//             aster_block::bio::BioStatus::NoSpace => {
//                 Error::with_message(Errno::ENOSPC, "Insufficient space on device")
//             }
//             aster_block::bio::BioStatus::IoError => {
//                 Error::with_message(Errno::EIO, "I/O operation fails")
//             }
//             status => panic!("Can not convert the status: {:?} to an error", status),
//         }
//     }
// }

impl From<core::num::TryFromIntError> for Error {
    fn from(_: core::num::TryFromIntError) -> Self {
        Error::with_message(Errno::EINVAL, "Invalid integer")
    }
}

impl From<core::str::Utf8Error> for Error {
    fn from(_: core::str::Utf8Error) -> Self {
        Error::with_message(Errno::EINVAL, "Invalid utf-8 string")
    }
}

impl From<alloc::string::FromUtf8Error> for Error {
    fn from(_: alloc::string::FromUtf8Error) -> Self {
        Error::with_message(Errno::EINVAL, "Invalid utf-8 string")
    }
}

impl From<core::ffi::FromBytesUntilNulError> for Error {
    fn from(_: core::ffi::FromBytesUntilNulError) -> Self {
        Error::with_message(Errno::E2BIG, "Cannot find null in cstring")
    }
}

impl From<core::ffi::FromBytesWithNulError> for Error {
    fn from(_: core::ffi::FromBytesWithNulError) -> Self {
        Error::with_message(Errno::E2BIG, "Cannot find null in cstring")
    }
}

// impl From<cpio_decoder::error::Error> for Error {
//     fn from(cpio_error: cpio_decoder::error::Error) -> Self {
//         match cpio_error {
//             cpio_decoder::error::Error::MagicError => {
//                 Error::with_message(Errno::EINVAL, "CPIO invalid magic number")
//             }
//             cpio_decoder::error::Error::Utf8Error => {
//                 Error::with_message(Errno::EINVAL, "CPIO invalid utf-8 string")
//             }
//             cpio_decoder::error::Error::ParseIntError => {
//                 Error::with_message(Errno::EINVAL, "CPIO parse int error")
//             }
//             cpio_decoder::error::Error::FileTypeError => {
//                 Error::with_message(Errno::EINVAL, "CPIO invalid file type")
//             }
//             cpio_decoder::error::Error::FileNameError => {
//                 Error::with_message(Errno::EINVAL, "CPIO invalid file name")
//             }
//             cpio_decoder::error::Error::BufferShortError => {
//                 Error::with_message(Errno::EINVAL, "CPIO buffer is too short")
//             }
//             cpio_decoder::error::Error::IoError => {
//                 Error::with_message(Errno::EIO, "CPIO buffer I/O error")
//             }
//         }
//     }
// }

impl From<Error> for ostd::Error {
    fn from(error: Error) -> Self {
        match error.errno {
            Errno::EACCES => ostd::Error::AccessDenied,
            Errno::EIO => ostd::Error::IoError,
            Errno::ENOMEM => ostd::Error::NoMemory,
            Errno::EFAULT => ostd::Error::PageFault,
            Errno::EINVAL => ostd::Error::InvalidArgs,
            Errno::EBUSY => ostd::Error::NotEnoughResources,
            _ => ostd::Error::InvalidArgs,
        }
    }
}

impl From<alloc::ffi::NulError> for Error {
    fn from(_: alloc::ffi::NulError) -> Self {
        Error::with_message(Errno::E2BIG, "Cannot find null in cstring")
    }
}

// impl From<int_to_c_enum::TryFromIntError> for Error {
//     fn from(_: int_to_c_enum::TryFromIntError) -> Self {
//         Error::with_message(Errno::EINVAL, "Invalid enum value")
//     }
// }

#[macro_export]
macro_rules! return_errno {
    ($errno: expr) => {
        return Err($crate::error_stack::Report::new($crate::Error::new($errno)))
    };
}

#[macro_export]
macro_rules! return_errno_with_message {
    ($errno: expr, $message: expr) => {
        return Err($crate::error_stack::Report::new(
            $crate::Error::with_message($errno, $message)
        ))
    };
}

#[track_caller]
pub fn errno_with_message(errno: Errno, message: &'static str) -> error_stack::Report<Error> {
    error_stack::Report::new(Error::with_message(errno, message))
}

impl error_stack::Context for Error {}

/// 在字符串(String)中添加当前代码位置信息
#[macro_export]
macro_rules! with_pos {
    ($msg: expr) => {
        {
            use alloc::{format, string::ToString};
            let msg = $msg.to_string();
            format!("{}:{}:{}: {}", file!(), line!(), column!(), msg)
        }
    };
}

/// 获取当前代码位置信息
#[macro_export]
macro_rules! pos {
    () => {
        format!("{}:{}:{}", file!(), line!(), column!())
    };
}