//! A fiação da aba com o laço do navegador e o que a rede dela divide com o fetcher: a linha de comandos e eventos, os
//! subrecursos que a página pediu, o `Fetch` do DevTools (pausa, `continueRequest`, `fulfillRequest`, `failRequest`) e os
//! corpos que o `Network.getResponseBody` devolve. O fetcher roda dentro do `Page::load` e dos scripts da página, sem
//! acesso à `Tab`; por isso o que ele precisa mora aqui, atrás de `RefCell`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use sysabi::{Fd, PollEvents, PollFd};
use ul_net::net::client::Response;

use super::json::{self, Json};
use super::monotonic_seconds;
use super::network;
use super::overrides::{NetOverrides, Observation, Observed};
use crate::fetch::Kind;
use crate::netmeta::NetMeta;
use crate::session::Profile;

/// O que o `Fetch.enable` pediu para pausar: o padrão da URL (`*` e `?`), o tipo de recurso (vazio vale todos) e se a
/// pausa é na resposta.
struct Pattern {
    url: String,
    resource_type: String,
    response_stage: bool,
}

/// O que o cliente decidiu de um pedido pausado.
pub enum Verdict {
    Continue { url: Option<String>, headers: Option<Vec<(String, String)>> },
    Fulfill { status: u16, reason: String, phrase: String, headers: Vec<(String, String)>, body: Vec<u8> },
    Fail(&'static str),
    /// O pedido do navegador (o favicon) que o cliente não decidiu: fica retido até um `continueRequest` ou o `Fetch.disable`.
    Held(String),
    /// O laço do navegador fechou a aba enquanto o pedido esperava.
    Aborted,
}

/// O que o `Network` já mostrou de um pedido que o `Fetch` pausou.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Announced {
    /// `requestWillBeSent`.
    Request,
    /// `requestWillBeSent` e os dois `ExtraInfo` (a pausa foi na resposta).
    Response,
}

/// O pedido do navegador que o `Fetch` reteve: sem decisão do cliente, ele só segue no `continueRequest` ou no `Fetch.disable`.
pub struct HeldRequest {
    pub job: String,
    /// O `requestId` do `Network` e o que ele já mostrou.
    pub id: String,
    pub announced: Announced,
    pub started: f64,
    pub wall: f64,
    /// A página que o pediu e o espaço de endereços dela.
    pub page_url: String,
    pub space: &'static str,
    /// O documento que o pediu: se a aba já navegou, o renderer largou o pedido e a resposta não chega ao `Network`.
    pub loader_id: String,
    /// A resposta que o fio já trouxe, quando a retenção foi na resposta.
    pub response: Option<Response>,
    /// Como o cliente o liberou: seguir, ou falhar com o código de rede.
    pub failure: Option<&'static str>,
}

/// A resposta que o pedido pausado já tem (a pausa na resposta).
pub struct Held {
    body: Vec<u8>,
}

/// O pedido pausado que o fetcher espera decidir.
pub struct Pending {
    id: String,
    held: Option<Held>,
}

pub struct Hub {
    input: Fd,
    output: Fd,
    inbox: RefCell<Vec<u8>>,
    /// Comandos lidos e ainda não tratados (os que chegaram enquanto a navegação carregava).
    pub queue: RefCell<VecDeque<String>>,
    /// O pipe do laço do navegador fechou.
    pub closed: Cell<bool>,
    pub target_id: String,
    /// As sessões com `Network` ligado.
    pub network: RefCell<Vec<String>>,
    fetch: RefCell<Vec<(String, Vec<Pattern>)>>,
    pub loader_id: RefCell<String>,
    /// A URL do documento corrente e o HTML dele (o `initiator` de parser mostra a linha e a coluna da tag).
    pub url: RefCell<String>,
    pub source: RefCell<String>,
    /// O espaço de endereços do documento corrente (o `initiatorIPAddressSpace` dos subrecursos).
    pub space: Cell<&'static str>,
    /// Os subrecursos de parser que o documento corrente já pediu (o Chromium pede cada um uma vez).
    requested: RefCell<Vec<(Kind, String)>>,
    /// Quantos subrecursos (favicon incluído) a aba já relatou: o contador do `requestId` `pid.n`.
    resources: Cell<u32>,
    jobs: Cell<u32>,
    /// Os pedidos que o `Fetch` anunciou e seguiram sem esperar (o favicon): um `continueRequest` tardio responde `{}`.
    settled: RefCell<Vec<String>>,
    /// Os pedidos do navegador retidos, os que o cliente já liberou e o interruptor que deixa o pedido liberado passar sem pausa.
    held: RefCell<Vec<HeldRequest>>,
    released: RefCell<Vec<HeldRequest>>,
    pub unpaused: Cell<bool>,
    /// As sessões com `Page` ligado (espelho da aba): só elas veem os diálogos.
    pub pages: RefCell<Vec<String>>,
    /// O `requestId` e o que o `Network` já mostrou do pedido que o fetcher acabou de pausar.
    pub announced: RefCell<Option<(String, Announced)>>,
    /// Os subrecursos que o fetcher viu e ainda faltam relatar.
    pub observed: Observed,
    pub net: Rc<RefCell<NetOverrides>>,
    pub profile: Arc<Profile>,
    bodies: RefCell<Vec<(String, Vec<u8>, String)>>,
}

impl Hub {
    pub fn new(target_id: String, input: Fd, output: Fd, profile: Arc<Profile>) -> Hub {
        Hub {
            input,
            output,
            inbox: RefCell::new(Vec::new()),
            queue: RefCell::new(VecDeque::new()),
            closed: Cell::new(false),
            target_id,
            network: RefCell::new(Vec::new()),
            fetch: RefCell::new(Vec::new()),
            loader_id: RefCell::new(String::new()),
            url: RefCell::new(String::new()),
            source: RefCell::new(String::new()),
            space: Cell::new("Unknown"),
            requested: RefCell::new(Vec::new()),
            resources: Cell::new(0),
            jobs: Cell::new(0),
            settled: RefCell::new(Vec::new()),
            held: RefCell::new(Vec::new()),
            released: RefCell::new(Vec::new()),
            unpaused: Cell::new(false),
            pages: RefCell::new(Vec::new()),
            announced: RefCell::new(None),
            observed: Default::default(),
            net: Default::default(),
            profile,
            bodies: RefCell::new(Vec::new()),
        }
    }

    pub fn write_line(&self, kind: char, text: &str) {
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

    pub fn send(&self, message: &Json) {
        self.write_line('M', &message.to_compact());
    }

    pub fn reply(&self, request: &Json, result: Json) {
        let mut fields = vec![("id".to_owned(), request.get("id").cloned().unwrap_or(Json::Null)), ("result".to_owned(), result)];
        if let Some(session) = request.get("sessionId") {
            fields.push(("sessionId".to_owned(), session.clone()));
        }
        self.send(&Json::Object(fields));
    }

    pub fn fail(&self, request: &Json, code: i64, text: &str) {
        let error = Json::object([("code", Json::Number(code as f64)), ("message", Json::str(text))]);
        let mut fields = vec![("id".to_owned(), request.get("id").cloned().unwrap_or(Json::Null)), ("error".to_owned(), error)];
        if let Some(session) = request.get("sessionId") {
            fields.push(("sessionId".to_owned(), session.clone()));
        }
        self.send(&Json::Object(fields));
    }

    /// Um evento para cada sessão da lista.
    pub fn emit(&self, sessions: &[String], method: &str, params: &Json) {
        for session in sessions {
            self.send(&Json::object([("method", Json::str(method)), ("params", params.clone()), ("sessionId", Json::str(session))]));
        }
    }

    /// Lê o que o laço do navegador mandou, esperando até `wait_ms`. Falso quando o pipe fechou.
    pub fn receive(&self, wait_ms: u64) -> bool {
        let mut fds = [PollFd { fd: self.input, events: PollEvents::IN, revents: PollEvents::empty() }];
        let sys = sysabi::sys::current();
        if !matches!(sys.poll(&mut fds, Some(std::time::Duration::from_millis(wait_ms))), Ok(n) if n > 0) {
            return true;
        }
        let mut buffer = [0u8; 65536];
        let mut inbox = self.inbox.borrow_mut();
        loop {
            match sysabi::sys::read(self.input, &mut buffer) {
                Ok(0) => return false,
                Ok(n) => inbox.extend_from_slice(&buffer[..n]),
                Err(_) => break,
            }
        }
        while let Some(end) = inbox.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = inbox.drain(..=end).collect();
            self.queue.borrow_mut().push_back(String::from_utf8_lossy(&line[..line.len() - 1]).into_owned());
        }
        true
    }

    /// Um subrecurso novo no documento: zera o que é do documento anterior.
    pub fn new_document(&self, loader_id: &str, url: &str, source: &str, space: &'static str) {
        self.space.set(space);
        *self.loader_id.borrow_mut() = loader_id.to_owned();
        *self.url.borrow_mut() = url.to_owned();
        *self.source.borrow_mut() = source.to_owned();
        self.requested.borrow_mut().clear();
    }

    /// O `requestId` do próximo subrecurso: `pid.n`.
    pub fn next_resource_id(&self) -> String {
        self.resources.set(self.resources.get() + 1);
        format!("{}.{}", sysabi::sys::current().getpid(), self.resources.get() + 1)
    }

    /// Os cabeçalhos do pedido como o renderer os mostra no `requestWillBeSent`.
    pub fn renderer_headers(&self, kind: Kind) -> Vec<(String, String)> {
        let url = self.url.borrow().clone();
        let list: Vec<(&str, &str)> = if kind == Kind::Document {
            vec![
                ("Upgrade-Insecure-Requests", "1"),
                ("User-Agent", super::http::USER_AGENT),
                ("sec-ch-ua", "\"Not A(Brand\";v=\"99\", \"Chromium\";v=\"154\""),
                ("sec-ch-ua-mobile", "?0"),
                ("sec-ch-ua-platform", "\"Linux\""),
            ]
        } else {
            vec![
                ("sec-ch-ua-platform", "\"Linux\""),
                ("Referer", url.as_str()),
                ("User-Agent", super::http::USER_AGENT),
                ("sec-ch-ua", "\"Not A(Brand\";v=\"99\", \"Chromium\";v=\"154\""),
                ("sec-ch-ua-mobile", "?0"),
            ]
        };
        let mut headers: Vec<(String, String)> = list.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
        self.net.borrow().apply(&mut headers);
        headers
    }

    /// Os cookies que o pedido levou, como o `associatedCookies` do `requestWillBeSentExtraInfo`: só a troca que levou o
    /// `Cookie:` associa cookies (o pote de agora pode ter ganho um depois dela).
    pub fn associated_cookies(&self, exchange: &crate::netmeta::NetExchange) -> Json {
        if !exchange.sent.iter().any(|(name, _)| name.eq_ignore_ascii_case("cookie")) {
            return Json::Array(Vec::new());
        }
        crate::cookies::associated(&self.profile.cookies.lock().unwrap_or_else(std::sync::PoisonError::into_inner), &exchange.url)
    }

    /// O `initiator` do subrecurso (o parser, com a linha e a coluna da tag, ou o script) e se ele bloqueia a renderização.
    fn initiator_for(&self, kind: Kind, address: &str) -> (Json, bool) {
        let parsed = matches!(kind, Kind::Stylesheet | Kind::Script | Kind::Image | Kind::Font);
        let url = self.url.borrow().clone();
        match parsed.then(|| network::parser_position(&self.source.borrow(), &url, address)).flatten() {
            Some((line, column)) => (
                Json::object([("type", Json::str("parser")), ("url", Json::str(url)), ("lineNumber", Json::Number(line as f64)), ("columnNumber", Json::Number(column as f64))]),
                matches!(kind, Kind::Stylesheet | Kind::Script),
            ),
            None => (Json::object([("type", Json::str("script")), ("stack", Json::object([("callFrames", Json::Array(Vec::new()))]))]), false),
        }
    }

    /// O `requestWillBeSent` de um subrecurso (ou o do favicon).
    fn announce(&self, id: &str, kind: Kind, address: &str, initiator: Json, blocking: bool, started: f64, wall: f64) {
        let headers = self.renderer_headers(kind);
        let headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let url = self.url.borrow().clone();
        let host = |u: &str| url::Url::parse(u).ok().and_then(|u| u.host_str().map(str::to_owned));
        let same_site = host(&url) == host(address);
        let loader = self.loader_id.borrow().clone();
        let sent = network::request_will_be_sent(id, &loader, &self.target_id, &url, address, &headers, kind.cdp_type(), kind.priority(), same_site, initiator, None, blocking, started, wall);
        self.emit(&self.network.borrow(), "Network.requestWillBeSent", &sent);
    }

    /// Os eventos de `Network` de um subrecurso que o fetcher viu: `requestWillBeSent` (se a pausa do `Fetch` ainda não o
    /// mostrou), os `ExtraInfo` do fio, `responseReceived`, `dataReceived` e `loadingFinished`, ou `loadingFailed`. O
    /// pedido sai sempre, os eventos só vão a quem ligou `Network`.
    pub fn report(&self, observation: &Observation) {
        let kind = observation.kind;
        let announced = observation.announced.clone();
        if kind != Kind::Favicon && announced.is_none() && matches!(kind, Kind::Stylesheet | Kind::Script | Kind::Image | Kind::Font) {
            let key = (kind, observation.address.clone());
            if self.requested.borrow().contains(&key) {
                return;
            }
            self.requested.borrow_mut().push(key);
        }
        let (started, wall) = (observation.started, observation.wall);
        let sessions = self.network.borrow().clone();
        let id = match &announced {
            Some((id, _)) => id.clone(),
            None => {
                let id = self.next_resource_id();
                let (initiator, blocking) = if kind == Kind::Favicon { (Json::object([("type", Json::str("other"))]), false) } else { self.initiator_for(kind, &observation.address) };
                let address = observation.result.as_ref().map_or(observation.address.as_str(), |meta| meta.last.url.as_str());
                self.announce(&id, kind, address, initiator, blocking, started, wall);
                id
            }
        };
        let loader = self.loader_id.borrow().clone();
        let meta = match &observation.result {
            Ok(meta) => meta,
            Err(error) => {
                let failed = Json::object([("requestId", Json::str(&id)), ("timestamp", Json::Number(monotonic_seconds())), ("type", Json::str(kind.cdp_type())), ("errorText", Json::str(error)), ("canceled", Json::Bool(false))]);
                self.emit(&sessions, "Network.loadingFailed", &failed);
                return;
            }
        };
        let wire = !meta.last.synthetic;
        if wire && !matches!(announced, Some((_, Announced::Response))) {
            self.emit(&sessions, "Network.requestWillBeSentExtraInfo", &network::request_extra_info(&id, &meta.last, started, Some(observation.space.unwrap_or(self.space.get())), self.associated_cookies(&meta.last)));
            self.emit(&sessions, "Network.responseReceivedExtraInfo", &network::response_extra_info(&id, &meta.last));
        }
        if observation.stale {
            return;
        }
        let received = Json::object([
            ("requestId", Json::str(&id)),
            ("loaderId", Json::str(loader)),
            ("timestamp", Json::Number(monotonic_seconds())),
            ("type", Json::str(kind.cdp_type())),
            ("response", network::response(&meta.last, true, started, wall * 1000.0)),
            ("hasExtraInfo", Json::Bool(wire)),
            ("frameId", Json::str(&self.target_id)),
        ]);
        self.emit(&sessions, "Network.responseReceived", &received);
        let data = |length: usize, encoded: usize| Json::object([("requestId", Json::str(&id)), ("timestamp", Json::Number(monotonic_seconds())), ("dataLength", Json::Number(length as f64)), ("encodedDataLength", Json::Number(encoded as f64))]);
        if meta.data_length > 0 {
            // `fetch` e a resposta fornecida entregam o corpo de uma vez; o resto chega em duas partes: o conteúdo e depois os
            // bytes do fio.
            if kind == Kind::Fetch || meta.last.synthetic {
                self.emit(&sessions, "Network.dataReceived", &data(meta.data_length, if meta.last.synthetic { 0 } else { meta.body_encoded }));
            } else {
                self.emit(&sessions, "Network.dataReceived", &data(meta.data_length, 0));
                if kind != Kind::Favicon {
                    self.emit(&sessions, "Network.dataReceived", &data(0, meta.body_encoded));
                }
            }
        }
        let finished = Json::object([("requestId", Json::str(&id)), ("timestamp", Json::Number(monotonic_seconds())), ("encodedDataLength", Json::Number((meta.last.head.len() + meta.body_encoded) as f64))]);
        self.emit(&sessions, "Network.loadingFinished", &finished);
        self.store_body(&id, &meta.body, &meta.last.mime_type);
    }

    /// Guarda o corpo de uma resposta para o `Network.getResponseBody`.
    pub fn store_body(&self, id: &str, body: &[u8], mime: &str) {
        self.bodies.borrow_mut().push((id.to_owned(), body.to_vec(), mime.to_owned()));
    }

    /// `Network.getResponseBody`: texto para os tipos de texto, `base64` para o resto.
    pub fn response_body(&self, id: &str) -> Option<Json> {
        let bodies = self.bodies.borrow();
        let (_, body, mime) = bodies.iter().rev().find(|(i, _, _)| i == id)?;
        let text = mime.starts_with("text/") || mime.contains("json") || mime.contains("javascript") || mime.contains("xml");
        let (body, base64) = if text { (String::from_utf8_lossy(body).into_owned(), false) } else { (ul_common::codec::base64_string(body), true) };
        Some(Json::object([("body", Json::str(body)), ("base64Encoded", Json::Bool(base64))]))
    }

    /// Algum padrão do `Fetch.enable` pausa este pedido (ou esta resposta)?
    pub fn intercepts(&self, kind: Kind, address: &str, response_stage: bool) -> bool {
        if self.unpaused.get() {
            return false;
        }
        let parsed = matches!(kind, Kind::Stylesheet | Kind::Script | Kind::Image | Kind::Font);
        if parsed && self.requested.borrow().contains(&(kind, address.to_owned())) {
            return false;
        }
        self.fetch.borrow().iter().any(|(_, patterns)| {
            patterns.iter().any(|p| p.response_stage == response_stage && glob(&p.url, address) && (p.resource_type.is_empty() || p.resource_type.eq_ignore_ascii_case(kind.cdp_type())))
        })
    }

    /// O `networkId` do pedido: o `loaderId` no documento, `pid.n` no resto.
    fn network_id(&self, kind: Kind) -> String {
        if kind == Kind::Document { self.loader_id.borrow().clone() } else { self.next_resource_id() }
    }

    /// Pausa o pedido antes dele sair: mostra o `requestWillBeSent`, o `Fetch.requestPaused`, e espera o cliente decidir.
    pub fn intercept_request(&self, kind: Kind, address: &str, headers: &[(String, String)]) -> Verdict {
        let (started, wall) = (monotonic_seconds(), ul_common::time::realtime_nanos() as f64 / 1e9);
        let id = self.network_id(kind);
        if matches!(kind, Kind::Stylesheet | Kind::Script | Kind::Image | Kind::Font) {
            self.requested.borrow_mut().push((kind, address.to_owned()));
        }
        let (initiator, blocking) = if matches!(kind, Kind::Document | Kind::Favicon) { (Json::object([("type", Json::str("other"))]), false) } else { self.initiator_for(kind, address) };
        let announced_url = address;
        if kind == Kind::Document {
            // O documento mostra a própria URL como `documentURL`.
            let headers = self.renderer_headers(kind);
            let headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let sent = network::request_will_be_sent(&id, &id, &self.target_id, announced_url, announced_url, &headers, "Document", kind.priority(), true, initiator, None, false, started, wall);
            self.emit(&self.network.borrow(), "Network.requestWillBeSent", &sent);
        } else {
            self.announce(&id, kind, address, initiator, blocking, started, wall);
        }
        *self.announced.borrow_mut() = Some((id.clone(), Announced::Request));
        self.pause(kind, address, headers, &id, None, None)
    }

    /// Pausa a resposta que o fio já trouxe: `ExtraInfo` e `Fetch.requestPaused` com o status e os cabeçalhos dela.
    pub fn intercept_response(&self, kind: Kind, address: &str, headers: &[(String, String)], response: &Response, meta: &NetMeta) -> Verdict {
        let (started, wall) = (monotonic_seconds(), ul_common::time::realtime_nanos() as f64 / 1e9);
        let id = match self.announced.borrow().clone() {
            Some((id, _)) => id,
            None => {
                let id = self.network_id(kind);
                let (initiator, blocking) = if matches!(kind, Kind::Document | Kind::Favicon) { (Json::object([("type", Json::str("other"))]), false) } else { self.initiator_for(kind, address) };
                if kind == Kind::Document {
                    let headers = self.renderer_headers(kind);
                    let headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                    let sent = network::request_will_be_sent(&id, &id, &self.target_id, address, address, &headers, "Document", kind.priority(), true, initiator, None, false, started, wall);
                    self.emit(&self.network.borrow(), "Network.requestWillBeSent", &sent);
                } else {
                    self.announce(&id, kind, address, initiator, blocking, started, wall);
                }
                id
            }
        };
        let sessions = self.network.borrow().clone();
        self.emit(&sessions, "Network.requestWillBeSentExtraInfo", &network::request_extra_info(&id, &meta.last, started, (kind != Kind::Document).then(|| self.space.get()), self.associated_cookies(&meta.last)));
        self.emit(&sessions, "Network.responseReceivedExtraInfo", &network::response_extra_info(&id, &meta.last));
        *self.announced.borrow_mut() = Some((id.clone(), Announced::Response));
        self.pause(kind, address, headers, &id, Some(response), Some(Held { body: response.body.clone() }))
    }

    fn pause(&self, kind: Kind, address: &str, headers: &[(String, String)], network_id: &str, response: Option<&Response>, held: Option<Held>) -> Verdict {
        let jobs = self.jobs.get() + 1;
        self.jobs.set(jobs);
        let job = format!("interception-job-{jobs}.0");
        let shown = ["accept", "referer", "upgrade-insecure-requests", "user-agent"];
        let request_headers: Vec<(String, String)> = headers.iter().filter(|(k, _)| shown.contains(&k.to_ascii_lowercase().as_str()) || k.to_ascii_lowercase().starts_with("sec-ch-")).cloned().collect();
        let mut fields = vec![
            ("requestId", Json::str(&job)),
            (
                "request",
                Json::object([
                    ("url", Json::str(address)),
                    ("method", Json::str("GET")),
                    ("headers", Json::Object(request_headers.iter().map(|(k, v)| (k.clone(), Json::str(v))).collect())),
                    ("initialPriority", Json::str(kind.priority())),
                    ("referrerPolicy", Json::str("strict-origin-when-cross-origin")),
                ]),
            ),
            ("frameId", Json::str(&self.target_id)),
            ("resourceType", Json::str(kind.cdp_type())),
        ];
        if let Some(response) = response {
            fields.push(("responseStatusCode", Json::Number(f64::from(response.status))));
            fields.push(("responseStatusText", Json::str(&response.reason)));
            let list = response.headers.iter().map(|(n, v)| Json::object([("name", Json::str(String::from_utf8_lossy(n))), ("value", Json::str(String::from_utf8_lossy(v)))])).collect();
            fields.push(("responseHeaders", Json::Array(list)));
        }
        fields.push(("networkId", Json::str(network_id)));
        let params = Json::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect());
        let sessions: Vec<String> = self.fetch.borrow().iter().map(|(s, _)| s.clone()).collect();
        self.emit(&sessions, "Fetch.requestPaused", &params);
        // O favicon é do navegador: o pedido dele não espera o cliente, fica retido até ele o liberar.
        if kind == Kind::Favicon {
            let announced = self.announced.borrow_mut().take().map_or(Announced::Request, |(_, a)| a);
            let (started, wall) = (monotonic_seconds(), ul_common::time::realtime_nanos() as f64 / 1e9);
            let page_url = self.url.borrow().clone();
            self.held.borrow_mut().push(HeldRequest { job: job.clone(), id: network_id.to_owned(), announced, started, wall, page_url, space: self.space.get(), loader_id: self.loader_id.borrow().clone(), response: None, failure: None });
            return Verdict::Held(job);
        }
        self.wait(&Pending { id: job, held })
    }

    /// A resposta que o fio trouxe ao pedido retido (a retenção na resposta).
    pub fn attach_response(&self, job: &str, response: Response) {
        if let Some(held) = self.held.borrow_mut().iter_mut().find(|h| h.job == job) {
            held.response = Some(response);
        }
    }

    /// Libera o pedido retido `job` (todos sem `job`): ele passa a esperar a aba o terminar.
    fn release(&self, job: Option<&str>, failure: Option<&'static str>) {
        let mut held = self.held.borrow_mut();
        let mut index = 0;
        while index < held.len() {
            if job.is_none_or(|j| held[index].job == j) {
                let mut request = held.remove(index);
                request.failure = failure;
                self.settled.borrow_mut().push(request.job.clone());
                self.released.borrow_mut().push(request);
            } else {
                index += 1;
            }
        }
    }

    /// Os pedidos do navegador que o cliente liberou desde a última chamada.
    pub fn take_released(&self) -> Vec<HeldRequest> {
        std::mem::take(&mut *self.released.borrow_mut())
    }

    /// Um diálogo do documento (`alert`, `confirm`, `prompt`): o renderer para até o cliente responder
    /// (`Page.handleJavaScriptDialog`) ou uma navegação o dispensar; os outros comandos esperam na fila, como no Chromium.
    /// Sem sessão com `Page` ligado o diálogo não aparece para ninguém e só uma navegação o encerra. `None` é o recusado.
    pub fn dialog(&self, kind: &str, message: &str, default_prompt: &str) -> Option<String> {
        let mut notified = self.pages.borrow().clone();
        let mut showing = !notified.is_empty();
        let url = self.url.borrow().clone();
        let opening = Json::object([
            ("url", Json::str(&url)),
            ("frameId", Json::str(&self.target_id)),
            ("message", Json::str(message)),
            ("type", Json::str(kind)),
            ("hasBrowserHandler", Json::Bool(true)),
            ("defaultPrompt", Json::str(if kind == "prompt" { default_prompt } else { "" })),
        ]);
        self.emit(&notified, "Page.javascriptDialogOpening", &opening);
        let closed = |this: &Self, sessions: &[String], accepted: bool, input: &str| {
            let params = Json::object([("frameId", Json::str(&this.target_id)), ("result", Json::Bool(accepted)), ("userInput", Json::str(input))]);
            this.emit(sessions, "Page.javascriptDialogClosed", &params);
        };
        loop {
            let mut commands = Vec::new();
            {
                let mut queue = self.queue.borrow_mut();
                if queue.iter().any(|l| l == "X") {
                    self.closed.set(true);
                    return None;
                }
                let mut kept = VecDeque::new();
                for line in queue.drain(..) {
                    match line.strip_prefix('M').and_then(|text| json::parse(text).ok()) {
                        Some(message) if message.get("method").and_then(Json::as_str).is_some_and(|m| m == "Page.handleJavaScriptDialog") => commands.push(message),
                        Some(message) => {
                            let method = message.get("method").and_then(Json::as_str).unwrap_or_default();
                            match method {
                                // A aba desligou o `Page`: o diálogo continua aberto, mas já não aparece para a sessão.
                                "Page.disable" => showing = false,
                                "Page.navigate" | "Page.reload" | "Page.navigateToHistoryEntry" => commands.push(Json::str("dismiss")),
                                _ => {}
                            }
                            kept.push_back(line);
                        }
                        None => kept.push_back(line),
                    }
                }
                *queue = kept;
            }
            for command in commands {
                if command.as_str() == Some("dismiss") {
                    closed(self, &notified, false, "");
                    return None;
                }
                if !showing {
                    self.fail(&command, -32602, "No dialog is showing");
                    continue;
                }
                let params = command.get("params").cloned().unwrap_or(Json::Object(Vec::new()));
                let accepted = params.get("accept").and_then(Json::as_bool).unwrap_or(false);
                let typed = params.get("promptText").and_then(Json::as_str).unwrap_or_default().to_owned();
                let input = if kind == "prompt" && accepted { typed } else { String::new() };
                notified = self.pages.borrow().iter().cloned().chain(notified).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
                closed(self, &notified, accepted, &input);
                self.reply(&command, Json::object([]));
                return accepted.then_some(input);
            }
            if !self.receive(50) {
                self.closed.set(true);
                return None;
            }
        }
    }

    /// Espera o cliente decidir o pedido pausado; os outros comandos ficam na fila para depois.
    fn wait(&self, pending: &Pending) -> Verdict {
        loop {
            let mut fetch_commands = Vec::new();
            {
                let mut queue = self.queue.borrow_mut();
                if queue.iter().any(|l| l == "X") {
                    self.closed.set(true);
                    return Verdict::Aborted;
                }
                let mut kept = VecDeque::new();
                for line in queue.drain(..) {
                    match line.strip_prefix('M').and_then(|text| json::parse(text).ok()) {
                        Some(message) if message.get("method").and_then(Json::as_str).is_some_and(|m| m.starts_with("Fetch.")) => fetch_commands.push(message),
                        _ => kept.push_back(line),
                    }
                }
                *queue = kept;
            }
            let mut commands = fetch_commands.into_iter();
            while let Some(message) = commands.next() {
                if let Some(verdict) = self.fetch_command(&message, Some(pending)) {
                    // O que veio depois da decisão volta para o começo da fila, na ordem.
                    let rest: Vec<String> = commands.map(|m| format!("M{}", m.to_compact())).collect();
                    let mut queue = self.queue.borrow_mut();
                    for line in rest.into_iter().rev() {
                        queue.push_front(line);
                    }
                    return verdict;
                }
            }
            if !self.receive(50) {
                self.closed.set(true);
                return Verdict::Aborted;
            }
        }
    }

    /// Os comandos do domínio `Fetch`. Devolve a decisão quando o comando resolve o pedido `pending`.
    pub fn fetch_command(&self, message: &Json, pending: Option<&Pending>) -> Option<Verdict> {
        let method = message.get("method").and_then(Json::as_str).unwrap_or_default();
        let params = message.get("params").cloned().unwrap_or(Json::Object(Vec::new()));
        let session = message.get("sessionId").and_then(Json::as_str).unwrap_or_default().to_owned();
        match method {
            "Fetch.enable" => {
                let patterns: Vec<Pattern> = match params.get("patterns") {
                    Some(Json::Array(list)) if !list.is_empty() => list
                        .iter()
                        .map(|p| Pattern {
                            url: p.get("urlPattern").and_then(Json::as_str).unwrap_or("*").to_owned(),
                            resource_type: p.get("resourceType").and_then(Json::as_str).unwrap_or_default().to_owned(),
                            response_stage: p.get("requestStage").and_then(Json::as_str) == Some("Response"),
                        })
                        .collect(),
                    _ => vec![Pattern { url: "*".to_owned(), resource_type: String::new(), response_stage: false }],
                };
                let mut fetch = self.fetch.borrow_mut();
                fetch.retain(|(s, _)| *s != session);
                fetch.push((session, patterns));
                drop(fetch);
                self.reply(message, Json::object([]));
                None
            }
            "Fetch.disable" => {
                self.fetch.borrow_mut().retain(|(s, _)| *s != session);
                if self.fetch.borrow().is_empty() {
                    self.release(None, None);
                }
                self.reply(message, Json::object([]));
                None
            }
            "Fetch.continueRequest" | "Fetch.continueResponse" | "Fetch.fulfillRequest" | "Fetch.failRequest" | "Fetch.getResponseBody" => {
                let id = params.get("requestId").and_then(Json::as_str).unwrap_or_default();
                let current = pending.filter(|p| p.id == id);
                let Some(current) = current else {
                    let held = self.held.borrow().iter().position(|h| h.job == id);
                    if let Some(at) = held {
                        match method {
                            "Fetch.getResponseBody" => match &self.held.borrow()[at].response {
                                Some(response) => self.reply(message, Json::object([("body", Json::str(ul_common::codec::base64_string(&response.body))), ("base64Encoded", Json::Bool(true))])),
                                None => self.fail(message, -32000, "Can only get response body on requests captured after headers received."),
                            },
                            "Fetch.failRequest" => match params.get("errorReason").and_then(Json::as_str).and_then(error_code) {
                                Some(code) => {
                                    self.release(Some(id), Some(code));
                                    self.reply(message, Json::object([]));
                                }
                                None => self.fail(message, -32602, "Invalid errorReason"),
                            },
                            _ => {
                                self.release(Some(id), None);
                                self.reply(message, Json::object([]));
                            }
                        }
                        return None;
                    }
                    if self.settled.borrow().iter().any(|j| j == id) {
                        self.reply(message, Json::object([]));
                    } else {
                        self.fail(message, -32602, "Invalid InterceptionId.");
                    }
                    return None;
                };
                match method {
                    "Fetch.getResponseBody" => {
                        match &current.held {
                            Some(held) => self.reply(message, Json::object([("body", Json::str(ul_common::codec::base64_string(&held.body))), ("base64Encoded", Json::Bool(true))])),
                            None => self.fail(message, -32000, "Can only get response body on requests captured after headers received."),
                        }
                        None
                    }
                    "Fetch.failRequest" => {
                        let reason = params.get("errorReason").and_then(Json::as_str).unwrap_or_default();
                        let Some(code) = error_code(reason) else {
                            self.fail(message, -32602, "Invalid errorReason");
                            return None;
                        };
                        self.reply(message, Json::object([]));
                        Some(Verdict::Fail(code))
                    }
                    "Fetch.fulfillRequest" => {
                        let status = params.get("responseCode").and_then(Json::as_int).unwrap_or(200) as u16;
                        let phrase = params.get("responsePhrase").and_then(Json::as_str).unwrap_or_default().to_owned();
                        let reason = if phrase.is_empty() { status_text(status).to_owned() } else { phrase.clone() };
                        let headers = match params.get("responseHeaders") {
                            Some(Json::Array(list)) => list.iter().filter_map(|h| Some((h.get("name")?.as_str()?.to_owned(), h.get("value")?.as_str()?.to_owned()))).collect(),
                            _ => Vec::new(),
                        };
                        let body = params.get("body").and_then(Json::as_str).and_then(|b| ul_common::codec::base64_decode(b.as_bytes(), false).ok()).unwrap_or_default();
                        self.reply(message, Json::object([]));
                        Some(Verdict::Fulfill { status, reason, phrase, headers, body })
                    }
                    _ => {
                        let url = params.get("url").and_then(Json::as_str).map(str::to_owned);
                        let headers = match params.get("headers") {
                            Some(Json::Array(list)) => Some(list.iter().filter_map(|h| Some((h.get("name")?.as_str()?.to_owned(), h.get("value")?.as_str()?.to_owned()))).collect()),
                            _ => None,
                        };
                        self.reply(message, Json::object([]));
                        Some(Verdict::Continue { url, headers })
                    }
                }
            }
            _ => {
                self.fail(message, -32601, &format!("'{method}' wasn't found"));
                None
            }
        }
    }
}

/// O código de rede do `Network.ErrorReason`.
fn error_code(reason: &str) -> Option<&'static str> {
    Some(match reason {
        "Failed" => "net::ERR_FAILED",
        "Aborted" => "net::ERR_ABORTED",
        "TimedOut" => "net::ERR_TIMED_OUT",
        "AccessDenied" => "net::ERR_ACCESS_DENIED",
        "ConnectionClosed" => "net::ERR_CONNECTION_CLOSED",
        "ConnectionReset" => "net::ERR_CONNECTION_RESET",
        "ConnectionRefused" => "net::ERR_CONNECTION_REFUSED",
        "ConnectionAborted" => "net::ERR_CONNECTION_ABORTED",
        "ConnectionFailed" => "net::ERR_CONNECTION_FAILED",
        "NameNotResolved" => "net::ERR_NAME_NOT_RESOLVED",
        "InternetDisconnected" => "net::ERR_INTERNET_DISCONNECTED",
        "AddressUnreachable" => "net::ERR_ADDRESS_UNREACHABLE",
        "BlockedByClient" => "net::ERR_BLOCKED_BY_CLIENT",
        "BlockedByResponse" => "net::ERR_BLOCKED_BY_RESPONSE",
        _ => return None,
    })
}

/// A frase padrão do status (`Fetch.fulfillRequest` sem `responsePhrase`).
pub fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        410 => "Gone",
        418 => "I'm a teapot",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

/// O padrão de URL do `Fetch`: `*` casa qualquer trecho e `?` um caractere.
fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) && p[pi] != '*' {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}
