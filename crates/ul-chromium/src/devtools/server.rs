//! O laço do navegador: aceita conexões HTTP no `--remote-debugging-port` (ou lê o `--remote-debugging-pipe`), responde
//! os domínios `Browser` e `Target`, mantém as sessões e repassa as mensagens de cada sessão à thread da página dela.

use std::io::Write as _;

use sysabi::{Ctx, Fd, PollEvents, PollFd};

use super::http::{self, Endpoint, Reply, TargetEntry};
use super::json::{self, Json};
use super::target::{self, Link};
use super::websocket::{self, Decoder, Incoming};
use super::{token, uuid};

/// Como o DevTools remoto foi pedido na linha de comando.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    /// `--remote-debugging-port=N` (0 escolhe uma porta livre).
    pub port: Option<u16>,
    /// `--remote-debugging-pipe`: mensagens separadas por NUL lidas do fd 3 e escritas no fd 4.
    pub pipe: bool,
    /// `--remote-allow-origins=a,b`.
    pub allowed_origins: Vec<String>,
}

struct Target {
    id: String,
    kind: &'static str,
    title: String,
    url: String,
    link: Link,
    window_id: i64,
    /// Comandos à espera de espaço no pipe da página (o laço do navegador nunca bloqueia escrevendo).
    pending: Vec<u8>,
    inbox: Vec<u8>,
}

struct Session {
    id: String,
    target: String,
    conn: u64,
    /// Sessão do WebSocket da própria página (`/devtools/page/ID`): as mensagens não levam `sessionId`.
    implicit: bool,
    /// Sessão sem `flatten`: as mensagens viajam dentro de `Target.sendMessageToTarget` e `receivedMessageFromTarget`.
    flatten: bool,
}

enum Transport {
    Http,
    WebSocket(Decoder, Endpoint),
    Pipe { output: Fd },
}

struct Conn {
    key: u64,
    fd: Fd,
    transport: Transport,
    inbox: Vec<u8>,
    outbox: Vec<u8>,
    discover: bool,
    /// `Target.setAutoAttach` no alvo browser: `(flatten, waitForDebuggerOnStart)`.
    auto_attach: Option<(bool, bool)>,
    closing: bool,
}

pub struct Browser {
    browser_id: String,
    context_id: String,
    contexts: Vec<String>,
    targets: Vec<Target>,
    sessions: Vec<Session>,
    conns: Vec<Conn>,
    next_key: u64,
    allowed_origins: Vec<String>,
    /// `Browser.close` pedido: o laço termina depois de mandar a resposta.
    quit: bool,
    /// Abas a fechar depois da resposta corrente.
    closing: Vec<String>,
    /// Janelas abertas até aqui: a da aba inicial e a do primeiro alvo criado, cada uma com os seus popups do omnibox
    /// (as abas seguintes entram na segunda janela, como no oráculo).
    windows: usize,
    schema: super::protocol::Schema,
}

/// O `TargetInfo` do protocolo.
fn target_info(target: &Target, attached: bool, context: &str) -> Json {
    Json::object([
        ("targetId", Json::str(&target.id)),
        ("type", Json::str(target.kind)),
        ("title", Json::str(&target.title)),
        ("url", Json::str(&target.url)),
        ("attached", Json::Bool(attached)),
        ("canAccessOpener", Json::Bool(false)),
        ("browserContextId", Json::str(context)),
    ])
}

fn error_message(id: Option<&Json>, code: i64, message: &str, session: Option<&Json>) -> Json {
    let mut fields = Vec::new();
    if let Some(id) = id {
        fields.push(("id".to_owned(), id.clone()));
    }
    fields.push(("error".to_owned(), Json::object([("code", Json::Number(code as f64)), ("message", Json::str(message))])));
    if let Some(session) = session {
        fields.push(("sessionId".to_owned(), session.clone()));
    }
    Json::Object(fields)
}

fn with_field(message: &Json, key: &str, value: Option<Json>) -> Json {
    let Json::Object(fields) = message else { return message.clone() };
    let mut fields: Vec<(String, Json)> = fields.iter().filter(|(k, _)| k != key).cloned().collect();
    if let Some(value) = value {
        fields.push((key.to_owned(), value));
    }
    Json::Object(fields)
}

impl http::Browser for Browser {
    fn browser_id(&self) -> String {
        self.browser_id.clone()
    }

    fn targets(&self) -> Vec<TargetEntry> {
        self.targets.iter().rev().map(|t| TargetEntry { id: t.id.clone(), kind: t.kind.to_owned(), title: t.title.clone(), url: t.url.clone() }).collect()
    }

    fn create(&mut self, url: &str) -> TargetEntry {
        let id = self.open_target(url, false, false);
        let target = self.target(&id).expect("alvo recém-criado");
        TargetEntry { id: target.id.clone(), kind: target.kind.to_owned(), title: String::new(), url: target.url.clone() }
    }

    fn activate(&mut self, id: &str) -> bool {
        self.target(id).is_some()
    }

    fn close(&mut self, id: &str) -> bool {
        self.close_target(id)
    }
}

impl Browser {
    pub fn new(options: &Options) -> Browser {
        Browser {
            browser_id: uuid(),
            context_id: token(),
            contexts: Vec::new(),
            targets: Vec::new(),
            sessions: Vec::new(),
            conns: Vec::new(),
            next_key: 1,
            allowed_origins: options.allowed_origins.clone(),
            quit: false,
            closing: Vec::new(),
            windows: 0,
            schema: super::protocol::Schema::load(),
        }
    }

    fn target(&self, id: &str) -> Option<&Target> {
        self.targets.iter().find(|t| t.id == id)
    }

    fn attached(&self, id: &str) -> bool {
        self.sessions.iter().any(|s| s.target == id)
    }

    fn info(&self, id: &str) -> Option<Json> {
        self.target(id).map(|t| target_info(t, self.attached(id), &self.context_id))
    }

    fn conn_mut(&mut self, key: u64) -> Option<&mut Conn> {
        self.conns.iter_mut().find(|c| c.key == key)
    }

    /// Escreve uma mensagem do protocolo na conexão `key`, no quadro do transporte dela.
    fn deliver(&mut self, key: u64, message: &Json) {
        let text = message.to_compact();
        let Some(conn) = self.conn_mut(key) else { return };
        match conn.transport {
            Transport::Pipe { .. } => {
                conn.outbox.extend_from_slice(text.as_bytes());
                conn.outbox.push(0);
            }
            _ => conn.outbox.extend(websocket::encode(websocket::TEXT, text.as_bytes())),
        }
    }

    fn event(method: &str, params: Json) -> Json {
        Json::object([("method", Json::str(method)), ("params", params)])
    }

    /// Um evento às conexões do alvo browser que pediram a descoberta de alvos.
    fn broadcast_discovery(&mut self, method: &str, params: Json) {
        let keys: Vec<u64> = self.conns.iter().filter(|c| c.discover).map(|c| c.key).collect();
        for key in keys {
            self.deliver(key, &Self::event(method, params.clone()));
        }
    }

    fn open_target(&mut self, url: &str, startup: bool, wait: bool) -> String {
        if self.windows < 2 {
            self.windows += 1;
            for (ui, _) in crate::page::WEBUI_DOCUMENTS {
                self.spawn_target("browser_ui", ui, "Omnibox Popup", true, wait);
            }
        }
        let shown = if url.is_empty() { "about:blank" } else { url };
        // A primeira aba já nasce com o documento comprometido (título é a URL); a criada pelo protocolo, sem título.
        self.spawn_target("page", shown, if startup { shown } else { "" }, startup, wait)
    }

    fn spawn_target(&mut self, kind: &'static str, url: &str, title: &str, startup: bool, wait: bool) -> String {
        let id = token();
        let Ok(link) = target::spawn(id.clone(), url.to_owned(), startup, wait) else { return id };
        let window_id = (super::random_i64() & 0x7FFF_FFFF).max(1);
        self.targets.push(Target { id: id.clone(), kind, title: title.to_owned(), url: url.to_owned(), link, window_id, pending: Vec::new(), inbox: Vec::new() });
        let info = self.info(&id).expect("alvo recém-criado");
        self.broadcast_discovery("Target.targetCreated", Json::object([("targetInfo", info)]));
        // A conexão com `setAutoAttach` anexa cada alvo novo.
        let auto: Vec<(u64, bool, bool)> = self.conns.iter().filter_map(|c| c.auto_attach.map(|(flatten, wait)| (c.key, flatten, wait))).collect();
        for (key, flatten, wait) in auto {
            self.attach(key, &id, flatten, wait);
        }
        id
    }

    /// Marca a aba para fechar depois da resposta do comando corrente (o Chromium responde antes de derrubar o
    /// renderer). Falso para id desconhecido.
    fn close_target(&mut self, id: &str) -> bool {
        if self.target(id).is_none() {
            return false;
        }
        if !self.closing.iter().any(|c| c == id) {
            self.closing.push(id.to_owned());
        }
        true
    }

    /// Fecha as abas marcadas: cada sessão recebe `Inspector.detached`, quem descobre vê o alvo desanexado, quem
    /// anexou recebe `detachedFromTarget`, e por fim `targetDestroyed`.
    fn finish_closing(&mut self) {
        for id in std::mem::take(&mut self.closing) {
            let Some(index) = self.targets.iter().position(|t| t.id == id) else { continue };
            let sessions: Vec<(String, u64, bool, bool)> = self.sessions.iter().filter(|s| s.target == id).map(|s| (s.id.clone(), s.conn, s.implicit, s.flatten)).collect();
            let detached = Json::object([("reason", Json::str("Render process gone."))]);
            for (session, key, implicit, flatten) in &sessions {
                let event = Self::event("Inspector.detached", detached.clone());
                if *implicit {
                    self.deliver(*key, &event);
                    if let Some(conn) = self.conn_mut(*key) {
                        conn.closing = true;
                    }
                } else if *flatten {
                    self.deliver(*key, &with_field(&event, "sessionId", Some(Json::str(session))));
                } else {
                    let wrapped = Json::object([("sessionId", Json::str(session)), ("message", Json::str(event.to_compact())), ("targetId", Json::str(&id))]);
                    self.deliver(*key, &Self::event("Target.receivedMessageFromTarget", wrapped));
                }
            }
            self.sessions.retain(|s| s.target != id);
            if !sessions.is_empty() {
                if let Some(info) = self.info(&id) {
                    self.broadcast_discovery("Target.targetInfoChanged", Json::object([("targetInfo", info)]));
                }
            }
            for (session, key, implicit, _) in sessions {
                if !implicit {
                    self.deliver(key, &Self::event("Target.detachedFromTarget", Json::object([("sessionId", Json::str(&session)), ("targetId", Json::str(&id))])));
                }
            }
            let target = self.targets.remove(index);
            let _ = sysabi::sys::write(target.link.commands, b"X\n");
            let _ = sysabi::sys::close(target.link.commands);
            let _ = sysabi::sys::close(target.link.events);
            self.broadcast_discovery("Target.targetDestroyed", Json::object([("targetId", Json::str(&id))]));
        }
    }

    /// Anexa uma sessão nova de `key` ao alvo `id` e anuncia (`targetInfoChanged` a quem descobre, `attachedToTarget`
    /// a quem anexou). Devolve o id da sessão.
    fn attach(&mut self, key: u64, id: &str, flatten: bool, waiting: bool) -> String {
        let was_attached = self.attached(id);
        let session = token();
        self.sessions.push(Session { id: session.clone(), target: id.to_owned(), conn: key, implicit: false, flatten });
        let info = self.info(id).expect("alvo existente");
        if !was_attached {
            self.broadcast_discovery("Target.targetInfoChanged", Json::object([("targetInfo", info.clone())]));
        }
        self.deliver(
            key,
            &Self::event("Target.attachedToTarget", Json::object([("sessionId", Json::str(&session)), ("targetInfo", info), ("waitingForDebugger", Json::Bool(waiting))])),
        );
        session
    }

    fn detach(&mut self, session: &str, announce: bool) {
        let Some(index) = self.sessions.iter().position(|s| s.id == session) else { return };
        let removed = self.sessions.remove(index);
        if let Some(target) = self.targets.iter_mut().find(|t| t.id == removed.target) {
            target.pending.extend_from_slice(format!("D{session}\n").as_bytes());
        }
        if announce {
            self.deliver(removed.conn, &Self::event("Target.detachedFromTarget", Json::object([("sessionId", Json::str(session)), ("targetId", Json::str(&removed.target))])));
        }
        if !self.attached(&removed.target) {
            if let Some(info) = self.info(&removed.target) {
                self.broadcast_discovery("Target.targetInfoChanged", Json::object([("targetInfo", info)]));
            }
        }
    }

    fn forward(&mut self, session: &str, message: &Json) {
        let Some(target) = self.sessions.iter().find(|s| s.id == session).map(|s| s.target.clone()) else { return };
        let text = with_field(message, "sessionId", Some(Json::str(session))).to_compact();
        if let Some(target) = self.targets.iter_mut().find(|t| t.id == target) {
            target.pending.push(b'M');
            target.pending.extend_from_slice(text.as_bytes());
            target.pending.push(b'\n');
        }
    }

    /// Uma mensagem de texto que chegou pela conexão `key`.
    fn on_message(&mut self, key: u64, text: &str, endpoint: &Endpoint) {
        let message = match json::parse(text) {
            Ok(message) => message,
            Err(position) => {
                self.deliver(key, &error_message(None, -32700, &format!("JSON: invalid token at position {position}"), None));
                return;
            }
        };
        let id = message.get("id").filter(|id| id.as_int().is_some()).cloned();
        let Some(id) = id else {
            self.deliver(key, &error_message(None, -32600, "Message must have integer 'id' property", None));
            return;
        };
        let session = message.get("sessionId").cloned();
        let Some(method) = message.get("method").and_then(Json::as_str).map(str::to_owned) else {
            self.deliver(key, &error_message(Some(&id), -32600, "Message must have string 'method' property", session.as_ref()));
            return;
        };
        // Os parâmetros passam pelo esquema antes do domínio; no alvo browser, só os domínios que ele tem (os outros
        // respondem `wasn't found`).
        let browser_level = session.is_none() && matches!(endpoint, Endpoint::Browser);
        let routed = !browser_level || method.starts_with("Browser.") || method.starts_with("Target.");
        if let Err(data) = if routed { self.schema.validate(&method, message.get("params")) } else { Ok(()) } {
            let mut reply = error_message(Some(&id), -32602, "Invalid parameters", None);
            if let Json::Object(fields) = &mut reply {
                if let Some((_, Json::Object(error))) = fields.iter_mut().find(|(k, _)| k == "error") {
                    error.push(("data".to_owned(), Json::str(data)));
                }
                let implicit = matches!(endpoint, Endpoint::Page(_));
                if let (Some(session), false) = (session, implicit) {
                    fields.push(("sessionId".to_owned(), session));
                }
            }
            self.deliver(key, &reply);
            return;
        }
        if let Endpoint::Page(target) = endpoint {
            // A conexão da página fala direto com ela, por uma sessão implícita criada no aperto de mão.
            let implicit = self.sessions.iter().find(|s| s.conn == key && s.implicit && s.target == *target).map(|s| s.id.clone());
            if let Some(implicit) = implicit {
                if !self.page_session_command(key, &implicit, &id, &method) {
                    self.forward(&implicit, &message);
                }
            }
            return;
        }
        if let Some(session_id) = session.as_ref().and_then(Json::as_str) {
            let known = self.sessions.iter().any(|s| s.id == session_id && s.conn == key && s.flatten);
            if !known {
                self.deliver(key, &error_message(Some(&id), -32001, "Session with given id not found.", None));
                return;
            }
            let session_id = session_id.to_owned();
            if !self.page_session_command(key, &session_id, &id, &method) {
                self.forward(&session_id, &message);
            }
            return;
        }
        let params = message.get("params").cloned().unwrap_or(Json::Object(Vec::new()));
        match self.browser_command(key, &method, &params) {
            Ok(result) => self.deliver(key, &Json::object([("id", id), ("result", result)])),
            Err((code, text)) => self.deliver(key, &error_message(Some(&id), code, &text, None)),
        }
    }

    /// Os comandos de `Target` que uma sessão de página responde no navegador. Falso: vai para a página.
    fn page_session_command(&mut self, key: u64, session: &str, id: &Json, method: &str) -> bool {
        let target = self.sessions.iter().find(|s| s.id == session).map(|s| s.target.clone()).unwrap_or_default();
        let implicit = self.sessions.iter().any(|s| s.id == session && s.implicit);
        let result = match method {
            "Target.setAutoAttach" | "Target.setDiscoverTargets" | "Target.autoAttachRelated" => Json::object([]),
            "Target.getTargetInfo" => match self.info(&target) {
                Some(info) => Json::object([("targetInfo", info)]),
                None => return false,
            },
            _ => return false,
        };
        let session_field = (!implicit).then(|| Json::str(session));
        let mut fields = vec![("id".to_owned(), id.clone()), ("result".to_owned(), result)];
        if let Some(session) = session_field {
            fields.push(("sessionId".to_owned(), session));
        }
        self.deliver(key, &Json::Object(fields));
        true
    }

    /// Os domínios `Browser` e `Target` no alvo browser.
    fn browser_command(&mut self, key: u64, method: &str, params: &Json) -> Result<Json, (i64, String)> {
        let not_found = || Err((-32602, "No target with given id found".to_owned()));
        let target_id = params.get("targetId").and_then(Json::as_str).map(str::to_owned);
        match method {
            "Browser.getVersion" => Ok(Json::object([
                ("protocolVersion", Json::str(http::PROTOCOL_VERSION)),
                ("product", Json::str(http::PRODUCT)),
                ("revision", Json::str(http::REVISION)),
                ("userAgent", Json::str(http::USER_AGENT)),
                ("jsVersion", Json::str(http::V8_VERSION)),
            ])),
            "Browser.close" => {
                self.quit = true;
                Ok(Json::object([]))
            }
            "Browser.getWindowForTarget" => match target_id {
                // A janela da aba: id aleatório e a moldura medida no headless do oráculo.
                Some(id) if self.target(&id).is_some() => Ok(Json::object([
                    ("windowId", Json::Number(self.target(&id).map_or(0, |t| t.window_id) as f64)),
                    ("bounds", Json::object([("left", Json::Number(10.0)), ("top", Json::Number(10.0)), ("width", Json::Number(780.0)), ("height", Json::Number(580.0)), ("windowState", Json::str("normal"))])),
                ])),
                Some(_) => not_found(),
                None => Err((-32000, "No web contents in the target".to_owned())),
            },
            "Target.getTargets" => {
                let types: Option<Vec<(bool, String)>> = params.get("filter").and_then(|f| match f {
                    Json::Array(entries) => Some(
                        entries
                            .iter()
                            .map(|e| (e.get("exclude").and_then(Json::as_bool) == Some(true), e.get("type").and_then(Json::as_str).unwrap_or_default().to_owned()))
                            .collect(),
                    ),
                    _ => None,
                });
                let infos: Vec<Json> = self
                    .targets
                    .iter()
                    .filter(|t| match &types {
                        // O filtro do protocolo: a primeira entrada que casa decide; tipo vazio casa com todos.
                        Some(entries) => entries.iter().find(|(_, kind)| kind.is_empty() || kind == t.kind).is_some_and(|(exclude, _)| !exclude),
                        None => true,
                    })
                    .map(|t| target_info(t, self.attached(&t.id), &self.context_id))
                    .collect();
                Ok(Json::object([("targetInfos", Json::Array(infos))]))
            }
            "Target.getTargetInfo" => match target_id {
                None => Ok(Json::object([(
                    "targetInfo",
                    Json::object([
                        ("targetId", Json::str(&self.browser_id)),
                        ("type", Json::str("browser")),
                        ("title", Json::str("")),
                        ("url", Json::str("")),
                        ("attached", Json::Bool(true)),
                        ("canAccessOpener", Json::Bool(false)),
                    ]),
                )])),
                Some(id) => self.info(&id).map(|info| Json::object([("targetInfo", info)])).ok_or((-32602, "No target with given id found".to_owned())),
            },
            "Target.setDiscoverTargets" => {
                let discover = params.get("discover").and_then(Json::as_bool) == Some(true);
                let infos: Vec<Json> = self.targets.iter().map(|t| target_info(t, self.attached(&t.id), &self.context_id)).collect();
                let already = self.conn_mut(key).map(|c| std::mem::replace(&mut c.discover, discover)).unwrap_or(false);
                if discover && !already {
                    for info in infos {
                        self.deliver(key, &Self::event("Target.targetCreated", Json::object([("targetInfo", info)])));
                    }
                }
                Ok(Json::object([]))
            }
            "Target.setAutoAttach" => {
                let auto = params.get("autoAttach").and_then(Json::as_bool) == Some(true);
                let wait = params.get("waitForDebuggerOnStart").and_then(Json::as_bool) == Some(true);
                let flatten = params.get("flatten").and_then(Json::as_bool) == Some(true);
                let before = self.conn_mut(key).map(|c| std::mem::replace(&mut c.auto_attach, auto.then_some((flatten, wait)))).unwrap_or(None);
                if auto && before.is_none() {
                    // Os alvos que já existem são anexados sem espera: só os novos nascem pausados.
                    let ids: Vec<String> = self.targets.iter().map(|t| t.id.clone()).collect();
                    for id in ids {
                        if !self.sessions.iter().any(|s| s.conn == key && s.target == id && !s.implicit) {
                            self.attach(key, &id, flatten, false);
                        }
                    }
                }
                Ok(Json::object([]))
            }
            "Target.attachToTarget" => {
                let Some(id) = target_id.filter(|id| self.target(id).is_some()) else { return not_found() };
                let flatten = params.get("flatten").and_then(Json::as_bool) == Some(true);
                let session = self.attach(key, &id, flatten, false);
                Ok(Json::object([("sessionId", Json::str(session))]))
            }
            "Target.attachToBrowserTarget" => Err((-32000, "Not allowed".to_owned())),
            "Target.detachFromTarget" => {
                let session = params.get("sessionId").and_then(Json::as_str).map(str::to_owned);
                let session = session.or_else(|| target_id.and_then(|id| self.sessions.iter().find(|s| s.target == id && s.conn == key).map(|s| s.id.clone())));
                match session.filter(|s| self.sessions.iter().any(|x| x.id == *s && x.conn == key)) {
                    Some(session) => {
                        self.detach(&session, true);
                        Ok(Json::object([]))
                    }
                    None => Err((-32602, "No session with given id".to_owned())),
                }
            }
            "Target.sendMessageToTarget" => {
                let session = params.get("sessionId").and_then(Json::as_str).map(str::to_owned);
                let session = session.or_else(|| target_id.and_then(|id| self.sessions.iter().find(|s| s.target == id && s.conn == key).map(|s| s.id.clone())));
                let Some(session) = session.filter(|s| self.sessions.iter().any(|x| x.id == *s && x.conn == key && !x.flatten)) else {
                    return Err((-32602, "No session with given id".to_owned()));
                };
                let inner = params.get("message").and_then(Json::as_str).unwrap_or_default();
                match json::parse(inner) {
                    Ok(inner) => self.forward(&session, &inner),
                    Err(position) => return Err((-32700, format!("JSON: invalid token at position {position}"))),
                }
                Ok(Json::object([]))
            }
            "Target.createTarget" => {
                let url = params.get("url").and_then(Json::as_str).unwrap_or_default().to_owned();
                if let Some(context) = params.get("browserContextId").and_then(Json::as_str) {
                    if context != self.context_id && !self.contexts.iter().any(|c| c == context) {
                        return Err((-32602, "Failed to find browser context with id ".to_owned() + context));
                    }
                }
                let wait = self.conn_mut(key).and_then(|c| c.auto_attach).is_some_and(|(_, wait)| wait);
                let id = self.open_target(&url, false, wait);
                Ok(Json::object([("targetId", Json::str(id))]))
            }
            "Target.closeTarget" => match target_id {
                Some(id) if self.close_target(&id) => Ok(Json::object([("success", Json::Bool(true))])),
                _ => not_found(),
            },
            "Target.activateTarget" => match target_id {
                Some(id) if self.target(&id).is_some() => Ok(Json::object([])),
                _ => not_found(),
            },
            "Target.createBrowserContext" => {
                let context = token();
                self.contexts.push(context.clone());
                Ok(Json::object([("browserContextId", Json::str(context))]))
            }
            "Target.getBrowserContexts" => Ok(Json::object([
                ("browserContextIds", Json::Array(self.contexts.iter().map(Json::str).collect())),
                ("defaultBrowserContextId", Json::str(&self.context_id)),
            ])),
            "Target.disposeBrowserContext" => {
                let context = params.get("browserContextId").and_then(Json::as_str).unwrap_or_default();
                match self.contexts.iter().position(|c| c == context) {
                    Some(index) => {
                        self.contexts.remove(index);
                        Ok(Json::object([]))
                    }
                    None => Err((-32602, "Failed to find context with id ".to_owned() + context)),
                }
            }
            _ => Err((-32601, format!("'{method}' wasn't found"))),
        }
    }

    /// Uma linha que a thread de uma página escreveu.
    fn on_target_line(&mut self, target: &str, line: &str) {
        let (kind, rest) = line.split_at(1.min(line.len()));
        let Ok(message) = json::parse(rest) else { return };
        match kind {
            "I" => {
                let title = message.get("title").and_then(Json::as_str).unwrap_or_default().to_owned();
                let url = message.get("url").and_then(Json::as_str).unwrap_or_default().to_owned();
                let Some(entry) = self.targets.iter_mut().find(|t| t.id == target) else { return };
                if entry.title == title && entry.url == url {
                    return;
                }
                entry.title = title;
                entry.url = url;
                if let Some(info) = self.info(target) {
                    self.broadcast_discovery("Target.targetInfoChanged", Json::object([("targetInfo", info)]));
                }
            }
            _ => {
                let Some(session_id) = message.get("sessionId").and_then(Json::as_str).map(str::to_owned) else { return };
                let Some(session) = self.sessions.iter().find(|s| s.id == session_id) else { return };
                let (key, implicit, flatten) = (session.conn, session.implicit, session.flatten);
                if implicit {
                    self.deliver(key, &with_field(&message, "sessionId", None));
                } else if flatten {
                    self.deliver(key, &message);
                } else {
                    let inner = with_field(&message, "sessionId", None).to_compact();
                    let event = Self::event(
                        "Target.receivedMessageFromTarget",
                        Json::object([("sessionId", Json::str(&session_id)), ("message", Json::str(inner)), ("targetId", Json::str(target))]),
                    );
                    self.deliver(key, &event);
                }
            }
        }
    }

    fn accept_upgrade(&mut self, key: u64, endpoint: &Endpoint) {
        if let Endpoint::Page(target) = endpoint {
            let session = token();
            self.sessions.push(Session { id: session, target: target.clone(), conn: key, implicit: true, flatten: true });
        }
    }

    fn drop_conn(&mut self, key: u64) {
        let sessions: Vec<String> = self.sessions.iter().filter(|s| s.conn == key).map(|s| s.id.clone()).collect();
        for session in sessions {
            self.detach(&session, false);
        }
        if let Some(index) = self.conns.iter().position(|c| c.key == key) {
            let conn = self.conns.remove(index);
            let _ = sysabi::sys::close(conn.fd);
        }
    }

    fn add_conn(&mut self, fd: Fd, transport: Transport) -> u64 {
        let key = self.next_key;
        self.next_key += 1;
        self.conns.push(Conn { key, fd, transport, inbox: Vec::new(), outbox: Vec::new(), discover: false, auto_attach: None, closing: false });
        key
    }

    /// Trata os bytes novos da conexão `key`.
    fn on_readable(&mut self, key: u64) {
        let Some(index) = self.conns.iter().position(|c| c.key == key) else { return };
        let mut buffer = vec![0u8; 65536];
        let read = sysabi::sys::read(self.conns[index].fd, &mut buffer);
        let n = match read {
            Ok(0) | Err(_) => {
                self.drop_conn(key);
                return;
            }
            Ok(n) => n,
        };
        self.conns[index].inbox.extend_from_slice(&buffer[..n]);
        loop {
            let Some(index) = self.conns.iter().position(|c| c.key == key) else { return };
            let conn = &mut self.conns[index];
            match &mut conn.transport {
                Transport::Pipe { .. } => {
                    let Some(end) = conn.inbox.iter().position(|&b| b == 0) else { return };
                    let raw: Vec<u8> = conn.inbox.drain(..=end).collect();
                    let text = String::from_utf8_lossy(&raw[..raw.len() - 1]).into_owned();
                    self.on_message(key, &text, &Endpoint::Browser);
                }
                Transport::WebSocket(decoder, endpoint) => {
                    let endpoint = endpoint.clone();
                    let messages = decoder.feed(&mut conn.inbox);
                    if messages.is_empty() {
                        return;
                    }
                    for incoming in messages {
                        match incoming {
                            Incoming::Text(text) => self.on_message(key, &text, &endpoint),
                            Incoming::Ping(payload) => {
                                if let Some(conn) = self.conn_mut(key) {
                                    conn.outbox.extend(websocket::encode(websocket::PONG, &payload));
                                }
                            }
                            Incoming::Close => {
                                if let Some(conn) = self.conn_mut(key) {
                                    conn.outbox.extend(websocket::encode(websocket::CLOSE, &[]));
                                    conn.closing = true;
                                }
                                return;
                            }
                        }
                    }
                }
                Transport::Http => {
                    let Some((request, used)) = http::parse_request(&conn.inbox) else { return };
                    conn.inbox.drain(..used);
                    let allowed = self.allowed_origins.clone();
                    match http::respond(&request, self, &allowed) {
                        Reply::Http(bytes) => {
                            if let Some(conn) = self.conn_mut(key) {
                                conn.outbox.extend(bytes);
                            }
                        }
                        Reply::Upgrade(bytes, endpoint) => {
                            self.accept_upgrade(key, &endpoint);
                            if let Some(conn) = self.conn_mut(key) {
                                conn.outbox.extend(bytes);
                                conn.transport = Transport::WebSocket(Decoder::default(), endpoint);
                            }
                        }
                        Reply::Close => {
                            if let Some(conn) = self.conn_mut(key) {
                                conn.closing = true;
                            }
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Escreve o que couber das saídas pendentes, sem bloquear.
    fn flush(&mut self) {
        for conn in &mut self.conns {
            let fd = match conn.transport {
                Transport::Pipe { output } => output,
                _ => conn.fd,
            };
            while !conn.outbox.is_empty() {
                match sysabi::sys::write(fd, &conn.outbox) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        conn.outbox.drain(..n);
                    }
                }
            }
        }
        for target in &mut self.targets {
            while !target.pending.is_empty() {
                match sysabi::sys::write(target.link.commands, &target.pending) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        target.pending.drain(..n);
                    }
                }
            }
        }
        let done: Vec<u64> = self.conns.iter().filter(|c| c.closing && c.outbox.is_empty()).map(|c| c.key).collect();
        for key in done {
            self.drop_conn(key);
        }
    }

    fn on_target_readable(&mut self, id: &str) {
        let Some(target) = self.targets.iter_mut().find(|t| t.id == id) else { return };
        let mut buffer = vec![0u8; 65536];
        match sysabi::sys::read(target.link.events, &mut buffer) {
            Ok(0) | Err(_) => {
                // A thread da página terminou (aba fechada pela própria página).
                let id = id.to_owned();
                self.close_target(&id);
                return;
            }
            Ok(n) => target.inbox.extend_from_slice(&buffer[..n]),
        }
        let mut lines = Vec::new();
        while let Some(end) = target.inbox.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = target.inbox.drain(..=end).collect();
            lines.push(String::from_utf8_lossy(&line[..line.len() - 1]).into_owned());
        }
        for line in lines {
            self.on_target_line(id, &line);
        }
    }

    /// O laço: espera conexão, mensagem ou saída de página, trata, e escreve o que ficou pendente.
    fn serve(&mut self, listener: Option<Fd>) {
        let sys = sysabi::sys::current();
        while !self.quit || self.conns.iter().any(|c| !c.outbox.is_empty()) {
            let mut fds = Vec::new();
            if let Some(listener) = listener {
                fds.push(PollFd { fd: listener, events: PollEvents::IN, revents: PollEvents::empty() });
            }
            for conn in &self.conns {
                let mut events = PollEvents::IN;
                if !conn.outbox.is_empty() {
                    events |= PollEvents::OUT;
                }
                fds.push(PollFd { fd: conn.fd, events, revents: PollEvents::empty() });
            }
            for target in &self.targets {
                fds.push(PollFd { fd: target.link.events, events: PollEvents::IN, revents: PollEvents::empty() });
            }
            let pending = self.targets.iter().any(|t| !t.pending.is_empty()) || self.conns.iter().any(|c| matches!(c.transport, Transport::Pipe { .. }) && !c.outbox.is_empty());
            let wait = if pending { Some(std::time::Duration::from_millis(1)) } else { None };
            if matches!(sys.poll(&mut fds, wait), Err(e) if e != sysabi::Errno::EINTR) {
                return;
            }
            let mut at = 0;
            if let Some(listener) = listener {
                if fds[0].revents.contains(PollEvents::IN) {
                    if let Ok((fd, _)) = sys.tcp_accept(listener, false, true) {
                        self.add_conn(fd, Transport::Http);
                    }
                }
                at = 1;
            }
            let conn_keys: Vec<(u64, PollEvents)> = self.conns.iter().zip(&fds[at..]).map(|(c, f)| (c.key, f.revents)).collect();
            let target_ids: Vec<(String, PollEvents)> = self.targets.iter().zip(&fds[at + conn_keys.len()..]).map(|(t, f)| (t.id.clone(), f.revents)).collect();
            for (key, revents) in conn_keys {
                if revents.intersects(PollEvents::IN | PollEvents::HUP | PollEvents::ERR) {
                    self.on_readable(key);
                }
            }
            for (id, revents) in target_ids {
                if revents.intersects(PollEvents::IN | PollEvents::HUP) {
                    self.on_target_readable(&id);
                }
            }
            self.finish_closing();
            self.flush();
        }
    }
}

/// O navegador com DevTools remoto: abre a primeira aba em `url`, anuncia o endereço no stderr e serve até o
/// `Browser.close`.
pub fn run(ctx: &mut Ctx, options: &Options, url: &str) -> i32 {
    let mut browser = Browser::new(options);
    let listener = match options.port {
        Some(port) if !options.pipe => sysabi::sys::tcp_listen_at(std::net::IpAddr::from([127, 0, 0, 1]), port, 128, true, true).ok().map(|(fd, bound)| {
            let _ = ctx.stderr().write_all(format!("\nDevTools listening on ws://127.0.0.1:{bound}/devtools/browser/{}\n", browser.browser_id).as_bytes());
            fd
        }),
        _ => None,
    };
    if options.pipe {
        browser.add_conn(Fd(3), Transport::Pipe { output: Fd(4) });
    }
    browser.open_target(url, true, false);
    browser.serve(listener);
    0
}
