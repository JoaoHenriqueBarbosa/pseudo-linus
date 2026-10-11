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

/// O `/favicon.ico` da origem de `page_url`, que o navegador pede depois de carregar uma página `http` ou `https`,
/// com os cabeçalhos que o Chromium 154 mandou ao oráculo.
pub fn favicon_response(fetcher: &Fetcher, page_url: &str) -> Option<Response> {
    let origin = origin_of(page_url).filter(|origin| origin.starts_with("http"))?;
    let headers: Vec<(String, String)> = [
        ("Accept", "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8"),
        ("Accept-Language", "en-US,en;q=0.9"),
        ("Connection", "keep-alive"),
        ("Referer", page_url),
        ("Sec-Fetch-Dest", "image"),
        ("Sec-Fetch-Mode", "no-cors"),
        ("Sec-Fetch-Site", "same-origin"),
        ("User-Agent", USER_AGENT),
        ("sec-ch-ua", "\"Not A(Brand\";v=\"99\", \"Chromium\";v=\"154\""),
        ("sec-ch-ua-mobile", "?0"),
        ("sec-ch-ua-platform", "\"Linux\""),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let spec = RequestSpec { headers: &headers, timeout: Some(std::time::Duration::from_secs(3)), ..RequestSpec::default() };
    fetcher(&format!("{origin}/favicon.ico"), &spec).ok()
}

/// O `favicon_response` como o cache de disco o guarda (a resposta, mesmo a 404, vai ao cache).
pub fn favicon(fetcher: &Fetcher, page_url: &str) -> Option<Captured> {
    let requested = webkit_now_micros();
    let response = favicon_response(fetcher, page_url)?;
    Some(Captured::from_response(&response, requested, webkit_now_micros()))
}
