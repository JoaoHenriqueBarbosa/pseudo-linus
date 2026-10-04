//! O `/proc`. Os dados de processo vêm do kernel por [`ProcProvider`]; aqui ficam a árvore, os números
//! de inode, os magic links (`/proc/<pid>/{cwd,exe,root,fd/N}`) e os formatos de texto.
//!
//! Números de inode: bits 63..56 dizem o tipo da entrada, 55..32 o pid e 31..0 o número da entrada ou do
//! fd. O conteúdo de um arquivo é gerado no `open` (como o `seq_file` gera na primeira leitura) e fica no
//! handle; `st_size` é 0, como no Linux.

use std::any::Any;
use std::sync::{Arc, OnceLock, Weak};

use crate::fs::{FileHandle, FileSystem, Link, NewNode, SetAttr, WritePos};
use crate::mount::{Loc, Namespace};
use crate::namei;
use crate::types::*;

/// `PROC_SUPER_MAGIC`.
pub const PROC_SUPER_MAGIC: u64 = 0x9fa0;

const ROOT: Ino = 1;
const KIND_STATIC: u64 = 0;
const KIND_PID: u64 = 1;
const KIND_FD: u64 = 2;

fn ino(kind: u64, pid: Pid, n: u32) -> Ino {
    (kind << 56) | ((pid as u64 & 0xff_ffff) << 32) | n as u64
}

fn split(ino: Ino) -> (u64, Pid, u32) {
    (ino >> 56, ((ino >> 32) & 0xff_ffff) as Pid, (ino & 0xffff_ffff) as u32)
}

/// Destino de `/proc/<pid>/fd/N`.
#[derive(Clone, Debug)]
pub struct FdLink {
    /// Texto do `readlink` (caminho, `pipe:[N]`, `socket:[N]`...).
    pub text: Vec<u8>,
    /// Pra onde abrir leva.
    pub target: Link,
    /// Bits de permissão do link conforme o modo de abertura (`lr-x------`, `l-wx------`, `lrwx------`).
    pub perm: Mode,
}

/// O que o procfs precisa saber de um processo.
#[derive(Clone, Debug, Default)]
pub struct ProcData {
    pub pid: Pid,
    pub ppid: Pid,
    pub pgid: Pid,
    pub sid: Pid,
    /// `R`, `S`, `D`, `T`, `Z`.
    pub state: char,
    pub comm: Vec<u8>,
    /// argv com um NUL depois de cada argumento.
    pub cmdline: Vec<u8>,
    /// Ambiente com um NUL depois de cada `NAME=valor`.
    pub environ: Vec<u8>,
    pub uid: Uid,
    pub gid: Gid,
    pub umask: Mode,
    pub cwd: Option<Loc>,
    pub root: Option<Loc>,
    pub exe: Option<Loc>,
}

/// Fonte de dados do procfs: o kernel implementa.
pub trait ProcProvider: Send + Sync {
    /// pids visíveis no sandbox, em ordem crescente.
    fn pids(&self) -> Vec<Pid>;
    fn process(&self, pid: Pid) -> Option<ProcData>;
    /// fds abertos, em ordem crescente.
    fn fds(&self, pid: Pid) -> Option<Vec<i32>>;
    fn fd(&self, cx: &Caller, pid: Pid, fd: i32) -> Option<FdLink>;
    /// `/proc/sys/kernel/pid_max`.
    fn pid_max(&self) -> u32 {
        4_194_304
    }
}

/// Entradas fixas da raiz.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Top {
    SelfLink = 2,
    ThreadSelf = 3,
    Mounts = 4,
    Sys = 5,
    SysKernel = 6,
    PidMax = 7,
}

const TOP_ENTRIES: &[(&str, Top)] =
    &[("self", Top::SelfLink), ("thread-self", Top::ThreadSelf), ("mounts", Top::Mounts), ("sys", Top::Sys)];

/// Entradas de `/proc/<pid>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PidEntry {
    Dir = 0,
    Fd = 1,
    Cwd = 2,
    Exe = 3,
    Root = 4,
    Cmdline = 5,
    Environ = 6,
    Comm = 7,
    Mounts = 8,
}

const PID_ENTRIES: &[(&str, PidEntry)] = &[
    ("fd", PidEntry::Fd),
    ("cwd", PidEntry::Cwd),
    ("exe", PidEntry::Exe),
    ("root", PidEntry::Root),
    ("cmdline", PidEntry::Cmdline),
    ("environ", PidEntry::Environ),
    ("comm", PidEntry::Comm),
    ("mounts", PidEntry::Mounts),
];

fn pid_entry(n: u32) -> Option<PidEntry> {
    Some(match n {
        0 => PidEntry::Dir,
        1 => PidEntry::Fd,
        2 => PidEntry::Cwd,
        3 => PidEntry::Exe,
        4 => PidEntry::Root,
        5 => PidEntry::Cmdline,
        6 => PidEntry::Environ,
        7 => PidEntry::Comm,
        8 => PidEntry::Mounts,
        _ => return None,
    })
}

/// O procfs de um sandbox.
pub struct Procfs {
    dev: u64,
    provider: Arc<dyn ProcProvider>,
    ns: OnceLock<Weak<Namespace>>,
    boot: TimeSpec,
}

impl std::fmt::Debug for Procfs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Procfs(dev {:#x})", self.dev)
    }
}

enum Node {
    Dir,
    File,
    Link,
}

impl Procfs {
    pub fn new(dev: u64, provider: Arc<dyn ProcProvider>, boot: TimeSpec) -> Arc<Procfs> {
        Arc::new(Procfs { dev, provider, ns: OnceLock::new(), boot })
    }

    /// Liga o procfs ao namespace onde ele está montado (pra `/proc/mounts`).
    pub fn set_namespace(&self, ns: &Arc<Namespace>) {
        let _ = self.ns.set(Arc::downgrade(ns));
    }

    fn node(&self, i: Ino) -> SysResult<(Node, Mode, Uid, Gid)> {
        let (kind, pid, n) = split(i);
        match kind {
            KIND_STATIC => match n as Ino {
                ROOT => Ok((Node::Dir, 0o555, 0, 0)),
                2..=4 => Ok((Node::Link, 0o777, 0, 0)),
                5 | 6 => Ok((Node::Dir, 0o555, 0, 0)),
                7 => Ok((Node::File, 0o644, 0, 0)),
                _ => Err(Errno::ENOENT),
            },
            KIND_PID => {
                let p = self.provider.process(pid).ok_or(Errno::ENOENT)?;
                let e = pid_entry(n).ok_or(Errno::ENOENT)?;
                let (node, perm) = match e {
                    PidEntry::Dir => (Node::Dir, 0o555),
                    PidEntry::Fd => (Node::Dir, 0o500),
                    PidEntry::Cwd | PidEntry::Exe | PidEntry::Root => (Node::Link, 0o777),
                    PidEntry::Environ => (Node::File, 0o400),
                    _ => (Node::File, 0o444),
                };
                Ok((node, perm, p.uid, p.gid))
            }
            KIND_FD => {
                let p = self.provider.process(pid).ok_or(Errno::ENOENT)?;
                let cx = crate::ops::kernel_caller(self.ns_root().ok_or(Errno::ENOENT)?);
                let l = self.provider.fd(&cx, pid, n as i32).ok_or(Errno::ENOENT)?;
                Ok((Node::Link, l.perm, p.uid, p.gid))
            }
            _ => Err(Errno::ENOENT),
        }
    }

    fn ns(&self) -> Option<Arc<Namespace>> {
        self.ns.get().and_then(Weak::upgrade)
    }

    fn ns_root(&self) -> Option<Loc> {
        self.ns().map(|n| n.root())
    }

    fn mounts_text(&self, cx: &Caller) -> Vec<u8> {
        let mut out = Vec::new();
        let Some(ns) = self.ns() else { return out };
        for m in ns.mounts() {
            let point = match &m.parent {
                None => b"/".to_vec(),
                Some((pm, mp)) => namei::d_path(&Loc { mnt: pm.clone(), ino: *mp }, &cx.root).unwrap_or_else(|_| b"/".to_vec()),
            };
            let mut opts = m.flags.describe();
            if !m.fs_options.is_empty() {
                opts.push(',');
                opts.push_str(&m.fs_options);
            }
            out.extend_from_slice(m.source.as_bytes());
            out.push(b' ');
            out.extend_from_slice(&point);
            out.push(b' ');
            out.extend_from_slice(m.fs.fs_type().as_bytes());
            out.push(b' ');
            out.extend_from_slice(opts.as_bytes());
            out.extend_from_slice(b" 0 0\n");
        }
        out
    }

    fn content(&self, cx: &Caller, i: Ino) -> SysResult<Vec<u8>> {
        let (kind, pid, n) = split(i);
        match kind {
            KIND_STATIC if n == Top::PidMax as u32 => Ok(format!("{}\n", self.provider.pid_max()).into_bytes()),
            KIND_PID => {
                let p = self.provider.process(pid).ok_or(Errno::ESRCH)?;
                match pid_entry(n) {
                    Some(PidEntry::Cmdline) => Ok(p.cmdline),
                    Some(PidEntry::Environ) => Ok(p.environ),
                    Some(PidEntry::Comm) => {
                        let mut c = p.comm.clone();
                        c.truncate(15);
                        c.push(b'\n');
                        Ok(c)
                    }
                    Some(PidEntry::Mounts) => Ok(self.mounts_text(cx)),
                    _ => Err(Errno::EINVAL),
                }
            }
            _ => Err(Errno::EINVAL),
        }
    }
}

impl FileSystem for Procfs {
    fn fs_type(&self) -> &'static str {
        "proc"
    }

    fn dev(&self) -> u64 {
        self.dev
    }

    fn root_ino(&self) -> Ino {
        ROOT
    }

    fn statfs(&self) -> StatFs {
        StatFs { fs_type: PROC_SUPER_MAGIC, bsize: 4096, namelen: 255, frsize: 4096, ..StatFs::default() }
    }

    fn getattr(&self, _cx: &Caller, i: Ino) -> SysResult<Stat> {
        let (node, perm, uid, gid) = self.node(i).map_err(|_| Errno::ENOENT)?;
        let (tbits, nlink) = match node {
            Node::Dir => (S_IFDIR, 2),
            Node::File => (S_IFREG, 1),
            Node::Link => (S_IFLNK, 1),
        };
        let size = match (node, split(i)) {
            (Node::Link, (KIND_STATIC, _, n)) if n == Top::SelfLink as u32 => 0,
            _ => 0,
        };
        Ok(Stat {
            dev: self.dev,
            ino: i,
            mode: tbits | perm,
            nlink,
            uid,
            gid,
            rdev: 0,
            size,
            blksize: 1024,
            blocks: 0,
            atime: self.boot,
            mtime: self.boot,
            ctime: self.boot,
            btime: None,
        })
    }

    fn lookup(&self, cx: &Caller, dir: Ino, name: &[u8]) -> SysResult<Ino> {
        let (kind, pid, n) = split(dir);
        match (kind, n) {
            (KIND_STATIC, 1) => {
                for (nm, t) in TOP_ENTRIES {
                    if nm.as_bytes() == name {
                        return Ok(*t as Ino);
                    }
                }
                let s = std::str::from_utf8(name).map_err(|_| Errno::ENOENT)?;
                if s.starts_with('0') || s.starts_with('+') {
                    return Err(Errno::ENOENT);
                }
                let pid: Pid = s.parse().map_err(|_| Errno::ENOENT)?;
                if pid <= 0 || self.provider.process(pid).is_none() {
                    return Err(Errno::ENOENT);
                }
                let _ = cx;
                Ok(ino(KIND_PID, pid, PidEntry::Dir as u32))
            }
            (KIND_STATIC, 5) if name == b"kernel" => Ok(Top::SysKernel as Ino),
            (KIND_STATIC, 6) if name == b"pid_max" => Ok(Top::PidMax as Ino),
            (KIND_PID, 0) => {
                for (nm, e) in PID_ENTRIES {
                    if nm.as_bytes() == name {
                        self.provider.process(pid).ok_or(Errno::ENOENT)?;
                        return Ok(ino(KIND_PID, pid, *e as u32));
                    }
                }
                Err(Errno::ENOENT)
            }
            (KIND_PID, 1) => {
                let s = std::str::from_utf8(name).map_err(|_| Errno::ENOENT)?;
                if s.starts_with('+') || (s.len() > 1 && s.starts_with('0')) {
                    return Err(Errno::ENOENT);
                }
                let fd: u32 = s.parse().map_err(|_| Errno::ENOENT)?;
                let fds = self.provider.fds(pid).ok_or(Errno::ENOENT)?;
                if !fds.contains(&(fd as i32)) {
                    return Err(Errno::ENOENT);
                }
                Ok(ino(KIND_FD, pid, fd))
            }
            _ => Err(Errno::ENOENT),
        }
    }

    fn parent(&self, dir: Ino) -> SysResult<Ino> {
        let (kind, pid, n) = split(dir);
        Ok(match (kind, n) {
            (KIND_STATIC, 1) | (KIND_STATIC, 5) => ROOT,
            (KIND_STATIC, 6) => Top::Sys as Ino,
            (KIND_PID, 0) => ROOT,
            (KIND_PID, _) => ino(KIND_PID, pid, 0),
            _ => ROOT,
        })
    }

    fn name_of(&self, i: Ino) -> Option<(Ino, Vec<u8>)> {
        let (kind, pid, n) = split(i);
        match (kind, n) {
            (KIND_STATIC, 1) => None,
            (KIND_STATIC, x) => {
                let nm = match x {
                    2 => "self",
                    3 => "thread-self",
                    4 => "mounts",
                    5 => "sys",
                    6 => "kernel",
                    7 => "pid_max",
                    _ => return None,
                };
                let parent = match x {
                    6 => Top::Sys as Ino,
                    7 => Top::SysKernel as Ino,
                    _ => ROOT,
                };
                Some((parent, nm.as_bytes().to_vec()))
            }
            (KIND_PID, 0) => Some((ROOT, pid.to_string().into_bytes())),
            (KIND_PID, e) => {
                let nm = PID_ENTRIES.iter().find(|(_, x)| *x as u32 == e)?.0;
                Some((ino(KIND_PID, pid, 0), nm.as_bytes().to_vec()))
            }
            (KIND_FD, fd) => Some((ino(KIND_PID, pid, PidEntry::Fd as u32), fd.to_string().into_bytes())),
            _ => None,
        }
    }

    fn readlink(&self, cx: &Caller, i: Ino) -> SysResult<Vec<u8>> {
        let (kind, pid, n) = split(i);
        match (kind, n) {
            (KIND_STATIC, 2) => {
                if cx.pid <= 0 {
                    return Err(Errno::ENOENT);
                }
                Ok(cx.pid.to_string().into_bytes())
            }
            (KIND_STATIC, 3) => {
                if cx.pid <= 0 {
                    return Err(Errno::ENOENT);
                }
                Ok(format!("{0}/task/{0}", cx.pid).into_bytes())
            }
            (KIND_STATIC, 4) => Ok(b"self/mounts".to_vec()),
            (KIND_PID, e) => {
                let p = self.provider.process(pid).ok_or(Errno::ENOENT)?;
                let loc = match pid_entry(e) {
                    Some(PidEntry::Cwd) => p.cwd,
                    Some(PidEntry::Exe) => p.exe,
                    Some(PidEntry::Root) => p.root,
                    _ => return Err(Errno::EINVAL),
                }
                .ok_or(Errno::ENOENT)?;
                let ns = self.ns().ok_or(Errno::ENOENT)?;
                Ok(ns.fd_path(cx, &loc))
            }
            (KIND_FD, fd) => Ok(self.provider.fd(cx, pid, fd as i32).ok_or(Errno::ENOENT)?.text),
            _ => Err(Errno::EINVAL),
        }
    }

    fn follow_link(&self, cx: &Caller, i: Ino) -> SysResult<Link> {
        let (kind, pid, n) = split(i);
        match kind {
            KIND_PID => {
                let p = self.provider.process(pid).ok_or(Errno::ENOENT)?;
                let loc = match pid_entry(n) {
                    Some(PidEntry::Cwd) => p.cwd,
                    Some(PidEntry::Exe) => p.exe,
                    Some(PidEntry::Root) => p.root,
                    _ => return Err(Errno::EINVAL),
                }
                .ok_or(Errno::ENOENT)?;
                Ok(Link::Jump(loc))
            }
            KIND_FD => Ok(self.provider.fd(cx, pid, n as i32).ok_or(Errno::ENOENT)?.target),
            _ => Ok(Link::Path(self.readlink(cx, i)?)),
        }
    }

    fn create(&self, _cx: &Caller, dir: Ino, name: &[u8], _node: NewNode) -> SysResult<Ino> {
        // O procfs não cria nada: um nome que existe dá EEXIST no VFS; um que não existe dá ENOENT.
        let _ = (dir, name);
        Err(Errno::ENOENT)
    }

    fn link(&self, _cx: &Caller, _ino: Ino, _dir: Ino, _name: &[u8]) -> SysResult<()> {
        Err(Errno::EPERM)
    }

    fn unlink(&self, _cx: &Caller, _dir: Ino, _name: &[u8]) -> SysResult<()> {
        Err(Errno::EPERM)
    }

    fn rmdir(&self, _cx: &Caller, _dir: Ino, _name: &[u8]) -> SysResult<()> {
        Err(Errno::EPERM)
    }

    fn rename(&self, _cx: &Caller, _o: Ino, _on: &[u8], _n: Ino, _nn: &[u8], _f: RenameFlags) -> SysResult<()> {
        Err(Errno::EPERM)
    }

    fn setattr(&self, _cx: &Caller, _ino: Ino, _a: &SetAttr) -> SysResult<()> {
        Err(Errno::EPERM)
    }

    fn open(self: Arc<Self>, cx: &Caller, i: Ino, _flags: OFlags) -> SysResult<Box<dyn FileHandle>> {
        let (node, _, _, _) = self.node(i)?;
        match node {
            Node::Dir => Ok(Box::new(ProcDir { fs: self, ino: i })),
            Node::File => {
                let data = self.content(cx, i)?;
                Ok(Box::new(ProcFile { data: Arc::from(data) }))
            }
            Node::Link => Err(Errno::ELOOP),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct ProcFile {
    data: Arc<[u8]>,
}

impl FileHandle for ProcFile {
    fn read(&self, _cx: &Caller, off: u64, buf: &mut [u8]) -> SysResult<usize> {
        let off = off.min(self.data.len() as u64) as usize;
        let n = buf.len().min(self.data.len() - off);
        buf[..n].copy_from_slice(&self.data[off..off + n]);
        Ok(n)
    }

    fn write(&self, _cx: &Caller, _pos: WritePos, _buf: &[u8]) -> SysResult<(usize, u64)> {
        Err(Errno::EINVAL)
    }

    fn size(&self, _cx: &Caller) -> SysResult<u64> {
        Ok(0)
    }

    fn seek_end_allowed(&self) -> bool {
        false
    }
}

struct ProcDir {
    fs: Arc<Procfs>,
    ino: Ino,
}

impl ProcDir {
    fn entries(&self) -> SysResult<Vec<DirEntry>> {
        let (kind, pid, n) = split(self.ino);
        let parent = self.fs.parent(self.ino)?;
        let mut out = vec![
            DirEntry { ino: self.ino, kind: FileType::Directory, name: b".".to_vec() },
            DirEntry { ino: parent, kind: FileType::Directory, name: b"..".to_vec() },
        ];
        match (kind, n) {
            (KIND_STATIC, 1) => {
                for (nm, t) in TOP_ENTRIES {
                    let k = if *t == Top::Sys { FileType::Directory } else { FileType::Symlink };
                    out.push(DirEntry { ino: *t as Ino, kind: k, name: nm.as_bytes().to_vec() });
                }
                for p in self.fs.provider.pids() {
                    out.push(DirEntry { ino: ino(KIND_PID, p, 0), kind: FileType::Directory, name: p.to_string().into_bytes() });
                }
            }
            (KIND_STATIC, 5) => out.push(DirEntry { ino: Top::SysKernel as Ino, kind: FileType::Directory, name: b"kernel".to_vec() }),
            (KIND_STATIC, 6) => out.push(DirEntry { ino: Top::PidMax as Ino, kind: FileType::Regular, name: b"pid_max".to_vec() }),
            (KIND_PID, 0) => {
                for (nm, e) in PID_ENTRIES {
                    let k = match e {
                        PidEntry::Fd => FileType::Directory,
                        PidEntry::Cwd | PidEntry::Exe | PidEntry::Root => FileType::Symlink,
                        _ => FileType::Regular,
                    };
                    out.push(DirEntry { ino: ino(KIND_PID, pid, *e as u32), kind: k, name: nm.as_bytes().to_vec() });
                }
            }
            (KIND_PID, 1) => {
                for fd in self.fs.provider.fds(pid).ok_or(Errno::ENOENT)? {
                    out.push(DirEntry { ino: ino(KIND_FD, pid, fd as u32), kind: FileType::Symlink, name: fd.to_string().into_bytes() });
                }
            }
            _ => return Err(Errno::ENOTDIR),
        }
        Ok(out)
    }
}

impl FileHandle for ProcDir {
    fn read(&self, _cx: &Caller, _off: u64, _buf: &mut [u8]) -> SysResult<usize> {
        Err(Errno::EISDIR)
    }

    fn write(&self, _cx: &Caller, _pos: WritePos, _buf: &[u8]) -> SysResult<(usize, u64)> {
        Err(Errno::EISDIR)
    }

    fn readdir(&self, _cx: &Caller, cookie: u64, max: usize) -> SysResult<(Vec<DirEntry>, u64)> {
        let all = self.entries()?;
        let start = (cookie as usize).min(all.len());
        let end = (start + max).min(all.len());
        Ok((all[start..end].to_vec(), end as u64))
    }

    fn size(&self, _cx: &Caller) -> SysResult<u64> {
        Ok(0)
    }
}
