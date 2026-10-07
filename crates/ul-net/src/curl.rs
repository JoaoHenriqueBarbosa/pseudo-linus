//! `curl` 8.14.1 (o do Debian 13) sobre a pilha de [`crate::net`].
//!
//! Cobre o uso de linha de comando de um agente: GET, HEAD e os outros métodos, corpo por `-d` e
//! variantes, `--json`, `-F`, `-T`, cabeçalhos, autenticação básica, redirecionamentos, `-f`,
//! `-o`/`-O`/`-D`, `-i`/`-I`, `-w`, `--compressed`, `-v` e o medidor de progresso. As mensagens de erro e
//! os códigos de saída são os do curl. Sem rede externa: um destino fora do loopback não resolve
//! (código 6), como num sandbox sem DNS.

use std::ffi::OsString;
use std::io::Write as _;
use std::os::unix::ffi::OsStrExt;
use std::time::Duration;

use sysabi::{Ctx, Errno, Fd, Mode, OFlags, sys};

use crate::net::http::{self, BodyError, BodyMode, Conn, Decoder, Head, HttpError, Stream};
use crate::net::io::{self, Deadline};
use crate::net::url::{self, Url, UrlError};

const VERSION: &str = "8.14.1";
const MAX_REDIRS: u32 = 50;

/// Erro com o código de saída do curl e a mensagem (sem o prefixo `curl: (N) `).
struct Fail(i32, String);

#[derive(Default)]
struct Opts {
    urls: Vec<String>,
    silent: bool,
    show_error: bool,
    fail: bool,
    fail_with_body: bool,
    location: bool,
    max_redirs: Option<u32>,
    outputs: Vec<Output>,
    create_dirs: bool,
    write_out: Option<String>,
    head: bool,
    include: bool,
    dump_header: Option<String>,
    method: Option<String>,
    data: Vec<u8>,
    has_data: bool,
    get: bool,
    headers: Vec<String>,
    user_agent: Option<String>,
    referer: Option<String>,
    user: Option<String>,
    max_time: Option<Duration>,
    connect_timeout: Option<Duration>,
    insecure: bool,
    compressed: bool,
    upload: Option<String>,
    form: Vec<String>,
    json: bool,
    verbose: bool,
    path_as_is: bool,
    globoff: bool,
}

#[derive(Clone)]
enum Output {
    File(String),
    Remote,
}

/// Lê o argumento de uma opção (o que vem colado em `-ofile` ou o próximo argv).
struct Args<'a> {
    argv: &'a [OsString],
    i: usize,
}

impl Args<'_> {
    fn next(&mut self) -> Option<String> {
        let a = self.argv.get(self.i)?;
        self.i += 1;
        Some(String::from_utf8_lossy(a.as_bytes()).into_owned())
    }
}

fn need(args: &mut Args, name: &str) -> Result<String, Fail> {
    args.next().ok_or_else(|| Fail(2, (format!("option {name}: requires parameter")).into()))
}

fn seconds(v: &str, name: &str) -> Result<Duration, Fail> {
    v.parse::<f64>()
        .ok()
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map(Duration::from_secs_f64)
        .ok_or_else(|| Fail(2, (format!("option {name}: expected a proper numerical parameter")).into()))
}

/// Conteúdo de `-d @arquivo`/`@-`. `strip` tira CR e LF (o `-d` faz isso, o `--data-binary` não).
fn data_arg(ctx: &mut Ctx, v: &str, at: bool, strip: bool) -> Result<Vec<u8>, Fail> {
    let mut bytes = if at && let Some(path) = v.strip_prefix('@') {
        read_source(ctx, path)?
    } else {
        v.as_bytes().to_vec()
    };
    if strip {
        bytes.retain(|&b| b != b'\r' && b != b'\n');
    }
    Ok(bytes)
}

fn read_source(ctx: &mut Ctx, path: &str) -> Result<Vec<u8>, Fail> {
    if path == "-" {
        let mut out = Vec::new();
        let _ = std::io::Read::read_to_end(&mut ctx.stdin(), &mut out);
        return Ok(out);
    }
    sys::read_file(path.as_bytes()).map_err(|_| Fail(26, (format!("Failed to open {path}")).into()))
}

/// `application/x-www-form-urlencoded` como o curl codifica no `--data-urlencode`.
fn form_encode(data: &[u8]) -> String {
    let mut out = String::new();
    for &b in data {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn data_urlencode(ctx: &mut Ctx, v: &str) -> Result<Vec<u8>, Fail> {
    if let Some(eq) = v.find('=') {
        let (name, content) = (&v[..eq], &v[eq + 1..]);
        let enc = form_encode(content.as_bytes());
        return Ok(if name.is_empty() { enc.into_bytes() } else { format!("{name}={enc}").into_bytes() });
    }
    if let Some(at) = v.find('@') {
        let (name, path) = (&v[..at], &v[at + 1..]);
        let enc = form_encode(&read_source(ctx, path)?);
        return Ok(if name.is_empty() { enc.into_bytes() } else { format!("{name}={enc}").into_bytes() });
    }
    Ok(form_encode(v.as_bytes()).into_bytes())
}

fn add_data(o: &mut Opts, piece: Vec<u8>) {
    if o.has_data {
        o.data.push(b'&');
    }
    o.data.extend(piece);
    o.has_data = true;
}

fn parse(ctx: &mut Ctx, argv: &[OsString]) -> Result<Opts, Fail> {
    let mut o = Opts::default();
    let mut args = Args { argv, i: 1 };
    while let Some(a) = args.next() {
        if a == "--" {
            while let Some(u) = args.next() {
                o.urls.push(u);
            }
            break;
        }
        if let Some(long) = a.strip_prefix("--") {
            let (name, no) = match long.strip_prefix("no-") {
                Some(rest) if matches!(rest, "silent" | "show-error" | "fail" | "location" | "include" | "insecure" | "compressed" | "verbose" | "create-dirs") => (rest, true),
                _ => (long, false),
            };
            let flag = !no;
            let opt = format!("--{long}");
            match name {
                "silent" => o.silent = flag,
                "show-error" => o.show_error = flag,
                "fail" => o.fail = flag,
                "fail-with-body" => o.fail_with_body = true,
                "location" => o.location = flag,
                "max-redirs" => o.max_redirs = need(&mut args, &opt)?.parse().ok(),
                "output" => o.outputs.push(Output::File(need(&mut args, &opt)?)),
                "remote-name" => o.outputs.push(Output::Remote),
                "create-dirs" => o.create_dirs = flag,
                "write-out" => o.write_out = Some(need(&mut args, &opt)?),
                "head" => o.head = true,
                "include" => o.include = flag,
                "dump-header" => o.dump_header = Some(need(&mut args, &opt)?),
                "request" => o.method = Some(need(&mut args, &opt)?),
                "data" | "data-ascii" => {
                    let v = need(&mut args, &opt)?;
                    let d = data_arg(ctx, &v, true, true)?;
                    add_data(&mut o, d);
                }
                "data-binary" => {
                    let v = need(&mut args, &opt)?;
                    let d = data_arg(ctx, &v, true, false)?;
                    add_data(&mut o, d);
                }
                "data-raw" => {
                    let v = need(&mut args, &opt)?;
                    add_data(&mut o, v.into_bytes());
                }
                "data-urlencode" => {
                    let v = need(&mut args, &opt)?;
                    let d = data_urlencode(ctx, &v)?;
                    add_data(&mut o, d);
                }
                "json" => {
                    let v = need(&mut args, &opt)?;
                    let d = data_arg(ctx, &v, true, false)?;
                    // O `--json` junta os pedaços sem separador.
                    o.data.extend(d);
                    o.has_data = true;
                    o.json = true;
                }
                "get" => o.get = true,
                "header" => o.headers.push(need(&mut args, &opt)?),
                "user-agent" => o.user_agent = Some(need(&mut args, &opt)?),
                "referer" => o.referer = Some(need(&mut args, &opt)?),
                "user" => o.user = Some(need(&mut args, &opt)?),
                "max-time" => o.max_time = Some(seconds(&need(&mut args, &opt)?, &opt)?),
                "connect-timeout" => o.connect_timeout = Some(seconds(&need(&mut args, &opt)?, &opt)?),
                "insecure" => o.insecure = flag,
                "compressed" => o.compressed = flag,
                "upload-file" => o.upload = Some(need(&mut args, &opt)?),
                "form" | "form-string" => o.form.push(need(&mut args, &opt)?),
                "verbose" => o.verbose = flag,
                "url" => o.urls.push(need(&mut args, &opt)?),
                "path-as-is" => o.path_as_is = true,
                "globoff" => o.globoff = true,
                // Sem efeito aqui: só HTTP/1.1, sem retentativa nem buffer.
                "http1.1" | "http1.0" | "no-buffer" | "ipv4" | "ipv6" | "no-keepalive" | "no-progress-meter" | "tcp-nodelay" => {
                    if name == "no-progress-meter" {
                        o.silent = true;
                        o.show_error = true;
                    }
                }
                "retry" | "retry-delay" | "retry-max-time" | "cacert" | "capath" | "cookie" | "cookie-jar" | "proxy" | "resolve" => {
                    need(&mut args, &opt)?;
                }
                "help" => {
                    let _ = ctx.stdout().write_all(HELP.as_bytes());
                    return Err(Fail(0, String::new()));
                }
                "version" => {
                    let _ = ctx.stdout().write_all(version_text().as_bytes());
                    return Err(Fail(0, String::new()));
                }
                _ => return Err(Fail(2, (format!("option {opt}: is unknown")).into())),
            }
            continue;
        }
        if a.len() > 1 && a.starts_with('-') {
            let flags: Vec<char> = a[1..].chars().collect();
            let mut k = 0;
            while k < flags.len() {
                let c = flags[k];
                k += 1;
                // Opção com argumento: o resto do grupo ou o próximo argv.
                let mut value = |args: &mut Args| -> Result<String, Fail> {
                    if k < flags.len() {
                        let v: String = flags[k..].iter().collect();
                        k = flags.len();
                        Ok(v)
                    } else {
                        need(args, &format!("-{c}"))
                    }
                };
                match c {
                    's' => o.silent = true,
                    'S' => o.show_error = true,
                    'f' => o.fail = true,
                    'L' => o.location = true,
                    'o' => o.outputs.push(Output::File(value(&mut args)?)),
                    'O' => o.outputs.push(Output::Remote),
                    'w' => o.write_out = Some(value(&mut args)?),
                    'I' => o.head = true,
                    'i' => o.include = true,
                    'D' => o.dump_header = Some(value(&mut args)?),
                    'X' => o.method = Some(value(&mut args)?),
                    'd' => {
                        let v = value(&mut args)?;
                        let d = data_arg(ctx, &v, true, true)?;
                        add_data(&mut o, d);
                    }
                    'G' => o.get = true,
                    'H' => o.headers.push(value(&mut args)?),
                    'A' => o.user_agent = Some(value(&mut args)?),
                    'e' => o.referer = Some(value(&mut args)?),
                    'u' => o.user = Some(value(&mut args)?),
                    'm' => o.max_time = Some(seconds(&value(&mut args)?, "-m")?),
                    'k' => o.insecure = true,
                    'T' => o.upload = Some(value(&mut args)?),
                    'F' => o.form.push(value(&mut args)?),
                    'v' => o.verbose = true,
                    'g' => o.globoff = true,
                    'N' | '4' | '6' | '#' => {}
                    'b' | 'c' | 'x' => {
                        value(&mut args)?;
                    }
                    'h' => {
                        let _ = ctx.stdout().write_all(HELP.as_bytes());
                        return Err(Fail(0, String::new()));
                    }
                    'V' => {
                        let _ = ctx.stdout().write_all(version_text().as_bytes());
                        return Err(Fail(0, String::new()));
                    }
                    _ => return Err(Fail(2, (format!("option -{c}: is unknown")).into())),
                }
            }
            continue;
        }
        o.urls.push(a);
    }
    Ok(o)
}

fn version_text() -> String {
    format!(
        "curl {VERSION} (x86_64-pc-linux-gnu) libcurl/{VERSION} OpenSSL/3.5.7 zlib/1.3.1 brotli/1.1.0 zstd/1.5.7 libidn2/2.3.8 libpsl/0.21.2 libssh2/1.11.1 nghttp2/1.64.0 nghttp3/1.8.0 librtmp/2.3 OpenLDAP/2.6.10\n\
Release-Date: 2025-06-04, security patched: 8.14.1-2+deb13u5\n\
Protocols: dict file ftp ftps gopher gophers http https imap imaps ipfs ipns ldap ldaps mqtt pop3 pop3s rtmp rtsp scp sftp smb smbs smtp smtps telnet tftp ws wss\n\
Features: alt-svc AsynchDNS brotli GSS-API HSTS HTTP2 HTTP3 HTTPS-proxy IDN IPv6 Kerberos Largefile libz NTLM PSL SPNEGO SSL threadsafe TLS-SRP UnixSockets zstd\n"
    )
}

const HELP: &str = "Usage: curl [options...] <url>
 -d, --data <data>           HTTP POST data
 -f, --fail                  Fail fast with no output on HTTP errors
 -h, --help <subject>        Get help for commands
 -o, --output <file>         Write to file instead of stdout
 -O, --remote-name           Write output to file named as remote file
 -i, --show-headers          Show response headers in output
 -s, --silent                Silent mode
 -T, --upload-file <file>    Transfer local FILE to destination
 -u, --user <user:password>  Server user and password
 -A, --user-agent <name>     Send User-Agent <name> to server
 -v, --verbose               Make the operation more talkative
 -V, --version               Show version number and quit

This is not the full help; this menu is split into categories.
Use \"--help category\" to get an overview of all categories, which are:
auth, connection, content, curl, deprecated, dns, file, ftp, global, http, imap, ldap, output, pop3, post,
proxy, scp, sftp, smtp, ssh, telnet, tftp, tls, ufile, upload, verbose.
Use \"--help all\" to list all options
Use \"--help [option]\" to view documentation for a given option
";

/// Base64 do `-u`.
use ul_common::codec::base64_string as base64;

/// O que se sabe de uma transferência, pro `-w`.
#[derive(Default, Clone)]
struct Info {
    code: u16,
    content_type: String,
    size_download: u64,
    size_header: u64,
    size_upload: u64,
    num_redirects: u32,
    url_effective: String,
    url: String,
    redirect_url: String,
    method: String,
    scheme: String,
    remote_ip: String,
    remote_port: u16,
    local_port: u16,
    http_version: String,
    time_total: f64,
    time_connect: f64,
    time_starttransfer: f64,
    filename: String,
    exitcode: i32,
    errormsg: String,
    headers: Vec<(Vec<u8>, Vec<u8>)>,
}

/// O medidor de progresso do curl (`lib/progress.c`): cabeçalho, uma linha no início e a final.
struct Meter {
    on: bool,
    started: bool,
}

fn max5(n: u64) -> String {
    if n < 100_000 {
        return format!("{n}");
    }
    if n < 10_000 * 1024 {
        return format!("{}k", n / 1024);
    }
    if n < 100 * 1024 * 1024 {
        return format!("{}.{}M", n / (1024 * 1024), (n % (1024 * 1024)) / (1024 * 1024 / 10));
    }
    format!("{}M", n / (1024 * 1024))
}

fn time8(secs: u64) -> String {
    if secs == 0 {
        return "--:--:--".to_string();
    }
    format!("{:2}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

impl Meter {
    fn line(&self, total: Option<u64>, down: u64, up: u64, elapsed: f64) -> String {
        let pct = |n: u64, t: Option<u64>| match t {
            Some(t) if t > 0 => n * 100 / t,
            _ => 0,
        };
        let speed = if elapsed > 0.0 { (down as f64 / elapsed) as u64 } else { 0 };
        let t = total.unwrap_or(0);
        format!(
            "\r{:3} {:>5}  {:3} {:>5}  {:3} {:>5}  {:>5}  {:>5} {} {} {} {:>5}",
            pct(down, total),
            max5(t),
            pct(down, total),
            max5(down),
            0,
            max5(up),
            max5(speed),
            0,
            "--:--:--",
            time8(elapsed as u64),
            "--:--:--",
            max5(speed)
        )
    }

    fn start(&mut self, ctx: &mut Ctx) {
        if self.on && !self.started {
            self.started = true;
            let mut e = ctx.stderr();
            let _ = e.write_all(
                b"  % Total    % Received % Xferd  Average Speed   Time    Time     Time  Current\n                                 Dload  Upload   Total   Spent    Left  Speed\n",
            );
            let _ = e.write_all(self.line(None, 0, 0, 0.0).as_bytes());
        }
    }

    fn finish(&mut self, ctx: &mut Ctx, total: Option<u64>, down: u64, up: u64, elapsed: f64) {
        if self.on && self.started {
            let line = self.line(total, down, up, elapsed);
            let _ = ctx.stderr().write_all(format!("{line}\n").as_bytes());
        }
    }
}

/// Destino do corpo.
enum Sink {
    Stdout,
    File { path: String, fd: Option<Fd> },
    Null,
}

impl Sink {
    fn write(&mut self, ctx: &mut Ctx, data: &[u8]) -> Result<(), Fail> {
        match self {
            Sink::Stdout => ctx.stdout().write_all(data).map_err(|_| Fail(23, (format!("Failure writing output to destination, passed {} returned 0", data.len())).into())),
            Sink::Null => Ok(()),
            Sink::File { path, fd } => {
                if fd.is_none() {
                    let f = sys::open(path.as_bytes(), OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, Mode::from(0o666u32))
                        .map_err(|e| Fail(23, (format!("Failed writing body: {path}: {}", e.message())).into()))?;
                    *fd = Some(f);
                }
                sys::write_all(fd.unwrap_or(Fd::STDOUT), data).map_err(|_| Fail(23, ("Failure writing output to destination").into()))
            }
        }
    }

    /// Abre o arquivo mesmo sem corpo (o curl cria a saída vazia).
    fn touch(&mut self, ctx: &mut Ctx) -> Result<(), Fail> {
        self.write(ctx, b"")
    }

    fn close(&mut self) {
        if let Sink::File { fd: Some(f), .. } = self {
            let _ = sys::close(*f);
        }
    }
}

fn mkdirs(path: &str) {
    let mut cur = String::new();
    let parts: Vec<&str> = path.split('/').collect();
    for (i, p) in parts.iter().enumerate() {
        if i + 1 == parts.len() {
            break;
        }
        if i > 0 || path.starts_with('/') {
            cur.push('/');
        }
        cur.push_str(p);
        if !p.is_empty() {
            let _ = sys::current().mkdirat(Fd::CWD, cur.as_bytes(), Mode::from(0o777u32));
        }
    }
}

/// Nome do `-O`: o último segmento do caminho.
fn remote_name(u: &Url) -> Result<String, Fail> {
    let name = u.path.rsplit('/').next().unwrap_or("");
    if name.is_empty() {
        return Err(Fail(23, ("Remote filename has no length").into()));
    }
    Ok(String::from_utf8_lossy(&url::percent_decode(name)).into_owned())
}

/// Monta o corpo multipart do `-F`.
fn multipart(ctx: &mut Ctx, fields: &[String]) -> Result<(Vec<u8>, String), Fail> {
    let boundary = format!("------------------------{:016x}", io::wall().1 as u64 ^ 0x5eed_c0ff_ee00_1234);
    let mut body = Vec::new();
    for f in fields {
        let (name, value) = f.split_once('=').ok_or_else(|| Fail(26, ("Failed to read form data").into()))?;
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        if let Some(path) = value.strip_prefix('@') {
            let path = path.split(';').next().unwrap_or(path);
            let data = read_source(ctx, path)?;
            let fname = path.rsplit('/').next().unwrap_or(path);
            body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{fname}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes(),
            );
            body.extend_from_slice(&data);
        } else {
            let data = if let Some(path) = value.strip_prefix('<') { read_source(ctx, path)? } else { value.as_bytes().to_vec() };
            body.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes());
            body.extend_from_slice(&data);
        }
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok((body, format!("multipart/form-data; boundary={boundary}")))
}

/// Um pedido HTTP pronto pra mandar.
struct Request {
    method: String,
    body: Vec<u8>,
    content_type: Option<String>,
    has_body: bool,
}

fn build_request(ctx: &mut Ctx, o: &Opts) -> Result<Request, Fail> {
    let mut r = Request { method: "GET".into(), body: Vec::new(), content_type: None, has_body: false };
    if o.head {
        r.method = "HEAD".into();
    }
    if let Some(path) = &o.upload {
        r.method = "PUT".into();
        r.body = read_source(ctx, path)?;
        r.has_body = true;
    } else if !o.form.is_empty() {
        let (body, ct) = multipart(ctx, &o.form)?;
        r.method = "POST".into();
        r.body = body;
        r.content_type = Some(ct);
        r.has_body = true;
    } else if o.has_data && !o.get {
        r.method = "POST".into();
        r.body = o.data.clone();
        r.content_type = Some(if o.json { "application/json" } else { "application/x-www-form-urlencoded" }.into());
        r.has_body = true;
    }
    if let Some(m) = &o.method {
        r.method = m.clone();
    }
    Ok(r)
}

/// Uma URL do usuário: o `-G` põe os dados na query.
fn target_url(o: &Opts, raw: &str) -> Result<Url, Fail> {
    let mut u = url::parse(raw, o.path_as_is).map_err(|e| url_fail(raw, e))?;
    if o.get && o.has_data {
        let q = String::from_utf8_lossy(&o.data).into_owned();
        u.query = Some(match &u.query {
            Some(old) if !old.is_empty() => format!("{old}&{q}"),
            _ => q,
        });
    }
    Ok(u)
}

fn url_fail(raw: &str, e: UrlError) -> Fail {
    if e == UrlError::BadScheme
        && let Some(scheme) = raw.split_once("://").map(|(s, _)| s)
    {
        return Fail(1, (format!("Protocol \"{scheme}\" not supported")).into());
    }
    Fail(3, (format!("URL rejected: {}", e.message())).into())
}

fn ms(d: Duration) -> u128 {
    d.as_millis()
}

struct Transfer<'a> {
    ctx: &'a mut Ctx,
    o: &'a Opts,
    meter: Meter,
    started: Duration,
}

impl Transfer<'_> {
    fn verbose(&mut self, prefix: &str, text: &str) {
        if self.o.verbose {
            let mut out = String::new();
            for line in text.split_inclusive('\n') {
                out.push_str(prefix);
                out.push_str(line);
            }
            if !out.ends_with('\n') {
                out.push('\n');
            }
            let _ = self.ctx.stderr().write_all(out.as_bytes());
        }
    }

    fn elapsed(&self) -> f64 {
        (io::now().saturating_sub(self.started)).as_secs_f64()
    }

    fn deadline(&self) -> Deadline {
        let left = self.o.max_time.map(|d| (self.started + d).saturating_sub(io::now()));
        Deadline::after(left)
    }

    /// `file://`: lê o arquivo local.
    fn file(&mut self, u: &Url, sink: &mut Sink, info: &mut Info) -> Result<(), Fail> {
        let path = String::from_utf8_lossy(&url::percent_decode(&u.path)).into_owned();
        let data = sys::read_file(path.as_bytes()).map_err(|_| Fail(37, (format!("Couldn't open file {path}")).into()))?;
        info.size_download = data.len() as u64;
        if !self.o.head {
            sink.write(self.ctx, &data)?;
        }
        Ok(())
    }

    fn run(&mut self, raw: &str, sink: &mut Sink) -> Result<Info, (Info, Fail)> {
        let mut info = Info { url: raw.to_string(), ..Info::default() };
        match self.run_inner(raw, sink, &mut info) {
            Ok(()) => Ok(info),
            Err(f) => Err((info, f)),
        }
    }

    fn run_inner(&mut self, raw: &str, sink: &mut Sink, info: &mut Info) -> Result<(), Fail> {
        let mut u = target_url(self.o, raw)?;
        info.url_effective = u.to_string_with(false);
        info.scheme = u.scheme.to_ascii_uppercase();
        if u.scheme == "file" {
            return self.file(&u, sink, info);
        }
        if u.scheme != "http" && u.scheme != "https" {
            return Err(Fail(1, (format!("Protocol \"{}\" not supported", u.scheme)).into()));
        }
        let mut req = build_request(self.ctx, self.o)?;
        info.method = req.method.clone();
        let max = self.o.max_redirs.unwrap_or(MAX_REDIRS);
        self.meter.start(self.ctx);
        loop {
            let head = self.exchange(&u, &req, sink, info)?;
            let Some(head) = head else { return Ok(()) };
            let redirect = matches!(head.status, 301 | 302 | 303 | 307 | 308);
            let location = head.get_str("Location");
            if let (true, Some(loc)) = (redirect, &location) {
                let next = url::resolve(&u, loc).map_err(|e| url_fail(loc, e))?;
                info.redirect_url = next.to_string_with(false);
                if !self.o.location {
                    return Ok(());
                }
                if info.num_redirects >= max {
                    return Err(Fail(47, (format!("Maximum ({max}) redirects followed")).into()));
                }
                info.num_redirects += 1;
                // Como o curl: 301/302/303 trocam POST por GET (303 troca qualquer método que não seja HEAD).
                if (matches!(head.status, 301 | 302) && req.method == "POST" && self.o.method.is_none())
                    || (head.status == 303 && req.method != "HEAD")
                {
                    req.method = "GET".into();
                    req.body.clear();
                    req.has_body = false;
                    req.content_type = None;
                }
                u = next;
                info.url_effective = u.to_string_with(false);
                info.redirect_url.clear();
                continue;
            }
            return Ok(());
        }
    }

    /// Um pedido e sua resposta. Devolve a cabeça quando é um redirecionamento a seguir (o corpo foi
    /// descartado); `None` quando a resposta final já foi entregue.
    fn exchange(&mut self, u: &Url, req: &Request, sink: &mut Sink, info: &mut Info) -> Result<Option<Head>, Fail> {
        let port = u.port_or_default();
        let host = u.host.clone();
        let tcp = match io::connect(host.as_bytes(), port, self.o.connect_timeout.or(self.o.max_time)) {
            Ok(t) => t,
            Err(Errno::ECONNREFUSED) => {
                let after = ms(io::now().saturating_sub(self.started));
                return Err(Fail(7, (format!("Failed to connect to {host} port {port} after {after} ms: Could not connect to server")).into()));
            }
            Err(Errno::ETIMEDOUT) => {
                let after = ms(io::now().saturating_sub(self.started));
                return Err(Fail(28, (format!("Failed to connect to {host} port {port} after {after} ms: Timeout was reached")).into()));
            }
            // Sem rede fora do loopback: o nome não resolve.
            Err(_) => return Err(Fail(6, (format!("Could not resolve host: {host}")).into())),
        };
        info.remote_ip = tcp.peer.ip().to_string();
        info.remote_port = tcp.peer.port();
        info.local_port = tcp.local.port();
        info.time_connect = self.elapsed();
        self.verbose("* ", &format!("  Trying {}:{port}...\nConnected to {host} ({}) port {port}", info.remote_ip, info.remote_ip));
        let stream = if u.scheme == "https" {
            let verify = if self.o.insecure {
                crate::net::tls::Verify::Insecure
            } else {
                let pem = sys::read_file(crate::net::tls::DEFAULT_CA_BUNDLE.as_bytes()).unwrap_or_default();
                crate::net::tls::Verify::Roots(pem)
            };
            let cfg = crate::net::tls::client_config(&verify, &[b"http/1.1"]).map_err(|e| Fail(35, e.into()))?;
            let t = crate::net::tls::TlsStream::handshake(cfg, &host, tcp).map_err(|e| Fail(60, (format!("{e:?}")).into()))?;
            Stream::Tls(Box::new(t))
        } else {
            Stream::Plain(tcp)
        };
        let mut conn = Conn::new(stream);
        conn.stream.set_deadline(self.deadline());
        let text = self.request_text(u, req);
        self.verbose("> ", &String::from_utf8_lossy(&text).replace("\r\n", "\n"));
        let mut wire = text;
        wire.extend_from_slice(&req.body);
        conn.stream.write_all(&wire).map_err(|e| self.io_fail(&e, info))?;
        info.size_upload += req.body.len() as u64;
        let head = loop {
            let h = conn.read_head().map_err(|e| match e {
                HttpError::Empty => Fail(52, ("Empty reply from server").into()),
                HttpError::WeirdReply => Fail(1, ("Received HTTP/0.9 when not allowed").into()),
                HttpError::TooLarge => Fail(56, ("Too large response headers").into()),
                HttpError::Io(e) => self.io_fail(&e, info),
            })?;
            info.size_header += h.raw.len() as u64;
            if (100..200).contains(&h.status) && h.status != 101 {
                continue;
            }
            break h;
        };
        info.time_starttransfer = self.elapsed();
        info.code = head.status;
        info.content_type = head.get_str("Content-Type").unwrap_or_default();
        info.http_version = match head.version {
            (1, 0) => "1.0".into(),
            (2, _) => "2".into(),
            _ => "1.1".into(),
        };
        info.headers = head.headers.clone();
        self.verbose("< ", &String::from_utf8_lossy(&head.raw).replace("\r\n", "\n"));
        let following = self.o.location && matches!(head.status, 301 | 302 | 303 | 307 | 308) && head.get("Location").is_some();
        if let Some(path) = &self.o.dump_header {
            let mut d = if path == "-" { Sink::Stdout } else { Sink::File { path: path.clone(), fd: None } };
            d.write(self.ctx, &head.raw)?;
            d.close();
        }
        let failing = (self.o.fail || self.o.fail_with_body) && head.status >= 400 && !following;
        if (self.o.include || self.o.head) && !(failing && !self.o.fail_with_body) {
            sink.write(self.ctx, &head.raw)?;
        }
        let mode = http::body_mode(&head, req.method == "HEAD");
        let total = match mode {
            BodyMode::Length(n) => Some(n),
            _ => None,
        };
        let deliver = !following && !(failing && !self.o.fail_with_body);
        let mut decoder = match (self.o.compressed, head.get_str("Content-Encoding")) {
            (true, Some(enc)) => Some(Decoder::for_encoding(&enc).map_err(|e| Fail(61, (format!("Unrecognized content encoding type: {e}")).into()))?),
            _ => None,
        };
        // `-f` desiste na cabeça, sem ler o corpo (fechar com ele por ler manda RST ao servidor).
        if failing && !self.o.fail_with_body {
            return Err(Fail(22, (format!("The requested URL returned error: {}", head.status)).into()));
        }
        let mut down = 0u64;
        {
            let mut body = conn.body_reader(mode);
            loop {
                let piece = match body.next_piece() {
                    Ok(Some(p)) => p,
                    Ok(None) => break,
                    Err(BodyError::Partial(Some(left))) => {
                        info.size_download = down;
                        return Err(Fail(18, (format!("end of response with {left} bytes missing")).into()));
                    }
                    Err(BodyError::Partial(None)) => return Err(Fail(18, ("transfer closed with outstanding read data remaining").into())),
                    Err(BodyError::BadChunk) => return Err(Fail(56, ("invalid chunk encoding").into())),
                    Err(BodyError::Io(e)) => {
                        info.size_download = down;
                        return Err(self.io_fail(&e, info));
                    }
                };
                down += piece.len() as u64;
                if deliver {
                    let out = match decoder.as_mut() {
                        Some(d) => d.feed(&piece).map_err(|e| Fail(61, (format!("Error while processing content unencoding: {}", e.0)).into()))?,
                        None => piece,
                    };
                    sink.write(self.ctx, &out)?;
                }
            }
        }
        if deliver && let Some(d) = decoder.as_mut() {
            let rest = d.finish().map_err(|e| Fail(61, (format!("Error while processing content unencoding: {}", e.0)).into()))?;
            sink.write(self.ctx, &rest)?;
        }
        info.size_download += down;
        if deliver && !self.o.head {
            sink.touch(self.ctx)?;
        }
        if following {
            return Ok(Some(head));
        }
        if failing {
            let elapsed = self.elapsed();
            self.meter.finish(self.ctx, total, down, info.size_upload, elapsed);
            return Err(Fail(22, (format!("The requested URL returned error: {}", head.status)).into()));
        }
        if matches!(head.status, 301 | 302 | 303 | 307 | 308) && head.get("Location").is_some() {
            return Ok(Some(head));
        }
        Ok(None)
    }

    fn io_fail(&self, e: &std::io::Error, info: &Info) -> Fail {
        if e.kind() == std::io::ErrorKind::TimedOut {
            let after = ms(io::now().saturating_sub(self.started));
            return Fail(28, (format!("Operation timed out after {after} milliseconds with {} bytes received", info.size_download)).into());
        }
        match sysabi::Errno::from_io(e) {
            Errno::ECONNRESET => Fail(56, ("Recv failure: Connection reset by peer").into()),
            Errno::EPIPE => Fail(55, ("Send failure: Broken pipe").into()),
            _ => Fail(56, (format!("Recv failure: {e}")).into()),
        }
    }

    fn request_text(&self, u: &Url, req: &Request) -> Vec<u8> {
        let o = self.o;
        let host_hdr = match u.port {
            Some(p) if Some(p) != url::default_port(&u.scheme) => format!("{}:{p}", u.host_for_url()),
            _ => u.host_for_url(),
        };
        let mut lines: Vec<(String, String)> = Vec::new();
        lines.push(("Host".into(), host_hdr));
        let user = o.user.clone().or_else(|| u.user.as_ref().map(|n| format!("{n}:{}", u.password.clone().unwrap_or_default())));
        if let Some(up) = user {
            let up = if up.contains(':') { up } else { format!("{up}:") };
            lines.push(("Authorization".into(), format!("Basic {}", base64(up.as_bytes()))));
        }
        lines.push(("User-Agent".into(), o.user_agent.clone().unwrap_or_else(|| format!("curl/{VERSION}"))));
        lines.push(("Accept".into(), if o.json { "application/json".into() } else { "*/*".into() }));
        if let Some(r) = &o.referer {
            lines.push(("Referer".into(), r.clone()));
        }
        if o.compressed {
            lines.push(("Accept-Encoding".into(), "deflate, gzip, br, zstd".into()));
        }
        if req.has_body {
            if let Some(ct) = &req.content_type {
                lines.push(("Content-Type".into(), ct.clone()));
            }
            lines.push(("Content-Length".into(), req.body.len().to_string()));
        }
        // `-H`: substitui o cabeçalho de mesmo nome, `Nome:` remove, `Nome;` manda vazio.
        for h in &o.headers {
            if let Some(name) = h.strip_suffix(';')
                && !name.contains(':')
            {
                lines.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
                lines.push((name.to_string(), String::new()));
                continue;
            }
            let Some((name, value)) = h.split_once(':') else { continue };
            let value = value.trim_start().to_string();
            let replaced = lines.iter().any(|(n, _)| n.eq_ignore_ascii_case(name));
            if replaced {
                if value.is_empty() {
                    lines.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
                } else if let Some(slot) = lines.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                    *slot = (name.to_string(), value);
                }
            } else if !value.is_empty() {
                lines.push((name.to_string(), value));
            }
        }
        let mut out = format!("{} {} HTTP/1.1\r\n", req.method, u.target()).into_bytes();
        for (n, v) in lines {
            out.extend_from_slice(format!("{n}: {v}\r\n").as_bytes());
        }
        out.extend_from_slice(b"\r\n");
        out
    }
}

/// Expande o `-w`.
fn write_out(fmt: &str, info: &Info) -> (Vec<u8>, Vec<u8>) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut to_err = false;
    let b = fmt.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let target = if to_err { &mut err } else { &mut out };
        if b[i] == b'\\' && i + 1 < b.len() {
            match b[i + 1] {
                b'n' => target.push(b'\n'),
                b't' => target.push(b'\t'),
                b'r' => target.push(b'\r'),
                b'\\' => target.push(b'\\'),
                c => {
                    target.push(b'\\');
                    target.push(c);
                }
            }
            i += 2;
            continue;
        }
        if b[i] == b'%' && b.get(i + 1) == Some(&b'%') {
            target.push(b'%');
            i += 2;
            continue;
        }
        if b[i] == b'%' && b.get(i + 1) == Some(&b'{')
            && let Some(end) = fmt[i + 2..].find('}')
        {
            let var = &fmt[i + 2..i + 2 + end];
            i += 3 + end;
            let val = match var {
                "stdout" => {
                    to_err = false;
                    continue;
                }
                "stderr" => {
                    to_err = true;
                    continue;
                }
                "http_code" | "response_code" => format!("{:03}", info.code),
                "content_type" => info.content_type.clone(),
                "size_download" => info.size_download.to_string(),
                "size_header" => info.size_header.to_string(),
                "size_upload" | "size_request" => info.size_upload.to_string(),
                "num_redirects" => info.num_redirects.to_string(),
                "url_effective" => info.url_effective.clone(),
                "url" => info.url.clone(),
                "redirect_url" => info.redirect_url.clone(),
                "method" => info.method.clone(),
                "scheme" => info.scheme.clone(),
                "remote_ip" => info.remote_ip.clone(),
                "remote_port" => info.remote_port.to_string(),
                "local_ip" => if info.remote_ip.is_empty() { String::new() } else { "127.0.0.1".into() },
                "local_port" => info.local_port.to_string(),
                "http_version" => if info.http_version.is_empty() { "0".into() } else { info.http_version.clone() },
                "time_total" => format!("{:.6}", info.time_total),
                "time_connect" | "time_appconnect" | "time_pretransfer" => format!("{:.6}", info.time_connect),
                "time_namelookup" => format!("{:.6}", info.time_connect / 2.0),
                "time_starttransfer" => format!("{:.6}", info.time_starttransfer),
                "time_redirect" => format!("{:.6}", 0.0),
                "speed_download" => format!("{}", if info.time_total > 0.0 { (info.size_download as f64 / info.time_total) as u64 } else { 0 }),
                "speed_upload" => "0".into(),
                "num_connects" => if info.remote_ip.is_empty() { "0".into() } else { "1".into() },
                "filename_effective" => info.filename.clone(),
                "exitcode" => info.exitcode.to_string(),
                "errormsg" => info.errormsg.clone(),
                v if v.starts_with("header{") => {
                    let name = v.trim_start_matches("header{");
                    info.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.as_bytes())).map(|(_, v)| String::from_utf8_lossy(v).into_owned()).unwrap_or_default()
                }
                _ => {
                    // Variável desconhecida: o curl avisa e segue.
                    err.extend_from_slice(format!("curl: unknown --write-out variable: '{var}'\n").as_bytes());
                    continue;
                }
            };
            let target = if to_err { &mut err } else { &mut out };
            target.extend_from_slice(val.as_bytes());
            continue;
        }
        target.push(b[i]);
        i += 1;
    }
    (out, err)
}

/// Expande o glob de URL do curl (`tool_urlglob.c`): `{a,b}`, `[1-10]`, `[01-10:2]`, `[a-z]`.
/// Devolve cada URL com o texto que cada glob produziu (pro `#N` do `-o`). O erro traz a mensagem e a
/// posição (índice em bytes) onde o curl põe o circunflexo.
fn expand_glob(raw: &str) -> Result<Vec<(String, Vec<String>)>, (&'static str, usize)> {
    enum Part {
        Lit(String),
        Set(Vec<String>),
    }
    let b = raw.as_bytes();
    let mut parts: Vec<Part> = Vec::new();
    let mut lit = Vec::new();
    let mut i = 0;
    let flush = |lit: &mut Vec<u8>, parts: &mut Vec<Part>| {
        if !lit.is_empty() {
            parts.push(Part::Lit(String::from_utf8_lossy(lit).into_owned()));
            lit.clear();
        }
    };
    while i < b.len() {
        match b[i] {
            b'\\' if i + 1 < b.len() && matches!(b[i + 1], b'[' | b']' | b'{' | b'}' | b',') => {
                lit.push(b[i + 1]);
                i += 2;
            }
            b'{' => {
                let start = i;
                let mut items = Vec::new();
                let mut cur = Vec::new();
                i += 1;
                loop {
                    match b.get(i) {
                        None => return Err(("unmatched brace", start)),
                        Some(b'}') => {
                            items.push(String::from_utf8_lossy(&cur).into_owned());
                            i += 1;
                            break;
                        }
                        Some(b',') => {
                            items.push(String::from_utf8_lossy(&cur).into_owned());
                            cur.clear();
                            i += 1;
                        }
                        Some(b'{' | b'[') => return Err(("nested brace", i)),
                        Some(b'\\') if i + 1 < b.len() => {
                            cur.push(b[i + 1]);
                            i += 2;
                        }
                        Some(&c) => {
                            cur.push(c);
                            i += 1;
                        }
                    }
                }
                flush(&mut lit, &mut parts);
                parts.push(Part::Set(items));
            }
            b'[' => {
                // Literal IPv6: só hex, `:`, `.` e o `%zona` até o `]`.
                if let Some(end) = raw[i + 1..].find(']') {
                    let inner = &b[i + 1..i + 1 + end];
                    if inner.contains(&b':') && inner.iter().all(|c| c.is_ascii_hexdigit() || matches!(c, b':' | b'.' | b'%') || c.is_ascii_alphanumeric()) {
                        lit.extend_from_slice(&b[i..i + 2 + end]);
                        i += 2 + end;
                        continue;
                    }
                }
                let p = i + 1;
                let items = if b.get(p).is_some_and(u8::is_ascii_alphabetic) {
                    // `[a-z]` ou `[a-z:2]`.
                    let (lo, hi) = (b[p], *b.get(p + 2).unwrap_or(&0));
                    if b.get(p + 1) != Some(&b'-') || !hi.is_ascii_alphabetic() {
                        return Err(("bad range", p));
                    }
                    let mut q = p + 3;
                    let mut step = 1u32;
                    if b.get(q) == Some(&b':') {
                        let s = q + 1;
                        let mut e = s;
                        while b.get(e).is_some_and(u8::is_ascii_digit) {
                            e += 1;
                        }
                        step = raw[s..e].parse().unwrap_or(0);
                        q = e;
                    }
                    if b.get(q) != Some(&b']') || lo > hi || step == 0 || lo.is_ascii_lowercase() != hi.is_ascii_lowercase() {
                        return Err(("bad range", if b.get(q) == Some(&b']') { q + 1 } else { q.min(b.len()) }));
                    }
                    i = q + 1;
                    (lo..=hi).step_by(step as usize).map(|c| (c as char).to_string()).collect::<Vec<_>>()
                } else {
                    // `[1-10]`, `[001-100:5]`.
                    let num = |mut k: usize| {
                        let s = k;
                        while b.get(k).is_some_and(u8::is_ascii_digit) {
                            k += 1;
                        }
                        (raw[s..k].parse::<u64>().ok(), k, k - s)
                    };
                    let (lo, q, width) = num(p);
                    let Some(lo) = lo else { return Err(("bad range", p)) };
                    if b.get(q) != Some(&b'-') {
                        return Err(("bad range", q.min(b.len())));
                    }
                    let (hi, mut q, _) = num(q + 1);
                    let Some(hi) = hi else { return Err(("bad range", q.min(b.len()))) };
                    let mut step = 1u64;
                    if b.get(q) == Some(&b':') {
                        let (s, e, _) = num(q + 1);
                        step = s.unwrap_or(0);
                        q = e;
                    }
                    if b.get(q) != Some(&b']') || lo > hi || step == 0 {
                        return Err(("bad range", if b.get(q) == Some(&b']') { q + 1 } else { q.min(b.len()) }));
                    }
                    i = q + 1;
                    let pad = if b[p] == b'0' && width > 1 { width } else { 0 };
                    (lo..=hi).step_by(step as usize).map(|n| format!("{n:0pad$}")).collect()
                };
                flush(&mut lit, &mut parts);
                parts.push(Part::Set(items));
            }
            b']' | b'}' => return Err(("unmatched close brace/bracket", i)),
            c => {
                lit.push(c);
                i += 1;
            }
        }
    }
    flush(&mut lit, &mut parts);
    // Produto cartesiano, o primeiro glob variando mais devagar.
    let mut out: Vec<(String, Vec<String>)> = vec![(String::new(), Vec::new())];
    for part in parts {
        match part {
            Part::Lit(s) => out.iter_mut().for_each(|(u, _)| u.push_str(&s)),
            Part::Set(items) => {
                out = out
                    .into_iter()
                    .flat_map(|(u, caps)| {
                        items.iter().map(move |it| {
                            let mut c = caps.clone();
                            c.push(it.clone());
                            (format!("{u}{it}"), c)
                        })
                    })
                    .collect();
            }
        }
    }
    Ok(out)
}

/// `#1`, `#2`... do `-o` trocados pelo texto de cada glob.
fn fill_output(tpl: &str, caps: &[String]) -> String {
    let b = tpl.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'#' && b.get(i + 1).is_some_and(u8::is_ascii_digit) {
            let mut e = i + 1;
            while b.get(e).is_some_and(u8::is_ascii_digit) {
                e += 1;
            }
            let n: usize = tpl[i + 1..e].parse().unwrap_or(0);
            match caps.get(n.wrapping_sub(1)) {
                Some(c) => out.push_str(c),
                None => out.push_str(&tpl[i..e]),
            }
            i = e;
            continue;
        }
        let ch = tpl[i..].chars().next().unwrap_or('#');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

pub fn main(ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let o = match parse(ctx, argv) {
        Ok(o) => o,
        Err(Fail(0, _)) => return 0,
        Err(Fail(code, msg)) => {
            let _ = ctx.stderr().write_all(format!("curl: {msg}\ncurl: try 'curl --help' or 'curl --manual' for more information\n").as_bytes());
            return code;
        }
    };
    if o.urls.is_empty() {
        let _ = ctx.stderr().write_all(b"curl: try 'curl --help' or 'curl --manual' for more information\n");
        return 2;
    }
    let show_err = !o.silent || o.show_error;
    let stdout_tty = ctx.sys().isatty(Fd::STDOUT);
    let mut rc = 0;
    // Cada URL do usuário vira uma ou mais transferências pelo glob; o `-o` de ordem `n` vale para
    // todas as da `n`-ésima URL, com `#N` trocado.
    let mut jobs: Vec<(String, Option<Output>)> = Vec::new();
    for (n, raw) in o.urls.iter().enumerate() {
        let out = o.outputs.get(n).cloned();
        if o.globoff {
            jobs.push((raw.clone(), out));
            continue;
        }
        match expand_glob(raw) {
            Ok(list) => {
                for (u, caps) in list {
                    let out = match &out {
                        Some(Output::File(p)) => Some(Output::File(fill_output(p, &caps))),
                        other => other.clone(),
                    };
                    jobs.push((u, out));
                }
            }
            Err((msg, pos)) => {
                if show_err {
                    let caret = " ".repeat(pos);
                    let _ = ctx.stderr().write_all(format!("curl: (3) {msg} in URL position {}:\n{raw}\n{caret}^\n", pos + 1).as_bytes());
                }
                rc = 3;
            }
        }
    }
    for (raw, out) in &jobs {
        let raw = raw.as_str();
        let out = out.clone();
        let mut sink = match &out {
            None => Sink::Stdout,
            Some(Output::File(p)) if p == "-" => Sink::Stdout,
            Some(Output::File(p)) if p == "/dev/null" => Sink::Null,
            Some(Output::File(p)) => Sink::File { path: p.clone(), fd: None },
            Some(Output::Remote) => {
                let name = url::parse(raw, o.path_as_is).ok().map(|u| remote_name(&u));
                match name {
                    Some(Ok(name)) => Sink::File { path: name, fd: None },
                    Some(Err(Fail(code, msg))) => {
                        if show_err {
                            let _ = ctx.stderr().write_all(format!("curl: ({code}) {msg}\n").as_bytes());
                        }
                        rc = code;
                        continue;
                    }
                    None => Sink::Stdout,
                }
            }
        };
        if let Sink::File { path, .. } = &sink
            && o.create_dirs
        {
            mkdirs(path);
        }
        let filename = match &sink {
            Sink::File { path, .. } => path.clone(),
            _ => String::new(),
        };
        let meter_on = !o.silent && !o.verbose && (!stdout_tty || !matches!(sink, Sink::Stdout));
        let started = io::now();
        let mut t = Transfer { ctx, o: &o, meter: Meter { on: meter_on, started: false }, started };
        let result = t.run(raw, &mut sink);
        let elapsed = t.elapsed();
        let (mut info, err) = match result {
            Ok(info) => {
                let total = Some(info.size_download).filter(|_| true);
                t.meter.finish(t.ctx, total, info.size_download, info.size_upload, elapsed);
                (info, None)
            }
            Err((info, f)) => {
                if t.meter.started && f.0 != 22 {
                    t.meter.finish(t.ctx, None, info.size_download, info.size_upload, elapsed);
                }
                (info, Some(f))
            }
        };
        sink.close();
        info.time_total = elapsed;
        info.filename = filename;
        if let Some(Fail(code, msg)) = &err {
            info.exitcode = *code;
            info.errormsg = msg.clone();
            if show_err {
                let _ = ctx.stderr().write_all(format!("curl: ({code}) {msg}\n").as_bytes());
            }
            rc = *code;
        }
        if let Some(fmt) = &o.write_out {
            let fmt = match fmt.strip_prefix('@') {
                Some(path) => String::from_utf8_lossy(&read_source(ctx, path).unwrap_or_default()).into_owned(),
                None => fmt.clone(),
            };
            let (out, errtext) = write_out(&fmt, &info);
            let _ = ctx.stdout().write_all(&out);
            let _ = ctx.stderr().write_all(&errtext);
        }
    }
    rc
}
