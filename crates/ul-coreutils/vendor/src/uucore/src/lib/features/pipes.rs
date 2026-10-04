// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

//! Zero-copy-related functions.

// Porte pseudo-linus: o contrato do pseudo-kernel não tem splice, tee nem F_SETPIPE_SZ, e o
// original usava rustix sobre fds do host. A API fica (os utilitários chamam), com `splice` sempre
// falhando com EINVAL e cada função caindo no caminho de leitura e escrita que o original já tinha
// como fallback. As escritas vão direto no fd (sem o buffer do stdout do processo, que é
// descarregado antes quando o destino é o fd 1, pra manter a ordem da saída).
use std::io::{Read, Write};
use sysio::fs::File;
use sysio::os::fd::{AsFd, AsRawFd, FromRawFd};
use sysio::sysabi::{Fd, FdReader, FdWriter, OFlags};

pub const MAX_ROOTLESS_PIPE_SIZE: usize = 1024 * 1024;

/// Leitor sem buffer no fd de `src` (sem tomar posse dele).
pub fn raw_reader(src: &impl AsFd) -> FdReader {
    FdReader(Fd(src.as_fd().as_raw_fd()))
}

/// Escritor sem buffer no fd de `dest` (sem tomar posse dele); se for o stdout, descarrega antes o
/// buffer do stdout do processo.
pub fn raw_writer(dest: &impl AsFd) -> FdWriter {
    let fd = dest.as_fd().as_raw_fd();
    if fd == 1 {
        let _ = sysio::io::flush_stdout();
    }
    FdWriter(Fd(fd))
}

/// Os dois lados de um pipe novo (o "PipeReader"/"PipeWriter" do original).
pub type PipeReader = File;
pub type PipeWriter = File;

/// A type allows to
/// - check that zero-copy succeed by ?.is_ok()
/// - check that zero-copy failed, but read/write fallback succeed by ?.is_err()
/// - catch the read/write fallback's error by ? or let Err(e)
///
/// use rustix::io::Result for functions without read/write fallback
type PipeRes = std::io::Result<Result<(), ()>>;

/// return pipe and try to extend its size
/// SIZE_REQUIRED should be true if you want to fail when changing pipe size failed
/// e.g. writing size to pipe should not hang
/// SIZE_REQUIRED=false allows to continue unbuffered splice I/O with default pipe size even if fcntl failed
///
/// used for resolving the limitation for splice: one of a input or output should be pipe
#[inline]
pub fn pipe<const SIZE_REQUIRED: bool>() -> std::io::Result<(PipeReader, PipeWriter)> {
    let (r, w) = sysio::errno::cvt(sysio::proc::sys().pipe2(OFlags::CLOEXEC))?;
    Ok((File::from_raw_fd(r.0), File::from_raw_fd(w.0)))
}

/// Less noisy wrapper around splice syscall
///
/// Porte pseudo-linus: sem splice no pseudo-kernel; sempre EINVAL (o erro que o Linux dá quando
/// nenhum dos lados aceita splice), pra que o chamador use o fallback.
#[inline]
pub fn splice(_source: &impl AsFd, _target: &impl AsFd, _len: usize) -> std::io::Result<usize> {
    Err(std::io::Error::from_raw_os_error(sysio::errno::EINVAL))
}

/// Move `len` bytes from `pipe` into `dest` (leitura e escrita).
#[inline]
pub fn drain_pipe(pipe: &PipeReader, dest: &impl AsFd, len: usize) -> PipeRes {
    let mut drain = Vec::new();
    drain.try_reserve(len.min(MAX_ROOTLESS_PIPE_SIZE)).map_err(|_| std::io::Error::from_raw_os_error(sysio::errno::ENOMEM))?;
    pipe.take(len as u64).read_to_end(&mut drain)?;
    raw_writer(dest).write_all(&drain)?;
    Ok(Err(()))
}

/// Porte pseudo-linus: sem splice, sempre "zero-copy falhou" (`Ok(Err(()))`); o chamador copia.
#[inline]
pub fn splice_unbounded_auto(_source: &impl AsFd, _dest: &mut impl AsFd) -> PipeRes {
    Ok(Err(()))
}

/// Copy `n` bytes without buffering (no original, splice com fallback de leitura e escrita).
/// return actually sent bytes
#[inline]
pub fn send_n_bytes(input: impl AsFd, target: impl AsFd, n: u64) -> std::io::Result<u64> {
    // remove buffering from this fallback, or order of output would be wrong with multiple input
    std::io::copy(&mut raw_reader(&input).take(n), &mut raw_writer(&target))
}

/// discard `n` bytes by splice
/// Porte pseudo-linus: sem splice, nada é descartado aqui (`Err(0)`); o chamador lê e descarta.
#[inline]
pub fn discard_n_bytes(_fd: impl AsFd, _n: usize) -> Result<usize, usize> {
    Err(0)
}

/// Return verified /dev/null
#[inline]
pub fn dev_null() -> Option<File> {
    use sysio::os::unix::fs::MetadataExt;
    let null = sysio::fs::OpenOptions::new().write(true).open("/dev/null").ok()?;
    let dev = null.metadata().ok()?.rdev();
    ((crate::fs::major(dev), crate::fs::minor(dev)) == (1, 3)).then_some(null)
}

// Less noisy wrapper around tee syscall
/// Porte pseudo-linus: sem tee(2) no pseudo-kernel; sempre EINVAL.
#[inline]
pub fn tee(_source: &impl AsFd, _target: &impl AsFd, _len: usize) -> std::io::Result<usize> {
    Err(std::io::Error::from_raw_os_error(sysio::errno::EINVAL))
}
