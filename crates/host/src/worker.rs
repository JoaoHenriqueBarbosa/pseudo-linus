//! O worker: um processo do host com um kernel (backend) e um conjunto de sandboxes.
//!
//! Lê chamadas do supervisor (stdin), atende cada uma numa thread própria do host (nenhuma é de pool:
//! trabalho de pseudo-processo nunca passa por thread não restrita) e responde no stdout de protocolo.
//! Se o supervisor some (stdin fecha), destrói tudo e sai; o `PR_SET_PDEATHSIG` cobre a morte abrupta.

use std::collections::{BTreeSet, HashMap};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use parking_lot::{Mutex, RwLock};
use sysabi::{KillTarget, Signal};

use crate::api::ProcessInfo;
use crate::backend::{Backend, Sandbox, SnapshotToken, SpawnRequest, WriteOpts, resolve_program};
use crate::exec::{self, Cancel, OutputSink, Stream};
use crate::ipc::{Call, ExecCall, FromWorker, Reply, ToWorker, WorkerStatus, read_frame, write_frame};
use crate::rpc::{RpcError, codes};
use crate::session::{Session, SessionError, ShellState};
use crate::{fsops, tarball};

struct SandboxRt {
    sb: Arc<dyn Sandbox>,
    snapshots: Mutex<HashMap<String, SnapshotToken>>,
    sessions: Mutex<BTreeSet<String>>,
}

struct SessionRt {
    sandbox_id: String,
    session: Session,
}

/// Estado do worker.
pub struct Worker {
    backend: Arc<dyn Backend>,
    sandboxes: RwLock<HashMap<String, Arc<SandboxRt>>>,
    sessions: RwLock<HashMap<String, Arc<SessionRt>>>,
    cancels: Mutex<HashMap<u64, Cancel>>,
    out: Mutex<BufWriter<Box<dyn Write + Send>>>,
    in_flight: AtomicU32,
}

struct EventSink<'a> {
    worker: &'a Worker,
    id: u64,
}

impl OutputSink for EventSink<'_> {
    fn output(&self, stream: Stream, data: &[u8]) {
        self.worker.send(&FromWorker::Event { id: self.id, stream, data: data.to_vec() });
    }
}

fn not_found_sandbox(id: &str) -> RpcError {
    RpcError::not_found("a sandbox", id)
}

fn session_err(e: SessionError) -> RpcError {
    match e {
        SessionError::Busy => RpcError::busy("a sessão já está executando um comando"),
        SessionError::Closed(r) => RpcError::new(codes::SESSION_LOST, "session_closed", r),
        SessionError::Backend(b) => b.into(),
    }
}

impl Worker {
    pub fn new(backend: Arc<dyn Backend>, out: Box<dyn Write + Send>) -> Arc<Worker> {
        Arc::new(Worker {
            backend,
            sandboxes: RwLock::new(HashMap::new()),
            sessions: RwLock::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            out: Mutex::new(BufWriter::new(out)),
            in_flight: AtomicU32::new(0),
        })
    }

    fn send(&self, msg: &FromWorker) {
        let mut out = self.out.lock();
        if let Err(e) = write_frame(&mut *out, msg) {
            // O supervisor sumiu: não há a quem responder. Sai sem deixar processo órfão.
            tracing::error!("worker: falha ao escrever pro supervisor: {e}; saindo");
            std::process::exit(70);
        }
    }

    /// Laço principal: só volta no fim do stdin ou num `Shutdown`.
    pub fn serve(self: &Arc<Worker>, input: impl Read) -> io::Result<()> {
        self.send(&FromWorker::Ready { pid: std::process::id(), info: self.backend.info() });
        let mut input = BufReader::new(input);
        loop {
            let msg = match read_frame::<ToWorker>(&mut input) {
                Ok(Some(m)) => m,
                Ok(None) => break,
                Err(e) => {
                    tracing::error!("worker: quadro inválido do supervisor: {e}");
                    break;
                }
            };
            match msg {
                ToWorker::Call { id, call } => self.dispatch(id, call),
                ToWorker::Cancel { id } => {
                    if let Some(c) = self.cancels.lock().get(&id) {
                        c.cancel();
                    }
                }
                ToWorker::Shutdown => break,
            }
        }
        self.destroy_all();
        Ok(())
    }

    fn dispatch(self: &Arc<Worker>, id: u64, call: Call) {
        let cancel = Cancel::new();
        self.cancels.lock().insert(id, cancel.clone());
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        let me = self.clone();
        let name = call.name();
        let spawned = std::thread::Builder::new().name(format!("call-{name}-{id}")).spawn(move || {
            let result = me.handle(id, call, &cancel);
            me.cancels.lock().remove(&id);
            me.in_flight.fetch_sub(1, Ordering::AcqRel);
            me.send(&FromWorker::Reply { id, result });
        });
        if let Err(e) = spawned {
            self.cancels.lock().remove(&id);
            self.in_flight.fetch_sub(1, Ordering::AcqRel);
            self.send(&FromWorker::Reply { id, result: Err(RpcError::internal(format!("worker sem thread pra atender: {e}"))) });
        }
    }

    fn sandbox(&self, id: &str) -> Result<Arc<SandboxRt>, RpcError> {
        self.sandboxes.read().get(id).cloned().ok_or_else(|| not_found_sandbox(id))
    }

    fn destroy_all(&self) {
        let sessions: Vec<_> = self.sessions.write().drain().map(|(_, s)| s).collect();
        for s in sessions {
            s.session.close();
        }
        let sbs: Vec<_> = self.sandboxes.write().drain().map(|(_, s)| s).collect();
        for s in sbs {
            s.sb.destroy();
        }
    }

    fn spawn_request(sb: &dyn Sandbox, e: &ExecCall) -> Result<SpawnRequest, RpcError> {
        spawn_request(sb, e.command.as_deref(), e.argv.as_deref(), &e.cwd, &e.env)
    }

    fn handle(&self, id: u64, call: Call, cancel: &Cancel) -> Result<Reply, RpcError> {
        match call {
            Call::Ping => Ok(Reply::Pong {
                status: WorkerStatus {
                    sandboxes: self.sandboxes.read().len() as u32,
                    sessions: self.sessions.read().len() as u32,
                    calls_in_flight: self.in_flight.load(Ordering::Acquire).saturating_sub(1),
                },
            }),
            Call::CreateSandbox { sandbox_id, user, spec } => {
                if self.sandboxes.read().contains_key(&sandbox_id) {
                    return Err(RpcError::internal(format!("a sandbox {sandbox_id} já existe neste worker")));
                }
                self.backend.ensure_user(&user)?;
                let sb = self.backend.create_sandbox(&sandbox_id, &user.user, &spec)?;
                self.sandboxes.write().insert(
                    sandbox_id,
                    Arc::new(SandboxRt { sb, snapshots: Mutex::new(HashMap::new()), sessions: Mutex::new(BTreeSet::new()) }),
                );
                Ok(Reply::Ok)
            }
            Call::RecoverSandbox { sandbox_id, user, spec, tar_path, max_bytes } => {
                let data = std::fs::read(&tar_path)
                    .map_err(|e| RpcError::internal(format!("{tar_path}: {}", crate::config::io_msg(&e))))?;
                self.backend.ensure_user(&user)?;
                let sb = self.backend.create_sandbox(&sandbox_id, &user.user, &spec)?;
                let restored = tarball::wipe_root(&*sb).and_then(|()| tarball::import(&*sb, b"/", &data, max_bytes));
                if let Err(e) = restored {
                    sb.destroy();
                    return Err(e.into());
                }
                self.sandboxes.write().insert(
                    sandbox_id,
                    Arc::new(SandboxRt { sb, snapshots: Mutex::new(HashMap::new()), sessions: Mutex::new(BTreeSet::new()) }),
                );
                Ok(Reply::Ok)
            }
            Call::DestroySandbox { sandbox_id } => {
                let rt = self.sandboxes.write().remove(&sandbox_id).ok_or_else(|| not_found_sandbox(&sandbox_id))?;
                let ids: Vec<String> = rt.sessions.lock().iter().cloned().collect();
                for sid in ids {
                    if let Some(s) = self.sessions.write().remove(&sid) {
                        s.session.close();
                    }
                }
                rt.sb.destroy();
                Ok(Reply::Ok)
            }
            Call::UpdateUser { user } => {
                self.backend.ensure_user(&user)?;
                Ok(Reply::Ok)
            }
            Call::Exec { sandbox_id, exec: e, stream } => {
                let rt = self.sandbox(&sandbox_id)?;
                let req = Self::spawn_request(&*rt.sb, &e)?;
                let sink = EventSink { worker: self, id };
                let sink_ref: Option<&dyn OutputSink> = if stream { Some(&sink) } else { None };
                let outcome = exec::run(&*rt.sb, req, e.stdin, e.limits.to_exec(), sink_ref, cancel)?;
                Ok(Reply::Exec { outcome })
            }
            Call::SessionOpen { sandbox_id, session_id, cwd, env, workdir } => {
                let rt = self.sandbox(&sandbox_id)?;
                let state = ShellState { cwd: cwd.into_bytes(), env: env.into_iter().map(String::into_bytes).collect() };
                let session = Session::open(rt.sb.clone(), &session_id, state, workdir.as_bytes())?;
                rt.sessions.lock().insert(session_id.clone());
                self.sessions.write().insert(session_id, Arc::new(SessionRt { sandbox_id, session }));
                Ok(Reply::Ok)
            }
            Call::SessionExec { session_id, command, stdin, limits, stream } => {
                let s = self
                    .sessions
                    .read()
                    .get(&session_id)
                    .cloned()
                    .ok_or_else(|| RpcError::not_found("a sessão", &session_id))?;
                let sink = EventSink { worker: self, id };
                let sink_ref: Option<&dyn OutputSink> = if stream { Some(&sink) } else { None };
                let outcome = s.session.exec(&command, &stdin, limits.to_exec(), sink_ref, cancel).map_err(session_err)?;
                Ok(Reply::Session { outcome })
            }
            Call::SessionClose { session_id } => {
                let s = self
                    .sessions
                    .write()
                    .remove(&session_id)
                    .ok_or_else(|| RpcError::not_found("a sessão", &session_id))?;
                if let Some(rt) = self.sandboxes.read().get(&s.sandbox_id) {
                    rt.sessions.lock().remove(&session_id);
                }
                s.session.close();
                Ok(Reply::Ok)
            }
            Call::FsRead { sandbox_id, path, offset, max } => {
                let rt = self.sandbox(&sandbox_id)?;
                let (data, size, eof) = fsops::read(&*rt.sb, path.as_bytes(), offset, max as usize)?;
                Ok(Reply::FsRead { data, size, eof })
            }
            Call::FsWrite { sandbox_id, path, data, mode, append, exclusive, create_parents } => {
                let rt = self.sandbox(&sandbox_id)?;
                fsops::write(&*rt.sb, path.as_bytes(), &data, WriteOpts { append, exclusive, mode }, create_parents)?;
                Ok(Reply::Ok)
            }
            Call::FsList { sandbox_id, path } => {
                let rt = self.sandbox(&sandbox_id)?;
                Ok(Reply::FsList { entries: fsops::list(&*rt.sb, path.as_bytes())? })
            }
            Call::FsStat { sandbox_id, path, follow } => {
                let rt = self.sandbox(&sandbox_id)?;
                Ok(Reply::FsStat { info: fsops::stat(&*rt.sb, path.as_bytes(), follow)? })
            }
            Call::FsMkdir { sandbox_id, path, parents, mode } => {
                let rt = self.sandbox(&sandbox_id)?;
                if parents {
                    fsops::mkdir_p(&*rt.sb, path.as_bytes(), mode)?;
                } else {
                    rt.sb.mkdir(path.as_bytes(), mode).map_err(|e| e.context(&path))?;
                }
                Ok(Reply::Ok)
            }
            Call::FsRemove { sandbox_id, path, recursive, force } => {
                let rt = self.sandbox(&sandbox_id)?;
                fsops::remove(&*rt.sb, path.as_bytes(), recursive, force)?;
                Ok(Reply::Ok)
            }
            Call::Snapshot { sandbox_id, snapshot_id, persist_to, max_bytes } => {
                let rt = self.sandbox(&sandbox_id)?;
                let token = rt.sb.snapshot()?;
                let persisted_bytes = match persist_to {
                    Some(p) => Some(persist(&*rt.sb, Path::new(&p), max_bytes)?),
                    None => None,
                };
                rt.snapshots.lock().insert(snapshot_id, token);
                Ok(Reply::Snapshot { persisted_bytes })
            }
            Call::Restore { sandbox_id, snapshot_id } => {
                let rt = self.sandbox(&sandbox_id)?;
                let token = rt
                    .snapshots
                    .lock()
                    .get(&snapshot_id)
                    .cloned()
                    .ok_or_else(|| RpcError::not_found("o snapshot", &snapshot_id))?;
                rt.sb.restore(&token)?;
                Ok(Reply::Ok)
            }
            Call::DropSnapshot { sandbox_id, snapshot_id } => {
                let rt = self.sandbox(&sandbox_id)?;
                rt.snapshots.lock().remove(&snapshot_id);
                Ok(Reply::Ok)
            }
            Call::Ps { sandbox_id } => {
                let rt = self.sandbox(&sandbox_id)?;
                let processes = rt
                    .sb
                    .processes()
                    .into_iter()
                    .map(|p| ProcessInfo {
                        pid: p.pid,
                        ppid: p.ppid,
                        pgid: p.pgid,
                        sid: p.sid,
                        state: p.state.to_string(),
                        comm: String::from_utf8_lossy(&p.comm).into_owned(),
                    })
                    .collect();
                Ok(Reply::Ps { processes })
            }
            Call::Kill { sandbox_id, pid, signal, group } => {
                let rt = self.sandbox(&sandbox_id)?;
                let target = if group { KillTarget::Group(pid) } else { KillTarget::Pid(pid) };
                rt.sb.kill(target, Signal(signal)).map_err(|e| e.context(&format!("kill {pid}")))?;
                Ok(Reply::Ok)
            }
            Call::Export { sandbox_id, path, max_bytes } => {
                let rt = self.sandbox(&sandbox_id)?;
                let (data, report) = tarball::export(&*rt.sb, path.as_bytes(), max_bytes)?;
                Ok(Reply::Export { data, report })
            }
            Call::Import { sandbox_id, path, data, max_bytes } => {
                let rt = self.sandbox(&sandbox_id)?;
                Ok(Reply::Import { report: tarball::import(&*rt.sb, path.as_bytes(), &data, max_bytes)? })
            }
            Call::Usage { sandbox_id } => {
                let rt = self.sandbox(&sandbox_id)?;
                Ok(Reply::Usage { usage: rt.sb.usage() })
            }
        }
    }
}

/// Pedido de processo de um `exec`: `command` vai pro `bash -c`; `argv` é buscado no PATH do ambiente
/// (como o `execvp`).
pub fn spawn_request(
    sb: &dyn Sandbox,
    command: Option<&str>,
    argv: Option<&[String]>,
    cwd: &str,
    env: &[String],
) -> Result<SpawnRequest, RpcError> {
    let env: Vec<Vec<u8>> = env.iter().map(|s| s.as_bytes().to_vec()).collect();
    let path_var = env.iter().find_map(|v| v.strip_prefix(b"PATH=")).map(<[u8]>::to_vec);
    let (path, argv) = match (command, argv) {
        (Some(cmd), None) => {
            let bash = resolve_program(sb, b"bash", path_var.as_deref(), cwd.as_bytes())?;
            (bash, vec![b"bash".to_vec(), b"-c".to_vec(), cmd.as_bytes().to_vec()])
        }
        (None, Some(argv)) if !argv.is_empty() => {
            let p = resolve_program(sb, argv[0].as_bytes(), path_var.as_deref(), cwd.as_bytes())?;
            (p, argv.iter().map(|a| a.as_bytes().to_vec()).collect())
        }
        _ => return Err(RpcError::invalid_params("exec precisa de exatamente um de command e argv (não vazio)")),
    };
    Ok(SpawnRequest { path, argv, env, cwd: cwd.as_bytes().to_vec() })
}

/// Monta o backend escolhido na configuração.
pub fn backend_from_config(cfg: &crate::config::Config, index: usize) -> Result<Arc<dyn Backend>, String> {
    let _ = index;
    make_backend(&cfg.backend, &cfg.isolation, cfg.cpus_per_worker)
}

/// Monta um backend pelo nome (`kernel` ou `fake`).
pub fn make_backend(
    kind: &str,
    isolation: &crate::config::IsolationConfig,
    cpus: usize,
) -> Result<Arc<dyn Backend>, String> {
    match kind {
        "fake" => fake_backend(),
        "kernel" => kernel_backend(isolation, cpus),
        other => Err(format!("backend desconhecido: {other}")),
    }
}

/// A tabela de programas da imagem: o userland inteiro (e os programas de teste, na feature).
#[cfg(feature = "kernel")]
pub fn programs() -> Vec<sysabi::Program> {
    #[allow(unused_mut)]
    let mut p = userland::all_programs();
    #[cfg(feature = "test-programs")]
    p.extend(crate::testprogs::programs());
    p
}

#[cfg(feature = "kernel")]
fn kernel_backend(isolation: &crate::config::IsolationConfig, cpus: usize) -> Result<Arc<dyn Backend>, String> {
    let profile = crate::isolation::IsolationProfile::new(isolation)?;
    Ok(Arc::new(crate::kernel_backend::KernelBackend::new(cpus, profile, programs())))
}

#[cfg(not(feature = "kernel"))]
fn kernel_backend(_isolation: &crate::config::IsolationConfig, _cpus: usize) -> Result<Arc<dyn Backend>, String> {
    Err("este build não tem o backend do kernel (feature kernel)".into())
}

#[cfg(feature = "fake-backend")]
fn fake_backend() -> Result<Arc<dyn Backend>, String> {
    if std::env::var("PL_ALLOW_FAKE_BACKEND").as_deref() != Ok("1") {
        return Err("o backend falso é só pra teste; ligue PL_ALLOW_FAKE_BACKEND=1 se é isso mesmo".into());
    }
    Ok(Arc::new(crate::fake::FakeBackend::new()))
}

#[cfg(not(feature = "fake-backend"))]
fn fake_backend() -> Result<Arc<dyn Backend>, String> {
    Err("este build não tem o backend falso (feature fake-backend)".into())
}

/// Grava o tar da sandbox inteira no host, de forma atômica (arquivo temporário e `rename`).
fn persist(sb: &dyn Sandbox, path: &Path, max_bytes: u64) -> Result<u64, RpcError> {
    let (data, _) = tarball::export(sb, b"/", max_bytes)?;
    let io = |e: io::Error| RpcError::internal(format!("{}: {}", path.display(), crate::config::io_msg(&e)));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let tmp = path.with_extension("tar.tmp");
    let mut f = std::fs::File::create(&tmp).map_err(io)?;
    f.write_all(&data).map_err(io)?;
    f.sync_all().map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)?;
    Ok(data.len() as u64)
}
