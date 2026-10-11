//! O CDP do `chromium` contra as transcrições do oráculo (`tests/devtools/oracle.json`, gravadas pelo
//! `cdp-record.py` no Chromium 154 do pl-oracle-chromium com os cenários de `scenarios.json`). Um cliente roda dentro
//! do sandbox, sobe o `chromium --remote-debugging-port`, repete cada cenário pelo WebSocket do navegador e devolve a
//! transcrição; o teste compara mensagem a mensagem, com os valores que variam a cada execução (ids aleatórios,
//! `timestamp`, o isolate dos `objectId`) trocados pelo formato deles.

use std::ffi::OsString;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use serde_json::Value;
use sysabi::sys::{self, write_all};
use sysabi::*;
use ul_chromium::devtools::websocket::{Decoder, Incoming};
mod support;
use support::{client_frame, normalize, send_all, Client};

const SCENARIOS: &str = include_str!("devtools/scenarios.json");
const ORACLE: &str = include_str!("devtools/oracle.json");
const PORT: u16 = 9222;

/// O cliente de dentro do sandbox: `cdp_client CENÁRIO` imprime a transcrição em JSON.
fn cdp_client(_ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let name = argv[1].to_string_lossy().into_owned();
    let scenarios: Value = serde_json::from_str(SCENARIOS).unwrap();
    let scenario = scenarios.as_array().unwrap().iter().find(|s| s["name"] == name.as_str()).unwrap().clone();
    let s = sys::current();
    // O servidor local das navegações http: os mesmos bytes do `serve` do `cdp-record.py`.
    if scenario.get("server").and_then(Value::as_bool) == Some(true) {
        let (listener, _) = s.tcp_listen_at(std::net::Ipv4Addr::LOCALHOST.into(), 8765, 16, false, true).unwrap();
        s.spawn_fn(ProcAttrs::default(), b"server".to_vec(), Box::new(move || support::serve_routes(listener))).unwrap();
    }
    // Os mesmos argumentos com que o `cdp-record.py` gravou o oráculo.
    let argv: Vec<Vec<u8>> = ["chromium", "--headless", "--no-sandbox", "--disable-gpu", &format!("--remote-debugging-port={PORT}"), "--user-data-dir=/tmp/profile", "about:blank"]
        .iter()
        .map(|a| a.as_bytes().to_vec())
        .collect();
    s.spawn(SpawnSpec { path: b"/usr/bin/chromium".to_vec(), argv, attrs: ProcAttrs::default() }).unwrap();
    let fd = support::open_browser_socket(PORT);
    let mut client = Client { fd, inbox: Vec::new(), decoder: Decoder::default(), log: Vec::new() };
    let mut saved: std::collections::HashMap<String, String> = Default::default();
    let mut next_id = 1i64;
    for step in scenario["steps"].as_array().unwrap() {
        // Espera o evento `wait` (com o texto `match`) e guarda o campo `save_field` dos parâmetros dele.
        if let Some(method) = step.get("wait").and_then(Value::as_str) {
            let deadline = std::time::Instant::now() + Duration::from_secs_f64(step.get("timeout").and_then(Value::as_f64).unwrap_or(5.0));
            let wanted = step.get("match").and_then(Value::as_str).unwrap_or_default();
            loop {
                let found = client.log.iter().rev().filter_map(|entry| entry.get("in")).find(|m| m.get("method").and_then(Value::as_str) == Some(method) && m.to_string().contains(wanted)).cloned();
                if let Some(message) = found {
                    if let (Some(key), Some(field)) = (step.get("save").and_then(Value::as_str), step.get("save_field").and_then(Value::as_str)) {
                        let value = message["params"].pointer(field).cloned().unwrap_or(Value::Null);
                        saved.insert(key.to_owned(), value.as_str().map_or_else(|| value.to_string(), str::to_owned));
                    }
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    break;
                }
                client.receive(None, Duration::from_millis(100));
            }
            continue;
        }
        if let Some(seconds) = step.get("drain").and_then(Value::as_f64) {
            client.receive(None, Duration::from_secs_f64(seconds));
            continue;
        }
        if let Some(raw) = step.get("raw").and_then(Value::as_str) {
            send_all(fd, &client_frame(raw));
            client.log.push(serde_json::json!({ "out_raw": raw }));
            client.receive(None, Duration::from_millis(500));
            continue;
        }
        let mut message = serde_json::json!({ "id": next_id, "method": step["method"] });
        next_id += 1;
        if let Some(params) = step.get("params") {
            let mut text = params.to_string().replace("$TARGET", saved.get("target").map(String::as_str).unwrap_or(""));
            for (key, value) in &saved {
                text = text.replace(&format!("\"${key}\""), &if let Ok(number) = value.parse::<i64>() { Value::from(number) } else { Value::String(value.clone()) }.to_string());
            }
            message["params"] = serde_json::from_str(&text).unwrap();
        }
        if let Some(session) = step.get("session").and_then(Value::as_str) {
            message["sessionId"] = Value::String(saved.get(session).cloned().unwrap_or_else(|| session.to_owned()));
        }
        send_all(fd, &client_frame(&message.to_string()));
        client.log.push(serde_json::json!({ "out": message }));
        let reply = if step.get("nowait").and_then(Value::as_bool) == Some(true) { client.receive(None, Duration::from_millis(200)) } else { client.receive(message["id"].as_i64(), Duration::from_secs(10)) };
        if let Some(result) = reply.as_ref().and_then(|r| r.get("result")) {
            if step["method"] == "Target.getTargets" && !saved.contains_key("target") {
                let page = result["targetInfos"].as_array().unwrap().iter().find(|t| t["type"] == "page").unwrap();
                saved.insert("target".into(), page["targetId"].as_str().unwrap().to_owned());
            }
            if let (Some(key), Some(field)) = (step.get("save").and_then(Value::as_str), step.get("save_field").and_then(Value::as_str)) {
                let value = if field.starts_with('/') { result.pointer(field).cloned().unwrap_or(Value::Null) } else { result[field].clone() };
                saved.insert(key.to_owned(), value.as_str().map_or_else(|| value.to_string(), str::to_owned));
            }
        }
        if let Some(seconds) = step.get("after").and_then(Value::as_f64) {
            client.receive(None, Duration::from_secs_f64(seconds));
        }
    }
    write_all(Fd::STDOUT, serde_json::to_string(&client.log).unwrap().as_bytes()).unwrap();
    send_all(fd, &client_frame(&serde_json::json!({ "id": next_id, "method": "Browser.close" }).to_string()));
    client.receive(Some(next_id), Duration::from_secs(5));
    0
}

fn run_scenario(name: &str) -> Vec<Value> {
    let kernel = Kernel::new(KernelConfig::default());
    let mut programs = ul_chromium::programs();
    programs.push(Program::bin("cdp_client", cdp_client));
    let sandbox = kernel.create_sandbox(SandboxConfig { programs, ..SandboxConfig::default() }).unwrap();
    let argv = vec![b"cdp_client".to_vec(), name.as_bytes().to_vec()];
    let output = sandbox.run(RunRequest { argv, timeout: Some(Duration::from_secs(120)), ..RunRequest::default() }).unwrap();
    // O stderr do navegador fica à vista: só a linha do DevTools deveria aparecer.
    eprint!("{name} stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.status, WaitStatus::Exited(0), "{name}: {}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

/// As mensagens que chegaram depois de cada envio, normalizadas, em ordem de chegada.
fn steps(log: &[Value]) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for entry in log {
        if let Some(sent) = entry.get("out").or_else(|| entry.get("out_raw")) {
            out.push((sent.to_string(), Vec::new()));
        } else if let (Some(message), Some(last)) = (entry.get("in"), out.last_mut()) {
            last.1.push(normalize(message, "").to_string());
        }
    }
    out
}

/// Compara o cenário com o do oráculo: (mensagens iguais, total do oráculo), com as divergências impressas.
fn score(name: &str) -> (usize, usize) {
    let oracle: Value = serde_json::from_str(ORACLE).unwrap();
    let expected = steps(oracle[name].as_array().unwrap());
    let actual = steps(&run_scenario(name));
    let (mut same, mut total) = (0, 0);
    for (index, (sent, wanted)) in expected.iter().enumerate() {
        let got = actual.get(index).map(|(_, got)| got.clone()).unwrap_or_default();
        let mut remaining = got.clone();
        for message in wanted {
            total += 1;
            if let Some(position) = remaining.iter().position(|m| m == message) {
                remaining.remove(position);
                same += 1;
            } else {
                eprintln!("{name} #{index} {sent}\n  oráculo: {message}");
            }
        }
        for extra in remaining {
            eprintln!("{name} #{index} {sent}\n  sobra:   {extra}");
        }
    }
    eprintln!("{name}: {same}/{total}");
    (same, total)
}

#[test]
fn transcripts_match_the_oracle() {
    let mut same = 0;
    let mut total = 0;
    for name in ["browser", "attach_evaluate", "runtime_enable", "page_navigate", "targets", "sessions", "unflatten_target_commands", "auto_attach_wait", "domains_enable", "network_navigate", "emulation_metrics", "network_http", "playwright_handshake", "playwright_state", "playwright_network", "playwright_cookies", "playwright_locale", "playwright_fetch"] {
        let (s, t) = score(name);
        same += s;
        total += t;
    }
    eprintln!("placar CDP: {same}/{total}");
    // A catraca: o placar medido quando o bloco entrou (128 a 130 de 166, conforme a corrida dos marcos de
    // tempo), com folga para essa variação; só sobe. O que falta está listado na saída: os alvos browser_ui do
    // omnibox, a pilha no formato do V8, consoleAPICalled e exceptionThrown, a página de erro.
    assert!(same >= 120, "placar CDP {same}/{total}");
}
