//! O que o DevTools muda na rede da aba (`Network.setExtraHTTPHeaders`, `Emulation.setUserAgentOverride`,
//! `Network.emulateNetworkConditions`) e o `Fetcher` que o aplica a todo pedido da aba, com o pote de cookies do perfil.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use ul_net::net::client::{FetchError, RequestSpec};

use crate::fetch::{self, Fetcher};
use crate::netmeta::NetMeta;
use crate::session::Profile;

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
}

/// Os subrecursos que o fetcher da aba viu e o laço da aba ainda não relatou.
pub type Observed = Rc<RefCell<Vec<Observation>>>;

/// O fetcher da aba: a rede do sandbox com as substituições vigentes, o `Cookie:` do pote e o `Set-Cookie` da resposta.
/// Os pedidos marcados com um destino ([`fetch::tagged`]) entram em `observed`.
pub fn fetcher(net: Rc<RefCell<NetOverrides>>, profile: Arc<Profile>, observed: Observed) -> Fetcher {
    let base = fetch::network();
    Rc::new(move |url: &str, spec: &RequestSpec<'_>| {
        let kind = fetch::take_tag();
        let started = (super::monotonic_seconds(), ul_common::time::realtime_nanos() as f64 / 1e9);
        let observe = |result: Result<NetMeta, String>| {
            if let Some(kind) = kind {
                observed.borrow_mut().push(Observation { kind, address: url.to_owned(), started: started.0, wall: started.1, result });
            }
        };
        let overrides = net.borrow().clone();
        if overrides.offline {
            observe(Err(FetchError::Offline.net_error().to_owned()));
            return Err(FetchError::Offline);
        }
        let mut headers = spec.headers.to_vec();
        overrides.apply(&mut headers);
        let response = match base(url, &RequestSpec { method: spec.method, headers: &headers, body: spec.body, max_redirects: spec.max_redirects, timeout: spec.timeout, cookies: Some(&*profile) }) {
            Ok(response) => response,
            Err(error) => {
                observe(Err(error.net_error().to_owned()));
                return Err(error);
            }
        };
        observe(Ok(NetMeta::from_response(&response)));
        Ok(response)
    })
}
