//! A fita: um proxy HTTP local entre o transporte nativo do prana e a API. Gravando, repassa cada
//! requisição à API de verdade e guarda a resposta crua (o corpo SSE inteiro); reproduzindo, devolve
//! as respostas guardadas sem rede e sem credencial. O sandbox continua de verdade nos dois modos:
//! só as falas do modelo vêm da fita, os comandos que ele pede rodam no pseudo-linus.
//!
//! Cada agente ganha um canal (`/<canal>/v1/messages`), com a sua sequência de interações. Agentes
//! simultâneos não embaralham a ordem uns dos outros.
//!
//! A saída do sandbox varia entre execuções (PIDs, portas, datas), então a requisição reproduzida nem
//! sempre é idêntica à gravada. A fita serve pela posição no canal e registra a divergência do hash
//! da requisição normalizada; `PL_CASSETTE_STRICT=1` transforma divergência em falha.
//!
//! Modo por `PL_CASSETTE`: `replay` (sem fita é erro), `record` (sempre a API, regrava) ou `auto`
//! (padrão: reproduz o que existe, grava o que falta).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use axum::body::{Body, Bytes};
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Replay,
    Record,
    Auto,
}

impl Mode {
    pub fn from_env() -> Result<Mode> {
        match std::env::var("PL_CASSETTE").as_deref() {
            Err(_) | Ok("") | Ok("auto") => Ok(Mode::Auto),
            Ok("replay") => Ok(Mode::Replay),
            Ok("record") => Ok(Mode::Record),
            Ok(other) => bail!("PL_CASSETTE inválido: {other} (replay, record ou auto)"),
        }
    }
}

/// Como a fita se comporta e para onde ela repassa quando grava.
#[derive(Clone, Debug)]
pub struct Options {
    pub mode: Mode,
    /// Divergência da requisição vira falha em vez de aviso.
    pub strict: bool,
    /// A API de verdade.
    pub upstream: String,
    pub api_key: Option<String>,
}

impl Options {
    /// `PL_CASSETTE`, `PL_CASSETTE_STRICT`, `PL_UPSTREAM_BASE_URL` (ou `ANTHROPIC_BASE_URL`) e
    /// `ANTHROPIC_API_KEY`.
    pub fn from_env() -> Result<Options> {
        Ok(Options {
            mode: Mode::from_env()?,
            strict: std::env::var("PL_CASSETTE_STRICT").is_ok_and(|v| v == "1"),
            upstream: std::env::var("PL_UPSTREAM_BASE_URL")
                .or_else(|_| std::env::var("ANTHROPIC_BASE_URL"))
                .unwrap_or_else(|_| "https://api.anthropic.com".into()),
            api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Interaction {
    request_hash: String,
    request: Value,
    status: u16,
    content_type: String,
    body: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Tape {
    channels: HashMap<String, Vec<Interaction>>,
}

struct Channel {
    recorded: Vec<Interaction>,
    next: usize,
    played: Vec<Interaction>,
}

struct Shared {
    mode: Mode,
    strict: bool,
    upstream: String,
    api_key: Option<String>,
    http: reqwest::Client,
    channels: Mutex<HashMap<String, Channel>>,
    divergences: Mutex<Vec<String>>,
    went_live: Mutex<bool>,
    path: PathBuf,
}

/// Grava o que já foi tocado em todos os canais. Roda a cada interação vinda da API, para que um
/// teste que entra em pânico (e nunca chega ao `finish`) deixe a fita da conversa que falhou.
fn persist(path: &Path, channels: &HashMap<String, Channel>) -> Result<()> {
    let tape = Tape { channels: channels.iter().map(|(k, c)| (k.clone(), c.played.clone())).collect() };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(&tape)?)?;
    Ok(())
}

pub struct Cassette {
    path: PathBuf,
    shared: Arc<Shared>,
    addr: std::net::SocketAddr,
    server: tokio::task::JoinHandle<()>,
}

impl Cassette {
    /// Abre a fita `cassettes/<name>.json` do crate e sobe o proxy numa porta livre.
    pub async fn open(name: &str) -> Result<Cassette> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("cassettes").join(format!("{name}.json"));
        Self::open_at(&path, Options::from_env()?).await
    }

    pub async fn open_at(path: &Path, o: Options) -> Result<Cassette> {
        let mode = o.mode;
        let tape: Tape = if mode != Mode::Record && path.exists() {
            serde_json::from_slice(&std::fs::read(path)?).with_context(|| format!("lendo {}", path.display()))?
        } else {
            Tape::default()
        };
        if mode == Mode::Replay && tape.channels.is_empty() {
            bail!("PL_CASSETTE=replay e não há fita em {}", path.display());
        }
        let channels = tape
            .channels
            .into_iter()
            .map(|(k, recorded)| (k, Channel { recorded, next: 0, played: Vec::new() }))
            .collect();
        let shared = Arc::new(Shared {
            mode,
            strict: o.strict,
            upstream: o.upstream.trim_end_matches('/').to_string(),
            api_key: o.api_key.filter(|k| !k.is_empty()),
            http: reqwest::Client::new(),
            channels: Mutex::new(channels),
            divergences: Mutex::new(Vec::new()),
            went_live: Mutex::new(false),
            path: path.to_path_buf(),
        });
        let app = Router::new().route("/{channel}/{*rest}", post(handle)).with_state(shared.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Cassette { path: path.to_path_buf(), shared, addr, server })
    }

    /// A `ANTHROPIC_BASE_URL` que o agente do canal `channel` deve usar.
    pub fn base_url(&self, channel: &str) -> String {
        format!("http://{}/{channel}", self.addr)
    }

    /// A chave que o transporte nativo recebe: a de verdade quando há, um marcador em replay.
    pub fn api_key(&self) -> String {
        self.shared.api_key.clone().unwrap_or_else(|| "cassette-replay".into())
    }

    /// Fecha a fita: grava o que foi tocado (se algo veio da API) e devolve as divergências.
    pub async fn finish(self) -> Result<Vec<String>> {
        self.server.abort();
        let live = *self.shared.went_live.lock().await;
        let channels = self.shared.channels.lock().await;
        if live {
            persist(&self.path, &channels)?;
        }
        let mut out = self.shared.divergences.lock().await.clone();
        for (name, c) in channels.iter() {
            if !live && c.next < c.recorded.len() {
                out.push(format!("canal {name}: {} interações gravadas não foram tocadas", c.recorded.len() - c.next));
            }
        }
        Ok(out)
    }
}

/// O corpo sem o que muda de uma execução para outra e não é conversa: `metadata` (`user_id` com o
/// id de sessão) não entra no hash.
fn normalize(body: &Value) -> Value {
    let mut v = body.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("metadata");
    }
    v
}

fn hash(v: &Value) -> String {
    let mut h = Sha256::new();
    h.update(serde_json::to_vec(v).unwrap_or_default());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

async fn handle(
    State(s): State<Arc<Shared>>,
    UrlPath((channel, rest)): UrlPath<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match serve(&s, &channel, &rest, &headers, body).await {
        Ok(r) => r,
        Err(e) => (StatusCode::BAD_GATEWAY, format!("fita: {e:#}")).into_response(),
    }
}

async fn serve(s: &Shared, channel: &str, rest: &str, headers: &HeaderMap, body: Bytes) -> Result<Response> {
    let request: Value = serde_json::from_slice(&body).context("requisição não é JSON")?;
    let normalized = normalize(&request);
    let request_hash = hash(&normalized);

    // Replay: a próxima interação do canal, se existir.
    if s.mode != Mode::Record {
        let mut channels = s.channels.lock().await;
        let c = channels
            .entry(channel.to_string())
            .or_insert_with(|| Channel { recorded: Vec::new(), next: 0, played: Vec::new() });
        if let Some(it) = c.recorded.get(c.next).cloned() {
            c.next += 1;
            if it.request_hash != request_hash {
                let at = first_difference(&it.request, &normalized, "$").unwrap_or_default();
                let msg = format!("canal {channel}, interação {}: a requisição divergiu da gravada em {at}", c.next);
                if s.strict {
                    bail!("{msg}");
                }
                s.divergences.lock().await.push(msg);
            }
            c.played.push(it.clone());
            return Ok(respond(it.status, &it.content_type, it.body));
        }
        if s.mode == Mode::Replay {
            bail!("canal {channel}: a fita acabou na interação {} (regrave com PL_CASSETTE=record)", c.next + 1);
        }
    }

    // Gravação: repassa à API de verdade.
    let key = s.api_key.clone().context("gravar a fita precisa de ANTHROPIC_API_KEY")?;
    let mut req = s.http.post(format!("{}/{rest}", s.upstream)).body(body.clone());
    for (name, value) in headers {
        let n = name.as_str();
        if matches!(n, "host" | "content-length" | "x-api-key" | "authorization" | "accept-encoding") {
            continue;
        }
        req = req.header(name, value);
    }
    req = req.header("x-api-key", &key);
    let resp = req.send().await.context("repassando à API")?;
    let status = resp.status().as_u16();
    let content_type =
        resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("application/json").to_string();
    let text = resp.text().await.context("lendo a resposta da API")?;
    *s.went_live.lock().await = true;
    let it = Interaction { request_hash, request: normalized, status, content_type: content_type.clone(), body: text.clone() };
    // Só a resposta boa entra na fita: erro transitório (429, 529) não deve virar replay.
    if (200..300).contains(&status) {
        let mut channels = s.channels.lock().await;
        let c = channels
            .entry(channel.to_string())
            .or_insert_with(|| Channel { recorded: Vec::new(), next: 0, played: Vec::new() });
        c.played.push(it);
        persist(&s.path, &channels)?;
    }
    Ok(respond(status, &content_type, text))
}

/// O primeiro caminho em que `a` e `b` diferem, com os dois valores (cortados) quando são folhas.
fn first_difference(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            keys.into_iter().find_map(|k| {
                first_difference(x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null), &format!("{path}.{k}"))
            })
        }
        (Value::Array(x), Value::Array(y)) => {
            (0..x.len().max(y.len())).find_map(|i| {
                first_difference(x.get(i).unwrap_or(&Value::Null), y.get(i).unwrap_or(&Value::Null), &format!("{path}[{i}]"))
            })
        }
        _ if a == b => None,
        _ => {
            let cut = |v: &Value| v.to_string().chars().take(120).collect::<String>();
            Some(format!("{path} (gravado {} / agora {})", cut(a), cut(b)))
        }
    }
}

fn respond(status: u16, content_type: &str, body: String) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", content_type)
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
