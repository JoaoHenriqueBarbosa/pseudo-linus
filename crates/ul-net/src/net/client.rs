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
}

/// A troca vista de fora do corpo: a versão do protocolo da resposta final, o corpo como veio no fio (antes do
/// `Content-Encoding`) e quantos redirecionamentos levaram até ela.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transfer {
    pub version: (u8, u8),
    pub encoded_size: usize,
    pub redirects: u32,
}

impl Transfer {
    /// Uma resposta HTTP/1.1 de corpo sem codificação, a primeira da cadeia.
    pub fn identity(body_len: usize) -> Transfer {
        Transfer { version: (1, 1), encoded_size: body_len, redirects: 0 }
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
        }
    }
}

/// O que o pedido leva além da linha de status.
pub struct RequestSpec<'a> {
    pub method: &'a str,
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
    pub max_redirects: u32,
    pub timeout: Option<Duration>,
}

impl Default for RequestSpec<'_> {
    fn default() -> Self {
        RequestSpec { method: "GET", headers: &[], body: &[], max_redirects: 20, timeout: Some(Duration::from_secs(30)) }
    }
}

/// Faz o pedido a `target` (URL absoluta) e segue os redirecionamentos.
pub fn request(target: &str, spec: &RequestSpec<'_>) -> Result<Response, FetchError> {
    let mut current = url::parse(target, false).map_err(FetchError::InvalidUrl)?;
    let mut method = spec.method.to_owned();
    let mut body = spec.body.to_vec();
    for redirects in 0..=spec.max_redirects {
        if current.scheme != "http" && current.scheme != "https" {
            return Err(FetchError::UnknownScheme(current.scheme));
        }
        let (mut response, location) = exchange(&current, &method, spec, &body)?;
        if let Some(location) = location {
            current = url::resolve(&current, &location).map_err(FetchError::InvalidUrl)?;
            // 301, 302 e 303 viram GET sem corpo, como o navegador faz.
            if matches!(response.status, 301..=303) && method != "GET" && method != "HEAD" {
                method = "GET".to_owned();
                body.clear();
            }
            continue;
        }
        response.transfer.redirects = redirects;
        return Ok(response);
    }
    Err(FetchError::TooManyRedirects)
}

/// Uma troca. Devolve a `Location` quando a resposta é um redirecionamento a seguir.
fn exchange(u: &Url, method: &str, spec: &RequestSpec<'_>, body: &[u8]) -> Result<(Response, Option<String>), FetchError> {
    let port = u.port_or_default();
    let tcp = io::connect(u.host.as_bytes(), port, spec.timeout).map_err(|errno| match errno {
        Errno::ECONNREFUSED => FetchError::ConnectionRefused,
        Errno::ETIMEDOUT => FetchError::TimedOut,
        _ => FetchError::NameNotResolved,
    })?;
    let stream = if u.scheme == "https" {
        let pem = sys::read_file(tls::DEFAULT_CA_BUNDLE.as_bytes()).unwrap_or_default();
        let config = tls::client_config(&tls::Verify::Roots(pem), &[b"http/1.1"]).map_err(FetchError::Tls)?;
        Stream::Tls(Box::new(tls::TlsStream::handshake(config, &u.host, tcp).map_err(|error| FetchError::Tls(format!("{error:?}")))?))
    } else {
        Stream::Plain(tcp)
    };
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
    lines.push(("Connection".into(), "close".into()));
    let mut wire = format!("{method} {} HTTP/1.1\r\n", u.target()).into_bytes();
    for (name, value) in &lines {
        wire.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    wire.extend_from_slice(b"\r\n");
    wire.extend_from_slice(body);
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
        transfer: Transfer { version: head.version, encoded_size, redirects: 0 },
    };
    Ok((response, location))
}
