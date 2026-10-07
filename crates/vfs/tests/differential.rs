//! Teste diferencial do VFS contra o tmpfs real do host (`/dev/shm`), no método do E03.
//!
//! Cada caso é uma sequência aleatória de operações sobre um universo pequeno de caminhos (diretórios
//! aninhados, `.`, `..` que não sobe acima da base, barra no fim, barra dupla, symlinks relativos e
//! absolutos, symlink pendurado, nome de 256 bytes, caminho acima de PATH_MAX). A mesma operação roda nos
//! dois lados: no host, num diretório único em `/dev/shm` (syscalls via rustix, API segura); no nosso VFS,
//! num [`Namespace`] com tmpfs na raiz onde o mesmo caminho absoluto (`/dev/shm/vfsdiff-...`) foi criado,
//! pra que os symlinks absolutos resolvam igual. Compara-se o resultado de cada operação (errno, bytes,
//! stat, entradas do diretório na ordem) e, depois de cada passo, um retrato da árvore inteira (tipo,
//! permissões, dono, nlink, tamanho, blocos, alvo de symlink, conteúdo e ordem do readdir).
//!
//! O host não roda como root: a credencial do nosso lado é o uid, o gid e os grupos de quem roda o
//! teste, e a umask é forçada em 022 nos dois lados.
//!
//! Ordem do readdir: comparada estritamente. Conferido à mão no 6.12.101 do host que a ordem é só a da
//! criação (ou do último rename), do mais novo pro mais antigo: um lookup negativo antes da criação não
//! segura a posição, e apagar e recriar um nome o põe como o mais novo. É o que o tmpfs do VFS faz.
//!
//! Escapes: nenhum alvo de symlink tem `..` nem aponta pra base em si, então nenhuma resolução sai da
//! base (o único `..` que sobe acima dela seria o de um caminho que resolve na base, e esses alvos não
//! existem no universo).
//!
//! Rodar: `cargo test -p vfs --test differential` (casos: `VFS_DIFF_CASES`, padrão 512).

use std::collections::BTreeMap;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed, TestCaseError, TestRunner};
use rustix::fs as rfs;
use vfs::tmpfs::{Tmpfs, TmpfsLimits};
use vfs::*;

const SEED: u64 = 0x7466_5f64_6966_6631;

static CASE_COUNTER: AtomicU64 = AtomicU64::new(0);
static OPS_RUN: AtomicU64 = AtomicU64::new(0);
static CASES_RUN: AtomicU64 = AtomicU64::new(0);
/// Sucessos e contagem de cada errno de um tipo de operação.
type Tally = (u64, BTreeMap<i32, u64>);
/// Por tipo de operação.
static OUTCOMES: std::sync::Mutex<BTreeMap<&'static str, Tally>> = std::sync::Mutex::new(BTreeMap::new());

fn op_name(op: &Op) -> &'static str {
    match op {
        Op::Open { .. } => "open",
        Op::Close { .. } => "close",
        Op::Write { .. } => "write",
        Op::Read { .. } => "read",
        Op::Pwrite { .. } => "pwrite",
        Op::Pread { .. } => "pread",
        Op::Truncate { .. } => "truncate",
        Op::Mkdir { .. } => "mkdir",
        Op::Mkfifo { .. } => "mkfifo",
        Op::Unlink { .. } => "unlink",
        Op::Rmdir { .. } => "rmdir",
        Op::Rename { .. } => "rename",
        Op::Link { .. } => "link",
        Op::Symlink { .. } => "symlink",
        Op::Readlink { .. } => "readlink",
        Op::Stat { .. } => "stat",
        Op::Chmod { .. } => "chmod",
        Op::Utimens { .. } => "utimens",
        Op::Access { .. } => "access",
        Op::Readdir { .. } => "readdir",
    }
}

fn record(op: &Op, r: &Res) {
    let mut g = OUTCOMES.lock().unwrap_or_else(|p| p.into_inner());
    let entry = g.entry(op_name(op)).or_default();
    match r {
        Ok(Out::Skipped) => {}
        Ok(_) => entry.0 += 1,
        Err(e) => *entry.1.entry(*e).or_default() += 1,
    }
}

// ------------------------------------------------------------------------------------------------
// Universo
// ------------------------------------------------------------------------------------------------

fn universe() -> Vec<Vec<u8>> {
    let mut u: Vec<Vec<u8>> = [
        "a", "b", "c", "d", "a/x", "a/y", "b/x", "b/y", "c/x", "a/x/z", "a/x/w", "b/x/z", "a/", "b/", "a/x/", "c/", ".",
        "a/.", "a/..", "a/x/..", "a/x/../y", "./a", "a//x", "b/./y", "s1", "s2", "a/s3", "b/s4", "a/x/z/q", "s1/x",
        "s2/", "a/s3/z",
    ]
    .iter()
    .map(|s| s.as_bytes().to_vec())
    .collect();
    u.push(vec![b'n'; 256]);
    let mut p = b"a/".to_vec();
    p.extend(std::iter::repeat_n(b'm', 255));
    u.push(p);
    // 4100 bytes: ENAMETOOLONG antes de tudo.
    u.push(b"a/".repeat(2050));
    u
}

/// Alvos de symlink. `ABS:` vira o caminho absoluto da base no caso.
const TARGETS: &[&str] = &[
    "a", "a/x", "s1", "s2", "nonexistent", "b/y", "x", "z", "x/z", "a/x/z", "b/", "ABS:/a", "ABS:/a/x", "ABS:/b/y",
    "ABS:/nonexistent", "ABS:/s1", "",
];

/// Combinações de flags do open (valores do Linux x86_64).
const FLAG_SETS: &[u32] = &[
    0o0,        // O_RDONLY
    0o101,      // O_WRONLY | O_CREAT
    0o301,      // O_WRONLY | O_CREAT | O_EXCL
    0o1102,     // O_RDWR | O_CREAT | O_TRUNC
    0o1001,     // O_WRONLY | O_TRUNC
    0o200000,   // O_RDONLY | O_DIRECTORY
    0o400000,   // O_RDONLY | O_NOFOLLOW
    0o400101,   // O_WRONLY | O_CREAT | O_NOFOLLOW
    0o2002,     // O_RDWR | O_APPEND
    0o2101,     // O_WRONLY | O_CREAT | O_APPEND
    0o10000000, // O_PATH
    0o10400000, // O_PATH | O_NOFOLLOW
    0o200100,   // O_CREAT | O_DIRECTORY (EINVAL)
    0o1000,     // O_RDONLY | O_TRUNC
    0o2,        // O_RDWR
    0o200002,   // O_RDWR | O_DIRECTORY
];

const MODES: &[u32] = &[0o644, 0o600, 0o755, 0o000, 0o444, 0o711, 0o1777, 0o2755, 0o4755, 0o222];

#[derive(Clone, Copy, Debug)]
enum T {
    Now,
    Omit,
    At(i64, i64),
}

#[derive(Clone, Debug)]
enum Op {
    Open { path: usize, flags: usize, mode: usize },
    Close { slot: usize },
    Write { slot: usize, len: usize, seed: u8 },
    Read { slot: usize, len: usize },
    Pwrite { slot: usize, off: u64, len: usize, seed: u8 },
    Pread { slot: usize, off: u64, len: usize },
    Truncate { path: usize, len: u64 },
    Mkdir { path: usize, mode: usize },
    Mkfifo { path: usize, mode: usize },
    Unlink { path: usize },
    Rmdir { path: usize },
    Rename { old: usize, new: usize, flags: u8 },
    Link { old: usize, new: usize, follow: bool },
    Symlink { target: usize, path: usize },
    Readlink { path: usize },
    Stat { path: usize, nofollow: bool },
    Chmod { path: usize, mode: usize },
    Utimens { path: usize, atime: T, mtime: T, nofollow: bool },
    Access { path: usize, mode: u32 },
    Readdir { path: usize },
}

fn t_strategy() -> impl Strategy<Value = T> {
    prop_oneof![
        Just(T::Now),
        Just(T::Omit),
        (1000i64..5000, prop_oneof![Just(0i64), Just(123_456_789), Just(999_999_999)]).prop_map(|(s, n)| T::At(s, n)),
    ]
}

fn op_strategy(npaths: usize) -> impl Strategy<Value = Op> {
    let p = 0..npaths;
    let nm = MODES.len();
    let lens = prop_oneof![Just(0usize), Just(1), Just(7), Just(4095), Just(4096), Just(4097), 0usize..9000];
    let offs = prop_oneof![Just(0u64), Just(4095), Just(4096), Just(8191), 0u64..20_000];
    prop_oneof![
        6 => (p.clone(), 0..FLAG_SETS.len(), 0..nm).prop_map(|(path, flags, mode)| Op::Open { path, flags, mode }),
        2 => (0usize..8).prop_map(|slot| Op::Close { slot }),
        3 => (0usize..8, lens.clone(), any::<u8>()).prop_map(|(slot, len, seed)| Op::Write { slot, len, seed }),
        2 => (0usize..8, lens.clone()).prop_map(|(slot, len)| Op::Read { slot, len }),
        2 => (0usize..8, offs.clone(), lens.clone(), any::<u8>()).prop_map(|(slot, off, len, seed)| Op::Pwrite { slot, off, len, seed }),
        2 => (0usize..8, offs.clone(), lens).prop_map(|(slot, off, len)| Op::Pread { slot, off, len }),
        2 => (p.clone(), offs).prop_map(|(path, len)| Op::Truncate { path, len }),
        5 => (p.clone(), 0..nm).prop_map(|(path, mode)| Op::Mkdir { path, mode }),
        1 => (p.clone(), 0..nm).prop_map(|(path, mode)| Op::Mkfifo { path, mode }),
        3 => p.clone().prop_map(|path| Op::Unlink { path }),
        3 => p.clone().prop_map(|path| Op::Rmdir { path }),
        4 => (p.clone(), p.clone(), 0u8..3).prop_map(|(old, new, flags)| Op::Rename { old, new, flags }),
        2 => (p.clone(), p.clone(), any::<bool>()).prop_map(|(old, new, follow)| Op::Link { old, new, follow }),
        4 => (0..TARGETS.len(), p.clone()).prop_map(|(target, path)| Op::Symlink { target, path }),
        2 => p.clone().prop_map(|path| Op::Readlink { path }),
        3 => (p.clone(), any::<bool>()).prop_map(|(path, nofollow)| Op::Stat { path, nofollow }),
        2 => (p.clone(), 0..nm).prop_map(|(path, mode)| Op::Chmod { path, mode }),
        2 => (p.clone(), t_strategy(), t_strategy(), any::<bool>())
            .prop_map(|(path, atime, mtime, nofollow)| Op::Utimens { path, atime, mtime, nofollow }),
        2 => (p.clone(), 0u32..8).prop_map(|(path, mode)| Op::Access { path, mode }),
        2 => p.prop_map(|path| Op::Readdir { path }),
    ]
}

/// Árvore inicial de todo caso (índices do [`universe`] e de [`TARGETS`]), pra que a parte aleatória
/// encontre coisa pra mexer em vez de só ENOENT.
fn setup_ops() -> Vec<Op> {
    let m755 = 2;
    let m644 = 0;
    let creat = 1; // O_WRONLY | O_CREAT
    vec![
        Op::Mkdir { path: 0, mode: m755 },  // a
        Op::Mkdir { path: 4, mode: m755 },  // a/x
        Op::Mkdir { path: 1, mode: m755 },  // b
        Op::Mkdir { path: 6, mode: m755 },  // b/x
        Op::Open { path: 2, flags: creat, mode: m644 }, // c
        Op::Write { slot: 0, len: 5000, seed: 1 },
        Op::Close { slot: 0 },
        Op::Open { path: 5, flags: creat, mode: m644 }, // a/y
        Op::Write { slot: 0, len: 100, seed: 2 },
        Op::Close { slot: 0 },
        Op::Open { path: 7, flags: 3, mode: m644 }, // b/y, O_RDWR | O_CREAT | O_TRUNC
        Op::Pwrite { slot: 0, off: 9000, len: 10, seed: 3 },
        Op::Close { slot: 0 },
        Op::Open { path: 9, flags: creat, mode: 1 }, // a/x/z, 0600
        Op::Close { slot: 0 },
        Op::Symlink { target: 0, path: 24 },  // s1 -> a
        Op::Symlink { target: 6, path: 26 },  // a/s3 -> x
        Op::Symlink { target: 4, path: 25 },  // s2 -> nonexistent
        Op::Symlink { target: 13, path: 27 }, // b/s4 -> ABS/b/y
        Op::Link { old: 2, new: 11, follow: false }, // b/x/z = c
    ]
}

fn data_of(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|i| seed.wrapping_add((i % 251) as u8)).collect()
}

// ------------------------------------------------------------------------------------------------
// Resultados
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
struct StatView {
    kind: char,
    perm: u32,
    nlink: u64,
    uid: u32,
    gid: u32,
    size: u64,
    blocks: u64,
    /// Só quando foi posto à mão (segundos abaixo de 1e9): o resto é relógio.
    atime: Option<(i64, i64)>,
    mtime: Option<(i64, i64)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Out {
    Unit,
    Fd(&'static str),
    Bytes(Vec<u8>),
    Count(usize),
    Stat(StatView),
    Names(Vec<(Vec<u8>, char)>),
    Skipped,
}

type Res = Result<Out, i32>;

fn kind_char(mode: u32) -> char {
    match mode & S_IFMT {
        S_IFREG => 'f',
        S_IFDIR => 'd',
        S_IFLNK => 'l',
        S_IFIFO => 'p',
        S_IFCHR => 'c',
        S_IFBLK => 'b',
        S_IFSOCK => 's',
        _ => '?',
    }
}

fn small(sec: i64, nsec: i64) -> Option<(i64, i64)> {
    (sec < 1_000_000_000).then_some((sec, nsec))
}

fn ftype_char(t: FileType) -> char {
    kind_char(t.mode_bits())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NodeView {
    stat: StatView,
    target: Option<Vec<u8>>,
    content: Option<Vec<u8>>,
    /// Entradas na ordem do readdir (sem `.` e `..`), com o d_type.
    children: Option<Vec<(Vec<u8>, char)>>,
}

type Dump = BTreeMap<Vec<u8>, NodeView>;

fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    if dir.is_empty() {
        name.to_vec()
    } else {
        [dir, b"/", name].concat()
    }
}

// ------------------------------------------------------------------------------------------------
// Lado do host
// ------------------------------------------------------------------------------------------------

struct HostFd {
    fd: OwnedFd,
}

struct Host {
    base: OwnedFd,
    fds: Vec<HostFd>,
}

fn herr(e: rustix::io::Errno) -> i32 {
    e.raw_os_error()
}

fn rel_or_dot(rel: &[u8]) -> &[u8] {
    if rel.is_empty() { b"." } else { rel }
}

fn host_stat_view(st: &rfs::Stat) -> StatView {
    StatView {
        kind: kind_char(st.st_mode),
        perm: st.st_mode & 0o7777,
        nlink: st.st_nlink,
        uid: st.st_uid,
        gid: st.st_gid,
        size: st.st_size as u64,
        blocks: st.st_blocks as u64,
        atime: small(st.st_atime, st.st_atime_nsec as i64),
        mtime: small(st.st_mtime, st.st_mtime_nsec as i64),
    }
}

fn rts(t: T) -> rfs::Timespec {
    match t {
        T::Now => rfs::Timespec { tv_sec: 0, tv_nsec: rfs::UTIME_NOW },
        T::Omit => rfs::Timespec { tv_sec: 0, tv_nsec: rfs::UTIME_OMIT },
        T::At(s, n) => rfs::Timespec { tv_sec: s, tv_nsec: n },
    }
}

impl Host {
    fn new(base_path: &str) -> Host {
        std::fs::create_dir(base_path).expect("criar base no /dev/shm");
        rfs::chmod(base_path, rfs::Mode::from_raw_mode(0o755)).expect("chmod base");
        let base = rfs::open(base_path, rfs::OFlags::RDONLY | rfs::OFlags::DIRECTORY | rfs::OFlags::CLOEXEC, rfs::Mode::empty())
            .expect("abrir base");
        Host { base, fds: Vec::new() }
    }

    fn is_fifo(&self, path: &[u8]) -> bool {
        matches!(rfs::statat(&self.base, path, rfs::AtFlags::empty()), Ok(st) if st.st_mode & S_IFMT == S_IFIFO)
    }

    fn apply(&mut self, u: &[Vec<u8>], op: &Op, target_abs: &dyn Fn(usize) -> Vec<u8>) -> Res {
        let b = &self.base;
        match op {
            Op::Open { path, flags, mode } => {
                let fl = FLAG_SETS[*flags];
                let fd = rfs::openat(
                    b,
                    u[*path].as_slice(),
                    rfs::OFlags::from_bits_retain(fl) | rfs::OFlags::CLOEXEC,
                    rfs::Mode::from_raw_mode(MODES[*mode]),
                )
                .map_err(herr)?;
                let kind = if fl & 0o10000000 != 0 {
                    "path"
                } else {
                    match rfs::fstat(&fd).map_err(herr)?.st_mode & S_IFMT {
                        S_IFDIR => "dir",
                        S_IFREG => "file",
                        _ => "other",
                    }
                };
                self.fds.push(HostFd { fd });
                Ok(Out::Fd(kind))
            }
            Op::Close { slot } => {
                if self.fds.is_empty() {
                    return Ok(Out::Skipped);
                }
                let i = slot % self.fds.len();
                self.fds.remove(i);
                Ok(Out::Unit)
            }
            Op::Write { slot, len, seed } => {
                let Some(f) = self.slot(*slot) else { return Ok(Out::Skipped) };
                Ok(Out::Count(rustix::io::write(&f.fd, &data_of(*len, *seed)).map_err(herr)?))
            }
            Op::Read { slot, len } => {
                let Some(f) = self.slot(*slot) else { return Ok(Out::Skipped) };
                let mut buf = vec![0u8; *len];
                let n = rustix::io::read(&f.fd, &mut buf).map_err(herr)?;
                buf.truncate(n);
                Ok(Out::Bytes(buf))
            }
            Op::Pwrite { slot, off, len, seed } => {
                let Some(f) = self.slot(*slot) else { return Ok(Out::Skipped) };
                Ok(Out::Count(rustix::io::pwrite(&f.fd, &data_of(*len, *seed), *off).map_err(herr)?))
            }
            Op::Pread { slot, off, len } => {
                let Some(f) = self.slot(*slot) else { return Ok(Out::Skipped) };
                let mut buf = vec![0u8; *len];
                let n = rustix::io::pread(&f.fd, &mut buf, *off).map_err(herr)?;
                buf.truncate(n);
                Ok(Out::Bytes(buf))
            }
            Op::Truncate { path, len } => {
                let fd = rfs::openat(b, u[*path].as_slice(), rfs::OFlags::WRONLY | rfs::OFlags::CLOEXEC, rfs::Mode::empty())
                    .map_err(herr)?;
                rfs::ftruncate(&fd, *len).map_err(herr)?;
                Ok(Out::Unit)
            }
            Op::Mkdir { path, mode } => {
                rfs::mkdirat(b, u[*path].as_slice(), rfs::Mode::from_raw_mode(MODES[*mode])).map_err(herr)?;
                Ok(Out::Unit)
            }
            Op::Mkfifo { path, mode } => {
                rfs::mkfifoat(b, u[*path].as_slice(), rfs::Mode::from_raw_mode(MODES[*mode])).map_err(herr)?;
                Ok(Out::Unit)
            }
            Op::Unlink { path } => {
                rfs::unlinkat(b, u[*path].as_slice(), rfs::AtFlags::empty()).map_err(herr)?;
                Ok(Out::Unit)
            }
            Op::Rmdir { path } => {
                rfs::unlinkat(b, u[*path].as_slice(), rfs::AtFlags::REMOVEDIR).map_err(herr)?;
                Ok(Out::Unit)
            }
            Op::Rename { old, new, flags } => {
                let f = match flags {
                    0 => rfs::RenameFlags::empty(),
                    1 => rfs::RenameFlags::NOREPLACE,
                    _ => rfs::RenameFlags::EXCHANGE,
                };
                rfs::renameat_with(b, u[*old].as_slice(), b, u[*new].as_slice(), f).map_err(herr)?;
                Ok(Out::Unit)
            }
            Op::Link { old, new, follow } => {
                let f = if *follow { rfs::AtFlags::SYMLINK_FOLLOW } else { rfs::AtFlags::empty() };
                rfs::linkat(b, u[*old].as_slice(), b, u[*new].as_slice(), f).map_err(herr)?;
                Ok(Out::Unit)
            }
            Op::Symlink { target, path } => {
                let t = target_abs(*target);
                rfs::symlinkat(t.as_slice(), b, u[*path].as_slice()).map_err(herr)?;
                Ok(Out::Unit)
            }
            Op::Readlink { path } => {
                let t = rfs::readlinkat(b, u[*path].as_slice(), Vec::new()).map_err(herr)?;
                Ok(Out::Bytes(t.into_bytes()))
            }
            Op::Stat { path, nofollow } => {
                let f = if *nofollow { rfs::AtFlags::SYMLINK_NOFOLLOW } else { rfs::AtFlags::empty() };
                let st = rfs::statat(b, u[*path].as_slice(), f).map_err(herr)?;
                Ok(Out::Stat(host_stat_view(&st)))
            }
            Op::Chmod { path, mode } => {
                rfs::chmodat(b, u[*path].as_slice(), rfs::Mode::from_raw_mode(MODES[*mode]), rfs::AtFlags::empty()).map_err(herr)?;
                Ok(Out::Unit)
            }
            Op::Utimens { path, atime, mtime, nofollow } => {
                let f = if *nofollow { rfs::AtFlags::SYMLINK_NOFOLLOW } else { rfs::AtFlags::empty() };
                let ts = rfs::Timestamps { last_access: rts(*atime), last_modification: rts(*mtime) };
                rfs::utimensat(b, u[*path].as_slice(), &ts, f).map_err(herr)?;
                Ok(Out::Unit)
            }
            Op::Access { path, mode } => {
                rfs::accessat(b, u[*path].as_slice(), rfs::Access::from_bits_retain(*mode), rfs::AtFlags::empty()).map_err(herr)?;
                Ok(Out::Unit)
            }
            Op::Readdir { path } => {
                let fd = rfs::openat(
                    b,
                    u[*path].as_slice(),
                    rfs::OFlags::RDONLY | rfs::OFlags::DIRECTORY | rfs::OFlags::CLOEXEC,
                    rfs::Mode::empty(),
                )
                .map_err(herr)?;
                Ok(Out::Names(host_readdir(&fd)?))
            }
        }
    }

    fn slot(&self, slot: usize) -> Option<&HostFd> {
        if self.fds.is_empty() { None } else { Some(&self.fds[slot % self.fds.len()]) }
    }

    /// Retrato da árvore a partir da base. Diretórios e arquivos sem permissão pro dono ganham
    /// permissão temporária pra serem lidos (só o ctime muda, e ele não é comparado).
    fn dump(&self) -> Dump {
        let mut out = Dump::new();
        self.walk(b"", &mut out);
        out
    }

    /// lstat de um caminho relativo à base; a própria base vai pelo fd (`AT_EMPTY_PATH`), porque ela
    /// pode estar sem permissão de busca.
    fn lstat_rel(&self, rel: &[u8]) -> rfs::Stat {
        if rel.is_empty() {
            rfs::statat(&self.base, "", rfs::AtFlags::EMPTY_PATH).expect("fstat da base")
        } else {
            rfs::statat(&self.base, rel, rfs::AtFlags::SYMLINK_NOFOLLOW).expect("lstat no retrato")
        }
    }

    fn chmod_rel(&self, rel: &[u8], perm: u32) {
        let m = rfs::Mode::from_raw_mode(perm);
        if rel.is_empty() {
            rfs::fchmod(&self.base, m).expect("fchmod da base");
        } else {
            rfs::chmodat(&self.base, rel, m, rfs::AtFlags::empty()).expect("chmod no retrato");
        }
    }

    fn walk(&self, rel: &[u8], out: &mut Dump) {
        let b = &self.base;
        let p = rel_or_dot(rel);
        let st = self.lstat_rel(rel);
        let mut node = NodeView { stat: host_stat_view(&st), target: None, content: None, children: None };
        let perm = st.st_mode & 0o7777;
        match st.st_mode & S_IFMT {
            S_IFDIR => {
                let changed = perm & 0o700 != 0o700;
                if changed {
                    self.chmod_rel(rel, perm | 0o700);
                }
                let fd = rfs::openat(b, p, rfs::OFlags::RDONLY | rfs::OFlags::DIRECTORY | rfs::OFlags::CLOEXEC, rfs::Mode::empty())
                    .expect("abrir diretório no retrato");
                let kids: Vec<(Vec<u8>, char)> = host_readdir(&fd)
                    .expect("readdir no retrato")
                    .into_iter()
                    .filter(|(n, _)| n != b"." && n != b"..")
                    .collect();
                drop(fd);
                for (n, _) in &kids {
                    self.walk(&join(rel, n), out);
                }
                if changed {
                    self.chmod_rel(rel, perm);
                }
                node.children = Some(kids);
            }
            S_IFREG => {
                let changed = perm & 0o400 == 0;
                if changed {
                    self.chmod_rel(rel, perm | 0o400);
                }
                let fd = rfs::openat(b, p, rfs::OFlags::RDONLY | rfs::OFlags::NOFOLLOW | rfs::OFlags::CLOEXEC, rfs::Mode::empty())
                    .expect("abrir arquivo no retrato");
                let mut data = Vec::new();
                let mut buf = vec![0u8; 65536];
                loop {
                    let n = rustix::io::read(&fd, &mut buf).expect("ler no retrato");
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(&buf[..n]);
                }
                drop(fd);
                if changed {
                    self.chmod_rel(rel, perm);
                }
                // O chmod temporário e a leitura não mexem no que comparamos, mas o stat é refeito pra
                // pegar o atime depois da leitura (relatime), igual ao nosso lado.
                node.stat = host_stat_view(&self.lstat_rel(rel));
                node.content = Some(data);
            }
            S_IFLNK => {
                node.target = Some(rfs::readlinkat(b, p, Vec::new()).expect("readlink no retrato").into_bytes());
            }
            _ => {}
        }
        if node.children.is_some() {
            // stat do diretório depois do readdir (atime por relatime), como do nosso lado.
            node.stat = host_stat_view(&self.lstat_rel(rel));
        }
        out.insert(rel.to_vec(), node);
    }
}

/// `getdents64` direto no fd (o `Dir` do rustix reabre "." e exigiria permissão de busca).
fn host_readdir(fd: &OwnedFd) -> Result<Vec<(Vec<u8>, char)>, i32> {
    let mut buf = vec![std::mem::MaybeUninit::<u8>::uninit(); 8192];
    let mut dir = rfs::RawDir::new(fd, &mut buf);
    let mut out = Vec::new();
    while let Some(e) = dir.next() {
        let e = e.map_err(herr)?;
        out.push((e.file_name().to_bytes().to_vec(), rtype_char(e.file_type())));
    }
    Ok(out)
}

fn rtype_char(t: rfs::FileType) -> char {
    match t {
        rfs::FileType::RegularFile => 'f',
        rfs::FileType::Directory => 'd',
        rfs::FileType::Symlink => 'l',
        rfs::FileType::Fifo => 'p',
        rfs::FileType::CharacterDevice => 'c',
        rfs::FileType::BlockDevice => 'b',
        rfs::FileType::Socket => 's',
        _ => '?',
    }
}

/// Apaga a base do host mesmo com diretórios sem permissão.
struct HostCleanup(String);

impl Drop for HostCleanup {
    fn drop(&mut self) {
        fn fix(p: &std::path::Path) {
            let _ = rfs::chmod(p, rfs::Mode::from_raw_mode(0o700));
            if let Ok(rd) = std::fs::read_dir(p) {
                for e in rd.flatten() {
                    if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        fix(&e.path());
                    }
                }
            }
        }
        let p = std::path::Path::new(&self.0);
        fix(p);
        let _ = std::fs::remove_dir_all(p);
    }
}

// ------------------------------------------------------------------------------------------------
// Nosso lado
// ------------------------------------------------------------------------------------------------

enum OurFd {
    File { handle: Box<dyn FileHandle>, readable: bool, writable: bool, append: bool, offset: u64 },
    Path,
}

struct Ours {
    ns: Arc<Namespace>,
    cx: Caller,
    root_cx: Caller,
    fds: Vec<OurFd>,
}

fn now() -> TimeSpec {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).expect("relógio");
    TimeSpec { sec: d.as_secs() as i64, nsec: d.subsec_nanos() }
}

fn our_stat_view(st: &Stat) -> StatView {
    StatView {
        kind: kind_char(st.mode),
        perm: st.mode & 0o7777,
        nlink: st.nlink,
        uid: st.uid,
        gid: st.gid,
        size: st.size,
        blocks: st.blocks,
        atime: small(st.atime.sec, st.atime.nsec as i64),
        mtime: small(st.mtime.sec, st.mtime.nsec as i64),
    }
}

fn ost(t: T) -> SetTime {
    match t {
        T::Now => SetTime::Now,
        T::Omit => SetTime::Omit,
        T::At(s, n) => SetTime::At(TimeSpec { sec: s, nsec: n as u32 }),
    }
}

fn e(x: Errno) -> i32 {
    x.0
}

impl Ours {
    fn new(base_path: &str) -> Ours {
        let t = now();
        let fs = Tmpfs::new(makedev(0, 77), 0o755, t, TmpfsLimits::default());
        let ns = Namespace::new(fs, MountFlags::RELATIME, "tmpfs", "");
        let mut root_cx = ops::kernel_caller(ns.root.root());
        root_cx.now = t;
        let s = &Start::Cwd;
        ns.mkdir(&root_cx, s, b"/dev", 0o755).expect("mkdir /dev");
        ns.mkdir(&root_cx, s, b"/dev/shm", 0o777).expect("mkdir /dev/shm");
        ns.chmod(&root_cx, s, b"/dev/shm", 0o1777, AtFlags::empty()).expect("chmod /dev/shm");
        ns.mkdir(&root_cx, s, base_path.as_bytes(), 0o755).expect("mkdir base");
        let uid = rustix::process::getuid().as_raw();
        let gid = rustix::process::getgid().as_raw();
        let groups: Vec<Gid> = rustix::process::getgroups().expect("getgroups").into_iter().map(|g| g.as_raw()).collect();
        ns.chown(&root_cx, s, base_path.as_bytes(), Some(uid), Some(gid), AtFlags::empty()).expect("chown base");
        let base = match ns.resolve(&root_cx, s, base_path.as_bytes(), true).expect("resolver base") {
            namei::Resolved::Loc(l, _) => l,
            _ => unreachable!(),
        };
        let cx = Caller {
            cred: Arc::new(Cred::new(uid, gid, groups)),
            root: ns.root.root(),
            cwd: base.clone(),
            umask: 0o022,
            now: t,
            pid: 1,
            tid: 1,
            fsize_limit: u64::MAX,
        };
        let mut root_cx = root_cx;
        root_cx.cwd = base;
        Ours { ns, cx, root_cx, fds: Vec::new() }
    }

    fn slot(&mut self, slot: usize) -> Option<&mut OurFd> {
        if self.fds.is_empty() {
            None
        } else {
            let n = self.fds.len();
            Some(&mut self.fds[slot % n])
        }
    }

    fn apply(&mut self, u: &[Vec<u8>], op: &Op, target_abs: &dyn Fn(usize) -> Vec<u8>) -> Res {
        self.cx.now = now();
        let cx = self.cx.clone();
        let ns = self.ns.clone();
        let s = &Start::Cwd;
        match op {
            Op::Open { path, flags, mode } => {
                let fl = FLAG_SETS[*flags];
                let o = ns.open(&cx, s, &u[*path], OFlags::from_bits_retain(fl), MODES[*mode]).map_err(e)?;
                let acc = fl & 3;
                match o {
                    Opened::File { handle, stat, .. } => {
                        let kind = if is_dir(stat.mode) { "dir" } else { "file" };
                        self.fds.push(OurFd::File {
                            handle,
                            readable: acc != 1,
                            writable: acc == 1 || acc == 2,
                            append: fl & 0o2000 != 0,
                            offset: 0,
                        });
                        Ok(Out::Fd(kind))
                    }
                    Opened::Path { .. } => {
                        self.fds.push(OurFd::Path);
                        Ok(Out::Fd("path"))
                    }
                    other => panic!("tipo de open inesperado no universo: {other:?}"),
                }
            }
            Op::Close { slot } => {
                if self.fds.is_empty() {
                    return Ok(Out::Skipped);
                }
                let i = slot % self.fds.len();
                self.fds.remove(i);
                Ok(Out::Unit)
            }
            Op::Write { slot, len, seed } => {
                let Some(f) = self.slot(*slot) else { return Ok(Out::Skipped) };
                match f {
                    OurFd::File { handle, writable: true, append, offset, .. } => {
                        let pos = if *append { WritePos::Append } else { WritePos::At(*offset) };
                        let (n, end) = handle.write(&cx, pos, &data_of(*len, *seed)).map_err(e)?;
                        // Como o kernel: o offset só anda com escrita positiva.
                        if n > 0 {
                            *offset = end;
                        }
                        Ok(Out::Count(n))
                    }
                    _ => Err(Errno::EBADF.0),
                }
            }
            Op::Read { slot, len } => {
                let Some(f) = self.slot(*slot) else { return Ok(Out::Skipped) };
                match f {
                    OurFd::File { handle, readable: true, offset, .. } => {
                        let mut buf = vec![0u8; *len];
                        let n = handle.read(&cx, *offset, &mut buf).map_err(e)?;
                        buf.truncate(n);
                        *offset += n as u64;
                        Ok(Out::Bytes(buf))
                    }
                    _ => Err(Errno::EBADF.0),
                }
            }
            Op::Pwrite { slot, off, len, seed } => {
                let Some(f) = self.slot(*slot) else { return Ok(Out::Skipped) };
                match f {
                    OurFd::File { handle, writable: true, append, .. } => {
                        let pos = if *append { WritePos::Append } else { WritePos::At(*off) };
                        let (n, _) = handle.write(&cx, pos, &data_of(*len, *seed)).map_err(e)?;
                        Ok(Out::Count(n))
                    }
                    _ => Err(Errno::EBADF.0),
                }
            }
            Op::Pread { slot, off, len } => {
                let Some(f) = self.slot(*slot) else { return Ok(Out::Skipped) };
                match f {
                    OurFd::File { handle, readable: true, .. } => {
                        let mut buf = vec![0u8; *len];
                        let n = handle.read(&cx, *off, &mut buf).map_err(e)?;
                        buf.truncate(n);
                        Ok(Out::Bytes(buf))
                    }
                    _ => Err(Errno::EBADF.0),
                }
            }
            Op::Truncate { path, len } => match ns.open(&cx, s, &u[*path], OFlags::WRONLY, 0).map_err(e)? {
                Opened::File { loc, .. } => {
                    ns.truncate_loc(&cx, &loc, *len, false).map_err(e)?;
                    Ok(Out::Unit)
                }
                other => panic!("truncate num tipo inesperado: {other:?}"),
            },
            Op::Mkdir { path, mode } => ns.mkdir(&cx, s, &u[*path], MODES[*mode]).map(|_| Out::Unit).map_err(e),
            Op::Mkfifo { path, mode } => ns.mknod(&cx, s, &u[*path], S_IFIFO | MODES[*mode], 0).map(|_| Out::Unit).map_err(e),
            Op::Unlink { path } => ns.unlink(&cx, s, &u[*path], AtFlags::empty()).map(|_| Out::Unit).map_err(e),
            Op::Rmdir { path } => ns.unlink(&cx, s, &u[*path], AtFlags::REMOVEDIR).map(|_| Out::Unit).map_err(e),
            Op::Rename { old, new, flags } => {
                let f = match flags {
                    0 => RenameFlags::empty(),
                    1 => RenameFlags::NOREPLACE,
                    _ => RenameFlags::EXCHANGE,
                };
                ns.rename(&cx, s, &u[*old], s, &u[*new], f).map(|_| Out::Unit).map_err(e)
            }
            Op::Link { old, new, follow } => {
                let f = if *follow { AtFlags::SYMLINK_FOLLOW } else { AtFlags::empty() };
                ns.link(&cx, s, &u[*old], s, &u[*new], f).map(|_| Out::Unit).map_err(e)
            }
            Op::Symlink { target, path } => {
                ns.symlink(&cx, &target_abs(*target), s, &u[*path]).map(|_| Out::Unit).map_err(e)
            }
            Op::Readlink { path } => ns.readlink(&cx, s, &u[*path]).map(Out::Bytes).map_err(e),
            Op::Stat { path, nofollow } => {
                let f = if *nofollow { AtFlags::SYMLINK_NOFOLLOW } else { AtFlags::empty() };
                let r = ns.stat(&cx, s, &u[*path], f).map_err(e)?;
                Ok(Out::Stat(our_stat_view(&r.stat())))
            }
            Op::Chmod { path, mode } => ns.chmod(&cx, s, &u[*path], MODES[*mode], AtFlags::empty()).map(|_| Out::Unit).map_err(e),
            Op::Utimens { path, atime, mtime, nofollow } => {
                let f = if *nofollow { AtFlags::SYMLINK_NOFOLLOW } else { AtFlags::empty() };
                ns.utimens(&cx, s, &u[*path], ost(*atime), ost(*mtime), f).map(|_| Out::Unit).map_err(e)
            }
            Op::Access { path, mode } => {
                ns.access(&cx, s, &u[*path], AccessMode::from_bits_retain(*mode), AtFlags::empty()).map(|_| Out::Unit).map_err(e)
            }
            Op::Readdir { path } => match ns.open(&cx, s, &u[*path], OFlags::RDONLY | OFlags::DIRECTORY, 0).map_err(e)? {
                Opened::File { handle, .. } => {
                    let mut out = Vec::new();
                    let mut cookie = 0;
                    loop {
                        // Lotes pequenos pra exercitar o cookie.
                        let (batch, next) = handle.readdir(&cx, cookie, 3).map_err(e)?;
                        if batch.is_empty() {
                            break;
                        }
                        out.extend(batch.into_iter().map(|d| (d.name, ftype_char(d.kind))));
                        cookie = next;
                    }
                    Ok(Out::Names(out))
                }
                other => panic!("readdir num tipo inesperado: {other:?}"),
            },
        }
    }

    fn dump(&mut self) -> Dump {
        self.root_cx.now = now();
        let mut out = Dump::new();
        let base = self.root_cx.cwd.clone();
        self.walk(&base, b"", &mut out);
        out
    }

    fn walk(&self, loc: &Loc, rel: &[u8], out: &mut Dump) {
        let cx = &self.root_cx;
        let ns = &self.ns;
        let st = ns.stat_loc(cx, loc).expect("stat no retrato");
        let mut node = NodeView { stat: our_stat_view(&st), target: None, content: None, children: None };
        match st.mode & S_IFMT {
            S_IFDIR => {
                let h = loc.fs().clone().open(cx, loc.ino, OFlags::RDONLY | OFlags::DIRECTORY).expect("abrir diretório");
                let mut kids = Vec::new();
                let mut cookie = 0;
                loop {
                    let (batch, next) = h.readdir(cx, cookie, 64).expect("readdir");
                    if batch.is_empty() {
                        break;
                    }
                    for d in batch {
                        if d.name != b"." && d.name != b".." {
                            kids.push((d.name, ftype_char(d.kind)));
                        }
                    }
                    cookie = next;
                }
                drop(h);
                for (n, _) in &kids {
                    let child = Loc { mnt: loc.mnt.clone(), ino: loc.fs().lookup(cx, loc.ino, n).expect("lookup no retrato") };
                    self.walk(&child, &join(rel, n), out);
                }
                node.stat = our_stat_view(&ns.stat_loc(cx, loc).expect("stat no retrato"));
                node.children = Some(kids);
            }
            S_IFREG => {
                let h = loc.fs().clone().open(cx, loc.ino, OFlags::RDONLY).expect("abrir arquivo");
                let mut data = Vec::new();
                let mut buf = vec![0u8; 65536];
                loop {
                    let n = h.read(cx, data.len() as u64, &mut buf).expect("ler");
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(&buf[..n]);
                }
                drop(h);
                node.stat = our_stat_view(&ns.stat_loc(cx, loc).expect("stat no retrato"));
                node.content = Some(data);
            }
            S_IFLNK => {
                // Como o readlinkat do host no retrato: lê e faz touch_atime.
                node.target = Some(loc.fs().readlink(cx, loc.ino).expect("readlink"));
                loc.fs().touch_atime(cx, loc.ino);
            }
            _ => {}
        }
        out.insert(rel.to_vec(), node);
    }
}

// ------------------------------------------------------------------------------------------------
// O caso
// ------------------------------------------------------------------------------------------------

/// Só as entradas que diferem, pra mensagem de falha caber na tela.
fn dump_diff(h: &Dump, o: &Dump) -> String {
    let mut out = String::new();
    let keys: std::collections::BTreeSet<&Vec<u8>> = h.keys().chain(o.keys()).collect();
    for k in keys {
        let (a, b) = (h.get(k), o.get(k));
        if a != b {
            let short = |n: Option<&NodeView>| match n {
                None => "ausente".to_string(),
                Some(n) => {
                    let mut n = n.clone();
                    if let Some(c) = &mut n.content
                        && c.len() > 32
                    {
                        c.truncate(32);
                    }
                    format!("{n:?}")
                }
            };
            out.push_str(&format!("\n  {:?}:\n    host {}\n    vfs  {}", String::from_utf8_lossy(k), short(a), short(b)));
        }
    }
    out
}

fn run_case(u: &[Vec<u8>], ops: &[Op]) -> Result<(), TestCaseError> {
    let id = CASE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let base_path = format!("/dev/shm/vfsdiff-{}-{id}", std::process::id());
    let _cleanup = HostCleanup(base_path.clone());
    let mut host = Host::new(&base_path);
    let mut ours = Ours::new(&base_path);
    let target_abs = |i: usize| -> Vec<u8> {
        let t = TARGETS[i];
        match t.strip_prefix("ABS:") {
            Some(rest) => format!("{base_path}{rest}").into_bytes(),
            None => t.as_bytes().to_vec(),
        }
    };
    let d0h = host.dump();
    let d0o = ours.dump();
    prop_assert!(d0h == d0o, "retrato inicial{}", dump_diff(&d0h, &d0o));
    let all: Vec<Op> = setup_ops().into_iter().chain(ops.iter().cloned()).collect();
    for (i, op) in all.iter().enumerate() {
        // FIFO: abrir bloquearia o host esperando a outra ponta, e o encontro é do kernel, não do VFS.
        let fifo_path = match op {
            Op::Open { path, .. } | Op::Truncate { path, .. } | Op::Readdir { path } => Some(*path),
            _ => None,
        };
        if let Some(p) = fifo_path
            && host.is_fifo(&u[p])
        {
            continue;
        }
        let rh = host.apply(u, op, &target_abs);
        let ro = ours.apply(u, op, &target_abs);
        OPS_RUN.fetch_add(1, Ordering::Relaxed);
        record(op, &rh);
        prop_assert_eq!(&rh, &ro, "passo {}: {:?} (host à esquerda, VFS à direita)", i, op);
        let dh = host.dump();
        let dob = ours.dump();
        prop_assert!(dh == dob, "retrato depois do passo {}: {:?}{}", i, op, dump_diff(&dh, &dob));
    }
    CASES_RUN.fetch_add(1, Ordering::Relaxed);
    drop(host);
    Ok(())
}

#[test]
fn vfs_matches_host_tmpfs() {
    let old = rustix::process::umask(rfs::Mode::from_raw_mode(0o022));
    let cases: u32 = std::env::var("VFS_DIFF_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(512);
    let u = universe();
    let config = Config {
        cases,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(std::env::var("VFS_DIFF_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(SEED)),
        max_shrink_iters: 4096,
        ..Config::default()
    };
    let mut runner = TestRunner::new(config);
    let r = runner.run(&vec(op_strategy(u.len()), 1..=60), |ops| run_case(&u, &ops));
    rustix::process::umask(old);
    eprintln!(
        "vfs diferencial: {} sequências completas, {} operações comparadas",
        CASES_RUN.load(Ordering::Relaxed),
        OPS_RUN.load(Ordering::Relaxed)
    );
    if std::env::var_os("VFS_DIFF_STATS").is_some() {
        let g = OUTCOMES.lock().unwrap_or_else(|p| p.into_inner());
        for (name, (ok, errs)) in g.iter() {
            let errs: Vec<String> = errs.iter().map(|(e, n)| format!("{:?}={n}", Errno(*e))).collect();
            eprintln!("  {name:9} ok={ok:5} {}", errs.join(" "));
        }
    }
    if let Err(e) = r {
        panic!("{e}");
    }
}
