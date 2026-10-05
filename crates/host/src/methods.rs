//! Os métodos do JSON-RPC, em cima do [`Supervisor`].
//!
//! Regras comuns: o usuário só enxerga as próprias sandboxes e sessões (as dos outros respondem
//! `not_found`, sem revelar que existem); admin enxerga tudo. Caminho relativo nos `fs.*` resolve a
//! partir do `workdir` da sandbox.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::api::*;
use crate::auth::{AuthError, KeyState, Principal, Role};
use crate::config::QuotaOverride;
use crate::exec::{ExecOutcome, Stream};
use crate::ids::{random_id, valid_id};
use crate::ipc::{Call, ExecCall, Reply, WireLimits};
use crate::rpc::{RpcError, codes, params};
use crate::supervisor::{SandboxEntry, SbStatus, SessionEntry, SnapEntry, Supervisor};
use crate::timeutil::{fmt_utc, now_unix, parse_expiry};

/// Ambiente padrão de todo processo, o mesmo do oráculo da bancada.
pub const DEFAULT_ENV: &[(&str, &str)] = &[
    ("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"),
    ("HOME", "/root"),
    ("USER", "root"),
    ("LOGNAME", "root"),
    ("SHELL", "/bin/bash"),
    ("LC_ALL", "C.UTF-8"),
    ("TZ", "UTC"),
];

/// Métodos que respondem em streaming (notificações `exec.output` antes da resposta).
pub const STREAMING_METHODS: &[&str] = &["exec.stream", "session.exec.stream"];

/// Pra onde vão as notificações de streaming de uma requisição.
#[derive(Clone, Debug)]
pub struct Notifier {
    pub tx: mpsc::UnboundedSender<Value>,
    pub request_id: Value,
}

/// Converte pedaços de bytes em texto sem quebrar um caractere UTF-8 entre duas notificações.
#[derive(Default)]
struct Utf8Carry {
    pending: Vec<u8>,
}

impl Utf8Carry {
    fn push(&mut self, data: &[u8]) -> String {
        self.pending.extend_from_slice(data);
        let cut = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => self.pending.len(),
        };
        let out = String::from_utf8_lossy(&self.pending[..cut]).into_owned();
        self.pending.drain(..cut);
        out
    }

    fn finish(&mut self) -> String {
        let out = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        out
    }
}

/// Encaminha os eventos de saída do worker como notificações, até o canal fechar.
fn spawn_forwarder(n: Notifier, enc: Encoding) -> (mpsc::UnboundedSender<(Stream, Vec<u8>)>, tokio::task::JoinHandle<()>) {
    let (tx, mut rx) = mpsc::unbounded_channel::<(Stream, Vec<u8>)>();
    let h = tokio::spawn(async move {
        let mut carry_out = Utf8Carry::default();
        let mut carry_err = Utf8Carry::default();
        let send = |stream: Stream, data: String| {
            if !data.is_empty() {
                let _ = n.tx.send(crate::rpc::notification(
                    "exec.output",
                    json!({ "request_id": n.request_id, "stream": stream, "encoding": enc, "data": data }),
                ));
            }
        };
        while let Some((stream, data)) = rx.recv().await {
            let text = match enc {
                Encoding::Base64 => b64::encode(&data),
                Encoding::Utf8 => {
                    if stream == Stream::Stdout {
                        carry_out.push(&data)
                    } else {
                        carry_err.push(&data)
                    }
                }
            };
            send(stream, text);
        }
        send(Stream::Stdout, carry_out.finish());
        send(Stream::Stderr, carry_err.finish());
    });
    (tx, h)
}

fn to_result(o: &ExecOutcome, enc: Encoding, streamed: bool) -> ExecResult {
    let (stdout, stdout_lossy) = if streamed { (String::new(), false) } else { encode_bytes(&o.stdout.data, enc) };
    let (stderr, stderr_lossy) = if streamed { (String::new(), false) } else { encode_bytes(&o.stderr.data, enc) };
    ExecResult {
        exit_code: o.exit_code,
        signal: o.signal,
        signal_name: o.signal.map(|s| sysabi::Signal(s).name().map(|n| format!("SIG{n}")).unwrap_or_else(|| s.to_string())),
        status: o.status(),
        timed_out: o.timed_out,
        cancelled: o.cancelled,
        duration_ms: o.duration_ms,
        encoding: enc,
        stdout,
        stderr,
        stdout_truncated: o.stdout.truncated,
        stderr_truncated: o.stderr.truncated,
        stdout_bytes: o.stdout.total,
        stderr_bytes: o.stderr.total,
        stdout_lossy,
        stderr_lossy,
        output_closed: o.stdout.closed || o.stderr.closed,
        background_detached: o.background_detached,
        streamed,
        cwd: None,
        session_reset: false,
        session_closed: false,
    }
}

fn json_of<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).expect("tipos da API sempre serializam")
}

fn require_admin(p: &Principal) -> Result<(), RpcError> {
    if p.is_admin() { Ok(()) } else { Err(RpcError::forbidden("método só pra admin")) }
}

fn auth_err(e: AuthError) -> RpcError {
    match e {
        AuthError::NoSuchUser(_) | AuthError::NoSuchKey(_) => RpcError::new(codes::NOT_FOUND, "not_found", e.to_string()),
        AuthError::UserExists(_) | AuthError::BadInput(_) => RpcError::invalid_params(e.to_string()),
        other => RpcError::internal(other.to_string()),
    }
}

fn bytes_param(text: Option<String>, b64v: Option<String>, what: &str) -> Result<Vec<u8>, RpcError> {
    match (text, b64v) {
        (Some(_), Some(_)) => Err(RpcError::invalid_params(format!("use {what} ou {what}_base64, não os dois"))),
        (Some(t), None) => Ok(t.into_bytes()),
        (None, Some(b)) => b64::decode(&b).map_err(|e| RpcError::invalid_params(format!("{what}_base64: {e}"))),
        (None, None) => Ok(Vec::new()),
    }
}

fn valid_env_name(k: &str) -> bool {
    !k.is_empty() && !k.contains('=') && !k.contains('\0')
}

fn check_env(env: &BTreeMap<String, String>) -> Result<(), RpcError> {
    for (k, v) in env {
        if !valid_env_name(k) || v.contains('\0') {
            return Err(RpcError::invalid_params(format!("variável de ambiente inválida: {k:?}")));
        }
    }
    Ok(())
}

fn check_path(p: &str) -> Result<(), RpcError> {
    if p.is_empty() || p.contains('\0') || p.len() > 4096 {
        return Err(RpcError::invalid_params(format!("caminho inválido: {p:?}")));
    }
    Ok(())
}

/// O que uma operação precisa saber da sandbox.
struct SbRef {
    id: String,
    worker: usize,
    workdir: String,
    env: BTreeMap<String, String>,
    owner: String,
}

impl SbRef {
    fn abs(&self, path: &str) -> Result<String, RpcError> {
        check_path(path)?;
        Ok(if path.starts_with('/') {
            path.to_string()
        } else {
            format!("{}/{}", self.workdir.trim_end_matches('/'), path)
        })
    }

    fn full_env(&self, extra: &BTreeMap<String, String>, clear: bool) -> Vec<String> {
        let mut env: BTreeMap<String, String> = BTreeMap::new();
        if !clear {
            for (k, v) in DEFAULT_ENV {
                env.insert(k.to_string(), v.to_string());
            }
            env.extend(self.env.clone());
        }
        env.extend(extra.clone());
        env.into_iter().map(|(k, v)| format!("{k}={v}")).collect()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListParams {
    #[serde(default)]
    all: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminUserCreate {
    name: String,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    quota: QuotaOverride,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminUserUpdate {
    name: String,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    disabled: Option<bool>,
    #[serde(default)]
    quota: Option<QuotaOverride>,
    #[serde(default)]
    reset_quota: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminName {
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminKeyCreate {
    user: String,
    #[serde(default)]
    label: String,
    /// `90d`, `12h`, `never` ou uma data `AAAA-MM-DD` (padrão: 90d).
    #[serde(default)]
    expires: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminKeyList {
    #[serde(default)]
    user: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminKeyRevoke {
    key_id: String,
}

impl Supervisor {
    /// Atende um método. `notifier` só existe quando o transporte aceita streaming.
    pub async fn handle(
        self: &Arc<Self>,
        p: &Principal,
        method: &str,
        prm: Value,
        notifier: Option<Notifier>,
    ) -> Result<Value, RpcError> {
        if self.is_shutting_down() {
            return Err(RpcError::cancelled("o daemon está desligando"));
        }
        match method {
            "whoami" => self.whoami(p),
            "sandbox.create" => self.sandbox_create(p, params(prm)?).await,
            "sandbox.destroy" => {
                let r: SandboxRef = params(prm)?;
                self.owned(p, &r.sandbox_id)?;
                self.destroy_sandbox(&r.sandbox_id).await?;
                Ok(json!({ "destroyed": true }))
            }
            "sandbox.list" => {
                let l: ListParams = params(prm)?;
                if l.all {
                    require_admin(p)?;
                }
                let st = self.state.lock();
                let mut v: Vec<SandboxInfo> =
                    st.sandboxes.values().filter(|s| l.all || s.owner == p.user).map(SandboxEntry::info).collect();
                v.sort_by_key(|s| s.created_at);
                Ok(json!({ "sandboxes": v }))
            }
            "sandbox.info" => {
                let r: SandboxRef = params(prm)?;
                let sb = self.active(p, &r.sandbox_id)?;
                let usage = match self.call(sb.worker, Call::Usage { sandbox_id: sb.id.clone() }, None).await? {
                    Reply::Usage { usage } => usage,
                    other => return Err(unexpected(&other)),
                };
                let info = self.state.lock().sandboxes.get(&sb.id).map(SandboxEntry::info);
                Ok(json!({ "sandbox": info, "usage": usage }))
            }
            "exec" => self.exec(p, params(prm)?, None).await,
            "exec.stream" => self.exec(p, params(prm)?, notifier).await,
            "session.open" => self.session_open(p, params(prm)?).await,
            "session.exec" => self.session_exec(p, params(prm)?, None).await,
            "session.exec.stream" => self.session_exec(p, params(prm)?, notifier).await,
            "session.close" => {
                let r: SessionRef = params(prm)?;
                self.session_close(p, &r.session_id).await
            }
            "session.list" => {
                let st = self.state.lock();
                let v: Vec<Value> = st
                    .sessions
                    .values()
                    .filter(|s| s.owner == p.user)
                    .map(|s| json!({ "session_id": s.id, "sandbox_id": s.sandbox_id, "created_at": s.created_at }))
                    .collect();
                Ok(json!({ "sessions": v }))
            }
            "fs.read" => self.fs_read(p, params(prm)?).await,
            "fs.write" => self.fs_write(p, params(prm)?).await,
            "fs.list" => {
                let r: FsPathParams = params(prm)?;
                let sb = self.active(p, &r.sandbox_id)?;
                let path = sb.abs(&r.path)?;
                match self.call(sb.worker, Call::FsList { sandbox_id: sb.id, path: path.clone() }, None).await? {
                    Reply::FsList { entries } => Ok(json!({ "path": path, "entries": entries })),
                    other => Err(unexpected(&other)),
                }
            }
            "fs.stat" => {
                let r: FsStatParams = params(prm)?;
                let sb = self.active(p, &r.sandbox_id)?;
                let path = sb.abs(&r.path)?;
                match self.call(sb.worker, Call::FsStat { sandbox_id: sb.id, path, follow: r.follow }, None).await? {
                    Reply::FsStat { info } => Ok(json_of(&info)),
                    other => Err(unexpected(&other)),
                }
            }
            "fs.mkdir" => {
                let r: FsMkdirParams = params(prm)?;
                let sb = self.active(p, &r.sandbox_id)?;
                let path = sb.abs(&r.path)?;
                let mode = r.mode.unwrap_or(0o755) & 0o7777;
                self.call(sb.worker, Call::FsMkdir { sandbox_id: sb.id, path, parents: r.parents, mode }, None).await?;
                Ok(json!({ "ok": true }))
            }
            "fs.remove" => {
                let r: FsRemoveParams = params(prm)?;
                let sb = self.active(p, &r.sandbox_id)?;
                let path = sb.abs(&r.path)?;
                if path.trim_end_matches('/').is_empty() {
                    return Err(RpcError::invalid_params("não dá pra remover a raiz da sandbox; use sandbox.destroy"));
                }
                self.call(sb.worker, Call::FsRemove { sandbox_id: sb.id, path, recursive: r.recursive, force: r.force }, None)
                    .await?;
                Ok(json!({ "ok": true }))
            }
            "snapshot" | "snapshot.create" => self.snapshot(p, params(prm)?).await,
            "snapshot.delete" => self.snapshot_delete(p, params(prm)?).await,
            "restore" | "snapshot.restore" => self.restore(p, params(prm)?).await,
            "ps" => {
                let r: SandboxRef = params(prm)?;
                let sb = self.active(p, &r.sandbox_id)?;
                match self.call(sb.worker, Call::Ps { sandbox_id: sb.id }, None).await? {
                    Reply::Ps { processes } => Ok(json!({ "processes": processes })),
                    other => Err(unexpected(&other)),
                }
            }
            "kill" => {
                let r: KillParams = params(prm)?;
                let sb = self.active(p, &r.sandbox_id)?;
                let sig = match &r.signal {
                    None => sysabi::Signal::SIGTERM,
                    Some(Value::Number(n)) => n.as_i64().and_then(|n| i32::try_from(n).ok()).map(sysabi::Signal).filter(|s| s.0 == 0 || s.is_valid()).ok_or_else(|| RpcError::invalid_params("sinal inválido"))?,
                    Some(Value::String(s)) => sysabi::Signal::parse(s).ok_or_else(|| RpcError::invalid_params(format!("sinal desconhecido: {s}")))?,
                    Some(_) => return Err(RpcError::invalid_params("signal precisa ser número ou nome")),
                };
                if r.pid <= 0 {
                    return Err(RpcError::invalid_params("pid precisa ser positivo (use group pra grupo de processos)"));
                }
                self.call(sb.worker, Call::Kill { sandbox_id: sb.id, pid: r.pid, signal: sig.0, group: r.group }, None)
                    .await?;
                Ok(json!({ "ok": true }))
            }
            "export" => {
                let r: ExportParams = params(prm)?;
                let sb = self.active(p, &r.sandbox_id)?;
                let path = sb.abs(r.path.as_deref().unwrap_or("/"))?;
                let max = self.cfg.service.max_request_bytes.0;
                match self.call(sb.worker, Call::Export { sandbox_id: sb.id, path, max_bytes: max }, None).await? {
                    Reply::Export { data, report } => {
                        Ok(json!({ "data_base64": b64::encode(&data), "bytes": data.len(), "report": report }))
                    }
                    other => Err(unexpected(&other)),
                }
            }
            "import" => {
                let r: ImportParams = params(prm)?;
                let sb = self.active(p, &r.sandbox_id)?;
                let path = sb.abs(r.path.as_deref().unwrap_or("/"))?;
                let data = b64::decode(&r.data_base64).map_err(|e| RpcError::invalid_params(format!("data_base64: {e}")))?;
                let max = self.cfg.service.max_request_bytes.0;
                match self.call(sb.worker, Call::Import { sandbox_id: sb.id, path, data, max_bytes: max }, None).await? {
                    Reply::Import { report } => Ok(json!({ "report": report })),
                    other => Err(unexpected(&other)),
                }
            }
            m if m.starts_with("admin.") => {
                require_admin(p)?;
                self.admin(p, m, prm).await
            }
            _ => Err(RpcError::method_not_found(method)),
        }
    }

    fn whoami(&self, p: &Principal) -> Result<Value, RpcError> {
        let quota = p.effective_quota(&self.cfg.quota);
        let usage = self.state.lock().usage(&p.user);
        Ok(json!({ "user": p.user, "role": p.role.as_str(), "key_id": p.key_id, "quota": quota, "usage": usage }))
    }

    /// Confere dono (admin passa) e devolve a entrada; sandboxes de outros são `not_found`.
    fn owned(&self, p: &Principal, id: &str) -> Result<(), RpcError> {
        if !valid_id("sb", id) {
            return Err(RpcError::not_found("a sandbox", id));
        }
        let st = self.state.lock();
        match st.sandboxes.get(id) {
            Some(sb) if sb.owner == p.user || p.is_admin() => Ok(()),
            _ => Err(RpcError::not_found("a sandbox", id)),
        }
    }

    /// Sandbox ativa do usuário; marca o uso (pro `idle_ttl_secs`).
    fn active(&self, p: &Principal, id: &str) -> Result<SbRef, RpcError> {
        self.owned(p, id)?;
        let mut st = self.state.lock();
        let sb = st.sandboxes.get_mut(id).ok_or_else(|| RpcError::not_found("a sandbox", id))?;
        match &sb.status {
            SbStatus::Active => {}
            SbStatus::Creating => {
                return Err(RpcError::new(codes::WORKER_UNAVAILABLE, "creating", "a sandbox ainda está sendo criada"));
            }
            SbStatus::Recovering { from } => {
                return Err(RpcError::new(
                    codes::WORKER_UNAVAILABLE,
                    "recovering",
                    format!("a sandbox está sendo recuperada do snapshot {from} depois da queda do worker {}", sb.worker),
                ));
            }
            SbStatus::Lost { reason, .. } => {
                return Err(RpcError::new(codes::SANDBOX_LOST, "sandbox_lost", format!("a sandbox {id} se perdeu: {reason}"))
                    .with("reason", reason.clone()));
            }
        }
        sb.last_used_at = now_unix();
        Ok(SbRef { id: sb.id.clone(), worker: sb.worker, workdir: sb.workdir.clone(), env: sb.env.clone(), owner: sb.owner.clone() })
    }

    async fn sandbox_create(self: &Arc<Self>, p: &Principal, r: SandboxCreateParams) -> Result<Value, RpcError> {
        let quota = p.effective_quota(&self.cfg.quota);
        let d = &self.cfg.sandbox;
        let image = r.image.unwrap_or_else(|| "default".into());
        if image != "default" {
            return Err(RpcError::invalid_params(format!("imagem desconhecida: {image} (só existe default)")));
        }
        let hostname = r.hostname.unwrap_or_else(|| "pseudo-linus".into());
        if hostname.is_empty()
            || hostname.len() > 64
            || !hostname.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
        {
            return Err(RpcError::invalid_params(format!("hostname inválido: {hostname:?}")));
        }
        let workdir = r.workdir.unwrap_or_else(|| "/root".into());
        check_path(&workdir)?;
        if !workdir.starts_with('/') {
            return Err(RpcError::invalid_params("workdir precisa ser absoluto"));
        }
        check_env(&r.env)?;
        if r.labels.len() > 32 || r.labels.iter().any(|(k, v)| k.len() > 64 || v.len() > 256) {
            return Err(RpcError::invalid_params("labels: até 32, chave até 64 e valor até 256 caracteres"));
        }
        let l = &r.limits;
        let limits = SandboxLimits {
            mem_bytes: l.mem_bytes.unwrap_or(d.mem_bytes.0),
            max_procs: l.max_procs.unwrap_or(d.max_procs),
            fs_bytes: l.fs_bytes.unwrap_or(d.fs_bytes.0),
            nofile: l.nofile.unwrap_or(d.nofile),
            cpu_weight: l.cpu_weight.unwrap_or(100),
            cpu_max: l.cpu_max,
        };
        if limits.mem_bytes < (16 << 20) || limits.max_procs == 0 || limits.fs_bytes < (1 << 20) || limits.nofile < 16 {
            return Err(RpcError::invalid_params(
                "limites pequenos demais (mínimos: mem_bytes 16 MiB, max_procs 1, fs_bytes 1 MiB, nofile 16)",
            ));
        }
        if !(1..=10_000).contains(&limits.cpu_weight) {
            return Err(RpcError::invalid_params("cpu_weight fora de 1..=10000"));
        }
        if let Some(c) = &limits.cpu_max {
            c.validate().map_err(RpcError::invalid_params)?;
        }
        let now = now_unix();
        let id = random_id("sb");
        let worker = {
            let mut st = self.state.lock();
            let u = st.usage(&p.user);
            if u.sandboxes >= quota.max_sandboxes {
                return Err(RpcError::quota(
                    "sandboxes",
                    u64::from(quota.max_sandboxes),
                    u64::from(u.sandboxes),
                    format!("limite de sandboxes do usuário atingido ({})", quota.max_sandboxes),
                ));
            }
            if u.mem_bytes + limits.mem_bytes > quota.mem_bytes.0 {
                return Err(RpcError::quota(
                    "mem_bytes",
                    quota.mem_bytes.0,
                    u.mem_bytes,
                    format!(
                        "a memória pedida ({} bytes) passa da quota do usuário ({} de {} bytes em uso)",
                        limits.mem_bytes, u.mem_bytes, quota.mem_bytes.0
                    ),
                ));
            }
            if u.procs + u64::from(limits.max_procs) > u64::from(quota.max_procs) {
                return Err(RpcError::quota(
                    "max_procs",
                    u64::from(quota.max_procs),
                    u.procs,
                    format!("os processos pedidos ({}) passam da quota do usuário ({})", limits.max_procs, quota.max_procs),
                ));
            }
            if st.total_sandboxes >= self.cfg.service.max_sandboxes {
                return Err(RpcError::capacity("sandboxes", "o serviço está no limite de sandboxes; tente mais tarde"));
            }
            if st.total_mem + limits.mem_bytes > self.cfg.service.memory_budget.0 {
                return Err(RpcError::capacity("memory", "o serviço está sem memória pra mais uma sandbox; tente mais tarde"));
            }
            let worker = self.pick_worker(&st, &p.user)?;
            st.user_worker.insert(p.user.clone(), worker);
            st.total_sandboxes += 1;
            st.total_mem += limits.mem_bytes;
            let uu = st.users.entry(p.user.clone()).or_default();
            uu.sandboxes += 1;
            uu.mem_bytes += limits.mem_bytes;
            uu.procs += u64::from(limits.max_procs);
            st.sandboxes.insert(
                id.clone(),
                SandboxEntry {
                    id: id.clone(),
                    owner: p.user.clone(),
                    worker,
                    image: image.clone(),
                    hostname: hostname.clone(),
                    workdir: workdir.clone(),
                    env: r.env.clone(),
                    labels: r.labels.clone(),
                    limits: limits.clone(),
                    created_at: now,
                    last_used_at: now,
                    status: SbStatus::Creating,
                    recovered_from: None,
                    snapshots: Vec::new(),
                    sessions: Default::default(),
                    reserved: true,
                    dirty: false,
                    last_autosave: 0,
                },
            );
            worker
        };
        let user = crate::backend::UserSched { user: p.user.clone(), cpu_weight: quota.cpu_weight, cpu_max: quota.cpu_max };
        let spec = crate::backend::SandboxSpec { image, hostname, limits };
        let r = self.call(worker, Call::CreateSandbox { sandbox_id: id.clone(), user, spec }, None).await;
        let mut st = self.state.lock();
        match r {
            Ok(_) => {
                let sb = st.sandboxes.get_mut(&id).ok_or_else(|| RpcError::internal("sandbox sumiu durante a criação"))?;
                if sb.status == SbStatus::Creating {
                    sb.status = SbStatus::Active;
                }
                let info = sb.info();
                Ok(json_of(&info))
            }
            Err(e) => {
                st.release(&id);
                st.sandboxes.remove(&id);
                st.forget_worker_if_idle(&p.user);
                Err(e)
            }
        }
    }

    fn limits_for(&self, p: &Principal, timeout_ms: Option<u64>, output: Option<u64>) -> Result<WireLimits, RpcError> {
        let q = p.effective_quota(&self.cfg.quota);
        let d = &self.cfg.sandbox;
        let timeout_ms = timeout_ms.unwrap_or(d.exec_timeout_ms.min(q.max_timeout_ms));
        if timeout_ms == 0 || timeout_ms > q.max_timeout_ms {
            return Err(RpcError::invalid_params(format!("timeout_ms precisa ficar entre 1 e {}", q.max_timeout_ms)));
        }
        let output_limit = output.unwrap_or(d.output_limit_bytes.0.min(q.max_output_bytes.0));
        if output_limit > q.max_output_bytes.0 {
            return Err(RpcError::invalid_params(format!(
                "output_limit_bytes passa do máximo do usuário ({})",
                q.max_output_bytes.0
            )));
        }
        Ok(WireLimits {
            timeout_ms,
            output_limit,
            max_discard: d.max_discard_bytes.0,
            drain_grace_ms: d.drain_grace_ms,
        })
    }

    async fn exec(self: &Arc<Self>, p: &Principal, r: ExecParams, notifier: Option<Notifier>) -> Result<Value, RpcError> {
        if r.command.is_some() == r.argv.is_some() {
            return Err(RpcError::invalid_params("use exatamente um de command e argv"));
        }
        if r.argv.as_ref().is_some_and(|a| a.is_empty() || a.iter().any(|s| s.contains('\0'))) {
            return Err(RpcError::invalid_params("argv vazio ou com NUL"));
        }
        if r.command.as_ref().is_some_and(|c| c.contains('\0')) {
            return Err(RpcError::invalid_params("command com NUL"));
        }
        check_env(&r.env)?;
        let stdin = bytes_param(r.stdin, r.stdin_base64, "stdin")?;
        let limits = self.limits_for(p, r.timeout_ms, r.output_limit_bytes)?;
        let sb = self.active(p, &r.sandbox_id)?;
        let cwd = sb.abs(r.cwd.as_deref().unwrap_or(&sb.workdir.clone()))?;
        let env = sb.full_env(&r.env, r.clear_env);
        let quota = self.user_quota(&sb.owner);
        let _permit = self.acquire_exec(&p.user, &quota)?;
        let call = Call::Exec {
            sandbox_id: sb.id.clone(),
            exec: ExecCall { command: r.command, argv: r.argv, cwd, env, stdin, limits },
            stream: notifier.is_some(),
        };
        let streamed = notifier.is_some();
        let (events, fwd) = match notifier {
            Some(n) => {
                let (tx, h) = spawn_forwarder(n, r.encoding);
                (Some(tx), Some(h))
            }
            None => (None, None),
        };
        let reply = self.call(sb.worker, call, events).await;
        if let Some(h) = fwd {
            let _ = h.await;
        }
        match reply? {
            Reply::Exec { outcome } => Ok(json_of(&to_result(&outcome, r.encoding, streamed))),
            other => Err(unexpected(&other)),
        }
    }

    async fn session_open(self: &Arc<Self>, p: &Principal, r: SessionOpenParams) -> Result<Value, RpcError> {
        check_env(&r.env)?;
        let sb = self.active(p, &r.sandbox_id)?;
        let quota = self.user_quota(&sb.owner);
        let session_id = random_id("ss");
        {
            let mut st = self.state.lock();
            let u = st.usage(&p.user);
            if u.sessions >= quota.max_sessions {
                return Err(RpcError::quota(
                    "sessions",
                    u64::from(quota.max_sessions),
                    u64::from(u.sessions),
                    format!("limite de sessões abertas do usuário atingido ({})", quota.max_sessions),
                ));
            }
            st.users.entry(p.user.clone()).or_default().sessions += 1;
        }
        let cwd = sb.abs(r.cwd.as_deref().unwrap_or(&sb.workdir.clone()))?;
        let env = sb.full_env(&r.env, false);
        let call = Call::SessionOpen {
            sandbox_id: sb.id.clone(),
            session_id: session_id.clone(),
            cwd,
            env,
            workdir: sb.workdir.clone(),
            dump: Vec::new(),
        };
        let res = self.call(sb.worker, call, None).await;
        let mut st = self.state.lock();
        match res {
            Ok(_) => {
                st.sessions.insert(
                    session_id.clone(),
                    SessionEntry {
                        id: session_id.clone(),
                        owner: p.user.clone(),
                        sandbox_id: sb.id.clone(),
                        created_at: now_unix(),
                        snapshot: None,
                    },
                );
                if let Some(e) = st.sandboxes.get_mut(&sb.id) {
                    e.sessions.insert(session_id.clone());
                }
                Ok(json!({ "session_id": session_id, "sandbox_id": sb.id }))
            }
            Err(e) => {
                if let Some(u) = st.users.get_mut(&p.user) {
                    u.sessions = u.sessions.saturating_sub(1);
                }
                Err(e)
            }
        }
    }

    fn session_of(&self, p: &Principal, id: &str) -> Result<SessionEntry, RpcError> {
        let st = self.state.lock();
        if let Some(s) = st.sessions.get(id)
            && (s.owner == p.user || p.is_admin())
        {
            return Ok(s.clone());
        }
        if let Some((owner, reason, _)) = st.dead_sessions.get(id)
            && (*owner == p.user || p.is_admin())
        {
            return Err(RpcError::new(codes::SESSION_LOST, "session_closed", reason.clone()));
        }
        Err(RpcError::not_found("a sessão", id))
    }

    fn bury_session(&self, id: &str, reason: String) {
        let mut st = self.state.lock();
        if let Some(s) = st.sessions.remove(id) {
            if let Some(u) = st.users.get_mut(&s.owner) {
                u.sessions = u.sessions.saturating_sub(1);
            }
            if let Some(sb) = st.sandboxes.get_mut(&s.sandbox_id) {
                sb.sessions.remove(id);
            }
            st.dead_sessions.insert(id.to_string(), (s.owner, reason, now_unix()));
        }
    }

    /// Recria, no worker novo, a sessão que ficou órfã com a queda do worker, com o estado do shell do
    /// último comando (cwd, ambiente, variáveis, funções, aliases). `false` se a sessão não é órfã.
    /// Enquanto a sandbox ainda está sendo recuperada, devolve o erro `recovering` e a sessão continua órfã.
    async fn revive_session(self: &Arc<Self>, p: &Principal, id: &str) -> Result<bool, RpcError> {
        let entry = {
            let st = self.state.lock();
            match st.orphan_sessions.get(id) {
                Some(s) if s.owner == p.user || p.is_admin() => s.clone(),
                _ => return Ok(false),
            }
        };
        let sb = self.active(p, &entry.sandbox_id)?;
        // Reivindica a sessão antes da chamada, pra dois comandos concorrentes não a recriarem duas vezes.
        if self.state.lock().orphan_sessions.remove(id).is_none() {
            return Ok(true);
        }
        let (cwd, env, dump) = match &entry.snapshot {
            Some(s) => (s.cwd.clone(), s.env.clone(), s.dump.clone()),
            None => (sb.workdir.clone(), sb.full_env(&BTreeMap::new(), false), Vec::new()),
        };
        let call = Call::SessionOpen {
            sandbox_id: sb.id.clone(),
            session_id: id.to_string(),
            cwd,
            env,
            workdir: sb.workdir.clone(),
            dump,
        };
        match self.call(sb.worker, call, None).await {
            Ok(_) => {
                let mut st = self.state.lock();
                st.sessions.insert(id.to_string(), entry.clone());
                if let Some(e) = st.sandboxes.get_mut(&sb.id) {
                    e.sessions.insert(id.to_string());
                }
                Ok(true)
            }
            Err(e) => {
                self.state.lock().orphan_sessions.insert(id.to_string(), entry);
                Err(e)
            }
        }
    }

    async fn session_exec(self: &Arc<Self>, p: &Principal, r: SessionExecParams, notifier: Option<Notifier>) -> Result<Value, RpcError> {
        let revived = self.revive_session(p, &r.session_id).await?;
        let s = self.session_of(p, &r.session_id)?;
        if r.command.contains('\0') {
            return Err(RpcError::invalid_params("command com NUL"));
        }
        let stdin = bytes_param(r.stdin, r.stdin_base64, "stdin")?;
        let limits = self.limits_for(p, r.timeout_ms, r.output_limit_bytes)?;
        let sb = self.active(p, &s.sandbox_id)?;
        let quota = self.user_quota(&sb.owner);
        let _permit = self.acquire_exec(&p.user, &quota)?;
        let streamed = notifier.is_some();
        let (events, fwd) = match notifier {
            Some(n) => {
                let (tx, h) = spawn_forwarder(n, r.encoding);
                (Some(tx), Some(h))
            }
            None => (None, None),
        };
        let call = Call::SessionExec { session_id: s.id.clone(), command: r.command, stdin, limits, stream: streamed };
        let reply = self.call(sb.worker, call, events).await;
        if let Some(h) = fwd {
            let _ = h.await;
        }
        let outcome = match reply {
            Ok(Reply::Session { outcome }) => outcome,
            Ok(other) => return Err(unexpected(&other)),
            Err(e) => {
                if e.code == codes::SESSION_LOST {
                    self.bury_session(&s.id, e.message.clone());
                }
                return Err(e);
            }
        };
        if outcome.closed {
            let code = outcome.exec.exit_code.map(|c| format!(" com {c}")).unwrap_or_default();
            self.bury_session(&s.id, format!("a sessão terminou: o shell saiu{code}"));
        }
        if let Some(snap) = outcome.snapshot.clone()
            && let Some(e) = self.state.lock().sessions.get_mut(&s.id)
        {
            e.snapshot = Some(snap);
        }
        let mut res = to_result(&outcome.exec, r.encoding, streamed);
        res.cwd = outcome.cwd;
        res.session_reset = outcome.reset || revived;
        res.session_closed = outcome.closed;
        Ok(json_of(&res))
    }

    async fn session_close(self: &Arc<Self>, p: &Principal, id: &str) -> Result<Value, RpcError> {
        // Sessão órfã (o worker caiu e a sandbox ainda volta): fechar só tira o registro.
        {
            let mut st = self.state.lock();
            if st.orphan_sessions.get(id).is_some_and(|s| s.owner == p.user || p.is_admin())
                && let Some(s) = st.orphan_sessions.remove(id)
            {
                if let Some(u) = st.users.get_mut(&s.owner) {
                    u.sessions = u.sessions.saturating_sub(1);
                }
                return Ok(json!({ "closed": true }));
            }
        }
        let s = match self.session_of(p, id) {
            Ok(s) => s,
            // Fechar uma sessão que já tinha acabado não é erro.
            Err(e) if e.code == codes::SESSION_LOST => {
                self.state.lock().dead_sessions.remove(id);
                return Ok(json!({ "closed": true }));
            }
            Err(e) => return Err(e),
        };
        let worker = self.state.lock().sandboxes.get(&s.sandbox_id).map(|sb| sb.worker);
        if let Some(w) = worker {
            match self.call(w, Call::SessionClose { session_id: id.to_string() }, None).await {
                Ok(_) => {}
                Err(e) if matches!(e.code, codes::NOT_FOUND | codes::WORKER_CRASHED | codes::WORKER_UNAVAILABLE) => {}
                Err(e) => return Err(e),
            }
        }
        self.bury_session(id, "a sessão foi fechada".into());
        self.state.lock().dead_sessions.remove(id);
        Ok(json!({ "closed": true }))
    }

    async fn fs_read(self: &Arc<Self>, p: &Principal, r: FsReadParams) -> Result<Value, RpcError> {
        let sb = self.active(p, &r.sandbox_id)?;
        let path = sb.abs(&r.path)?;
        let cap = self.cfg.service.max_request_bytes.0;
        let max = r.length.unwrap_or(cap);
        if max > cap {
            return Err(RpcError::too_large(format!("length passa do teto de resposta ({cap} bytes); leia em pedaços com offset")));
        }
        match self.call(sb.worker, Call::FsRead { sandbox_id: sb.id, path, offset: r.offset, max }, None).await? {
            Reply::FsRead { data, size, eof } => {
                let (text, lossy) = encode_bytes(&data, r.encoding);
                let mut v = json!({
                    "data": text, "encoding": r.encoding, "size": size, "offset": r.offset,
                    "bytes": data.len(), "eof": eof,
                });
                if lossy {
                    v["lossy"] = json!(true);
                }
                Ok(v)
            }
            other => Err(unexpected(&other)),
        }
    }

    async fn fs_write(self: &Arc<Self>, p: &Principal, r: FsWriteParams) -> Result<Value, RpcError> {
        let data = bytes_param(r.data, r.data_base64, "data")?;
        let sb = self.active(p, &r.sandbox_id)?;
        let path = sb.abs(&r.path)?;
        let mode = r.mode.unwrap_or(0o644) & 0o7777;
        let n = data.len();
        let call = Call::FsWrite {
            sandbox_id: sb.id,
            path,
            data,
            mode,
            append: r.append,
            exclusive: r.exclusive,
            create_parents: r.create_parents,
        };
        self.call(sb.worker, call, None).await?;
        Ok(json!({ "bytes_written": n }))
    }

    async fn snapshot(self: &Arc<Self>, p: &Principal, r: SnapshotParams) -> Result<Value, RpcError> {
        let sb = self.active(p, &r.sandbox_id)?;
        let quota = self.user_quota(&sb.owner);
        let name = r.name.unwrap_or_default();
        if name.len() > 128 {
            return Err(RpcError::invalid_params("name até 128 caracteres"));
        }
        let snapshot_id = random_id("sn");
        let persist_path = r.persist.then(|| self.cfg.snapshots_dir().join(&sb.id).join(format!("{snapshot_id}.tar")));
        {
            let mut st = self.state.lock();
            let entry = st.sandboxes.get(&sb.id).ok_or_else(|| RpcError::not_found("a sandbox", &sb.id))?;
            if entry.snapshots.len() as u32 >= self.cfg.sandbox.max_snapshots {
                return Err(RpcError::quota(
                    "snapshots",
                    u64::from(self.cfg.sandbox.max_snapshots),
                    entry.snapshots.len() as u64,
                    format!("limite de snapshots por sandbox atingido ({}); apague algum", self.cfg.sandbox.max_snapshots),
                ));
            }
            if r.persist {
                let u = st.usage(&sb.owner);
                if u.persisted_snapshots >= quota.max_persisted_snapshots {
                    return Err(RpcError::quota(
                        "persisted_snapshots",
                        u64::from(quota.max_persisted_snapshots),
                        u64::from(u.persisted_snapshots),
                        format!("limite de snapshots persistidos do usuário atingido ({})", quota.max_persisted_snapshots),
                    ));
                }
                st.users.entry(sb.owner.clone()).or_default().persisted_snapshots += 1;
            }
        }
        let call = Call::Snapshot {
            sandbox_id: sb.id.clone(),
            snapshot_id: snapshot_id.clone(),
            persist_to: persist_path.as_ref().map(|p| p.display().to_string()),
            max_bytes: self.cfg.service.max_request_bytes.0.max(1 << 30),
        };
        let res = self.call(sb.worker, call, None).await;
        let generation = self.worker_generation(sb.worker);
        let mut st = self.state.lock();
        match res {
            Ok(Reply::Snapshot { persisted_bytes }) => {
                let now = now_unix();
                let Some(entry) = st.sandboxes.get_mut(&sb.id) else {
                    return Err(RpcError::not_found("a sandbox", &sb.id));
                };
                entry.snapshots.push(SnapEntry {
                    id: snapshot_id.clone(),
                    name: name.clone(),
                    created_at: now,
                    generation,
                    persisted: persist_path.zip(persisted_bytes),
                });
                Ok(json!({ "snapshot_id": snapshot_id, "name": name, "created_at": now, "persisted": r.persist, "persisted_bytes": persisted_bytes }))
            }
            Ok(other) => Err(unexpected(&other)),
            Err(e) => {
                if r.persist
                    && let Some(u) = st.users.get_mut(&sb.owner)
                {
                    u.persisted_snapshots = u.persisted_snapshots.saturating_sub(1);
                }
                Err(e)
            }
        }
    }

    fn find_snapshot(&self, p: &Principal, r: &SnapshotRef) -> Result<(SbRef, SnapEntry), RpcError> {
        let sb = self.active(p, &r.sandbox_id)?;
        let st = self.state.lock();
        let snap = st
            .sandboxes
            .get(&sb.id)
            .and_then(|e| e.snapshots.iter().find(|s| s.id == r.snapshot_id).cloned())
            .ok_or_else(|| RpcError::not_found("o snapshot", &r.snapshot_id))?;
        Ok((sb, snap))
    }

    async fn snapshot_delete(self: &Arc<Self>, p: &Principal, r: SnapshotRef) -> Result<Value, RpcError> {
        let (sb, snap) = self.find_snapshot(p, &r)?;
        if snap.generation == self.worker_generation(sb.worker) {
            match self.call(sb.worker, Call::DropSnapshot { sandbox_id: sb.id.clone(), snapshot_id: snap.id.clone() }, None).await {
                Ok(_) => {}
                Err(e) if e.code == codes::NOT_FOUND => {}
                Err(e) => return Err(e),
            }
        }
        {
            let mut st = self.state.lock();
            if let Some(e) = st.sandboxes.get_mut(&sb.id) {
                e.snapshots.retain(|s| s.id != snap.id);
            }
            if snap.persisted.is_some()
                && snap.name != crate::supervisor::AUTOSAVE_NAME
                && let Some(u) = st.users.get_mut(&sb.owner)
            {
                u.persisted_snapshots = u.persisted_snapshots.saturating_sub(1);
            }
        }
        if let Some((path, _)) = snap.persisted {
            let _ = tokio::task::spawn_blocking(move || std::fs::remove_file(path)).await;
        }
        Ok(json!({ "deleted": true }))
    }

    async fn restore(self: &Arc<Self>, p: &Principal, r: SnapshotRef) -> Result<Value, RpcError> {
        let (sb, snap) = self.find_snapshot(p, &r)?;
        if snap.generation == self.worker_generation(sb.worker) {
            self.call(sb.worker, Call::Restore { sandbox_id: sb.id.clone(), snapshot_id: snap.id.clone() }, None).await?;
            return Ok(json!({ "restored": true, "recreated": false }));
        }
        // O snapshot em memória morreu numa queda do worker; só resta o tar persistido: a sandbox é
        // recriada a partir dele (processos e sessões abertos morrem).
        let Some((path, _)) = snap.persisted.clone() else {
            return Err(RpcError::not_found("o snapshot", &snap.id));
        };
        let (spec, sessions) = {
            let st = self.state.lock();
            let e = st.sandboxes.get(&sb.id).ok_or_else(|| RpcError::not_found("a sandbox", &sb.id))?;
            (e.spec(), e.sessions.clone())
        };
        match self.call(sb.worker, Call::DestroySandbox { sandbox_id: sb.id.clone() }, None).await {
            Ok(_) => {}
            Err(e) if e.code == codes::NOT_FOUND => {}
            Err(e) => return Err(e),
        }
        for sid in sessions {
            self.bury_session(&sid, format!("a sessão foi encerrada pelo restore do snapshot {}", snap.id));
        }
        let call = Call::RecoverSandbox {
            sandbox_id: sb.id.clone(),
            user: self.user_sched(&sb.owner),
            spec,
            tar_path: path.display().to_string(),
            max_bytes: self.cfg.service.max_request_bytes.0.max(1 << 30),
        };
        if let Err(e) = self.call(sb.worker, call, None).await {
            let mut st = self.state.lock();
            if let Some(entry) = st.sandboxes.get_mut(&sb.id) {
                entry.status = SbStatus::Lost { reason: format!("o restore do snapshot {} falhou: {}", snap.id, e.message), at: now_unix() };
            }
            st.release(&sb.id);
            return Err(e);
        }
        let mut st = self.state.lock();
        if let Some(entry) = st.sandboxes.get_mut(&sb.id) {
            // Os outros snapshots em memória morreram com a sandbox antiga.
            entry.snapshots.retain(|s| s.persisted.is_some());
        }
        Ok(json!({ "restored": true, "recreated": true }))
    }

    async fn admin(self: &Arc<Self>, p: &Principal, method: &str, prm: Value) -> Result<Value, RpcError> {
        let auth = self.auth.clone();
        let now = now_unix();
        let blocking = |f: Box<dyn FnOnce() -> Result<Value, RpcError> + Send>| async move {
            tokio::task::spawn_blocking(f).await.map_err(|e| RpcError::internal(format!("tarefa de admin: {e}")))?
        };
        match method {
            "admin.workers" => Ok(json!({ "workers": self.workers(), "uptime_secs": self.uptime().as_secs() })),
            "admin.users.list" => {
                let data = self.auth.snapshot().map_err(auth_err)?;
                let st = self.state.lock();
                let users: Vec<Value> = data
                    .users
                    .iter()
                    .map(|u| {
                        json!({
                            "name": u.name, "role": u.role.as_str(), "disabled": u.disabled,
                            "created_at": u.created_at, "quota_override": u.quota,
                            "quota": u.quota.apply(&self.cfg.quota), "usage": st.usage(&u.name),
                            "keys": data.keys.iter().filter(|k| k.user == u.name && k.state(now) == KeyState::Active).count(),
                        })
                    })
                    .collect();
                Ok(json!({ "users": users }))
            }
            "admin.users.create" => {
                let r: AdminUserCreate = params(prm)?;
                let role = Role::parse(r.role.as_deref().unwrap_or("user")).map_err(RpcError::invalid_params)?;
                r.quota.apply(&self.cfg.quota).validate().map_err(RpcError::invalid_params)?;
                blocking(Box::new(move || {
                    let u = auth.create_user(&r.name, role, r.quota, now).map_err(auth_err)?;
                    Ok(json_of(&u))
                }))
                .await
            }
            "admin.users.update" => {
                let r: AdminUserUpdate = params(prm)?;
                let role = r.role.as_deref().map(Role::parse).transpose().map_err(RpcError::invalid_params)?;
                self.guard_last_admin(&r.name, role == Some(Role::User) || r.disabled == Some(true))?;
                if let Some(q) = &r.quota {
                    q.apply(&self.cfg.quota).validate().map_err(RpcError::invalid_params)?;
                }
                let sched_changed = r.quota.as_ref().is_some_and(|q| q.cpu_weight.is_some() || q.cpu_max.is_some()) || r.reset_quota;
                let name = r.name.clone();
                let mut out = blocking(Box::new(move || {
                    let u = auth.update_user(&r.name, role, r.disabled, r.quota.as_ref(), r.reset_quota).map_err(auth_err)?;
                    Ok(json_of(&u))
                }))
                .await?;
                if sched_changed && let Err(e) = self.push_user_sched(&name).await {
                    tracing::warn!(user = %name, "peso/teto de CPU não aplicado ao vivo: {}", e.message);
                    out["sched_warning"] = json!(format!(
                        "a quota foi gravada, mas o grupo de CPU não foi atualizado agora: {}; vale na próxima sandbox",
                        e.message
                    ));
                }
                Ok(out)
            }
            "admin.users.remove" => {
                let r: AdminName = params(prm)?;
                self.guard_last_admin(&r.name, true)?;
                let name = r.name.clone();
                let revoked = blocking(Box::new(move || {
                    let n = auth.remove_user(&r.name, now).map_err(auth_err)?;
                    Ok(json!(n))
                }))
                .await?;
                let ids: Vec<String> =
                    self.state.lock().sandboxes.values().filter(|s| s.owner == name).map(|s| s.id.clone()).collect();
                for id in &ids {
                    let _ = self.destroy_sandbox(id).await;
                }
                Ok(json!({ "removed": true, "keys_revoked": revoked, "sandboxes_destroyed": ids.len() }))
            }
            "admin.keys.create" => {
                let r: AdminKeyCreate = params(prm)?;
                let expires_at =
                    parse_expiry(r.expires.as_deref().unwrap_or("90d"), now).map_err(RpcError::invalid_params)?;
                blocking(Box::new(move || {
                    let k = auth.create_key(&r.user, &r.label, expires_at, now).map_err(auth_err)?;
                    Ok(json!({
                        "key_id": k.record.id, "user": k.record.user, "label": k.record.label, "token": k.token,
                        "created_at": k.record.created_at, "expires_at": k.record.expires_at,
                        "expires_at_utc": k.record.expires_at.map(fmt_utc),
                    }))
                }))
                .await
            }
            "admin.keys.list" => {
                let r: AdminKeyList = params(prm)?;
                let data = self.auth.snapshot().map_err(auth_err)?;
                let keys: Vec<Value> = data
                    .keys
                    .iter()
                    .filter(|k| r.user.as_ref().is_none_or(|u| &k.user == u))
                    .map(|k| {
                        json!({
                            "key_id": k.id, "user": k.user, "label": k.label, "state": k.state(now),
                            "created_at": k.created_at, "expires_at": k.expires_at, "revoked_at": k.revoked_at,
                            "last_used_at": k.last_used_at,
                        })
                    })
                    .collect();
                Ok(json!({ "keys": keys }))
            }
            "admin.keys.revoke" => {
                let r: AdminKeyRevoke = params(prm)?;
                if r.key_id == p.key_id {
                    tracing::warn!(user = %p.user, "admin revogando a própria chave em uso");
                }
                blocking(Box::new(move || {
                    let k = auth.revoke_key(&r.key_id, now).map_err(auth_err)?;
                    Ok(json!({ "key_id": k.id, "revoked_at": k.revoked_at }))
                }))
                .await
            }
            _ => Err(RpcError::method_not_found(method)),
        }
    }

    /// Recusa tirar o último admin ativo (o daemon ficaria sem administração remota).
    fn guard_last_admin(&self, name: &str, demoting: bool) -> Result<(), RpcError> {
        if !demoting {
            return Ok(());
        }
        let data = self.auth.snapshot().map_err(auth_err)?;
        let admins: Vec<&str> =
            data.users.iter().filter(|u| u.role == Role::Admin && !u.disabled).map(|u| u.name.as_str()).collect();
        if admins == [name] {
            return Err(RpcError::invalid_params(
                "esse é o último admin ativo; crie outro antes (ou use o comando local pseudo-linusd admin)",
            ));
        }
        Ok(())
    }

    /// Reaplica peso e teto de CPU do usuário no worker dele (quando a quota muda). Sem sandbox viva
    /// não há grupo a atualizar: o próximo `sandbox.create` já cria com a quota nova.
    async fn push_user_sched(&self, user: &str) -> Result<(), RpcError> {
        let Some(worker) = self.state.lock().user_worker.get(user).copied() else { return Ok(()) };
        self.call(worker, Call::UpdateUser { user: self.user_sched(user) }, None).await.map(|_| ())
    }
}

fn unexpected(r: &Reply) -> RpcError {
    RpcError::internal(format!("resposta inesperada do worker: {r:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_carry_does_not_split_characters() {
        let mut c = Utf8Carry::default();
        let s = "ação".as_bytes();
        let mut out = String::new();
        for b in s.chunks(1) {
            out.push_str(&c.push(b));
        }
        out.push_str(&c.finish());
        assert_eq!(out, "ação");
        let mut c = Utf8Carry::default();
        assert_eq!(c.push(&[b'a', 0xff, b'b']), "a\u{fffd}b");
    }
}
