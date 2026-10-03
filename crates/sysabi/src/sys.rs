//! As syscalls do pseudo-linus.
//!
//! O kernel implementa [`Syscalls`] pra cada pseudo-processo e instala o objeto na thread do processo
//! (modelo de execução A: uma thread do SO por pseudo-processo). Programas chamam as funções livres
//! deste módulo, que encontram o processo corrente pelo thread-local, exatamente como um programa C
//! chama a libc sem saber qual processo ele é.
//!
//! Regras do contrato:
//!
//! - Caminhos são bytes (`&[u8]`), como no Linux; nada de `Path` do host.
//! - Toda syscall é ponto de preempção e de entrega de sinal. Um sinal fatal pendente faz a syscall não
//!   voltar: o kernel desenrola a pilha do processo (ver [`KillUnwind`]).
//! - Erros são sempre [`Errno`] com o número do Linux.

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;

use crate::linux::{Errno, Signal};
use crate::types::*;

pub type SysResult<T> = Result<T, Errno>;

/// A interface de um processo com o kernel. Todos os métodos agem sobre o processo dono do objeto.
pub trait Syscalls: Send + Sync {
    // ---- arquivos ----
    fn openat(&self, dirfd: Fd, path: &[u8], flags: OFlags, mode: Mode) -> SysResult<Fd>;
    fn close(&self, fd: Fd) -> SysResult<()>;
    fn read(&self, fd: Fd, buf: &mut [u8]) -> SysResult<usize>;
    fn write(&self, fd: Fd, buf: &[u8]) -> SysResult<usize>;
    fn pread(&self, fd: Fd, buf: &mut [u8], offset: u64) -> SysResult<usize>;
    fn pwrite(&self, fd: Fd, buf: &[u8], offset: u64) -> SysResult<usize>;
    fn lseek(&self, fd: Fd, offset: i64, whence: Whence) -> SysResult<u64>;
    fn fstat(&self, fd: Fd) -> SysResult<Stat>;
    fn fstatat(&self, dirfd: Fd, path: &[u8], flags: AtFlags) -> SysResult<Stat>;
    fn faccessat(&self, dirfd: Fd, path: &[u8], mode: AccessMode, flags: AtFlags) -> SysResult<()>;
    fn mkdirat(&self, dirfd: Fd, path: &[u8], mode: Mode) -> SysResult<()>;
    /// `AtFlags::REMOVEDIR` remove diretório (rmdir).
    fn unlinkat(&self, dirfd: Fd, path: &[u8], flags: AtFlags) -> SysResult<()>;
    fn renameat2(&self, olddir: Fd, old: &[u8], newdir: Fd, new: &[u8], flags: RenameFlags) -> SysResult<()>;
    fn linkat(&self, olddir: Fd, old: &[u8], newdir: Fd, new: &[u8], flags: AtFlags) -> SysResult<()>;
    fn symlinkat(&self, target: &[u8], dirfd: Fd, path: &[u8]) -> SysResult<()>;
    fn readlinkat(&self, dirfd: Fd, path: &[u8]) -> SysResult<Vec<u8>>;
    /// FIFO (`mkfifo`) e, pro root, dispositivos.
    fn mknodat(&self, dirfd: Fd, path: &[u8], mode: Mode, dev: u64) -> SysResult<()>;
    fn fchmodat(&self, dirfd: Fd, path: &[u8], mode: Mode, flags: AtFlags) -> SysResult<()>;
    fn fchmod(&self, fd: Fd, mode: Mode) -> SysResult<()>;
    /// `None` não muda o dono/grupo (o `-1` do Linux).
    fn fchownat(&self, dirfd: Fd, path: &[u8], uid: Option<Uid>, gid: Option<Gid>, flags: AtFlags) -> SysResult<()>;
    fn utimensat(&self, dirfd: Fd, path: &[u8], atime: SetTime, mtime: SetTime, flags: AtFlags) -> SysResult<()>;
    fn futimens(&self, fd: Fd, atime: SetTime, mtime: SetTime) -> SysResult<()>;
    fn ftruncate(&self, fd: Fd, len: u64) -> SysResult<()>;
    fn fsync(&self, fd: Fd) -> SysResult<()>;
    /// Próximo lote de entradas de um diretório aberto (como `getdents64`); vazio no fim. Inclui `.` e
    /// `..`, na ordem do sistema de arquivos.
    fn getdents(&self, fd: Fd) -> SysResult<Vec<DirEntry>>;
    fn statfs(&self, path: &[u8]) -> SysResult<StatFs>;
    fn fstatfs(&self, fd: Fd) -> SysResult<StatFs>;

    // ---- descritores ----
    fn dup(&self, fd: Fd) -> SysResult<Fd>;
    /// `dup3`; `cloexec` liga `FD_CLOEXEC` no novo. `old == new` é EINVAL (como `dup3`).
    fn dup3(&self, old: Fd, new: Fd, cloexec: bool) -> SysResult<Fd>;
    /// Menor fd livre `>= min` (`F_DUPFD`, `F_DUPFD_CLOEXEC`).
    fn dup_min(&self, fd: Fd, min: Fd, cloexec: bool) -> SysResult<Fd>;
    fn get_cloexec(&self, fd: Fd) -> SysResult<bool>;
    fn set_cloexec(&self, fd: Fd, on: bool) -> SysResult<()>;
    /// `F_GETFL`.
    fn get_status_flags(&self, fd: Fd) -> SysResult<OFlags>;
    /// `F_SETFL` (só APPEND e NONBLOCK mudam, como no Linux).
    fn set_status_flags(&self, fd: Fd, flags: OFlags) -> SysResult<()>;
    fn pipe2(&self, flags: OFlags) -> SysResult<(Fd, Fd)>;
    fn isatty(&self, fd: Fd) -> bool;
    fn tcgetwinsize(&self, fd: Fd) -> SysResult<Winsize>;
    /// Fds abertos (pra `/proc/self/fd` e pra ferramentas que fecham tudo).
    fn open_fds(&self) -> Vec<Fd>;

    // ---- diretório corrente e máscara ----
    fn chdir(&self, path: &[u8]) -> SysResult<()>;
    fn fchdir(&self, fd: Fd) -> SysResult<()>;
    fn getcwd(&self) -> SysResult<Vec<u8>>;
    fn umask(&self, mask: Mode) -> Mode;

    // ---- processos ----
    fn getpid(&self) -> Pid;
    fn getppid(&self) -> Pid;
    fn getpgid(&self, pid: Pid) -> SysResult<Pid>;
    fn setpgid(&self, pid: Pid, pgid: Pid) -> SysResult<()>;
    fn getsid(&self, pid: Pid) -> SysResult<Pid>;
    fn setsid(&self) -> SysResult<Pid>;
    /// Executa um programa num processo novo (`posix_spawn`). ENOENT, EACCES, ENOEXEC como o `execve`.
    fn spawn(&self, spec: SpawnSpec) -> SysResult<Pid>;
    /// Cria um processo novo que herda uma cópia do estado deste (cwd, ambiente, umask, fds com as
    /// ações aplicadas, disposições de sinal, rlimits) e roda `body` nele. É o `fork` do pseudo-linus:
    /// o shell usa pra subshell, pipeline e substituição de comando.
    fn spawn_fn(&self, attrs: ProcAttrs, name: Vec<u8>, body: ProcessFn) -> SysResult<Pid>;
    /// Substitui o programa do processo corrente (mesmo pid). Só volta em caso de erro.
    fn execve(&self, path: &[u8], argv: &[Vec<u8>], env: Option<&[Vec<u8>]>) -> Errno;
    /// `None` com `NOHANG` e nenhum filho mudou; ECHILD sem filhos.
    fn wait4(&self, target: WaitTarget, options: WaitOptions) -> SysResult<Option<(Pid, WaitStatus)>>;
    fn kill(&self, target: KillTarget, sig: Signal) -> SysResult<()>;
    fn sigaction(&self, sig: Signal, disposition: SigDisposition) -> SysResult<SigDisposition>;
    /// Sinais com disposição `Catch` que chegaram desde a última chamada, na ordem de chegada.
    fn take_caught_signals(&self) -> Vec<Signal>;
    /// Ponto de preempção e entrega de sinal pra laços de CPU que não fazem syscall.
    fn checkpoint(&self);
    fn sched_yield(&self);
    fn getpriority(&self, pid: Pid) -> SysResult<i32>;
    fn setpriority(&self, pid: Pid, nice: i32) -> SysResult<()>;
    fn getrlimit(&self, res: Resource) -> SysResult<Rlimit>;
    fn setrlimit(&self, res: Resource, lim: Rlimit) -> SysResult<()>;
    fn getrusage(&self, who: RusageWho) -> SysResult<Rusage>;
    /// Processos visíveis neste sandbox.
    fn list_processes(&self) -> Vec<ProcInfo>;
    /// Thread nova no mesmo processo (`clone` com `CLONE_THREAD`): divide memória, fds e cwd, é
    /// escalonada como qualquer thread e morre junto com o processo. `exit` em qualquer thread termina o
    /// processo inteiro (como `exit_group`); a thread acaba normalmente quando `body` volta.
    fn spawn_thread(&self, body: ThreadFn) -> SysResult<Tid>;
    /// Espera a thread `tid` terminar (`pthread_join`). ESRCH se não existe, EINVAL se já foi juntada,
    /// EDEADLK se for a própria.
    fn join_thread(&self, tid: Tid) -> SysResult<()>;
    fn gettid(&self) -> Tid;
    /// CPUs em que o processo pode rodar (`sched_getaffinity`): as CPUs virtuais do sandbox.
    fn sched_getaffinity(&self) -> Vec<usize>;

    // ---- identidade e ambiente ----
    fn getuid(&self) -> Uid;
    fn geteuid(&self) -> Uid;
    fn getgid(&self) -> Gid;
    fn getegid(&self) -> Gid;
    fn getgroups(&self) -> Vec<Gid>;
    /// argv do processo (o mesmo passado ao `main`).
    fn argv(&self) -> Vec<Vec<u8>>;
    /// Ambiente do processo, `NAME=valor`.
    fn environ(&self) -> Vec<Vec<u8>>;
    fn getenv(&self, name: &[u8]) -> Option<Vec<u8>>;
    fn setenv(&self, name: &[u8], value: &[u8]) -> SysResult<()>;
    fn unsetenv(&self, name: &[u8]) -> SysResult<()>;
    fn uname(&self) -> Utsname;
    fn sethostname(&self, name: &[u8]) -> SysResult<()>;

    // ---- tempo e aleatoriedade ----
    fn clock_gettime(&self, clock: Clock) -> SysResult<TimeSpec>;
    /// EINTR se um sinal capturado chegar antes do fim.
    fn nanosleep(&self, d: Duration) -> SysResult<()>;
    fn getrandom(&self, buf: &mut [u8]) -> SysResult<usize>;
    /// Fuso local do sandbox (conteúdo de `/etc/localtime` ou `TZ`), pra ferramentas de data.
    fn local_timezone(&self) -> Vec<u8>;

    // ---- espera e travas ----
    /// `poll(2)` em qualquer fd (socket, pipe, arquivo). `None` espera sem limite. EINTR com sinal
    /// capturado. Devolve quantas entradas têm `revents` não vazio.
    fn poll(&self, fds: &mut [PollFd], timeout: Option<Duration>) -> SysResult<usize>;
    /// `F_OFD_SETLK`/`F_OFD_SETLKW`: o dono da trava é a open file description, e ela solta quando o
    /// último fd que a referencia fecha. `wait = false` dá EAGAIN em conflito; `wait = true` bloqueia
    /// (EINTR com sinal capturado, EDEADLK em impasse).
    fn ofd_setlk(&self, fd: Fd, lock: FileLock, wait: bool) -> SysResult<()>;
    /// `F_OFD_GETLK`: a primeira trava de outro dono que conflita com `lock`, ou `None`.
    fn ofd_getlk(&self, fd: Fd, lock: FileLock) -> SysResult<Option<FileLock>>;

    // ---- rede (resolução, política de allowlist e conexão ficam no kernel) ----
    /// Conexão TCP. `timeout` cobre resolução e conexão. Erros: ENOENT = nome não resolve; EACCES = a
    /// política negou (nome fora da allowlist ou endereço interno); ECONNREFUSED, ETIMEDOUT,
    /// ENETUNREACH, EHOSTUNREACH como no `connect(2)`.
    fn net_connect(&self, host: &[u8], port: u16, timeout: Option<Duration>) -> SysResult<NetConn>;
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<dyn Syscalls>>> = const { RefCell::new(None) };
}

/// Instala o processo corrente da thread. Só o kernel (e o testkit) chamam isto.
pub fn install(sys: Arc<dyn Syscalls>) -> Option<Arc<dyn Syscalls>> {
    CURRENT.with(|c| c.borrow_mut().replace(sys))
}

/// Remove o processo corrente da thread.
pub fn uninstall() -> Option<Arc<dyn Syscalls>> {
    CURRENT.with(|c| c.borrow_mut().take())
}

/// O processo corrente. Panic se a thread não pertence a um pseudo-processo (bug do chamador).
pub fn current() -> Arc<dyn Syscalls> {
    CURRENT.with(|c| c.borrow().clone()).expect("thread sem pseudo-processo instalado")
}

pub fn try_current() -> Option<Arc<dyn Syscalls>> {
    CURRENT.with(|c| c.borrow().clone())
}

/// Payload de unwind de `exit`: o kernel captura na entrada do processo e termina com esse código.
#[derive(Debug)]
pub struct ExitUnwind(pub i32);

/// Payload de unwind de morte por sinal (SIGKILL, ou ação padrão de terminar).
#[derive(Debug)]
pub struct KillUnwind(pub Signal);

/// Payload de unwind de `execve` bem-sucedido: o kernel troca o programa no mesmo processo.
pub struct ExecUnwind {
    pub path: Vec<u8>,
    pub argv: Vec<Vec<u8>>,
    pub env: Option<Vec<Vec<u8>>>,
}

/// Termina o processo corrente com `code` (`_exit`). Desenrola a pilha, então os `Drop` rodam.
pub fn exit(code: i32) -> ! {
    std::panic::resume_unwind(Box::new(ExitUnwind(code)))
}

// ---- atalhos de uso comum, sobre o processo corrente ----

pub fn open(path: &[u8], flags: OFlags, mode: Mode) -> SysResult<Fd> {
    current().openat(Fd::CWD, path, flags, mode)
}

pub fn close(fd: Fd) -> SysResult<()> {
    current().close(fd)
}

pub fn read(fd: Fd, buf: &mut [u8]) -> SysResult<usize> {
    current().read(fd, buf)
}

pub fn write(fd: Fd, buf: &[u8]) -> SysResult<usize> {
    current().write(fd, buf)
}

/// Escreve tudo, repetindo em escrita parcial; EINTR é repetido.
pub fn write_all(fd: Fd, mut buf: &[u8]) -> SysResult<()> {
    let sys = current();
    while !buf.is_empty() {
        match sys.write(fd, buf) {
            Ok(0) => return Err(Errno::EIO),
            Ok(n) => buf = &buf[n..],
            Err(Errno::EINTR) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Lê até o fim.
pub fn read_to_end(fd: Fd) -> SysResult<Vec<u8>> {
    let sys = current();
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match sys.read(fd, &mut buf) {
            Ok(0) => return Ok(out),
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(Errno::EINTR) => {}
            Err(e) => return Err(e),
        }
    }
}

pub fn stat(path: &[u8]) -> SysResult<Stat> {
    current().fstatat(Fd::CWD, path, AtFlags::empty())
}

pub fn lstat(path: &[u8]) -> SysResult<Stat> {
    current().fstatat(Fd::CWD, path, AtFlags::SYMLINK_NOFOLLOW)
}

pub fn getenv(name: &str) -> Option<Vec<u8>> {
    current().getenv(name.as_bytes())
}

pub fn checkpoint() {
    if let Some(sys) = try_current() {
        sys.checkpoint();
    }
}

/// Lê um arquivo inteiro.
pub fn read_file(path: &[u8]) -> SysResult<Vec<u8>> {
    let fd = open(path, OFlags::RDONLY | OFlags::CLOEXEC, 0)?;
    let data = read_to_end(fd);
    let _ = close(fd);
    data
}

/// Lista um diretório inteiro (sem `.` e `..`), na ordem do sistema de arquivos.
pub fn read_dir(path: &[u8]) -> SysResult<Vec<DirEntry>> {
    let sys = current();
    let fd = sys.openat(Fd::CWD, path, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC, 0)?;
    let mut out = Vec::new();
    loop {
        match sys.getdents(fd) {
            Ok(batch) if batch.is_empty() => break,
            Ok(batch) => out.extend(batch.into_iter().filter(|e| e.name != b"." && e.name != b"..")),
            Err(e) => {
                let _ = sys.close(fd);
                return Err(e);
            }
        }
    }
    let _ = sys.close(fd);
    Ok(out)
}
