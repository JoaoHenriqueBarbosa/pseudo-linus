//! Harness mínimo de agente para o pseudo-linus: o laço do prana com o transporte nativo, uma
//! única tool (`osh`) e uma fita de gravação/replay das falas do modelo.

pub mod agent;
pub mod cassette;
pub mod daemon;
pub mod osh_tool;
pub mod sandbox;

use anyhow::Result;

/// Lê `example/.env` (fora do git) uma vez; variável que já está no ambiente ganha do arquivo.
pub fn load_dotenv() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
        let Ok(text) = std::fs::read_to_string(path) else { return };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let (k, v) = (k.trim(), v.trim().trim_matches('"'));
                if std::env::var_os(k).is_none() {
                    std::env::set_var(k, v);
                }
            }
        }
    });
}

/// O que cada cenário recebe: um daemon próprio, um sandbox e a fita com o nome do cenário.
pub struct Scenario {
    pub daemon: daemon::Daemon,
    pub sandbox: sandbox::Sandbox,
    pub cassette: cassette::Cassette,
}

impl Scenario {
    pub async fn start(name: &str) -> Result<Scenario> {
        load_dotenv();
        let daemon = tokio::task::spawn_blocking(daemon::Daemon::start).await??;
        let sandbox = sandbox::Sandbox::create(&daemon.base_url, &daemon.token, "box").await?;
        let cassette = cassette::Cassette::open(name).await?;
        Ok(Scenario { daemon, sandbox, cassette })
    }

    pub async fn agent(&self, channel: &str, prompt: &str) -> Result<agent::Transcript> {
        agent::run(&self.sandbox, &self.cassette, channel, prompt).await
    }

    /// Fecha a fita e mostra as divergências do replay (não são falha fora do modo estrito).
    pub async fn finish(self) -> Result<()> {
        let _ = self.sandbox.destroy().await;
        for d in self.cassette.finish().await? {
            eprintln!("fita: {d}");
        }
        Ok(())
    }
}
