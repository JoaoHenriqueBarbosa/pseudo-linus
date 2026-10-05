//! O supervisor: mantém os workers de pé, roteia cada sandbox pro worker do dono e faz a contabilidade
//! de quotas e o controle de admissão.
//!
//! - Cada worker é um processo filho (`pseudo-linusd worker`). Uma tarefa por worker sobe o processo,
//!   espera o `Ready`, encaminha chamadas e respostas, faz ping de saúde, e quando o worker cai (ou trava
//!   e para de responder ao ping) responde erro claro pra toda chamada em andamento, marca as sandboxes
//!   dele como perdidas (ou em recuperação, se tiverem snapshot persistido), e sobe outro com backoff
//!   exponencial.
//! - Todas as sandboxes de um usuário ficam no mesmo worker (o grupo de escalonamento do usuário vive no
//!   kernel daquele worker). O usuário vai pro worker com menos usuários quando cria a primeira sandbox e
//!   fica nele enquanto tiver alguma.
//! - Reservas: criar sandbox reserva memória, processos e uma vaga na quota do usuário e no orçamento do
//!   serviço; destruir (ou perder) devolve. `exec` e sessões contam em andamento.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use tokio::process::Command;
use tokio::sync::{Notify, mpsc, oneshot};

use crate::api::{SandboxInfo, SandboxLimits, SnapshotInfo};
use crate::auth::AuthStore;
use crate::backend::{BackendInfo, SandboxSpec, UserSched};
use crate::config::{Config, Quota};
use crate::exec::Stream;
use crate::ipc::{Call, FromWorker, Reply, ToWorker, tokio_io};
use crate::rpc::{RpcError, codes};
use crate::timeutil::now_unix;

/// Como subir um worker.
#[derive(Clone, Debug)]
pub struct WorkerCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SlotState {
    Starting,
    Ready,
    Down,
    Stopped,
}

struct Pending {
    reply: oneshot::Sender<Result<Reply, RpcError>>,
    events: Option<mpsc::UnboundedSender<(Stream, Vec<u8>)>>,
}

struct SlotInner {
    state: SlotState,
    tx: Option<mpsc::Sender<ToWorker>>,
    generation: u64,
    pid: Option<u32>,
    restarts: u64,
    info: Option<BackendInfo>,
    last_exit: Option<String>,
    last_pong: Instant,
}

struct Slot {
    index: usize,
    inner: Mutex<SlotInner>,
    pending: Mutex<HashMap<u64, Pending>>,
    next_id: AtomicU64,
    ready: Notify,
}

/// Estado de um worker pra `/healthz` e `admin.workers`.
#[derive(Clone, Debug, Serialize)]
pub struct WorkerView {
    pub index: usize,
    pub state: SlotState,
    pub pid: Option<u32>,
    pub generation: u64,
    pub restarts: u64,
    pub last_exit: Option<String>,
    pub backend: Option<BackendInfo>,
    pub users: usize,
    pub sandboxes: usize,
    pub calls_in_flight: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SbStatus {
    /// Reservada, esperando o worker criar.
    Creating,
    Active,
    /// O worker caiu; volta do snapshot persistido quando ele subir de novo.
    Recovering { from: String },
    Lost { reason: String, at: u64 },
}

#[derive(Clone, Debug)]
pub struct SnapEntry {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    /// Geração do worker em que o snapshot em memória existe.
    pub generation: u64,
    pub persisted: Option<(PathBuf, u64)>,
}

/// Nome do snapshot que o host persiste sozinho (um por sandbox; não conta na cota do usuário).
pub const AUTOSAVE_NAME: &str = "autosave";

#[derive(Clone, Debug)]
pub struct SandboxEntry {
    pub id: String,
    pub owner: String,
    pub worker: usize,
    pub image: String,
    pub hostname: String,
    pub workdir: String,
    pub env: BTreeMap<String, String>,
    pub labels: BTreeMap<String, String>,
    pub limits: SandboxLimits,
    pub created_at: u64,
    pub last_used_at: u64,
    pub status: SbStatus,
    pub recovered_from: Option<String>,
    pub snapshots: Vec<SnapEntry>,
    pub sessions: BTreeSet<String>,
    /// Se as reservas desta sandbox ainda contam (deixam de contar quando ela se perde).
    pub reserved: bool,
    /// Houve escrita (exec, sessão, fs) depois do último autosave.
    pub dirty: bool,
    /// Quando o último autosave terminou (segundos Unix; 0 = nunca).
    pub last_autosave: u64,
}

impl SandboxEntry {
    pub fn info(&self) -> SandboxInfo {
        let (state, lost_reason) = match &self.status {
            SbStatus::Creating => ("creating", None),
            SbStatus::Active => ("active", None),
            SbStatus::Recovering { .. } => ("recovering", None),
            SbStatus::Lost { reason, .. } => ("lost", Some(reason.clone())),
        };
        SandboxInfo {
            sandbox_id: self.id.clone(),
            owner: self.owner.clone(),
            image: self.image.clone(),
            hostname: self.hostname.clone(),
            workdir: self.workdir.clone(),
            limits: self.limits.clone(),
            labels: self.labels.clone(),
            created_at: self.created_at,
            last_used_at: self.last_used_at,
            state: state.to_string(),
            lost_reason,
            recovered_from: self.recovered_from.clone(),
            worker: self.worker,
            sessions: self.sessions.iter().cloned().collect(),
            snapshots: self
                .snapshots
                .iter()
                .map(|s| SnapshotInfo {
                    snapshot_id: s.id.clone(),
                    name: s.name.clone(),
                    created_at: s.created_at,
                    persisted: s.persisted.is_some(),
                    persisted_bytes: s.persisted.as_ref().map(|p| p.1),
                })
                .collect(),
        }
    }

    pub fn spec(&self) -> SandboxSpec {
        SandboxSpec { image: self.image.clone(), hostname: self.hostname.clone(), limits: self.limits.clone() }
    }
}

#[derive(Clone, Debug)]
pub struct SessionEntry {
    pub id: String,
    pub owner: String,
    pub sandbox_id: String,
    pub created_at: u64,
    /// Estado do shell depois do último comando; com ele a sessão é recriada se o worker cair.
    pub snapshot: Option<crate::session::SessionSnapshot>,
}

/// O que um usuário está usando agora.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct UserUsage {
    pub sandboxes: u32,
    pub mem_bytes: u64,
    pub procs: u64,
    pub execs: u32,
    pub sessions: u32,
    pub persisted_snapshots: u32,
}

#[derive(Default)]
pub struct State {
    pub sandboxes: HashMap<String, SandboxEntry>,
    pub sessions: HashMap<String, SessionEntry>,
    /// Sessões que acabaram (o shell saiu) ou se perderam, com o motivo, pra responder com clareza.
    pub dead_sessions: HashMap<String, (String, String, u64)>,
    /// Sessões cujo worker caiu mas cuja sandbox tem snapshot persistido: voltam no próximo comando,
    /// com o estado do shell, quando a sandbox terminar de ser recuperada.
    pub orphan_sessions: HashMap<String, SessionEntry>,
    pub user_worker: HashMap<String, usize>,
    pub users: HashMap<String, UserUsage>,
    pub total_mem: u64,
    pub total_sandboxes: u32,
    pub total_execs: u32,
}

impl State {
    pub fn usage(&self, user: &str) -> UserUsage {
        self.users.get(user).cloned().unwrap_or_default()
    }

    /// Enterra as sessões órfãs de uma sandbox que não voltou (a recuperação falhou ou ela foi apagada).
    pub fn bury_orphans(&mut self, sb_id: &str, reason: &str) {
        let ids: Vec<String> = self.orphan_sessions.values().filter(|s| s.sandbox_id == sb_id).map(|s| s.id.clone()).collect();
        let now = now_unix();
        for id in ids {
            if let Some(s) = self.orphan_sessions.remove(&id) {
                if let Some(u) = self.users.get_mut(&s.owner) {
                    u.sessions = u.sessions.saturating_sub(1);
                }
                self.dead_sessions.insert(id, (s.owner, format!("a sessão se perdeu: {reason}"), now));
            }
        }
    }

    /// Devolve as reservas de uma sandbox (uma vez só).
    pub fn release(&mut self, sb_id: &str) {
        let Some(sb) = self.sandboxes.get_mut(sb_id) else { return };
        if !sb.reserved {
            return;
        }
        sb.reserved = false;
        let (owner, mem, procs, persisted) = (
            sb.owner.clone(),
            sb.limits.mem_bytes,
            u64::from(sb.limits.max_procs),
            sb.snapshots.iter().filter(|s| s.persisted.is_some() && s.name != AUTOSAVE_NAME).count() as u32,
        );
        self.total_mem = self.total_mem.saturating_sub(mem);
        self.total_sandboxes = self.total_sandboxes.saturating_sub(1);
        let u = self.users.entry(owner.clone()).or_default();
        u.sandboxes = u.sandboxes.saturating_sub(1);
        u.mem_bytes = u.mem_bytes.saturating_sub(mem);
        u.procs = u.procs.saturating_sub(procs);
        u.persisted_snapshots = u.persisted_snapshots.saturating_sub(persisted);
        self.forget_worker_if_idle(&owner);
    }

    /// Solta o vínculo usuário-worker quando o usuário não tem mais sandbox viva.
    pub fn forget_worker_if_idle(&mut self, user: &str) {
        let has = self.sandboxes.values().any(|s| s.owner == user && s.reserved);
        if !has {
            self.user_worker.remove(user);
        }
    }
}

/// Permissão de um `exec` em andamento; devolve a vaga ao sair de escopo (inclusive se o cliente
/// desconectar e o futuro for descartado).
pub struct ExecPermit {
    sup: Arc<Supervisor>,
    user: String,
}

impl Drop for ExecPermit {
    fn drop(&mut self) {
        let mut st = self.sup.state.lock();
        st.total_execs = st.total_execs.saturating_sub(1);
        if let Some(u) = st.users.get_mut(&self.user) {
            u.execs = u.execs.saturating_sub(1);
        }
    }
}

/// Garante o `Cancel` pro worker se a chamada for abandonada no meio.
struct CallGuard<'a> {
    slot: &'a Slot,
    id: u64,
    tx: mpsc::Sender<ToWorker>,
    done: bool,
}

impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.slot.pending.lock().remove(&self.id);
            let _ = self.tx.try_send(ToWorker::Cancel { id: self.id });
        }
    }
}

pub struct Supervisor {
    pub cfg: Arc<Config>,
    pub auth: Arc<AuthStore>,
    slots: Vec<Arc<Slot>>,
    pub state: Mutex<State>,
    worker_cmd: WorkerCommand,
    shutting_down: AtomicBool,
    shutdown: Notify,
    started: Instant,
}

impl std::fmt::Debug for Supervisor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Supervisor").field("workers", &self.slots.len()).finish_non_exhaustive()
    }
}

fn describe_exit(status: &std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    if let Some(code) = status.code() {
        format!("saiu com {code}")
    } else if let Some(sig) = status.signal() {
        let name = sysabi::Signal(sig).name().map(|n| format!("SIG{n}")).unwrap_or_else(|| sig.to_string());
        format!("morto pelo sinal {sig} ({name})")
    } else {
        format!("{status}")
    }
}

impl Supervisor {
    /// Cria o supervisor e sobe os workers (não espera ficarem prontos; veja [`Supervisor::wait_ready`]).
    pub fn start(cfg: Config, auth: Arc<AuthStore>, worker_cmd: WorkerCommand) -> Arc<Supervisor> {
        let slots = (0..cfg.workers)
            .map(|index| {
                Arc::new(Slot {
                    index,
                    inner: Mutex::new(SlotInner {
                        state: SlotState::Starting,
                        tx: None,
                        generation: 0,
                        pid: None,
                        restarts: 0,
                        info: None,
                        last_exit: None,
                        last_pong: Instant::now(),
                    }),
                    pending: Mutex::new(HashMap::new()),
                    next_id: AtomicU64::new(1),
                    ready: Notify::new(),
                })
            })
            .collect();
        let sup = Arc::new(Supervisor {
            cfg: Arc::new(cfg),
            auth,
            slots,
            state: Mutex::new(State::default()),
            worker_cmd,
            shutting_down: AtomicBool::new(false),
            shutdown: Notify::new(),
            started: Instant::now(),
        });
        for i in 0..sup.slots.len() {
            tokio::spawn(sup.clone().run_slot(i));
        }
        tokio::spawn(sup.clone().housekeeping());
        sup
    }

    pub fn uptime(&self) -> Duration {
        self.started.elapsed()
    }

    /// Espera todos os workers ficarem prontos (ou o prazo acabar). Devolve quantos estão prontos.
    pub async fn wait_ready(&self, timeout: Duration) -> usize {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let ready = self.slots.iter().filter(|s| s.inner.lock().state == SlotState::Ready).count();
            if ready == self.slots.len() || tokio::time::Instant::now() >= deadline {
                return ready;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Acquire)
    }

    /// Pede pros workers saírem (destruindo as sandboxes) e espera até `timeout`.
    pub async fn shutdown(&self, timeout: Duration) {
        self.shutting_down.store(true, Ordering::Release);
        self.shutdown.notify_waiters();
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let alive = self.slots.iter().filter(|s| s.inner.lock().state != SlotState::Stopped).count();
            if alive == 0 || tokio::time::Instant::now() >= deadline {
                break;
            }
            self.shutdown.notify_waiters();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if let Err(e) = self.auth.flush_last_used() {
            tracing::warn!("não deu pra gravar o último uso das chaves: {e}");
        }
    }

    pub fn workers(&self) -> Vec<WorkerView> {
        let st = self.state.lock();
        self.slots
            .iter()
            .map(|s| {
                let i = s.inner.lock();
                WorkerView {
                    index: s.index,
                    state: i.state,
                    pid: i.pid,
                    generation: i.generation,
                    restarts: i.restarts,
                    last_exit: i.last_exit.clone(),
                    backend: i.info.clone(),
                    users: st.user_worker.values().filter(|w| **w == s.index).count(),
                    sandboxes: st.sandboxes.values().filter(|b| b.worker == s.index && b.reserved).count(),
                    calls_in_flight: s.pending.lock().len(),
                }
            })
            .collect()
    }

    pub fn worker_generation(&self, idx: usize) -> u64 {
        self.slots[idx].inner.lock().generation
    }

    pub fn worker_ready(&self, idx: usize) -> bool {
        self.slots[idx].inner.lock().state == SlotState::Ready
    }

    /// Worker pra um usuário: o que ele já usa, ou o pronto com menos usuários.
    pub fn pick_worker(&self, st: &State, user: &str) -> Result<usize, RpcError> {
        if let Some(&w) = st.user_worker.get(user) {
            return Ok(w);
        }
        let mut best: Option<(usize, usize)> = None;
        for s in &self.slots {
            if s.inner.lock().state != SlotState::Ready {
                continue;
            }
            let users = st.user_worker.values().filter(|w| **w == s.index).count();
            if best.is_none_or(|(_, u)| users < u) {
                best = Some((s.index, users));
            }
        }
        best.map(|(i, _)| i).ok_or_else(|| {
            RpcError::new(codes::WORKER_UNAVAILABLE, "worker_unavailable", "nenhum worker pronto; tente de novo em instantes")
        })
    }

    fn unavailable(&self, idx: usize) -> RpcError {
        let i = self.slots[idx].inner.lock();
        let why = i.last_exit.clone().map(|r| format!(" (última queda: {r})")).unwrap_or_default();
        RpcError::new(
            codes::WORKER_UNAVAILABLE,
            "worker_unavailable",
            format!("o worker {idx} está reiniciando{why}; tente de novo em instantes"),
        )
        .with("worker", idx)
    }

    /// Chama um worker e espera a resposta. Com `events`, recebe a saída em streaming.
    pub async fn call(
        &self,
        idx: usize,
        call: Call,
        events: Option<mpsc::UnboundedSender<(Stream, Vec<u8>)>>,
    ) -> Result<Reply, RpcError> {
        self.mark_dirty(&call);
        let slot = &self.slots[idx];
        let tx = {
            let i = slot.inner.lock();
            match (&i.state, &i.tx) {
                (SlotState::Ready, Some(tx)) => tx.clone(),
                _ => {
                    drop(i);
                    return Err(self.unavailable(idx));
                }
            }
        };
        let id = slot.next_id.fetch_add(1, Ordering::Relaxed);
        let (rtx, rrx) = oneshot::channel();
        slot.pending.lock().insert(id, Pending { reply: rtx, events });
        let mut guard = CallGuard { slot, id, tx: tx.clone(), done: false };
        if tx.send(ToWorker::Call { id, call }).await.is_err() {
            guard.done = true;
            slot.pending.lock().remove(&id);
            return Err(self.unavailable(idx));
        }
        let r = rrx.await;
        guard.done = true;
        r.unwrap_or_else(|_| Err(self.unavailable(idx)))
    }

    async fn run_slot(self: Arc<Self>, idx: usize) {
        let tuning = self.cfg.worker.clone();
        let mut backoff = Duration::from_millis(tuning.restart_backoff_min_ms);
        loop {
            if self.is_shutting_down() {
                break;
            }
            let started = Instant::now();
            let reason = self.run_worker_once(idx).await;
            self.on_worker_down(idx, &reason);
            if self.is_shutting_down() {
                break;
            }
            if started.elapsed() >= Duration::from_millis(tuning.stable_after_ms) {
                backoff = Duration::from_millis(tuning.restart_backoff_min_ms);
            }
            tracing::error!(worker = idx, "worker caiu: {reason}; reiniciando em {} ms", backoff.as_millis());
            tokio::select! {
                _ = tokio::time::sleep(backoff) => {}
                _ = self.shutdown.notified() => break,
            }
            backoff = (backoff * 2).min(Duration::from_millis(tuning.restart_backoff_max_ms));
        }
        let mut i = self.slots[idx].inner.lock();
        i.state = SlotState::Stopped;
        i.tx = None;
    }

    /// Sobe um worker e atende até ele cair. Devolve o motivo da queda.
    async fn run_worker_once(self: &Arc<Self>, idx: usize) -> String {
        let slot = self.slots[idx].clone();
        slot.inner.lock().state = SlotState::Starting;
        let mut cmd = Command::new(&self.worker_cmd.program);
        cmd.args(&self.worker_cmd.args)
            .arg("--index")
            .arg(idx.to_string())
            .envs(self.worker_cmd.env.iter().map(|(k, v)| (k, v)))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return format!("não subiu: {}: {}", self.worker_cmd.program.display(), crate::config::io_msg(&e)),
        };
        let mut stdin = child.stdin.take().expect("stdin pipe");
        let mut stdout = child.stdout.take().expect("stdout pipe");

        let startup = Duration::from_millis(self.cfg.worker.startup_timeout_ms);
        let first = tokio::time::timeout(startup, tokio_io::read_frame::<FromWorker, _>(&mut stdout)).await;
        let (pid, info) = match first {
            Ok(Ok(Some(FromWorker::Ready { pid, info }))) => (pid, info),
            Ok(Ok(Some(_))) => return self.reap(child, "mandou outra coisa antes do Ready".into()).await,
            Ok(Ok(None)) => return self.reap(child, "fechou o protocolo antes do Ready".into()).await,
            Ok(Err(e)) => return self.reap(child, format!("protocolo inválido no Ready: {e}")).await,
            Err(_) => return self.reap(child, format!("não ficou pronto em {} ms", startup.as_millis())).await,
        };

        // Leitor dedicado: ler um quadro não é seguro de cancelar no meio, então nunca entra num select.
        let (ftx, mut frx) = mpsc::channel::<std::io::Result<Option<FromWorker>>>(256);
        let reader = tokio::spawn(async move {
            loop {
                let f = tokio_io::read_frame::<FromWorker, _>(&mut stdout).await;
                let end = !matches!(f, Ok(Some(_)));
                if ftx.send(f).await.is_err() || end {
                    break;
                }
            }
        });
        let (tx, mut rx) = mpsc::channel::<ToWorker>(1024);
        let writer = tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                let shutdown = matches!(m, ToWorker::Shutdown);
                if tokio_io::write_frame(&mut stdin, &m).await.is_err() || shutdown {
                    break;
                }
            }
        });
        let generation = {
            let mut i = slot.inner.lock();
            i.state = SlotState::Ready;
            i.tx = Some(tx.clone());
            i.generation += 1;
            i.pid = Some(pid);
            i.info = Some(info);
            i.last_pong = Instant::now();
            i.generation
        };
        slot.ready.notify_waiters();
        tracing::info!(worker = idx, pid, generation, "worker pronto");
        tokio::spawn(self.clone().recover_sandboxes(idx, generation));

        let ping_every = Duration::from_millis(self.cfg.worker.ping_interval_ms);
        let ping_timeout = Duration::from_millis(self.cfg.worker.ping_timeout_ms);
        let mut ticker = tokio::time::interval(ping_every);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let reason = loop {
            tokio::select! {
                f = frx.recv() => match f {
                    Some(Ok(Some(FromWorker::Reply { id, result }))) => {
                        if let Some(p) = slot.pending.lock().remove(&id) {
                            let _ = p.reply.send(result);
                        }
                    }
                    Some(Ok(Some(FromWorker::Event { id, stream, data }))) => {
                        if let Some(Pending { events: Some(ev), .. }) = slot.pending.lock().get(&id) {
                            let _ = ev.send((stream, data));
                        }
                    }
                    Some(Ok(Some(FromWorker::Ready { .. }))) => break "mandou Ready de novo".to_string(),
                    Some(Ok(None)) | None => break "fechou o protocolo".to_string(),
                    Some(Err(e)) => break format!("protocolo inválido: {e}"),
                },
                _ = ticker.tick() => {
                    let last = slot.inner.lock().last_pong;
                    if last.elapsed() > ping_timeout {
                        break format!("travou: sem resposta ao ping há {} ms", last.elapsed().as_millis());
                    }
                    let me = self.clone();
                    tokio::spawn(async move {
                        if let Ok(Reply::Pong { .. }) = me.call(idx, Call::Ping, None).await {
                            let s = &me.slots[idx];
                            let mut i = s.inner.lock();
                            if i.generation == generation {
                                i.last_pong = Instant::now();
                            }
                        }
                    });
                }
                _ = self.shutdown.notified() => {
                    let _ = tx.send(ToWorker::Shutdown).await;
                    match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
                        Ok(_) => break "desligado".to_string(),
                        Err(_) => break "não saiu em 10 s depois do Shutdown".to_string(),
                    }
                }
            }
        };
        {
            let mut i = slot.inner.lock();
            i.state = SlotState::Down;
            i.tx = None;
        }
        writer.abort();
        reader.abort();
        self.reap(child, reason).await
    }

    /// Garante que o processo morreu e descreve como.
    async fn reap(&self, mut child: tokio::process::Child, reason: String) -> String {
        let status = match tokio::time::timeout(Duration::from_millis(500), child.wait()).await {
            Ok(Ok(s)) => Some(s),
            _ => {
                let _ = child.start_kill();
                child.wait().await.ok()
            }
        };
        match status {
            Some(s) => format!("{reason}; {}", describe_exit(&s)),
            None => reason,
        }
    }

    fn on_worker_down(&self, idx: usize, reason: &str) {
        let slot = &self.slots[idx];
        {
            let mut i = slot.inner.lock();
            i.state = if self.is_shutting_down() { SlotState::Stopped } else { SlotState::Down };
            i.tx = None;
            i.pid = None;
            i.restarts += u64::from(!self.is_shutting_down());
            i.last_exit = Some(reason.to_string());
        }
        let crashed = RpcError::new(
            codes::WORKER_CRASHED,
            "worker_crashed",
            format!("o worker {idx} caiu com a requisição em andamento ({reason})"),
        )
        .with("worker", idx);
        let pending: Vec<Pending> = slot.pending.lock().drain().map(|(_, p)| p).collect();
        for p in pending {
            let _ = p.reply.send(Err(crashed.clone()));
        }
        if self.is_shutting_down() {
            return;
        }
        let now = now_unix();
        let mut st = self.state.lock();
        let ids: Vec<String> = st.sandboxes.values().filter(|s| s.worker == idx).map(|s| s.id.clone()).collect();
        let mut lost = 0;
        let mut recovering = 0;
        for id in ids {
            let sessions: Vec<String> = {
                let sb = st.sandboxes.get_mut(&id).expect("listada acima");
                if matches!(sb.status, SbStatus::Lost { .. }) {
                    continue;
                }
                // Snapshots em memória morreram com o worker; os persistidos ficam.
                sb.snapshots.retain(|s| s.persisted.is_some());
                let latest = sb.snapshots.iter().max_by_key(|s| s.created_at).map(|s| s.id.clone());
                sb.status = match latest {
                    Some(from) if sb.status != SbStatus::Creating => {
                        recovering += 1;
                        SbStatus::Recovering { from }
                    }
                    _ => {
                        lost += 1;
                        SbStatus::Lost { reason: format!("o worker {idx} caiu ({reason})"), at: now }
                    }
                };
                std::mem::take(&mut sb.sessions).into_iter().collect()
            };
            for sid in sessions {
                if let Some(s) = st.sessions.remove(&sid) {
                    if matches!(st.sandboxes[&id].status, SbStatus::Recovering { .. }) {
                        // O VFS volta do snapshot; a sessão volta junto, no próximo comando. Continua
                        // contando na cota do usuário.
                        st.orphan_sessions.insert(sid, s);
                        continue;
                    }
                    if let Some(u) = st.users.get_mut(&s.owner) {
                        u.sessions = u.sessions.saturating_sub(1);
                    }
                    st.dead_sessions.insert(sid, (s.owner, format!("a sessão se perdeu: o worker {idx} caiu ({reason})"), now));
                }
            }
            if matches!(st.sandboxes[&id].status, SbStatus::Lost { .. }) {
                st.release(&id);
            }
        }
        tracing::error!(worker = idx, lost, recovering, "worker caiu: {reason}");
    }

    /// Recria, no worker que acabou de subir, as sandboxes que tinham snapshot persistido.
    async fn recover_sandboxes(self: Arc<Self>, idx: usize, generation: u64) {
        let todo: Vec<(String, String, PathBuf, UserSched, SandboxSpec)> = {
            let st = self.state.lock();
            st.sandboxes
                .values()
                .filter(|s| s.worker == idx)
                .filter_map(|s| match &s.status {
                    SbStatus::Recovering { from } => {
                        let snap = s.snapshots.iter().find(|x| &x.id == from)?;
                        let path = snap.persisted.as_ref()?.0.clone();
                        Some((s.id.clone(), from.clone(), path, self.user_sched(&s.owner), s.spec()))
                    }
                    _ => None,
                })
                .collect()
        };
        for (sb_id, from, path, user, spec) in todo {
            let call = Call::RecoverSandbox {
                sandbox_id: sb_id.clone(),
                user,
                spec,
                tar_path: path.display().to_string(),
                max_bytes: self.cfg.service.max_request_bytes.0.max(1 << 30),
            };
            let r = self.call(idx, call, None).await;
            let mut st = self.state.lock();
            let Some(sb) = st.sandboxes.get_mut(&sb_id) else { continue };
            if !matches!(sb.status, SbStatus::Recovering { .. }) || self.worker_generation(idx) != generation {
                continue;
            }
            match r {
                Ok(_) => {
                    sb.status = SbStatus::Active;
                    sb.recovered_from = Some(from.clone());
                    for s in &mut sb.snapshots {
                        s.generation = 0;
                    }
                    tracing::info!(sandbox = %sb_id, snapshot = %from, "sandbox recuperada do snapshot persistido");
                }
                Err(e) => {
                    let reason = format!("o worker {idx} caiu e a recuperação do snapshot {from} falhou: {}", e.message);
                    sb.status = SbStatus::Lost { reason: reason.clone(), at: now_unix() };
                    st.release(&sb_id);
                    st.bury_orphans(&sb_id, &reason);
                    tracing::error!(sandbox = %sb_id, "recuperação falhou: {}", e.message);
                }
            }
        }
    }

    /// Grupo de escalonamento de um usuário, a partir da quota efetiva.
    pub fn user_sched(&self, user: &str) -> UserSched {
        let q = self.user_quota(user);
        UserSched { user: user.to_string(), cpu_weight: q.cpu_weight, cpu_max: q.cpu_max }
    }

    pub fn user_quota(&self, user: &str) -> Quota {
        match self.auth.snapshot() {
            Ok(d) => d.user(user).map(|u| u.quota.apply(&self.cfg.quota)).unwrap_or_else(|| self.cfg.quota.clone()),
            Err(_) => self.cfg.quota.clone(),
        }
    }

    /// Faxina periódica: sandboxes ociosas, donos removidos, lápides velhas e o último uso das chaves.
    /// Marca a sandbox como alterada quando a chamada pode escrever nela (o autosave só persiste as sujas).
    fn mark_dirty(&self, call: &Call) {
        let mut st = self.state.lock();
        let id = match call {
            Call::Exec { sandbox_id, .. }
            | Call::FsWrite { sandbox_id, .. }
            | Call::FsMkdir { sandbox_id, .. }
            | Call::FsRemove { sandbox_id, .. }
            | Call::Import { sandbox_id, .. }
            | Call::Restore { sandbox_id, .. } => sandbox_id.clone(),
            Call::SessionExec { session_id, .. } => match st.sessions.get(session_id) {
                Some(s) => s.sandbox_id.clone(),
                None => return,
            },
            _ => return,
        };
        if let Some(sb) = st.sandboxes.get_mut(&id) {
            sb.dirty = true;
        }
    }

    /// Persiste sozinho um snapshot (`autosave`) de cada sandbox alterada desde o último, no máximo um
    /// por `autosave_secs`, pra que a queda do worker recupere o estado mais novo sem ninguém ter pedido
    /// snapshot. Fica um só `autosave` por sandbox (o novo substitui o antigo) e ele não conta na cota
    /// de snapshots persistidos do usuário.
    async fn autosave(self: &Arc<Self>) {
        let every = self.cfg.sandbox.autosave_secs;
        if every == 0 {
            return;
        }
        let now = now_unix();
        let due: Vec<(String, usize)> = {
            let mut st = self.state.lock();
            st.sandboxes
                .values_mut()
                .filter(|s| s.status == SbStatus::Active && s.dirty && now.saturating_sub(s.last_autosave) >= every)
                .map(|s| {
                    s.dirty = false;
                    (s.id.clone(), s.worker)
                })
                .collect()
        };
        for (id, worker) in due {
            let snap_id = crate::ids::random_id("sn");
            let path = self.cfg.snapshots_dir().join(&id).join(format!("{snap_id}.tar"));
            let call = Call::Snapshot {
                sandbox_id: id.clone(),
                snapshot_id: snap_id.clone(),
                persist_to: Some(path.display().to_string()),
                max_bytes: self.cfg.service.max_request_bytes.0.max(1 << 30),
            };
            let res = self.call(worker, call, None).await;
            let generation = self.worker_generation(worker);
            let old: Vec<SnapEntry> = match res {
                Ok(Reply::Snapshot { persisted_bytes }) => {
                    let mut st = self.state.lock();
                    let Some(entry) = st.sandboxes.get_mut(&id) else { continue };
                    let (gone, kept): (Vec<SnapEntry>, Vec<SnapEntry>) =
                        std::mem::take(&mut entry.snapshots).into_iter().partition(|s| s.name == AUTOSAVE_NAME);
                    entry.snapshots = kept;
                    entry.snapshots.push(SnapEntry {
                        id: snap_id,
                        name: AUTOSAVE_NAME.to_string(),
                        created_at: now_unix(),
                        generation,
                        persisted: Some(path).zip(persisted_bytes),
                    });
                    entry.last_autosave = now_unix();
                    gone
                }
                other => {
                    tracing::warn!(sandbox = %id, "autosave falhou: {}", other.err().map(|e| e.message).unwrap_or_default());
                    if let Some(e) = self.state.lock().sandboxes.get_mut(&id) {
                        e.dirty = true;
                    }
                    continue;
                }
            };
            for o in old {
                if o.generation == generation {
                    let _ = self.call(worker, Call::DropSnapshot { sandbox_id: id.clone(), snapshot_id: o.id.clone() }, None).await;
                }
                if let Some((path, _)) = o.persisted {
                    let _ = tokio::task::spawn_blocking(move || std::fs::remove_file(path)).await;
                }
            }
        }
    }

    async fn housekeeping(self: Arc<Self>) {
        let secs = match self.cfg.sandbox.autosave_secs {
            0 => 30,
            n => n.min(30),
        };
        let mut tick = tokio::time::interval(Duration::from_secs(secs));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = self.shutdown.notified() => return,
            }
            if self.is_shutting_down() {
                return;
            }
            let auth = self.auth.clone();
            let _ = tokio::task::spawn_blocking(move || auth.flush_last_used()).await;
            self.autosave().await;
            let now = now_unix();
            let ttl = self.cfg.sandbox.idle_ttl_secs;
            let users: Option<BTreeSet<String>> =
                self.auth.snapshot().ok().map(|d| d.users.into_iter().map(|u| u.name).collect());
            let doomed: Vec<(String, &'static str)> = {
                let mut st = self.state.lock();
                st.dead_sessions.retain(|_, (_, _, at)| now.saturating_sub(*at) < 24 * 3600);
                let lost_old: Vec<String> = st
                    .sandboxes
                    .values()
                    .filter(|s| matches!(s.status, SbStatus::Lost { at, .. } if now.saturating_sub(at) > 24 * 3600))
                    .map(|s| s.id.clone())
                    .collect();
                for id in lost_old {
                    st.sandboxes.remove(&id);
                }
                st.sandboxes
                    .values()
                    .filter(|s| s.status == SbStatus::Active)
                    .filter_map(|s| {
                        if users.as_ref().is_some_and(|u| !u.contains(&s.owner)) {
                            Some((s.id.clone(), "o dono foi removido"))
                        } else if ttl > 0 && now.saturating_sub(s.last_used_at) > ttl && s.sessions.is_empty() {
                            Some((s.id.clone(), "ociosa além do idle_ttl_secs"))
                        } else {
                            None
                        }
                    })
                    .collect()
            };
            for (id, why) in doomed {
                tracing::info!(sandbox = %id, "destruindo sandbox: {why}");
                if let Err(e) = self.destroy_sandbox(&id).await {
                    tracing::warn!(sandbox = %id, "falha ao destruir: {}", e.message);
                }
            }
        }
    }

    /// Destrói uma sandbox (sem checar dono: quem chama já checou).
    pub async fn destroy_sandbox(&self, id: &str) -> Result<(), RpcError> {
        let (worker, active, sessions) = {
            let st = self.state.lock();
            let sb = st.sandboxes.get(id).ok_or_else(|| RpcError::not_found("a sandbox", id))?;
            (sb.worker, matches!(sb.status, SbStatus::Active), sb.sessions.clone())
        };
        if active {
            match self.call(worker, Call::DestroySandbox { sandbox_id: id.to_string() }, None).await {
                Ok(_) => {}
                // A sandbox já não existe no worker (caiu no meio, ou nunca chegou a existir).
                Err(e) if matches!(e.code, codes::NOT_FOUND | codes::WORKER_CRASHED) => {}
                Err(e) if e.code == codes::WORKER_UNAVAILABLE => {}
                Err(e) => return Err(e),
            }
        }
        let snapshot_dir = self.cfg.snapshots_dir().join(id);
        {
            let mut st = self.state.lock();
            st.release(id);
            for sid in sessions {
                if let Some(s) = st.sessions.remove(&sid)
                    && let Some(u) = st.users.get_mut(&s.owner)
                {
                    u.sessions = u.sessions.saturating_sub(1);
                }
            }
            if let Some(sb) = st.sandboxes.remove(id) {
                st.forget_worker_if_idle(&sb.owner);
            }
        }
        let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(snapshot_dir)).await;
        Ok(())
    }

    /// Reserva uma vaga de `exec` pro usuário.
    pub fn acquire_exec(self: &Arc<Self>, user: &str, quota: &Quota) -> Result<ExecPermit, RpcError> {
        let mut st = self.state.lock();
        let used = st.usage(user).execs;
        if used >= quota.max_concurrent_execs {
            return Err(RpcError::quota(
                "concurrent_execs",
                u64::from(quota.max_concurrent_execs),
                u64::from(used),
                format!("limite de execuções simultâneas do usuário atingido ({})", quota.max_concurrent_execs),
            ));
        }
        if st.total_execs >= self.cfg.service.max_concurrent_execs {
            return Err(RpcError::capacity("concurrent_execs", "o serviço está no limite de execuções simultâneas; tente de novo"));
        }
        st.total_execs += 1;
        st.users.entry(user.to_string()).or_default().execs += 1;
        Ok(ExecPermit { sup: self.clone(), user: user.to_string() })
    }
}
