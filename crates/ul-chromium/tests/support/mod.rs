//! O que os testes de perfil do `--dump-dom` e do DevTools repartem.
#![allow(dead_code)]

use std::time::Duration;

use sysabi::sys;
use sysabi::*;
use serde_json::Value;
use ul_chromium::devtools::websocket::{self, Decoder, Incoming};

/// Responde toda requisição com a página, em HTTP/1.0 como o `http.server` do Python.
pub fn serve(listener: Fd, page: &'static str) -> i32 {
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
        let response = format!("HTTP/1.0 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{page}", page.len());
        let _ = sys::write_all(fd, response.as_bytes());
        let _ = sys::close(fd);
    }
}

/// O servidor das navegações http do `cdp-record.py` (HTTP/1.0): `/page` responde 200 com o corpo e `/redirect` leva a
/// `/page`; o resto é 404 sem corpo.
pub fn serve_routes(listener: Fd) -> i32 {
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
        let path = String::from_utf8_lossy(&request).split(' ').nth(1).unwrap_or("/").to_owned();
        let response: &[u8] = match path.as_str() {
            "/page" => b"HTTP/1.0 200 OK\r\nContent-Type: text/html\r\nContent-Length: 29\r\n\r\n<title>H</title><p>hello</p>\n",
            "/redirect" => b"HTTP/1.0 302 Found\r\nLocation: /page\r\nContent-Length: 0\r\n\r\n",
            _ => b"HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\n\r\n",
        };
        let _ = sys::write_all(fd, response);
        let _ = sys::close(fd);
    }
}

/// Escreve todos os bytes no descritor.
pub fn send_all(fd: Fd, bytes: &[u8]) {
    let mut at = 0;
    while at < bytes.len() {
        at += sys::write(fd, &bytes[at..]).unwrap();
    }
}

/// Um quadro de texto do cliente, mascarado como a RFC exige.
pub fn client_frame(text: &str) -> Vec<u8> {
    let mut frame = websocket::encode(websocket::TEXT, text.as_bytes());
    let header = frame.len() - text.len();
    frame[1] |= 0x80;
    let mask = [0x12u8, 0x34, 0x56, 0x78];
    let mut body = frame.split_off(header);
    body.iter_mut().enumerate().for_each(|(i, b)| *b ^= mask[i % 4]);
    frame.extend_from_slice(&mask);
    frame.extend(body);
    frame
}

/// Espera a porta do DevTools abrir, pega a URL do navegador em `/json/version` e faz o upgrade do WebSocket; devolve o
/// descritor já no protocolo.
pub fn open_browser_socket(port: u16) -> Fd {
    let s = sys::current();
    let lo: std::net::IpAddr = std::net::Ipv4Addr::LOCALHOST.into();
    let fd = (0..500)
        .find_map(|_| match s.tcp_connect_at(lo, port, false, true) {
            Ok((fd, _)) => Some(fd),
            Err(_) => {
                let mut none: [PollFd; 0] = [];
                let _ = s.poll(&mut none, Some(Duration::from_millis(20)));
                None
            }
        })
        .expect("o DevTools não abriu a porta");
    send_all(fd, format!("GET /json/version HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").as_bytes());
    let mut response = Vec::new();
    let mut buffer = [0u8; 65536];
    let version: serde_json::Value = loop {
        let n = sys::read(fd, &mut buffer).unwrap();
        response.extend_from_slice(&buffer[..n]);
        let text = String::from_utf8_lossy(&response).into_owned();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let length: usize = head.split("Content-Length:").nth(1).unwrap().split("\r\n").next().unwrap().parse().unwrap();
            if body.len() >= length {
                break serde_json::from_str(body).unwrap();
            }
        }
    };
    let path = version["webSocketDebuggerUrl"].as_str().unwrap().split(&format!("{port}")).nth(1).unwrap().to_owned();
    send_all(
        fd,
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n").as_bytes(),
    );
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0u8; 1];
        sys::read(fd, &mut byte).unwrap();
        head.push(byte[0]);
    }
    fd
}

/// Sobe o servidor da página em 127.0.0.1:8000 e o navegador com o DevTools (porta 9222), `--user-data-dir=/tmp/h` e a
/// página como URL; espera `seconds` depois da porta abrir e o encerra por `term` (SIGTERM) ou `close` (`Browser.close`).
/// Devolve o pid do servidor, que quem chama mata quando terminar de olhar o perfil.
pub fn run_devtools(page: &'static str, name: &str, how: &str, seconds: f64) -> Pid {
    let s = sys::current();
    let (listener, _) = s.tcp_listen_at(std::net::Ipv4Addr::LOCALHOST.into(), 8000, 16, false, true).unwrap();
    let server = s.spawn_fn(ProcAttrs::default(), b"server".to_vec(), Box::new(move || serve(listener, page))).unwrap();
    let args: Vec<Vec<u8>> = ["chromium", "--headless", "--no-sandbox", "--disable-gpu", "--remote-debugging-port=9222", "--user-data-dir=/tmp/h", &format!("http://127.0.0.1:8000/{name}")]
        .iter()
        .map(|a| a.as_bytes().to_vec())
        .collect();
    let pid = s.spawn(SpawnSpec { path: b"/usr/bin/chromium".to_vec(), argv: args, attrs: ProcAttrs::default() }).unwrap();
    let fd = open_browser_socket(9222);
    let mut none: [PollFd; 0] = [];
    let _ = s.poll(&mut none, Some(Duration::from_secs_f64(seconds)));
    if how == "close" {
        send_all(fd, &client_frame(r#"{"id":1,"method":"Browser.close"}"#));
    } else {
        s.kill(KillTarget::Pid(pid), Signal::SIGTERM).unwrap();
    }
    let _ = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty());
    server
}

pub struct Client {
    pub fd: Fd,
    pub inbox: Vec<u8>,
    pub decoder: Decoder,
    pub log: Vec<Value>,
}

impl Client {
    /// Espera até `wait` por mensagens; cada uma vai para o registro. Devolve a que tem `id` igual a `until`.
    pub fn receive(&mut self, until: Option<i64>, wait: Duration) -> Option<Value> {
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

/// Troca o que varia entre execuções pelo formato: ids de 128 bits, UUID, `objectId` (o isolate), `uniqueId`,
/// `timestamp` e o `Content-Length` não entram.
pub fn normalize(value: &Value, key: &str) -> Value {
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

