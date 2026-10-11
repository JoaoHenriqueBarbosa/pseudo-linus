//! O que o DevTools muda na rede da aba (`Network.setExtraHTTPHeaders`, `Emulation.setUserAgentOverride`,
//! `Network.emulateNetworkConditions`) e o `Fetcher` que o aplica a todo pedido da aba, com o pote de cookies do perfil.

use std::cell::RefCell;
use std::rc::Rc;

use ul_net::net::client::{FetchError, RequestSpec, Response, Transfer};

use super::hub::{Announced, Hub, Verdict};

use crate::fetch::{self, Fetcher};
use crate::netmeta::NetMeta;

/// As substituições de cabeçalho e a rede desligada. Campo vazio não substitui nada.
#[derive(Clone, Debug, Default)]
pub struct NetOverrides {
    pub user_agent: String,
    pub accept_language: String,
    /// A plataforma de `userAgentMetadata`, que vira o `sec-ch-ua-platform`.
    pub platform: String,
    pub extra: Vec<(String, String)>,
    pub offline: bool,
}

fn set(headers: &mut Vec<(String, String)>, name: &str, value: &str) {
    match headers.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
        Some(header) => header.1 = value.to_owned(),
        None => headers.push((name.to_owned(), value.to_owned())),
    }
}

impl NetOverrides {
    /// Põe as substituições num conjunto de cabeçalhos: o do pedido no fio e o que o renderer mostra no
    /// `requestWillBeSent`.
    pub fn apply(&self, headers: &mut Vec<(String, String)>) {
        if !self.user_agent.is_empty() {
            set(headers, "User-Agent", &self.user_agent);
        }
        if !self.accept_language.is_empty() {
            set(headers, "Accept-Language", &self.accept_language);
        }
        if !self.platform.is_empty() {
            set(headers, "sec-ch-ua-platform", &format!("\"{}\"", self.platform));
        }
        for (name, value) in &self.extra {
            set(headers, name, value);
        }
    }
}

/// Um subrecurso que a página pediu (folha, script, imagem, `fetch`, XHR): o que o DevTools mostra dele.
pub struct Observation {
    pub kind: fetch::Kind,
    pub address: String,
    /// O relógio monotônico e o de parede (segundos) quando o pedido saiu.
    pub started: f64,
    pub wall: f64,
    /// A troca, ou o erro de rede (`net::ERR_...`).
    pub result: Result<NetMeta, String>,
    /// O `requestId` e o que o `Fetch` já mostrou do pedido, quando ele o pausou.
    pub announced: Option<(String, Announced)>,
    /// O espaço de endereços do documento que o pediu, quando não é o corrente (o pedido retido).
    pub space: Option<&'static str>,
    /// O documento que o pediu já foi embora: só os `ExtraInfo` do fio saem.
    pub stale: bool,
}

/// Os subrecursos que o fetcher da aba viu e o laço da aba ainda não relatou.
pub type Observed = Rc<RefCell<Vec<Observation>>>;

/// O que o cliente deu ao `Fetch.fulfillRequest`.
struct Fulfilled {
    status: u16,
    reason: String,
    phrase: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// A resposta que o cliente forneceu ao `Fetch.fulfillRequest`: sem conexão, a cabeça montada como o Chromium a mostra.
fn synthetic(address: &str, fulfilled: Fulfilled, sent: Vec<(String, String)>) -> Result<Response, FetchError> {
    let Fulfilled { status, reason, phrase, headers, body } = fulfilled;
    let url = ul_net::net::url::parse(address, false).map_err(FetchError::InvalidUrl)?;
    // O Chromium mede a cabeça sintética nos cabeçalhos crus, separados por NUL e sem a frase quando o cliente não a deu.
    let mut head = format!("HTTP/1.1 {status} {phrase}\0");
    for (name, value) in &headers {
        head.push_str(&format!("{name}: {value}\0"));
    }
    head.push('\0');
    let transfer = Transfer { version: (1, 1), encoded_size: body.len(), redirects: 0, timing: None };
    let headers = headers.into_iter().map(|(n, v)| (n.into_bytes(), v.into_bytes())).collect();
    Ok(Response { status, reason, headers, body, url, transfer, head: head.into_bytes(), sent, hops: Vec::new(), synthetic: true })
}

/// O fetcher da aba: a rede do sandbox com as substituições vigentes, o `Cookie:` do pote e o `Set-Cookie` da resposta, e a
/// pausa do `Fetch` do DevTools. Os pedidos marcados com um destino ([`fetch::tagged`]) entram em `hub.observed`.
pub fn fetcher(hub: Rc<Hub>) -> Fetcher {
    let base = fetch::network();
    Rc::new(move |url: &str, spec: &RequestSpec<'_>| {
        let kind = fetch::take_tag();
        let started = (super::monotonic_seconds(), ul_common::time::realtime_nanos() as f64 / 1e9);
        let observe = |result: Result<NetMeta, String>| {
            // O documento e o favicon são relatados por quem os pediu.
            if let Some(kind) = kind.filter(|k| !matches!(k, fetch::Kind::Document | fetch::Kind::Favicon)) {
                let announced = hub.announced.borrow_mut().take();
                hub.observed.borrow_mut().push(Observation { kind, address: url.to_owned(), started: started.0, wall: started.1, result, announced, space: None, stale: false });
            }
        };
        let overrides = hub.net.borrow().clone();
        if overrides.offline {
            observe(Err(FetchError::Offline.net_error().to_owned()));
            return Err(FetchError::Offline);
        }
        let mut headers = spec.headers.to_vec();
        overrides.apply(&mut headers);
        let mut target = url.to_owned();
        if let Some(kind) = kind.filter(|k| hub.intercepts(*k, url, false)) {
            match hub.intercept_request(kind, url, &headers) {
                Verdict::Continue { url: rewritten, headers: replaced } => {
                    if let Some(rewritten) = rewritten {
                        target = rewritten;
                    }
                    // Os `headers` do `continueRequest` substituem os do renderer; o que a pilha de rede põe (`User-Agent`,
                    // `Accept-Language`, `Connection`, `Sec-Fetch-*`) fica.
                    if let Some(replaced) = replaced {
                        headers.retain(|(k, _)| {
                            let k = k.to_ascii_lowercase();
                            matches!(k.as_str(), "user-agent" | "accept-language" | "connection") || k.starts_with("sec-fetch-")
                        });
                        for (name, value) in replaced {
                            set(&mut headers, &name, &value);
                        }
                    }
                }
                Verdict::Fulfill { status, reason, phrase, headers: given, body } => {
                    let response = synthetic(&target, Fulfilled { status, reason, phrase, headers: given, body }, headers)?;
                    observe(Ok(NetMeta::from_response(&response)));
                    return Ok(response);
                }
                Verdict::Fail(code) => {
                    observe(Err(code.to_owned()));
                    return Err(FetchError::Blocked(code));
                }
                Verdict::Held(_) => return Err(FetchError::Blocked("net::ERR_ABORTED")),
                Verdict::Aborted => {
                    observe(Err("net::ERR_ABORTED".to_owned()));
                    return Err(FetchError::Blocked("net::ERR_ABORTED"));
                }
            }
        }
        let mut response = match base(&target, &RequestSpec { method: spec.method, headers: &headers, body: spec.body, max_redirects: spec.max_redirects, timeout: spec.timeout, cookies: Some(&*hub.profile) }) {
            Ok(response) => response,
            Err(error) => {
                observe(Err(error.net_error().to_owned()));
                return Err(error);
            }
        };
        // A URL que o `continueRequest` reescreveu só vale no fio: o documento e o `Network` seguem com a original.
        if target != url {
            if let Ok(original) = ul_net::net::url::parse(url, false) {
                response.url = original;
            }
        }
        // A pausa na resposta vale para o pedido sem redirecionamento: o `Fetch` do Chromium pausa cada salto.
        if let Some(kind) = kind.filter(|k| response.hops.is_empty() && hub.intercepts(*k, url, true)) {
            let meta = NetMeta::from_response(&response);
            match hub.intercept_response(kind, url, &headers, &response, &meta) {
                Verdict::Continue { .. } => {}
                Verdict::Fulfill { status, reason, phrase, headers: given, body } => response = synthetic(&target, Fulfilled { status, reason, phrase, headers: given, body }, headers)?,
                Verdict::Fail(code) => {
                    observe(Err(code.to_owned()));
                    return Err(FetchError::Blocked(code));
                }
                Verdict::Held(job) => {
                    hub.attach_response(&job, response);
                    return Err(FetchError::Blocked("net::ERR_ABORTED"));
                }
                Verdict::Aborted => {
                    observe(Err("net::ERR_ABORTED".to_owned()));
                    return Err(FetchError::Blocked("net::ERR_ABORTED"));
                }
            }
        }
        observe(Ok(NetMeta::from_response(&response)));
        Ok(response)
    })
}
