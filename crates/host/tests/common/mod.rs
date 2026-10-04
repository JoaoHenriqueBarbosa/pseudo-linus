//! Arnês dos testes de integração: sobe o `pseudo-linusd` de verdade (supervisor e workers em processos
//! separados) num diretório temporário, com o backend pedido, e fala com ele por HTTP.

#![allow(dead_code)]

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use host::client::{Client, ClientError};
use serde_json::{Value, json};

pub const BIN: &str = env!("CARGO_BIN_EXE_pseudo-linusd");

pub struct Daemon {
    child: Child,
    pub url: String,
    pub dir: tempfile::TempDir,
    pub admin_token: String,
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

pub fn admin_cli(data: &Path, args: &[&str]) -> Value {
    let out = Command::new(BIN)
        .arg("admin")
        .arg("--data-dir")
        .arg(data)
        .arg("--json")
        .args(args)
        .env_remove("PL_CONFIG")
        .output()
        .unwrap();
    assert!(out.status.success(), "admin {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

impl Daemon {
    pub fn fake(extra: &str) -> Daemon {
        Daemon::start("fake", &[], extra)
    }

    pub fn fake_with(worker: &[(&str, u64)], extra: &str) -> Daemon {
        Daemon::start("fake", worker, extra)
    }

    pub fn kernel(extra: &str) -> Daemon {
        Daemon::start("kernel", &[], extra)
    }

    /// `worker`: chaves da seção `[worker]` que trocam os padrões do teste.
    pub fn start(backend: &str, worker: &[(&str, u64)], extra: &str) -> Daemon {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let boot = admin_cli(&data, &["bootstrap", "--expires", "1d"]);
        let admin_token = boot["token"].as_str().unwrap().to_string();
        let port = free_port();
        let mut w: std::collections::BTreeMap<&str, u64> =
            [("restart_backoff_min_ms", 50), ("ping_interval_ms", 500), ("ping_timeout_ms", 5000)].into_iter().collect();
        w.extend(worker.iter().copied());
        let worker_toml: String = w.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
        let cfg = format!(
            "listen = \"127.0.0.1:{port}\"\ndata_dir = \"{}\"\nbackend = \"{backend}\"\nworkers = 2\n{extra}\n[worker]\n{worker_toml}",
            data.display()
        );
        let cfg_path = dir.path().join("config.toml");
        std::fs::write(&cfg_path, cfg).unwrap();
        let log = std::fs::File::create(dir.path().join("daemon.log")).unwrap();
        let child = Command::new(BIN)
            .arg("serve")
            .arg("--config")
            .arg(&cfg_path)
            .env("PL_ALLOW_FAKE_BACKEND", "1")
            .env("PL_LOG", "info")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        let d = Daemon { child, url: format!("http://127.0.0.1:{port}"), dir, admin_token };
        d.wait_health(|v| v["status"] == "ok", Duration::from_secs(30));
        d
    }

    pub fn data_dir(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    pub fn health(&self) -> Option<Value> {
        let agent = ureq::Agent::config_builder().http_status_as_error(false).build().new_agent();
        let mut r = agent.get(format!("{}/healthz", self.url)).call().ok()?;
        serde_json::from_str(&r.body_mut().read_to_string().ok()?).ok()
    }

    pub fn wait_health(&self, pred: impl Fn(&Value) -> bool, timeout: Duration) -> Value {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(v) = self.health()
                && pred(&v)
            {
                return v;
            }
            assert!(Instant::now() < deadline, "daemon não chegou no estado esperado; log:\n{}", self.log());
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("daemon.log")).unwrap_or_default()
    }

    pub fn admin(&self) -> Client {
        Client::new(&self.url, &self.admin_token)
    }

    pub fn client(&self, token: &str) -> Client {
        Client::new(&self.url, token)
    }

    /// Cria usuário (com sobrescritas de quota) e devolve uma chave dele.
    pub fn user(&self, name: &str, quota: Value) -> String {
        self.admin().call("admin.users.create", json!({ "name": name, "quota": quota })).unwrap();
        let k = self.admin().call("admin.keys.create", json!({ "user": name, "label": "teste" })).unwrap();
        k["token"].as_str().unwrap().to_string()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = rustix::process::kill_process(
            rustix::process::Pid::from_child(&self.child),
            rustix::process::Signal::TERM,
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn rpc_code(e: &ClientError) -> i32 {
    e.rpc().unwrap_or_else(|| panic!("esperava erro JSON-RPC, veio {e}")).code
}

pub fn sandbox(c: &Client) -> String {
    c.call("sandbox.create", json!({})).unwrap()["sandbox_id"].as_str().unwrap().to_string()
}

pub fn exec(c: &Client, sb: &str, command: &str) -> Value {
    c.call("exec", json!({ "sandbox_id": sb, "command": command })).unwrap()
}

pub fn argv(c: &Client, sb: &str, argv: &[&str]) -> Value {
    c.call("exec", json!({ "sandbox_id": sb, "argv": argv })).unwrap()
}

/// Processos vivos na sandbox, sem o init (pid 1).
pub fn live_processes(c: &Client, sb: &str) -> Vec<Value> {
    let ps = c.call("ps", json!({ "sandbox_id": sb })).unwrap();
    ps["processes"].as_array().unwrap().iter().filter(|p| p["state"] != "Z" && p["pid"] != 1).cloned().collect()
}
