//! Subconjunto de `std::fs` sobre o VFS do processo corrente.
//!
//! Os nomes e assinaturas seguem o std, pra que o porte seja trocar `use std::fs::...` por
//! `use sysio::fs::...`. O que não dá pra imitar por import é registrado no README do experimento
//! (métodos inerentes de `std::path::Path` como `exists()` e `is_dir()`, que tocam o FS do host e
//! não podem ser sobrescritos; pra eles há [`exists`], [`is_dir`], [`is_file`] e [`is_symlink`]).

use std::ffi::OsString;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::errno::*;
use crate::proc::{self, lock};
use crate::vfs::{Ino, NodeKind, Stat};

fn with_vfs<R>(f: impl FnOnce(&mut crate::vfs::Vfs, &Path, SystemTime, u32) -> Result<R, i32>) -> io::Result<R> {
    let p = proc::current();
    let cwd = lock(&p.cwd).clone();
    let mut vfs = lock(&p.vfs);
    f(&mut vfs, &cwd, p.now, p.umask).map_err(err)
}

// ---------------------------------------------------------------------------------------------
// Metadata, FileType, Permissions

#[derive(Clone, Debug)]
pub struct Metadata {
    pub(crate) st: Stat,
}

impl Metadata {
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
    pub fn len(&self) -> u64 {
        self.st.size
    }
    pub fn is_empty(&self) -> bool {
        self.st.size == 0
    }
    pub fn permissions(&self) -> Permissions {
        Permissions { mode: self.st.mode & 0o7777 }
    }
    pub fn modified(&self) -> io::Result<SystemTime> {
        Ok(self.st.mtime)
    }
    pub fn accessed(&self) -> io::Result<SystemTime> {
        Ok(self.st.atime)
    }
    /// O ext4 do oráculo tem btime, mas o VFS não guarda: mesmo erro que o std dá sem statx.
    pub fn created(&self) -> io::Result<SystemTime> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "creation time is not available on this platform"))
    }
    pub fn stat(&self) -> &Stat {
        &self.st
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileType {
    pub(crate) mode: u32,
}

impl FileType {
    pub fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }
    pub fn is_file(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }
    pub fn is_symlink(&self) -> bool {
        self.mode & S_IFMT == S_IFLNK
    }
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

/// `fstat(2)` de um handle aberto: arquivo do VFS ou um dos fluxos padrão. É o que substitui
/// `AsFd` + `rustix::fs::fstat` nos portes.
pub trait Fstat {
    fn fstat(&self) -> io::Result<Metadata>;
    /// `lseek(fd, 0, SEEK_CUR)`: pipe dá ESPIPE.
    fn seek_position(&self) -> io::Result<u64> {
        Err(err(ESPIPE))
    }
    /// `fcntl(fd, F_GETFL) & O_APPEND`.
    fn is_appending(&self) -> bool {
        false
    }
}

impl<T: Fstat + ?Sized> Fstat for &T {
    fn fstat(&self) -> io::Result<Metadata> {
        (**self).fstat()
    }
    fn seek_position(&self) -> io::Result<u64> {
        (**self).seek_position()
    }
    fn is_appending(&self) -> bool {
        (**self).is_appending()
    }
}

impl Fstat for File {
    fn fstat(&self) -> io::Result<Metadata> {
        self.metadata()
    }
    fn seek_position(&self) -> io::Result<u64> {
        Ok(*lock(&self.pos))
    }
    fn is_appending(&self) -> bool {
        self.append
    }
}

/// Metadados de um pipe anônimo (é o que stdin/stdout/stderr são pro programa na bancada).
pub(crate) fn pipe_metadata(id: u64) -> Metadata {
    let now = proc::try_current().map(|p| p.now).unwrap_or(SystemTime::UNIX_EPOCH);
    Metadata {
        st: Stat {
            dev: 0xc,
            ino: id,
            mode: S_IFIFO | 0o600,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 0,
            blocks: 0,
            blksize: 4096,
            atime: now,
            mtime: now,
            ctime: now,
        },
    }
}

// ---------------------------------------------------------------------------------------------
// File e OpenOptions

#[derive(Debug)]
pub struct File {
    ino: Ino,
    pos: Arc<Mutex<u64>>,
    read: bool,
    write: bool,
    append: bool,
    path: PathBuf,
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

    pub fn metadata(&self) -> io::Result<Metadata> {
        with_vfs(|vfs, _, _, _| Ok(Metadata { st: vfs.stat(self.ino) }))
    }

    pub fn set_len(&self, size: u64) -> io::Result<()> {
        with_vfs(|vfs, _, now, _| {
            let node = vfs.inode_mut(self.ino);
            match &mut node.kind {
                NodeKind::File(d) => d.resize(size as usize, 0),
                _ => return Err(EINVAL),
            }
            node.mtime = now;
            Ok(())
        })
    }

    pub fn sync_all(&self) -> io::Result<()> {
        Ok(())
    }

    pub fn sync_data(&self) -> io::Result<()> {
        Ok(())
    }

    /// Como `dup(2)`: o clone compartilha a posição.
    pub fn try_clone(&self) -> io::Result<File> {
        Ok(File {
            ino: self.ino,
            pos: Arc::clone(&self.pos),
            read: self.read,
            write: self.write,
            append: self.append,
            path: self.path.clone(),
        })
    }

    pub fn set_permissions(&self, perm: Permissions) -> io::Result<()> {
        with_vfs(|vfs, _, now, _| {
            let node = vfs.inode_mut(self.ino);
            node.perm = perm.mode & 0o7777;
            node.ctime = now;
            Ok(())
        })
    }

    /// Identidade do arquivo aberto (o que `fstat` daria em `st_dev`/`st_ino`).
    pub fn ino(&self) -> u64 {
        self.ino
    }

    fn do_read(&self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.read {
            return Err(err(EBADF));
        }
        let mut pos = lock(&self.pos);
        let n = with_vfs(|vfs, _, _, _| match &vfs.inode(self.ino).kind {
            NodeKind::File(d) => {
                let start = (*pos as usize).min(d.len());
                let n = buf.len().min(d.len() - start);
                buf[..n].copy_from_slice(&d[start..start + n]);
                Ok(n)
            }
            NodeKind::Dir(_) => Err(EISDIR),
            NodeKind::Symlink(_) => Err(ELOOP),
        })?;
        *pos += n as u64;
        Ok(n)
    }

    fn do_write(&self, buf: &[u8]) -> io::Result<usize> {
        if !self.write {
            return Err(err(EBADF));
        }
        let mut pos = lock(&self.pos);
        let end = with_vfs(|vfs, _, now, _| {
            let node = vfs.inode_mut(self.ino);
            let NodeKind::File(d) = &mut node.kind else { return Err(EISDIR) };
            let start = if self.append { d.len() } else { *pos as usize };
            if d.len() < start {
                d.resize(start, 0);
            }
            let overlap = (d.len() - start).min(buf.len());
            d[start..start + overlap].copy_from_slice(&buf[..overlap]);
            d.extend_from_slice(&buf[overlap..]);
            node.mtime = now;
            node.ctime = now;
            Ok(start + buf.len())
        })?;
        *pos = end as u64;
        Ok(buf.len())
    }

    fn do_seek(&self, from: SeekFrom) -> io::Result<u64> {
        let mut pos = lock(&self.pos);
        let len = with_vfs(|vfs, _, _, _| Ok(vfs.stat(self.ino).size))?;
        let new = match from {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::End(d) => len as i128 + d as i128,
            SeekFrom::Current(d) => *pos as i128 + d as i128,
        };
        if new < 0 {
            return Err(err(EINVAL));
        }
        *pos = new as u64;
        Ok(*pos)
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.do_read(buf)
    }
}

impl Read for &File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.do_read(buf)
    }
}

impl Write for File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.do_write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for &File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.do_write(buf)
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

#[derive(Clone, Debug)]
pub struct OpenOptions {
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
    mode: u32,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenOptions {
    pub fn new() -> OpenOptions {
        OpenOptions {
            read: false,
            write: false,
            append: false,
            truncate: false,
            create: false,
            create_new: false,
            mode: 0o666,
        }
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

    pub fn open<P: AsRef<Path>>(&self, path: P) -> io::Result<File> {
        let path = path.as_ref();
        let writes = self.write || self.append;
        let (ino, abs) = with_vfs(|vfs, cwd, now, umask| {
            let w = vfs.walk(cwd, path, !self.create_new)?;
            let ino = match w.ino {
                Some(_) if self.create_new => return Err(EEXIST),
                Some(ino) => ino,
                None if self.create || self.create_new => {
                    let name = w.name.clone().ok_or(EISDIR)?;
                    vfs.create_file(w.parent, &name, self.mode & !umask & 0o7777, now)?
                }
                None => return Err(ENOENT),
            };
            match &mut vfs.inode_mut(ino).kind {
                NodeKind::Dir(_) if writes => return Err(EISDIR),
                NodeKind::File(d) if self.truncate && writes && !d.is_empty() => {
                    d.clear();
                    let node = vfs.inode_mut(ino);
                    node.mtime = now;
                    node.ctime = now;
                }
                _ => {}
            }
            Ok((ino, w.path()))
        })?;
        Ok(File {
            ino,
            pos: Arc::new(Mutex::new(0)),
            read: self.read || !writes,
            write: writes,
            append: self.append,
            path: abs,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// read_dir

#[derive(Clone, Debug)]
pub struct DirEntry {
    path: PathBuf,
    name: OsString,
    ino: Ino,
    mode: u32,
}

impl DirEntry {
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }
    pub fn file_name(&self) -> OsString {
        self.name.clone()
    }
    /// Como no std: não segue link simbólico.
    pub fn metadata(&self) -> io::Result<Metadata> {
        with_vfs(|vfs, _, _, _| Ok(Metadata { st: vfs.stat(self.ino) }))
    }
    pub fn file_type(&self) -> io::Result<FileType> {
        Ok(FileType { mode: self.mode })
    }
    pub fn ino(&self) -> u64 {
        self.ino
    }
}

#[derive(Debug)]
pub struct ReadDir {
    inner: std::vec::IntoIter<DirEntry>,
}

impl Iterator for ReadDir {
    type Item = io::Result<DirEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(Ok)
    }
}

pub fn read_dir<P: AsRef<Path>>(path: P) -> io::Result<ReadDir> {
    let path = path.as_ref();
    let entries = with_vfs(|vfs, cwd, _, _| {
        let (ino, _) = vfs.resolve(cwd, path, true)?;
        let list = vfs.list(ino)?;
        Ok(list
            .into_iter()
            .map(|(name, child)| DirEntry {
                path: path.join(&name),
                mode: vfs.stat(child).mode,
                name,
                ino: child,
            })
            .collect::<Vec<_>>())
    })?;
    Ok(ReadDir { inner: entries.into_iter() })
}

// ---------------------------------------------------------------------------------------------
// Funções soltas

pub fn metadata<P: AsRef<Path>>(path: P) -> io::Result<Metadata> {
    with_vfs(|vfs, cwd, _, _| {
        let (ino, _) = vfs.resolve(cwd, path.as_ref(), true)?;
        Ok(Metadata { st: vfs.stat(ino) })
    })
}

pub fn symlink_metadata<P: AsRef<Path>>(path: P) -> io::Result<Metadata> {
    with_vfs(|vfs, cwd, _, _| {
        let (ino, _) = vfs.resolve(cwd, path.as_ref(), false)?;
        Ok(Metadata { st: vfs.stat(ino) })
    })
}

/// Troca de `Path::exists()` (que tocaria o FS do host).
pub fn exists<P: AsRef<Path>>(path: P) -> bool {
    metadata(path).is_ok()
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
    with_vfs(|vfs, cwd, _, _| {
        let (ino, _) = vfs.resolve(cwd, path.as_ref(), false)?;
        match &vfs.inode(ino).kind {
            NodeKind::Symlink(t) => Ok(PathBuf::from(t)),
            _ => Err(EINVAL),
        }
    })
}

pub fn canonicalize<P: AsRef<Path>>(path: P) -> io::Result<PathBuf> {
    with_vfs(|vfs, cwd, _, _| Ok(vfs.resolve(cwd, path.as_ref(), true)?.1))
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

fn parent_and_name(vfs: &crate::vfs::Vfs, cwd: &Path, path: &Path) -> Result<(Ino, OsString), i32> {
    let w = vfs.walk(cwd, path, false)?;
    let name = w.name.ok_or(EBUSY_OR_INVAL)?;
    Ok((w.parent, name))
}

/// `rmdir("/")`, `unlink(".")` e afins: o Linux devolve EBUSY ou EINVAL; usamos EINVAL.
const EBUSY_OR_INVAL: i32 = EINVAL;

pub fn remove_file<P: AsRef<Path>>(path: P) -> io::Result<()> {
    with_vfs(|vfs, cwd, now, _| {
        let (parent, name) = parent_and_name(vfs, cwd, path.as_ref())?;
        vfs.unlink(parent, &name, now)
    })
}

pub fn remove_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    with_vfs(|vfs, cwd, now, _| {
        let (parent, name) = parent_and_name(vfs, cwd, path.as_ref())?;
        vfs.rmdir(parent, &name, now)
    })
}

pub fn remove_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref();
    let meta = symlink_metadata(path)?;
    if !meta.is_dir() {
        return remove_file(path);
    }
    for entry in read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            remove_dir_all(entry.path())?;
        } else {
            remove_file(entry.path())?;
        }
    }
    remove_dir(path)
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
        let path = path.as_ref();
        if self.recursive {
            if is_dir(path) {
                return Ok(());
            }
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                self.create(parent)?;
            }
            return match self.create_one(path) {
                Err(e) if e.raw_os_error() == Some(EEXIST) && is_dir(path) => Ok(()),
                other => other,
            };
        }
        self.create_one(path)
    }
    fn create_one(&self, path: &Path) -> io::Result<()> {
        with_vfs(|vfs, cwd, now, umask| {
            let w = vfs.walk(cwd, path, false)?;
            if w.ino.is_some() {
                return Err(EEXIST);
            }
            let name = w.name.ok_or(EEXIST)?;
            vfs.mkdir(w.parent, &name, self.mode & !umask & 0o7777, now)?;
            Ok(())
        })
    }
}

pub fn rename<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<()> {
    with_vfs(|vfs, cwd, now, _| {
        let (fp, fname) = parent_and_name(vfs, cwd, from.as_ref())?;
        if vfs.lookup(fp, &fname).is_none() {
            return Err(ENOENT);
        }
        let (tp, tname) = parent_and_name(vfs, cwd, to.as_ref())?;
        vfs.rename(fp, &fname, tp, &tname, now)
    })
}

pub fn copy<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<u64> {
    let data = read(&from)?;
    let perm = metadata(&from)?.permissions();
    write(&to, &data)?;
    set_permissions(&to, perm)?;
    Ok(data.len() as u64)
}

pub fn set_permissions<P: AsRef<Path>>(path: P, perm: Permissions) -> io::Result<()> {
    with_vfs(|vfs, cwd, now, _| {
        let (ino, _) = vfs.resolve(cwd, path.as_ref(), true)?;
        let node = vfs.inode_mut(ino);
        node.perm = perm.mode & 0o7777;
        node.ctime = now;
        Ok(())
    })
}

pub fn hard_link<P: AsRef<Path>, Q: AsRef<Path>>(original: P, link: Q) -> io::Result<()> {
    with_vfs(|vfs, cwd, now, _| {
        let (ino, _) = vfs.resolve(cwd, original.as_ref(), false)?;
        if matches!(vfs.inode(ino).kind, NodeKind::Dir(_)) {
            return Err(EPERM);
        }
        let w = vfs.walk(cwd, link.as_ref(), false)?;
        if w.ino.is_some() {
            return Err(EEXIST);
        }
        let name = w.name.ok_or(EEXIST)?;
        vfs.link_existing(w.parent, &name, ino, now)
    })
}

/// `std::os::unix::fs::symlink`.
pub fn symlink<P: AsRef<Path>, Q: AsRef<Path>>(original: P, link: Q) -> io::Result<()> {
    with_vfs(|vfs, cwd, now, _| {
        let w = vfs.walk(cwd, link.as_ref(), false)?;
        if w.ino.is_some() {
            return Err(EEXIST);
        }
        let name = w.name.ok_or(EEXIST)?;
        vfs.symlink(w.parent, &name, original.as_ref().as_os_str(), now)?;
        Ok(())
    })
}

/// Muda mtime/atime (o que `File::set_times`/`utimensat` fariam).
pub fn set_times<P: AsRef<Path>>(path: P, atime: Option<SystemTime>, mtime: Option<SystemTime>) -> io::Result<()> {
    with_vfs(|vfs, cwd, now, _| {
        let (ino, _) = vfs.resolve(cwd, path.as_ref(), true)?;
        let node = vfs.inode_mut(ino);
        if let Some(a) = atime {
            node.atime = a;
        }
        if let Some(m) = mtime {
            node.mtime = m;
        }
        node.ctime = now;
        Ok(())
    })
}
