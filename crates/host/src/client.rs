//! Cliente JSON-RPC bloqueante do daemon (usado pelo `osh --remote` e pelos testes).

use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use crate::rpc::{self, RpcError};

/// Maior resposta lida de uma vez (a resposta JSON-RPC inteira, base64 incluído).
const MAX_RESPONSE: u64 = 1 << 30;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// O daemon respondeu com erro JSON-RPC.
    #[error("{} (código {})", .0.message, .0.code)]
    Rpc(RpcError),
    /// Não deu pra falar com o daemon.
    #[error("{0}")]
    Transport(String),
    /// Resposta que não é JSON-RPC.
    #[error("{0}")]
    Protocol(String),
}

impl ClientError {
    pub fn rpc(&self) -> Option<&RpcError> {
        match self {
            ClientError::Rpc(e) => Some(e),
            _ => None,
        }
    }
}

pub struct Client {
    base: String,
    token: String,
    agent: ureq::Agent,
    next_id: AtomicU64,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("base", &self.base).finish_non_exhaustive()
    }
}

impl Client {
    /// `base` é a URL do daemon (`https://pl.exemplo.com` ou `http://127.0.0.1:8080`).
    pub fn new(base: &str, token: &str) -> Client {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(15)))
            .http_status_as_error(false)
            .build()
            .new_agent();
        Client { base: base.trim_end_matches('/').to_string(), token: token.to_string(), agent, next_id: AtomicU64::new(1) }
    }

    fn post(&self, body: &Value) -> Result<ureq::http::Response<ureq::Body>, ClientError> {
        self.agent
            .post(format!("{}/rpc", self.base))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .send(serde_json::to_vec(body).expect("JSON sempre serializa"))
            .map_err(|e| ClientError::Transport(format!("{}: {e}", self.base)))
    }

    fn decode(&self, text: &str) -> Result<Value, ClientError> {
        let v: Value = serde_json::from_str(text).map_err(|e| ClientError::Protocol(format!("resposta não é JSON: {e}")))?;
        rpc::parse_response(v).map_err(ClientError::Rpc)
    }

    /// Chama um método e espera a resposta.
    pub fn call(&self, method: &str, params: Value) -> Result<Value, ClientError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut resp = self.post(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        let text = resp
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE)
            .read_to_string()
            .map_err(|e| ClientError::Transport(format!("lendo a resposta: {e}")))?;
        self.decode(&text)
    }

    /// Chama um método de streaming (`exec.stream`, `session.exec.stream`); `on_output` recebe cada
    /// notificação `exec.output` (`stream`, `data`) na hora em que chega.
    pub fn call_stream(&self, method: &str, params: Value, mut on_output: impl FnMut(&str, &str)) -> Result<Value, ClientError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let resp = self.post(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        let ndjson = resp.headers().get("content-type").is_some_and(|v| v.as_bytes().starts_with(b"application/x-ndjson"));
        let mut reader = BufReader::new(resp.into_body().into_reader());
        if !ndjson {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut reader, &mut text).map_err(|e| ClientError::Transport(e.to_string()))?;
            return self.decode(&text);
        }
        let mut line = String::new();
        loop {
            line.clear();
            let n = reader.read_line(&mut line).map_err(|e| ClientError::Transport(format!("lendo o stream: {e}")))?;
            if n == 0 {
                return Err(ClientError::Protocol("o stream acabou sem a resposta final".into()));
            }
            let v: Value = serde_json::from_str(line.trim_end())
                .map_err(|e| ClientError::Protocol(format!("linha do stream não é JSON: {e}")))?;
            if v.get("method").and_then(Value::as_str) == Some("exec.output") {
                let p = &v["params"];
                on_output(p["stream"].as_str().unwrap_or("stdout"), p["data"].as_str().unwrap_or(""));
                continue;
            }
            return rpc::parse_response(v).map_err(ClientError::Rpc);
        }
    }
}
