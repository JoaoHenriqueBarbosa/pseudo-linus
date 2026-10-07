//! O backend de verdade: o kernel do pseudo-linus (`crates/kernel`) com a tabela de programas do
//! `crates/userland`.
//!
//! - Um [`kernel::Kernel`] por worker, com `cpus` CPUs virtuais e o hook da spawner thread aplicando o
//!   [`IsolationProfile`] (Landlock e seccomp) em cada sandbox antes do primeiro processo.
//! - Cada sandbox é um [`kernel::Sandbox`]; as primitivas do trait [`Sandbox`] mapeiam 1:1 na API do
//!   kernel (`spawn` com `HostStdio`, `wait`, `kill`, `processes`, `fs()`, `snapshot`, `restore`).
//! - Processo criado pelo host abre sessão e grupo novos (pgid = sid = pid), o que o timeout de parede
//!   usa pra matar o comando inteiro.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use sysabi::{DirEntry, Errno, KillTarget, Mode, Pid, ProcAttrs, ProcInfo, Program, SetTime, Signal, SpawnSpec, Stat, TimeSpec, WaitStatus};

use crate::api::SandboxUsage;
use crate::backend::{
    BResult, Backend, BackendError, BackendInfo, ExitInfo, ExitWaiter, HostReader, HostWriter, ReadOutcome, Sandbox,
    SandboxSpec, SnapshotToken, SpawnRequest, Spawned, UserSched, WriteOpts, WriteOutcome,
};
use crate::isolation::{IsolationProfile, IsolationReport};

fn os(e: Errno, ctx: &[u8]) -> BackendError {
    BackendError::os(e, String::from_utf8_lossy(ctx))
}

struct KReader(kernel::HostReader);

impl HostReader for KReader {
    fn read_timeout(&mut self, buf: &mut [u8], timeout: Duration) -> ReadOutcome {
        match self.0.read(buf, Some(Instant::now() + timeout)) {
            kernel::ReadOutcome::Data(n) => ReadOutcome::Data(n),
            kernel::ReadOutcome::Eof => ReadOutcome::Eof,
            kernel::ReadOutcome::Timeout => ReadOutcome::TimedOut,
        }
    }
}

/// Ponta de stdin ausente (o kernel devolve `None` quando o stdio não é pipe).
struct Closed;

impl HostReader for Closed {
    fn read_timeout(&mut self, _buf: &mut [u8], _timeout: Duration) -> ReadOutcome {
        ReadOutcome::Eof
    }
}

impl HostWriter for Closed {
    fn write_timeout(&mut self, _buf: &[u8], _timeout: Duration) -> WriteOutcome {
        WriteOutcome::Closed
    }
}

struct KWriter(kernel::HostWriter);

impl HostWriter for KWriter {
    fn write_timeout(&mut self, buf: &[u8], timeout: Duration) -> WriteOutcome {
        match self.0.write_all(buf, Some(Instant::now() + timeout)) {
            Ok(()) => WriteOutcome::Wrote(buf.len()),
            Err((_, written)) if written > 0 => WriteOutcome::Wrote(written),
            Err((e, _)) if e == Errno::ETIMEDOUT => WriteOutcome::TimedOut,
            Err(_) => WriteOutcome::Closed,
        }
    }
}

struct KWaiter {
    sb: Arc<KSandbox>,
    pid: Pid,
}

impl ExitWaiter for KWaiter {
    fn wait_timeout(&mut self, timeout: Duration) -> Option<ExitInfo> {
        let guard = self.sb.inner.read();
        let Some(sb) = guard.as_ref() else {
            // A sandbox foi destruída: o processo morreu com ela.
            return Some(ExitInfo { status: WaitStatus::Signaled { signal: Signal::SIGKILL, core_dumped: false }, cpu_ns: 0 });
        };
        match sb.wait(self.pid, Some(Instant::now() + timeout)) {
            Ok(Some((status, ru))) => Some(ExitInfo { status, cpu_ns: (ru.utime + ru.stime).as_nanos() as u64 }),
            Ok(None) => None,
            // ECHILD: já colhido ou nunca foi filho do host; não há mais o que esperar.
            Err(_) => Some(ExitInfo { status: WaitStatus::Signaled { signal: Signal::SIGKILL, core_dumped: false }, cpu_ns: 0 }),
        }
    }
}

/// Uma sandbox do kernel. O `RwLock<Option<..>>` deixa destruir (que consome o valor) enquanto outras
/// threads ainda seguram a referência: depois do `destroy`, toda operação dá [`BackendError::Gone`].
pub struct KSandbox {
    inner: RwLock<Option<kernel::Sandbox>>,
    me: Mutex<std::sync::Weak<KSandbox>>,
}

impl KSandbox {
    fn with<R>(&self, f: impl FnOnce(&kernel::Sandbox) -> BResult<R>) -> BResult<R> {
        let g = self.inner.read();
        match g.as_ref() {
            Some(sb) => f(sb),
            None => Err(BackendError::Gone),
        }
    }

    fn fs<R>(&self, path: &[u8], f: impl FnOnce(kernel::SandboxFs<'_>) -> Result<R, Errno>) -> BResult<R> {
        self.with(|sb| f(sb.fs()).map_err(|e| os(e, path)))
    }
}

impl Sandbox for KSandbox {
    fn spawn(&self, req: SpawnRequest) -> BResult<Spawned> {
        let me = self.me.lock().upgrade().ok_or(BackendError::Gone)?;
        self.with(|sb| {
            let spec = SpawnSpec {
                path: req.path.clone(),
                argv: req.argv,
                attrs: ProcAttrs { env: Some(req.env), cwd: Some(req.cwd.clone()), ..ProcAttrs::default() },
            };
            let sp = sb.spawn(spec, kernel::HostStdio::default()).map_err(|e| match e {
                Errno::EAGAIN => BackendError::Limit("limite de processos da sandbox atingido".into()),
                Errno::ENOENT | Errno::ENOTDIR if sb.fs().stat(&req.cwd).is_err() => os(e, &req.cwd),
                other => os(other, &req.path),
            })?;
            Ok(Spawned {
                pid: sp.pid,
                stdin: match sp.stdin {
                    Some(w) => Box::new(KWriter(w)),
                    None => Box::new(Closed),
                },
                stdout: match sp.stdout {
                    Some(r) => Box::new(KReader(r)),
                    None => Box::new(Closed),
                },
                stderr: match sp.stderr {
                    Some(r) => Box::new(KReader(r)),
                    None => Box::new(Closed),
                },
                exit: Box::new(KWaiter { sb: me.clone(), pid: sp.pid }),
            })
        })
    }

    fn kill(&self, target: KillTarget, sig: Signal) -> BResult<()> {
        self.with(|sb| sb.kill(target, sig).map_err(|e| os(e, b"")))
    }

    fn processes(&self) -> Vec<ProcInfo> {
        self.with(|sb| Ok(sb.processes())).unwrap_or_default()
    }

    fn stat(&self, path: &[u8], follow: bool) -> BResult<Stat> {
        self.fs(path, |fs| if follow { fs.stat(path) } else { fs.lstat(path) })
    }

    fn read_dir(&self, path: &[u8]) -> BResult<Vec<DirEntry>> {
        self.fs(path, |fs| {
            Ok(fs.readdir(path)?.into_iter().map(|e| DirEntry { ino: e.ino, kind: e.kind, name: e.name }).collect())
        })
    }

    fn read_file(&self, path: &[u8], offset: u64, max: usize) -> BResult<Vec<u8>> {
        self.fs(path, |fs| fs.read(path, offset, max))
    }

    fn write_file(&self, path: &[u8], data: &[u8], opts: WriteOpts) -> BResult<()> {
        let how = if opts.exclusive {
            kernel::WriteMode::CreateNew
        } else if opts.append {
            kernel::WriteMode::Append
        } else {
            kernel::WriteMode::Truncate
        };
        self.fs(path, |fs| fs.write(path, data, how, opts.mode))
    }

    fn mkdir(&self, path: &[u8], mode: Mode) -> BResult<()> {
        self.fs(path, |fs| fs.mkdir(path, mode))
    }

    fn unlink(&self, path: &[u8]) -> BResult<()> {
        self.fs(path, |fs| fs.unlink(path))
    }

    fn rmdir(&self, path: &[u8]) -> BResult<()> {
        self.fs(path, |fs| fs.rmdir(path))
    }

    fn symlink(&self, target: &[u8], path: &[u8]) -> BResult<()> {
        self.fs(path, |fs| fs.symlink(target, path))
    }

    fn readlink(&self, path: &[u8]) -> BResult<Vec<u8>> {
        self.fs(path, |fs| fs.readlink(path))
    }

    fn link(&self, existing: &[u8], new: &[u8]) -> BResult<()> {
        self.fs(new, |fs| fs.link(existing, new))
    }

    fn mknod(&self, path: &[u8], _mode: Mode, _dev: u64) -> BResult<()> {
        // Sem `mknod` no FS direto do kernel ainda (pedido); quem chama conta como pulado.
        Err(os(Errno::EOPNOTSUPP, path))
    }

    fn chmod(&self, path: &[u8], mode: Mode) -> BResult<()> {
        self.fs(path, |fs| fs.chmod(path, mode))
    }

    fn chown(&self, path: &[u8], uid: u32, gid: u32, follow: bool) -> BResult<()> {
        self.fs(path, |fs| fs.chown(path, Some(uid), Some(gid), !follow))
    }

    fn set_times(&self, path: &[u8], atime: TimeSpec, mtime: TimeSpec, follow: bool) -> BResult<()> {
        self.fs(path, |fs| fs.utimens(path, SetTime::At(atime), SetTime::At(mtime), !follow))
    }

    fn snapshot(&self) -> BResult<SnapshotToken> {
        self.with(|sb| Ok(Arc::new(sb.snapshot()) as SnapshotToken))
    }

    fn restore(&self, snap: &SnapshotToken) -> BResult<()> {
        let s = snap.downcast_ref::<kernel::Snapshot>().ok_or_else(|| BackendError::Internal("snapshot de outro backend".into()))?;
        self.with(|sb| {
            sb.restore(s);
            Ok(())
        })
    }

    fn usage(&self) -> SandboxUsage {
        self.with(|sb| {
            let u = sb.usage();
            Ok(SandboxUsage { procs: u.procs, mem_bytes: 0, fs_bytes: u.fs_bytes, cpu_ns: u.cpu_ns })
        })
        .unwrap_or_default()
    }

    fn destroy(&self) {
        let taken = self.inner.write().take();
        if let Some(sb) = taken {
            drop(sb);
        }
    }
}

/// O kernel de um worker.
pub struct KernelBackend {
    kernel: kernel::Kernel,
    programs: Vec<Program>,
    cpus: usize,
    isolation: Arc<Mutex<Option<IsolationReport>>>,
    users: Mutex<HashMap<String, UserSched>>,
}

impl KernelBackend {
    pub fn new(cpus: usize, profile: IsolationProfile, programs: Vec<Program>) -> KernelBackend {
        let report: Arc<Mutex<Option<IsolationReport>>> = Arc::new(Mutex::new(None));
        let r2 = report.clone();
        let hook: kernel::SpawnerHook = Arc::new(move |info: &kernel::SpawnerInfo| {
            let rep = profile.apply_current_thread(&info.host_mounts)?;
            let mut slot = r2.lock();
            if slot.is_none() {
                tracing::info!(sandbox = info.sandbox_id, isolation = %rep.summary(), "isolamento aplicado na spawner thread");
            }
            *slot = Some(rep);
            Ok(())
        });
        let kernel = kernel::Kernel::new(kernel::KernelConfig {
            cpus,
            spawner_hook: Some(hook),
            ..kernel::KernelConfig::default()
        });
        KernelBackend { kernel, programs, cpus, isolation: report, users: Mutex::new(HashMap::new()) }
    }
}

impl Backend for KernelBackend {
    fn info(&self) -> BackendInfo {
        BackendInfo {
            name: "kernel".into(),
            cpus: self.cpus,
            programs: self.programs.len(),
            isolation: self
                .isolation
                .lock()
                .as_ref()
                .map(IsolationReport::summary)
                .unwrap_or_else(|| "aplicado na primeira sandbox".into()),
        }
    }

    fn ensure_user(&self, user: &UserSched) -> BResult<()> {
        // Os grupos de CPU por usuário são o marco 2 do kernel (`create_user_group`); até lá o peso e o
        // teto ficam registrados aqui e a divisão de CPU é por processo.
        self.users.lock().insert(user.user.clone(), user.clone());
        Ok(())
    }

    fn create_sandbox(&self, sandbox_id: &str, _user: &str, spec: &SandboxSpec) -> BResult<Arc<dyn Sandbox>> {
        if spec.image != "default" {
            return Err(BackendError::Internal(format!("imagem desconhecida: {}", spec.image)));
        }
        let cfg = kernel::SandboxConfig {
            programs: self.programs.clone(),
            hostname: spec.hostname.clone(),
            limits: kernel::SandboxLimits {
                max_procs: Some(spec.limits.max_procs),
                mem_bytes: Some(spec.limits.mem_bytes),
                fs_bytes: Some(spec.limits.fs_bytes),
                fs_inodes: None,
                nofile: spec.limits.nofile,
                fsize: sysabi::RLIM_INFINITY,
            },
            ..kernel::SandboxConfig::default()
        };
        let sb = self.kernel.create_sandbox(cfg).map_err(|e| match e {
            kernel::CreateError::Spawner(m) => BackendError::Isolation(format!("isolamento da sandbox {sandbox_id}: {m}")),
            kernel::CreateError::Errno(e) => BackendError::os(e, format!("criando a sandbox {sandbox_id}")),
        })?;
        let k = Arc::new(KSandbox { inner: RwLock::new(Some(sb)), me: Mutex::new(std::sync::Weak::new()) });
        *k.me.lock() = Arc::downgrade(&k);
        Ok(k)
    }
}
