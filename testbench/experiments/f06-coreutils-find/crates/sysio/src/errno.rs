//! Números de errno do Linux usados pelo shim. Viram `io::Error::from_raw_os_error`, então o texto
//! ("No such file or directory") e o `ErrorKind` saem iguais aos do std sobre o kernel real.

pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EIO: i32 = 5;
pub const EBADF: i32 = 9;
pub const EACCES: i32 = 13;
pub const EEXIST: i32 = 17;
pub const EXDEV: i32 = 18;
pub const ENOTDIR: i32 = 20;
pub const EISDIR: i32 = 21;
pub const EINVAL: i32 = 22;
pub const ESPIPE: i32 = 29;
pub const ENAMETOOLONG: i32 = 36;
pub const ENOTEMPTY: i32 = 39;
pub const ELOOP: i32 = 40;

/// Bits de tipo do `st_mode`.
pub const S_IFMT: u32 = 0o170000;
pub const S_IFSOCK: u32 = 0o140000;
pub const S_IFLNK: u32 = 0o120000;
pub const S_IFREG: u32 = 0o100000;
pub const S_IFBLK: u32 = 0o060000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFCHR: u32 = 0o020000;
pub const S_IFIFO: u32 = 0o010000;

pub fn err(code: i32) -> std::io::Error {
    std::io::Error::from_raw_os_error(code)
}
