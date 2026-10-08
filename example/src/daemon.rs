//! Um `pseudo-linusd` de verdade por teste: diretório de dados temporário, porta livre, token de
//! admin emitido pelo `admin bootstrap` e espera pelo `/healthz`. O processo morre no `Drop`.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

pub struct Daemon {
    child: Child,
    pub base_url: String,
    pub token: String,
    _data_dir: tempfile::TempDir,
}

/// O binário vem de `PL_DAEMON_BIN` ou do `target/debug` do workspace do pseudo-linus.
fn daemon_bin() -> PathBuf {
    if let Ok(p) = std::env::var("PL_DAEMON_BIN") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/pseudo-linusd")
}

fn free_port() -> Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

impl Daemon {
    pub fn start() -> Result<Daemon> {
        let bin = daemon_bin();
        if !bin.exists() {
            bail!("pseudo-linusd não encontrado em {} (cargo build -p host ou PL_DAEMON_BIN)", bin.display());
        }
        let data_dir = tempfile::tempdir()?;
        let out = Command::new(&bin)
            .args(["admin", "--json", "--data-dir"])
            .arg(data_dir.path())
            .args(["bootstrap", "--user", "admin", "--expires", "1d"])
            .output()
            .context("rodando admin bootstrap")?;
        if !out.status.success() {
            bail!("admin bootstrap falhou: {}", String::from_utf8_lossy(&out.stderr));
        }
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).context("saída do bootstrap não é JSON")?;
        let token = v["token"].as_str().context("bootstrap sem token")?.to_string();

        let port = free_port()?;
        let mut cmd = Command::new(&bin);
        cmd.arg("serve")
            .env("PL_LISTEN", format!("127.0.0.1:{port}"))
            .env("PL_DATA_DIR", data_dir.path())
            .env("PL_WORKERS", std::env::var("PL_WORKERS").unwrap_or_else(|_| "2".into()));
        // O `pip install` dos cenários sai do espelho do PyPI com os wheels fixados da bancada
        // (`testbench/mirror/fetch.sh`), não da internet: a fita reproduz igual em qualquer máquina.
        let wheels = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../testbench/mirror/wheels");
        if std::env::var_os("PL_PYPI_MIRROR").is_none() && wheels.is_dir() {
            cmd.env("PL_PYPI_MIRROR", wheels);
        }
        let child = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .context("subindo pseudo-linusd serve")?;
        let mut d = Daemon { child, base_url: format!("http://127.0.0.1:{port}"), token, _data_dir: data_dir };
        d.wait_healthy(Duration::from_secs(30))?;
        Ok(d)
    }

    fn wait_healthy(&mut self, limit: Duration) -> Result<()> {
        let start = Instant::now();
        let url = format!("{}/healthz", self.base_url);
        while start.elapsed() < limit {
            if let Some(status) = self.child.try_wait()? {
                bail!("pseudo-linusd saiu antes de ficar pronto: {status}");
            }
            if healthz_ok(&url) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        bail!("pseudo-linusd não respondeu /healthz em {limit:?}")
    }
}

/// `GET /healthz` com o mesmo cliente HTTP do `host::client`; qualquer resposta que não seja 200
/// (conexão recusada, 503 sem worker pronto) conta como "ainda não".
fn healthz_ok(url: &str) -> bool {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(2)))
        .http_status_as_error(false)
        .build()
        .into();
    agent.get(url).call().is_ok_and(|r| r.status() == 200)
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
