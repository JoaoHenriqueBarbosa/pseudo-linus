//! Errno do Linux pra quem porta código que compara `raw_os_error()` com constantes da libc, e as
//! conversões entre [`sysabi::Errno`] e `std::io::Error`.
//!
//! A mensagem de um erro sempre sai de [`strerror`] (a tabela da glibc 2.41 do `sysabi`), nunca do
//! `Display` do `std::io::Error`, que acrescenta " (os error N)" e consulta o `strerror` do host.

use std::io;

use sysabi::Errno;

pub const EPERM: i32 = Errno::EPERM.0;
pub const ENOENT: i32 = Errno::ENOENT.0;
pub const ESRCH: i32 = Errno::ESRCH.0;
pub const EINTR: i32 = Errno::EINTR.0;
pub const EIO: i32 = Errno::EIO.0;
pub const ENXIO: i32 = Errno::ENXIO.0;
pub const E2BIG: i32 = Errno::E2BIG.0;
pub const ENOEXEC: i32 = Errno::ENOEXEC.0;
pub const EBADF: i32 = Errno::EBADF.0;
pub const ECHILD: i32 = Errno::ECHILD.0;
pub const EAGAIN: i32 = Errno::EAGAIN.0;
pub const ENOMEM: i32 = Errno::ENOMEM.0;
pub const EACCES: i32 = Errno::EACCES.0;
pub const EFAULT: i32 = Errno::EFAULT.0;
pub const EBUSY: i32 = Errno::EBUSY.0;
pub const EEXIST: i32 = Errno::EEXIST.0;
pub const EXDEV: i32 = Errno::EXDEV.0;
pub const ENODEV: i32 = Errno::ENODEV.0;
pub const ENOTDIR: i32 = Errno::ENOTDIR.0;
pub const EISDIR: i32 = Errno::EISDIR.0;
pub const EINVAL: i32 = Errno::EINVAL.0;
pub const ENFILE: i32 = Errno::ENFILE.0;
pub const EMFILE: i32 = Errno::EMFILE.0;
pub const ENOTTY: i32 = Errno::ENOTTY.0;
pub const ETXTBSY: i32 = Errno::ETXTBSY.0;
pub const EFBIG: i32 = Errno::EFBIG.0;
pub const ENOSPC: i32 = Errno::ENOSPC.0;
pub const ESPIPE: i32 = Errno::ESPIPE.0;
pub const EROFS: i32 = Errno::EROFS.0;
pub const EMLINK: i32 = Errno::EMLINK.0;
pub const EPIPE: i32 = Errno::EPIPE.0;
pub const EDOM: i32 = Errno::EDOM.0;
pub const ERANGE: i32 = Errno::ERANGE.0;
pub const ENAMETOOLONG: i32 = Errno::ENAMETOOLONG.0;
pub const ENOSYS: i32 = Errno::ENOSYS.0;
pub const ENOTEMPTY: i32 = Errno::ENOTEMPTY.0;
pub const ELOOP: i32 = Errno::ELOOP.0;
pub const ENODATA: i32 = Errno::ENODATA.0;
pub const EOVERFLOW: i32 = Errno::EOVERFLOW.0;
pub const EILSEQ: i32 = Errno::EILSEQ.0;
pub const ENOTSUP: i32 = Errno::ENOTSUP.0;
/// No Linux é o mesmo número de `ENOTSUP` (a tabela do sysabi só guarda um nome por número).
pub const EOPNOTSUPP: i32 = Errno::ENOTSUP.0;
pub const ENETUNREACH: i32 = Errno::ENETUNREACH.0;
pub const ETIMEDOUT: i32 = Errno::ETIMEDOUT.0;

/// Bits de tipo do `st_mode` (mantidos aqui pelos portes que já os importavam deste módulo).
pub use sysabi::mode::{S_IFBLK, S_IFCHR, S_IFDIR, S_IFIFO, S_IFLNK, S_IFMT, S_IFREG, S_IFSOCK};

/// `sysabi::Errno` vira `io::Error` preservando o número.
pub fn from_errno(e: Errno) -> io::Error {
    io::Error::from_raw_os_error(e.0)
}

/// Converte o resultado de uma syscall.
pub fn cvt<T>(r: sysabi::SysResult<T>) -> io::Result<T> {
    r.map_err(from_errno)
}

/// Mensagem do erro como a glibc escreveria (`strerror`), sem o " (os error N)" do std. Erros sem
/// errno (criados com `io::Error::new`) mostram o próprio texto.
pub fn strerror(e: &io::Error) -> String {
    match e.raw_os_error() {
        Some(n) => Errno(n).message(),
        None => match e.get_ref() {
            Some(inner) => inner.to_string(),
            None => Errno::from_io(e).message(),
        },
    }
}
