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
use support::{client_frame, send_all};

const SCENARIOS: &str = include_str!("devtools/scenarios.json");
const ORACLE: &str = include_str!("devtools/oracle.json");
const PORT: u16 = 9222;

struct Client {
    fd: Fd,
    inbox: Vec<u8>,
    decoder: Decoder,
    log: Vec<Value>,
}

impl Client {
    /// Espera até `wait` por mensagens; cada uma vai para o registro. Devolve a que tem `id` igual a `until`.
    fn receive(&mut self, until: Option<i64>, wait: Duration) -> Option<Value> {
        let deadline = std::time::Instant::now() + wait;
        loop {
            // Todos os quadros já lidos entram no registro, mesmo os que chegaram junto com a resposta esperada.
            let mut found = None;
            for incoming in self.decoder.feed(&mut self.inbox) {
                if let Incoming::Text(text) = incoming {
                    let message: Value = serde_json::from_str(&text).unwrap();
                    self.log.push(serde_json::json!({ "in": message }));
                    if until.is_some() && message.get("id").and_then(Value::as_i64) == until {
                        found = Some(message);
                    }
                }
            }
            if found.is_some() {
                return found;
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return None;
            }
            let mut fds = [PollFd { fd: self.fd, events: PollEvents::IN, revents: PollEvents::empty() }];
            if sys::current().poll(&mut fds, Some(left)).unwrap_or(0) == 0 {
                continue;
            }
            let mut buffer = [0u8; 65536];
            match sys::read(self.fd, &mut buffer) {
                Ok(0) | Err(_) => return None,
                Ok(n) => self.inbox.extend_from_slice(&buffer[..n]),
            }
        }
    }
}

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
                text = text.replace(&format!("\"${key}\""), &Value::String(value.clone()).to_string());
            }
            message["params"] = serde_json::from_str(&text).unwrap();
        }
        if let Some(session) = step.get("session").and_then(Value::as_str) {
            message["sessionId"] = Value::String(saved.get(session).cloned().unwrap_or_else(|| session.to_owned()));
        }
        send_all(fd, &client_frame(&message.to_string()));
        client.log.push(serde_json::json!({ "out": message }));
        let reply = client.receive(message["id"].as_i64(), Duration::from_secs(10));
        if let Some(result) = reply.as_ref().and_then(|r| r.get("result")) {
            if step["method"] == "Target.getTargets" && !saved.contains_key("target") {
                let page = result["targetInfos"].as_array().unwrap().iter().find(|t| t["type"] == "page").unwrap();
                saved.insert("target".into(), page["targetId"].as_str().unwrap().to_owned());
            }
            if let (Some(key), Some(field)) = (step.get("save").and_then(Value::as_str), step.get("save_field").and_then(Value::as_str)) {
                saved.insert(key.to_owned(), result[field].as_str().unwrap().to_owned());
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

/// Troca o que varia entre execuções pelo formato: ids de 128 bits, UUID, `objectId` (o isolate), `uniqueId`,
/// `timestamp` e o `Content-Length` não entram.
fn normalize(value: &Value, key: &str) -> Value {
    let token = |s: &str| s.len() == 32 && s.bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b));
    let uuid = |s: &str| s.len() == 36 && s.bytes().all(|b| b == b'-' || b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    match value {
        // O título do popup do omnibox depende de quanto ele já carregou quando o cliente pergunta. O oráculo gravou
        // "", "Omnibox Popup" e o título provisório, que é o host no popup da raiz e a URL inteira no
        // `omnibox_popup_aim.html`. Só esses valem; outro fica como veio e diverge.
        Value::Object(map) if map.get("type").and_then(Value::as_str) == Some("browser_ui") => {
            let provisional = match map.get("url").and_then(Value::as_str) {
                Some("chrome://omnibox-popup.top-chrome/") => "chrome://omnibox-popup.top-chrome",
                Some(url) => url,
                None => "",
            };
            Value::Object(
                map.iter()
                    .map(|(k, v)| {
                        let measured = v.as_str().is_some_and(|t| t.is_empty() || t == "Omnibox Popup" || t == provisional);
                        (k.clone(), if k == "title" && measured { Value::String("<omnibox>".into()) } else { normalize(v, k) })
                    })
                    .collect(),
            )
        }
        // Os instantes e as durações da troca de rede variam a cada execução; os campos que o Chromium deixa em -1 por
        // não ter feito DNS nem conexão (conexão reaproveitada) também variam com o estado do pool de sockets.
        Value::Object(map) if key == "timing" || key == "connectTiming" => Value::Object(map.keys().map(|k| (k.clone(), Value::String("<n>".into()))).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), normalize(v, k))).collect()),
        // O `getTargets` do oráculo sai na ordem de um mapa de hash, que muda de uma execução para outra: qualquer
        // ordem vale, então a comparação é como conjunto.
        Value::Array(items) if key == "targetInfos" => {
            let mut items: Vec<Value> = items.iter().map(|v| normalize(v, key)).collect();
            items.sort_by_key(|v| v.to_string());
            Value::Array(items)
        }
        Value::Array(items) => Value::Array(items.iter().map(|v| normalize(v, key)).collect()),
        Value::Number(_) if key == "timestamp" || key == "wallTime" || key == "responseTime" => Value::String("<timestamp>".into()),
        // O id da conexão e o reaproveitamento dela dependem do pool de sockets do processo (o oráculo mostrou
        // `connectionReused` verdadeiro e falso para o mesmo pedido em corridas diferentes).
        Value::Number(_) if key == "connectionId" => Value::String("<n>".into()),
        Value::Bool(_) if key == "connectionReused" => Value::String("<bool>".into()),
        // O `requestId` de subrecurso é `pid.n`.
        Value::String(s) if key == "requestId" && s.split_once('.').is_some_and(|(a, b)| !a.is_empty() && a.bytes().all(|c| c.is_ascii_digit()) && b.bytes().all(|c| c.is_ascii_digit())) => Value::String("<requestId>".into()),
        Value::Number(_) if key == "windowId" => Value::String("<windowId>".into()),
        Value::String(s) if token(s) => Value::String("<token>".into()),
        Value::String(s) if uuid(s) => Value::String("<uuid>".into()),
        Value::String(s) if key == "objectId" => Value::String(format!("<isolate>.{}", s.split_once('.').map(|(_, rest)| rest).unwrap_or_default())),
        Value::String(_) if key == "uniqueId" => Value::String("<unique>".into()),
        Value::String(s) if s.starts_with("ws://") || s.contains("/devtools/") => Value::String(s.split("/devtools/").next().unwrap_or_default().to_owned()),
        other => other.clone(),
    }
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
    for name in ["browser", "attach_evaluate", "runtime_enable", "page_navigate", "targets", "sessions", "unflatten_target_commands", "auto_attach_wait", "domains_enable", "network_navigate", "emulation_metrics", "network_http"] {
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
