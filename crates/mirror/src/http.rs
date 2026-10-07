//! HTTP/1.1 mínimo (GET e HEAD, keep-alive) e o roteamento da Simple API.

use crate::index::{Format, Index, normalize, percent_decode, percent_encode};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;

const MAX_HEAD: usize = 64 * 1024;
const MAX_BODY: usize = 1 << 20;

enum Body {
    Bytes(Vec<u8>),
    File { path: PathBuf, size: u64 },
}

pub(crate) struct Response {
    status: u16,
    content_type: &'static str,
    headers: Vec<(&'static str, String)>,
    body: Body,
}

struct Request {
    method: String,
    target: String,
    http10: bool,
    accept: Option<String>,
    connection: Vec<String>,
    content_length: usize,
    chunked: bool,
}

impl Request {
    fn keep_alive(&self) -> bool {
        let has = |t: &str| self.connection.iter().any(|c| c == t);
        if self.http10 { has("keep-alive") } else { !has("close") }
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        301 => "Moved Permanently",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Unknown",
    }
}

fn page(status: u16, title: &str) -> Response {
    Response {
        status,
        content_type: "text/html",
        headers: Vec::new(),
        body: Body::Bytes(
            format!(
                "<!DOCTYPE html>\n<html>\n  <head><title>{title}</title></head>\n  <body><h1>{title}</h1></body>\n</html>\n"
            )
            .into_bytes(),
        ),
    }
}

fn redirect(location: String) -> Response {
    let mut r = page(301, "301 Moved Permanently");
    r.headers.push(("Location", location));
    r
}

fn ok(content_type: &'static str, body: Vec<u8>) -> Response {
    Response {
        status: 200,
        content_type,
        headers: Vec::new(),
        body: Body::Bytes(body),
    }
}

/// Escolhe o formato pelo Accept: o maior q entre os tipos que o espelho produz; sem
/// correspondência exata (inclusive `*/*` ou ausência do cabeçalho) vale `text/html`.
fn negotiate(accept: Option<&str>) -> Format {
    let mut best: Option<(u32, u8, Format)> = None;
    for part in accept.unwrap_or_default().split(',') {
        let mut fields = part.split(';');
        let media = fields.next().unwrap_or_default().trim().to_ascii_lowercase();
        let (format, rank) = match media.as_str() {
            "application/vnd.pypi.simple.v1+json" | "application/vnd.pypi.simple.latest+json" => {
                (Format::Json, 2)
            }
            "application/vnd.pypi.simple.v1+html" | "application/vnd.pypi.simple.latest+html" => {
                (Format::HtmlV1, 1)
            }
            "text/html" => (Format::Html, 0),
            _ => continue,
        };
        let mut q = 1000u32;
        for p in fields {
            if let Some(v) = p.trim().strip_prefix("q=") {
                q = v.trim().parse::<f32>().map_or(0, |f| (f.clamp(0.0, 1.0) * 1000.0) as u32);
            }
        }
        if q > 0 && best.is_none_or(|(bq, br, _)| (q, rank) > (bq, br)) {
            best = Some((q, rank, format));
        }
    }
    best.map_or(Format::Html, |(_, _, f)| f)
}

fn route(index: &Index, req: &Request) -> Response {
    if req.method != "GET" && req.method != "HEAD" {
        let mut r = page(405, "405 Method Not Allowed");
        r.headers.push(("Allow", "GET, HEAD".to_string()));
        return r;
    }
    let path = req.target.split(['?', '#']).next().unwrap_or_default();
    let format = negotiate(req.accept.as_deref());
    if path == "/simple" {
        return redirect("/simple/".to_string());
    }
    if path == "/simple/" {
        return ok(format.content_type(), index.render_root(format).into_bytes());
    }
    if let Some(rest) = path.strip_prefix("/simple/") {
        let Some(rest) = percent_decode(rest) else {
            return page(404, "404 Not Found");
        };
        let (name, slash) = match rest.strip_suffix('/') {
            Some(n) => (n, true),
            None => (rest.as_str(), false),
        };
        if name.is_empty() || name.contains('/') {
            return page(404, "404 Not Found");
        }
        let normalized = normalize(name);
        if !slash || normalized != name {
            return redirect(format!("/simple/{}/", percent_encode(&normalized)));
        }
        return match index.render_project(&normalized, format) {
            Some(body) => ok(format.content_type(), body.into_bytes()),
            None => page(404, "404 Not Found"),
        };
    }
    if let Some(rest) = path.strip_prefix("/files/") {
        return file_response(index, rest);
    }
    page(404, "404 Not Found")
}

fn file_response(index: &Index, encoded: &str) -> Response {
    let Some(name) = percent_decode(encoded) else {
        return page(404, "404 Not Found");
    };
    if let Some(entry) = index.files.get(&name) {
        return Response {
            status: 200,
            content_type: "application/octet-stream",
            headers: Vec::new(),
            body: Body::File {
                path: entry.path.clone(),
                size: entry.size,
            },
        };
    }
    let metadata = name
        .strip_suffix(".metadata")
        .and_then(|wheel| index.files.get(wheel))
        .and_then(|e| e.metadata.as_ref());
    match metadata {
        Some(m) => ok("application/octet-stream", m.bytes.clone()),
        None => page(404, "404 Not Found"),
    }
}

/// Lê o cabeçalho de `buf`: `Ok(None)` se incompleto, `Err` se malformado.
fn parse_head(buf: &[u8]) -> Result<Option<(Request, usize)>, ()> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut parsed = httparse::Request::new(&mut headers);
    let used = match parsed.parse(buf) {
        Ok(httparse::Status::Complete(n)) => n,
        Ok(httparse::Status::Partial) => return Ok(None),
        Err(_) => return Err(()),
    };
    let mut req = Request {
        method: parsed.method.unwrap_or_default().to_string(),
        target: parsed.path.unwrap_or_default().to_string(),
        http10: parsed.version == Some(0),
        accept: None,
        connection: Vec::new(),
        content_length: 0,
        chunked: false,
    };
    for h in parsed.headers.iter() {
        let value = String::from_utf8_lossy(h.value).into_owned();
        if h.name.eq_ignore_ascii_case("accept") {
            req.accept = Some(match req.accept.take() {
                Some(prev) => format!("{prev},{value}"),
                None => value,
            });
        } else if h.name.eq_ignore_ascii_case("connection") {
            req.connection
                .extend(value.split(',').map(|t| t.trim().to_ascii_lowercase()));
        } else if h.name.eq_ignore_ascii_case("content-length") {
            req.content_length = value.trim().parse().map_err(|_| ())?;
        } else if h.name.eq_ignore_ascii_case("transfer-encoding") {
            req.chunked = true;
        }
    }
    Ok(Some((req, used)))
}

fn write_response<S: Write>(
    io: &mut S,
    resp: &Response,
    head_only: bool,
    keep_alive: bool,
) -> io::Result<()> {
    let length = match &resp.body {
        Body::Bytes(b) => b.len() as u64,
        Body::File { size, .. } => *size,
    };
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n",
        resp.status,
        reason(resp.status),
        resp.content_type,
        length
    );
    for (k, v) in &resp.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(if keep_alive {
        "Connection: keep-alive\r\n\r\n"
    } else {
        "Connection: close\r\n\r\n"
    });
    io.write_all(head.as_bytes())?;
    if !head_only {
        match &resp.body {
            Body::Bytes(b) => io.write_all(b)?,
            Body::File { path, size } => {
                let copied = io::copy(&mut File::open(path)?.take(*size), io)?;
                if copied != *size {
                    return Err(io::Error::other("arquivo alterado durante a leitura"));
                }
            }
        }
    }
    io.flush()
}

fn closed(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::BrokenPipe
    )
}

/// Atende requisições no stream até EOF ou `Connection: close`.
pub(crate) fn serve<S: Read + Write>(index: &Index, io: &mut S) -> io::Result<()> {
    match serve_loop(index, io) {
        Err(e) if closed(&e) => Ok(()),
        r => r,
    }
}

fn serve_loop<S: Read + Write>(index: &Index, io: &mut S) -> io::Result<()> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let (req, used) = loop {
            match parse_head(&buf) {
                Ok(Some(found)) => break found,
                Ok(None) if buf.len() <= MAX_HEAD => {}
                _ => return write_response(io, &page(400, "400 Bad Request"), false, false),
            }
            let n = io.read(&mut chunk)?;
            if n == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&chunk[..n]);
        };
        buf.drain(..used);
        if req.chunked || req.content_length > MAX_BODY {
            return write_response(io, &page(400, "400 Bad Request"), false, false);
        }
        let mut pending = req.content_length;
        let have = pending.min(buf.len());
        buf.drain(..have);
        pending -= have;
        while pending > 0 {
            let n = io.read(&mut chunk)?;
            if n == 0 {
                return Ok(());
            }
            let take = n.min(pending);
            pending -= take;
            buf.extend_from_slice(&chunk[take..n]);
        }
        let keep_alive = req.keep_alive();
        let resp = route(index, &req);
        write_response(io, &resp, req.method == "HEAD", keep_alive)?;
        if !keep_alive {
            return Ok(());
        }
    }
}
