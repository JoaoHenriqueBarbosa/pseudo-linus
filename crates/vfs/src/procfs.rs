//! O `/proc`. Os dados de processo e da máquina vêm do kernel por [`ProcProvider`]; aqui ficam a árvore,
//! os números de inode, os magic links (`/proc/<pid>/{cwd,exe,root,fd/N}`) e a geração do conteúdo (os
//! formatos de texto estão em `procfs/render.rs`).
//!
//! Números de inode: bits 63..56 dizem o tipo da entrada, 55..32 o pid e 31..0 o número da entrada. Em
//! `/proc/<pid>/task/<tid>/...` o número leva o tid nos bits 31..8 e a entrada nos 7..0. O conteúdo de
//! um arquivo é gerado no `open` (como o `seq_file` gera na primeira leitura) e fica no handle; `st_size`
//! é 0, como no Linux.
//!
//! A ordem do `readdir` é a do kernel: na raiz as entradas fixas por tamanho de nome e depois ordem de
//! bytes (a ordem da árvore de `proc_dir_entry`), então `self`, `thread-self` e os pids; dentro de um
//! processo a ordem das tabelas `tgid_base_stuff` e `tid_base_stuff`.

use std::any::Any;
use std::sync::{Arc, OnceLock, Weak};

use crate::fs::{FileHandle, FileSystem, Link, NewNode, SetAttr, WritePos};
use crate::mount::{Loc, Namespace};
use crate::namei;
use crate::types::*;

mod data;
mod render;

pub use data::*;

/// `PROC_SUPER_MAGIC`.
pub const PROC_SUPER_MAGIC: u64 = 0x9fa0;

const ROOT: Ino = 1;
const KIND_STATIC: u64 = 0;
const KIND_PID: u64 = 1;
const KIND_FD: u64 = 2;
const KIND_TASK: u64 = 3;
const KIND_FDINFO: u64 = 4;

fn ino(kind: u64, pid: Pid, n: u32) -> Ino {
    (kind << 56) | ((pid as u64 & 0xff_ffff) << 32) | n as u64
}

fn split(ino: Ino) -> (u64, Pid, u32) {
    (ino >> 56, ((ino >> 32) & 0xff_ffff) as Pid, (ino & 0xffff_ffff) as u32)
}

/// Número de uma entrada de `/proc/<pid>/task/<tid>`: o tid e a entrada.
fn task_ino(pid: Pid, tid: Pid, e: Ent) -> Ino {
    ino(KIND_TASK, pid, ((tid as u32) << 8) | e as u32)
}

/// Forma de um nó.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Dir,
    File,
    Link,
}

fn file_type(s: Shape) -> FileType {
    match s {
        Shape::Dir => FileType::Directory,
        Shape::File => FileType::Regular,
        Shape::Link => FileType::Symlink,
    }
}

/// Entrada fixa da árvore: tudo que não depende de um pid.
struct Static {
    n: u32,
    name: &'static str,
    /// Número do diretório que a contém (1 é a raiz).
    parent: u32,
    shape: Shape,
    mode: Mode,
}

const S_SELF: u32 = 2;
const S_THREAD_SELF: u32 = 3;
const S_MOUNTS: u32 = 4;
const S_SYS: u32 = 5;
const S_SYS_KERNEL: u32 = 6;
const S_PID_MAX: u32 = 7;
const S_MEMINFO: u32 = 8;
const S_CPUINFO: u32 = 9;
const S_STAT: u32 = 10;
const S_UPTIME: u32 = 11;
const S_LOADAVG: u32 = 12;
const S_VERSION: u32 = 13;
const S_FILESYSTEMS: u32 = 14;

const STATICS: &[Static] = &[
    Static { n: S_SELF, name: "self", parent: 1, shape: Shape::Link, mode: 0o777 },
    Static { n: S_THREAD_SELF, name: "thread-self", parent: 1, shape: Shape::Link, mode: 0o777 },
    Static { n: S_MOUNTS, name: "mounts", parent: 1, shape: Shape::Link, mode: 0o777 },
    Static { n: S_SYS, name: "sys", parent: 1, shape: Shape::Dir, mode: 0o555 },
    Static { n: S_SYS_KERNEL, name: "kernel", parent: S_SYS, shape: Shape::Dir, mode: 0o555 },
    Static { n: S_PID_MAX, name: "pid_max", parent: S_SYS_KERNEL, shape: Shape::File, mode: 0o644 },
    Static { n: S_MEMINFO, name: "meminfo", parent: 1, shape: Shape::File, mode: 0o444 },
    Static { n: S_CPUINFO, name: "cpuinfo", parent: 1, shape: Shape::File, mode: 0o444 },
    Static { n: S_STAT, name: "stat", parent: 1, shape: Shape::File, mode: 0o444 },
    Static { n: S_UPTIME, name: "uptime", parent: 1, shape: Shape::File, mode: 0o444 },
    Static { n: S_LOADAVG, name: "loadavg", parent: 1, shape: Shape::File, mode: 0o444 },
    Static { n: S_VERSION, name: "version", parent: 1, shape: Shape::File, mode: 0o444 },
    Static { n: S_FILESYSTEMS, name: "filesystems", parent: 1, shape: Shape::File, mode: 0o444 },
];

fn static_ent(n: u32) -> Option<&'static Static> {
    STATICS.iter().find(|s| s.n == n)
}

/// Entradas de `/proc/<pid>` e de `/proc/<pid>/task/<tid>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ent {
    /// O próprio diretório (`/proc/<pid>` ou `/proc/<pid>/task/<tid>`).
    Dir = 0,
    Task = 1,
    Fd = 2,
    Fdinfo = 3,
    Environ = 4,
    Status = 5,
    Limits = 6,
    Comm = 7,
    Cmdline = 8,
    Stat = 9,
    Statm = 10,
    Cwd = 11,
    Root = 12,
    Exe = 13,
    Mounts = 14,
    Children = 15,
    Wchan = 16,
    Schedstat = 17,
    Cpuset = 18,
    Cgroup = 19,
    Sessionid = 20,
}

impl Ent {
    fn from_u32(n: u32) -> Option<Ent> {
        Some(match n {
            0 => Ent::Dir,
            1 => Ent::Task,
            2 => Ent::Fd,
            3 => Ent::Fdinfo,
            4 => Ent::Environ,
            5 => Ent::Status,
            6 => Ent::Limits,
            7 => Ent::Comm,
            8 => Ent::Cmdline,
            9 => Ent::Stat,
            10 => Ent::Statm,
            11 => Ent::Cwd,
            12 => Ent::Root,
            13 => Ent::Exe,
            14 => Ent::Mounts,
            15 => Ent::Children,
            16 => Ent::Wchan,
            17 => Ent::Schedstat,
            18 => Ent::Cpuset,
            19 => Ent::Cgroup,
            20 => Ent::Sessionid,
            _ => return None,
        })
    }
}

/// `/proc/<pid>`, na ordem de `tgid_base_stuff` (só o que existe aqui).
const PID_ENTRIES: &[(&str, Ent)] = &[
    ("task", Ent::Task),
    ("fd", Ent::Fd),
    ("fdinfo", Ent::Fdinfo),
    ("environ", Ent::Environ),
    ("status", Ent::Status),
    ("limits", Ent::Limits),
    ("comm", Ent::Comm),
    ("cmdline", Ent::Cmdline),
    ("stat", Ent::Stat),
    ("statm", Ent::Statm),
    ("cwd", Ent::Cwd),
    ("root", Ent::Root),
    ("exe", Ent::Exe),
    ("mounts", Ent::Mounts),
    ("wchan", Ent::Wchan),
    ("schedstat", Ent::Schedstat),
    ("cpuset", Ent::Cpuset),
    ("cgroup", Ent::Cgroup),
    ("sessionid", Ent::Sessionid),
];

/// `/proc/<pid>/task/<tid>`, na ordem de `tid_base_stuff` (tem `children` e não tem `task`).
const TASK_ENTRIES: &[(&str, Ent)] = &[
    ("fd", Ent::Fd),
    ("fdinfo", Ent::Fdinfo),
    ("environ", Ent::Environ),
    ("status", Ent::Status),
    ("limits", Ent::Limits),
    ("comm", Ent::Comm),
    ("cmdline", Ent::Cmdline),
    ("stat", Ent::Stat),
    ("statm", Ent::Statm),
    ("children", Ent::Children),
    ("cwd", Ent::Cwd),
    ("root", Ent::Root),
    ("exe", Ent::Exe),
    ("mounts", Ent::Mounts),
    ("wchan", Ent::Wchan),
    ("schedstat", Ent::Schedstat),
    ("cpuset", Ent::Cpuset),
    ("cgroup", Ent::Cgroup),
    ("sessionid", Ent::Sessionid),
];

/// Forma e permissão de uma entrada de processo.
fn ent_shape(e: Ent) -> (Shape, Mode) {
    match e {
        Ent::Dir | Ent::Task | Ent::Fdinfo => (Shape::Dir, 0o555),
        Ent::Fd => (Shape::Dir, 0o500),
        Ent::Cwd | Ent::Root | Ent::Exe => (Shape::Link, 0o777),
        Ent::Environ => (Shape::File, 0o400),
        Ent::Comm => (Shape::File, 0o644),
        _ => (Shape::File, 0o444),
    }
}

/// Um número decimal de nome de arquivo: só dígitos, sem zero à esquerda (fora o próprio 0).
fn parse_num(name: &[u8]) -> Option<u32> {
    if name.is_empty() || !name.iter().all(u8::is_ascii_digit) || (name.len() > 1 && name[0] == b'0') {
        return None;
    }
    std::str::from_utf8(name).ok()?.parse().ok()
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

/// Forma, permissão e dono de um nó.
struct NodeInfo {
    shape: Shape,
    mode: Mode,
    uid: Uid,
    gid: Gid,
}

impl Procfs {
    pub fn new(dev: u64, provider: Arc<dyn ProcProvider>, boot: TimeSpec) -> Arc<Procfs> {
        Arc::new(Procfs { dev, provider, ns: OnceLock::new(), boot })
    }

    /// Liga o procfs ao namespace onde ele está montado (pra `/proc/mounts`).
    pub fn set_namespace(&self, ns: &Arc<Namespace>) {
        let _ = self.ns.set(Arc::downgrade(ns));
    }

    fn ns(&self) -> Option<Arc<Namespace>> {
        self.ns.get().and_then(Weak::upgrade)
    }

    fn ns_root(&self) -> Option<Loc> {
        self.ns().map(|n| n.root())
    }

    fn node(&self, i: Ino) -> SysResult<NodeInfo> {
        let (kind, pid, n) = split(i);
        match kind {
            KIND_STATIC => {
                if n as Ino == ROOT {
                    return Ok(NodeInfo { shape: Shape::Dir, mode: 0o555, uid: 0, gid: 0 });
                }
                let s = static_ent(n).ok_or(Errno::ENOENT)?;
                Ok(NodeInfo { shape: s.shape, mode: s.mode, uid: 0, gid: 0 })
            }
            KIND_PID | KIND_TASK => {
                let (uid, gid) = self.provider.owner(pid).ok_or(Errno::ENOENT)?;
                let e = Ent::from_u32(if kind == KIND_PID { n } else { n & 0xff }).ok_or(Errno::ENOENT)?;
                let (shape, mode) = ent_shape(e);
                Ok(NodeInfo { shape, mode, uid, gid })
            }
            KIND_FD => {
                let (uid, gid) = self.provider.owner(pid).ok_or(Errno::ENOENT)?;
                let cx = crate::ops::kernel_caller(self.ns_root().ok_or(Errno::ENOENT)?);
                let l = self.provider.fd(&cx, pid, n as i32).ok_or(Errno::ENOENT)?;
                Ok(NodeInfo { shape: Shape::Link, mode: l.perm, uid, gid })
            }
            KIND_FDINFO => {
                let (uid, gid) = self.provider.owner(pid).ok_or(Errno::ENOENT)?;
                let fds = self.provider.fds(pid).ok_or(Errno::ENOENT)?;
                if !fds.contains(&(n as i32)) {
                    return Err(Errno::ENOENT);
                }
                Ok(NodeInfo { shape: Shape::File, mode: 0o444, uid, gid })
            }
            _ => Err(Errno::ENOENT),
        }
    }

    /// `st_nlink` de um diretório: 2 mais os subdiretórios. `/proc/sys` e abaixo são sysctl, que o
    /// kernel mostra com 1.
    fn dir_nlink(&self, i: Ino) -> u64 {
        let (kind, pid, n) = split(i);
        match kind {
            KIND_STATIC if n as Ino == ROOT => 2 + 1 + self.provider.pids().len() as u64,
            KIND_STATIC => 1,
            KIND_PID if n == Ent::Dir as u32 => {
                2 + PID_ENTRIES.iter().filter(|(_, e)| ent_shape(*e).0 == Shape::Dir).count() as u64
            }
            KIND_PID if n == Ent::Task as u32 => 2 + self.provider.tids(pid).map_or(0, |t| t.len() as u64),
            KIND_TASK if n & 0xff == 0 => {
                2 + TASK_ENTRIES.iter().filter(|(_, e)| ent_shape(*e).0 == Shape::Dir).count() as u64
            }
            _ => 2,
        }
    }

    /// `st_size` de um diretório: a tabela de fds mostra quantos fds estão abertos.
    fn dir_size(&self, i: Ino) -> u64 {
        let (kind, pid, n) = split(i);
        let is_fd_dir = (kind == KIND_PID && n == Ent::Fd as u32) || (kind == KIND_TASK && n & 0xff == Ent::Fd as u32);
        if is_fd_dir { self.provider.fds(pid).map_or(0, |f| f.len() as u64) } else { 0 }
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

    /// Conteúdo de um arquivo fixo.
    fn static_content(&self, n: u32) -> SysResult<Vec<u8>> {
        match n {
            S_PID_MAX => Ok(format!("{}\n", self.provider.pid_max()).into_bytes()),
            S_MEMINFO => Ok(render::meminfo(&self.provider.system().mem)),
            S_CPUINFO => Ok(render::cpuinfo(self.provider.ncpus())),
            S_STAT => Ok(render::stat_global(&self.provider.system())),
            S_UPTIME => Ok(render::uptime(&self.provider.system())),
            S_LOADAVG => Ok(render::loadavg(&self.provider.system())),
            S_VERSION => Ok(self.provider.version()),
            S_FILESYSTEMS => Ok(render::FILESYSTEMS.as_bytes().to_vec()),
            _ => Err(Errno::EINVAL),
        }
    }

    /// Conteúdo de um arquivo de processo (`tid` é `Some` dentro de `task/<tid>`).
    fn pid_content(&self, cx: &Caller, pid: Pid, tid: Option<Pid>, e: u32) -> SysResult<Vec<u8>> {
        let p = match tid {
            None => self.provider.process(pid),
            Some(t) => self.provider.thread(pid, t),
        }
        .ok_or(Errno::ESRCH)?;
        match Ent::from_u32(e) {
            Some(Ent::Cmdline) => Ok(p.cmdline),
            Some(Ent::Environ) => Ok(p.environ),
            Some(Ent::Comm) => {
                let mut c = p.comm.clone();
                c.truncate(15);
                c.push(b'\n');
                Ok(c)
            }
            Some(Ent::Mounts) => Ok(self.mounts_text(cx)),
            Some(Ent::Stat) => Ok(render::stat(&p)),
            Some(Ent::Statm) => Ok(render::statm(&p)),
            Some(Ent::Status) => Ok(render::status(&p)),
            Some(Ent::Limits) => Ok(render::limits(&p)),
            Some(Ent::Schedstat) => Ok(render::schedstat(&p)),
            Some(Ent::Children) => {
                let kids = self.provider.children(pid, tid.unwrap_or(pid)).ok_or(Errno::ESRCH)?;
                Ok(render::children(&kids))
            }
            Some(Ent::Wchan) => Ok(b"0".to_vec()),
            Some(Ent::Cpuset) => Ok(b"/\n".to_vec()),
            Some(Ent::Cgroup) => Ok(b"0::/\n".to_vec()),
            Some(Ent::Sessionid) => Ok(b"4294967295".to_vec()),
            _ => Err(Errno::EINVAL),
        }
    }

    fn content(&self, cx: &Caller, i: Ino) -> SysResult<Vec<u8>> {
        let (kind, pid, n) = split(i);
        match kind {
            KIND_STATIC => self.static_content(n),
            KIND_PID => self.pid_content(cx, pid, None, n),
            KIND_TASK => self.pid_content(cx, pid, Some((n >> 8) as Pid), n & 0xff),
            KIND_FDINFO => {
                let info = self.provider.fdinfo(cx, pid, n as i32).ok_or(Errno::ENOENT)?;
                Ok(render::fdinfo(&info))
            }
            _ => Err(Errno::EINVAL),
        }
    }

    /// `fd/N` ou `fdinfo/N` de um processo: o fd precisa estar aberto.
    fn fd_child(&self, pid: Pid, name: &[u8], kind: u64) -> SysResult<Ino> {
        let fd = i32::try_from(parse_num(name).ok_or(Errno::ENOENT)?).map_err(|_| Errno::ENOENT)?;
        let fds = self.provider.fds(pid).ok_or(Errno::ENOENT)?;
        if !fds.contains(&fd) {
            return Err(Errno::ENOENT);
        }
        Ok(ino(kind, pid, fd as u32))
    }

    /// O pedaço de `/proc/<pid>` ou `/proc/<pid>/task/<tid>` que `readlink` e `follow_link` resolvem.
    fn proc_link_loc(&self, pid: Pid, e: Option<Ent>) -> SysResult<Loc> {
        let p = self.provider.process(pid).ok_or(Errno::ENOENT)?;
        let loc = match e {
            Some(Ent::Cwd) => p.cwd,
            Some(Ent::Exe) => p.exe,
            Some(Ent::Root) => p.root,
            _ => return Err(Errno::EINVAL),
        };
        loc.ok_or(Errno::ENOENT)
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
        let info = self.node(i).map_err(|_| Errno::ENOENT)?;
        let kind = split(i).0;
        let (tbits, nlink, size) = match info.shape {
            Shape::Dir => (S_IFDIR, self.dir_nlink(i), self.dir_size(i)),
            Shape::File => (S_IFREG, 1, 0),
            // Os links de `fd/N` mostram 64 (o tamanho de um `PATH_MAX` de texto no kernel).
            Shape::Link => (S_IFLNK, 1, if kind == KIND_FD { 64 } else { 0 }),
        };
        Ok(Stat {
            dev: self.dev,
            ino: i,
            mode: tbits | info.mode,
            nlink,
            uid: info.uid,
            gid: info.gid,
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

    fn lookup(&self, _cx: &Caller, dir: Ino, name: &[u8]) -> SysResult<Ino> {
        let (kind, pid, n) = split(dir);
        match kind {
            KIND_STATIC if n as Ino == ROOT => {
                if let Some(s) = STATICS.iter().find(|s| s.parent == 1 && s.name.as_bytes() == name) {
                    return Ok(s.n as Ino);
                }
                let pid = Pid::try_from(parse_num(name).ok_or(Errno::ENOENT)?).map_err(|_| Errno::ENOENT)?;
                if pid <= 0 || self.provider.owner(pid).is_none() {
                    return Err(Errno::ENOENT);
                }
                Ok(ino(KIND_PID, pid, Ent::Dir as u32))
            }
            KIND_STATIC => STATICS
                .iter()
                .find(|s| s.parent == n && s.name.as_bytes() == name)
                .map(|s| s.n as Ino)
                .ok_or(Errno::ENOENT),
            KIND_PID => match Ent::from_u32(n) {
                Some(Ent::Dir) => {
                    let (_, e) = PID_ENTRIES.iter().find(|(nm, _)| nm.as_bytes() == name).ok_or(Errno::ENOENT)?;
                    self.provider.owner(pid).ok_or(Errno::ENOENT)?;
                    Ok(ino(KIND_PID, pid, *e as u32))
                }
                Some(Ent::Task) => {
                    let tid = Pid::try_from(parse_num(name).ok_or(Errno::ENOENT)?).map_err(|_| Errno::ENOENT)?;
                    if tid >= 1 << 24 || !self.provider.tids(pid).ok_or(Errno::ENOENT)?.contains(&tid) {
                        return Err(Errno::ENOENT);
                    }
                    Ok(task_ino(pid, tid, Ent::Dir))
                }
                Some(Ent::Fd) => self.fd_child(pid, name, KIND_FD),
                Some(Ent::Fdinfo) => self.fd_child(pid, name, KIND_FDINFO),
                _ => Err(Errno::ENOENT),
            },
            KIND_TASK => {
                let tid = (n >> 8) as Pid;
                match Ent::from_u32(n & 0xff) {
                    Some(Ent::Dir) => {
                        let (_, e) = TASK_ENTRIES.iter().find(|(nm, _)| nm.as_bytes() == name).ok_or(Errno::ENOENT)?;
                        if !self.provider.tids(pid).ok_or(Errno::ENOENT)?.contains(&tid) {
                            return Err(Errno::ENOENT);
                        }
                        Ok(task_ino(pid, tid, *e))
                    }
                    Some(Ent::Fd) => self.fd_child(pid, name, KIND_FD),
                    Some(Ent::Fdinfo) => self.fd_child(pid, name, KIND_FDINFO),
                    _ => Err(Errno::ENOENT),
                }
            }
            _ => Err(Errno::ENOENT),
        }
    }

    fn parent(&self, dir: Ino) -> SysResult<Ino> {
        let (kind, pid, n) = split(dir);
        Ok(match kind {
            KIND_STATIC => static_ent(n).map_or(ROOT, |s| s.parent as Ino),
            KIND_PID if n == Ent::Dir as u32 => ROOT,
            KIND_PID => ino(KIND_PID, pid, Ent::Dir as u32),
            KIND_TASK if n & 0xff == 0 => ino(KIND_PID, pid, Ent::Task as u32),
            KIND_TASK => ino(KIND_TASK, pid, n & !0xff),
            KIND_FD => ino(KIND_PID, pid, Ent::Fd as u32),
            KIND_FDINFO => ino(KIND_PID, pid, Ent::Fdinfo as u32),
            _ => ROOT,
        })
    }

    fn name_of(&self, i: Ino) -> Option<(Ino, Vec<u8>)> {
        let (kind, pid, n) = split(i);
        match kind {
            KIND_STATIC => {
                let s = static_ent(n)?;
                Some((s.parent as Ino, s.name.as_bytes().to_vec()))
            }
            KIND_PID if n == Ent::Dir as u32 => Some((ROOT, pid.to_string().into_bytes())),
            KIND_PID => {
                let nm = PID_ENTRIES.iter().find(|(_, x)| *x as u32 == n)?.0;
                Some((ino(KIND_PID, pid, Ent::Dir as u32), nm.as_bytes().to_vec()))
            }
            KIND_TASK if n & 0xff == 0 => Some((ino(KIND_PID, pid, Ent::Task as u32), (n >> 8).to_string().into_bytes())),
            KIND_TASK => {
                let nm = TASK_ENTRIES.iter().find(|(_, x)| *x as u32 == n & 0xff)?.0;
                Some((ino(KIND_TASK, pid, n & !0xff), nm.as_bytes().to_vec()))
            }
            KIND_FD => Some((ino(KIND_PID, pid, Ent::Fd as u32), n.to_string().into_bytes())),
            KIND_FDINFO => Some((ino(KIND_PID, pid, Ent::Fdinfo as u32), n.to_string().into_bytes())),
            _ => None,
        }
    }

    fn readlink(&self, cx: &Caller, i: Ino) -> SysResult<Vec<u8>> {
        let (kind, pid, n) = split(i);
        match kind {
            KIND_STATIC => match n {
                S_SELF => {
                    if cx.pid <= 0 {
                        return Err(Errno::ENOENT);
                    }
                    Ok(cx.pid.to_string().into_bytes())
                }
                S_THREAD_SELF => {
                    if cx.pid <= 0 {
                        return Err(Errno::ENOENT);
                    }
                    Ok(format!("{}/task/{}", cx.pid, cx.tid).into_bytes())
                }
                S_MOUNTS => Ok(b"self/mounts".to_vec()),
                _ => Err(Errno::EINVAL),
            },
            KIND_PID | KIND_TASK => {
                let loc = self.proc_link_loc(pid, Ent::from_u32(if kind == KIND_PID { n } else { n & 0xff }))?;
                let ns = self.ns().ok_or(Errno::ENOENT)?;
                Ok(ns.fd_path(cx, &loc))
            }
            KIND_FD => Ok(self.provider.fd(cx, pid, n as i32).ok_or(Errno::ENOENT)?.text),
            _ => Err(Errno::EINVAL),
        }
    }

    fn follow_link(&self, cx: &Caller, i: Ino) -> SysResult<Link> {
        let (kind, pid, n) = split(i);
        match kind {
            KIND_PID | KIND_TASK => {
                let loc = self.proc_link_loc(pid, Ent::from_u32(if kind == KIND_PID { n } else { n & 0xff }))?;
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

    fn open(self: Arc<Self>, cx: &Caller, i: Ino, flags: OFlags) -> SysResult<Box<dyn FileHandle>> {
        let info = self.node(i)?;
        match info.shape {
            Shape::Dir => Ok(Box::new(ProcDir { fs: self, ino: i })),
            Shape::File => {
                let (kind, _, n) = split(i);
                // Os arquivos fixos (entradas do `proc_create`) recusam abrir pra escrita, mesmo pro
                // root; os de processo abrem e a escrita dá EINVAL.
                if kind == KIND_STATIC && n != S_PID_MAX && flags.writable() {
                    return Err(Errno::EACCES);
                }
                let data = self.content(cx, i)?;
                Ok(Box::new(ProcFile { data: Arc::from(data) }))
            }
            Shape::Link => Err(Errno::ELOOP),
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

fn dirent(ino: Ino, shape: Shape, name: &[u8]) -> DirEntry {
    DirEntry { ino, kind: file_type(shape), name: name.to_vec() }
}

impl ProcDir {
    /// Entradas do diretório, na ordem do kernel.
    fn entries(&self) -> SysResult<Vec<DirEntry>> {
        let (kind, pid, n) = split(self.ino);
        let parent = self.fs.parent(self.ino)?;
        let mut out = vec![
            DirEntry { ino: self.ino, kind: FileType::Directory, name: b".".to_vec() },
            DirEntry { ino: parent, kind: FileType::Directory, name: b"..".to_vec() },
        ];
        let prov = &self.fs.provider;
        match kind {
            KIND_STATIC if n as Ino == ROOT => {
                // A árvore de `proc_dir_entry` ordena por tamanho do nome e depois por bytes.
                let mut fixed: Vec<&Static> =
                    STATICS.iter().filter(|s| s.parent == 1 && s.n != S_SELF && s.n != S_THREAD_SELF).collect();
                fixed.sort_by(|a, b| (a.name.len(), a.name).cmp(&(b.name.len(), b.name)));
                for s in fixed {
                    out.push(dirent(s.n as Ino, s.shape, s.name.as_bytes()));
                }
                out.push(dirent(S_SELF as Ino, Shape::Link, b"self"));
                out.push(dirent(S_THREAD_SELF as Ino, Shape::Link, b"thread-self"));
                for p in prov.pids() {
                    out.push(dirent(ino(KIND_PID, p, Ent::Dir as u32), Shape::Dir, p.to_string().as_bytes()));
                }
            }
            KIND_STATIC => {
                for s in STATICS.iter().filter(|s| s.parent == n) {
                    out.push(dirent(s.n as Ino, s.shape, s.name.as_bytes()));
                }
            }
            KIND_PID if n == Ent::Dir as u32 => {
                for (nm, e) in PID_ENTRIES {
                    out.push(dirent(ino(KIND_PID, pid, *e as u32), ent_shape(*e).0, nm.as_bytes()));
                }
            }
            KIND_PID if n == Ent::Task as u32 => {
                for t in prov.tids(pid).ok_or(Errno::ENOENT)? {
                    out.push(dirent(task_ino(pid, t, Ent::Dir), Shape::Dir, t.to_string().as_bytes()));
                }
            }
            KIND_TASK if n & 0xff == 0 => {
                let tid = (n >> 8) as Pid;
                for (nm, e) in TASK_ENTRIES {
                    out.push(dirent(task_ino(pid, tid, *e), ent_shape(*e).0, nm.as_bytes()));
                }
            }
            KIND_PID | KIND_TASK if (if kind == KIND_PID { n } else { n & 0xff }) == Ent::Fd as u32 => {
                for fd in prov.fds(pid).ok_or(Errno::ENOENT)? {
                    out.push(dirent(ino(KIND_FD, pid, fd as u32), Shape::Link, fd.to_string().as_bytes()));
                }
            }
            KIND_PID | KIND_TASK if (if kind == KIND_PID { n } else { n & 0xff }) == Ent::Fdinfo as u32 => {
                for fd in prov.fds(pid).ok_or(Errno::ENOENT)? {
                    out.push(dirent(ino(KIND_FDINFO, pid, fd as u32), Shape::File, fd.to_string().as_bytes()));
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
