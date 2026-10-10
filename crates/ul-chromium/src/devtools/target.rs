//! Um alvo `page`: a aba com o seu programa JavaScript, numa thread própria do processo (como o renderer do
//! Chromium vive à parte do navegador). Conversa com o laço do navegador por dois pipes de linhas: comandos chegam como
//! `M<json>` (uma mensagem do protocolo, sempre com `sessionId`), `D<sessão>` (a sessão saiu) e `X` (fechar a aba);
//! saem `M<json>` (resposta ou evento, com `sessionId`) e `I<json>` (título e URL novos, para o `Target.targetInfoChanged`).
//! Os domínios que dependem do carregamento (`Page`) ficam aqui em Rust; o resto vai ao agente JavaScript da página.

use sysabi::{Fd, OFlags, PollEvents, PollFd};
use zjsc::api::embed::{HostArg, HostHandle, PageRunner};

use super::json::{self, Json};
use super::{monotonic_seconds, random_i64, token};
use crate::fetch;
use crate::page::Page;

/// As pontas do laço do navegador: escreve comandos em `commands` (sem bloquear) e lê saídas de `events`.
pub struct Link {
    pub commands: Fd,
    pub events: Fd,
}

/// Abre a aba `target_id` numa thread nova. A primeira aba do processo (`startup`) carrega `url` direto; a aba criada
/// pelo protocolo nasce com o documento vazio e navega para `url` em seguida, e com `wait` (o
/// `waitForDebuggerOnStart`) a navegação espera o `Runtime.runIfWaitingForDebugger`.
pub fn spawn(target_id: String, url: String, startup: bool, wait: bool) -> sysabi::SysResult<Link> {
    let sys = sysabi::sys::current();
    let (command_read, command_write) = sys.pipe2(OFlags::CLOEXEC | OFlags::NONBLOCK)?;
    let (event_read, event_write) = sys.pipe2(OFlags::CLOEXEC)?;
    sysio::thread::spawn(move || {
        let mut tab = Tab::new(target_id, command_read, event_write);
        tab.waiting = wait;
        if startup || url == "about:blank" {
            tab.run(url);
        } else {
            tab.held = Some(url);
            tab.run("about:blank".to_owned());
        }
        let _ = sysabi::sys::close(command_read);
        let _ = sysabi::sys::close(event_write);
    });
    Ok(Link { commands: command_write, events: event_read })
}

/// O que encerra o laço de uma página carregada.
enum Next {
    /// `Page.navigate` ou `Page.reload`: a mensagem a responder quando a navegação comprometer.
    Navigate(String, Option<Json>),
    /// A primeira carga da aba (sem eventos de início de navegação).
    Initial(String),
    Close,
}

struct Tab {
    target_id: String,
    input: Fd,
    output: Fd,
    inbox: Vec<u8>,
    /// Comandos lidos e ainda não tratados (os que chegaram enquanto a navegação carregava).
    queue: std::collections::VecDeque<String>,
    isolate_id: String,
    next_context: i64,
    loader_id: String,
    url: String,
    title: String,
    page_enabled: Vec<String>,
    lifecycle_enabled: Vec<String>,
    runtime_enabled: Vec<String>,
    /// Os marcos do carregador corrente com o seu `timestamp`, repetidos a quem liga os eventos de ciclo de vida depois.
    lifecycle: Vec<(String, f64)>,
    context: Option<(i64, String)>,
    closed: bool,
    /// A navegação que a aba nova ainda vai fazer.
    held: Option<String>,
    /// Pausada até o `Runtime.runIfWaitingForDebugger`.
    waiting: bool,
    /// Os marcos de rede ociosa pendentes: (quando saem, o `timestamp` deles, se houve pintura).
    idle: Option<(f64, f64, bool)>,
}

/// Quanto tempo sem rede o Chromium espera antes de declarar `networkIdle` (a janela do `IdlenessDetector`).
const NETWORK_QUIET_SECONDS: f64 = 1.0;

/// Quanto o laço da página espera por comando antes de deixar o relógio andar.
const TICK_MS: u64 = 4;

fn message_session(message: &Json) -> String {
    message.get("sessionId").and_then(Json::as_str).unwrap_or_default().to_owned()
}

impl Tab {
    fn new(target_id: String, input: Fd, output: Fd) -> Tab {
        Tab {
            target_id,
            input,
            output,
            inbox: Vec::new(),
            queue: Default::default(),
            isolate_id: random_i64().unsigned_abs().to_string(),
            next_context: 1,
            loader_id: token(),
            url: String::new(),
            title: String::new(),
            page_enabled: Vec::new(),
            lifecycle_enabled: Vec::new(),
            runtime_enabled: Vec::new(),
            lifecycle: Vec::new(),
            context: None,
            closed: false,
            held: None,
            waiting: false,
            idle: None,
        }
    }

    fn write_line(&self, kind: char, text: &str) {
        let mut bytes = Vec::with_capacity(text.len() + 2);
        bytes.push(kind as u8);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(b'\n');
        let mut at = 0;
        while at < bytes.len() {
            match sysabi::sys::write(self.output, &bytes[at..]) {
                Ok(n) => at += n,
                Err(sysabi::Errno::EINTR) => {}
                Err(_) => return,
            }
        }
    }

    fn send(&self, message: &Json) {
        self.write_line('M', &message.to_compact());
    }

    fn reply(&self, request: &Json, result: Json) {
        let mut fields = vec![("id".to_owned(), request.get("id").cloned().unwrap_or(Json::Null)), ("result".to_owned(), result)];
        if let Some(session) = request.get("sessionId") {
            fields.push(("sessionId".to_owned(), session.clone()));
        }
        self.send(&Json::Object(fields));
    }

    /// Um evento para cada sessão da lista.
    fn emit(&self, sessions: &[String], method: &str, params: &Json) {
        for session in sessions {
            self.send(&Json::object([("method", Json::str(method)), ("params", params.clone()), ("sessionId", Json::str(session))]));
        }
    }

    fn lifecycle_event(&mut self, name: &str, timestamp: f64) {
        self.lifecycle.push((name.to_owned(), timestamp));
        let params = Json::object([
            ("frameId", Json::str(&self.target_id)),
            ("loaderId", Json::str(&self.loader_id)),
            ("name", Json::str(name)),
            ("timestamp", Json::Number(timestamp)),
        ]);
        self.emit(&self.lifecycle_enabled.clone(), "Page.lifecycleEvent", &params);
    }

    fn frame(&self, unreachable: Option<&str>) -> Json {
        let mut fields = vec![
            ("id", Json::str(&self.target_id)),
            ("loaderId", Json::str(&self.loader_id)),
            ("url", Json::str(&self.url)),
            ("domainAndRegistry", Json::str(domain_and_registry(&self.url))),
            ("securityOrigin", Json::str(security_origin(&self.url))),
            ("securityOriginDetails", Json::object([("isLocalhost", Json::Bool(is_localhost(&self.url)))])),
            ("mimeType", Json::str("text/html")),
        ];
        if let Some(url) = unreachable {
            fields.push(("unreachableUrl", Json::str(url)));
        }
        fields.extend([
            ("adFrameStatus", Json::object([("adFrameType", Json::str("none"))])),
            ("secureContextType", Json::str(secure_context_type(&self.url))),
            ("crossOriginIsolatedContextType", Json::str("NotIsolated")),
            ("gatedAPIFeatures", Json::Array(Vec::new())),
        ]);
        Json::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }

    /// Lê o que o laço do navegador mandou, esperando até `wait_ms`. Falso quando o pipe fechou.
    fn receive(&mut self, wait_ms: u64) -> bool {
        let mut fds = [PollFd { fd: self.input, events: PollEvents::IN, revents: PollEvents::empty() }];
        let sys = sysabi::sys::current();
        if !matches!(sys.poll(&mut fds, Some(std::time::Duration::from_millis(wait_ms))), Ok(n) if n > 0) {
            return true;
        }
        let mut buffer = [0u8; 65536];
        loop {
            match sysabi::sys::read(self.input, &mut buffer) {
                Ok(0) => return false,
                Ok(n) => self.inbox.extend_from_slice(&buffer[..n]),
                Err(_) => break,
            }
        }
        while let Some(end) = self.inbox.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.inbox.drain(..=end).collect();
            self.queue.push_back(String::from_utf8_lossy(&line[..line.len() - 1]).into_owned());
        }
        true
    }

    fn run(mut self, url: String) {
        let mut next = Next::Initial(url);
        loop {
            next = match next {
                Next::Navigate(url, request) => self.load(url, request, true),
                Next::Initial(url) => self.load(url, None, false),
                Next::Close => return,
            };
            if self.closed {
                return;
            }
        }
    }

    /// Carrega `url` e roda a página até a próxima navegação. `announce`: os eventos de início de navegação saem
    /// (toda navegação menos a primeira carga da aba).
    fn load(&mut self, url: String, request: Option<Json>, announce: bool) -> Next {
        self.loader_id = token();
        let started = Json::object([
            ("frameId", Json::str(&self.target_id)),
            ("url", Json::str(&url)),
            ("loaderId", Json::str(&self.loader_id)),
            ("navigationType", Json::str("differentDocument")),
        ]);
        if announce {
            let sessions = self.page_enabled.clone();
            self.emit(&sessions, "Page.frameStartedNavigating", &started);
            self.emit(&sessions, "Page.frameStartedLoading", &Json::object([("frameId", Json::str(&self.target_id))]));
        }
        let (page, error) = match Page::load(&url, fetch::network()) {
            Ok(page) => (page, None),
            Err(error) => {
                // A navegação que falha compromete a página de erro do Chromium no lugar do documento.
                let page = Page::error_page(&url, &error, fetch::network()).unwrap_or_else(|| Page::load("about:blank", fetch::network()).expect("about:blank"));
                (page, Some(error))
            }
        };
        self.url = if error.is_some() { "chrome-error://chromewebdata/".to_owned() } else { page.bridge.url.borrow().clone() };
        if let Some(request) = &request {
            let mut result = vec![("frameId", Json::str(&self.target_id)), ("loaderId", Json::str(&self.loader_id))];
            if let Some(error) = &error {
                result.push(("errorText", Json::str(error)));
            }
            result.push(("isDownload", Json::Bool(false)));
            self.reply(request, Json::Object(result.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()));
        }
        let unreachable = error.as_ref().map(|_| url.clone());
        let tab = std::cell::RefCell::new(&mut *self);
        page.run_with(
            |runner, hook| tab.borrow_mut().commit(runner, hook, unreachable.as_deref()),
            |runner, hook| tab.borrow_mut().serve(runner, hook, &page),
        )
        .unwrap_or(Next::Close)
    }

    /// O documento novo comprometeu: contexto de execução novo, `init` e `frameNavigated`. O Chromium manda
    /// `executionContextsCleared` duas vezes (a do documento que sai e a do mundo isolado) e nenhum
    /// `executionContextDestroyed` (medido no oráculo).
    fn commit(&mut self, runner: &PageRunner, hook: &HostHandle, unreachable: Option<&str>) {
        self.lifecycle.clear();
        self.idle = None;
        self.lifecycle_event("init", monotonic_seconds());
        if self.context.take().is_some() {
            let sessions = self.runtime_enabled.clone();
            for _ in 0..2 {
                self.emit(&sessions, "Runtime.executionContextsCleared", &Json::object([]));
            }
        }
        let navigated = Json::object([("frame", self.frame(unreachable)), ("type", Json::str("Navigation"))]);
        self.emit(&self.page_enabled.clone(), "Page.frameNavigated", &navigated);
        let id = self.next_context;
        self.next_context += 1;
        let unique = format!("{}.{}", random_i64(), random_i64());
        self.context = Some((id, unique.clone()));
        let config = Json::object([
            ("isolateId", Json::str(&self.isolate_id)),
            ("contextId", Json::Number(id as f64)),
            ("uniqueId", Json::str(unique)),
            ("frameId", Json::str(&self.target_id)),
            ("origin", Json::str(security_origin(&self.url))),
            ("enabled", Json::Array(self.runtime_enabled.iter().map(Json::str).collect())),
        ]);
        let _ = runner.call_hidden(hook, &[HostArg::Str("cdpInit"), HostArg::Str(&config.to_compact())]);
        self.drain(runner, hook);
        self.title.clear();
        self.announce();
    }

    /// Título e URL para o `TargetInfo`. Sem `<title>`, o título é a URL; a aba que ainda vai navegar mostra o destino.
    fn announce(&self) {
        let url = self.held.as_deref().unwrap_or(&self.url);
        let title = if self.title.is_empty() { url } else { &self.title };
        self.write_line('I', &Json::object([("url", Json::str(url)), ("title", Json::str(title))]).to_compact());
    }

    /// Os marcos de rede ociosa e a primeira pintura significativa, que o Chromium só decide depois de um tempo sem
    /// rede, mas datados do momento em que a rede parou.
    fn idle_events(&mut self) {
        let Some((due, stamp, contentful)) = self.idle else { return };
        if monotonic_seconds() < due {
            return;
        }
        self.idle = None;
        self.lifecycle_event("networkAlmostIdle", stamp);
        if contentful {
            let paint = self.lifecycle.iter().find(|(name, _)| name == "firstPaint").map_or(stamp, |(_, t)| *t);
            self.lifecycle_event("firstMeaningfulPaint", paint);
        }
        self.lifecycle_event("networkIdle", stamp);
    }

    /// Recolhe a caixa de saída do agente: respostas e eventos seguem; os marcos do carregamento viram eventos de `Page`.
    fn drain(&mut self, runner: &PageRunner, hook: &HostHandle) {
        let Some(text) = runner.call_hidden(hook, &[HostArg::Str("cdpDrain")]).ok().and_then(|h| h.as_text()) else { return };
        for line in text.split('\n').filter(|l| !l.is_empty()) {
            let Ok(item) = json::parse(line) else { continue };
            let Some(name) = item.get("$lifecycle").and_then(Json::as_str) else {
                self.write_line('M', line);
                continue;
            };
            let now = monotonic_seconds();
            let sessions = self.page_enabled.clone();
            match name {
                "DOMContentLoaded" => {
                    self.emit(&sessions, "Page.domContentEventFired", &Json::object([("timestamp", Json::Number(now))]));
                    self.lifecycle_event("DOMContentLoaded", now);
                }
                _ => {
                    self.title = item.get("title").and_then(Json::as_str).unwrap_or_default().to_owned();
                    self.announce();
                    self.emit(&sessions, "Page.loadEventFired", &Json::object([("timestamp", Json::Number(now))]));
                    self.lifecycle_event("load", now);
                    self.emit(&sessions, "Page.frameStoppedLoading", &Json::object([("frameId", Json::str(&self.target_id))]));
                    let paint = monotonic_seconds();
                    let contentful = item.get("contentful").and_then(Json::as_bool) == Some(true);
                    if contentful {
                        for event in ["firstMeaningfulPaintCandidate", "firstContentfulPaint", "firstPaint"] {
                            self.lifecycle_event(event, paint);
                        }
                        if item.get("images").and_then(Json::as_bool) == Some(true) {
                            self.lifecycle_event("firstImagePaint", paint);
                        }
                    }
                    self.idle = Some((now + NETWORK_QUIET_SECONDS, now, contentful));
                }
            }
        }
    }

    /// O laço da página carregada: comandos, o relógio da página e a caixa de saída, até a próxima navegação.
    fn serve(&mut self, runner: &PageRunner, hook: &HostHandle, page: &Page) -> Next {
        let mut last = monotonic_seconds();
        loop {
            self.drain(runner, hook);
            self.idle_events();
            if let Some(url) = page.pending_navigation() {
                return Next::Navigate(url, None);
            }
            if !self.waiting {
                if let Some(url) = self.held.take() {
                    return Next::Navigate(url, None);
                }
            }
            while let Some(line) = self.queue.pop_front() {
                if let Some(next) = self.command(runner, hook, &line) {
                    return next;
                }
                self.drain(runner, hook);
            }
            if !self.receive(TICK_MS) {
                self.closed = true;
                return Next::Close;
            }
            let now = monotonic_seconds();
            runner.run_event_loop_for((now - last) * 1000.0);
            last = now;
        }
    }

    /// Trata uma linha do laço do navegador; `Some` encerra a página corrente.
    fn command(&mut self, runner: &PageRunner, hook: &HostHandle, line: &str) -> Option<Next> {
        let (kind, rest) = line.split_at(1.min(line.len()));
        match kind {
            "X" => {
                self.closed = true;
                return Some(Next::Close);
            }
            "D" => {
                for list in [&mut self.page_enabled, &mut self.lifecycle_enabled, &mut self.runtime_enabled] {
                    list.retain(|s| s != rest);
                }
                return None;
            }
            _ => {}
        }
        let Ok(message) = json::parse(rest) else { return None };
        let method = message.get("method").and_then(Json::as_str).unwrap_or_default().to_owned();
        let session = message_session(&message);
        let params = message.get("params").cloned().unwrap_or(Json::Object(Vec::new()));
        match method.as_str() {
            "Page.enable" => {
                if !self.page_enabled.contains(&session) {
                    self.page_enabled.push(session);
                }
                self.reply(&message, Json::object([]));
            }
            "Page.disable" => {
                self.page_enabled.retain(|s| *s != session);
                self.lifecycle_enabled.retain(|s| *s != session);
                self.reply(&message, Json::object([]));
            }
            "Page.getFrameTree" => self.reply(&message, Json::object([("frameTree", Json::object([("frame", self.frame(None))]))])),
            "Page.setLifecycleEventsEnabled" => {
                let enabled = params.get("enabled").and_then(Json::as_bool) == Some(true);
                self.lifecycle_enabled.retain(|s| *s != session);
                if enabled {
                    self.lifecycle_enabled.push(session.clone());
                    // O Chromium repete à sessão nova os marcos que o carregador corrente já passou (o `init` sai como
                    // `commit`).
                    for (name, timestamp) in self.lifecycle.clone() {
                        let name = if name == "init" { "commit".to_owned() } else { name };
                        let params = Json::object([
                            ("frameId", Json::str(&self.target_id)),
                            ("loaderId", Json::str(&self.loader_id)),
                            ("name", Json::str(name)),
                            ("timestamp", Json::Number(timestamp)),
                        ]);
                        self.emit(std::slice::from_ref(&session), "Page.lifecycleEvent", &params);
                    }
                }
                self.reply(&message, Json::object([]));
            }
            "Page.navigate" => {
                let url = params.get("url").and_then(Json::as_str).unwrap_or_default().to_owned();
                return Some(Next::Navigate(url, Some(message)));
            }
            "Page.reload" => {
                self.reply(&message, Json::object([]));
                return Some(Next::Navigate(self.url.clone(), None));
            }
            _ => {
                if method == "Runtime.enable" && !self.runtime_enabled.contains(&session) {
                    self.runtime_enabled.push(session);
                } else if method == "Runtime.disable" {
                    self.runtime_enabled.retain(|s| *s != session);
                } else if method == "Runtime.runIfWaitingForDebugger" {
                    self.waiting = false;
                }
                let _ = runner.call_hidden(hook, &[HostArg::Str("cdp"), HostArg::Str(rest)]);
            }
        }
        None
    }
}

/// A origem como o `url::Origin::Serialize()` do Chromium: `esquema://host[:porta]`, e `://` para origem opaca.
fn security_origin(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else { return "://".to_owned() };
    match parsed.scheme() {
        "http" | "https" | "ws" | "wss" | "ftp" => {
            let host = parsed.host_str().unwrap_or_default();
            match parsed.port() {
                Some(port) => format!("{}://{host}:{port}", parsed.scheme()),
                None => format!("{}://{host}", parsed.scheme()),
            }
        }
        _ => "://".to_owned(),
    }
}

fn is_localhost(url: &str) -> bool {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_owned)).is_some_and(|h| h == "localhost" || h == "127.0.0.1" || h == "[::1]" || h.ends_with(".localhost"))
}

fn secure_context_type(url: &str) -> &'static str {
    match url::Url::parse(url).map(|u| u.scheme().to_owned()).unwrap_or_default().as_str() {
        "https" | "wss" | "file" => "Secure",
        "http" if is_localhost(url) => "SecureLocalhost",
        _ => "InsecureScheme",
    }
}

/// O domínio registrável do host da URL (`registry::domain_and_registry`).
fn domain_and_registry(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(super::registry::domain_and_registry)).unwrap_or_default()
}
