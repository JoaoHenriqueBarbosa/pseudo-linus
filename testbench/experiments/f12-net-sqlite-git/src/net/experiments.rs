//! Medições do H34 com servidores HTTP locais (std::net) que contam as conexões que recebem.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{Value as Json, json};

use super::{Policy, sandbox_agent};

/// Servidor HTTP mínimo: `/ok` responde 200; `/redirect` responde 302 pra `redirect_to`.
pub struct Server {
    pub addr: SocketAddr,
    accepted: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

fn serve(mut s: TcpStream, redirect_to: Option<&str>, me: SocketAddr) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let req = String::from_utf8_lossy(&buf);
    let path = req.split_whitespace().nth(1).unwrap_or("/");
    let resp = match (path.starts_with("/redirect"), redirect_to) {
        (true, Some(to)) => format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
        _ => {
            let body = format!("hello from {me}");
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
        }
    };
    let _ = s.write_all(resp.as_bytes());
}

impl Server {
    pub fn start(ip: &str, redirect_to: Option<String>) -> std::io::Result<Server> {
        let listener = TcpListener::bind((ip, 0))?;
        let addr = listener.local_addr()?;
        let accepted = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (acc, st) = (accepted.clone(), stop.clone());
        let handle = std::thread::spawn(move || {
            for conn in listener.incoming() {
                if st.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(s) = conn {
                    acc.fetch_add(1, Ordering::SeqCst);
                    serve(s, redirect_to.as_deref(), addr);
                }
            }
        });
        Ok(Server { addr, accepted, stop, handle: Some(handle) })
    }

    pub fn connections(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn hosts(pairs: &[(&str, &str)]) -> BTreeMap<String, Vec<IpAddr>> {
    pairs.iter().map(|(h, ip)| (h.to_string(), vec![ip.parse().expect("ip")])).collect()
}

/// Uma requisição pelo agente do sandbox: status ou erro.
fn get(agent: &ureq::Agent, url: &str) -> Result<(u16, String), String> {
    match agent.get(url).call() {
        Ok(mut r) => {
            let status = r.status().as_u16();
            let body = r.body_mut().read_to_string().unwrap_or_default();
            Ok((status, body))
        }
        Err(e) => Err(e.to_string()),
    }
}

struct Scenario {
    name: &'static str,
    expect_success: bool,
    result: Result<(u16, String), String>,
    /// (servidor, conexões vistas, conexões esperadas)
    servers: Vec<(&'static str, usize, usize)>,
    dns_lookups: usize,
    policy_log: Vec<String>,
}

impl Scenario {
    fn passed(&self) -> bool {
        self.result.is_ok() == self.expect_success && self.servers.iter().all(|(_, seen, want)| seen == want)
    }

    fn json(&self) -> Json {
        json!({
            "passed": self.passed(),
            "expect_success": self.expect_success,
            "result": match &self.result { Ok((s, b)) => json!({"status": s, "body": b}), Err(e) => json!({"error": e}) },
            "servers": self.servers.iter().map(|(n, seen, want)| json!({"server": n, "connections": seen, "expected": want})).collect::<Vec<_>>(),
            "dns_lookups_on_host": self.dns_lookups,
            "policy_log": self.policy_log,
        })
    }
}

fn run(
    name: &'static str,
    allow: &[&str],
    block_private: bool,
    table: &[(&str, &str)],
    url: &str,
    expect_success: bool,
    servers: &[(&'static str, &Server, usize)],
) -> Scenario {
    let policy = Arc::new(Policy::new(allow, block_private));
    let (agent, lookups) = sandbox_agent(policy.clone(), hosts(table));
    let result = get(&agent, url);
    std::thread::sleep(Duration::from_millis(50));
    Scenario {
        name,
        expect_success,
        result,
        servers: servers.iter().map(|(n, s, want)| (*n, s.connections(), *want)).collect(),
        dns_lookups: lookups.load(Ordering::Relaxed),
        policy_log: policy.log.lock().expect("log").clone(),
    }
}

/// Todos os cenários da allowlist com o ureq.
pub fn ureq_scenarios() -> Json {
    let mut out = Vec::new();
    // Servidores novos por cenário, pra que as contagens sejam isoladas.
    {
        let s = Server::start("127.0.0.1", None).expect("servidor");
        let url = format!("http://allowed.test:{}/ok", s.port());
        out.push(run("allowed_name_passes", &["allowed.test"], false, &[("allowed.test", "127.0.0.1")], &url, true, &[("target", &s, 1)]));
    }
    {
        let s = Server::start("127.0.0.1", None).expect("servidor");
        let url = format!("http://denied.test:{}/ok", s.port());
        out.push(run("denied_name_never_connects", &["allowed.test"], false, &[("denied.test", "127.0.0.1")], &url, false, &[("target", &s, 0)]));
    }
    {
        let s = Server::start("127.0.0.1", None).expect("servidor");
        let url = format!("http://rebind.test:{}/ok", s.port());
        out.push(run("allowed_name_resolving_to_loopback_blocked", &["rebind.test"], true, &[("rebind.test", "127.0.0.1")], &url, false, &[("target", &s, 0)]));
    }
    {
        let s = Server::start("127.0.0.1", None).expect("servidor");
        let url = format!("http://localhost:{}/ok", s.port());
        out.push(run("allowed_localhost_via_host_dns_blocked", &["localhost"], true, &[], &url, false, &[("target", &s, 0)]));
    }
    {
        let target = Server::start("127.0.0.2", None).expect("servidor");
        let to = format!("http://denied.test:{}/ok", target.port());
        let front = Server::start("127.0.0.1", Some(to)).expect("servidor");
        let url = format!("http://allowed.test:{}/redirect", front.port());
        out.push(run(
            "redirect_to_denied_host_blocked",
            &["allowed.test"],
            false,
            &[("allowed.test", "127.0.0.1"), ("denied.test", "127.0.0.2")],
            &url,
            false,
            &[("front", &front, 1), ("redirect_target", &target, 0)],
        ));
    }
    {
        let target = Server::start("127.0.0.2", None).expect("servidor");
        let to = format!("http://127.0.0.2:{}/ok", target.port());
        let front = Server::start("127.0.0.1", Some(to)).expect("servidor");
        let url = format!("http://allowed.test:{}/redirect", front.port());
        out.push(run(
            "redirect_to_denied_ip_literal_blocked",
            &["allowed.test"],
            false,
            &[("allowed.test", "127.0.0.1")],
            &url,
            false,
            &[("front", &front, 1), ("redirect_target", &target, 0)],
        ));
    }
    {
        let target = Server::start("127.0.0.2", None).expect("servidor");
        let to = format!("http://other.test:{}/ok", target.port());
        let front = Server::start("127.0.0.1", Some(to)).expect("servidor");
        let url = format!("http://allowed.test:{}/redirect", front.port());
        out.push(run(
            "redirect_to_allowed_host_followed",
            &["allowed.test", "other.test"],
            false,
            &[("allowed.test", "127.0.0.1"), ("other.test", "127.0.0.2")],
            &url,
            true,
            &[("front", &front, 1), ("redirect_target", &target, 1)],
        ));
    }
    {
        let s = Server::start("127.0.0.1", None).expect("servidor");
        let url = format!("http://127.0.0.1:{}/ok", s.port());
        out.push(run("ip_literal_not_in_allowlist_denied", &["allowed.test"], false, &[], &url, false, &[("target", &s, 0)]));
    }
    {
        let s = Server::start("127.0.0.1", None).expect("servidor");
        let url = format!("http://127.0.0.1:{}/ok", s.port());
        out.push(run("ip_literal_allowlisted_but_private_blocked", &["127.0.0.1"], true, &[], &url, false, &[("target", &s, 0)]));
    }
    if let Ok(s) = Server::start("::1", None) {
        let url = format!("http://[::1]:{}/ok", s.port());
        out.push(run("ipv6_literal_denied", &["allowed.test"], false, &[], &url, false, &[("target", &s, 0)]));
    }
    {
        let s = Server::start("127.0.0.1", None).expect("servidor");
        let url = format!("http://allowed.test@denied.test:{}/ok", s.port());
        out.push(run(
            "userinfo_trick_denied",
            &["allowed.test"],
            false,
            &[("denied.test", "127.0.0.1"), ("allowed.test", "127.0.0.1")],
            &url,
            false,
            &[("target", &s, 0)],
        ));
    }
    {
        let s = Server::start("127.0.0.1", None).expect("servidor");
        let url = format!("http://ALLOWED.Test.:{}/ok", s.port());
        out.push(run("case_and_trailing_dot_normalized", &["allowed.test"], false, &[("allowed.test", "127.0.0.1")], &url, true, &[("target", &s, 1)]));
    }
    let passed = out.iter().filter(|s| s.passed()).count();
    let mut map = serde_json::Map::new();
    for s in &out {
        map.insert(s.name.to_string(), s.json());
    }
    json!({"total": out.len(), "passed": passed, "scenarios": map})
}

/// GET HTTPS de verdade (rustls + webpki-roots), opcional se não houver rede.
pub fn https_real() -> Json {
    let policy = Arc::new(Policy::new(&["example.com"], true));
    let (agent, lookups) = sandbox_agent(policy.clone(), BTreeMap::new());
    let started = std::time::Instant::now();
    let r = get(&agent, "https://example.com/");
    let elapsed = started.elapsed().as_millis();
    match r {
        Ok((status, body)) => json!({
            "ran": true,
            "status": status,
            "body_has_example_domain": body.contains("Example Domain"),
            "dns_lookups_on_host": lookups.load(Ordering::Relaxed),
            "policy_log": policy.log.lock().expect("log").clone(),
            "ms": elapsed,
        }),
        Err(e) => {
            let offline = e.contains("host not found") || e.contains("resolve") || e.contains("Connection refused") || e.contains("timeout") || e.contains("Network is unreachable");
            json!({"ran": !offline, "skipped_no_network": offline, "error": e, "ms": elapsed})
        }
    }
}

/// Bloqueio por URL inicial apenas (o máximo que dá sem gancho de DNS/conexão) e o que acontece num
/// redirect pra host negado, com e sem seguir redirects automaticamente.
pub fn alternatives() -> Json {
    let mut out = serde_json::Map::new();
    for lib in ["attohttpc", "minreq"] {
        let mut per = serde_json::Map::new();
        for follow in [true, false] {
            let target = Server::start("127.0.0.2", None).expect("servidor");
            let to = format!("http://127.0.0.2:{}/ok", target.port());
            let front = Server::start("127.0.0.1", Some(to)).expect("servidor");
            let url = format!("http://127.0.0.1:{}/redirect", front.port());
            // Política: só 127.0.0.1. O invólucro checa a URL antes de chamar a crate.
            let policy = Policy::new(&["127.0.0.1"], false);
            let initial_allowed = policy.host_allowed("127.0.0.1");
            let result: Result<(u16, Option<String>), String> = match lib {
                "attohttpc" => attohttpc::get(&url)
                    .follow_redirects(follow)
                    .timeout(Duration::from_secs(5))
                    .send()
                    .map(|r| (r.status().as_u16(), r.headers().get("location").and_then(|v| v.to_str().ok()).map(str::to_string)))
                    .map_err(|e| e.to_string()),
                _ => minreq::get(&url)
                    .with_max_redirects(if follow { 5 } else { 0 })
                    .with_timeout(5)
                    .send()
                    .map(|r| {
                        let loc = r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("location")).map(|(_, v)| v.clone());
                        (r.status_code, loc)
                    })
                    .map_err(|e| e.to_string()),
            };
            std::thread::sleep(Duration::from_millis(50));
            // Sem seguir automaticamente, o invólucro pode checar o Location antes de seguir.
            let wrapper_blocks_location = match &result {
                Ok((_, Some(loc))) => {
                    let host = loc.parse::<ureq::http::Uri>().ok().and_then(|u| u.host().map(str::to_string)).unwrap_or_default();
                    !policy.host_allowed(&host)
                }
                _ => false,
            };
            per.insert(
                if follow { "follow_redirects_automatically" } else { "redirects_disabled_wrapper_checks_location" }.to_string(),
                json!({
                    "initial_url_allowed_by_wrapper": initial_allowed,
                    "result": match &result { Ok((s, l)) => json!({"status": s, "location": l}), Err(e) => json!({"error": e}) },
                    "denied_target_connections": target.connections(),
                    "policy_violated": target.connections() > 0,
                    "wrapper_can_block_location": wrapper_blocks_location,
                }),
            );
        }
        out.insert(lib.to_string(), Json::Object(per));
    }
    Json::Object(out)
}

/// Árvore de dependências de cada cliente: tokio, C, toque no host.
pub fn dependency_trees() -> Json {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let mut out = serde_json::Map::new();
    for pkg in ["ureq", "attohttpc", "minreq"] {
        match depscan::scan(&manifest, pkg) {
            Ok(scan) => {
                let names: Vec<String> = scan.deps.iter().map(|d| d.name.clone()).collect();
                let async_runtime: Vec<&String> =
                    names.iter().filter(|n| ["tokio", "async-std", "smol", "mio", "futures-executor", "hyper"].contains(&n.as_str())).collect();
                out.insert(
                    pkg.to_string(),
                    json!({
                        "version": scan.root.version,
                        "own_category": scan.root.category.letter(),
                        "tree_category": scan.tree_category.letter(),
                        "transitive_deps": names.len(),
                        "async_runtime_crates": async_runtime,
                        "has_tokio": names.iter().any(|n| n == "tokio"),
                        "c_deps": scan.c_deps,
                        "host_touching_deps": scan.host_touching_deps,
                        "own_unsafe": scan.root.counts.unsafe_total(),
                    }),
                );
            }
            Err(e) => {
                out.insert(pkg.to_string(), json!({"error": e.to_string()}));
            }
        }
    }
    Json::Object(out)
}

/// Quanto custa o caminho sem chamada de rede: uma política negando 1000 nomes.
pub fn deny_cost() -> Json {
    let policy = Arc::new(Policy::new(&["allowed.test"], true));
    let (agent, lookups) = sandbox_agent(policy, BTreeMap::new());
    let n = 1000;
    let start = std::time::Instant::now();
    let mut denied = 0;
    for i in 0..n {
        if get(&agent, &format!("http://h{i}.denied.test/")).is_err() {
            denied += 1;
        }
    }
    json!({"requests": n, "denied": denied, "dns_lookups_on_host": lookups.load(Ordering::Relaxed), "us_per_denied_request": start.elapsed().as_micros() as f64 / n as f64})
}
