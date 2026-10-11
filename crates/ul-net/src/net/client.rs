//! Cliente HTTP de uma chamada só (`request`), para quem embute a pilha de rede sem passar pela CLI do curl: o
//! navegador headless busca a página, os scripts e o que `fetch` e `XMLHttpRequest` pedem por aqui. Segue
//! redirecionamentos, decodifica `Content-Encoding` e devolve o corpo inteiro.

use std::io::Write as _;
use std::time::Duration;

use sysabi::{Errno, sys};

use super::http::{self, BodyMode, Conn, Decoder, HttpError, Stream};
use super::io;
use super::tls;
use super::url::{self, Url, UrlError};

/// A resposta final de [`request`].
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    /// Cabeçalhos como chegaram (nome e valor).
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
    /// Corpo já decodificado (`Content-Encoding`).
    pub body: Vec<u8>,
    /// A URL depois dos redirecionamentos.
    pub url: Url,
    /// O que o Resource Timing do navegador mostra da troca.
    pub transfer: Transfer,
    /// A cabeça da resposta como chegou no fio (a `headersText` do DevTools).
    pub head: Vec<u8>,
    /// Os cabeçalhos que o pedido levou, na ordem em que saíram.
    pub sent: Vec<(String, String)>,
    /// Os redirecionamentos seguidos antes da resposta final, em ordem.
    pub hops: Vec<Hop>,
}

/// Um redirecionamento seguido: a troca que respondeu com a `Location`.
#[derive(Debug, Clone)]
pub struct Hop {
    pub url: Url,
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
    pub head: Vec<u8>,
    pub sent: Vec<(String, String)>,
    pub version: (u8, u8),
}

/// A troca vista de fora do corpo: a versão do protocolo da resposta final, o corpo como veio no fio (antes do
/// `Content-Encoding`), quantos redirecionamentos levaram até ela e quando cada etapa da troca final aconteceu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transfer {
    pub version: (u8, u8),
    pub encoded_size: usize,
    pub redirects: u32,
    pub timing: Option<Timing>,
}

/// Os instantes da troca final no relógio monotônico (`ul_common::time::monotonic_nanos`), o que o Resource Timing
/// mostra: a conexão (com o aperto de mão TLS no fim dela, quando há), o envio do pedido e a chegada da resposta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub connect_start: i128,
    /// O começo do TLS (`secureConnectionStart`), em `https`.
    pub secure_start: Option<i128>,
    pub connect_end: i128,
    pub request_start: i128,
    pub response_start: i128,
    pub response_end: i128,
}

impl Transfer {
    /// Uma resposta HTTP/1.1 de corpo sem codificação, a primeira da cadeia, sem os instantes da troca.
    pub fn identity(body_len: usize) -> Transfer {
        Transfer { version: (1, 1), encoded_size: body_len, redirects: 0, timing: None }
    }

    /// O `nextHopProtocol` do Chromium (`http/1.1`, `http/1.0`, `h2`).
    pub fn protocol(&self) -> String {
        match self.version {
            (2, _) => "h2".to_owned(),
            (major, minor) => format!("http/{major}.{minor}"),
        }
    }
}

impl Response {
    /// Primeiro valor do cabeçalho `name`, sem diferenciar caixa.
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.as_bytes())).map(|(_, v)| String::from_utf8_lossy(v).into_owned())
    }
}

/// Por que o pedido falhou, no vocabulário do que o navegador mostra (`net::ERR_*`).
#[derive(Debug, PartialEq, Eq)]
pub enum FetchError {
    InvalidUrl(UrlError),
    /// `net::ERR_NAME_NOT_RESOLVED`.
    NameNotResolved,
    /// `net::ERR_CONNECTION_REFUSED`.
    ConnectionRefused,
    /// `net::ERR_CONNECTION_TIMED_OUT`.
    TimedOut,
    /// `net::ERR_CERT_AUTHORITY_INVALID` e afins.
    Tls(String),
    /// `net::ERR_EMPTY_RESPONSE`.
    EmptyResponse,
    /// `net::ERR_INVALID_HTTP_RESPONSE`.
    InvalidResponse,
    /// `net::ERR_TOO_MANY_REDIRECTS`.
    TooManyRedirects,
    /// `net::ERR_CONNECTION_RESET` e outros erros de E/S no meio da troca.
    Io(String),
    /// `net::ERR_UNKNOWN_URL_SCHEME`.
    UnknownScheme(String),
    /// `net::ERR_INTERNET_DISCONNECTED`: o navegador está emulando a rede desligada.
    Offline,
}

impl FetchError {
    /// O código de erro de rede do Chromium.
    pub fn net_error(&self) -> &'static str {
        match self {
            FetchError::InvalidUrl(_) => "net::ERR_INVALID_URL",
            FetchError::NameNotResolved => "net::ERR_NAME_NOT_RESOLVED",
            FetchError::ConnectionRefused => "net::ERR_CONNECTION_REFUSED",
            FetchError::TimedOut => "net::ERR_CONNECTION_TIMED_OUT",
            FetchError::Tls(_) => "net::ERR_CERT_AUTHORITY_INVALID",
            FetchError::EmptyResponse => "net::ERR_EMPTY_RESPONSE",
            FetchError::InvalidResponse => "net::ERR_INVALID_HTTP_RESPONSE",
            FetchError::TooManyRedirects => "net::ERR_TOO_MANY_REDIRECTS",
            FetchError::Io(_) => "net::ERR_CONNECTION_RESET",
            FetchError::UnknownScheme(_) => "net::ERR_UNKNOWN_URL_SCHEME",
            FetchError::Offline => "net::ERR_INTERNET_DISCONNECTED",
        }
    }
}

/// O pote de cookies que o cliente consulta a cada salto: o `Cookie:` do pedido e o `Set-Cookie` da resposta, de modo que
/// um login que redireciona leva o cookie que acabou de receber.
pub trait CookieHook {
    /// O valor do `Cookie:` para um pedido a `url`.
    fn header_for(&self, url: &Url) -> Option<String>;
    /// Guarda os `Set-Cookie` de uma resposta de `url`.
    fn store(&self, url: &Url, headers: &[(Vec<u8>, Vec<u8>)]);
}

/// O que o pedido leva além da linha de status.
pub struct RequestSpec<'a> {
    pub method: &'a str,
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
    pub max_redirects: u32,
    pub timeout: Option<Duration>,
    /// O pote de cookies; sem ele o pedido só leva os cabeçalhos de `headers`.
    pub cookies: Option<&'a dyn CookieHook>,
}

impl Default for RequestSpec<'_> {
    fn default() -> Self {
        RequestSpec { method: "GET", headers: &[], body: &[], max_redirects: 20, timeout: Some(Duration::from_secs(30)), cookies: None }
    }
}

/// Faz o pedido a `target` (URL absoluta) e segue os redirecionamentos.
pub fn request(target: &str, spec: &RequestSpec<'_>) -> Result<Response, FetchError> {
    let mut current = url::parse(target, false).map_err(FetchError::InvalidUrl)?;
    let mut method = spec.method.to_owned();
    let mut body = spec.body.to_vec();
    let mut hops = Vec::new();
    for redirects in 0..=spec.max_redirects {
        if current.scheme != "http" && current.scheme != "https" {
            return Err(FetchError::UnknownScheme(current.scheme));
        }
        // O `Cookie:` é do salto: o pote pode ter ganho um cookie na resposta anterior.
        let mut headers = spec.headers.to_vec();
        if let Some(cookie) = spec.cookies.and_then(|jar| jar.header_for(&current)).filter(|_| !headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("Cookie"))) {
            headers.push(("Cookie".to_owned(), cookie));
        }
        let hop_spec = RequestSpec { method: spec.method, headers: &headers, body: spec.body, max_redirects: spec.max_redirects, timeout: spec.timeout, cookies: spec.cookies };
        let (mut response, location) = exchange(&current, &method, &hop_spec, &body)?;
        if let Some(jar) = spec.cookies {
            jar.store(&current, &response.headers);
        }
        if let Some(location) = location {
            hops.push(Hop { url: current.clone(), status: response.status, reason: response.reason.clone(), headers: response.headers.clone(), head: response.head.clone(), sent: response.sent.clone(), version: response.transfer.version });
            current = url::resolve(&current, &location).map_err(FetchError::InvalidUrl)?;
            // 301, 302 e 303 viram GET sem corpo, como o navegador faz.
            if matches!(response.status, 301..=303) && method != "GET" && method != "HEAD" {
                method = "GET".to_owned();
                body.clear();
            }
            continue;
        }
        response.transfer.redirects = redirects;
        response.hops = hops;
        return Ok(response);
    }
    Err(FetchError::TooManyRedirects)
}

/// Uma troca. Devolve a `Location` quando a resposta é um redirecionamento a seguir.
fn exchange(u: &Url, method: &str, spec: &RequestSpec<'_>, body: &[u8]) -> Result<(Response, Option<String>), FetchError> {
    let port = u.port_or_default();
    let now = ul_common::time::monotonic_nanos;
    let connect_start = now();
    let tcp = io::connect(u.host.as_bytes(), port, spec.timeout).map_err(|errno| match errno {
        Errno::ECONNREFUSED => FetchError::ConnectionRefused,
        Errno::ETIMEDOUT => FetchError::TimedOut,
        _ => FetchError::NameNotResolved,
    })?;
    let mut secure_start = None;
    let stream = if u.scheme == "https" {
        secure_start = Some(now());
        let pem = sys::read_file(tls::DEFAULT_CA_BUNDLE.as_bytes()).unwrap_or_default();
        let config = tls::client_config(&tls::Verify::Roots(pem), &[b"http/1.1"]).map_err(FetchError::Tls)?;
        Stream::Tls(Box::new(tls::TlsStream::handshake(config, &u.host, tcp).map_err(|error| FetchError::Tls(format!("{error:?}")))?))
    } else {
        Stream::Plain(tcp)
    };
    let connect_end = now();
    let mut conn = Conn::new(stream);
    conn.stream.set_deadline(io::Deadline::after(spec.timeout));
    let host_header = match u.port {
        Some(p) if Some(p) != url::default_port(&u.scheme) => format!("{}:{p}", u.host_for_url()),
        _ => u.host_for_url(),
    };
    let mut lines: Vec<(String, String)> = vec![("Host".into(), host_header)];
    for (name, value) in spec.headers {
        lines.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
        lines.push((name.clone(), value.clone()));
    }
    if !body.is_empty() && !lines.iter().any(|(name, _)| name.eq_ignore_ascii_case("Content-Length")) {
        lines.push(("Content-Length".into(), body.len().to_string()));
    }
    if !lines.iter().any(|(name, _)| name.eq_ignore_ascii_case("Accept-Encoding")) {
        lines.push(("Accept-Encoding".into(), "gzip, deflate, br, zstd".into()));
    }
    // O pedido fecha a conexão depois da resposta; o `Connection` do chamador (o `keep-alive` do navegador) vale.
    if !lines.iter().any(|(name, _)| name.eq_ignore_ascii_case("Connection")) {
        lines.push(("Connection".into(), "close".into()));
    }
    let mut wire = format!("{method} {} HTTP/1.1\r\n", u.target()).into_bytes();
    for (name, value) in &lines {
        wire.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    wire.extend_from_slice(b"\r\n");
    wire.extend_from_slice(body);
    let request_start = now();
    conn.stream.write_all(&wire).map_err(|error| FetchError::Io(error.to_string()))?;
    let head = loop {
        let head = conn.read_head().map_err(|error| match error {
            HttpError::Empty => FetchError::EmptyResponse,
            HttpError::WeirdReply | HttpError::TooLarge => FetchError::InvalidResponse,
            HttpError::Io(error) => FetchError::Io(error.to_string()),
        })?;
        if (100..200).contains(&head.status) && head.status != 101 {
            continue;
        }
        break head;
    };
    let response_start = now();
    let location = matches!(head.status, 301 | 302 | 303 | 307 | 308).then(|| head.get_str("Location")).flatten();
    let mode = http::body_mode(&head, method == "HEAD");
    let mut decoder = match head.get_str("Content-Encoding") {
        Some(encoding) => Decoder::for_encoding(&encoding).map_err(|_| FetchError::InvalidResponse)?,
        None => Decoder::Identity,
    };
    let mut collected = Vec::new();
    let mut encoded_size = 0;
    if mode != BodyMode::None {
        let mut reader = conn.body_reader(mode);
        loop {
            match reader.next_piece() {
                Ok(Some(piece)) => {
                    encoded_size += piece.len();
                    collected.extend(decoder.feed(&piece).map_err(|_| FetchError::InvalidResponse)?);
                }
                Ok(None) => break,
                // Corpo cortado: o navegador entrega o que veio.
                Err(_) => break,
            }
        }
        collected.extend(decoder.finish().map_err(|_| FetchError::InvalidResponse)?);
    }
    let response = Response {
        status: head.status,
        reason: String::from_utf8_lossy(&head.reason).into_owned(),
        headers: head.headers.clone(),
        body: collected,
        url: u.clone(),
        transfer: Transfer {
            version: head.version,
            encoded_size,
            redirects: 0,
            timing: Some(Timing { connect_start, secure_start, connect_end, request_start, response_start, response_end: now() }),
        },
        head: head.raw.clone(),
        sent: lines,
        hops: Vec::new(),
    };
    Ok((response, location))
}
