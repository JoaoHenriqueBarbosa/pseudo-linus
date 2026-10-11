//! O que o DevTools muda na rede da aba (`Network.setExtraHTTPHeaders`, `Emulation.setUserAgentOverride`,
//! `Network.emulateNetworkConditions`) e o `Fetcher` que o aplica a todo pedido da aba, com o pote de cookies do perfil.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use ul_net::net::client::{FetchError, RequestSpec};

use crate::cookies;
use crate::fetch::{self, Fetcher};
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

/// O fetcher da aba: a rede do sandbox com as substituições vigentes, o `Cookie:` do pote e o `Set-Cookie` da resposta.
pub fn fetcher(net: Rc<RefCell<NetOverrides>>, profile: Arc<Profile>) -> Fetcher {
    let base = fetch::network();
    Rc::new(move |url: &str, spec: &RequestSpec<'_>| {
        let overrides = net.borrow().clone();
        if overrides.offline {
            return Err(FetchError::Offline);
        }
        let mut headers = spec.headers.to_vec();
        overrides.apply(&mut headers);
        let scope = cookies::scope(url);
        if let Some((host, path, secure)) = &scope {
            let jar = profile.cookies.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(cookie) = jar.header_for(host, path, *secure, cookies::now()).filter(|_| !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("cookie"))) {
                headers.push(("Cookie".to_owned(), cookie));
            }
        }
        let response = base(url, &RequestSpec { method: spec.method, headers: &headers, body: spec.body, max_redirects: spec.max_redirects, timeout: spec.timeout })?;
        if scope.is_some() {
            cookies::store_response(&mut profile.cookies.lock().unwrap_or_else(std::sync::PoisonError::into_inner), &response);
        }
        Ok(response)
    })
}
