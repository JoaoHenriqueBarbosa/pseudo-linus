//! Um alvo `page`: a aba com o seu programa JavaScript, numa thread própria do processo (como o renderer do
//! Chromium vive à parte do navegador). Conversa com o laço do navegador por dois pipes de linhas: comandos chegam como
//! `M<json>` (uma mensagem do protocolo, sempre com `sessionId`), `D<sessão>` (a sessão saiu) e `X` (fechar a aba);
//! saem `M<json>` (resposta ou evento, com `sessionId`) e `I<json>` (título e URL novos, para o `Target.targetInfoChanged`).
//! Os domínios que dependem do carregamento (`Page`) ficam aqui em Rust; o resto vai ao agente JavaScript da página.

use sysabi::{Fd, OFlags, PollEvents, PollFd};
use zjsc::api::embed::{HostArg, HostHandle, PageRunner};

use super::json::{self, Json};
use super::{monotonic_seconds, random_i64, token};
use super::overrides::{self, NetOverrides, Observation, Observed};
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
pub fn spawn(target_id: String, url: String, startup: bool, wait: bool, profile: std::sync::Arc<crate::session::Profile>) -> sysabi::SysResult<Link> {
    let sys = sysabi::sys::current();
    let (command_read, command_write) = sys.pipe2(OFlags::CLOEXEC | OFlags::NONBLOCK)?;
    let (event_read, event_write) = sys.pipe2(OFlags::CLOEXEC)?;
    sysio::thread::spawn(move || {
        let mut tab = Tab::new(target_id, command_read, event_write, profile);
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

/// Uma entrada do histórico de navegação.
struct HistoryEntry {
    id: i64,
    url: String,
    /// O que o usuário digitou: vazio na navegação que falhou.
    typed: String,
    title: String,
    transition: &'static str,
}

/// Como uma carga entra no histórico.
#[derive(Clone, Copy, PartialEq)]
enum NavKind {
    /// `Page.navigate` e a primeira carga: entrada nova, `typed`.
    Typed,
    /// Navegação da página (script, link): entrada nova, `link`.
    Link,
    /// `Page.reload`: a entrada corrente fica.
    Reload,
    /// `Page.navigateToHistoryEntry`: só a entrada corrente muda.
    History(usize),
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
    network_enabled: Vec<String>,
    /// O `dataReceived` e o `loadingFinished` do documento, que saem depois do contexto de execução novo: os bytes
    /// do corpo e os do fio.
    finishing: Option<(usize, usize)>,
    /// O que o `Emulation`, a janela e os mundos da aba guardam entre os documentos.
    emu: Emulation,
    /// As substituições de rede do DevTools, que o fetcher da aba lê a cada pedido.
    net: std::rc::Rc<std::cell::RefCell<NetOverrides>>,
    /// O histórico de navegação da aba (`Page.getNavigationHistory`) e a entrada corrente.
    history: Vec<HistoryEntry>,
    current: usize,
    next_entry: i64,
    /// Como a próxima carga entra no histórico.
    nav: NavKind,
    /// Uma navegação de histórico em curso: o `Page.getNavigationHistory` que chega antes dela comprometer falha.
    history_busy: bool,
    /// A carga corrente restaura uma entrada do histórico (o `Page.backForwardCacheNotUsed` sai no compromisso).
    restoring: bool,
    /// Os ids de contexto dos mundos isolados do documento corrente.
    world_ids: Vec<i64>,
    /// Quantos subrecursos (favicon incluído) esta aba já relatou: o contador do `requestId` `pid.n`.
    resource_requests: u32,
    /// Os subrecursos que o fetcher da aba viu e ainda faltam relatar.
    observed: Observed,
    /// O HTML do documento corrente, para o `initiator` de parser.
    source: String,
    /// Os subrecursos de parser que o documento corrente já pediu (o Chromium pede cada um uma vez).
    requested: Vec<(crate::fetch::Kind, String)>,
    security_enabled: Vec<String>,
    /// Os marcos do carregador corrente com o seu `timestamp`, repetidos a quem liga os eventos de ciclo de vida depois.
    lifecycle: Vec<(String, f64)>,
    context: Option<(i64, String)>,
    closed: bool,
    /// A navegação que a aba nova ainda vai fazer.
    held: Option<String>,
    /// Os endereços pedidos antes do final, um por redirecionamento do servidor (o `TargetInfo` do navegador os leva ao History).
    redirects: Vec<String>,
    /// Pausada até o `Runtime.runIfWaitingForDebugger`.
    waiting: bool,
    /// Os marcos de rede ociosa pendentes: (quando saem, o `timestamp` deles, se houve pintura).
    idle: Option<(f64, f64, bool)>,
    /// O storage da aba e do perfil do contexto dela, que atravessa as navegações.
    session: std::rc::Rc<crate::session::Tab>,
}

/// Quanto tempo sem rede o Chromium espera antes de declarar `networkIdle` (a janela do `IdlenessDetector`).
const NETWORK_QUIET_SECONDS: f64 = 1.0;

/// Quanto o laço da página espera por comando antes de deixar o relógio andar.
const TICK_MS: u64 = 4;

/// O estado da aba que sobrevive à navegação: a janela (`Browser.setWindowBounds`), o `Emulation`, os scripts de documento
/// novo e os mundos isolados. O agente de JS recebe o que lhe interessa a cada documento (`__emulation`).
struct Emulation {
    /// O tamanho da janela, com a moldura: 780x580 no headless do oráculo (o conteúdo é a janela menos 143 de altura).
    window: (i64, i64),
    /// `Emulation.setDeviceMetricsOverride`: largura, altura e escala do conteúdo.
    metrics: Option<(i64, i64, f64)>,
    touch: i64,
    media: String,
    features: Vec<(String, String)>,
    /// `Page.addScriptToEvaluateOnNewDocument`: identificador, fonte e mundo (vazio é o principal).
    scripts: Vec<(String, String, String)>,
    next_script: u32,
    /// Os mundos de `Page.createIsolatedWorld`, que sobrevivem à navegação no mesmo processo.
    created: Vec<String>,
    /// O documento novo troca de processo (sai de um documento que não é `about:blank`): os ids de contexto recomeçam e
    /// os mundos criados caem (medido no oráculo, `playwright_handshake`).
    swap: bool,
    /// Quantos documentos a aba já carregou (o `about:blank` inicial conta).
    loads: u32,
    /// `Emulation.setUserAgentOverride` com `platform`: o `navigator.platform`.
    nav_platform: String,
    /// `Emulation.setLocaleOverride`. Guardado e devolvido; o `Intl` do motor ainda não o lê.
    locale: String,
    /// `Emulation.setTimezoneOverride`: vale para o documento seguinte (o motor resolve o fuso ao criar a página).
    timezone: String,
    /// `Emulation.setGeolocationOverride`: latitude, longitude e precisão.
    geo: Option<(f64, f64, f64)>,
    /// `Runtime.addBinding`: os nomes que o agente instala em cada documento.
    bindings: Vec<String>,
}

impl Default for Emulation {
    fn default() -> Self {
        Emulation { window: (780, 580), metrics: None, touch: 0, media: String::new(), features: Vec::new(), scripts: Vec::new(), next_script: 1, created: Vec::new(), swap: false, loads: 0, nav_platform: String::new(), locale: String::new(), timezone: String::new(), geo: None, bindings: Vec::new() }
    }
}

impl Emulation {
    /// Os nomes dos mundos do documento novo: os dos scripts registrados e depois os criados, sem repetição.
    fn world_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for name in self.scripts.iter().map(|(_, _, world)| world).chain(&self.created) {
            if !name.is_empty() && !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
    }

    fn json(&self, worlds: &[i64], net: &NetOverrides) -> Json {
        let metrics = self.metrics.map_or(Json::Null, |(w, h, scale)| Json::object([("width", Json::Number(w as f64)), ("height", Json::Number(h as f64)), ("scale", Json::Number(scale))]));
        Json::object([
            ("window", Json::Array(vec![Json::Number(self.window.0 as f64), Json::Number(self.window.1 as f64)])),
            ("override", metrics),
            ("touch", Json::Number(self.touch as f64)),
            ("media", Json::str(&self.media)),
            ("features", Json::Object(self.features.iter().map(|(k, v)| (k.clone(), Json::str(v))).collect())),
            ("worlds", Json::Array(worlds.iter().map(|id| Json::Number(*id as f64)).collect())),
            ("userAgent", Json::str(&net.user_agent)),
            ("acceptLanguage", Json::str(&net.accept_language)),
            ("platform", Json::str(&self.nav_platform)),
            ("offline", Json::Bool(net.offline)),
            ("geo", self.geo.map_or(Json::Null, |(latitude, longitude, accuracy)| Json::object([("latitude", Json::Number(latitude)), ("longitude", Json::Number(longitude)), ("accuracy", Json::Number(accuracy))]))),
            ("bindings", Json::Array(self.bindings.iter().map(Json::str).collect())),
        ])
    }
}

fn message_session(message: &Json) -> String {
    message.get("sessionId").and_then(Json::as_str).unwrap_or_default().to_owned()
}

impl Tab {
    fn new(target_id: String, input: Fd, output: Fd, profile: std::sync::Arc<crate::session::Profile>) -> Tab {
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
            network_enabled: Vec::new(),
            finishing: None,
            emu: Emulation::default(),
            net: Default::default(),
            history: vec![HistoryEntry { id: 1, url: "about:blank".to_owned(), typed: "about:blank".to_owned(), title: String::new(), transition: "auto_toplevel" }],
            current: 0,
            next_entry: 2,
            nav: NavKind::Typed,
            history_busy: false,
            restoring: false,
            world_ids: Vec::new(),
            resource_requests: 0,
            observed: Default::default(),
            source: String::new(),
            requested: Vec::new(),
            security_enabled: Vec::new(),
            lifecycle: Vec::new(),
            context: None,
            closed: false,
            held: None,
            redirects: Vec::new(),
            waiting: false,
            idle: None,
            session: std::rc::Rc::new(crate::session::Tab::new(profile)),
        }
    }

    /// O fetcher da aba: a rede com as substituições do DevTools e o pote de cookies do perfil.
    fn fetcher(&self) -> fetch::Fetcher {
        overrides::fetcher(self.net.clone(), self.session.profile.clone(), self.observed.clone())
    }

    /// Os cookies que o pedido a `url` levou, como o `associatedCookies` do `requestWillBeSentExtraInfo`.
    fn associated_cookies(&self, url: &str) -> Json {
        crate::cookies::associated(&self.session.profile.cookies.lock().unwrap_or_else(std::sync::PoisonError::into_inner), url)
    }

    /// A carga que acaba de comprometer entra no histórico (ou só move a entrada corrente).
    fn record_entry(&mut self, kind: NavKind, url: &str, failed: bool) {
        match kind {
            NavKind::Reload => {}
            NavKind::History(index) => self.current = index,
            NavKind::Typed | NavKind::Link => {
                // A primeira carga de uma aba nova em `about:blank` é a entrada que ela já tem.
                if kind == NavKind::Typed && url == "about:blank" && self.history.len() == 1 {
                    return;
                }
                self.history.truncate(self.current + 1);
                let id = self.next_entry;
                self.next_entry += 1;
                let typed = if failed { String::new() } else { url.to_owned() };
                self.history.push(HistoryEntry { id, url: url.to_owned(), typed, title: String::new(), transition: if kind == NavKind::Typed { "typed" } else { "link" } });
                self.current = self.history.len() - 1;
            }
        }
    }

    /// `Page.getNavigationHistory`.
    fn history_json(&self) -> Json {
        let entries = self.history.iter().map(|e| {
            Json::object([
                ("id", Json::Number(e.id as f64)),
                ("url", Json::str(&e.url)),
                ("userTypedURL", Json::str(&e.typed)),
                ("title", Json::str(&e.title)),
                ("transitionType", Json::str(e.transition)),
            ])
        });
        Json::object([("currentIndex", Json::Number(self.current as f64)), ("entries", Json::Array(entries.collect()))])
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

    fn fail(&self, request: &Json, code: i64, text: &str) {
        let error = Json::object([("code", Json::Number(code as f64)), ("message", Json::str(text))]);
        let mut fields = vec![("id".to_owned(), request.get("id").cloned().unwrap_or(Json::Null)), ("error".to_owned(), error)];
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
        let kind = std::mem::replace(&mut self.nav, NavKind::Link);
        self.loader_id = token();
        let started = Json::object([
            ("frameId", Json::str(&self.target_id)),
            ("url", Json::str(&url)),
            ("loaderId", Json::str(&self.loader_id)),
            ("navigationType", Json::str(match kind {
                NavKind::Reload => "reload",
                NavKind::History(_) => "historyDifferentDocument",
                _ => "differentDocument",
            })),
        ]);
        if announce {
            let sessions = self.page_enabled.clone();
            self.emit(&sessions, "Page.frameStartedNavigating", &started);
            self.emit(&sessions, "Page.frameStartedLoading", &Json::object([("frameId", Json::str(&self.target_id))]));
        }
        // O motor resolve o fuso ao criar a página: o override do DevTools vale daqui em diante.
        zjsc::runtime::process_time_zone::set_time_zone_spec_override((!self.emu.timezone.is_empty()).then_some(self.emu.timezone.as_str()));
        let fetcher = self.fetcher();
        let (page, error) = match Page::load(&url, fetcher.clone()) {
            Ok(page) => (page, None),
            Err(error) => {
                // A navegação que falha compromete a página de erro do Chromium no lugar do documento.
                let page = Page::error_page(&url, &error, fetcher.clone()).unwrap_or_else(|| Page::load("about:blank", fetcher.clone()).expect("about:blank"));
                (page, Some(error))
            }
        };
        self.record_entry(kind, &url, error.is_some());
        self.source = page.source().to_owned();
        self.requested.clear();
        self.restoring = matches!(kind, NavKind::History(_));
        page.enter_tab(self.session.clone());
        let next_url = if error.is_some() { "chrome-error://chromewebdata/".to_owned() } else { page.bridge.url.borrow().clone() };
        // Troca de processo (medido no oráculo): a primeira navegação da aba fica no processo do `about:blank` inicial, o
        // `about:blank` nunca troca, e o resto troca quando o documento novo tem origem opaca (data, página de erro) ou
        // outra origem (http para http da mesma origem não).
        let opaque = |url: &str| security_origin(url) == "://";
        self.emu.loads += 1;
        self.emu.swap = self.emu.loads >= 3 && next_url != "about:blank" && (opaque(&next_url) || security_origin(&self.url) != security_origin(&next_url));
        self.network_document(&url, page.net_meta(), error.as_deref());
        self.redirects = page.redirects();
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
        if std::mem::take(&mut self.history_busy) {
            // O `Page.getNavigationHistory` que chegou com a navegação de histórico ainda em voo falha (medido no oráculo).
            self.receive(30);
            let mut kept = std::collections::VecDeque::new();
            for line in std::mem::take(&mut self.queue) {
                match json::parse(line.get(1..).unwrap_or_default()) {
                    Ok(message) if message.get("method").and_then(Json::as_str) == Some("Page.getNavigationHistory") => self.fail(&message, -32000, "Not attached to an active page"),
                    _ => kept.push_back(line),
                }
            }
            self.queue = kept;
        }
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
        self.emit(&self.network_enabled.clone(), "Network.policyUpdated", &Json::object([]));
        if std::mem::take(&mut self.restoring) {
            self.emit_back_forward_cache_not_used();
        }
        self.emit_security_state(&self.security_enabled.clone());
        if std::mem::take(&mut self.emu.swap) {
            self.next_context = 1;
            self.emu.created.clear();
        }
        let id = self.next_context;
        self.next_context += 1;
        let unique = format!("{}.{}", random_i64(), random_i64());
        self.context = Some((id, unique.clone()));
        let worlds: Vec<(String, i64)> = self.emu.world_names().into_iter().map(|name| {
            let world = self.next_context;
            self.next_context += 1;
            (name, world)
        }).collect();
        let config = Json::object([
            ("isolateId", Json::str(&self.isolate_id)),
            ("contextId", Json::Number(id as f64)),
            ("uniqueId", Json::str(unique)),
            ("frameId", Json::str(&self.target_id)),
            ("origin", Json::str(security_origin(&self.url))),
            ("enabled", Json::Array(self.runtime_enabled.iter().map(Json::str).collect())),
        ]);
        let _ = runner.call_hidden(hook, &[HostArg::Str("cdpInit"), HostArg::Str(&config.to_compact())]);
        self.world_ids = worlds.iter().map(|(_, id)| *id).collect();
        self.push_emulation(runner, hook);
        self.drain(runner, hook);
        let sessions = self.runtime_enabled.clone();
        for (name, world) in &worlds {
            self.emit_world(&sessions, name, *world);
        }
        for (_, source, world) in self.emu.scripts.clone() {
            if world.is_empty() {
                let message = Json::object([("method", Json::str("__runScript")), ("params", Json::object([("source", Json::str(source))]))]);
                let _ = runner.call_hidden(hook, &[HostArg::Str("cdp"), HostArg::Str(&message.to_compact())]);
            }
        }
        self.drain(runner, hook);
        if let Some((data, encoded)) = self.finishing.take() {
            let sessions = self.network_enabled.clone();
            let id = Json::str(&self.loader_id);
            let params = Json::object([("requestId", id.clone()), ("timestamp", Json::Number(monotonic_seconds())), ("dataLength", Json::Number(data as f64)), ("encodedDataLength", Json::Number(0.0))]);
            self.emit(&sessions, "Network.dataReceived", &params);
            let params = Json::object([("requestId", id), ("timestamp", Json::Number(monotonic_seconds())), ("encodedDataLength", Json::Number(encoded as f64))]);
            self.emit(&sessions, "Network.loadingFinished", &params);
        }
        self.title.clear();
        self.announce();
    }

    /// `Page.backForwardCacheNotUsed`: a entrada restaurada do histórico não veio do cache de ida e volta. As razões
    /// medidas no oráculo valem para `about:blank`; a página `http` perde por `ConflictingBrowsingInstance`.
    fn emit_back_forward_cache_not_used(&self) {
        let web = self.url.starts_with("http://") || self.url.starts_with("https://");
        let reasons: Vec<Json> = if web { vec!["ConflictingBrowsingInstance"] } else { vec!["HTTPStatusNotOK", "SchemeNotHTTPOrHTTPS", "BrowsingInstanceNotSwapped"] }
            .into_iter()
            .map(|reason| Json::object([("type", Json::str("Circumstantial")), ("reason", Json::str(reason))]))
            .collect();
        let params = Json::object([
            ("loaderId", Json::str(&self.loader_id)),
            ("frameId", Json::str(&self.target_id)),
            ("notRestoredExplanations", Json::Array(reasons.clone())),
            ("notRestoredExplanationsTree", Json::object([("url", Json::str(&self.url)), ("explanations", Json::Array(reasons)), ("children", Json::Array(Vec::new()))])),
        ]);
        self.emit(&self.page_enabled, "Page.backForwardCacheNotUsed", &params);
    }

    /// Manda ao agente o estado da aba (janela, `Emulation`, mundos).
    fn push_emulation(&self, runner: &PageRunner, hook: &HostHandle) {
        let message = Json::object([("method", Json::str("__emulation")), ("params", self.emu.json(&self.world_ids, &self.net.borrow()))]);
        let _ = runner.call_hidden(hook, &[HostArg::Str("cdp"), HostArg::Str(&message.to_compact())]);
    }

    /// O `Runtime.executionContextCreated` de um mundo isolado.
    fn emit_world(&self, sessions: &[String], name: &str, id: i64) {
        let context = Json::object([
            ("id", Json::Number(id as f64)),
            ("origin", Json::str(security_origin(&self.url))),
            ("name", Json::str(name)),
            ("uniqueId", Json::str(format!("{}.{}", random_i64(), random_i64()))),
            ("auxData", Json::object([("isDefault", Json::Bool(false)), ("type", Json::str("isolated")), ("frameId", Json::str(&self.target_id))])),
        ]);
        self.emit(sessions, "Runtime.executionContextCreated", &Json::object([("context", context)]));
    }

    /// Os eventos de `Network` do pedido do documento: `requestWillBeSent` (um por redirecionamento), os `ExtraInfo` do
    /// fio e a resposta (`responseReceived`) ou a falha (`loadingFailed`). O `dataReceived` e o `loadingFinished` saem no
    /// `commit`. `about:blank` e `file:` não têm (`meta` vazio e sem erro).
    fn network_document(&mut self, url: &str, meta: Option<&crate::netmeta::NetMeta>, error: Option<&str>) {
        use super::network;
        if meta.is_none() && error.is_none() {
            return;
        }
        let sessions = self.network_enabled.clone();
        let id = self.loader_id.clone();
        let frame = self.target_id.clone();
        let wire = meta.is_some_and(|m| m.last.protocol != "data") || error.is_some();
        let mut renderer_headers: Vec<(String, String)> = if wire {
            [
                ("Upgrade-Insecure-Requests", "1"),
                ("User-Agent", super::http::USER_AGENT),
                ("sec-ch-ua", "\"Not A(Brand\";v=\"99\", \"Chromium\";v=\"154\""),
                ("sec-ch-ua-mobile", "?0"),
                ("sec-ch-ua-platform", "\"Linux\""),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
        } else {
            Vec::new()
        };
        if wire {
            self.net.borrow().apply(&mut renderer_headers);
        }
        let headers: Vec<(&str, &str)> = renderer_headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let headers = headers.as_slice();
        let wall = || ul_common::time::realtime_nanos() as f64 / 1e9;
        let sent = |this: &Self, document: &str, request: &str, redirect: Option<Json>| {
            let params = network::request_will_be_sent(&id, &id, &frame, document, request, headers, "Document", "VeryHigh", true, Json::object([("type", Json::str("other"))]), redirect, false, monotonic_seconds(), wall());
            this.emit(&sessions, "Network.requestWillBeSent", &params);
        };
        if let Some(error) = error {
            sent(self, url, url, None);
            let failed = Json::object([("requestId", Json::str(&id)), ("timestamp", Json::Number(monotonic_seconds())), ("type", Json::str("Document")), ("errorText", Json::str(error)), ("canceled", Json::Bool(false))]);
            self.emit(&sessions, "Network.loadingFailed", &failed);
            return;
        }
        let Some(meta) = meta else { return };
        let now_ms = wall() * 1000.0;
        let trustworthy = |u: &str| meta.last.protocol == "data" || crate::page::potentially_trustworthy(u);
        let mut previous: Option<&crate::netmeta::NetExchange> = None;
        for exchange in meta.redirects.iter().chain(std::iter::once(&meta.last)) {
            let redirect = previous.map(|p| network::response(p, trustworthy(&p.url), monotonic_seconds(), now_ms));
            sent(self, &exchange.url, &exchange.url, redirect);
            previous = Some(exchange);
        }
        if wire {
            for exchange in meta.redirects.iter().chain(std::iter::once(&meta.last)) {
                self.emit(&sessions, "Network.requestWillBeSentExtraInfo", &network::request_extra_info(&id, exchange, monotonic_seconds(), false, self.associated_cookies(&exchange.url)));
                self.emit(&sessions, "Network.responseReceivedExtraInfo", &network::response_extra_info(&id, exchange));
            }
        }
        let response = vec![
            ("requestId".to_owned(), Json::str(&id)),
            ("loaderId".to_owned(), Json::str(&id)),
            ("timestamp".to_owned(), Json::Number(monotonic_seconds())),
            ("type".to_owned(), Json::str("Document")),
            ("response".to_owned(), network::response(&meta.last, trustworthy(&meta.last.url), monotonic_seconds(), now_ms)),
            ("hasExtraInfo".to_owned(), Json::Bool(wire)),
            ("frameId".to_owned(), Json::str(&frame)),
        ];
        self.emit(&sessions, "Network.responseReceived", &Json::Object(response));
        self.finishing = Some((meta.data_length, meta.last.head.len() + meta.body_encoded));
    }

    /// O `/favicon.ico` que o navegador pede depois de carregar uma página `http` ou `https`, uma vez por origem (o
    /// oráculo não o repete na navegação seguinte à mesma origem). O pedido sai sempre, o servidor o vê; os eventos de
    /// `Network` só vão a quem os ligou. O `dataReceived` só existe com corpo.
    fn favicon(&mut self) {
        use super::network;
        static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let Some(origin) = crate::bridge::origin_of(&self.url) else { return };
        {
            let Ok(mut seen) = SEEN.lock() else { return };
            if seen.contains(&origin) {
                return;
            }
            seen.push(origin);
        }
        let started = (monotonic_seconds(), ul_common::time::realtime_nanos() as f64 / 1e9);
        let Some(response) = fetch::favicon_response(&self.fetcher(), &self.url) else { return };
        let observation = Observation { kind: fetch::Kind::Other, address: String::new(), started: started.0, wall: started.1, result: Ok(crate::netmeta::NetMeta::from_response(&response)) };
        self.emit_resource(&observation, Json::object([("type", Json::str("other"))]), "High", false);
    }

    /// Os eventos de `Network` de um subrecurso: `requestWillBeSent`, os `ExtraInfo` do fio, `responseReceived`,
    /// `dataReceived` e `loadingFinished`, ou `loadingFailed`. O pedido sai sempre, os eventos só vão a quem ligou `Network`.
    fn emit_resource(&mut self, observation: &Observation, initiator: Json, priority: &str, blocking: bool) {
        use super::network;
        self.resource_requests += 1;
        let id = format!("{}.{}", sysabi::sys::current().getpid(), self.resource_requests + 1);
        let sessions = self.network_enabled.clone();
        let (started, wall) = (observation.started, observation.wall);
        let mut renderer_headers: Vec<(String, String)> = [("sec-ch-ua-platform", "\"Linux\""), ("Referer", self.url.as_str()), ("User-Agent", super::http::USER_AGENT), ("sec-ch-ua", "\"Not A(Brand\";v=\"99\", \"Chromium\";v=\"154\""), ("sec-ch-ua-mobile", "?0")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        self.net.borrow().apply(&mut renderer_headers);
        let headers: Vec<(&str, &str)> = renderer_headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let kind = observation.kind;
        let address = observation.result.as_ref().map_or(observation.address.as_str(), |meta| meta.last.url.as_str());
        let host = |u: &str| url::Url::parse(u).ok().and_then(|u| u.host_str().map(str::to_owned));
        let same_site = host(&self.url) == host(address);
        let sent = network::request_will_be_sent(&id, &self.loader_id, &self.target_id, &self.url, address, &headers, kind.cdp_type(), priority, same_site, initiator, None, blocking, started, wall);
        self.emit(&sessions, "Network.requestWillBeSent", &sent);
        let meta = match &observation.result {
            Ok(meta) => meta,
            Err(error) => {
                let failed = Json::object([("requestId", Json::str(&id)), ("timestamp", Json::Number(monotonic_seconds())), ("type", Json::str(kind.cdp_type())), ("errorText", Json::str(error)), ("canceled", Json::Bool(false))]);
                self.emit(&sessions, "Network.loadingFailed", &failed);
                return;
            }
        };
        self.emit(&sessions, "Network.requestWillBeSentExtraInfo", &network::request_extra_info(&id, &meta.last, started, true, self.associated_cookies(&meta.last.url)));
        self.emit(&sessions, "Network.responseReceivedExtraInfo", &network::response_extra_info(&id, &meta.last));
        let received = Json::object([
            ("requestId", Json::str(&id)),
            ("loaderId", Json::str(&self.loader_id)),
            ("timestamp", Json::Number(monotonic_seconds())),
            ("type", Json::str(kind.cdp_type())),
            ("response", network::response(&meta.last, true, started, wall * 1000.0)),
            ("hasExtraInfo", Json::Bool(true)),
            ("frameId", Json::str(&self.target_id)),
        ]);
        self.emit(&sessions, "Network.responseReceived", &received);
        let data = |length: usize, encoded: usize| Json::object([("requestId", Json::str(&id)), ("timestamp", Json::Number(monotonic_seconds())), ("dataLength", Json::Number(length as f64)), ("encodedDataLength", Json::Number(encoded as f64))]);
        if meta.data_length > 0 {
            // `fetch` entrega o corpo de uma vez; o resto chega em duas partes: o conteúdo e depois os bytes do fio.
            if kind == fetch::Kind::Fetch {
                self.emit(&sessions, "Network.dataReceived", &data(meta.data_length, meta.body_encoded));
            } else {
                self.emit(&sessions, "Network.dataReceived", &data(meta.data_length, 0));
                if kind != fetch::Kind::Other {
                    self.emit(&sessions, "Network.dataReceived", &data(0, meta.body_encoded));
                }
            }
        }
        let finished = Json::object([("requestId", Json::str(&id)), ("timestamp", Json::Number(monotonic_seconds())), ("encodedDataLength", Json::Number((meta.last.head.len() + meta.body_encoded) as f64))]);
        self.emit(&sessions, "Network.loadingFinished", &finished);
    }

    /// Relata os subrecursos que o fetcher viu: o `initiator` é o parser (linha e coluna da tag no HTML) quando o
    /// documento tem a tag, e o script no resto (`fetch`, XHR, elemento inserido pela página).
    fn report_resources(&mut self) {
        let observed = std::mem::take(&mut *self.observed.borrow_mut());
        for observation in observed {
            use fetch::Kind;
            let parsed = matches!(observation.kind, Kind::Stylesheet | Kind::Script | Kind::Image | Kind::Font);
            if parsed {
                let key = (observation.kind, observation.address.clone());
                if self.requested.contains(&key) {
                    continue;
                }
                self.requested.push(key);
            }
            let position = parsed.then(|| super::network::parser_position(&self.source, &self.url, &observation.address)).flatten();
            let (initiator, blocking) = match position {
                Some((line, column)) => (
                    Json::object([("type", Json::str("parser")), ("url", Json::str(&self.url)), ("lineNumber", Json::Number(line as f64)), ("columnNumber", Json::Number(column as f64))]),
                    matches!(observation.kind, Kind::Stylesheet | Kind::Script),
                ),
                None => (Json::object([("type", Json::str("script")), ("stack", Json::object([("callFrames", Json::Array(Vec::new()))]))]), false),
            };
            self.emit_resource(&observation, initiator, observation.kind.priority(), blocking);
        }
    }

    /// Título e URL para o `TargetInfo`. Sem `<title>`, o título é a URL; a aba que ainda vai navegar mostra o destino.
    fn announce(&self) {
        let url = self.held.as_deref().unwrap_or(&self.url);
        let title = if self.title.is_empty() { url } else { &self.title };
        let redirects = Json::Array(self.redirects.iter().map(|hop| Json::str(hop)).collect());
        self.write_line('I', &Json::object([("url", Json::str(url)), ("title", Json::str(title)), ("redirects", redirects)]).to_compact());
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
        self.report_resources();
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
                    if let Some(entry) = self.history.get_mut(self.current) {
                        entry.title = self.title.clone();
                    }
                    self.announce();
                    self.emit(&sessions, "Page.loadEventFired", &Json::object([("timestamp", Json::Number(now))]));
                    self.lifecycle_event("load", now);
                    self.emit(&sessions, "Page.frameStoppedLoading", &Json::object([("frameId", Json::str(&self.target_id))]));
                    self.favicon();
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
                    self.nav = NavKind::Typed;
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

    /// O `Security.visibleSecurityStateChanged` do documento corrente: `https` é seguro, `about:blank` neutro e o resto
    /// (data, http, file) inseguro por esquema não criptográfico.
    fn emit_security_state(&self, sessions: &[String]) {
        let (state, issues) = if self.url.starts_with("https:") {
            ("secure", Vec::new())
        } else if self.url.is_empty() || self.url == "about:blank" {
            ("neutral", vec![Json::str("scheme-is-not-cryptographic")])
        } else {
            ("insecure", vec![Json::str("scheme-is-not-cryptographic")])
        };
        let params = Json::object([(
            "visibleSecurityState",
            Json::object([("securityState", Json::str(state)), ("securityStateIssueIds", Json::Array(issues))]),
        )]);
        self.emit(sessions, "Security.visibleSecurityStateChanged", &params);
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
                for list in [&mut self.page_enabled, &mut self.lifecycle_enabled, &mut self.runtime_enabled, &mut self.network_enabled, &mut self.security_enabled] {
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
            "Log.enable" | "Log.disable" | "Performance.enable" | "Performance.disable" => self.reply(&message, Json::object([])),
            "Network.enable" | "Network.disable" => {
                self.network_enabled.retain(|s| *s != session);
                if method == "Network.enable" {
                    self.network_enabled.push(session);
                }
                self.reply(&message, Json::object([]));
            }
            "Security.enable" | "Security.disable" => {
                let first = method == "Security.enable" && !self.security_enabled.contains(&session);
                self.security_enabled.retain(|s| *s != session);
                if method == "Security.enable" {
                    self.security_enabled.push(session.clone());
                }
                // O estado visível sai antes do resultado, só na primeira ligação (medido no oráculo).
                if first {
                    self.emit_security_state(std::slice::from_ref(&session));
                }
                self.reply(&message, Json::object([]));
            }
            "Browser.setWindowBounds" => {
                let bounds = params.get("bounds").cloned().unwrap_or(Json::Object(Vec::new()));
                let before = self.emu.window;
                let size = |key: &str, current: i64| bounds.get(key).and_then(Json::as_int).unwrap_or(current);
                self.emu.window = (size("width", before.0), size("height", before.1));
                self.push_emulation(runner, hook);
                if self.emu.window != before {
                    self.emit(&self.page_enabled.clone(), "Page.frameResized", &Json::object([]));
                }
                self.reply(&message, Json::object([]));
            }
            "Emulation.setDeviceMetricsOverride" => {
                let number = |key: &str| params.get(key).and_then(Json::as_f64).unwrap_or(0.0);
                self.emu.metrics = Some((number("width") as i64, number("height") as i64, if number("deviceScaleFactor") > 0.0 { number("deviceScaleFactor") } else { 1.0 }));
                self.push_emulation(runner, hook);
                self.emit(&self.page_enabled.clone(), "Page.frameResized", &Json::object([]));
                self.reply(&message, Json::object([]));
            }
            "Emulation.clearDeviceMetricsOverride" => {
                self.emu.metrics = None;
                self.push_emulation(runner, hook);
                for _ in 0..3 {
                    self.emit(&self.page_enabled.clone(), "Page.frameResized", &Json::object([]));
                }
                self.reply(&message, Json::object([]));
            }
            "Emulation.setTouchEmulationEnabled" => {
                self.emu.touch = if params.get("enabled").and_then(Json::as_bool) == Some(true) { params.get("maxTouchPoints").and_then(Json::as_int).filter(|n| *n > 0).unwrap_or(1) } else { 0 };
                self.push_emulation(runner, hook);
                self.reply(&message, Json::object([]));
            }
            "Emulation.setEmulatedMedia" => {
                self.emu.media = params.get("media").and_then(Json::as_str).unwrap_or_default().to_owned();
                self.emu.features = match params.get("features") {
                    Some(Json::Array(features)) => features.iter().filter_map(|f| Some((f.get("name")?.as_str()?.to_owned(), f.get("value")?.as_str()?.to_owned()))).collect(),
                    _ => Vec::new(),
                };
                self.push_emulation(runner, hook);
                self.reply(&message, Json::object([]));
            }
            "Emulation.setFocusEmulationEnabled" | "Page.setFontFamilies" => self.reply(&message, Json::object([])),
            "Page.addScriptToEvaluateOnNewDocument" => {
                let text = |key: &str| params.get(key).and_then(Json::as_str).unwrap_or_default().to_owned();
                let identifier = self.emu.next_script.to_string();
                self.emu.next_script += 1;
                self.emu.scripts.push((identifier.clone(), text("source"), text("worldName")));
                self.reply(&message, Json::object([("identifier", Json::str(identifier))]));
            }
            "Page.removeScriptToEvaluateOnNewDocument" => {
                let identifier = params.get("identifier").and_then(Json::as_str).unwrap_or_default();
                match self.emu.scripts.iter().position(|(id, _, _)| id == identifier) {
                    Some(index) => {
                        self.emu.scripts.remove(index);
                        self.reply(&message, Json::object([]));
                    }
                    None => self.fail(&message, -32000, "Script not found"),
                }
            }
            "Page.createIsolatedWorld" => {
                if params.get("frameId").and_then(Json::as_str) != Some(self.target_id.as_str()) {
                    self.fail(&message, -32602, "No frame for given id found");
                } else {
                    let name = params.get("worldName").and_then(Json::as_str).unwrap_or_default().to_owned();
                    let id = self.next_context;
                    self.next_context += 1;
                    self.emu.created.push(name.clone());
                    self.world_ids.push(id);
                    self.emit_world(&self.runtime_enabled.clone(), &name, id);
                    self.push_emulation(runner, hook);
                    self.reply(&message, Json::object([("executionContextId", Json::Number(id as f64))]));
                }
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
                self.nav = NavKind::Typed;
                return Some(Next::Navigate(url, Some(message)));
            }
            "Page.reload" => {
                self.reply(&message, Json::object([]));
                self.nav = NavKind::Reload;
                return Some(Next::Navigate(self.url.clone(), None));
            }
            "Page.getNavigationHistory" => self.reply(&message, self.history_json()),
            "Page.navigateToHistoryEntry" => {
                let id = params.get("entryId").and_then(Json::as_int).unwrap_or(-1);
                match self.history.iter().position(|e| e.id == id) {
                    Some(index) => {
                        let url = self.history[index].url.clone();
                        self.reply(&message, Json::object([]));
                        self.nav = NavKind::History(index);
                        self.history_busy = true;
                        return Some(Next::Navigate(url, None));
                    }
                    None => self.fail(&message, -32602, "No entry with passed id"),
                }
            }
            "Network.setExtraHTTPHeaders" => {
                self.net.borrow_mut().extra = match params.get("headers") {
                    Some(Json::Object(headers)) => headers.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned())).collect(),
                    _ => Vec::new(),
                };
                self.reply(&message, Json::object([]));
            }
            "Network.setUserAgentOverride" | "Emulation.setUserAgentOverride" => {
                let text = |key: &str| params.get(key).and_then(Json::as_str).unwrap_or_default().to_owned();
                {
                    let mut net = self.net.borrow_mut();
                    net.user_agent = text("userAgent");
                    net.accept_language = text("acceptLanguage");
                    net.platform = params.get("userAgentMetadata").and_then(|m| m.get("platform")).and_then(Json::as_str).unwrap_or_default().to_owned();
                }
                self.emu.nav_platform = text("platform");
                self.push_emulation(runner, hook);
                self.reply(&message, Json::object([]));
            }
            "Network.emulateNetworkConditions" => {
                self.net.borrow_mut().offline = params.get("offline").and_then(Json::as_bool) == Some(true);
                self.push_emulation(runner, hook);
                self.reply(&message, Json::object([]));
            }
            "Network.setCacheDisabled" => self.reply(&message, Json::object([])),
            "Emulation.setLocaleOverride" => {
                self.emu.locale = params.get("locale").and_then(Json::as_str).unwrap_or_default().to_owned();
                self.reply(&message, Json::object([]));
            }
            "Emulation.setTimezoneOverride" => {
                self.emu.timezone = params.get("timezoneId").and_then(Json::as_str).unwrap_or_default().to_owned();
                self.reply(&message, Json::object([]));
            }
            "Emulation.setGeolocationOverride" => {
                let number = |key: &str| params.get(key).and_then(Json::as_f64);
                self.emu.geo = number("latitude").zip(number("longitude")).map(|(latitude, longitude)| (latitude, longitude, number("accuracy").unwrap_or(0.0)));
                self.push_emulation(runner, hook);
                self.reply(&message, Json::object([]));
            }
            "Runtime.addBinding" => {
                let name = params.get("name").and_then(Json::as_str).unwrap_or_default().to_owned();
                if !self.emu.bindings.contains(&name) {
                    self.emu.bindings.push(name);
                }
                self.push_emulation(runner, hook);
                self.reply(&message, Json::object([]));
            }
            "Runtime.removeBinding" => {
                let name = params.get("name").and_then(Json::as_str).unwrap_or_default();
                self.emu.bindings.retain(|b| b != name);
                self.reply(&message, Json::object([]));
            }
            _ => {
                // O `frameResized` sai antes do resultado: uma vez ao pôr as métricas, três ao limpar (medido no oráculo).
                let resized = match method.as_str() {
                    "Emulation.setDeviceMetricsOverride" => 1,
                    "Emulation.clearDeviceMetricsOverride" => 3,
                    _ => 0,
                };
                for _ in 0..resized {
                    self.emit(&self.page_enabled.clone(), "Page.frameResized", &Json::object([]));
                }
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
        "http" | "https" | "ws" | "wss" | "ftp" | "chrome" => {
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
        "https" | "wss" | "file" | "chrome" => "Secure",
        "http" if is_localhost(url) => "SecureLocalhost",
        _ => "InsecureScheme",
    }
}

/// O domínio registrável do host da URL (`registry::domain_and_registry`).
fn domain_and_registry(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(super::registry::domain_and_registry)).unwrap_or_default()
}
