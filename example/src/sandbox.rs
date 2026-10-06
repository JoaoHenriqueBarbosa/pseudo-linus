//! Um sandbox do daemon visto pelo harness: sessões nomeadas (abertas sob demanda, cada uma com o
//! seu shell, cwd e jobs), snapshots e restauração. O `host::client::Client` é bloqueante, então
//! toda chamada passa por `spawn_blocking`.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use host::client::Client;
use serde_json::{json, Value};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Sandbox {
    inner: Arc<Inner>,
}

struct Inner {
    client: Client,
    id: String,
    sessions: Mutex<HashMap<String, String>>,
}

/// O que o `session.exec` devolve, reduzido ao que o agente e as asserções usam.
#[derive(Clone, Debug)]
pub struct ExecOutput {
    pub exit_code: Option<i64>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub session_reset: bool,
    pub cwd: Option<String>,
}

impl Sandbox {
    pub async fn create(base_url: &str, token: &str, hostname: &str) -> Result<Sandbox> {
        let client = Client::new(base_url, token);
        let hostname = hostname.to_string();
        let (client, v) = blocking(move || {
            let v = client.call("sandbox.create", json!({ "hostname": hostname }));
            (client, v)
        })
        .await?;
        let v = v.map_err(|e| anyhow!("sandbox.create: {e}"))?;
        let id = v["sandbox_id"].as_str().context("sandbox.create sem sandbox_id")?.to_string();
        Ok(Sandbox { inner: Arc::new(Inner { client, id, sessions: Mutex::new(HashMap::new()) }) })
    }

    pub fn id(&self) -> &str {
        &self.inner.id
    }

    async fn call(&self, method: &'static str, params: Value) -> Result<Value> {
        let inner = self.inner.clone();
        blocking(move || inner.client.call(method, params)).await?.map_err(|e| anyhow!("{method}: {e}"))
    }

    /// O `session_id` da sessão `name`, abrindo-a na primeira vez.
    async fn session(&self, name: &str) -> Result<String> {
        let mut sessions = self.inner.sessions.lock().await;
        if let Some(id) = sessions.get(name) {
            return Ok(id.clone());
        }
        let v = self.call("session.open", json!({ "sandbox_id": self.inner.id })).await?;
        let id = v["session_id"].as_str().context("session.open sem session_id")?.to_string();
        sessions.insert(name.to_string(), id.clone());
        Ok(id)
    }

    pub async fn exec(&self, session: &str, command: &str, timeout_ms: Option<u64>) -> Result<ExecOutput> {
        let sid = self.session(session).await?;
        let mut params = json!({ "session_id": sid, "command": command });
        if let Some(t) = timeout_ms {
            params["timeout_ms"] = json!(t);
        }
        let v = self.call("session.exec", params).await?;
        Ok(ExecOutput {
            exit_code: v["exit_code"].as_i64(),
            stdout: v["stdout"].as_str().unwrap_or_default().to_string(),
            stderr: v["stderr"].as_str().unwrap_or_default().to_string(),
            timed_out: v["timed_out"].as_bool().unwrap_or(false),
            session_reset: v["session_reset"].as_bool().unwrap_or(false),
            cwd: v["cwd"].as_str().map(str::to_string),
        })
    }

    pub async fn snapshot(&self, name: &str) -> Result<String> {
        let v = self.call("snapshot.create", json!({ "sandbox_id": self.inner.id, "name": name })).await?;
        Ok(v["snapshot_id"].as_str().context("snapshot.create sem snapshot_id")?.to_string())
    }

    /// Restaura o snapshot. Quando o daemon precisou recriar a sandbox a partir do tar persistido,
    /// as sessões morreram junto e o harness as esquece: a próxima chamada abre outras.
    pub async fn restore(&self, snapshot_id: &str) -> Result<()> {
        let v = self.call("snapshot.restore", json!({ "sandbox_id": self.inner.id, "snapshot_id": snapshot_id })).await?;
        if v["recreated"].as_bool().unwrap_or(false) {
            self.inner.sessions.lock().await.clear();
        }
        Ok(())
    }

    pub async fn read_file(&self, path: &str) -> Result<String> {
        let v = self.call("fs.read", json!({ "sandbox_id": self.inner.id, "path": path })).await?;
        Ok(v["data"].as_str().unwrap_or_default().to_string())
    }

    pub async fn destroy(&self) -> Result<()> {
        self.call("sandbox.destroy", json!({ "sandbox_id": self.inner.id })).await.map(|_| ())
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| anyhow!("tarefa bloqueante: {e}"))
}
