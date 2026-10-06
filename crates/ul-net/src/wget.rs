//! `wget` 1.25.0 (o do Debian 13) sobre a pilha de [`crate::net`].
//!
//! Cobre o uso de um agente: baixar para arquivo (`-O`, `-P`, nome pela URL com `.1`, `.2`...), para a
//! saída (`-O -`), `-q`/`-nv`/`-S`, `--spider`, `-c`, `-nc`, `--post-data` e variantes, `--method`,
//! `--header`, `-U`, `--max-redirect`, `--user`/`--password` (Basic depois do desafio 401) e o registro no
//! stderr com a data local e a barra de pontos do `progress.c`. Os códigos de saída são os do wget:
//! 1 genérico, 2 opção inválida, 3 erro de arquivo, 4 rede, 5 TLS, 8 erro do servidor.

use std::ffi::OsString;
use std::io::Write as _;
use std::os::unix::ffi::OsStrExt;
use std::time::Duration;

use sysabi::{Ctx, Errno, Fd, Mode, OFlags, sys};

use crate::net::http::{self, BodyError, BodyMode, Conn, Head, HttpError, Stream};
use crate::net::io::{self, Deadline};
use crate::net::url::{self, Url};
use crate::net::{tls, tz};

const VERSION: &str = "1.25.0";
const USAGE_TAIL: &str = "Usage: wget [OPTION]... [URL]...\n\nTry `wget --help' for more options.\n";

#[derive(PartialEq, Eq, Clone, Copy)]
enum Verbosity {
    Quiet,
    NoVerbose,
    Verbose,
}

struct Opts {
    urls: Vec<String>,
    verbosity: Verbosity,
    output: Option<String>,
    prefix: Option<String>,
    server_response: bool,
    spider: bool,
    body: Option<Vec<u8>>,
    method: Option<String>,
    headers: Vec<String>,
    user_agent: Option<String>,
    max_redirect: u32,
    timeout: Option<Duration>,
    cont: bool,
    no_clobber: bool,
    insecure: bool,
    user: Option<String>,
    password: Option<String>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            urls: Vec::new(),
            verbosity: Verbosity::Verbose,
            output: None,
            prefix: None,
            server_response: false,
            spider: false,
            body: None,
            method: None,
            headers: Vec::new(),
            user_agent: None,
            max_redirect: 20,
            timeout: None,
            cont: false,
            no_clobber: false,
            insecure: false,
            user: None,
            password: None,
        }
    }
}

/// Fim do processamento das opções: código de saída e o que já foi escrito.
struct Exit(i32);

fn usage_error(ctx: &mut Ctx, msg: &str) -> Exit {
    let _ = ctx.stderr().write_all(format!("wget: {msg}\n{USAGE_TAIL}").as_bytes());
    Exit(2)
}

/// Opções longas que levam argumento.
const LONG_WITH_ARG: &[&str] = &[
    "output-document",
    "directory-prefix",
    "post-data",
    "post-file",
    "body-data",
    "body-file",
    "method",
    "header",
    "user-agent",
    "max-redirect",
    "tries",
    "timeout",
    "read-timeout",
    "connect-timeout",
    "dns-timeout",
    "user",
    "password",
    "http-user",
    "http-password",
    "input-file",
    "output-file",
    "append-output",
    "wait",
    "progress",
    "execute",
    "ca-certificate",
    "load-cookies",
    "save-cookies",
    "referer",
    "limit-rate",
];

fn parse(ctx: &mut Ctx, argv: &[OsString]) -> Result<Opts, Exit> {
    let mut o = Opts::default();
    let args: Vec<String> = argv.iter().skip(1).map(|a| String::from_utf8_lossy(a.as_bytes()).into_owned()).collect();
    let mut i = 0;
    let mut input_files = Vec::new();
    while i < args.len() {
        let a = args[i].clone();
        i += 1;
        if a == "--" {
            o.urls.extend(args[i..].iter().cloned());
            break;
        }
        if let Some(long) = a.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (long.to_string(), None),
            };
            // Prefixo único também vale, como no getopt_long.
            let known = |n: &str| LONG_WITH_ARG.contains(&n) || LONG_FLAGS.contains(&n);
            let name = if known(&name) {
                name
            } else {
                let cands: Vec<&str> = LONG_WITH_ARG.iter().chain(LONG_FLAGS.iter()).copied().filter(|c| c.starts_with(name.as_str())).collect();
                match cands.as_slice() {
                    [one] => (*one).to_string(),
                    [] => return Err(usage_error(ctx, &format!("unrecognized option '--{name}'"))),
                    _ => return Err(usage_error(ctx, &format!("option '--{name}' is ambiguous"))),
                }
            };
            let value = if LONG_WITH_ARG.contains(&name.as_str()) {
                match inline {
                    Some(v) => Some(v),
                    None if i < args.len() => {
                        i += 1;
                        Some(args[i - 1].clone())
                    }
                    None => return Err(usage_error(ctx, &format!("option '--{name}' requires an argument"))),
                }
            } else {
                None
            };
            apply_long(ctx, &mut o, &name, value, &mut input_files)?;
            continue;
        }
        if a.len() > 1 && a.starts_with('-') {
            let chars: Vec<char> = a[1..].chars().collect();
            let mut k = 0;
            while k < chars.len() {
                let c = chars[k];
                k += 1;
                // `-nv`, `-nc`, `-nd`, `-nH`: o `n` junta com a letra seguinte.
                if c == 'n' {
                    let Some(&d) = chars.get(k) else {
                        return Err(usage_error(ctx, "invalid option -- 'n'"));
                    };
                    k += 1;
                    match d {
                        'v' => o.verbosity = Verbosity::NoVerbose,
                        'c' => o.no_clobber = true,
                        'd' | 'H' | 'p' => {}
                        _ => return Err(usage_error(ctx, &format!("invalid option -- 'n{d}'"))),
                    }
                    continue;
                }
                let takes = matches!(c, 'O' | 'P' | 'U' | 't' | 'T' | 'o' | 'a' | 'i' | 'e' | 'w');
                let value = if takes {
                    if k < chars.len() {
                        let v: String = chars[k..].iter().collect();
                        k = chars.len();
                        Some(v)
                    } else if i < args.len() {
                        i += 1;
                        Some(args[i - 1].clone())
                    } else {
                        return Err(usage_error(ctx, &format!("option requires an argument -- '{c}'")));
                    }
                } else {
                    None
                };
                let name = match c {
                    'q' => "quiet",
                    'v' => "verbose",
                    'O' => "output-document",
                    'P' => "directory-prefix",
                    'S' => "server-response",
                    'U' => "user-agent",
                    't' => "tries",
                    'T' => "timeout",
                    'c' => "continue",
                    'i' => "input-file",
                    'o' | 'a' | 'e' | 'w' => "ignored",
                    'N' | 'r' | 'p' | 'k' | 'x' | 'b' | 'd' | 'E' | 'K' | 'm' => "ignored-flag",
                    'V' => "version",
                    'h' => "help",
                    _ => return Err(usage_error(ctx, &format!("invalid option -- '{c}'"))),
                };
                apply_long(ctx, &mut o, name, value, &mut input_files)?;
            }
            continue;
        }
        o.urls.push(a);
    }
    for f in input_files {
        let data = if f == "-" {
            let mut v = Vec::new();
            let _ = std::io::Read::read_to_end(&mut ctx.stdin(), &mut v);
            v
        } else {
            match sys::read_file(f.as_bytes()) {
                Ok(d) => d,
                Err(e) => {
                    let _ = ctx.stderr().write_all(format!("{f}: {}\nNo URLs found in {f}.\n", e.message()).as_bytes());
                    return Err(Exit(3));
                }
            }
        };
        for line in String::from_utf8_lossy(&data).lines() {
            let line = line.trim();
            if !line.is_empty() {
                o.urls.push(line.to_string());
            }
        }
    }
    Ok(o)
}

/// Opções longas sem argumento.
const LONG_FLAGS: &[&str] = &[
    "quiet",
    "verbose",
    "no-verbose",
    "server-response",
    "spider",
    "continue",
    "no-clobber",
    "no-check-certificate",
    "version",
    "help",
    "show-progress",
    "no-cache",
    "content-disposition",
    "timestamping",
    "no-directories",
    "no-host-directories",
    "inet4-only",
    "inet6-only",
    "no-proxy",
    "ignored",
    "ignored-flag",
];

fn apply_long(ctx: &mut Ctx, o: &mut Opts, name: &str, value: Option<String>, inputs: &mut Vec<String>) -> Result<(), Exit> {
    let v = value.unwrap_or_default();
    let secs = |v: &str| v.parse::<f64>().ok().filter(|s| s.is_finite() && *s > 0.0).map(Duration::from_secs_f64);
    match name {
        "quiet" => o.verbosity = Verbosity::Quiet,
        "verbose" => o.verbosity = Verbosity::Verbose,
        "no-verbose" => o.verbosity = Verbosity::NoVerbose,
        "output-document" => o.output = Some(v),
        "directory-prefix" => o.prefix = Some(v),
        "server-response" => o.server_response = true,
        "spider" => o.spider = true,
        "post-data" | "body-data" => {
            o.body = Some(v.into_bytes());
            if name == "post-data" {
                o.method = Some("POST".into());
            }
        }
        "post-file" | "body-file" => {
            let data = sys::read_file(v.as_bytes()).map_err(|e| {
                let _ = ctx.stderr().write_all(format!("{v}: {}\n", e.message()).as_bytes());
                Exit(3)
            })?;
            o.body = Some(data);
            if name == "post-file" {
                o.method = Some("POST".into());
            }
        }
        "method" => o.method = Some(v.to_ascii_uppercase()),
        "header" => o.headers.push(v),
        "user-agent" => o.user_agent = Some(v),
        "max-redirect" => o.max_redirect = v.parse().unwrap_or(20),
        "timeout" | "read-timeout" | "connect-timeout" => o.timeout = secs(&v),
        "continue" => o.cont = true,
        "no-clobber" => o.no_clobber = true,
        "no-check-certificate" => o.insecure = true,
        "user" | "http-user" => o.user = Some(v),
        "password" | "http-password" => o.password = Some(v),
        "input-file" => inputs.push(v),
        "referer" => o.headers.push(format!("Referer: {v}")),
        "version" => {
            let _ = ctx.stdout().write_all(VERSION_TEXT.as_bytes());
            return Err(Exit(0));
        }
        "help" => {
            let _ = ctx.stdout().write_all(HELP.as_bytes());
            return Err(Exit(0));
        }
        _ => {}
    }
    Ok(())
}

const VERSION_TEXT: &str = "GNU Wget 1.25.0 built on linux-gnu.

-cares +digest -gpgme +https +ipv6 +iri +large-file -metalink +nls
+ntlm +opie +psl +ssl/gnutls

Copyright (C) 2024 Free Software Foundation, Inc.
License GPLv3+: GNU GPL version 3 or later
<https://www.gnu.org/licenses/gpl.html>.
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.

Originally written by Hrvoje Niksic <hniksic@xemacs.org>.
Please send bug reports and questions to <bug-wget@gnu.org>.
";

const HELP: &str = "GNU Wget 1.25.0, a non-interactive network retriever.
Usage: wget [OPTION]... [URL]...

Mandatory arguments to long options are mandatory for short options too.

Startup:
  -V,  --version                   display the version of Wget and exit
  -h,  --help                      print this help

Logging and input file:
  -o,  --output-file=FILE          log messages to FILE
  -q,  --quiet                     quiet (no output)
  -v,  --verbose                   be verbose (this is the default)
  -nv, --no-verbose                turn off verboseness, without being quiet
  -i,  --input-file=FILE           download URLs found in local or external FILE

Download:
  -t,  --tries=NUMBER              set number of retries to NUMBER (0 unlimits)
  -O,  --output-document=FILE      write documents to FILE
  -nc, --no-clobber                skip downloads that would download to
                                     existing files (overwriting them)
  -c,  --continue                  resume getting a partially-downloaded file
  -S,  --server-response           print server response
       --spider                    don't download anything
  -T,  --timeout=SECONDS           set all timeout values to SECONDS
       --user=USER                 set both ftp and http user to USER
       --password=PASS             set both ftp and http password to PASS

Directories:
  -P,  --directory-prefix=PREFIX   save files to PREFIX/..

HTTP options:
       --header=STRING             insert STRING among the headers
       --max-redirect              maximum redirections allowed per page
  -U,  --user-agent=AGENT          identify as AGENT instead of Wget/VERSION
       --post-data=STRING          use the POST method; send STRING as the data
       --post-file=FILE            use the POST method; send contents of FILE
       --method=HTTPMethod         use method \"HTTPMethod\" in the request
       --body-data=STRING          send STRING as data. --method MUST be set
       --body-file=FILE            send contents of FILE. --method MUST be set

HTTPS (SSL/TLS) options:
       --no-check-certificate      don't validate the server's certificate

Email bug reports, questions, discussions to <bug-wget@gnu.org>
and/or open issues at https://savannah.gnu.org/bugs/?func=additem&group=wget.
";

/// Registro no stderr, filtrado pela verbosidade.
struct Log {
    level: Verbosity,
}

impl Log {
    fn verbose(&self, ctx: &mut Ctx, s: &str) {
        if self.level == Verbosity::Verbose {
            let _ = ctx.stderr().write_all(s.as_bytes());
        }
    }

    /// Nível normal: aparece com `-nv` também.
    fn notquiet(&self, ctx: &mut Ctx, s: &str) {
        if self.level != Verbosity::Quiet {
            let _ = ctx.stderr().write_all(s.as_bytes());
        }
    }
}

fn stamp() -> String {
    tz::now_local("%Y-%m-%d %H:%M:%S")
}

/// `human_readable(n, 10, 1)` do wget: `(150K)`, `(1.5M)`.
fn human(n: u64) -> String {
    const P: [char; 6] = ['K', 'M', 'G', 'T', 'P', 'E'];
    let mut val = n as f64;
    for p in P {
        val /= 1024.0;
        if val < 1024.0 || p == 'E' {
            let dec = if val < 10.0 { 1 } else { 0 };
            return format!("{val:.dec$}{p}");
        }
    }
    n.to_string()
}

/// `calc_rate`: taxa e índice da unidade (B, K, M, G).
fn rate(bytes: u64, secs: f64) -> (f64, usize) {
    let secs = if secs <= 0.0 { 1e-6 } else { secs };
    let mut r = bytes as f64 / secs;
    let mut u = 0;
    while r >= 1024.0 && u < 3 {
        r /= 1024.0;
        u += 1;
    }
    (r, u)
}

fn rate_prec(r: f64) -> usize {
    if r >= 99.95 {
        0
    } else if r >= 9.995 {
        1
    } else {
        2
    }
}

/// `retr_rate`: "1.96 MB/s".
fn rate_text(bytes: u64, secs: f64) -> String {
    const N: [&str; 4] = ["B/s", "KB/s", "MB/s", "GB/s"];
    let (r, u) = rate(bytes, secs);
    format!("{r:.p$} {}", N[u], p = rate_prec(r))
}

/// `print_decimal` do progress.c.
fn decimal(secs: f64) -> String {
    if secs < 0.0005 {
        "0".into()
    } else if secs < 0.01 {
        format!("{secs:.3}")
    } else if secs < 0.1 {
        format!("{secs:.2}")
    } else {
        format!("{secs:.1}")
    }
}

/// `eta_to_human_short`.
fn eta_short(secs: u64) -> String {
    if secs < 100 {
        format!("{secs}s")
    } else if secs < 100 * 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs < 48 * 3600 {
        format!("{}h {}m", secs / 3600, secs / 60 % 60)
    } else {
        format!("{}d {}h", secs / 86400, secs / 3600 % 24)
    }
}

/// A barra de pontos (`--progress=dot`, o padrão quando o stderr não é terminal): 1K por ponto, 10
/// pontos por grupo, 50 por linha.
struct Dots {
    on: bool,
    total: Option<u64>,
    /// Bytes já mostrados antes desta transferência (o `-c`).
    initial: u64,
    accumulated: u64,
    dots: u32,
    rows: u64,
    row_start: f64,
    started: Duration,
}

const ROW: u64 = 50 * 1024;

impl Dots {
    fn new(on: bool, total: Option<u64>, initial: u64) -> Dots {
        Dots { on, total, initial, accumulated: 0, dots: 0, rows: initial / ROW, row_start: 0.0, started: io::now() }
    }

    fn elapsed(&self) -> f64 {
        io::now().saturating_sub(self.started).as_secs_f64()
    }

    fn begin(&self, ctx: &mut Ctx) {
        if self.on {
            let _ = ctx.stderr().write_all(format!("\n{:>6}K", self.rows * ROW / 1024).as_bytes());
        }
    }

    fn row_stats(&mut self, ctx: &mut Ctx, last: bool) {
        const U: [char; 4] = [' ', 'K', 'M', 'G'];
        let now = self.elapsed();
        let shown = self.rows * ROW + u64::from(self.dots) * 1024 + if last { self.accumulated } else { 0 };
        let mut s = String::new();
        if let Some(t) = self.total.filter(|t| *t > 0) {
            let shown = if last { self.initial_total_done() } else { shown };
            s.push_str(&format!("{:3}%", (shown as f64 * 100.0 / t as f64) as u64));
        }
        let row_bytes = if last { u64::from(self.dots) * 1024 + self.accumulated } else { ROW };
        let (r, u) = rate(row_bytes, now - self.row_start);
        s.push_str(&format!(" {r:4.p$}{}", U[u], p = rate_prec(r)));
        if last {
            s.push_str(&format!("={}s", decimal(now)));
        } else if let Some(t) = self.total {
            let done = shown.saturating_sub(self.initial).max(1);
            let left = t.saturating_sub(shown);
            let eta = (now * left as f64 / done as f64 + 0.5) as u64;
            s.push(' ');
            s.push_str(&eta_short(eta));
        }
        self.row_start = now;
        let _ = ctx.stderr().write_all(s.as_bytes());
    }

    fn initial_total_done(&self) -> u64 {
        self.rows * ROW + u64::from(self.dots) * 1024 + self.accumulated
    }

    fn update(&mut self, ctx: &mut Ctx, n: u64) {
        if !self.on {
            return;
        }
        self.accumulated += n;
        while self.accumulated >= 1024 {
            self.accumulated -= 1024;
            let mut out = String::new();
            if self.dots % 10 == 0 {
                out.push(' ');
            }
            out.push('.');
            let _ = ctx.stderr().write_all(out.as_bytes());
            self.dots += 1;
            if self.dots == 50 {
                self.row_stats(ctx, false);
                self.rows += 1;
                self.dots = 0;
                let _ = ctx.stderr().write_all(format!("\n{:>6}K", self.rows * ROW / 1024).as_bytes());
            }
        }
    }

    fn finish(&mut self, ctx: &mut Ctx) {
        if !self.on {
            return;
        }
        let mut pad = String::new();
        for i in self.dots..50 {
            if i % 10 == 0 {
                pad.push(' ');
            }
            pad.push(' ');
        }
        let _ = ctx.stderr().write_all(pad.as_bytes());
        self.row_stats(ctx, true);
        let _ = ctx.stderr().write_all(b"\n\n");
    }
}

/// Conexão mantida viva entre pedidos ao mesmo servidor.
struct Kept {
    key: (String, String, u16),
    conn: Conn,
}

/// Resultado de uma URL: o código do wget (0 ok).
type Code = i32;

struct Wget<'a> {
    ctx: &'a mut Ctx,
    o: &'a Opts,
    log: Log,
    kept: Option<Kept>,
    /// Arquivo do `-O` (aberto uma vez, todas as URLs escrevem nele).
    doc: Option<Fd>,
}

fn exists(path: &str) -> Option<u64> {
    sys::stat(path.as_bytes()).ok().map(|s| s.size)
}

/// Nome local pela URL: o último segmento (com a query), `index.html` quando vazio.
fn local_name(u: &Url) -> String {
    let seg = u.path.rsplit('/').next().unwrap_or("");
    let mut name = String::from_utf8_lossy(&url::percent_decode(seg)).into_owned();
    if name.is_empty() {
        name = "index.html".into();
    }
    if let Some(q) = &u.query {
        name.push('?');
        name.push_str(q);
    }
    name
}

/// Pedido a mandar.
struct Req {
    method: String,
    body: Option<Vec<u8>>,
    auth: Option<String>,
    range: Option<u64>,
}

fn base64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for ch in data.chunks(3) {
        let n = (u32::from(ch[0]) << 16) | (u32::from(*ch.get(1).unwrap_or(&0)) << 8) | u32::from(*ch.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= ch.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

impl Wget<'_> {
    fn request_text(&self, u: &Url, req: &Req) -> Vec<u8> {
        let host = match u.port {
            Some(p) if Some(p) != url::default_port(&u.scheme) => format!("{}:{p}", u.host_for_url()),
            _ => u.host_for_url(),
        };
        let mut lines: Vec<(String, String)> = vec![
            ("User-Agent".into(), self.o.user_agent.clone().unwrap_or_else(|| format!("Wget/{VERSION}"))),
            ("Accept".into(), "*/*".into()),
            ("Accept-Encoding".into(), "identity".into()),
        ];
        if let Some(from) = req.range {
            lines.push(("Range".into(), format!("bytes={from}-")));
        }
        if let Some(a) = &req.auth {
            lines.push(("Authorization".into(), a.clone()));
        }
        lines.push(("Host".into(), host));
        lines.push(("Connection".into(), "Keep-Alive".into()));
        for h in &self.o.headers {
            let Some((name, value)) = h.split_once(':') else { continue };
            let value = value.trim().to_string();
            if let Some(slot) = lines.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                slot.1 = value;
            } else {
                lines.push((name.to_string(), value));
            }
        }
        if let Some(body) = &req.body {
            if !lines.iter().any(|(n, _)| n.eq_ignore_ascii_case("Content-Type")) {
                lines.push(("Content-Type".into(), "application/x-www-form-urlencoded".into()));
            }
            lines.push(("Content-Length".into(), body.len().to_string()));
        }
        let mut out = format!("{} {} HTTP/1.1\r\n", req.method, u.target()).into_bytes();
        for (n, v) in lines {
            out.extend_from_slice(format!("{n}: {v}\r\n").as_bytes());
        }
        out.extend_from_slice(b"\r\n");
        if let Some(body) = &req.body {
            out.extend_from_slice(body);
        }
        out
    }

    /// Conecta (ou reaproveita) e manda o pedido; devolve a cabeça da resposta.
    fn send(&mut self, u: &Url, req: &Req) -> Result<Head, Code> {
        let key = (u.scheme.clone(), u.host.clone(), u.port_or_default());
        let port = key.2;
        let hostport = format!("{}:{port}", u.host_for_url());
        let wire = self.request_text(u, req);
        if let Some(k) = self.kept.take()
            && k.key == key
        {
            let mut conn = k.conn;
            self.log.verbose(self.ctx, &format!("Reusing existing connection to {hostport}.\n"));
            if conn.stream.write_all(&wire).is_ok() {
                self.log.verbose(self.ctx, "HTTP request sent, awaiting response... ");
                match conn.read_head() {
                    Ok(h) => {
                        self.kept = Some(Kept { key, conn });
                        return Ok(h);
                    }
                    Err(HttpError::Empty) => {}
                    Err(_) => return Err(self.read_error()),
                }
            }
            // O servidor fechou a conexão mantida: abre outra.
        }
        let is_ip = u.ipv6 || u.host.parse::<std::net::Ipv4Addr>().is_ok();
        if !is_ip {
            let loopback = matches!(u.host.as_str(), "localhost" | "localhost.localdomain");
            if loopback {
                self.log.verbose(self.ctx, &format!("Resolving {h} ({h})... 127.0.0.1\n", h = u.host));
            } else {
                // Sem DNS no sandbox: a falha é a de um resolvedor inalcançável (EAI_AGAIN).
                self.log.verbose(self.ctx, &format!("Resolving {h} ({h})... failed: Temporary failure in name resolution.\n", h = u.host));
                self.log.notquiet(self.ctx, &format!("wget: unable to resolve host address ‘{}’\n", u.host));
                return Err(4);
            }
        }
        let shown = if is_ip { hostport.clone() } else { format!("{h} ({h})|127.0.0.1|:{port}", h = u.host) };
        self.log.verbose(self.ctx, &format!("Connecting to {shown}... "));
        let tcp = match io::connect(u.host.as_bytes(), port, self.o.timeout) {
            Ok(t) => t,
            Err(e) => {
                let why = match e {
                    Errno::ECONNREFUSED => "Connection refused",
                    Errno::ETIMEDOUT => "Connection timed out",
                    _ => "Network is unreachable",
                };
                self.log.verbose(self.ctx, &format!("failed: {why}.\n"));
                return Err(4);
            }
        };
        self.log.verbose(self.ctx, "connected.\n");
        let stream = if u.scheme == "https" {
            let verify = if self.o.insecure {
                tls::Verify::Insecure
            } else {
                tls::Verify::Roots(sys::read_file(tls::DEFAULT_CA_BUNDLE.as_bytes()).unwrap_or_default())
            };
            let cfg = tls::client_config(&verify, &[b"http/1.1"]).map_err(|_| 5)?;
            match tls::TlsStream::handshake(cfg, &u.host, tcp) {
                Ok(t) => Stream::Tls(Box::new(t)),
                Err(e) => {
                    let msg = match e {
                        tls::TlsError::Verify(m) => format!("ERROR: cannot verify {}'s certificate: {m}\nTo connect to {} insecurely, use `--no-check-certificate'.\n", u.host, u.host),
                        tls::TlsError::NameMismatch => format!("ERROR: no certificate subject alternative name matches\n\trequested host name ‘{}’.\nTo connect to {} insecurely, use `--no-check-certificate'.\n", u.host, u.host),
                        _ => "Unable to establish SSL connection.\n".into(),
                    };
                    self.log.notquiet(self.ctx, &msg);
                    return Err(5);
                }
            }
        } else {
            Stream::Plain(tcp)
        };
        let mut conn = Conn::new(stream);
        conn.stream.set_deadline(Deadline::after(self.o.timeout));
        if conn.stream.write_all(&wire).is_err() {
            return Err(self.read_error());
        }
        self.log.verbose(self.ctx, "HTTP request sent, awaiting response... ");
        match conn.read_head() {
            Ok(h) => {
                self.kept = Some(Kept { key, conn });
                Ok(h)
            }
            Err(HttpError::Empty) => {
                self.log.verbose(self.ctx, "No data received.\n");
                Err(4)
            }
            Err(_) => Err(self.read_error()),
        }
    }

    fn read_error(&mut self) -> Code {
        self.log.verbose(self.ctx, "Read error (Connection reset by peer) in headers.\n");
        4
    }

    fn show_head(&mut self, head: &Head) {
        if self.o.server_response {
            let mut s = String::new();
            if self.log.level == Verbosity::Verbose {
                s.push('\n');
            }
            for l in &head.lines {
                s.push_str("  ");
                s.push_str(&String::from_utf8_lossy(l));
                s.push('\n');
            }
            // O `-S` escreve as cabeças mesmo com `-q`.
            let _ = self.ctx.stderr().write_all(s.as_bytes());
        } else {
            self.log.verbose(self.ctx, &format!("{} {}\n", head.status, String::from_utf8_lossy(&head.reason)));
        }
    }

    /// Lê o corpo inteiro (quando não interessa) para manter a conexão utilizável.
    fn drain(&mut self, head: &Head, is_head: bool) {
        let mode = http::body_mode(head, is_head);
        let mut ok = mode != BodyMode::Close;
        if let Some(k) = self.kept.as_mut() {
            let mut r = k.conn.body_reader(mode);
            loop {
                match r.next_piece() {
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(_) => {
                        ok = false;
                        break;
                    }
                }
            }
        }
        if !ok || closes(head) {
            self.kept = None;
        }
    }

    fn one(&mut self, raw: &str) -> Code {
        let raw = if raw.contains("://") { raw.to_string() } else { format!("http://{raw}") };
        let mut u = match url::parse(&raw, false) {
            Ok(u) if u.scheme == "http" || u.scheme == "https" => u,
            Ok(_) | Err(url::UrlError::BadScheme) => {
                self.log.notquiet(self.ctx, &format!("{raw}: Unsupported scheme.\n"));
                return 1;
            }
            Err(e) => {
                self.log.notquiet(self.ctx, &format!("{raw}: {}.\n", e.message()));
                return 1;
            }
        };
        // Destino local (decidido pela URL pedida, antes dos redirecionamentos).
        let to_stdout = self.o.output.as_deref() == Some("-");
        let mut name = match &self.o.output {
            Some(p) => p.clone(),
            None => {
                let n = local_name(&u);
                match &self.o.prefix {
                    Some(dir) => format!("{}/{n}", dir.trim_end_matches('/')),
                    None => n,
                }
            }
        };
        if self.o.output.is_none() && !self.o.spider {
            if self.o.no_clobber && exists(&name).is_some() {
                self.log.verbose(self.ctx, &format!("File ‘{name}’ already there; not retrieving.\n\n"));
                return 0;
            }
            if !self.o.cont && exists(&name).is_some() {
                let mut k = 1;
                while exists(&format!("{name}.{k}")).is_some() {
                    k += 1;
                }
                name = format!("{name}.{k}");
            }
        }
        let have = if self.o.cont && !to_stdout { exists(&name).unwrap_or(0) } else { 0 };
        let mut req = Req {
            method: self.o.method.clone().unwrap_or_else(|| if self.o.spider { "HEAD".into() } else { "GET".into() }),
            body: self.o.body.clone(),
            auth: None,
            range: (have > 0).then_some(have),
        };
        let mut redirects = 0u32;
        let mut tried_auth = false;
        loop {
            let url_text = u.to_string_with(false);
            self.log.verbose(self.ctx, &format!("--{}--  {url_text}\n", stamp()));
            let head = match self.send(&u, &req) {
                Ok(h) => h,
                Err(code) => return code,
            };
            self.show_head(&head);
            let is_head = req.method == "HEAD";
            if matches!(head.status, 301 | 302 | 303 | 307 | 308)
                && let Some(loc) = head.get_str("Location")
            {
                self.drain(&head, is_head);
                self.log.verbose(self.ctx, &format!("Location: {loc} [following]\n"));
                if redirects >= self.o.max_redirect {
                    self.log.notquiet(self.ctx, &format!("{} redirections exceeded.\n", self.o.max_redirect));
                    return 8;
                }
                redirects += 1;
                match url::resolve(&u, &loc) {
                    Ok(next) => u = next,
                    Err(e) => {
                        self.log.notquiet(self.ctx, &format!("{loc}: {}.\n", e.message()));
                        return 1;
                    }
                }
                if head.status != 307 && head.status != 308 && req.method != "HEAD" {
                    req.method = "GET".into();
                    req.body = None;
                }
                continue;
            }
            if head.status == 401
                && !tried_auth
                && let Some(user) = &self.o.user
            {
                let challenge = head.get_str("WWW-Authenticate").unwrap_or_default();
                if challenge.to_ascii_lowercase().starts_with("basic") {
                    self.drain(&head, is_head);
                    self.log.verbose(self.ctx, &format!("Authentication selected: {challenge}\n"));
                    let pw = self.o.password.clone().unwrap_or_default();
                    req.auth = Some(format!("Basic {}", base64(format!("{user}:{pw}").as_bytes())));
                    tried_auth = true;
                    continue;
                }
            }
            if head.status == 416 && have > 0 {
                self.drain(&head, is_head);
                self.log.verbose(self.ctx, "\n    The file is already fully retrieved; nothing to do.\n\n");
                return 0;
            }
            if self.o.spider {
                self.drain(&head, is_head);
                if head.status >= 400 {
                    self.log.verbose(self.ctx, "Remote file does not exist -- broken link!!!\n\n");
                    return 8;
                }
                self.length_line(&head, 0);
                self.log.verbose(self.ctx, "Remote file exists.\n\n");
                return 0;
            }
            if head.status >= 400 || !(200..300).contains(&head.status) && head.status != 304 {
                self.drain(&head, is_head);
                if self.log.level == Verbosity::NoVerbose {
                    self.log.notquiet(self.ctx, &format!("{url_text}:\n"));
                }
                self.log.notquiet(self.ctx, &format!("{} ERROR {}: {}.\n", stamp(), head.status, String::from_utf8_lossy(&head.reason)));
                self.log.verbose(self.ctx, "\n");
                return 8;
            }
            let mode = http::body_mode(&head, is_head);
            let partial = head.status == 206 && have > 0;
            if have > 0 && !partial && let BodyMode::Length(n) = mode && n <= have {
                self.drain(&head, is_head);
                self.log.verbose(self.ctx, "\n    The file is already fully retrieved; nothing to do.\n\n");
                return 0;
            }
            return self.download(&url_text, &head, mode, &name, to_stdout, if partial { have } else { 0 });
        }
    }

    fn length_line(&mut self, head: &Head, initial: u64) {
        let ctype = head.get_str("Content-Type").map(|t| t.split(';').next().unwrap_or("").trim().to_string());
        let ty = ctype.map(|t| format!(" [{t}]")).unwrap_or_default();
        let line = match http::body_mode(head, false) {
            BodyMode::Length(n) => {
                let total = n + initial;
                let mut s = format!("Length: {total}");
                if total >= 1024 {
                    s.push_str(&format!(" ({})", human(total)));
                }
                if initial > 0 {
                    s.push_str(&format!(", {n} ({}) remaining", human(n)));
                }
                s
            }
            _ => "Length: unspecified".into(),
        };
        self.log.verbose(self.ctx, &format!("{line}{ty}\n"));
    }

    fn download(&mut self, url_text: &str, head: &Head, mode: BodyMode, name: &str, to_stdout: bool, initial: u64) -> Code {
        self.length_line(head, initial);
        let shown = if to_stdout { "STDOUT".to_string() } else { name.to_string() };
        self.log.verbose(self.ctx, &format!("Saving to: ‘{shown}’\n"));
        // Destino: `-O` abre uma vez; nome pela URL abre agora (append no `-c`).
        let fd = if to_stdout {
            None
        } else if self.o.output.is_some() && self.doc.is_some() {
            self.doc
        } else {
            let flags = if initial > 0 { OFlags::WRONLY | OFlags::CREAT | OFlags::APPEND } else { OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC };
            match sys::open(name.as_bytes(), flags, Mode::from(0o666u32)) {
                Ok(f) => {
                    if self.o.output.is_some() {
                        self.doc = Some(f);
                    }
                    Some(f)
                }
                Err(e) => {
                    self.log.notquiet(self.ctx, &format!("{name}: {}\n", e.message()));
                    self.kept = None;
                    return 3;
                }
            }
        };
        let total = match mode {
            BodyMode::Length(n) => Some(n + initial),
            _ => None,
        };
        let mut dots = Dots::new(self.log.level == Verbosity::Verbose, total, initial);
        dots.begin(self.ctx);
        let mut got = 0u64;
        let mut failed = None;
        {
            let Some(k) = self.kept.as_mut() else { return 4 };
            let mut r = k.conn.body_reader(mode);
            loop {
                match r.next_piece() {
                    Ok(Some(p)) => {
                        got += p.len() as u64;
                        let w = match fd {
                            Some(f) => sys::write_all(f, &p).is_ok(),
                            None => self.ctx.stdout().write_all(&p).is_ok(),
                        };
                        if !w {
                            failed = Some(3);
                            break;
                        }
                        dots.update(self.ctx, p.len() as u64);
                    }
                    Ok(None) => break,
                    Err(BodyError::Partial(_) | BodyError::BadChunk | BodyError::Io(_)) => {
                        failed = Some(4);
                        break;
                    }
                }
            }
        }
        if fd.is_some() && self.o.output.is_none() {
            let _ = sys::close(fd.unwrap_or(Fd::STDOUT));
        }
        let secs = dots.elapsed();
        dots.finish(self.ctx);
        if failed.is_some() || mode == BodyMode::Close || closes(head) {
            self.kept = None;
        }
        if let Some(code) = failed {
            self.log.notquiet(self.ctx, &format!("{} ({}) - Read error at byte {}{}. \n", stamp(), rate_text(got, secs), got + initial, total.map(|t| format!("/{t}")).unwrap_or_default()));
            return code;
        }
        let done = got + initial;
        let sizes = match total {
            Some(t) => format!("[{done}/{t}]"),
            None => format!("[{done}]"),
        };
        match self.log.level {
            Verbosity::Verbose => {
                let what = if to_stdout { "written to stdout".to_string() } else { format!("‘{name}’ saved") };
                self.log.verbose(self.ctx, &format!("{} ({}) - {what} {sizes}\n\n", stamp(), rate_text(got, secs)));
            }
            Verbosity::NoVerbose => {
                let target = if to_stdout { "-" } else { name };
                self.log.notquiet(self.ctx, &format!("{} URL:{url_text} {sizes} -> \"{target}\" [1]\n", stamp()));
            }
            Verbosity::Quiet => {}
        }
        0
    }
}

fn closes(head: &Head) -> bool {
    head.version == (1, 0) || head.get_str("Connection").is_some_and(|c| c.eq_ignore_ascii_case("close"))
}

pub fn main(ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let o = match parse(ctx, argv) {
        Ok(o) => o,
        Err(Exit(code)) => return code,
    };
    if o.urls.is_empty() {
        let _ = ctx.stderr().write_all(format!("wget: missing URL\n{USAGE_TAIL}").as_bytes());
        return 1;
    }
    let log = Log { level: o.verbosity };
    if o.spider {
        log.verbose(ctx, "Spider mode enabled. Check if remote file exists.\n");
    }
    let mut w = Wget { ctx, o: &o, log, kept: None, doc: None };
    // `-O arquivo` trunca já no começo, como o wget (fica vazio se nada for baixado).
    if let Some(p) = &o.output
        && p != "-"
        && !o.spider
        && !o.cont
    {
        match sys::open(p.as_bytes(), OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, Mode::from(0o666u32)) {
            Ok(f) => w.doc = Some(f),
            Err(e) => {
                let _ = w.ctx.stderr().write_all(format!("{p}: {}\n", e.message()).as_bytes());
                return 3;
            }
        }
    }
    if let Some(dir) = &o.prefix {
        let mut cur = String::new();
        for (i, part) in dir.split('/').enumerate() {
            if i > 0 || dir.starts_with('/') {
                cur.push('/');
            }
            cur.push_str(part);
            if !part.is_empty() {
                let _ = sys::current().mkdirat(Fd::CWD, cur.as_bytes(), Mode::from(0o777u32));
            }
        }
    }
    let mut rc = 0;
    for raw in &o.urls {
        let code = w.one(raw);
        if code != 0 {
            rc = code;
        }
    }
    if let Some(f) = w.doc {
        let _ = sys::close(f);
    }
    rc
}
