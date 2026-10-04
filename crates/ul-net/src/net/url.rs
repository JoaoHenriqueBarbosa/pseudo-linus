//! URLs com as regras de aceitação do curl 8.14.1 (o que ele recusa, como normaliza e como monta o
//! alvo do pedido), escritas a partir do comportamento observado e da documentação da API de URL.

use std::net::{Ipv4Addr, Ipv6Addr};

/// Erros de URL do curl (`curl_url_strerror`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UrlError {
    MalformedInput,
    BadPortNumber,
    NoHost,
    BadHostname,
    BadIpv6,
    BadLogin,
    BadSlashes,
    BadScheme,
    BadFileUrl,
}

impl UrlError {
    pub fn message(self) -> &'static str {
        match self {
            UrlError::MalformedInput => "Malformed input to a URL function",
            UrlError::BadPortNumber => "Port number was not a decimal number between 0 and 65535",
            UrlError::NoHost => "No host part in the URL",
            UrlError::BadHostname => "Bad hostname",
            UrlError::BadIpv6 => "Bad IPv6 address",
            UrlError::BadLogin => "Bad login part",
            UrlError::BadSlashes => "Unsupported number of slashes following scheme",
            UrlError::BadScheme => "Bad scheme",
            UrlError::BadFileUrl => "Bad file:// URL",
        }
    }
}

/// URL decomposta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    /// Esquema em minúsculas.
    pub scheme: String,
    pub user: Option<String>,
    pub password: Option<String>,
    /// Host como escrito (IPv6 sem colchetes, IPv4 normalizado).
    pub host: String,
    pub ipv6: bool,
    pub zoneid: Option<String>,
    /// Porta escrita (normalizada, sem zeros à esquerda).
    pub port: Option<u16>,
    /// Caminho já normalizado e codificado (começa com `/` ou é vazio).
    pub path: String,
    pub query: Option<String>,
    pub fragment: Option<String>,
    /// O esquema veio do palpite (URL sem `scheme://`).
    pub guessed: bool,
}

/// Porta padrão de um esquema conhecido.
pub fn default_port(scheme: &str) -> Option<u16> {
    Some(match scheme {
        "http" | "ws" => 80,
        "https" | "wss" => 443,
        "ftp" => 21,
        "ftps" => 990,
        "dict" => 2628,
        "ldap" => 389,
        "ldaps" => 636,
        "imap" => 143,
        "imaps" => 993,
        "pop3" => 110,
        "pop3s" => 995,
        "smtp" => 25,
        "smtps" => 465,
        "gopher" => 70,
        "gophers" => 70,
        "telnet" => 23,
        "tftp" => 69,
        "scp" | "sftp" => 22,
        "rtsp" => 554,
        "mqtt" => 1883,
        "smb" | "smbs" => 445,
        _ => return None,
    })
}

/// Esquemas que o curl do Debian conhece (os outros dão "Protocol ... not supported").
pub const KNOWN_SCHEMES: &[&str] = &[
    "dict", "file", "ftp", "ftps", "gopher", "gophers", "http", "https", "imap", "imaps", "ipfs", "ipns", "ldap",
    "ldaps", "mqtt", "pop3", "pop3s", "rtmp", "rtsp", "scp", "sftp", "smb", "smbs", "smtp", "smtps", "telnet",
    "tftp", "ws", "wss",
];

impl Url {
    /// Porta efetiva.
    pub fn port_or_default(&self) -> u16 {
        self.port.or_else(|| default_port(&self.scheme)).unwrap_or(0)
    }

    /// Host como vai no `Host:` e na URL (`[::1]` pra IPv6).
    pub fn host_for_url(&self) -> String {
        if self.ipv6 { format!("[{}]", self.host) } else { self.host.clone() }
    }

    /// Alvo do pedido: caminho (ou `/`) e a consulta.
    pub fn target(&self) -> String {
        let mut t = if self.path.is_empty() { "/".to_string() } else { self.path.clone() };
        if let Some(q) = &self.query {
            t.push('?');
            t.push_str(q);
        }
        t
    }

    /// URL completa como o curl a mostra (`CURLINFO_EFFECTIVE_URL`): sem fragmento quando
    /// `with_fragment` é falso, com a porta só quando foi escrita.
    pub fn to_string_with(&self, with_fragment: bool) -> String {
        let mut s = format!("{}://", self.scheme);
        if self.scheme == "file" {
            s.push_str(&self.path);
            return s;
        }
        if let Some(u) = &self.user {
            s.push_str(u);
            if let Some(p) = &self.password {
                s.push(':');
                s.push_str(p);
            }
            s.push('@');
        }
        s.push_str(&self.host_for_url());
        if let Some(p) = self.port {
            s.push(':');
            s.push_str(&p.to_string());
        }
        s.push_str(if self.path.is_empty() { "/" } else { &self.path });
        if let Some(q) = &self.query {
            s.push('?');
            s.push_str(q);
        }
        if with_fragment && let Some(f) = &self.fragment {
            s.push('#');
            s.push_str(f);
        }
        s
    }
}

/// `Curl_junkscan`: controle e espaço são recusados.
fn junkscan(url: &str) -> Result<(), UrlError> {
    if url.bytes().any(|b| b < 0x20 || b == 0x7f || b == b' ') {
        return Err(UrlError::MalformedInput);
    }
    Ok(())
}

/// Esquema no começo (`scheme:` seguido de `/` quando há palpite), em minúsculas.
fn absolute_scheme(url: &str, guess: bool) -> Option<(String, usize)> {
    let b = url.as_bytes();
    if b.first().is_none_or(|c| !c.is_ascii_alphabetic()) {
        return None;
    }
    let mut i = 1;
    while i < b.len() && i < 40 && (b[i].is_ascii_alphanumeric() || b[i] == b'+' || b[i] == b'-' || b[i] == b'.') {
        i += 1;
    }
    if i < b.len() && b[i] == b':' && (!guess || b.get(i + 1) == Some(&b'/')) {
        return Some((url[..i].to_ascii_lowercase(), i));
    }
    None
}

/// Normaliza IPv4 nas formas que o curl aceita (inteiro único, octal, hexadecimal, 2 ou 3 partes).
fn ipv4_normalize(h: &str) -> Option<String> {
    let mut parts: Vec<u64> = Vec::new();
    for p in h.split('.') {
        let v = if let Some(x) = p.strip_prefix("0x") {
            if x.is_empty() {
                0
            } else {
                u64::from_str_radix(x, 16).ok()?
            }
        } else if p.len() > 1 && p.starts_with('0') {
            u64::from_str_radix(&p[1..], 8).ok()?
        } else {
            if p.is_empty() || !p.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            p.parse::<u64>().ok()?
        };
        if v > u64::from(u32::MAX) {
            return None;
        }
        parts.push(v);
    }
    let n: u32 = match parts.len() {
        1 => parts[0] as u32,
        2 if parts[0] <= 0xff && parts[1] <= 0xff_ffff => ((parts[0] as u32) << 24) | parts[1] as u32,
        3 if parts[0] <= 0xff && parts[1] <= 0xff && parts[2] <= 0xffff => {
            ((parts[0] as u32) << 24) | ((parts[1] as u32) << 16) | parts[2] as u32
        }
        4 if parts.iter().all(|&p| p <= 0xff) => {
            ((parts[0] as u32) << 24) | ((parts[1] as u32) << 16) | ((parts[2] as u32) << 8) | parts[3] as u32
        }
        _ => return None,
    };
    Some(Ipv4Addr::from(n).to_string())
}

fn hexval(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

/// Decodifica `%XX` (pro host).
pub fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let (Some(h), Some(l)) = (b.get(i + 1).and_then(|&c| hexval(c)), b.get(i + 2).and_then(|&c| hexval(c)))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// Codifica bytes não ASCII (e só eles) como `%xx` minúsculo, como o curl faz no caminho e na
/// consulta.
fn encode_high(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &c in s.as_bytes() {
        if c >= 0x80 {
            out.push_str(&format!("%{c:02x}"));
        } else {
            out.push(c as char);
        }
    }
    out
}

/// Remove os segmentos `.` e `..` (RFC 3986 5.2.4), como o curl sem `--path-as-is`.
pub fn remove_dot_segments(path: &str) -> String {
    let mut input = path.to_string();
    let mut out = String::new();
    while !input.is_empty() {
        if input.starts_with("../") {
            input.drain(..3);
        } else if input.starts_with("./") {
            input.drain(..2);
        } else if input.starts_with("/./") {
            input.replace_range(..3, "/");
        } else if input == "/." {
            input = "/".to_string();
        } else if input.starts_with("/../") {
            input.replace_range(..4, "/");
            if let Some(p) = out.rfind('/') {
                out.truncate(p);
            } else {
                out.clear();
            }
        } else if input == "/.." {
            input = "/".to_string();
            if let Some(p) = out.rfind('/') {
                out.truncate(p);
            } else {
                out.clear();
            }
        } else if input == "." || input == ".." {
            input.clear();
        } else {
            let start = usize::from(input.starts_with('/'));
            let end = input[start..].find('/').map(|p| p + start).unwrap_or(input.len());
            out.push_str(&input[..end]);
            input.drain(..end);
        }
    }
    out
}

/// Interpreta `url` como o curl (com palpite de esquema). `path_as_is` desliga a normalização.
pub fn parse(url: &str, path_as_is: bool) -> Result<Url, UrlError> {
    junkscan(url)?;
    let (scheme, rest, guessed) = match absolute_scheme(url, true) {
        Some((s, n)) => (s, &url[n + 1..], false),
        None => (String::new(), url, true),
    };
    if scheme == "file" {
        return parse_file(url, rest);
    }
    let mut hostp = rest;
    if !guessed {
        let slashes = hostp.bytes().take(4).take_while(|&c| c == b'/').count();
        if !(1..=3).contains(&slashes) {
            return Err(UrlError::BadSlashes);
        }
        hostp = &hostp[slashes..];
    }
    let hostlen = hostp.find(['/', '?', '#']).unwrap_or(hostp.len());
    let authority = &hostp[..hostlen];
    let mut tail = &hostp[hostlen..];
    if authority.is_empty() {
        return Err(UrlError::NoHost);
    }
    // Login: até o primeiro '@'.
    let (login, hostport) = match authority.find('@') {
        Some(p) => (Some(&authority[..p]), &authority[p + 1..]),
        None => (None, authority),
    };
    let (user, password) = match login {
        Some(l) => match l.find(':') {
            Some(p) => (Some(l[..p].to_string()), Some(l[p + 1..].to_string())),
            None => (Some(l.to_string()), None),
        },
        None => (None, None),
    };
    // Porta.
    let (host_raw, port) = if hostport.starts_with('[') {
        let close = hostport.find(']').ok_or(UrlError::BadIpv6)?;
        let after = &hostport[close + 1..];
        let port = if after.is_empty() {
            None
        } else {
            let p = after.strip_prefix(':').ok_or(UrlError::BadPortNumber)?;
            parse_port(p, !guessed)?
        };
        (&hostport[..=close], port)
    } else {
        match hostport.find(':') {
            Some(c) => (&hostport[..c], parse_port(&hostport[c + 1..], !guessed)?),
            None => (hostport, None),
        }
    };
    if host_raw.is_empty() {
        return Err(UrlError::NoHost);
    }
    let mut ipv6 = false;
    let mut zoneid = None;
    let host = if let Some(inner) = host_raw.strip_prefix('[') {
        let inner = inner.strip_suffix(']').ok_or(UrlError::BadIpv6)?;
        if inner.len() < 2 {
            return Err(UrlError::BadIpv6);
        }
        let (addr, zone) = match inner.find('%') {
            Some(p) => {
                let mut z = &inner[p + 1..];
                if z.starts_with("25") && z.len() > 2 {
                    z = &z[2..];
                }
                if z.is_empty() || z.len() > 15 {
                    return Err(UrlError::BadIpv6);
                }
                (&inner[..p], Some(z.to_string()))
            }
            None => (inner, None),
        };
        if !addr.bytes().all(|c| c.is_ascii_hexdigit() || c == b':' || c == b'.') {
            return Err(UrlError::BadIpv6);
        }
        let a: Ipv6Addr = addr.parse().map_err(|_| UrlError::BadIpv6)?;
        ipv6 = true;
        zoneid = zone;
        a.to_string()
    } else {
        let decoded = if host_raw.contains('%') {
            let d = percent_decode(host_raw);
            if d.iter().any(|&c| c < 0x20) {
                return Err(UrlError::BadHostname);
            }
            String::from_utf8(d).map_err(|_| UrlError::BadHostname)?
        } else {
            host_raw.to_string()
        };
        const BAD: &[u8] = b" \r\n\t/:#?!@{}[]\\$'\"^`*<>=;,+&()%";
        if decoded.bytes().any(|c| BAD.contains(&c)) {
            return Err(UrlError::BadHostname);
        }
        ipv4_normalize(&decoded).unwrap_or(decoded)
    };
    let scheme = if guessed {
        let h = host.to_ascii_lowercase();
        let s = if h.starts_with("ftp.") {
            "ftp"
        } else if h.starts_with("dict.") {
            "dict"
        } else if h.starts_with("ldap.") {
            "ldap"
        } else if h.starts_with("imap.") {
            "imap"
        } else if h.starts_with("smtp.") {
            "smtp"
        } else if h.starts_with("pop3.") {
            "pop3"
        } else {
            "http"
        };
        s.to_string()
    } else {
        scheme
    };
    // Fragmento, consulta e caminho.
    let mut fragment = None;
    if let Some(p) = tail.find('#') {
        let f = &tail[p + 1..];
        if !f.is_empty() {
            fragment = Some(f.to_string());
        }
        tail = &tail[..p];
    }
    let mut query = None;
    if let Some(p) = tail.find('?') {
        query = Some(encode_high(&tail[p + 1..]));
        tail = &tail[..p];
    }
    let mut path = encode_high(tail);
    if !path_as_is && !path.is_empty() {
        path = remove_dot_segments(&path);
        if path.is_empty() {
            path = "/".to_string();
        }
    }
    Ok(Url { scheme, user, password, host, ipv6, zoneid, port, path, query, fragment, guessed })
}

fn parse_port(p: &str, has_scheme: bool) -> Result<Option<u16>, UrlError> {
    if p.is_empty() {
        return if has_scheme { Ok(None) } else { Err(UrlError::BadPortNumber) };
    }
    if !p.bytes().all(|c| c.is_ascii_digit()) {
        return Err(UrlError::BadPortNumber);
    }
    let v: u64 = p.trim_start_matches('0').parse().unwrap_or(0);
    if p.trim_start_matches('0').len() > 5 || v > 0xffff {
        return Err(UrlError::BadPortNumber);
    }
    Ok(Some(v as u16))
}

fn parse_file(url: &str, rest: &str) -> Result<Url, UrlError> {
    if url.len() <= 6 {
        return Err(UrlError::BadFileUrl);
    }
    let mut path = rest;
    if let Some(p) = path.strip_prefix("//") {
        if !p.starts_with('/') {
            if let Some(x) = p.strip_prefix("localhost/").or_else(|| p.strip_prefix("127.0.0.1/")) {
                path = &p[p.len() - x.len() - 1..];
            } else {
                return Err(UrlError::BadFileUrl);
            }
        } else {
            path = p;
        }
    }
    let mut query = None;
    let mut fragment = None;
    let mut tail = path;
    if let Some(p) = tail.find('#') {
        fragment = Some(tail[p + 1..].to_string()).filter(|f| !f.is_empty());
        tail = &tail[..p];
    }
    if let Some(p) = tail.find('?') {
        query = Some(tail[p + 1..].to_string());
        tail = &tail[..p];
    }
    Ok(Url {
        scheme: "file".into(),
        user: None,
        password: None,
        host: String::new(),
        ipv6: false,
        zoneid: None,
        port: None,
        path: remove_dot_segments(tail),
        query,
        fragment,
        guessed: false,
    })
}

/// Resolve uma `Location` relativa contra a URL corrente (RFC 3986 seção 5).
pub fn resolve(base: &Url, location: &str) -> Result<Url, UrlError> {
    let loc = location.trim();
    if absolute_scheme(loc, false).is_some() {
        return parse(loc, false);
    }
    if let Some(rest) = loc.strip_prefix("//") {
        return parse(&format!("{}://{}", base.scheme, rest), false);
    }
    let mut out = base.clone();
    out.fragment = None;
    let (pathq, frag) = match loc.find('#') {
        Some(p) => (&loc[..p], Some(loc[p + 1..].to_string()).filter(|f| !f.is_empty())),
        None => (loc, None),
    };
    let (p, q) = match pathq.find('?') {
        Some(i) => (&pathq[..i], Some(encode_high(&pathq[i + 1..]))),
        None => (pathq, None),
    };
    if p.is_empty() {
        if q.is_some() {
            out.query = q;
        }
    } else if p.starts_with('/') {
        out.path = remove_dot_segments(&encode_high(p));
        out.query = q;
    } else {
        let dir = match base.path.rfind('/') {
            Some(i) => &base.path[..=i],
            None => "/",
        };
        out.path = remove_dot_segments(&format!("{dir}{}", encode_high(p)));
        out.query = q;
    }
    out.fragment = frag;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_like_curl() {
        let u = parse("http://127.0.0.1:18081/a/../b/./c?x=1#frag", false).unwrap();
        assert_eq!(u.target(), "/b/c?x=1");
        assert_eq!(u.fragment.as_deref(), Some("frag"));
        let u = parse("HTTP://Localhost:18081/X", false).unwrap();
        assert_eq!((u.scheme.as_str(), u.host.as_str(), u.port), ("http", "Localhost", Some(18081)));
        let u = parse("127.0.0.1:18081/noscheme", false).unwrap();
        assert!(u.guessed);
        assert_eq!(u.scheme, "http");
        assert_eq!(parse("http://a b/", false), Err(UrlError::MalformedInput));
        assert_eq!(parse("http://127.0.0.1:99999/", false), Err(UrlError::BadPortNumber));
        assert_eq!(parse("http://user@", false), Err(UrlError::NoHost));
        let u = parse("http://h/%7euser/ä", false).unwrap();
        assert_eq!(u.path, "/%7euser/%c3%a4");
        let u = parse("http://[::1]:8080/", false).unwrap();
        assert_eq!(u.host_for_url(), "[::1]");
        let u = parse("http://0x7f.1/", false).unwrap();
        assert_eq!(u.host, "127.0.0.1");
        let u = parse("ftp.example.com/x", false).unwrap();
        assert_eq!(u.scheme, "ftp");
    }

    #[test]
    fn resolves_locations() {
        let b = parse("http://h/a/b/c?q", false).unwrap();
        assert_eq!(resolve(&b, "d").unwrap().target(), "/a/b/d");
        assert_eq!(resolve(&b, "../x?y=1").unwrap().target(), "/a/x?y=1");
        assert_eq!(resolve(&b, "/root").unwrap().target(), "/root");
        assert_eq!(resolve(&b, "//other/p").unwrap().host, "other");
        assert_eq!(resolve(&b, "https://z/").unwrap().scheme, "https");
    }
}
