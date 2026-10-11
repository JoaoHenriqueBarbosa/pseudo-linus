//! A sessão CDP que o Playwright e o pyppeteer reais tiveram com o Chromium 154 (`testbench/oracle/automation/golden/*.cdp.jsonl`,
//! gravadas por `run.sh`) repetida contra o `chromium` do sandbox: cada mensagem que o cliente mandou vai na ordem, os
//! ids que o navegador sorteia (sessão, alvo, contexto, janela, objeto) são trocados pelos do nosso navegador à medida
//! que chegam, e o que o oráculo respondeu e emitiu é comparado mensagem a mensagem. O placar sai por cena, com a lista
//! dos métodos que o nosso navegador ainda não conhece (`wasn't found`): é o conjunto exato que o cliente usa.
//!
//! O cliente real manda várias mensagens sem esperar a resposta da anterior; aqui cada uma espera a sua, então a ordem dos
//! eventos pode diferir do oráculo: o placar compara como multiconjunto por mensagem, como o `devtools.rs`.

use std::collections::HashMap;
use std::ffi::OsString;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use serde_json::Value;
use sysabi::sys::{self, write_all};
use sysabi::*;
use ul_chromium::devtools::websocket::Decoder;

mod support;
use support::{client_frame, normalize, send_all, Client};

const PORT: u16 = 9222;
const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testbench/oracle/automation/golden/");

/// As cenas com o Playwright, da mais simples à mais cheia.
const SCENES: [&str; 6] = ["pw_evaluate", "pw_locators", "pw_actions", "pw_context", "pw_navigation", "pw_frames"];

/// Os textos grandes que a gravação guardou em `blobs/<sha256>.txt` (`<sha256:HEX len=N>`) voltam ao lugar, nas mensagens
/// que o cliente mandou: o script injetado do Playwright é um deles.
fn expand(value: Value) -> Value {
    match value {
        Value::String(s) if s.starts_with("<sha256:") => {
            let digest = s["<sha256:".len()..].split(' ').next().unwrap_or_default();
            std::fs::read_to_string(format!("{GOLDEN}blobs/{digest}.txt")).map(Value::String).unwrap_or(Value::String(s))
        }
        Value::Array(items) => Value::Array(items.into_iter().map(expand).collect()),
        Value::Object(map) => Value::Object(map.into_iter().map(|(k, v)| (k, expand(v))).collect()),
        other => other,
    }
}

fn golden(name: &str) -> Vec<(bool, Value)> {
    let text = std::fs::read_to_string(format!("{GOLDEN}{name}.cdp.jsonl")).unwrap();
    text.lines()
        .map(|line| {
            let entry: Value = serde_json::from_str(line).unwrap();
            match entry.get("out") {
                Some(message) => (true, expand(message.clone())),
                None => (false, entry["in"].clone()),
            }
        })
        .collect()
}

/// Um id que o navegador sorteia: 32 algarismos hexadecimais maiúsculos.
fn is_token(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
}

/// As chaves cujo valor é um id do navegador que o cliente repete depois.
fn is_id_key(key: &str) -> bool {
    key.ends_with("Id") || key == "objectId" || key == "uniqueId" || key == "uniqueContextId" || key == "sessionId"
}

/// Pareia o que o oráculo disse com o que nós dissemos e aprende a troca de cada id que difere.
fn learn(map: &mut HashMap<String, String>, windows: &mut HashMap<i64, i64>, oracle: &Value, ours: &Value, key: &str) {
    match (oracle, ours) {
        (Value::String(o), Value::String(a)) if o != a && (is_token(o) || is_id_key(key)) => {
            map.entry(o.clone()).or_insert_with(|| a.clone());
        }
        (Value::Number(o), Value::Number(a)) if (key == "windowId" || key == "backendNodeId") && o != a => {
            windows.entry(o.as_i64().unwrap_or(0)).or_insert_with(|| a.as_i64().unwrap_or(0));
        }
        (Value::Object(o), Value::Object(a)) => {
            for (k, v) in o {
                if let Some(w) = a.get(k) {
                    learn(map, windows, v, w, k);
                }
            }
        }
        (Value::Array(o), Value::Array(a)) => {
            for (v, w) in o.iter().zip(a) {
                learn(map, windows, v, w, key);
            }
        }
        _ => {}
    }
}

/// A mensagem do cliente com os ids do oráculo trocados pelos nossos.
fn translate(map: &HashMap<String, String>, windows: &HashMap<i64, i64>, value: &Value, key: &str) -> Value {
    match value {
        Value::String(s) => Value::String(map.get(s).cloned().unwrap_or_else(|| s.clone())),
        Value::Number(n) if key == "windowId" || key == "backendNodeId" => Value::from(windows.get(&n.as_i64().unwrap_or(0)).copied().unwrap_or(n.as_i64().unwrap_or(0))),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), translate(map, windows, v, k))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(|v| translate(map, windows, v, key)).collect()),
        other => other.clone(),
    }
}

/// O servidor das páginas da fixture, em HTTP/1.0 como o `http.server` do Python: as mesmas três páginas.
fn serve_fixture(listener: Fd) -> i32 {
    const INDEX: &str = include_str!("../../../testbench/oracle/automation/fixtures/index.html");
    const SECOND: &str = include_str!("../../../testbench/oracle/automation/fixtures/second.html");
    const FRAMES: &str = include_str!("../../../testbench/oracle/automation/fixtures/frames.html");
    const FRAME_CHILD: &str = include_str!("../../../testbench/oracle/automation/fixtures/frame-child.html");
    const API: &str = include_str!("../../../testbench/oracle/automation/fixtures/api.json");
    let s = sys::current();
    loop {
        let Ok((fd, _)) = s.tcp_accept(listener, false, true) else { continue };
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            match sys::read(fd, &mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => request.extend_from_slice(&buffer[..n]),
            }
        }
        let text = String::from_utf8_lossy(&request).into_owned();
        let path = text.split(' ').nth(1).unwrap_or("/").split('?').next().unwrap_or("/").to_owned();
        let (kind, body) = match path.as_str() {
            "/" | "/index.html" => ("text/html", INDEX),
            "/second.html" => ("text/html", SECOND),
            "/frames.html" => ("text/html", FRAMES),
            "/frame-child.html" => ("text/html", FRAME_CHILD),
            "/api.json" => ("application/json", API),
            _ => ("", ""),
        };
        let response = if kind.is_empty() {
            "HTTP/1.0 404 File not found\r\nContent-Type: text/html;charset=utf-8\r\nContent-Length: 0\r\n\r\n".to_owned()
        } else {
            format!("HTTP/1.0 200 OK\r\nContent-type: {kind}\r\nContent-Length: {}\r\n\r\n{body}", body.len())
        };
        let _ = sys::write_all(fd, response.as_bytes());
        let _ = sys::close(fd);
    }
}

/// O cliente de dentro do sandbox: `replay_client CENA` imprime `{ "log": [...], "methods": [...] }`.
fn replay_client(_ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let name = argv[1].to_string_lossy().into_owned();
    let oracle = golden(&name);
    let s = sys::current();
    let (listener, _) = s.tcp_listen_at(std::net::Ipv4Addr::LOCALHOST.into(), 8765, 16, false, true).unwrap();
    s.spawn_fn(ProcAttrs::default(), b"fixture".to_vec(), Box::new(move || serve_fixture(listener))).unwrap();
    let args: Vec<Vec<u8>> = ["chromium", "--headless", "--no-sandbox", "--disable-gpu", &format!("--remote-debugging-port={PORT}"), "--user-data-dir=/tmp/profile", "about:blank"]
        .iter()
        .map(|a| a.as_bytes().to_vec())
        .collect();
    s.spawn(SpawnSpec { path: b"/usr/bin/chromium".to_vec(), argv: args, attrs: ProcAttrs::default() }).unwrap();
    let fd = support::open_browser_socket(PORT);
    let mut client = Client { fd, inbox: Vec::new(), decoder: Decoder::default(), log: Vec::new() };
    let (mut map, mut windows): (HashMap<String, String>, HashMap<i64, i64>) = Default::default();
    let oracle_in: Vec<&Value> = oracle.iter().filter(|(out, _)| !out).map(|(_, m)| m).collect();
    let mut used_oracle = vec![false; oracle_in.len()];
    let mut used_ours = 0usize;
    let mut unknown: Vec<String> = Vec::new();
    for (out, message) in &oracle {
        if !out {
            continue;
        }
        let mut sent = translate(&map, &windows, message, "");
        let id = sent["id"].as_i64().unwrap_or(0);
        if id < 0 {
            continue;
        }
        sent["id"] = Value::from(id);
        send_all(fd, &client_frame(&sent.to_string()));
        client.log.push(serde_json::json!({ "out": sent }));
        let reply = client.receive(Some(id), Duration::from_secs(5));
        client.receive(None, Duration::from_millis(120));
        if reply.as_ref().and_then(|r| r["error"]["code"].as_i64()) == Some(-32601) {
            unknown.push(sent["method"].as_str().unwrap_or_default().to_owned());
        }
        // Aprende os ids novos: cada mensagem nossa ainda sem par encontra a do oráculo de mesmo id (resposta) ou de
        // mesmo método (evento).
        let ours: Vec<Value> = client.log.iter().filter_map(|e| e.get("in").cloned()).collect();
        while used_ours < ours.len() {
            let ours_message = &ours[used_ours];
            used_ours += 1;
            let found = (0..oracle_in.len()).find(|&i| {
                !used_oracle[i]
                    && match (oracle_in[i].get("id"), ours_message.get("id")) {
                        (Some(a), Some(b)) => a == b || (translate(&map, &windows, a, "id") == *b),
                        (None, None) => oracle_in[i].get("method") == ours_message.get("method"),
                        _ => false,
                    }
            });
            if let Some(i) = found {
                used_oracle[i] = true;
                learn(&mut map, &mut windows, oracle_in[i], ours_message, "");
            }
        }
    }
    let methods: Vec<Value> = unknown.into_iter().map(Value::String).collect();
    // Só o que chegou vai à saída (o placar não usa o que o cliente mandou, e o texto do script injetado é grande).
    let log: Vec<&Value> = client.log.iter().filter(|e| e.get("in").is_some()).collect();
    let out = serde_json::json!({ "log": log, "methods": methods });
    write_all(Fd::STDOUT, out.to_string().as_bytes()).unwrap();
    send_all(fd, &client_frame(r#"{"id":99999,"method":"Browser.close"}"#));
    client.receive(Some(99999), Duration::from_secs(5));
    0
}

fn run(name: &str) -> Value {
    let kernel = Kernel::new(KernelConfig::default());
    let mut programs = ul_chromium::programs();
    programs.push(Program::bin("replay_client", replay_client));
    let sandbox = kernel.create_sandbox(SandboxConfig { programs, ..SandboxConfig::default() }).unwrap();
    let argv = vec![b"replay_client".to_vec(), name.as_bytes().to_vec()];
    let output = sandbox.run(RunRequest { argv, timeout: Some(Duration::from_secs(300)), ..RunRequest::default() }).unwrap();
    eprint!("{name} stderr: {}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| panic!("{name}: {e}: {}", String::from_utf8_lossy(&output.stdout)))
}

/// (mensagens iguais, total do oráculo, métodos desconhecidos).
fn score(name: &str) -> (usize, usize, Vec<String>) {
    let result = run(name);
    let mut ours: Vec<String> = result["log"].as_array().unwrap().iter().filter_map(|e| e.get("in")).map(|m| normalize(m, "").to_string()).collect();
    let (mut same, mut total) = (0, 0);
    for (out, message) in golden(name) {
        // As respostas a mensagens que o cliente mandou depois do fim (o `Browser.close` com id negativo) e os blocos
        // grandes que a gravação guardou em arquivo (`<sha256:...>`) não se comparam.
        if out || message.get("id").and_then(Value::as_i64).is_some_and(|id| id < 0) || message.to_string().contains("<sha256:") {
            continue;
        }
        total += 1;
        let wanted = normalize(&message, "").to_string();
        if let Some(position) = ours.iter().position(|m| *m == wanted) {
            ours.remove(position);
            same += 1;
        } else {
            eprintln!("{name} oráculo: {}", wanted.chars().take(400).collect::<String>());
        }
    }
    for extra in &ours {
        eprintln!("{name} sobra:   {}", extra.chars().take(400).collect::<String>());
    }
    let mut methods: Vec<String> = result["methods"].as_array().unwrap().iter().map(|m| m.as_str().unwrap().to_owned()).collect();
    methods.sort();
    methods.dedup();
    eprintln!("{name}: {same}/{total}; faltam {}: {}", methods.len(), methods.join(", "));
    (same, total, methods)
}

#[test]
fn playwright_sessions_match_the_oracle() {
    let (mut same, mut total) = (0, 0);
    // `SCENE=pw_frames` roda só uma cena (diagnóstico).
    let only = std::env::var("SCENE").ok();
    for name in SCENES.into_iter().filter(|name| only.as_deref().is_none_or(|only| only == *name)) {
        let (s, t, _) = score(name);
        same += s;
        total += t;
    }
    eprintln!("placar Playwright: {same}/{total}");
    assert!(total > 0 && same > 0, "placar Playwright {same}/{total}");
}
