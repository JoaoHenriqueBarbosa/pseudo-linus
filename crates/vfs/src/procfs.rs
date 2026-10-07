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
use std::sync::{Arc, Mutex, OnceLock, Weak};

use crate::fs::{FileHandle, FileSystem, Link, NewNode, SetAttr, WritePos};
use crate::mount::{Loc, Namespace};
use crate::namei;
use crate::types::*;

mod data;
mod maps;
mod net;
mod render;
mod sysctl;

pub use data::*;
pub use maps::{MapFiles, MappedFile};

/// `PROC_SUPER_MAGIC`.
pub const PROC_SUPER_MAGIC: u64 = 0x9fa0;

const ROOT: Ino = 1;
const KIND_STATIC: u64 = 0;
const KIND_PID: u64 = 1;
const KIND_FD: u64 = 2;
const KIND_TASK: u64 = 3;
const KIND_FDINFO: u64 = 4;
/// `/proc/<pid>/net/...`: o número é o índice na tabela de `net.rs`.
const KIND_NET: u64 = 5;

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
const S_SYS_FS: u32 = 15;
const S_SYS_VM: u32 = 16;
const S_SYS_NET: u32 = 17;
const S_SYS_NET_CORE: u32 = 18;
const S_OSTYPE: u32 = 19;
const S_OSRELEASE: u32 = 20;
const S_HOSTNAME: u32 = 21;
const S_DOMAINNAME: u32 = 22;
const S_KVERSION: u32 = 23;
const S_THREADS_MAX: u32 = 24;
const S_FILE_MAX: u32 = 25;
const S_OVERCOMMIT: u32 = 26;
const S_SWAPPINESS: u32 = 27;
const S_SOMAXCONN: u32 = 28;
const S_VMSTAT: u32 = 29;
const S_DISKSTATS: u32 = 30;
const S_SLABINFO: u32 = 31;
const S_NET: u32 = 32;

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
    Static { n: S_SYS_FS, name: "fs", parent: S_SYS, shape: Shape::Dir, mode: 0o555 },
    Static { n: S_SYS_VM, name: "vm", parent: S_SYS, shape: Shape::Dir, mode: 0o555 },
    Static { n: S_SYS_NET, name: "net", parent: S_SYS, shape: Shape::Dir, mode: 0o555 },
    Static { n: S_SYS_NET_CORE, name: "core", parent: S_SYS_NET, shape: Shape::Dir, mode: 0o555 },
    Static { n: S_OSTYPE, name: "ostype", parent: S_SYS_KERNEL, shape: Shape::File, mode: 0o444 },
    Static { n: S_OSRELEASE, name: "osrelease", parent: S_SYS_KERNEL, shape: Shape::File, mode: 0o444 },
    Static { n: S_HOSTNAME, name: "hostname", parent: S_SYS_KERNEL, shape: Shape::File, mode: 0o644 },
    Static { n: S_DOMAINNAME, name: "domainname", parent: S_SYS_KERNEL, shape: Shape::File, mode: 0o644 },
    Static { n: S_KVERSION, name: "version", parent: S_SYS_KERNEL, shape: Shape::File, mode: 0o444 },
    Static { n: S_THREADS_MAX, name: "threads-max", parent: S_SYS_KERNEL, shape: Shape::File, mode: 0o644 },
    Static { n: S_FILE_MAX, name: "file-max", parent: S_SYS_FS, shape: Shape::File, mode: 0o644 },
    Static { n: S_OVERCOMMIT, name: "overcommit_memory", parent: S_SYS_VM, shape: Shape::File, mode: 0o644 },
    Static { n: S_SWAPPINESS, name: "swappiness", parent: S_SYS_VM, shape: Shape::File, mode: 0o644 },
    Static { n: S_SOMAXCONN, name: "somaxconn", parent: S_SYS_NET_CORE, shape: Shape::File, mode: 0o644 },
    Static { n: S_VMSTAT, name: "vmstat", parent: 1, shape: Shape::File, mode: 0o444 },
    Static { n: S_DISKSTATS, name: "diskstats", parent: 1, shape: Shape::File, mode: 0o444 },
    Static { n: S_SLABINFO, name: "slabinfo", parent: 1, shape: Shape::File, mode: 0o400 },
    Static { n: S_NET, name: "net", parent: 1, shape: Shape::Link, mode: 0o777 },
];

fn static_ent(n: u32) -> Option<&'static Static> {
    STATICS.iter().find(|s| s.n == n)
}

/// A entrada fica em `/proc/sys` (é um sysctl: dono root, permissão só pelos bits de modo, `chmod` dá
/// EPERM).
fn is_sysctl(n: u32) -> bool {
    let mut cur = n;
    while let Some(s) = static_ent(cur) {
        if cur == S_SYS {
            return true;
        }
        cur = s.parent;
    }
    false
}

/// `ptrace_may_access(PTRACE_MODE_READ)`: o root ou o dono do processo. É o que o kernel exige pra abrir
/// `maps` e `smaps` e pra mostrar os endereços do `stat`.
fn may_read_mm(cx: &Caller, owner: Uid) -> bool {
    cx.cred.is_root() || cx.cred.uid == owner
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
    Maps = 21,
    Smaps = 22,
    OomScore = 23,
    OomScoreAdj = 24,
    Mountinfo = 25,
    Net = 26,
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
            21 => Ent::Maps,
            22 => Ent::Smaps,
            23 => Ent::OomScore,
            24 => Ent::OomScoreAdj,
            25 => Ent::Mountinfo,
            26 => Ent::Net,
            _ => return None,
        })
    }
}

/// `/proc/<pid>`, na ordem de `tgid_base_stuff` (só o que existe aqui).
const PID_ENTRIES: &[(&str, Ent)] = &[
    ("task", Ent::Task),
    ("fd", Ent::Fd),
    ("fdinfo", Ent::Fdinfo),
    ("net", Ent::Net),
    ("environ", Ent::Environ),
    ("status", Ent::Status),
    ("limits", Ent::Limits),
    ("comm", Ent::Comm),
    ("cmdline", Ent::Cmdline),
    ("stat", Ent::Stat),
    ("statm", Ent::Statm),
    ("maps", Ent::Maps),
    ("cwd", Ent::Cwd),
    ("root", Ent::Root),
    ("exe", Ent::Exe),
    ("mounts", Ent::Mounts),
    ("mountinfo", Ent::Mountinfo),
    ("smaps", Ent::Smaps),
    ("wchan", Ent::Wchan),
    ("schedstat", Ent::Schedstat),
    ("cpuset", Ent::Cpuset),
    ("cgroup", Ent::Cgroup),
    ("sessionid", Ent::Sessionid),
    ("oom_score", Ent::OomScore),
    ("oom_score_adj", Ent::OomScoreAdj),
];

/// `/proc/<pid>/task/<tid>`, na ordem de `tid_base_stuff` (tem `children` e não tem `task`).
const TASK_ENTRIES: &[(&str, Ent)] = &[
    ("fd", Ent::Fd),
    ("fdinfo", Ent::Fdinfo),
    ("net", Ent::Net),
    ("environ", Ent::Environ),
    ("status", Ent::Status),
    ("limits", Ent::Limits),
    ("comm", Ent::Comm),
    ("cmdline", Ent::Cmdline),
    ("stat", Ent::Stat),
    ("statm", Ent::Statm),
    ("maps", Ent::Maps),
    ("children", Ent::Children),
    ("cwd", Ent::Cwd),
    ("root", Ent::Root),
    ("exe", Ent::Exe),
    ("mounts", Ent::Mounts),
    ("mountinfo", Ent::Mountinfo),
    ("smaps", Ent::Smaps),
    ("wchan", Ent::Wchan),
    ("schedstat", Ent::Schedstat),
    ("cpuset", Ent::Cpuset),
    ("cgroup", Ent::Cgroup),
    ("sessionid", Ent::Sessionid),
    ("oom_score", Ent::OomScore),
    ("oom_score_adj", Ent::OomScoreAdj),
];

/// Forma e permissão de uma entrada de processo.
fn ent_shape(e: Ent) -> (Shape, Mode) {
    match e {
        Ent::Dir | Ent::Task | Ent::Fdinfo | Ent::Net => (Shape::Dir, 0o555),
        Ent::Fd => (Shape::Dir, 0o500),
        Ent::Cwd | Ent::Root | Ent::Exe => (Shape::Link, 0o777),
        Ent::Environ => (Shape::File, 0o400),
        Ent::Comm | Ent::OomScoreAdj => (Shape::File, 0o644),
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
    /// Os parâmetros graváveis de `/proc/sys` que não moram no kernel.
    tunables: Mutex<sysctl::Tunables>,
    /// O `oom_score_adj` gravado de cada processo (o padrão é 0).
    oom_adj: Mutex<std::collections::HashMap<Pid, i32>>,
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
        Arc::new(Procfs { dev, provider, ns: OnceLock::new(), boot, tunables: Mutex::new(sysctl::Tunables::default()),
            oom_adj: Mutex::new(std::collections::HashMap::new()),
        })
    }

    fn oom_adj_of(&self, pid: Pid) -> i32 {
        self.oom_adj.lock().unwrap_or_else(|e| e.into_inner()).get(&pid).copied().unwrap_or(0)
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
            KIND_NET => {
                self.provider.owner(pid).ok_or(Errno::ENOENT)?;
                let e = net::ent(n).ok_or(Errno::ENOENT)?;
                Ok(NodeInfo { shape: if e.dir { Shape::Dir } else { Shape::File }, mode: e.mode, uid: 0, gid: 0 })
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
            KIND_PID | KIND_TASK if (if kind == KIND_PID { n } else { n & 0xff }) == Ent::Net as u32 => {
                2 + net::children(0).into_iter().filter(|&c| net::ent(c).is_some_and(|e| e.dir)).count() as u64
            }
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

    /// `/proc/<pid>/mountinfo`: id, id do pai, `maj:min`, raiz dentro do sistema de arquivos, ponto de
    /// montagem, opções genéricas, `-`, tipo, origem e opções do sistema de arquivos. Sem campos
    /// opcionais (nenhuma montagem do sandbox tem propagação compartilhada).
    fn mountinfo_text(&self, cx: &Caller) -> Vec<u8> {
        fn mangle(p: &[u8], out: &mut Vec<u8>) {
            for &b in p {
                if matches!(b, b' ' | b'\t' | b'\n' | b'\\') {
                    out.extend_from_slice(format!("\\{b:03o}").as_bytes());
                } else {
                    out.push(b);
                }
            }
        }
        let mut out = Vec::new();
        let Some(ns) = self.ns() else { return out };
        for m in ns.mounts() {
            let (parent_id, point) = match &m.parent {
                None => (m.id, b"/".to_vec()),
                Some((pm, mp)) => (
                    pm.id,
                    namei::d_path(&Loc { mnt: pm.clone(), ino: *mp }, &cx.root).unwrap_or_else(|_| b"/".to_vec()),
                ),
            };
            let dev = m.fs.dev();
            out.extend_from_slice(format!("{} {} {}:{} / ", m.id, parent_id, dev_major(dev), dev_minor(dev)).as_bytes());
            mangle(&point, &mut out);
            out.push(b' ');
            out.extend_from_slice(m.flags.describe().as_bytes());
            out.extend_from_slice(b" - ");
            out.extend_from_slice(m.fs.fs_type().as_bytes());
            out.push(b' ');
            mangle(m.source.as_bytes(), &mut out);
            out.push(b' ');
            out.extend_from_slice(if m.read_only() { b"ro" } else { b"rw" });
            if !m.fs_options.is_empty() {
                out.push(b',');
                out.extend_from_slice(m.fs_options.as_bytes());
            }
            out.push(b'\n');
        }
        out
    }

    fn tunables(&self) -> std::sync::MutexGuard<'_, sysctl::Tunables> {
        self.tunables.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Conteúdo de um arquivo fixo.
    fn static_content(&self, n: u32) -> SysResult<Vec<u8>> {
        let line = |mut v: Vec<u8>| -> SysResult<Vec<u8>> {
            v.push(b'\n');
            Ok(v)
        };
        let t = *self.tunables();
        match n {
            S_PID_MAX => Ok(format!("{}\n", t.pid_max.unwrap_or(u64::from(self.provider.pid_max()))).into_bytes()),
            S_MEMINFO => Ok(render::meminfo(&self.provider.mem())),
            S_CPUINFO => Ok(render::cpuinfo(self.provider.ncpus())),
            S_STAT => Ok(render::stat_global(&self.provider.system())),
            S_UPTIME => Ok(render::uptime(&self.provider.system())),
            S_LOADAVG => Ok(render::loadavg(&self.provider.system())),
            S_VERSION => Ok(self.provider.version()),
            S_FILESYSTEMS => Ok(render::FILESYSTEMS.as_bytes().to_vec()),
            S_VMSTAT => Ok(render::vmstat(&self.provider.mem())),
            S_DISKSTATS => Ok(Vec::new()),
            S_SLABINFO => Ok(render::SLABINFO.as_bytes().to_vec()),
            S_OSTYPE => Ok(b"Linux\n".to_vec()),
            S_OSRELEASE => line(self.provider.os_release()),
            S_KVERSION => line(self.provider.kernel_version()),
            S_HOSTNAME => line(self.provider.hostname()),
            S_DOMAINNAME => line(self.provider.domainname()),
            S_THREADS_MAX => {
                let v = t.threads_max.unwrap_or_else(|| sysctl::default_threads_max(self.provider.mem().total));
                Ok(format!("{v}\n").into_bytes())
            }
            S_FILE_MAX => Ok(format!("{}\n", t.file_max).into_bytes()),
            S_OVERCOMMIT => Ok(format!("{}\n", t.overcommit_memory).into_bytes()),
            S_SWAPPINESS => Ok(format!("{}\n", t.swappiness).into_bytes()),
            S_SOMAXCONN => Ok(format!("{}\n", t.somaxconn).into_bytes()),
            _ => Err(Errno::EINVAL),
        }
    }

    /// Escrita num sysctl (o handler de cada `ctl_table`). Quem chama já garantiu que é root e que o
    /// modo tem o bit de escrita.
    fn sysctl_write(&self, n: u32, buf: &[u8]) -> SysResult<()> {
        let int_max = i64::from(i32::MAX);
        match n {
            S_HOSTNAME => return self.provider.set_hostname(&sysctl::uts_value(buf)),
            S_DOMAINNAME => return self.provider.set_domainname(&sysctl::uts_value(buf)),
            _ => {}
        }
        let mut t = self.tunables();
        match n {
            S_PID_MAX => t.pid_max = Some(sysctl::parse_int(buf, sysctl::PID_MAX_MIN, sysctl::PID_MAX_LIMIT)?),
            S_THREADS_MAX => t.threads_max = Some(sysctl::parse_int(buf, 1, sysctl::THREADS_MAX_LIMIT)?),
            S_FILE_MAX => t.file_max = sysctl::parse_ulong(buf, 0, i64::MAX as u64)?,
            S_OVERCOMMIT => t.overcommit_memory = sysctl::parse_int(buf, 0, 2)?,
            S_SWAPPINESS => t.swappiness = sysctl::parse_int(buf, 0, 200)?,
            S_SOMAXCONN => t.somaxconn = sysctl::parse_int(buf, 0, int_max)?,
            _ => return Err(Errno::EINVAL),
        }
        Ok(())
    }

    /// Um arquivo que o mapa mostra, procurado a partir da raiz de quem lê; sem ele no sandbox, o
    /// dispositivo do binário e um inode estável tirado do caminho.
    fn mapped_file(&self, cx: &Caller, path: &[u8], fallback_dev: (u32, u32)) -> MappedFile {
        let found = self.ns().and_then(|ns| {
            let w = namei::Walker::new(&ns, cx);
            let mut cur = cx.root.clone();
            for c in path.split(|b| *b == b'/').filter(|c| !c.is_empty()) {
                cur = w.lookup_child(&cur, c).ok()?;
            }
            w.getattr(&cur).ok()
        });
        match found {
            Some(st) => MappedFile { path: path.to_vec(), dev: (dev_major(st.dev), dev_minor(st.dev)), ino: st.ino },
            None => MappedFile { path: path.to_vec(), dev: fallback_dev, ino: maps::synthetic_ino(path) },
        }
    }

    /// O espaço de endereçamento de um processo (`None` num zumbi).
    fn layout(&self, cx: &Caller, p: &ProcData) -> Option<maps::Layout> {
        p.mem?;
        let exe = match (&p.exe, self.ns()) {
            (Some(loc), Some(ns)) => {
                let path = ns.fd_path(cx, loc);
                let (dev, ino) = loc.fs().getattr(cx, loc.ino).map_or((0, 0), |st| (st.dev, st.ino));
                MappedFile { path, dev: (dev_major(dev), dev_minor(dev)), ino }
            }
            _ => {
                let mut path = b"/usr/bin/".to_vec();
                path.extend_from_slice(&p.comm);
                let ino = maps::synthetic_ino(&path);
                MappedFile { path, dev: (0, 0), ino }
            }
        };
        let libc = self.mapped_file(cx, maps::LIBC_PATH, exe.dev);
        let ld = self.mapped_file(cx, maps::LD_PATH, exe.dev);
        maps::layout(p, &MapFiles { exe, libc, ld })
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
            Some(Ent::Mountinfo) => Ok(self.mountinfo_text(cx)),
            Some(Ent::Stat) => {
                let addrs = if may_read_mm(cx, p.uid) { self.layout(cx, &p).map(|l| l.addrs) } else { None };
                Ok(render::stat(&p, addrs.as_ref()))
            }
            Some(Ent::Maps) => Ok(maps::maps(self.layout(cx, &p).as_ref())),
            Some(Ent::Smaps) => Ok(maps::smaps(self.layout(cx, &p).as_ref())),
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
            Some(Ent::OomScore) => Ok(b"0\n".to_vec()),
            Some(Ent::OomScoreAdj) => Ok(format!("{}\n", self.oom_adj_of(pid)).into_bytes()),
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
            KIND_NET => net::content(n, self.provider.ncpus()).ok_or(Errno::EINVAL),
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
                Some(Ent::Net) => net::child(0, name).map(|c| ino(KIND_NET, pid, c)).ok_or(Errno::ENOENT),
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
                    Some(Ent::Net) => net::child(0, name).map(|c| ino(KIND_NET, pid, c)).ok_or(Errno::ENOENT),
                    _ => Err(Errno::ENOENT),
                }
            }
            KIND_NET => {
                if !net::ent(n).is_some_and(|e| e.dir) {
                    return Err(Errno::ENOTDIR);
                }
                net::child(n, name).map(|c| ino(KIND_NET, pid, c)).ok_or(Errno::ENOENT)
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
            KIND_NET => match net::ent(n).map(|e| e.parent) {
                Some(p) if p != 0 => ino(KIND_NET, pid, p),
                _ => ino(KIND_PID, pid, Ent::Net as u32),
            },
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
            KIND_NET => {
                let e = net::ent(n)?;
                let dir = if e.parent == 0 { ino(KIND_PID, pid, Ent::Net as u32) } else { ino(KIND_NET, pid, e.parent) };
                Some((dir, e.name.as_bytes().to_vec()))
            }
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
                S_NET => Ok(b"self/net".to_vec()),
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

    fn setattr(&self, _cx: &Caller, _i: Ino, a: &SetAttr) -> SysResult<()> {
        // `proc_setattr`/`proc_sys_setattr`: modo (e dono, que aqui não muda) dão EPERM; o tamanho do
        // `O_TRUNC` e os carimbos passam sem efeito, senão o `sysctl -w` (que abre com `O_TRUNC`) nem
        // chegaria a escrever, e um `> /proc/self/stat` não chegaria ao EINVAL da escrita.
        if a.mode.is_none() && a.uid.is_none() && a.gid.is_none() {
            return Ok(());
        }
        Err(Errno::EPERM)
    }

    fn open(self: Arc<Self>, cx: &Caller, i: Ino, flags: OFlags) -> SysResult<Box<dyn FileHandle>> {
        let info = self.node(i)?;
        match info.shape {
            Shape::Dir => Ok(Box::new(ProcDir { fs: self, ino: i })),
            Shape::File => {
                let (kind, _, n) = split(i);
                if kind == KIND_STATIC {
                    let in_sys = is_sysctl(n);
                    // Os arquivos fixos (entradas do `proc_create`) recusam abrir pra escrita, mesmo pro
                    // root. Nos sysctls vale só o modo, sem o privilégio do root passar por cima
                    // (`test_perm` olha os bits do dono quando é root, os de outros quando não é): um
                    // 0444 dá EACCES até pro root.
                    if flags.writable() {
                        let bits = if cx.cred.is_root() { info.mode >> 6 } else { info.mode };
                        if !in_sys || bits & 0o2 == 0 {
                            return Err(Errno::EACCES);
                        }
                    }
                    if in_sys {
                        let data = self.content(cx, i)?;
                        return Ok(Box::new(SysctlFile { fs: self, n, data: Arc::from(data) }));
                    }
                }
                if kind == KIND_PID || kind == KIND_TASK {
                    let e = Ent::from_u32(if kind == KIND_PID { n } else { n & 0xff });
                    // `proc_mem_open` do `maps` e do `smaps`: só quem pode inspecionar o processo.
                    if matches!(e, Some(Ent::Maps | Ent::Smaps)) && !may_read_mm(cx, info.uid) {
                        return Err(Errno::EACCES);
                    }
                }
                if kind == KIND_PID || kind == KIND_TASK {
                    let e = Ent::from_u32(if kind == KIND_PID { n } else { n & 0xff });
                    if matches!(e, Some(Ent::OomScoreAdj)) {
                        let (_, pid, _) = split(i);
                        let data = self.content(cx, i)?;
                        return Ok(Box::new(OomAdjFile { fs: self, pid, data: Arc::from(data) }));
                    }
                }
                // Os do `/proc/net` não têm escrita: o open pra escrever dá EACCES até pro root.
                if kind == KIND_NET && flags.writable() {
                    return Err(Errno::EACCES);
                }
                // Os de processo abrem e a escrita dá EINVAL.
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

/// `/proc/<pid>/oom_score_adj`: grava o ajuste, de -1000 a 1000 (fora disso o kernel dá EINVAL).
struct OomAdjFile {
    fs: Arc<Procfs>,
    pid: Pid,
    data: Arc<[u8]>,
}

impl FileHandle for OomAdjFile {
    fn read(&self, _cx: &Caller, off: u64, buf: &mut [u8]) -> SysResult<usize> {
        let off = off.min(self.data.len() as u64) as usize;
        let n = buf.len().min(self.data.len() - off);
        buf[..n].copy_from_slice(&self.data[off..off + n]);
        Ok(n)
    }

    fn write(&self, _cx: &Caller, _pos: WritePos, buf: &[u8]) -> SysResult<(usize, u64)> {
        let text = std::str::from_utf8(buf).map_err(|_| Errno::EINVAL)?;
        let v: i32 = text.trim().parse().map_err(|_| Errno::EINVAL)?;
        if !(-1000..=1000).contains(&v) {
            return Err(Errno::EINVAL);
        }
        self.fs.oom_adj.lock().unwrap_or_else(|e| e.into_inner()).insert(self.pid, v);
        Ok((buf.len(), buf.len() as u64))
    }

    fn size(&self, _cx: &Caller) -> SysResult<u64> {
        Ok(0)
    }

    fn seek_end_allowed(&self) -> bool {
        false
    }
}

/// Um arquivo de `/proc/sys` aberto: lê o valor do momento da abertura e escreve pelo handler da
/// entrada.
struct SysctlFile {
    fs: Arc<Procfs>,
    n: u32,
    data: Arc<[u8]>,
}

impl FileHandle for SysctlFile {
    fn read(&self, _cx: &Caller, off: u64, buf: &mut [u8]) -> SysResult<usize> {
        let off = off.min(self.data.len() as u64) as usize;
        let n = buf.len().min(self.data.len() - off);
        buf[..n].copy_from_slice(&self.data[off..off + n]);
        Ok(n)
    }

    /// `sysctl_writes_strict` = 1 (o padrão): o valor tem que vir inteiro numa escrita no início do
    /// arquivo. Num número, a escrita fora da posição 0 é ignorada e devolve 0 bytes; numa string ela
    /// é aceita sem mudar nada.
    fn write(&self, _cx: &Caller, pos: WritePos, buf: &[u8]) -> SysResult<(usize, u64)> {
        let off = match pos {
            WritePos::At(o) => o,
            WritePos::Append => 0,
        };
        if off != 0 {
            let string = matches!(self.n, S_HOSTNAME | S_DOMAINNAME);
            return Ok(if string { (buf.len(), off + buf.len() as u64) } else { (0, off) });
        }
        self.fs.sysctl_write(self.n, buf)?;
        Ok((buf.len(), buf.len() as u64))
    }

    fn size(&self, _cx: &Caller) -> SysResult<u64> {
        Ok(0)
    }

    fn seek_end_allowed(&self) -> bool {
        false
    }
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
                // Os diretórios de `/proc/sys` listam em ordem de nome (a árvore rubro-negra do sysctl).
                let mut kids: Vec<&Static> = STATICS.iter().filter(|s| s.parent == n).collect();
                kids.sort_by(|a, b| a.name.cmp(b.name));
                for s in kids {
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
            KIND_PID | KIND_TASK if (if kind == KIND_PID { n } else { n & 0xff }) == Ent::Net as u32 => {
                for c in net::children(0) {
                    let e = net::ent(c).expect("índice da tabela");
                    out.push(dirent(ino(KIND_NET, pid, c), if e.dir { Shape::Dir } else { Shape::File }, e.name.as_bytes()));
                }
            }
            KIND_NET if net::ent(n).is_some_and(|e| e.dir) => {
                for c in net::children(n) {
                    let e = net::ent(c).expect("índice da tabela");
                    out.push(dirent(ino(KIND_NET, pid, c), if e.dir { Shape::Dir } else { Shape::File }, e.name.as_bytes()));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn path_of(n: u32) -> String {
        let mut parts = Vec::new();
        let mut cur = n;
        while let Some(s) = static_ent(cur) {
            parts.push(s.name);
            cur = s.parent;
        }
        parts.reverse();
        parts.join("/")
    }

    #[test]
    fn sysctl_tree_has_the_linux_modes() {
        let modes: Vec<(String, Mode)> =
            STATICS.iter().filter(|s| is_sysctl(s.n) && s.shape == Shape::File).map(|s| (path_of(s.n), s.mode)).collect();
        let want = [
            ("sys/kernel/pid_max", 0o644),
            ("sys/kernel/ostype", 0o444),
            ("sys/kernel/osrelease", 0o444),
            ("sys/kernel/hostname", 0o644),
            ("sys/kernel/domainname", 0o644),
            ("sys/kernel/version", 0o444),
            ("sys/kernel/threads-max", 0o644),
            ("sys/fs/file-max", 0o644),
            ("sys/vm/overcommit_memory", 0o644),
            ("sys/vm/swappiness", 0o644),
            ("sys/net/core/somaxconn", 0o644),
        ];
        for (p, m) in want {
            assert!(modes.iter().any(|(q, n)| q == p && *n == m), "{p} {m:o}: {modes:?}");
        }
        assert_eq!(modes.len(), want.len());
        assert!(!is_sysctl(S_MEMINFO) && !is_sysctl(S_VERSION) && is_sysctl(S_SYS_NET_CORE));
    }

    #[test]
    fn maps_and_smaps_sit_where_the_kernel_tables_put_them() {
        let pos = |t: &[(&str, Ent)], nm: &str| t.iter().position(|(n, _)| *n == nm).unwrap();
        assert_eq!(pos(PID_ENTRIES, "maps"), pos(PID_ENTRIES, "statm") + 1);
        assert_eq!(pos(PID_ENTRIES, "mountinfo"), pos(PID_ENTRIES, "mounts") + 1);
        assert_eq!(pos(PID_ENTRIES, "smaps"), pos(PID_ENTRIES, "mountinfo") + 1);
        assert_eq!(pos(TASK_ENTRIES, "maps") + 1, pos(TASK_ENTRIES, "children"));
        assert_eq!(pos(TASK_ENTRIES, "smaps"), pos(TASK_ENTRIES, "mountinfo") + 1);
        assert_eq!(ent_shape(Ent::Maps), (Shape::File, 0o444));
        assert_eq!(ent_shape(Ent::Smaps), (Shape::File, 0o444));
        assert_eq!(Ent::from_u32(Ent::Smaps as u32), Some(Ent::Smaps));
    }
}
