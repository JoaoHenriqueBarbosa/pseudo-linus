//! De onde vêm os bytes de uma URL. A produção usa o cliente HTTP do `ul-net`; os testes injetam uma fita gravada.
//! É um ponto de injeção de dados, não de comportamento: o que o navegador faz com a resposta é o mesmo nos dois casos.

use std::rc::Rc;

use ul_common::time::webkit_now_micros;
use ul_net::net::client::{self, FetchError, RequestSpec, Response};

use crate::bridge::{origin_of, USER_AGENT};
use crate::http_cache::Captured;

/// Busca `url` (absoluta), seguindo redirecionamentos.
pub type Fetcher = Rc<dyn Fn(&str, &RequestSpec<'_>) -> Result<Response, FetchError>>;

/// O fetcher de produção: a pilha de rede do sandbox.
pub fn network() -> Fetcher {
    Rc::new(client::request)
}

/// O `/favicon.ico` da origem de `page_url`, que o navegador pede depois de carregar uma página `http` ou `https`
/// (a resposta, mesmo a 404, vai ao cache de disco).
pub fn favicon(fetcher: &Fetcher, page_url: &str) -> Option<Captured> {
    let origin = origin_of(page_url).filter(|origin| origin.starts_with("http"))?;
    let headers = vec![("User-Agent".to_owned(), USER_AGENT.to_owned()), ("Accept".to_owned(), "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8".to_owned())];
    let spec = RequestSpec { headers: &headers, ..RequestSpec::default() };
    let requested = webkit_now_micros();
    let response = fetcher(&format!("{origin}/favicon.ico"), &spec).ok()?;
    Some(Captured::from_response(&response, requested, webkit_now_micros()))
}
