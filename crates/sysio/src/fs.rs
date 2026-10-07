//! `std::fs` sobre as syscalls do pseudo-processo.
//!
//! Nomes e assinaturas seguem o std, pra que o porte seja trocar `use std::fs::...` por
//! `use sysio::fs::...`. O que não dá pra imitar por import são os métodos inerentes de
//! `std::path::Path` que tocam o FS do host (`exists()`, `is_dir()`, `metadata()`,
//! `canonicalize()`...): pra eles há as funções livres deste módulo ([`exists`], [`is_dir`]...) e o
//! trait [`crate::path::PathExt`] (`p.sys_exists()`, `p.sys_is_dir()`...).

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sysabi::{AccessMode, AtFlags, Errno, Fd, OFlags, RenameFlags, SetTime, Whence};

pub use sysabi::{Stat, StatFs, TimeSpec};

use crate::errno::{cvt, from_errno};
pub use crate::fd::AsFd as Fstat;
use crate::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use crate::proc;

/// Bytes de um caminho.
pub fn path_bytes(p: &Path) -> &[u8] {
    p.as_os_str().as_bytes()
}

fn pb<P: AsRef<Path> + ?Sized>(p: &P) -> &[u8] {
    p.as_ref().as_os_str().as_bytes()
}

/// `TimeSpec` do sysabi pra `SystemTime`.
pub fn timespec_to_system(t: TimeSpec) -> SystemTime {
    if t.sec >= 0 {
        UNIX_EPOCH + Duration::new(t.sec as u64, t.nsec)
    } else {
        UNIX_EPOCH - Duration::from_secs(t.sec.unsigned_abs()) + Duration::from_nanos(u64::from(t.nsec))
    }
}

/// `SystemTime` pra `TimeSpec` (antes da época dá segundos negativos com nanossegundos positivos).
pub fn system_to_timespec(t: SystemTime) -> TimeSpec {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => TimeSpec { sec: d.as_secs() as i64, nsec: d.subsec_nanos() },
        Err(e) => {
            let d = e.duration();
            let mut sec = -(d.as_secs() as i64);
            let mut nsec = d.subsec_nanos();
            if nsec > 0 {
                sec -= 1;
                nsec = 1_000_000_000 - nsec;
            }
            TimeSpec { sec, nsec }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Metadata, FileType, Permissions, FileTimes

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub(crate) st: Stat,
}

impl Metadata {
    pub fn from_stat(st: Stat) -> Metadata {
        Metadata { st }
    }
    pub fn file_type(&self) -> FileType {
        FileType { mode: self.st.mode }
    }
    pub fn is_dir(&self) -> bool {
        self.file_type().is_dir()
    }
    pub fn is_file(&self) -> bool {
        self.file_type().is_file()
    }
    pub fn is_symlink(&self) -> bool {
        self.file_type().is_symlink()
    }
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> u64 {
        self.st.size
    }
    pub fn permissions(&self) -> Permissions {
        Permissions { mode: self.st.mode & 0o7777 }
    }
    pub fn modified(&self) -> io::Result<SystemTime> {
        Ok(timespec_to_system(self.st.mtime))
    }
    pub fn accessed(&self) -> io::Result<SystemTime> {
        Ok(timespec_to_system(self.st.atime))
    }
    /// `statx` com `STX_BTIME`; sem btime, o mesmo erro que o std dá.
    pub fn created(&self) -> io::Result<SystemTime> {
        match self.st.btime {
            Some(t) => Ok(timespec_to_system(t)),
            None => Err(io::Error::new(io::ErrorKind::Unsupported, "creation time is not available for the filesystem")),
        }
    }
    /// A `struct stat` crua.
    pub fn stat(&self) -> &Stat {
        &self.st
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileType {
    pub(crate) mode: u32,
}

impl FileType {
    pub fn from_mode(mode: u32) -> FileType {
        FileType { mode }
    }
    pub fn is_dir(&self) -> bool {
        self.mode & sysabi::mode::S_IFMT == sysabi::mode::S_IFDIR
    }
    pub fn is_file(&self) -> bool {
        self.mode & sysabi::mode::S_IFMT == sysabi::mode::S_IFREG
    }
    pub fn is_symlink(&self) -> bool {
        self.mode & sysabi::mode::S_IFMT == sysabi::mode::S_IFLNK
    }
    /// Bits de tipo (`S_IFMT`) do `st_mode`.
    pub fn mode(&self) -> u32 {
        self.mode
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permissions {
    pub(crate) mode: u32,
}

impl Permissions {
    pub fn readonly(&self) -> bool {
        self.mode & 0o222 == 0
    }
    pub fn set_readonly(&mut self, ro: bool) {
        if ro {
            self.mode &= !0o222;
        } else {
            self.mode |= 0o222;
        }
    }
}

/// `std::fs::FileTimes`.
#[derive(Clone, Copy, Debug, Default)]
pub struct FileTimes {
    accessed: Option<SystemTime>,
    modified: Option<SystemTime>,
}

impl FileTimes {
    pub fn new() -> FileTimes {
        FileTimes::default()
    }
    pub fn set_accessed(mut self, t: SystemTime) -> Self {
        self.accessed = Some(t);
        self
    }
    pub fn set_modified(mut self, t: SystemTime) -> Self {
        self.modified = Some(t);
        self
    }
}

fn set_time(t: Option<SystemTime>) -> SetTime {
    match t {
        Some(t) => SetTime::At(system_to_timespec(t)),
        None => SetTime::Omit,
    }
}

// ---------------------------------------------------------------------------------------------
// File e OpenOptions

/// Um arquivo aberto (dono do fd).
#[derive(Debug)]
pub struct File {
    fd: OwnedFd,
}

fn read_raw(fd: Fd, buf: &mut [u8]) -> io::Result<usize> {
    let sys = proc::sys();
    loop {
        match sys.read(fd, buf) {
            Err(Errno::EINTR) => continue,
            r => return cvt(r),
        }
    }
}

fn write_raw(fd: Fd, buf: &[u8]) -> io::Result<usize> {
    let sys = proc::sys();
    loop {
        match sys.write(fd, buf) {
            Err(Errno::EINTR) => continue,
            r => return cvt(r),
        }
    }
}

impl File {
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<File> {
        OpenOptions::new().read(true).open(path)
    }

    pub fn create<P: AsRef<Path>>(path: P) -> io::Result<File> {
        OpenOptions::new().write(true).create(true).truncate(true).open(path)
    }

    pub fn create_new<P: AsRef<Path>>(path: P) -> io::Result<File> {
        OpenOptions::new().read(true).write(true).create_new(true).open(path)
    }

    pub fn options() -> OpenOptions {
        OpenOptions::new()
    }

    fn raw(&self) -> Fd {
        self.fd.raw()
    }

    pub fn metadata(&self) -> io::Result<Metadata> {
        cvt(proc::sys().fstat(self.raw())).map(Metadata::from_stat)
    }

    pub fn set_len(&self, size: u64) -> io::Result<()> {
        cvt(proc::sys().ftruncate(self.raw(), size))
    }

    pub fn sync_all(&self) -> io::Result<()> {
        cvt(proc::sys().fsync(self.raw()))
    }

    pub fn sync_data(&self) -> io::Result<()> {
        cvt(proc::sys().fsync(self.raw()))
    }

    /// `dup(2)`: o clone compartilha a posição (mesma open file description).
    pub fn try_clone(&self) -> io::Result<File> {
        Ok(File { fd: self.fd.try_clone()? })
    }

    pub fn set_permissions(&self, perm: Permissions) -> io::Result<()> {
        cvt(proc::sys().fchmod(self.raw(), perm.mode))
    }

    pub fn set_times(&self, times: FileTimes) -> io::Result<()> {
        cvt(proc::sys().futimens(self.raw(), set_time(times.accessed), set_time(times.modified)))
    }

    pub fn set_modified(&self, t: SystemTime) -> io::Result<()> {
        self.set_times(FileTimes::new().set_modified(t))
    }

    /// Identidade do arquivo aberto (`st_ino`).
    pub fn ino(&self) -> u64 {
        self.metadata().map(|m| m.st.ino).unwrap_or(0)
    }

    /// `pread(2)`.
    pub fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        cvt(proc::sys().pread(self.raw(), buf, offset))
    }

    /// `pwrite(2)`.
    pub fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<usize> {
        cvt(proc::sys().pwrite(self.raw(), buf, offset))
    }

    fn do_seek(&self, pos: SeekFrom) -> io::Result<u64> {
        let (off, whence) = match pos {
            SeekFrom::Start(n) => (i64::try_from(n).map_err(|_| from_errno(sysabi::Errno::EINVAL))?, Whence::Set),
            SeekFrom::End(n) => (n, Whence::End),
            SeekFrom::Current(n) => (n, Whence::Cur),
        };
        cvt(proc::sys().lseek(self.raw(), off, whence))
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        read_raw(self.raw(), buf)
    }
}

impl Read for &File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        read_raw(self.raw(), buf)
    }
}

impl Write for File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        write_raw(self.raw(), buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for &File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        write_raw(self.raw(), buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for File {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.do_seek(pos)
    }
}

impl Seek for &File {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.do_seek(pos)
    }
}

impl AsFd for File {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

impl AsRawFd for File {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl FromRawFd for File {
    fn from_raw_fd(fd: RawFd) -> Self {
        File { fd: OwnedFd::from_raw_fd(fd) }
    }
}

impl IntoRawFd for File {
    fn into_raw_fd(self) -> RawFd {
        self.fd.into_raw_fd()
    }
}

impl From<OwnedFd> for File {
    fn from(fd: OwnedFd) -> Self {
        File { fd }
    }
}

impl From<File> for OwnedFd {
    fn from(f: File) -> Self {
        f.fd
    }
}

impl crate::io::IsTerminal for File {
    fn is_terminal(&self) -> bool {
        crate::io::isatty(self.as_raw_fd())
    }
}

#[derive(Clone, Debug)]
pub struct OpenOptions {
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
    mode: u32,
    custom_flags: u32,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenOptions {
    pub fn new() -> OpenOptions {
        OpenOptions { read: false, write: false, append: false, truncate: false, create: false, create_new: false, mode: 0o666, custom_flags: 0 }
    }
    pub fn read(&mut self, v: bool) -> &mut Self {
        self.read = v;
        self
    }
    pub fn write(&mut self, v: bool) -> &mut Self {
        self.write = v;
        self
    }
    pub fn append(&mut self, v: bool) -> &mut Self {
        self.append = v;
        self
    }
    pub fn truncate(&mut self, v: bool) -> &mut Self {
        self.truncate = v;
        self
    }
    pub fn create(&mut self, v: bool) -> &mut Self {
        self.create = v;
        self
    }
    pub fn create_new(&mut self, v: bool) -> &mut Self {
        self.create_new = v;
        self
    }
    /// `OpenOptionsExt::mode` do std.
    pub fn mode(&mut self, mode: u32) -> &mut Self {
        self.mode = mode;
        self
    }
    /// `OpenOptionsExt::custom_flags` do std (bits `O_*` do Linux, ex. `O_NOFOLLOW`).
    pub fn custom_flags(&mut self, flags: i32) -> &mut Self {
        self.custom_flags = flags as u32;
        self
    }

    /// Flags do `open(2)`, com as mesmas regras e erros do std.
    fn flags(&self) -> io::Result<OFlags> {
        let access = match (self.read, self.write, self.append) {
            (true, false, false) => OFlags::RDONLY,
            (false, true, false) => OFlags::WRONLY,
            (true, true, false) => OFlags::RDWR,
            (false, _, true) => OFlags::WRONLY | OFlags::APPEND,
            (true, _, true) => OFlags::RDWR | OFlags::APPEND,
            (false, false, false) => return Err(from_errno(sysabi::Errno::EINVAL)),
        };
        let writable = self.write || self.append;
        let creation = match (self.create, self.truncate, self.create_new) {
            (false, false, false) => OFlags::empty(),
            (true, false, false) => OFlags::CREAT,
            (false, true, false) => OFlags::TRUNC,
            (true, true, false) => OFlags::CREAT | OFlags::TRUNC,
            (_, _, true) => OFlags::CREAT | OFlags::EXCL,
        };
        if !writable && (self.create || self.truncate || self.create_new) {
            return Err(from_errno(sysabi::Errno::EINVAL));
        }
        if self.append && self.truncate && !self.create_new {
            return Err(from_errno(sysabi::Errno::EINVAL));
        }
        let custom = OFlags::from_bits_retain(self.custom_flags & !OFlags::ACCMODE);
        Ok(access | creation | custom | OFlags::CLOEXEC)
    }

    pub fn open<P: AsRef<Path>>(&self, path: P) -> io::Result<File> {
        let flags = self.flags()?;
        let fd = cvt(proc::sys().openat(Fd::CWD, pb(&path), flags, self.mode))?;
        Ok(File { fd: OwnedFd::from_raw_fd(fd.0) })
    }
}

// ---------------------------------------------------------------------------------------------
// read_dir

/// Diretório aberto, compartilhado pelo `ReadDir` e pelos `DirEntry` (pro `fstatat` relativo).
#[derive(Debug)]
struct DirHandle {
    fd: OwnedFd,
}

#[derive(Clone, Debug)]
pub struct DirEntry {
    dir: Arc<DirHandle>,
    root: PathBuf,
    name: OsString,
    ino: u64,
    kind: sysabi::FileType,
}

impl DirEntry {
    pub fn path(&self) -> PathBuf {
        self.root.join(&self.name)
    }
    pub fn file_name(&self) -> OsString {
        self.name.clone()
    }
    /// Como no std: `fstatat` no diretório, sem seguir link simbólico.
    pub fn metadata(&self) -> io::Result<Metadata> {
        cvt(proc::sys().fstatat(self.dir.fd.raw(), self.name.as_bytes(), AtFlags::SYMLINK_NOFOLLOW)).map(Metadata::from_stat)
    }
    /// Tipo do `d_type` (sem syscall).
    pub fn file_type(&self) -> io::Result<FileType> {
        Ok(FileType { mode: self.kind.mode_bits() })
    }
    pub fn ino(&self) -> u64 {
        self.ino
    }
    /// fd do diretório que contém a entrada (pras syscalls `*at`).
    pub fn dir_fd(&self) -> BorrowedFd<'_> {
        self.dir.fd.as_fd()
    }
}

#[derive(Debug)]
pub struct ReadDir {
    dir: Arc<DirHandle>,
    root: PathBuf,
    pending: VecDeque<sysabi::DirEntry>,
    done: bool,
}

impl Iterator for ReadDir {
    type Item = io::Result<DirEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(e) = self.pending.pop_front() {
                if e.name == b"." || e.name == b".." {
                    continue;
                }
                return Some(Ok(DirEntry {
                    dir: Arc::clone(&self.dir),
                    root: self.root.clone(),
                    name: OsString::from_vec(e.name),
                    ino: e.ino,
                    kind: e.kind,
                }));
            }
            if self.done {
                return None;
            }
            match proc::sys().getdents(self.dir.fd.raw()) {
                Ok(batch) if batch.is_empty() => self.done = true,
                Ok(batch) => self.pending.extend(batch),
                Err(e) => {
                    self.done = true;
                    return Some(Err(from_errno(e)));
                }
            }
        }
    }
}

fn open_dir_at(dirfd: Fd, path: &[u8], nofollow: bool) -> io::Result<OwnedFd> {
    let mut flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
    if nofollow {
        flags |= OFlags::NOFOLLOW;
    }
    cvt(proc::sys().openat(dirfd, path, flags, 0)).map(|fd| OwnedFd::from_raw_fd(fd.0))
}

pub fn read_dir<P: AsRef<Path>>(path: P) -> io::Result<ReadDir> {
    let fd = open_dir_at(Fd::CWD, pb(&path), false)?;
    Ok(ReadDir { dir: Arc::new(DirHandle { fd }), root: path.as_ref().to_path_buf(), pending: VecDeque::new(), done: false })
}

// ---------------------------------------------------------------------------------------------
// Funções soltas

pub fn metadata<P: AsRef<Path>>(path: P) -> io::Result<Metadata> {
    cvt(proc::sys().fstatat(Fd::CWD, pb(&path), AtFlags::empty())).map(Metadata::from_stat)
}

pub fn symlink_metadata<P: AsRef<Path>>(path: P) -> io::Result<Metadata> {
    cvt(proc::sys().fstatat(Fd::CWD, pb(&path), AtFlags::SYMLINK_NOFOLLOW)).map(Metadata::from_stat)
}

/// Troca de `Path::exists()` (que tocaria o FS do host).
pub fn exists<P: AsRef<Path>>(path: P) -> bool {
    metadata(path).is_ok()
}

/// `std::fs::exists`: `Ok(false)` só quando o caminho não existe.
pub fn try_exists<P: AsRef<Path>>(path: P) -> io::Result<bool> {
    match metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Troca de `Path::is_dir()`.
pub fn is_dir<P: AsRef<Path>>(path: P) -> bool {
    metadata(path).is_ok_and(|m| m.is_dir())
}

/// Troca de `Path::is_file()`.
pub fn is_file<P: AsRef<Path>>(path: P) -> bool {
    metadata(path).is_ok_and(|m| m.is_file())
}

/// Troca de `Path::is_symlink()`.
pub fn is_symlink<P: AsRef<Path>>(path: P) -> bool {
    symlink_metadata(path).is_ok_and(|m| m.is_symlink())
}

pub fn read_link<P: AsRef<Path>>(path: P) -> io::Result<PathBuf> {
    cvt(proc::sys().readlinkat(Fd::CWD, pb(&path))).map(|t| PathBuf::from(OsString::from_vec(t)))
}

/// `realpath(3)`: caminho absoluto sem `.`, `..` nem links simbólicos. Os erros são os da glibc:
/// ENOENT pra componente que falta, ENOTDIR pra componente que não é diretório (inclusive barra
/// final num arquivo), ELOOP depois de 40 links.
pub fn canonicalize<P: AsRef<Path>>(path: P) -> io::Result<PathBuf> {
    cvt(realpath(pb(&path))).map(|p| PathBuf::from(OsString::from_vec(p)))
}

const MAXSYMLINKS: usize = 40;

pub(crate) fn realpath(path: &[u8]) -> sysabi::SysResult<Vec<u8>> {
    let sys = proc::sys();
    if path.is_empty() {
        return Err(Errno::ENOENT);
    }
    let mut result: Vec<Vec<u8>> = Vec::new();
    if !path.starts_with(b"/") {
        let cwd = sys.getcwd()?;
        result.extend(cwd.split(|b| *b == b'/').filter(|c| !c.is_empty()).map(<[u8]>::to_vec));
    }
    let mut pending: VecDeque<Vec<u8>> = path.split(|b| *b == b'/').filter(|c| !c.is_empty()).map(<[u8]>::to_vec).collect();
    let must_be_dir = path.ends_with(b"/");
    let mut links = 0;
    let join = |comps: &[Vec<u8>]| -> Vec<u8> {
        if comps.is_empty() {
            return b"/".to_vec();
        }
        let mut out = Vec::new();
        for c in comps {
            out.push(b'/');
            out.extend_from_slice(c);
        }
        out
    };
    let mut last_is_dir = true;
    while let Some(c) = pending.pop_front() {
        if c == b"." {
            continue;
        }
        if c == b".." {
            result.pop();
            last_is_dir = true;
            continue;
        }
        result.push(c);
        let candidate = join(&result);
        let st = sys.fstatat(Fd::CWD, &candidate, AtFlags::SYMLINK_NOFOLLOW)?;
        match st.file_type() {
            sysabi::FileType::Symlink => {
                links += 1;
                if links > MAXSYMLINKS {
                    return Err(Errno::ELOOP);
                }
                let target = sys.readlinkat(Fd::CWD, &candidate)?;
                result.pop();
                if target.starts_with(b"/") {
                    result.clear();
                }
                let mut next: VecDeque<Vec<u8>> =
                    target.split(|b| *b == b'/').filter(|c| !c.is_empty()).map(<[u8]>::to_vec).collect();
                next.extend(pending.drain(..));
                pending = next;
                last_is_dir = true;
            }
            sysabi::FileType::Directory => last_is_dir = true,
            _ => {
                if !pending.is_empty() {
                    return Err(Errno::ENOTDIR);
                }
                last_is_dir = false;
            }
        }
    }
    if must_be_dir && !last_is_dir {
        return Err(Errno::ENOTDIR);
    }
    Ok(join(&result))
}

pub fn read<P: AsRef<Path>>(path: P) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    File::open(path)?.read_to_end(&mut out)?;
    Ok(out)
}

pub fn read_to_string<P: AsRef<Path>>(path: P) -> io::Result<String> {
    let mut out = String::new();
    File::open(path)?.read_to_string(&mut out)?;
    Ok(out)
}

pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> io::Result<()> {
    File::create(path)?.write_all(contents.as_ref())
}

pub fn remove_file<P: AsRef<Path>>(path: P) -> io::Result<()> {
    cvt(proc::sys().unlinkat(Fd::CWD, pb(&path), AtFlags::empty()))
}

pub fn remove_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    cvt(proc::sys().unlinkat(Fd::CWD, pb(&path), AtFlags::REMOVEDIR))
}

/// Como o std: um link simbólico é removido sem seguir; um diretório é esvaziado pelas syscalls
/// `*at` (sem seguir link no caminho) e removido. Entradas que somem no meio são ignoradas.
pub fn remove_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = pb(&path);
    let meta = cvt(proc::sys().fstatat(Fd::CWD, path, AtFlags::SYMLINK_NOFOLLOW))?;
    if meta.file_type() != sysabi::FileType::Directory {
        return cvt(proc::sys().unlinkat(Fd::CWD, path, AtFlags::empty()));
    }
    remove_dir_all_at(Fd::CWD, path)
}

fn remove_dir_all_at(parent: Fd, name: &[u8]) -> io::Result<()> {
    let sys = proc::sys();
    let dir = match open_dir_at(parent, name, true) {
        Ok(d) => d,
        Err(e) if e.raw_os_error() == Some(crate::errno::ENOENT) => return Ok(()),
        Err(e) => return Err(e),
    };
    loop {
        let batch = cvt(sys.getdents(dir.raw()))?;
        if batch.is_empty() {
            break;
        }
        for e in batch {
            if e.name == b"." || e.name == b".." {
                continue;
            }
            let r = if e.kind == sysabi::FileType::Directory {
                remove_dir_all_at(dir.raw(), &e.name)
            } else {
                cvt(sys.unlinkat(dir.raw(), &e.name, AtFlags::empty()))
            };
            match r {
                Err(e) if e.raw_os_error() == Some(crate::errno::ENOENT) => {}
                other => other?,
            }
        }
    }
    drop(dir);
    match cvt(sys.unlinkat(parent, name, AtFlags::REMOVEDIR)) {
        Err(e) if e.raw_os_error() == Some(crate::errno::ENOENT) => Ok(()),
        other => other,
    }
}

pub fn create_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    DirBuilder::new().create(path)
}

pub fn create_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
    DirBuilder::new().recursive(true).create(path)
}

#[derive(Clone, Debug)]
pub struct DirBuilder {
    recursive: bool,
    mode: u32,
}

impl Default for DirBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DirBuilder {
    pub fn new() -> DirBuilder {
        DirBuilder { recursive: false, mode: 0o777 }
    }
    pub fn recursive(&mut self, r: bool) -> &mut Self {
        self.recursive = r;
        self
    }
    pub fn mode(&mut self, mode: u32) -> &mut Self {
        self.mode = mode;
        self
    }
    pub fn create<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        if self.recursive { self.create_all(path.as_ref()) } else { self.mkdir(path.as_ref()) }
    }
    fn mkdir(&self, path: &Path) -> io::Result<()> {
        cvt(proc::sys().mkdirat(Fd::CWD, path_bytes(path), self.mode))
    }
    /// O algoritmo do std: tenta criar; se falta o pai, cria o pai e tenta de novo; se já existe um
    /// diretório, tudo bem.
    fn create_all(&self, path: &Path) -> io::Result<()> {
        if path.as_os_str().is_empty() {
            return Ok(());
        }
        match self.mkdir(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) if is_dir(path) => return Ok(()),
            Err(e) => return Err(e),
        }
        match path.parent() {
            Some(p) => self.create_all(p)?,
            None => return Err(io::Error::other("failed to create whole tree")),
        }
        match self.mkdir(path) {
            Ok(()) => Ok(()),
            Err(_) if is_dir(path) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

pub fn rename<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<()> {
    cvt(proc::sys().renameat2(Fd::CWD, pb(&from), Fd::CWD, pb(&to), RenameFlags::empty()))
}

/// Como o std: copia o conteúdo e as permissões; a origem tem que ser arquivo regular (ou link
/// pra um). Devolve o número de bytes copiados.
pub fn copy<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<u64> {
    let mut reader = File::open(&from)?;
    let meta = reader.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "the source path is neither a regular file nor a symlink to a regular file"));
    }
    let perm = meta.permissions();
    let mut writer = OpenOptions::new().write(true).create(true).truncate(true).mode(perm.mode).open(&to)?;
    if writer.metadata()?.is_file() {
        writer.set_permissions(perm)?;
    }
    io::copy(&mut reader, &mut writer)
}

pub fn set_permissions<P: AsRef<Path>>(path: P, perm: Permissions) -> io::Result<()> {
    cvt(proc::sys().fchmodat(Fd::CWD, pb(&path), perm.mode, AtFlags::empty()))
}

/// `link(2)`: como o std, não segue link simbólico na origem.
pub fn hard_link<P: AsRef<Path>, Q: AsRef<Path>>(original: P, link: Q) -> io::Result<()> {
    cvt(proc::sys().linkat(Fd::CWD, pb(&original), Fd::CWD, pb(&link), AtFlags::empty()))
}

/// `std::os::unix::fs::symlink`.
pub fn symlink<P: AsRef<Path>, Q: AsRef<Path>>(original: P, link: Q) -> io::Result<()> {
    cvt(proc::sys().symlinkat(pb(&original), Fd::CWD, pb(&link)))
}

#[deprecated(note = "use `symlink`")]
pub fn soft_link<P: AsRef<Path>, Q: AsRef<Path>>(original: P, link: Q) -> io::Result<()> {
    symlink(original, link)
}

/// Muda atime e/ou mtime (seguindo link); `None` não mexe (`UTIME_OMIT`).
pub fn set_times<P: AsRef<Path>>(path: P, atime: Option<SystemTime>, mtime: Option<SystemTime>) -> io::Result<()> {
    cvt(proc::sys().utimensat(Fd::CWD, pb(&path), set_time(atime), set_time(mtime), AtFlags::empty()))
}

/// `utimensat(2)` completo: `SetTime::Now`, `Omit` ou um instante, com ou sem seguir link.
pub fn utimensat<P: AsRef<Path>>(path: P, atime: SetTime, mtime: SetTime, follow: bool) -> io::Result<()> {
    let flags = if follow { AtFlags::empty() } else { AtFlags::SYMLINK_NOFOLLOW };
    cvt(proc::sys().utimensat(Fd::CWD, pb(&path), atime, mtime, flags))
}

/// `chown(2)` (segue link); `None` não muda.
pub fn chown<P: AsRef<Path>>(path: P, uid: Option<u32>, gid: Option<u32>) -> io::Result<()> {
    cvt(proc::sys().fchownat(Fd::CWD, pb(&path), uid, gid, AtFlags::empty()))
}

/// `lchown(2)`.
pub fn lchown<P: AsRef<Path>>(path: P, uid: Option<u32>, gid: Option<u32>) -> io::Result<()> {
    cvt(proc::sys().fchownat(Fd::CWD, pb(&path), uid, gid, AtFlags::SYMLINK_NOFOLLOW))
}

/// `fchown(2)`.
pub fn fchown<F: AsFd>(fd: F, uid: Option<u32>, gid: Option<u32>) -> io::Result<()> {
    cvt(proc::sys().fchownat(Fd(fd.as_fd().as_raw_fd()), b"", uid, gid, AtFlags::EMPTY_PATH))
}

/// `chroot(2)`: o caminho é resolvido como no Linux (ENOENT, ENOTDIR), mas o pseudo-linus não
/// troca a raiz de um processo, então o resultado é sempre EPERM.
pub fn chroot<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let st = cvt(proc::sys().fstatat(Fd::CWD, pb(&path), AtFlags::empty()))?;
    if st.file_type() != sysabi::FileType::Directory {
        return Err(from_errno(sysabi::Errno::ENOTDIR));
    }
    Err(from_errno(sysabi::Errno::EPERM))
}

/// `mkfifo(3)`.
pub fn mkfifo<P: AsRef<Path>>(path: P, mode: u32) -> io::Result<()> {
    cvt(proc::sys().mknodat(Fd::CWD, pb(&path), sysabi::mode::S_IFIFO | (mode & 0o7777), 0))
}

/// `mknod(2)`: `mode` traz o tipo (`S_IFIFO`, `S_IFCHR`, `S_IFBLK`, `S_IFREG`).
pub fn mknod<P: AsRef<Path>>(path: P, mode: u32, dev: u64) -> io::Result<()> {
    cvt(proc::sys().mknodat(Fd::CWD, pb(&path), mode, dev))
}

/// `statfs(2)`.
pub fn statfs<P: AsRef<Path>>(path: P) -> io::Result<StatFs> {
    cvt(proc::sys().statfs(pb(&path)))
}

/// `fstatfs(2)`.
pub fn fstatfs<F: AsFd>(fd: F) -> io::Result<StatFs> {
    cvt(proc::sys().fstatfs(Fd(fd.as_fd().as_raw_fd())))
}

/// `access(2)` (`faccessat` com o uid real). `mode` vazio é `F_OK`.
pub fn access<P: AsRef<Path>>(path: P, mode: AccessMode) -> io::Result<()> {
    cvt(proc::sys().faccessat(Fd::CWD, pb(&path), mode, AtFlags::empty()))
}

/// `euidaccess(3)`/`faccessat(..., AT_EACCESS)`: com o uid efetivo.
pub fn eaccess<P: AsRef<Path>>(path: P, mode: AccessMode) -> io::Result<()> {
    cvt(proc::sys().faccessat(Fd::CWD, pb(&path), mode, AtFlags::REMOVEDIR))
}

pub use sysabi::AccessMode as Access;

/// Lê o conteúdo de um link como bytes (sem passar por `PathBuf`).
pub fn read_link_bytes(path: &OsStr) -> io::Result<Vec<u8>> {
    cvt(proc::sys().readlinkat(Fd::CWD, path.as_bytes()))
}
