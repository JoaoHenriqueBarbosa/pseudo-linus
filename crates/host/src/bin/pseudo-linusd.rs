//! `pseudo-linusd`: o daemon multiusuário do pseudo-linus.
//!
//! - `serve`: supervisor + workers + HTTP (JSON-RPC).
//! - `admin`: usuários e chaves de API, direto no diretório de dados (vale com o daemon rodando).
//! - `healthcheck`: pro `HEALTHCHECK` da imagem.
//! - `worker`: interno; o supervisor sobe os workers com ele.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use host::auth::{AuthStore, KeyState, Role};
use host::config::{ByteSize, Config, CpuMax, QuotaOverride};
use host::supervisor::{Supervisor, WorkerCommand};
use host::timeutil::{fmt_utc, now_unix, parse_duration, parse_expiry};

/// Allocator global rastreado (E07): a tabela de grupos é do kernel, que a instala no worker.
#[global_allocator]
static GLOBAL: tracking_allocator::Allocator<mimalloc::MiMalloc> =
    tracking_allocator::Allocator::from_allocator(mimalloc::MiMalloc);

#[derive(Parser)]
#[command(name = "pseudo-linusd", version, about = "Daemon multiusuário do pseudo-linus (JSON-RPC sobre HTTP)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

// Um valor por execução do programa; o tamanho da variante não importa.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Cmd {
    /// Sobe o supervisor, os workers e o servidor HTTP.
    Serve {
        /// Arquivo TOML de configuração.
        #[arg(long, env = "PL_CONFIG")]
        config: Option<PathBuf>,
    },
    /// Administração local de usuários e chaves (no diretório de dados).
    Admin(AdminArgs),
    /// Consulta o /healthz e sai com 0 se o daemon está atendendo.
    Healthcheck {
        #[arg(long, env = "PL_HEALTH_URL", default_value = "http://127.0.0.1:8080/healthz")]
        url: String,
    },
    /// Mostra a configuração efetiva (arquivo + ambiente) em TOML.
    PrintConfig {
        #[arg(long, env = "PL_CONFIG")]
        config: Option<PathBuf>,
    },
    /// Confere se Landlock e seccomp valem neste host (rode dentro do container, na VPS, antes de
    /// abrir pra usuários). Sai com 0 se o isolamento bate com a configuração.
    Selftest {
        #[arg(long, env = "PL_CONFIG")]
        config: Option<PathBuf>,
    },
    /// Interno: processo worker (o supervisor passa a configuração em PL_WORKER_CONFIG).
    #[command(hide = true)]
    Worker {
        #[arg(long)]
        index: usize,
    },
}

#[derive(Args)]
struct AdminArgs {
    #[arg(long, env = "PL_CONFIG")]
    config: Option<PathBuf>,
    /// Diretório de dados (sobrescreve o da configuração).
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Saída em JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: AdminCmd,
}

#[derive(Subcommand)]
enum AdminCmd {
    /// Cria o primeiro admin (se não existir) e uma chave pra ele. Imprime o token uma vez só.
    Bootstrap {
        #[arg(long, default_value = "admin")]
        user: String,
        #[arg(long, default_value = "90d")]
        expires: String,
        #[arg(long, default_value = "bootstrap")]
        label: String,
    },
    #[command(subcommand)]
    User(UserCmd),
    #[command(subcommand)]
    Key(KeyCmd),
}

#[derive(Args, Default)]
struct QuotaArgs {
    /// Sandboxes simultâneas.
    #[arg(long)]
    max_sandboxes: Option<u32>,
    /// Soma da memória das sandboxes (ex.: 2GiB).
    #[arg(long)]
    mem: Option<String>,
    /// Soma dos processos das sandboxes.
    #[arg(long)]
    max_procs: Option<u32>,
    /// Peso de CPU do usuário (1 a 10000).
    #[arg(long)]
    cpu_weight: Option<u32>,
    /// Teto de CPU: `QUOTA_US PERIOD_US` (ex.: "100000 100000" = 1 CPU) ou `max`.
    #[arg(long)]
    cpu_max: Option<String>,
    /// exec simultâneos.
    #[arg(long)]
    max_execs: Option<u32>,
    /// Sessões abertas.
    #[arg(long)]
    max_sessions: Option<u32>,
    /// Maior timeout de exec (ex.: 30m).
    #[arg(long)]
    max_timeout: Option<String>,
    /// Maior limite de saída por fluxo (ex.: 16MiB).
    #[arg(long)]
    max_output: Option<String>,
    /// Snapshots persistidos.
    #[arg(long)]
    max_persisted: Option<u32>,
}

impl QuotaArgs {
    fn to_override(&self) -> Result<QuotaOverride, String> {
        let cpu_max = match self.cpu_max.as_deref() {
            None => None,
            Some("max") => Some(None),
            Some(s) => {
                let mut it = s.split_whitespace();
                let q = it.next().and_then(|x| x.parse().ok());
                let p = it.next().and_then(|x| x.parse().ok());
                match (q, p, it.next()) {
                    (Some(quota_us), Some(period_us), None) => {
                        let c = CpuMax { quota_us, period_us };
                        c.validate()?;
                        Some(Some(c))
                    }
                    _ => return Err(format!("--cpu-max inválido: {s:?} (use \"QUOTA_US PERIOD_US\" ou max)")),
                }
            }
        };
        Ok(QuotaOverride {
            max_sandboxes: self.max_sandboxes,
            mem_bytes: self.mem.as_deref().map(ByteSize::parse).transpose()?,
            max_procs: self.max_procs,
            cpu_weight: self.cpu_weight,
            cpu_max,
            max_concurrent_execs: self.max_execs,
            max_sessions: self.max_sessions,
            max_timeout_ms: self.max_timeout.as_deref().map(parse_duration).transpose()?.map(|d| d.as_millis() as u64),
            max_output_bytes: self.max_output.as_deref().map(ByteSize::parse).transpose()?,
            max_persisted_snapshots: self.max_persisted,
        })
    }
}

#[derive(Subcommand)]
enum UserCmd {
    /// Cria um usuário.
    Add {
        name: String,
        #[arg(long, default_value = "user")]
        role: String,
        #[command(flatten)]
        quota: QuotaArgs,
    },
    /// Lista os usuários.
    List,
    /// Muda papel, estado ou quota.
    Update {
        name: String,
        #[arg(long)]
        role: Option<String>,
        #[arg(long, conflicts_with = "enable")]
        disable: bool,
        #[arg(long)]
        enable: bool,
        /// Volta a quota pro padrão da configuração (antes de aplicar as opções desta chamada).
        #[arg(long)]
        reset_quota: bool,
        #[command(flatten)]
        quota: QuotaArgs,
    },
    /// Remove o usuário e revoga as chaves dele (o daemon destrói as sandboxes dele na próxima faxina).
    Remove { name: String },
}

#[derive(Subcommand)]
enum KeyCmd {
    /// Cria uma chave. O token aparece uma vez só.
    Create {
        user: String,
        #[arg(long, default_value = "")]
        label: String,
        /// `90d`, `12h`, `never` ou data `AAAA-MM-DD` (UTC).
        #[arg(long, default_value = "90d")]
        expires: String,
    },
    /// Lista chaves (sem segredo).
    List {
        #[arg(long)]
        user: Option<String>,
    },
    /// Revoga uma chave pelo id.
    Revoke { key_id: String },
    /// Apaga registros de chaves revogadas ou expiradas há mais de `--older-than`.
    Prune {
        #[arg(long, default_value = "30d")]
        older_than: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Serve { config } => serve(config),
        Cmd::Worker { index } => worker(index),
        Cmd::Admin(a) => match admin(a) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("pseudo-linusd admin: {e}");
                ExitCode::FAILURE
            }
        },
        Cmd::Healthcheck { url } => healthcheck(&url),
        Cmd::Selftest { config } => selftest(config),
        Cmd::PrintConfig { config } => match Config::load(config.as_deref()) {
            Ok(c) => {
                print!("{}", toml::to_string_pretty(&c).unwrap_or_default());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("pseudo-linusd: {e}");
                ExitCode::FAILURE
            }
        },
    }
}

fn serve(config: Option<PathBuf>) -> ExitCode {
    host::logging::init("supervisor");
    let cfg = match Config::load(config.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("configuração: {e}");
            return ExitCode::from(78);
        }
    };
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().thread_name("supervisor").build() {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!("runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(serve_async(cfg)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve_async(cfg: Config) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    if !cfg.data_dir.exists() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&cfg.data_dir)
            .map_err(|e| format!("{}: {}", cfg.data_dir.display(), host::config::io_msg(&e)))?;
    }
    let auth = Arc::new(AuthStore::open(cfg.auth_path()).map_err(|e| e.to_string())?);
    let data = auth.snapshot().map_err(|e| e.to_string())?;
    if !data.users.iter().any(|u| u.role == Role::Admin && !u.disabled) {
        tracing::warn!("nenhum admin cadastrado: crie a primeira chave com `pseudo-linusd admin bootstrap`");
    }
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let cfg_json = serde_json::to_string(&cfg).map_err(|e| e.to_string())?;
    let worker_cmd = WorkerCommand {
        program: exe,
        args: vec!["worker".into()],
        env: vec![
            ("PL_WORKER_CONFIG".into(), cfg_json),
            ("PL_SUPERVISOR_PID".into(), std::process::id().to_string()),
        ],
    };
    let addr = cfg.listen_addr();
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| format!("bind {addr}: {}", host::config::io_msg(&e)))?;
    let startup = Duration::from_millis(cfg.worker.startup_timeout_ms);
    let workers = cfg.workers;
    let sup = Supervisor::start(cfg, auth, worker_cmd);
    let ready = sup.wait_ready(startup).await;
    tracing::info!(%addr, ready, workers, "pseudo-linusd atendendo");
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let server = tokio::spawn(host::server::serve(sup.clone(), listener, stop_rx, Duration::from_secs(15)));
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).map_err(|e| e.to_string())?;
    let mut int = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).map_err(|e| e.to_string())?;
    tokio::select! {
        _ = term.recv() => tracing::info!("SIGTERM: desligando"),
        _ = int.recv() => tracing::info!("SIGINT: desligando"),
    }
    let _ = stop_tx.send(true);
    sup.shutdown(Duration::from_secs(20)).await;
    let _ = tokio::time::timeout(Duration::from_secs(20), server).await;
    tracing::info!("pseudo-linusd parado");
    Ok(())
}

fn worker(index: usize) -> ExitCode {
    host::logging::init("worker");
    // Morre junto com o supervisor, mesmo se ele levar SIGKILL. Se o supervisor já morreu antes do
    // prctl, o pai agora é outro processo: confere contra o pid que ele deixou no ambiente (num
    // container o supervisor é o PID 1, então "pai == 1" não serve de sinal).
    let _ = rustix::process::set_parent_process_death_signal(Some(rustix::process::Signal::KILL));
    // SIGTERM/SIGINT chegam a todo o grupo (systemctl stop, Ctrl-C): o worker não morre com eles, senão
    // o autosave final do supervisor falharia. Ele sai pelo `Shutdown` do supervisor ou quando o pai morre.
    let ignored = Arc::new(AtomicBool::new(false));
    for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        let _ = signal_hook::flag::register(sig, ignored.clone());
    }
    let expected = std::env::var("PL_SUPERVISOR_PID").ok().and_then(|p| p.parse::<i32>().ok());
    let parent = rustix::process::getppid().map(|p| p.as_raw_nonzero().get());
    if expected.is_none() || parent != expected {
        tracing::error!(worker = index, ?expected, ?parent, "o worker só roda como filho do supervisor (pseudo-linusd serve)");
        return ExitCode::FAILURE;
    }
    let cfg: Config = match std::env::var("PL_WORKER_CONFIG").map_err(|e| e.to_string()).and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string())) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(worker = index, "PL_WORKER_CONFIG inválido: {e}");
            return ExitCode::from(78);
        }
    };
    // O protocolo sai num fd próprio; o fd 1 passa a apontar pro stderr, então um print perdido de
    // alguma dependência vai pro log em vez de corromper os quadros.
    let proto = match rustix::io::dup(std::io::stdout()) {
        Ok(fd) => fd,
        Err(e) => {
            tracing::error!(worker = index, "dup do stdout: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = rustix::stdio::dup2_stdout(std::io::stderr()) {
        tracing::error!(worker = index, "dup2 do stderr: {e}");
        return ExitCode::FAILURE;
    }
    let backend = match host::worker::backend_from_config(&cfg, index) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(worker = index, "{e}");
            return ExitCode::from(78);
        }
    };
    let info = backend.info();
    tracing::info!(worker = index, backend = %info.name, cpus = info.cpus, isolation = %info.isolation, "worker subindo");
    let w = host::worker::Worker::new(backend, Box::new(std::fs::File::from(proto)));
    match w.serve(std::io::stdin().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(worker = index, "{e}");
            ExitCode::FAILURE
        }
    }
}

fn selftest(config: Option<PathBuf>) -> ExitCode {
    let cfg = match Config::load(config.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("pseudo-linusd: {e}");
            return ExitCode::from(78);
        }
    };
    let dir = std::env::temp_dir().join(format!("pl-selftest-{}", std::process::id()));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("pseudo-linusd selftest: {}: {e}", dir.display());
        return ExitCode::FAILURE;
    }
    let r = host::isolation::selftest(&cfg.isolation, &dir);
    let _ = std::fs::remove_dir_all(&dir);
    match r {
        Ok(t) => {
            println!("{}", serde_json::to_string_pretty(&t).unwrap_or_default());
            if t.ok {
                eprintln!("isolamento ok: {}", t.applied.summary());
                ExitCode::SUCCESS
            } else {
                eprintln!("isolamento com problema: {}", t.problems.join("; "));
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("pseudo-linusd selftest: {e}");
            ExitCode::FAILURE
        }
    }
}

fn healthcheck(url: &str) -> ExitCode {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .http_status_as_error(false)
        .build()
        .new_agent();
    match agent.get(url).call() {
        Ok(r) if r.status() == 200 => ExitCode::SUCCESS,
        Ok(r) => {
            eprintln!("healthcheck: HTTP {}", r.status());
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("healthcheck: {e}");
            ExitCode::FAILURE
        }
    }
}

fn admin(a: AdminArgs) -> Result<(), String> {
    let mut cfg = Config::load(a.config.as_deref())?;
    if let Some(d) = a.data_dir {
        cfg.data_dir = d;
    }
    let store = AuthStore::open(cfg.auth_path()).map_err(|e| e.to_string())?;
    let now = now_unix();
    let out = |v: serde_json::Value, human: String| {
        if a.json {
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        } else {
            println!("{human}");
        }
    };
    let fmt_opt = |t: Option<u64>| t.map(fmt_utc).unwrap_or_else(|| "-".into());
    match a.cmd {
        AdminCmd::Bootstrap { user, expires, label } => {
            let expires_at = parse_expiry(&expires, now)?;
            let data = store.snapshot().map_err(|e| e.to_string())?;
            match data.user(&user) {
                Some(u) if u.role != Role::Admin => {
                    return Err(format!("o usuário {user} já existe e não é admin"));
                }
                Some(_) => {}
                None => {
                    store.create_user(&user, Role::Admin, QuotaOverride::default(), now).map_err(|e| e.to_string())?;
                }
            }
            let k = store.create_key(&user, &label, expires_at, now).map_err(|e| e.to_string())?;
            out(
                serde_json::json!({ "user": user, "key_id": k.record.id, "token": k.token, "expires_at": k.record.expires_at }),
                format!(
                    "admin: {user}\nchave: {} (expira em {})\ntoken (guarde agora, ele não aparece de novo):\n{}",
                    k.record.id,
                    fmt_opt(k.record.expires_at),
                    k.token
                ),
            );
        }
        AdminCmd::User(UserCmd::Add { name, role, quota }) => {
            let role = Role::parse(&role)?;
            let q = quota.to_override()?;
            q.apply(&cfg.quota).validate()?;
            let u = store.create_user(&name, role, q, now).map_err(|e| e.to_string())?;
            out(serde_json::to_value(&u).unwrap_or_default(), format!("usuário {} criado ({})", u.name, u.role.as_str()));
        }
        AdminCmd::User(UserCmd::List) => {
            let d = store.snapshot().map_err(|e| e.to_string())?;
            let mut lines = vec![format!("{:<20} {:<6} {:<9} {:>6}  quota", "usuário", "papel", "estado", "chaves")];
            for u in &d.users {
                let keys = d.keys.iter().filter(|k| k.user == u.name && k.state(now) == KeyState::Active).count();
                let q = u.quota.apply(&cfg.quota);
                lines.push(format!(
                    "{:<20} {:<6} {:<9} {:>6}  sandboxes={} mem={} procs={} cpu.weight={} cpu.max={}",
                    u.name,
                    u.role.as_str(),
                    if u.disabled { "desativ." } else { "ativo" },
                    keys,
                    q.max_sandboxes,
                    q.mem_bytes,
                    q.max_procs,
                    q.cpu_weight,
                    q.cpu_max.map_or("max".to_string(), |c| format!("{} {}", c.quota_us, c.period_us)),
                ));
            }
            out(serde_json::to_value(&d.users).unwrap_or_default(), lines.join("\n"));
        }
        AdminCmd::User(UserCmd::Update { name, role, disable, enable, reset_quota, quota }) => {
            let role = role.as_deref().map(Role::parse).transpose()?;
            let disabled = if disable { Some(true) } else if enable { Some(false) } else { None };
            let q = quota.to_override()?;
            let base = if reset_quota { QuotaOverride::default() } else { store.snapshot().map_err(|e| e.to_string())?.user(&name).map(|u| u.quota.clone()).unwrap_or_default() };
            let mut merged = base;
            merged.merge(&q);
            merged.apply(&cfg.quota).validate()?;
            let u = store.update_user(&name, role, disabled, Some(&q), reset_quota).map_err(|e| e.to_string())?;
            out(serde_json::to_value(&u).unwrap_or_default(), format!("usuário {} atualizado", u.name));
        }
        AdminCmd::User(UserCmd::Remove { name }) => {
            let n = store.remove_user(&name, now).map_err(|e| e.to_string())?;
            out(serde_json::json!({ "removed": name, "keys_revoked": n }), format!("usuário {name} removido; {n} chave(s) revogada(s)"));
        }
        AdminCmd::Key(KeyCmd::Create { user, label, expires }) => {
            let expires_at = parse_expiry(&expires, now)?;
            let k = store.create_key(&user, &label, expires_at, now).map_err(|e| e.to_string())?;
            out(
                serde_json::json!({ "key_id": k.record.id, "user": user, "token": k.token, "expires_at": k.record.expires_at }),
                format!(
                    "chave {} de {user} (expira em {})\ntoken (guarde agora, ele não aparece de novo):\n{}",
                    k.record.id,
                    fmt_opt(k.record.expires_at),
                    k.token
                ),
            );
        }
        AdminCmd::Key(KeyCmd::List { user }) => {
            let d = store.snapshot().map_err(|e| e.to_string())?;
            let keys: Vec<_> = d.keys.iter().filter(|k| user.as_ref().is_none_or(|u| &k.user == u)).collect();
            let mut lines =
                vec![format!("{:<16} {:<16} {:<8} {:<20} {:<20} {:<20} rótulo", "id", "usuário", "estado", "criada", "expira", "último uso")];
            for k in &keys {
                let state = match k.state(now) {
                    KeyState::Active => "ativa",
                    KeyState::Expired => "expirada",
                    KeyState::Revoked => "revogada",
                };
                lines.push(format!(
                    "{:<16} {:<16} {:<8} {:<20} {:<20} {:<20} {}",
                    k.id,
                    k.user,
                    state,
                    fmt_utc(k.created_at),
                    fmt_opt(k.expires_at),
                    fmt_opt(k.last_used_at),
                    k.label
                ));
            }
            let v: Vec<serde_json::Value> = keys
                .iter()
                .map(|k| {
                    serde_json::json!({
                        "key_id": k.id, "user": k.user, "label": k.label, "state": k.state(now),
                        "created_at": k.created_at, "expires_at": k.expires_at, "revoked_at": k.revoked_at,
                        "last_used_at": k.last_used_at,
                    })
                })
                .collect();
            out(serde_json::Value::Array(v), lines.join("\n"));
        }
        AdminCmd::Key(KeyCmd::Revoke { key_id }) => {
            let k = store.revoke_key(&key_id, now).map_err(|e| e.to_string())?;
            out(serde_json::json!({ "key_id": k.id, "revoked_at": k.revoked_at }), format!("chave {} revogada", k.id));
        }
        AdminCmd::Key(KeyCmd::Prune { older_than }) => {
            let d = parse_duration(&older_than)?;
            let n = store.prune_keys(now, d.as_secs()).map_err(|e| e.to_string())?;
            out(serde_json::json!({ "pruned": n }), format!("{n} registro(s) de chave apagado(s)"));
        }
    }
    Ok(())
}
