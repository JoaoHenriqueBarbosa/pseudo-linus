//! O servidor HTTP do DevTools (`content/browser/devtools/devtools_http_handler.cc`): os comandos `/json/*`, a
//! página `/devtools/inspector.html`, a recusa de Host que não é local e o aperto de mão do WebSocket. Os cabeçalhos
//! saem como no `net::HttpServer` do Chromium, sem espaço depois dos dois pontos.

use super::json::Json;

/// O que o `/json/version` e o `Browser.getVersion` declaram, medido no Chromium do oráculo.
pub const PRODUCT: &str = "Chrome/154.0.8037.92";
pub const PROTOCOL_VERSION: &str = "1.3";
pub const REVISION: &str = "@334b65d254ccc35df4fca82706d1753227b01039";
pub const V8_VERSION: &str = "15.4.80.19";
pub const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/154.0.0.0 Safari/537.36";

/// O `/json/protocol`, o arquivo estático do Chromium do oráculo byte a byte.
const PROTOCOL: &[u8] = include_bytes!("../../data/devtools/protocol.json");
/// O `/devtools/inspector.html` do pacote do frontend embutido.
const INSPECTOR: &[u8] = include_bytes!("../../data/devtools/inspector.html");

/// Uma requisição já lida inteira (cabeçalhos e corpo pelo `Content-Length`).
#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub target: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// Lê uma requisição do começo de `buffer`: `Some((requisição, bytes consumidos))`, ou `None` enquanto não chegou
/// inteira.
pub fn parse_request(buffer: &[u8]) -> Option<(Request, usize)> {
    let end = buffer.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&buffer[..end]);
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next().unwrap_or_default().to_owned();
    let target = first.next().unwrap_or_default().to_owned();
    let version = first.next().unwrap_or_default().to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':').map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned())))
        .collect();
    let request = Request { method, target, version, headers };
    let body = request.header("Content-Length").and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
    let total = end + 4 + body;
    (buffer.len() >= total).then_some((request, total))
}

/// Uma página ou outro alvo como o `/json/list` o descreve.
#[derive(Clone, Debug)]
pub struct TargetEntry {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub url: String,
}

/// O que o servidor HTTP pede ao navegador.
pub trait Browser {
    /// O id do alvo browser (o UUID do `webSocketDebuggerUrl`).
    fn browser_id(&self) -> String;
    /// Os alvos que o `/json/list` mostra, na ordem do Chromium (o mais novo primeiro).
    fn targets(&self) -> Vec<TargetEntry>;
    /// O `/json/new`: abre uma aba em `url` e devolve a entrada dela (de título ainda vazio).
    fn create(&mut self, url: &str) -> TargetEntry;
    fn activate(&mut self, id: &str) -> bool;
    fn close(&mut self, id: &str) -> bool;
}

/// Para onde vai uma conexão que pediu o WebSocket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    Browser,
    Page(String),
}

/// A resposta de uma requisição.
pub enum Reply {
    /// Bytes a escrever; a conexão segue (keep-alive).
    Http(Vec<u8>),
    /// O 101 já montado; a conexão passa a ser WebSocket do `Endpoint`.
    Upgrade(Vec<u8>, Endpoint),
    /// Sem resposta: a conexão fecha (o HTTP/1.0 sem Host do Chromium).
    Close,
}

fn response(status: &str, extra: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\n{extra}Content-Length:{}\r\nContent-Type:{content_type}\r\n\r\n", body.len())
        .into_bytes();
    out.extend_from_slice(body);
    out
}

const JSON_TYPE: &str = "application/json; charset=UTF-8";
const CSP: &str = "Content-Security-Policy:frame-ancestors 'none'\r\n";

fn json_reply(status: &str, body: &[u8]) -> Reply {
    Reply::Http(response(status, CSP, JSON_TYPE, body))
}

/// O `Host` precisa ser IP ou `localhost` (com porta ou não), a defesa contra DNS rebinding do Chromium.
fn host_is_local(host: &str) -> bool {
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or_default()
    } else {
        host.rsplit_once(':').filter(|(_, port)| port.bytes().all(|b| b.is_ascii_digit())).map_or(host, |(name, _)| name)
    };
    name.eq_ignore_ascii_case("localhost") || name.parse::<std::net::IpAddr>().is_ok()
}

fn target_json(entry: &TargetEntry, host: &str) -> Json {
    Json::object([
        ("description", Json::str("")),
        (
            "devtoolsFrontendUrl",
            Json::str(format!(
                "https://chrome-devtools-frontend.appspot.com/serve_rev/{REVISION}/inspector.html?ws={host}/devtools/page/{}",
                entry.id
            )),
        ),
        ("id", Json::str(&entry.id)),
        ("title", Json::str(&entry.title)),
        ("type", Json::str(&entry.kind)),
        ("url", Json::str(&entry.url)),
        ("webSocketDebuggerUrl", Json::str(format!("ws://{host}/devtools/page/{}", entry.id))),
    ])
}

/// O `Sec-WebSocket-Accept` da RFC 6455.
pub fn websocket_accept(key: &str) -> String {
    let mut input = key.as_bytes().to_vec();
    input.extend_from_slice(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    ul_common::codec::base64_string(&ul_common::hash::sha1(&input))
}

/// Responde `request`. `allowed_origins` é o `--remote-allow-origins` (`*` libera tudo).
pub fn respond(request: &Request, browser: &mut dyn Browser, allowed_origins: &[String]) -> Reply {
    let Some(host) = request.header("Host").map(str::to_owned).or_else(|| (request.version != "HTTP/1.0").then(String::new))
    else {
        return Reply::Close;
    };
    if !host.is_empty() && !host_is_local(&host) {
        return Reply::Http(response(
            "500 Internal Server Error",
            "",
            "text/html",
            b"Host header is specified and is not an IP address or localhost.",
        ));
    }
    let path = request.target.split('?').next().unwrap_or_default();
    let query = request.target.split_once('?').map(|(_, q)| q).unwrap_or_default();

    if let Some(rest) = path.strip_prefix("/devtools/") {
        if rest == "inspector.html" {
            return Reply::Http(response("200 OK", "", "text/html", INSPECTOR));
        }
        let upgrade = request.header("Upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
        let endpoint = if rest == format!("browser/{}", browser.browser_id()) {
            Some(Endpoint::Browser)
        } else {
            rest.strip_prefix("page/")
                .filter(|id| browser.targets().iter().any(|t| t.id == *id))
                .map(|id| Endpoint::Page(id.to_owned()))
        };
        if !upgrade {
            return Reply::Http(response("200 OK", "", "text/plain", b""));
        }
        let Some(endpoint) = endpoint else {
            return Reply::Http(response("404 Not Found", "", "text/html", b""));
        };
        if let Some(origin) = request.header("Origin") {
            if !allowed_origins.iter().any(|o| o == "*" || o == origin) {
                let body = format!(
                    "Rejected an incoming WebSocket connection from the {origin} origin. Use the command line flag \
                     --remote-allow-origins={origin} to allow connections from this origin or \
                     --remote-allow-origins=* to allow all origins."
                );
                return Reply::Http(response("403 Forbidden", "", "text/html", body.as_bytes()));
            }
        }
        let accept = websocket_accept(request.header("Sec-WebSocket-Key").unwrap_or_default());
        let head = format!(
            "HTTP/1.1 101 WebSocket Protocol Handshake\r\nUpgrade: WebSocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
        );
        return Reply::Upgrade(head.into_bytes(), endpoint);
    }

    let Some(command) = path.strip_prefix("/json").filter(|c| c.is_empty() || c.starts_with('/')) else {
        return Reply::Http(response("200 OK", "X-Frame-Options:DENY\r\n", "text/html; charset=UTF-8", b""));
    };
    let command = command.trim_start_matches('/');
    let (command, argument) = command.split_once('/').unwrap_or((command, ""));
    let argument = argument.trim_end_matches('/');
    match command {
        "" | "list" => {
            let list = Json::Array(browser.targets().iter().map(|t| target_json(t, &host)).collect());
            json_reply("200 OK", list.to_pretty().as_bytes())
        }
        "version" => {
            let version = Json::object([
                ("Browser", Json::str(PRODUCT)),
                ("Protocol-Version", Json::str(PROTOCOL_VERSION)),
                ("User-Agent", Json::str(USER_AGENT)),
                ("V8-Version", Json::str(V8_VERSION)),
                ("WebKit-Version", Json::str(format!("537.36 ({REVISION})"))),
                ("webSocketDebuggerUrl", Json::str(format!("ws://{host}/devtools/browser/{}", browser.browser_id()))),
            ]);
            json_reply("200 OK", version.to_pretty().as_bytes())
        }
        "protocol" => Reply::Http(response("200 OK", "", JSON_TYPE, PROTOCOL)),
        "new" => {
            if request.method != "PUT" {
                let body =
                    format!("Using unsafe HTTP verb {} to invoke /json/new. This action supports only PUT verb.", request.method);
                return json_reply("405 Method Not Allowed", body.as_bytes());
            }
            let url = if query.is_empty() { "about:blank".to_owned() } else { percent_decode(query) };
            let entry = browser.create(&url);
            json_reply("200 OK", target_json(&entry, &host).to_pretty().as_bytes())
        }
        "activate" | "close" => {
            let done = if command == "activate" { browser.activate(argument) } else { browser.close(argument) };
            if done {
                json_reply("200 OK", if command == "activate" { b"Target activated" } else { b"Target is closing" })
            } else {
                json_reply("404 Not Found", format!("No such target id: {argument}").as_bytes())
            }
        }
        other => json_reply("404 Not Found", format!("Unknown command: {other}").as_bytes()),
    }
}

/// O `UnescapeURLComponent` do `/json/new?URL`: só as sequências `%XX`.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(Vec<TargetEntry>);

    const BROWSER: &str = "00425c63-2389-4739-acf8-584931b00f57";
    const PAGE: &str = "BE8EC8FB741117089526801BE79E9E68";

    impl Browser for Fake {
        fn browser_id(&self) -> String {
            BROWSER.to_owned()
        }
        fn targets(&self) -> Vec<TargetEntry> {
            self.0.clone()
        }
        fn create(&mut self, url: &str) -> TargetEntry {
            let entry = TargetEntry { id: "69C2E713367A96545587566571A36209".into(), kind: "page".into(), title: String::new(), url: url.into() };
            self.0.insert(0, entry.clone());
            entry
        }
        fn activate(&mut self, id: &str) -> bool {
            self.0.iter().any(|t| t.id == id)
        }
        fn close(&mut self, id: &str) -> bool {
            self.activate(id)
        }
    }

    fn fake() -> Fake {
        Fake(vec![TargetEntry { id: PAGE.into(), kind: "page".into(), title: "about:blank".into(), url: "about:blank".into() }])
    }

    fn ask(raw: &str) -> String {
        let (request, used) = parse_request(raw.as_bytes()).unwrap();
        assert_eq!(used, raw.len());
        match respond(&request, &mut fake(), &[]) {
            Reply::Http(bytes) | Reply::Upgrade(bytes, _) => String::from_utf8(bytes).unwrap(),
            Reply::Close => String::new(),
        }
    }

    const KEY: &str = "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n";

    #[test]
    fn version_as_the_oracle() {
        let reply = ask("GET /json/version HTTP/1.1\r\nHost: 127.0.0.1:9222\r\n\r\n");
        let expected_body = format!(
            "{{\n   \"Browser\": \"Chrome/154.0.8037.92\",\n   \"Protocol-Version\": \"1.3\",\n   \"User-Agent\": \"{USER_AGENT}\",\n   \"V8-Version\": \"15.4.80.19\",\n   \"WebKit-Version\": \"537.36 (@334b65d254ccc35df4fca82706d1753227b01039)\",\n   \"webSocketDebuggerUrl\": \"ws://127.0.0.1:9222/devtools/browser/{BROWSER}\"\n}}\n"
        );
        assert_eq!(
            reply,
            format!("HTTP/1.1 200 OK\r\nContent-Security-Policy:frame-ancestors 'none'\r\nContent-Length:411\r\nContent-Type:application/json; charset=UTF-8\r\n\r\n{expected_body}")
        );
        // Sem Host, o endereço sai vazio (397 bytes no oráculo); o HTTP/1.0 sem Host não recebe resposta.
        assert!(ask("GET /json/version HTTP/1.1\r\n\r\n").contains("Content-Length:397\r\n"));
        assert_eq!(ask("GET /json/version HTTP/1.0\r\n\r\n"), "");
        assert!(ask("POST /json/version/?x=1 HTTP/1.1\r\nHost: localhost:9222\r\nContent-Length: 0\r\n\r\n").contains("ws://localhost:9222/"));
    }

    #[test]
    fn list_and_commands_as_the_oracle() {
        let list = ask("GET /json/list/ HTTP/1.1\r\nHost: 127.0.0.1:9222\r\n\r\n");
        assert!(list.ends_with(&format!(
            "[ {{\n   \"description\": \"\",\n   \"devtoolsFrontendUrl\": \"https://chrome-devtools-frontend.appspot.com/serve_rev/@334b65d254ccc35df4fca82706d1753227b01039/inspector.html?ws=127.0.0.1:9222/devtools/page/{PAGE}\",\n   \"id\": \"{PAGE}\",\n   \"title\": \"about:blank\",\n   \"type\": \"page\",\n   \"url\": \"about:blank\",\n   \"webSocketDebuggerUrl\": \"ws://127.0.0.1:9222/devtools/page/{PAGE}\"\n}} ]\n"
        )));
        let host = "Host: 127.0.0.1:9222\r\n\r\n";
        assert!(ask(&format!("PUT /json/new?about:blank HTTP/1.1\r\n{host}")).contains("Content-Length:433\r\n"));
        assert_eq!(
            ask(&format!("GET /json/new?about:blank HTTP/1.1\r\n{host}")),
            "HTTP/1.1 405 Method Not Allowed\r\nContent-Security-Policy:frame-ancestors 'none'\r\nContent-Length:83\r\nContent-Type:application/json; charset=UTF-8\r\n\r\nUsing unsafe HTTP verb GET to invoke /json/new. This action supports only PUT verb."
        );
        assert_eq!(
            ask(&format!("GET /json/nope HTTP/1.1\r\n{host}")),
            "HTTP/1.1 404 Not Found\r\nContent-Security-Policy:frame-ancestors 'none'\r\nContent-Length:21\r\nContent-Type:application/json; charset=UTF-8\r\n\r\nUnknown command: nope"
        );
        assert!(ask(&format!("GET /json/close/XYZ HTTP/1.1\r\n{host}")).ends_with("\r\n\r\nNo such target id: XYZ"));
        assert!(ask(&format!("GET /json/activate/{PAGE} HTTP/1.1\r\n{host}")).ends_with("Content-Length:16\r\nContent-Type:application/json; charset=UTF-8\r\n\r\nTarget activated"));
        assert_eq!(
            ask(&format!("GET / HTTP/1.1\r\n{host}")),
            "HTTP/1.1 200 OK\r\nX-Frame-Options:DENY\r\nContent-Length:0\r\nContent-Type:text/html; charset=UTF-8\r\n\r\n"
        );
        assert!(ask(&format!("GET /json/protocol HTTP/1.1\r\n{host}")).starts_with("HTTP/1.1 200 OK\r\nContent-Length:1578134\r\nContent-Type:application/json; charset=UTF-8\r\n\r\n{\n    \"version\""));
        assert!(ask(&format!("GET /devtools/inspector.html HTTP/1.1\r\n{host}")).starts_with("HTTP/1.1 200 OK\r\nContent-Length:990\r\nContent-Type:text/html\r\n\r\n<!--"));
    }

    #[test]
    fn websocket_and_host_as_the_oracle() {
        let host = "Host: 127.0.0.1:9222\r\n";
        let upgrade = "Upgrade: websocket\r\nConnection: Upgrade\r\n";
        assert_eq!(
            ask(&format!("GET /devtools/browser/{BROWSER} HTTP/1.1\r\n{host}{upgrade}{KEY}\r\n")),
            "HTTP/1.1 101 WebSocket Protocol Handshake\r\nUpgrade: WebSocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n"
        );
        assert_eq!(
            ask(&format!("GET /devtools/browser/nope HTTP/1.1\r\n{host}{upgrade}{KEY}\r\n")),
            "HTTP/1.1 404 Not Found\r\nContent-Length:0\r\nContent-Type:text/html\r\n\r\n"
        );
        assert_eq!(
            ask(&format!("GET /devtools/browser/{BROWSER} HTTP/1.1\r\n{host}\r\n")),
            "HTTP/1.1 200 OK\r\nContent-Length:0\r\nContent-Type:text/plain\r\n\r\n"
        );
        assert_eq!(
            ask("GET /json/version HTTP/1.1\r\nHost: example.com\r\n\r\n"),
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length:63\r\nContent-Type:text/html\r\n\r\nHost header is specified and is not an IP address or localhost."
        );
        let rejected = ask(&format!("GET /devtools/browser/{BROWSER} HTTP/1.1\r\n{host}Origin: http://evil.com\r\n{upgrade}{KEY}\r\n"));
        assert!(rejected.starts_with("HTTP/1.1 403 Forbidden\r\nContent-Length:227\r\nContent-Type:text/html\r\n\r\nRejected an incoming"));
    }
}
