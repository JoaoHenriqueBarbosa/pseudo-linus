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
use crate::sched::{SchedAttr, SchedParam};
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
    /// `fallocate(2)`: reserva (ou, com PUNCH_HOLE, libera) o intervalo `[offset, offset + len)`.
    /// `offset` e `len` são `loff_t` com sinal, como no Linux, pra que os EINVAL de valor negativo saiam
    /// do kernel. Os errnos e a ordem seguem o `vfs_fallocate` e o `shmem_fallocate` do Linux 6.12.
    fn fallocate(&self, fd: Fd, mode: FallocFlags, offset: i64, len: i64) -> SysResult<()>;
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
    /// `TCGETS2`: atributos do terminal. EBADF pra fd ruim, ENOTTY pra fd que não é terminal.
    fn tcgetattr(&self, fd: Fd) -> SysResult<Termios>;
    /// `TCSETS2`/`TCSETSW2`/`TCSETSF2` conforme `when`. Mesmos erros do [`Syscalls::tcgetattr`].
    fn tcsetattr(&self, fd: Fd, when: SetAttrWhen, termios: &Termios) -> SysResult<()>;
    /// `TIOCSWINSZ`. Mudar o tamanho manda SIGWINCH pro grupo em primeiro plano.
    fn tcsetwinsize(&self, fd: Fd, ws: Winsize) -> SysResult<()>;
    /// `TIOCGPTN`: número do pty de um mestre (`/dev/pts/N`). ENOTTY pra fd que não é mestre.
    fn pty_number(&self, fd: Fd) -> SysResult<u32> {
        self.fstat(fd)?;
        Err(Errno::ENOTTY)
    }

    // ---- TCP de loopback (`127.0.0.1`) entre os processos do sandbox ----
    /// `socket` + `bind` + `listen`: porta 0 escolhe uma efêmera. Devolve o fd e a porta.
    fn tcp_listen(&self, port: u16, backlog: u32, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        let _ = (port, backlog, nonblock, cloexec);
        Err(Errno::ENOSYS)
    }
    /// `accept4`: a conexão e a porta de quem conectou.
    fn tcp_accept(&self, fd: Fd, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        let _ = (fd, nonblock, cloexec);
        Err(Errno::ENOSYS)
    }
    /// `socket` + `connect`: a conexão e a porta efêmera local.
    fn tcp_connect(&self, port: u16, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        let _ = (port, nonblock, cloexec);
        Err(Errno::ENOSYS)
    }
    /// `tcp_listen` com o endereço do `bind` (`0.0.0.0`, `127.0.0.1`, `::`), que o `/proc/net/tcp`
    /// mostra e que decide a família do socket.
    fn tcp_listen_at(&self, ip: std::net::IpAddr, port: u16, backlog: u32, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        let _ = ip;
        self.tcp_listen(port, backlog, nonblock, cloexec)
    }
    /// `tcp_connect` a um endereço de loopback (`127.0.0.1`, `::1`): a família do socket é a dele.
    fn tcp_connect_at(&self, ip: std::net::IpAddr, port: u16, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        let _ = ip;
        self.tcp_connect(port, nonblock, cloexec)
    }
    /// `shutdown(2)`: fecha a leitura, a escrita ou as duas.
    fn tcp_shutdown(&self, fd: Fd, read: bool, write: bool) -> SysResult<()> {
        let _ = (fd, read, write);
        Err(Errno::ENOSYS)
    }
    /// Portas local e remota (`getsockname`, `getpeername`); a remota é `None` num socket em escuta.
    fn tcp_ports(&self, fd: Fd) -> SysResult<(u16, Option<u16>)> {
        let _ = fd;
        Err(Errno::ENOSYS)
    }
    /// `TIOCSPTLCK`: trava (`true`) ou destrava o escravo de um mestre (`unlockpt` destrava).
    fn pty_set_lock(&self, fd: Fd, locked: bool) -> SysResult<()> {
        let _ = locked;
        self.fstat(fd)?;
        Err(Errno::ENOTTY)
    }
    /// `TIOCGPGRP`: grupo em primeiro plano. Pelo escravo, só no terminal de controle (ENOTTY).
    fn tcgetpgrp(&self, fd: Fd) -> SysResult<Pid> {
        self.fstat(fd)?;
        Err(Errno::ENOTTY)
    }
    /// `TIOCSPGRP`: só no terminal de controle (ENOTTY); EINVAL pra grupo negativo, ESRCH pra grupo
    /// que não existe, EPERM pra grupo de outra sessão.
    fn tcsetpgrp(&self, fd: Fd, pgrp: Pid) -> SysResult<()> {
        let _ = pgrp;
        self.fstat(fd)?;
        Err(Errno::ENOTTY)
    }
    /// `TIOCGSID`: sessão de que o terminal é terminal de controle.
    fn tcgetsid(&self, fd: Fd) -> SysResult<Pid> {
        self.fstat(fd)?;
        Err(Errno::ENOTTY)
    }
    /// `TIOCSCTTY`: o terminal vira o terminal de controle da sessão de quem chama (que tem de ser
    /// líder e não ter outro). `force` é o argumento 1, que deixa o root roubar de outra sessão.
    fn tiocsctty(&self, fd: Fd, force: bool) -> SysResult<()> {
        let _ = force;
        self.fstat(fd)?;
        Err(Errno::ENOTTY)
    }
    /// `TIOCNOTTY`: solta o terminal de controle.
    fn tiocnotty(&self, fd: Fd) -> SysResult<()> {
        self.fstat(fd)?;
        Err(Errno::ENOTTY)
    }
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
    /// `sched_getaffinity(pid, ...)`: a máscara do processo `pid` (0 é o corrente), só com CPUs online.
    /// ESRCH se o processo não existe.
    fn sched_getaffinity_of(&self, pid: Pid) -> SysResult<Vec<usize>> {
        let _ = pid;
        Err(Errno::ENOSYS)
    }
    /// `sched_setaffinity(pid, mask)`. A máscara é a lista de CPUs; as que não estão online são
    /// ignoradas e, se não sobra nenhuma, EINVAL. ESRCH se o processo não existe, EPERM se é de outro
    /// dono (o contêiner padrão não tem `CAP_SYS_NICE`). O filho herda a máscara no fork.
    fn sched_setaffinity(&self, pid: Pid, cpus: &[usize]) -> SysResult<()> {
        let _ = (pid, cpus);
        Err(Errno::ENOSYS)
    }
    /// `personality(2)`: devolve a personality de antes; `0xffffffff` só lê. É por processo, herdada no
    /// fork e no exec (o exec setuid apaga `PER_CLEAR_ON_SETID`). Segue o seccomp padrão do docker: só
    /// 0x0, 0x8, 0x20000, 0x20008 e a leitura passam, o resto é EPERM. `uname` reflete `PER_LINUX32`
    /// (`machine` vira `i686`) e `UNAME26` (`release` vira `2.6.N`).
    fn personality(&self, persona: u32) -> SysResult<u32> {
        let _ = persona;
        Err(Errno::ENOSYS)
    }
    /// `sched_getscheduler`: a política, com `SCHED_RESET_ON_FORK` se ligado. EINVAL pra pid negativo.
    fn sched_getscheduler(&self, pid: Pid) -> SysResult<i32> {
        let _ = pid;
        Err(Errno::ENOSYS)
    }
    /// `sched_setscheduler`: política (0 OTHER, 1 FIFO, 2 RR, 3 BATCH, 5 IDLE, com `SCHED_RESET_ON_FORK`
    /// opcional) e prioridade. EINVAL de forma, ESRCH, e EPERM sem `CAP_SYS_NICE` pra tempo real e outras
    /// escaladas (ver [`crate::sched`]).
    fn sched_setscheduler(&self, pid: Pid, policy: i32, param: SchedParam) -> SysResult<()> {
        let _ = (pid, policy, param);
        Err(Errno::ENOSYS)
    }
    /// `sched_getparam`.
    fn sched_getparam(&self, pid: Pid) -> SysResult<SchedParam> {
        let _ = pid;
        Err(Errno::ENOSYS)
    }
    /// `sched_setparam`: muda só a prioridade, a política fica.
    fn sched_setparam(&self, pid: Pid, param: SchedParam) -> SysResult<()> {
        let _ = (pid, param);
        Err(Errno::ENOSYS)
    }
    fn sched_get_priority_min(&self, policy: i32) -> SysResult<i32> {
        crate::sched::priority_min(policy)
    }
    fn sched_get_priority_max(&self, policy: i32) -> SysResult<i32> {
        crate::sched::priority_max(policy)
    }
    /// `sched_rr_get_interval`: a fatia (100 ms em RR, 0 em FIFO).
    fn sched_rr_get_interval(&self, pid: Pid) -> SysResult<Duration> {
        let _ = pid;
        Err(Errno::ENOSYS)
    }
    /// `sched_getattr(pid, attr, size, flags)`: `size` entre 48 e 4096 e `flags` 0, senão EINVAL.
    fn sched_getattr(&self, pid: Pid, size: u32, flags: u32) -> SysResult<SchedAttr> {
        let _ = (pid, size, flags);
        Err(Errno::ENOSYS)
    }
    /// `sched_setattr(pid, attr, flags)`: `attr.size` abaixo de 48 é E2BIG (0 vale 48), `flags` não zero
    /// é EINVAL.
    fn sched_setattr(&self, pid: Pid, attr: &SchedAttr, flags: u32) -> SysResult<()> {
        let _ = (pid, attr, flags);
        Err(Errno::ENOSYS)
    }
    /// `ioprio_get(which, who)`: `which` é `IOPRIO_WHO_PROCESS` (1), `PGRP` (2) ou `USER` (3), `who` 0
    /// é o corrente. Devolve `(classe << 13) | dados`; sem valor gravado, a classe BE com nível
    /// `(nice + 20) / 5` (IDLE ou RT se a política do processo for essa). ESRCH sem alvo.
    fn ioprio_get(&self, which: i32, who: i32) -> SysResult<i32> {
        let _ = (which, who);
        Err(Errno::ENOSYS)
    }
    /// `ioprio_set(which, who, ioprio)`. EINVAL pra classe ou nível inválido, EPERM pra RT (sem
    /// `CAP_SYS_ADMIN`) e pra processo de outro dono.
    fn ioprio_set(&self, which: i32, who: i32, ioprio: i32) -> SysResult<()> {
        let _ = (which, who, ioprio);
        Err(Errno::ENOSYS)
    }

    // ---- identidade e ambiente ----
    fn getuid(&self) -> Uid;
    fn geteuid(&self) -> Uid;
    fn getgid(&self) -> Gid;
    fn getegid(&self) -> Gid;
    fn getgroups(&self) -> Vec<Gid>;
    /// `getresuid`: (real, efetivo, salvo).
    fn getresuid(&self) -> (Uid, Uid, Uid) {
        let (r, e) = (self.getuid(), self.geteuid());
        (r, e, e)
    }
    /// `getresgid`: (real, efetivo, salvo).
    fn getresgid(&self) -> (Gid, Gid, Gid) {
        let (r, e) = (self.getgid(), self.getegid());
        (r, e, e)
    }
    /// `setresuid(r, e, s)`; [`ID_UNCHANGED`] (o `-1` do C) mantém o valor.
    fn setresuid(&self, r: Uid, e: Uid, s: Uid) -> SysResult<()> {
        let _ = (r, e, s);
        Err(Errno::EPERM)
    }
    fn setresgid(&self, r: Gid, e: Gid, s: Gid) -> SysResult<()> {
        let _ = (r, e, s);
        Err(Errno::EPERM)
    }
    /// `setreuid(r, e)` com a regra do salvo do Linux.
    fn setreuid(&self, r: Uid, e: Uid) -> SysResult<()> {
        let _ = (r, e);
        Err(Errno::EPERM)
    }
    fn setregid(&self, r: Gid, e: Gid) -> SysResult<()> {
        let _ = (r, e);
        Err(Errno::EPERM)
    }
    /// `setuid(uid)`: com `CAP_SETUID` troca real, efetivo e salvo; sem, só o efetivo (para o real ou
    /// o salvo).
    fn setuid(&self, uid: Uid) -> SysResult<()> {
        let _ = uid;
        Err(Errno::EPERM)
    }
    fn setgid(&self, gid: Gid) -> SysResult<()> {
        let _ = gid;
        Err(Errno::EPERM)
    }
    /// `setgroups`: exige `CAP_SETGID`; mais de `NGROUPS_MAX` (65536) dá EINVAL.
    fn setgroups(&self, groups: &[Gid]) -> SysResult<()> {
        let _ = groups;
        Err(Errno::EPERM)
    }
    /// argv do processo (o mesmo passado ao `main`).
    fn argv(&self) -> Vec<Vec<u8>>;
    /// Ambiente do processo, `NAME=valor`.
    fn environ(&self) -> Vec<Vec<u8>>;
    fn getenv(&self, name: &[u8]) -> Option<Vec<u8>>;
    fn setenv(&self, name: &[u8], value: &[u8]) -> SysResult<()>;
    fn unsetenv(&self, name: &[u8]) -> SysResult<()>;
    fn uname(&self) -> Utsname;
    fn sethostname(&self, name: &[u8]) -> SysResult<()>;
    /// `setdomainname(2)`: o domínio NIS do `uname`.
    fn setdomainname(&self, name: &[u8]) -> SysResult<()>;

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

/// `fallocate(2)` sobre o processo corrente.
pub fn fallocate(fd: Fd, mode: FallocFlags, offset: i64, len: i64) -> SysResult<()> {
    current().fallocate(fd, mode, offset, len)
}

/// `personality(2)` sobre o processo corrente; `0xffffffff` só lê.
pub fn personality(persona: u32) -> SysResult<u32> {
    current().personality(persona)
}

pub fn sched_getscheduler(pid: Pid) -> SysResult<i32> {
    current().sched_getscheduler(pid)
}

pub fn sched_setscheduler(pid: Pid, policy: i32, param: SchedParam) -> SysResult<()> {
    current().sched_setscheduler(pid, policy, param)
}

pub fn sched_getparam(pid: Pid) -> SysResult<SchedParam> {
    current().sched_getparam(pid)
}

pub fn sched_setparam(pid: Pid, param: SchedParam) -> SysResult<()> {
    current().sched_setparam(pid, param)
}

pub fn sched_get_priority_min(policy: i32) -> SysResult<i32> {
    current().sched_get_priority_min(policy)
}

pub fn sched_get_priority_max(policy: i32) -> SysResult<i32> {
    current().sched_get_priority_max(policy)
}

pub fn sched_rr_get_interval(pid: Pid) -> SysResult<Duration> {
    current().sched_rr_get_interval(pid)
}

pub fn sched_getattr(pid: Pid, size: u32, flags: u32) -> SysResult<SchedAttr> {
    current().sched_getattr(pid, size, flags)
}

pub fn sched_setattr(pid: Pid, attr: &SchedAttr, flags: u32) -> SysResult<()> {
    current().sched_setattr(pid, attr, flags)
}

/// `sched_getaffinity(pid)`: a máscara do processo `pid` (0 é o corrente).
pub fn sched_getaffinity_of(pid: Pid) -> SysResult<Vec<usize>> {
    current().sched_getaffinity_of(pid)
}

pub fn sched_setaffinity(pid: Pid, cpus: &[usize]) -> SysResult<()> {
    current().sched_setaffinity(pid, cpus)
}

pub fn ioprio_get(which: i32, who: i32) -> SysResult<i32> {
    current().ioprio_get(which, who)
}

pub fn ioprio_set(which: i32, who: i32, ioprio: i32) -> SysResult<()> {
    current().ioprio_set(which, who, ioprio)
}

pub fn tcgetattr(fd: Fd) -> SysResult<Termios> {
    current().tcgetattr(fd)
}

pub fn tcsetattr(fd: Fd, when: SetAttrWhen, termios: &Termios) -> SysResult<()> {
    current().tcsetattr(fd, when, termios)
}

pub fn tcgetwinsize(fd: Fd) -> SysResult<Winsize> {
    current().tcgetwinsize(fd)
}

pub fn tcsetwinsize(fd: Fd, ws: Winsize) -> SysResult<()> {
    current().tcsetwinsize(fd, ws)
}

pub fn tcgetpgrp(fd: Fd) -> SysResult<Pid> {
    current().tcgetpgrp(fd)
}

pub fn tcp_listen(port: u16, backlog: u32, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
    current().tcp_listen(port, backlog, nonblock, cloexec)
}

pub fn tcp_accept(fd: Fd, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
    current().tcp_accept(fd, nonblock, cloexec)
}

pub fn tcp_connect(port: u16, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
    current().tcp_connect(port, nonblock, cloexec)
}

pub fn tcp_listen_at(ip: std::net::IpAddr, port: u16, backlog: u32, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
    current().tcp_listen_at(ip, port, backlog, nonblock, cloexec)
}

pub fn tcp_connect_at(ip: std::net::IpAddr, port: u16, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
    current().tcp_connect_at(ip, port, nonblock, cloexec)
}

pub fn tcp_shutdown(fd: Fd, read: bool, write: bool) -> SysResult<()> {
    current().tcp_shutdown(fd, read, write)
}

pub fn tcp_ports(fd: Fd) -> SysResult<(u16, Option<u16>)> {
    current().tcp_ports(fd)
}

pub fn tcsetpgrp(fd: Fd, pgrp: Pid) -> SysResult<()> {
    current().tcsetpgrp(fd, pgrp)
}

pub fn tcgetsid(fd: Fd) -> SysResult<Pid> {
    current().tcgetsid(fd)
}

/// `posix_openpt` da glibc: `open("/dev/ptmx", flags)`.
pub fn posix_openpt(flags: OFlags) -> SysResult<Fd> {
    open(b"/dev/ptmx", flags, 0)
}

/// `grantpt` da glibc no Linux: o devpts já cria o nó com o dono, o grupo `tty` e o modo certos, então
/// só confere que o fd é um mestre (EINVAL se não é).
pub fn grantpt(fd: Fd) -> SysResult<()> {
    match current().pty_number(fd) {
        Ok(_) => Ok(()),
        Err(Errno::ENOTTY) => Err(Errno::EINVAL),
        Err(e) => Err(e),
    }
}

/// `unlockpt`: `TIOCSPTLCK` com 0 (EINVAL se o fd não é mestre).
pub fn unlockpt(fd: Fd) -> SysResult<()> {
    match current().pty_set_lock(fd, false) {
        Err(Errno::ENOTTY) => Err(Errno::EINVAL),
        r => r,
    }
}

/// `ptsname`: `/dev/pts/N` pelo `TIOCGPTN` (ENOTTY se o fd não é mestre).
pub fn ptsname(fd: Fd) -> SysResult<Vec<u8>> {
    let n = current().pty_number(fd)?;
    Ok(format!("/dev/pts/{n}").into_bytes())
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
