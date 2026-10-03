//! Tipos das syscalls, com os mesmos números e bits do Linux x86_64.

use std::time::Duration;

use bitflags::bitflags;

use crate::linux::Signal;

/// Descritor de arquivo de um pseudo-processo.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fd(pub i32);

impl Fd {
    pub const STDIN: Fd = Fd(0);
    pub const STDOUT: Fd = Fd(1);
    pub const STDERR: Fd = Fd(2);
    /// `AT_FDCWD`: caminhos relativos resolvem a partir do cwd.
    pub const CWD: Fd = Fd(-100);
}

pub type Pid = i32;
pub type Uid = u32;
pub type Gid = u32;
/// Bits de permissão e tipo (`st_mode`).
pub type Mode = u32;

bitflags! {
    /// Flags do `open(2)`, valores do Linux x86_64.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct OFlags: u32 {
        const WRONLY = 0o1;
        const RDWR = 0o2;
        const CREAT = 0o100;
        const EXCL = 0o200;
        const NOCTTY = 0o400;
        const TRUNC = 0o1000;
        const APPEND = 0o2000;
        const NONBLOCK = 0o4000;
        const DSYNC = 0o10000;
        const DIRECTORY = 0o200000;
        const NOFOLLOW = 0o400000;
        const NOATIME = 0o1000000;
        const CLOEXEC = 0o2000000;
        const SYNC = 0o4010000;
        const PATH = 0o10000000;
        const TMPFILE = 0o20200000;
    }
}

impl OFlags {
    /// `O_RDONLY` é zero; ficam aqui pra deixar a intenção explícita.
    pub const RDONLY: OFlags = OFlags::empty();
    pub const ACCMODE: u32 = 0o3;

    pub fn readable(self) -> bool {
        self.bits() & Self::ACCMODE != 1
    }

    pub fn writable(self) -> bool {
        matches!(self.bits() & Self::ACCMODE, 1 | 2)
    }
}

bitflags! {
    /// Flags `AT_*` das syscalls `*at`.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct AtFlags: u32 {
        const SYMLINK_NOFOLLOW = 0x100;
        /// Pra `unlinkat` é `AT_REMOVEDIR`; pra `faccessat` o mesmo bit é `AT_EACCESS`.
        const REMOVEDIR = 0x200;
        const SYMLINK_FOLLOW = 0x400;
        const NO_AUTOMOUNT = 0x800;
        const EMPTY_PATH = 0x1000;
    }
}

bitflags! {
    /// `renameat2`.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct RenameFlags: u32 {
        const NOREPLACE = 1;
        const EXCHANGE = 2;
        const WHITEOUT = 4;
    }
}

bitflags! {
    /// Modo do `access(2)`: `F_OK` é vazio.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct AccessMode: u32 {
        const X_OK = 1;
        const W_OK = 2;
        const R_OK = 4;
    }
}

/// Bits de `st_mode`.
pub mod mode {
    use super::Mode;
    pub const S_IFMT: Mode = 0o170000;
    pub const S_IFSOCK: Mode = 0o140000;
    pub const S_IFLNK: Mode = 0o120000;
    pub const S_IFREG: Mode = 0o100000;
    pub const S_IFBLK: Mode = 0o060000;
    pub const S_IFDIR: Mode = 0o040000;
    pub const S_IFCHR: Mode = 0o020000;
    pub const S_IFIFO: Mode = 0o010000;
    pub const S_ISUID: Mode = 0o4000;
    pub const S_ISGID: Mode = 0o2000;
    pub const S_ISVTX: Mode = 0o1000;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FileType {
    Regular,
    Directory,
    Symlink,
    CharDevice,
    BlockDevice,
    Fifo,
    Socket,
}

impl FileType {
    pub fn from_mode(m: Mode) -> FileType {
        match m & mode::S_IFMT {
            mode::S_IFDIR => FileType::Directory,
            mode::S_IFLNK => FileType::Symlink,
            mode::S_IFCHR => FileType::CharDevice,
            mode::S_IFBLK => FileType::BlockDevice,
            mode::S_IFIFO => FileType::Fifo,
            mode::S_IFSOCK => FileType::Socket,
            _ => FileType::Regular,
        }
    }

    pub fn mode_bits(self) -> Mode {
        match self {
            FileType::Regular => mode::S_IFREG,
            FileType::Directory => mode::S_IFDIR,
            FileType::Symlink => mode::S_IFLNK,
            FileType::CharDevice => mode::S_IFCHR,
            FileType::BlockDevice => mode::S_IFBLK,
            FileType::Fifo => mode::S_IFIFO,
            FileType::Socket => mode::S_IFSOCK,
        }
    }

    /// `d_type` do `getdents64`.
    pub fn dirent_type(self) -> u8 {
        match self {
            FileType::Fifo => 1,
            FileType::CharDevice => 2,
            FileType::Directory => 4,
            FileType::BlockDevice => 6,
            FileType::Regular => 8,
            FileType::Symlink => 10,
            FileType::Socket => 12,
        }
    }
}

/// Instante com nanossegundos, como `struct timespec`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeSpec {
    pub sec: i64,
    pub nsec: u32,
}

impl TimeSpec {
    pub fn from_duration_since_epoch(d: Duration) -> TimeSpec {
        TimeSpec { sec: d.as_secs() as i64, nsec: d.subsec_nanos() }
    }
}

/// Alvo de `utimensat`: um instante, "agora" (`UTIME_NOW`) ou "não mexe" (`UTIME_OMIT`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SetTime {
    Now,
    Omit,
    At(TimeSpec),
}

/// `struct stat` (com `btime` do statx).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stat {
    pub dev: u64,
    pub ino: u64,
    /// Tipo e permissões (`S_IFREG | 0o644`...).
    pub mode: Mode,
    pub nlink: u64,
    pub uid: Uid,
    pub gid: Gid,
    pub rdev: u64,
    pub size: u64,
    pub blksize: u64,
    /// Em blocos de 512 bytes.
    pub blocks: u64,
    pub atime: TimeSpec,
    pub mtime: TimeSpec,
    pub ctime: TimeSpec,
    pub btime: Option<TimeSpec>,
}

impl Stat {
    pub fn file_type(&self) -> FileType {
        FileType::from_mode(self.mode)
    }

    pub fn perm(&self) -> Mode {
        self.mode & 0o7777
    }
}

/// Entrada de diretório.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub ino: u64,
    pub kind: FileType,
    pub name: Vec<u8>,
}

/// `struct statfs`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatFs {
    /// `f_type` (TMPFS_MAGIC = 0x01021994, PROC_SUPER_MAGIC = 0x9fa0...).
    pub fs_type: u64,
    pub bsize: u64,
    pub blocks: u64,
    pub bfree: u64,
    pub bavail: u64,
    pub files: u64,
    pub ffree: u64,
    pub namelen: u64,
    pub frsize: u64,
    pub flags: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Whence {
    Set,
    Cur,
    End,
    /// `SEEK_DATA` e `SEEK_HOLE`.
    Data,
    Hole,
}

/// `struct winsize`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Winsize {
    pub rows: u16,
    pub cols: u16,
    pub xpixel: u16,
    pub ypixel: u16,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Clock {
    Realtime,
    Monotonic,
    Boottime,
    ProcessCpuTime,
    ThreadCpuTime,
}

/// `getrusage`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rusage {
    pub utime: Duration,
    pub stime: Duration,
    pub maxrss_kib: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RusageWho {
    SelfProcess,
    Children,
}

/// Recursos de `getrlimit`/`setrlimit`, com os números do Linux.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Resource {
    Cpu = 0,
    Fsize = 1,
    Data = 2,
    Stack = 3,
    Core = 4,
    Rss = 5,
    Nproc = 6,
    Nofile = 7,
    Memlock = 8,
    As = 9,
    Locks = 10,
    Sigpending = 11,
    Msgqueue = 12,
    Nice = 13,
    Rtprio = 14,
    Rttime = 15,
}

pub const RLIM_INFINITY: u64 = u64::MAX;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Rlimit {
    pub cur: u64,
    pub max: u64,
}

/// `uname(2)`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Utsname {
    pub sysname: Vec<u8>,
    pub nodename: Vec<u8>,
    pub release: Vec<u8>,
    pub version: Vec<u8>,
    pub machine: Vec<u8>,
    pub domainname: Vec<u8>,
}

/// Disposição de um sinal no processo.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SigDisposition {
    Default,
    Ignore,
    /// O processo quer ser avisado: o kernel enfileira e o programa consulta com
    /// [`crate::Syscalls::take_caught_signals`] (é assim que o `trap` do shell funciona).
    Catch,
}

/// Como um processo terminou ou mudou de estado, como o `wait4` informa.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WaitStatus {
    Exited(i32),
    Signaled { signal: Signal, core_dumped: bool },
    Stopped(Signal),
    Continued,
}

impl WaitStatus {
    /// Código de saída visto pelo shell (`$?`): 128 + sinal quando morto por sinal.
    pub fn shell_status(self) -> i32 {
        match self {
            WaitStatus::Exited(c) => c & 0xff,
            WaitStatus::Signaled { signal, .. } => 128 + signal.0,
            WaitStatus::Stopped(s) => 128 + s.0,
            WaitStatus::Continued => 0,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WaitTarget {
    /// `-1`: qualquer filho.
    Any,
    Pid(Pid),
    /// Qualquer filho do grupo (`-pgid`; `0` = grupo do chamador).
    Group(Pid),
}

bitflags! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct WaitOptions: u32 {
        const NOHANG = 1;
        const UNTRACED = 2;
        const CONTINUED = 8;
    }
}

/// Alvo de `kill`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum KillTarget {
    Pid(Pid),
    /// Grupo de processos (`kill -- -pgid`); `0` = grupo do chamador.
    Group(Pid),
    /// `kill -1`: todos os processos que o chamador pode sinalizar, menos o 1 e ele mesmo.
    All,
}

/// Ajuste de fd no processo novo, aplicado na ordem, antes do programa começar (como
/// `posix_spawn_file_actions`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FdAction {
    /// `dup2(from, to)`; o fd novo não herda `FD_CLOEXEC`.
    Dup2 { from: Fd, to: Fd },
    Close(Fd),
    Open { fd: Fd, path: Vec<u8>, flags: OFlags, mode: Mode },
}

/// Grupo de processos do filho.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProcessGroup {
    /// Fica no grupo do pai.
    Inherit,
    /// Grupo novo com o próprio pid (`setpgid(0, 0)`).
    New,
    /// Entra num grupo existente.
    Join(Pid),
}

/// Atributos comuns de processo novo, tanto pra `spawn` (programa) quanto pra `spawn_fn` (fork).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcAttrs {
    /// Ambiente completo do filho (`NAME=valor`); `None` herda o do pai.
    pub env: Option<Vec<Vec<u8>>>,
    /// cwd do filho; `None` herda.
    pub cwd: Option<Vec<u8>>,
    pub fd_actions: Vec<FdAction>,
    pub group: ProcessGroup,
    /// `setsid()` no filho.
    pub new_session: bool,
    /// Sinais que voltam à disposição padrão no filho (o bash faz isso com os que ele captura).
    pub reset_signals: Vec<Signal>,
    /// Sinais ignorados no filho (`cmd &` em shell não interativo ignora SIGINT e SIGQUIT).
    pub ignore_signals: Vec<Signal>,
}

impl Default for ProcAttrs {
    fn default() -> Self {
        ProcAttrs {
            env: None,
            cwd: None,
            fd_actions: Vec::new(),
            group: ProcessGroup::Inherit,
            new_session: false,
            reset_signals: Vec::new(),
            ignore_signals: Vec::new(),
        }
    }
}

/// Executa um programa num processo novo (`posix_spawn`): `path` já resolvido pelo chamador
/// (o kernel não procura no PATH, como o `execve`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnSpec {
    pub path: Vec<u8>,
    pub argv: Vec<Vec<u8>>,
    pub attrs: ProcAttrs,
}

/// Corpo de um processo criado por `spawn_fn` (o equivalente a `fork` seguido de código no filho).
pub type ProcessFn = Box<dyn FnOnce() -> i32 + Send + 'static>;

/// Informação mínima de processo (pra `ps`, `jobs`, `/proc`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: Pid,
    pub ppid: Pid,
    pub pgid: Pid,
    pub sid: Pid,
    /// Estado de uma letra como no `/proc/<pid>/stat`: R, S, D, T, Z.
    pub state: char,
    pub comm: Vec<u8>,
}
