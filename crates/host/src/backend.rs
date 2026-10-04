//! O que o worker precisa do kernel do pseudo-linus, como traits.
//!
//! O worker fala com o kernel só por aqui. Tudo é chamado de threads do host (nunca de dentro de um
//! pseudo-processo): criar sandbox, lançar processo com stdin/stdout/stderr ligados a pontas de pipe
//! que o host lê e escreve, esperar, sinalizar, listar processos, mexer no sistema de arquivos como root
//! e tirar snapshot. A lógica de `exec` (timeout de parede, limite de saída, escoamento), das sessões e
//! do tar fica no host ([`crate::exec`], [`crate::session`], [`crate::tarball`]) em cima destas
//! primitivas.

use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sysabi::{DirEntry, Errno, KillTarget, Mode, Pid, ProcInfo, Signal, Stat, TimeSpec, WaitStatus};

use crate::api::{SandboxLimits, SandboxUsage};
use crate::config::CpuMax;
use crate::rpc::{RpcError, codes};

/// Grupo de escalonamento de um usuário (o nível de cima da hierarquia usuário > sandbox > processo).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSched {
    pub user: String,
    pub cpu_weight: u32,
    pub cpu_max: Option<CpuMax>,
}

/// O que o kernel precisa pra montar uma sandbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxSpec {
    pub image: String,
    pub hostname: String,
    pub limits: SandboxLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BackendError {
    /// Erro de syscall dentro da sandbox.
    #[error("{}", fmt_os(*.errno, .context))]
    Os { errno: Errno, context: String },
    /// Um limite da sandbox impediu a operação (processos, memória, cota do tmpfs).
    #[error("{0}")]
    Limit(String),
    /// O isolamento exigido não pôde ser aplicado.
    #[error("{0}")]
    Isolation(String),
    /// A sandbox já foi destruída.
    #[error("a sandbox foi destruída")]
    Gone,
    #[error("{0}")]
    Internal(String),
}

fn fmt_os(errno: Errno, context: &str) -> String {
    if context.is_empty() { errno.message() } else { format!("{context}: {}", errno.message()) }
}

impl BackendError {
    pub fn os(errno: Errno, context: impl Into<String>) -> BackendError {
        BackendError::Os { errno, context: context.into() }
    }

    /// Mesmo erro com outro contexto (o caminho que o cliente pediu, por exemplo).
    pub fn context(self, ctx: &str) -> BackendError {
        match self {
            BackendError::Os { errno, .. } => BackendError::Os { errno, context: ctx.to_string() },
            other => other,
        }
    }
}

impl From<BackendError> for RpcError {
    fn from(e: BackendError) -> RpcError {
        match e {
            BackendError::Os { errno, context } => RpcError::errno(errno, &context),
            BackendError::Limit(m) => RpcError::new(codes::QUOTA_EXCEEDED, "sandbox_limit", m),
            BackendError::Isolation(m) => RpcError::new(codes::ISOLATION, "isolation", m),
            BackendError::Gone => RpcError::new(codes::NOT_FOUND, "not_found", "a sandbox foi destruída"),
            BackendError::Internal(m) => RpcError::internal(m),
        }
    }
}

pub type BResult<T> = Result<T, BackendError>;

/// Resultado de uma leitura com prazo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOutcome {
    Data(usize),
    /// Todos os escritores fecharam.
    Eof,
    TimedOut,
}

/// Resultado de uma escrita com prazo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    Wrote(usize),
    /// Não há mais leitores (o EPIPE do lado do host; o host não leva SIGPIPE).
    Closed,
    TimedOut,
}

/// Ponta de leitura de um pipe do kernel, do lado do host. Soltar o objeto fecha a ponta.
pub trait HostReader: Send {
    fn read_timeout(&mut self, buf: &mut [u8], timeout: Duration) -> ReadOutcome;
}

/// Ponta de escrita de um pipe do kernel, do lado do host. Soltar o objeto fecha a ponta (EOF pro
/// processo).
pub trait HostWriter: Send {
    fn write_timeout(&mut self, buf: &[u8], timeout: Duration) -> WriteOutcome;
}

/// Como um processo terminou, com o tempo de CPU que ele gastou.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitInfo {
    pub status: WaitStatus,
    pub cpu_ns: u64,
}

/// Espera o término de um processo lançado pelo host (o host é o "pai" dele).
pub trait ExitWaiter: Send {
    /// `None` se não terminou dentro do prazo.
    fn wait_timeout(&mut self, timeout: Duration) -> Option<ExitInfo>;
}

/// Processo novo. O kernel resolve `path` como o `execve` (sem PATH); o host já fez a busca.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnRequest {
    pub path: Vec<u8>,
    pub argv: Vec<Vec<u8>>,
    /// Ambiente completo (`NOME=valor`).
    pub env: Vec<Vec<u8>>,
    pub cwd: Vec<u8>,
}

/// Processo lançado: pid (que também é o pgid e o sid, porque o processo nasce em sessão e grupo
/// próprios), as três pontas de pipe e quem espera o término.
pub struct Spawned {
    pub pid: Pid,
    pub stdin: Box<dyn HostWriter>,
    pub stdout: Box<dyn HostReader>,
    pub stderr: Box<dyn HostReader>,
    pub exit: Box<dyn ExitWaiter>,
}

impl std::fmt::Debug for Spawned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spawned").field("pid", &self.pid).finish_non_exhaustive()
    }
}

/// Como abrir um arquivo pra escrita direta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteOpts {
    /// Acrescenta no fim; falso trunca.
    pub append: bool,
    /// Falha com EEXIST se já existe.
    pub exclusive: bool,
    /// Modo de arquivo novo (a umask não se aplica: o host passa o modo final).
    pub mode: Mode,
}

/// Snapshot opaco de uma sandbox (do kernel: O(1) sobre o tmpfs persistente).
pub type SnapshotToken = Arc<dyn Any + Send + Sync>;

/// O que o worker informa ao supervisor sobre o kernel dele.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendInfo {
    pub name: String,
    pub cpus: usize,
    pub programs: usize,
    /// Relatório do isolamento em runtime aplicado nas spawner threads.
    pub isolation: String,
}

/// O kernel de um worker.
pub trait Backend: Send + Sync + 'static {
    fn info(&self) -> BackendInfo;
    /// Cria ou atualiza o grupo de escalonamento do usuário.
    fn ensure_user(&self, user: &UserSched) -> BResult<()>;
    /// Monta uma sandbox nova no grupo do usuário.
    fn create_sandbox(&self, sandbox_id: &str, user: &str, spec: &SandboxSpec) -> BResult<Arc<dyn Sandbox>>;
}

/// Uma sandbox: processos e sistema de arquivos próprios.
pub trait Sandbox: Send + Sync {
    /// Lança um processo em sessão e grupo próprios (`setsid`), com stdin, stdout e stderr ligados ao
    /// host.
    fn spawn(&self, req: SpawnRequest) -> BResult<Spawned>;
    fn kill(&self, target: KillTarget, sig: Signal) -> BResult<()>;
    fn processes(&self) -> Vec<ProcInfo>;

    fn stat(&self, path: &[u8], follow: bool) -> BResult<Stat>;
    /// Entradas sem `.` e `..`, na ordem do sistema de arquivos.
    fn read_dir(&self, path: &[u8]) -> BResult<Vec<DirEntry>>;
    /// Até `max` bytes a partir de `offset` (menos só no fim do arquivo).
    fn read_file(&self, path: &[u8], offset: u64, max: usize) -> BResult<Vec<u8>>;
    fn write_file(&self, path: &[u8], data: &[u8], opts: WriteOpts) -> BResult<()>;
    fn mkdir(&self, path: &[u8], mode: Mode) -> BResult<()>;
    fn unlink(&self, path: &[u8]) -> BResult<()>;
    fn rmdir(&self, path: &[u8]) -> BResult<()>;
    fn symlink(&self, target: &[u8], path: &[u8]) -> BResult<()>;
    fn readlink(&self, path: &[u8]) -> BResult<Vec<u8>>;
    fn link(&self, existing: &[u8], new: &[u8]) -> BResult<()>;
    /// FIFO e dispositivos (no import de tar).
    fn mknod(&self, path: &[u8], mode: Mode, dev: u64) -> BResult<()>;
    fn chmod(&self, path: &[u8], mode: Mode) -> BResult<()>;
    fn chown(&self, path: &[u8], uid: u32, gid: u32, follow: bool) -> BResult<()>;
    fn set_times(&self, path: &[u8], atime: TimeSpec, mtime: TimeSpec, follow: bool) -> BResult<()>;

    fn snapshot(&self) -> BResult<SnapshotToken>;
    /// Volta o sistema de arquivos ao snapshot. Os processos vivos continuam (com os fds que têm).
    fn restore(&self, snap: &SnapshotToken) -> BResult<()>;
    fn usage(&self) -> SandboxUsage;
    /// Mata tudo e libera. Depois disso toda operação dá [`BackendError::Gone`].
    fn destroy(&self);
}

/// Busca de `PATH` como o `execvp`: nome com barra vale como está; senão, o primeiro diretório do
/// `PATH` com um arquivo regular executável. Devolve o errno do `execvp` em falha.
pub fn resolve_program(sb: &dyn Sandbox, name: &[u8], path_var: Option<&[u8]>, cwd: &[u8]) -> BResult<Vec<u8>> {
    let absolute = |p: &[u8]| -> Vec<u8> {
        if p.starts_with(b"/") {
            p.to_vec()
        } else {
            let mut v = cwd.to_vec();
            if !v.ends_with(b"/") {
                v.push(b'/');
            }
            v.extend_from_slice(p);
            v
        }
    };
    let ctx = String::from_utf8_lossy(name).into_owned();
    if name.is_empty() {
        return Err(BackendError::os(Errno::ENOENT, ctx));
    }
    if name.contains(&b'/') {
        return Ok(absolute(name));
    }
    let path_var = path_var.unwrap_or(b"/usr/local/bin:/usr/bin:/bin");
    let mut eacces = false;
    for dir in path_var.split(|b| *b == b':') {
        let dir: &[u8] = if dir.is_empty() { b"." } else { dir };
        let mut cand = absolute(dir);
        if !cand.ends_with(b"/") {
            cand.push(b'/');
        }
        cand.extend_from_slice(name);
        match sb.stat(&cand, true) {
            Ok(st) if st.file_type() == sysabi::FileType::Regular => {
                if st.mode & 0o111 != 0 {
                    return Ok(cand);
                }
                eacces = true;
            }
            Ok(_) => {}
            Err(BackendError::Os { errno, .. }) if errno == Errno::EACCES => eacces = true,
            Err(BackendError::Os { .. }) => {}
            Err(e) => return Err(e),
        }
    }
    Err(BackendError::os(if eacces { Errno::EACCES } else { Errno::ENOENT }, ctx))
}

/// Junta `base` e `rel` como o kernel resolveria um caminho relativo (só concatena; quem resolve `..`
/// e symlinks é o namei do kernel).
pub fn join_path(base: &[u8], rel: &[u8]) -> Vec<u8> {
    if rel.starts_with(b"/") {
        return rel.to_vec();
    }
    let mut v = base.to_vec();
    if !v.ends_with(b"/") {
        v.push(b'/');
    }
    v.extend_from_slice(rel);
    v
}
