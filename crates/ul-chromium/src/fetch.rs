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

/// O destino de um subrecurso: o que o Chromium põe em `Sec-Fetch-Dest`, no `Accept` e no `type` dos eventos de `Network`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Stylesheet,
    Script,
    Image,
    Font,
    Fetch,
    Xhr,
    Other,
}

impl Kind {
    /// O nome que o prelúdio manda na ponte (`netFetch`).
    pub fn from_name(name: &str) -> Kind {
        match name {
            "stylesheet" => Kind::Stylesheet,
            "script" => Kind::Script,
            "image" => Kind::Image,
            "xhr" => Kind::Xhr,
            "fetch" => Kind::Fetch,
            _ => Kind::Other,
        }
    }

    /// O destino do que o Blitz pede só pela URL: a extensão decide (imagem é o resto).
    pub fn infer(address: &str) -> Kind {
        let path = address.split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
        match path.rsplit('.').next().unwrap_or_default() {
            "css" => Kind::Stylesheet,
            "woff" | "woff2" | "ttf" | "otf" => Kind::Font,
            _ => Kind::Image,
        }
    }

    /// O `type` do `ResourceType` do protocolo.
    pub fn cdp_type(self) -> &'static str {
        match self {
            Kind::Stylesheet => "Stylesheet",
            Kind::Script => "Script",
            Kind::Image => "Image",
            Kind::Font => "Font",
            Kind::Fetch => "Fetch",
            Kind::Xhr => "XHR",
            Kind::Other => "Other",
        }
    }

    pub fn priority(self) -> &'static str {
        match self {
            Kind::Stylesheet => "VeryHigh",
            Kind::Script | Kind::Font | Kind::Fetch | Kind::Xhr => "High",
            Kind::Image | Kind::Other => "Medium",
        }
    }

    fn accept(self) -> &'static str {
        match self {
            Kind::Stylesheet => "text/css,*/*;q=0.1",
            Kind::Image => "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8",
            _ => "*/*",
        }
    }

    fn dest(self) -> &'static str {
        match self {
            Kind::Stylesheet => "style",
            Kind::Script => "script",
            Kind::Image => "image",
            Kind::Font => "font",
            Kind::Fetch | Kind::Xhr | Kind::Other => "empty",
        }
    }

    fn mode(self) -> &'static str {
        match self {
            Kind::Fetch | Kind::Xhr | Kind::Font => "cors",
            _ => "no-cors",
        }
    }
}

thread_local! {
    /// O destino do pedido que está para sair: quem busca o marca, o fetcher da aba o lê e o apaga.
    static TAG: std::cell::Cell<Option<Kind>> = const { std::cell::Cell::new(None) };
}

/// Roda `f` com o pedido que ele fizer marcado como `kind`, para o fetcher da aba relatar ao DevTools.
pub fn tagged<T>(kind: Kind, f: impl FnOnce() -> T) -> T {
    TAG.with(|tag| tag.set(Some(kind)));
    let out = f();
    TAG.with(|tag| tag.set(None));
    out
}

/// O destino marcado para o pedido corrente (e o apaga).
pub fn take_tag() -> Option<Kind> {
    TAG.with(std::cell::Cell::take)
}

/// A relação entre o documento e o alvo do pedido, como o `Sec-Fetch-Site`.
fn site_relation(document_url: &str, target: &str) -> &'static str {
    let (Some(a), Some(b)) = (origin_of(document_url), origin_of(target)) else { return "cross-site" };
    if a == b {
        return "same-origin";
    }
    let host = |origin: &str| origin.split_once("://").map(|(_, h)| h.split(':').next().unwrap_or_default().to_owned()).unwrap_or_default();
    if host(&a) == host(&b) { "same-site" } else { "cross-site" }
}

/// Os cabeçalhos que o Chromium manda num subrecurso de um documento `http(s)`. Vazio para os outros documentos.
pub fn subresource_headers(kind: Kind, document_url: &str, target: &str) -> Vec<(String, String)> {
    if origin_of(document_url).is_none() {
        return Vec::new();
    }
    let relation = site_relation(document_url, target);
    // `strict-origin-when-cross-origin`: o mesmo origin leva a URL, o resto só a origem.
    let referer = if relation == "same-origin" { document_url.split('#').next().unwrap_or(document_url).to_owned() } else { format!("{}/", origin_of(document_url).unwrap_or_default()) };
    [
        ("Accept", kind.accept().to_owned()),
        ("Accept-Language", "en-US,en;q=0.9".to_owned()),
        ("Referer", referer),
        ("Sec-Fetch-Dest", kind.dest().to_owned()),
        ("Sec-Fetch-Mode", kind.mode().to_owned()),
        ("Sec-Fetch-Site", relation.to_owned()),
        ("User-Agent", USER_AGENT.to_owned()),
        ("sec-ch-ua", "\"Not A(Brand\";v=\"99\", \"Chromium\";v=\"154\"".to_owned()),
        ("sec-ch-ua-mobile", "?0".to_owned()),
        ("sec-ch-ua-platform", "\"Linux\"".to_owned()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect()
}

/// Busca `address` como subrecurso do documento: com os cabeçalhos do Chromium e o destino marcado para o DevTools.
pub fn subresource(fetcher: &Fetcher, kind: Kind, document_url: &str, address: &str) -> Result<Response, FetchError> {
    let mut headers = subresource_headers(kind, document_url, address);
    if headers.is_empty() {
        headers = vec![("User-Agent".to_owned(), USER_AGENT.to_owned()), ("Accept".to_owned(), kind.accept().to_owned())];
    }
    let spec = RequestSpec { headers: &headers, ..RequestSpec::default() };
    tagged(kind, || fetcher(address, &spec))
}
