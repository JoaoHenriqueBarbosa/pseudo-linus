//! `osh`: o shell do pseudo-linus.
//!
//! - `osh -c 'cmd'`, `osh script.sh [args]`, `osh` interativo.
//! - Sem `--remote`: a sandbox roda neste processo (kernel do pseudo-linus em memória), sem daemon.
//! - Com `--remote URL`: fala com o `pseudo-linusd`; a chave vem de `--key`, `--key-file` ou
//!   `OSH_KEY` (prefira o arquivo ou a variável: argumento de linha de comando aparece no `ps`).
//!
//! O interativo é linha a linha (sem edição avançada): cada linha vai pra uma sessão persistente, então
//! `cd`, `export` e variáveis valem entre linhas; linha terminada em `\` continua na próxima.

use std::collections::BTreeMap;
use std::io::{BufRead, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use host::api::ExecResult;
use host::backend::Sandbox;
use host::client::{Client, ClientError};
use host::config::IsolationConfig;
use host::exec::{Cancel, ExecLimits, OutputSink, StdinFeed, Stream};
use host::session::{Session, ShellState};
use serde_json::{Value, json};

#[global_allocator]
static GLOBAL: tracking_allocator::Allocator<mimalloc::MiMalloc> =
    tracking_allocator::Allocator::from_allocator(mimalloc::MiMalloc);

#[derive(Parser)]
#[command(name = "osh", version, about = "Shell do pseudo-linus (local ou num pseudo-linusd remoto)")]
struct Cli {
    /// Roda o comando e sai com o status dele.
    #[arg(short = 'c', value_name = "COMANDO")]
    command: Option<String>,
    /// URL do pseudo-linusd (ex.: https://pl.exemplo.com).
    #[arg(long, env = "OSH_REMOTE")]
    remote: Option<String>,
    /// Chave de API (prefira --key-file ou OSH_KEY).
    #[arg(long, env = "OSH_KEY", hide_env_values = true)]
    key: Option<String>,
    /// Arquivo com a chave de API.
    #[arg(long)]
    key_file: Option<PathBuf>,
    /// Usa uma sandbox que já existe no daemon em vez de criar outra.
    #[arg(long)]
    sandbox: Option<String>,
    /// Não destrói a sandbox criada ao sair (imprime o id no stderr).
    #[arg(long)]
    keep: bool,
    /// Timeout de cada comando (ex.: 30s, 10m).
    #[arg(long, default_value = "10m")]
    timeout: String,
    /// Diretório de trabalho inicial.
    #[arg(long)]
    workdir: Option<String>,
    /// Interno (testes): backend local `kernel` ou `fake`.
    #[arg(long, hide = true, default_value = "kernel")]
    backend: String,
    /// Script e argumentos.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    script: Vec<String>,
}

/// O que o osh precisa de uma sandbox, local ou remota.
trait Target {
    /// Roda um comando avulso; a saída sai direto no terminal. Devolve o `$?`.
    fn run(&mut self, command: Option<&str>, argv: Option<&[String]>, stdin: StdinFeed) -> Result<ExecResult, String>;
    /// Roda uma linha na sessão persistente.
    fn session(&mut self, line: &str) -> Result<ExecResult, String>;
    fn close(&mut self);
}

fn print_chunk(stream: &str, data: &[u8]) {
    if stream == "stderr" {
        let mut e = std::io::stderr().lock();
        let _ = e.write_all(data);
        let _ = e.flush();
    } else {
        let mut o = std::io::stdout().lock();
        let _ = o.write_all(data);
        let _ = o.flush();
    }
}

struct Remote {
    client: Client,
    url: String,
    key: String,
    sandbox: String,
    created: bool,
    keep: bool,
    session: Option<String>,
    timeout_ms: u64,
    /// Diretório inicial da sessão (`--workdir` com `--sandbox`; na criação a sandbox já nasce nele).
    cwd: Option<String>,
}

fn client_err(e: ClientError) -> String {
    e.to_string()
}

/// Manda o stdin do osh ao exec remoto em pedaços, até o EOF (ou até o exec acabar).
fn pump_stdin(client: Client, sandbox: String, id: String, mut src: Box<dyn Read + Send>) {
    let mut buf = vec![0u8; 64 << 10];
    loop {
        let n = match src.read(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => 0,
        };
        let p = json!({ "sandbox_id": sandbox, "stdin_id": id, "data_base64": base64::Engine::encode(&host::api::b64::STANDARD, &buf[..n]), "eof": n == 0 });
        if client.call("exec.stdin", p).is_err() || n == 0 {
            return;
        }
    }
}

impl Remote {
    fn result(v: Value) -> Result<ExecResult, String> {
        serde_json::from_value(v).map_err(|e| format!("resposta inesperada do daemon: {e}"))
    }
}

impl Target for Remote {
    fn run(&mut self, command: Option<&str>, argv: Option<&[String]>, stdin: StdinFeed) -> Result<ExecResult, String> {
        let mut p = json!({ "sandbox_id": self.sandbox, "timeout_ms": self.timeout_ms });
        match stdin {
            StdinFeed::Bytes(v) => p["stdin_base64"] = json!(base64::Engine::encode(&host::api::b64::STANDARD, &v)),
            // Stdin em fluxo: uma thread manda os pedaços por `exec.stdin` (noutra conexão) à medida que
            // chegam; o comando não espera o EOF. A thread fica solta: pode estar presa num `read`.
            StdinFeed::Stream(r) => {
                let id = host::ids::random_id("in");
                p["stdin_id"] = json!(id);
                let client = Client::new(&self.url, &self.key);
                let sandbox = self.sandbox.clone();
                let _ = std::thread::Builder::new().name("osh-stdin".into()).spawn(move || pump_stdin(client, sandbox, id, r));
            }
        }
        if let Some(cwd) = &self.cwd {
            p["cwd"] = json!(cwd);
        }
        match (command, argv) {
            (Some(c), _) => p["command"] = json!(c),
            (None, Some(a)) => p["argv"] = json!(a),
            (None, None) => return Err("nada pra rodar".into()),
        }
        let v = self.client.call_stream("exec.stream", p, |s, d| print_chunk(s, d.as_bytes())).map_err(client_err)?;
        Remote::result(v)
    }

    fn session(&mut self, line: &str) -> Result<ExecResult, String> {
        if self.session.is_none() {
            let mut p = json!({ "sandbox_id": self.sandbox });
            if let Some(cwd) = &self.cwd {
                p["cwd"] = json!(cwd);
            }
            let v = self.client.call("session.open", p).map_err(client_err)?;
            self.session = v["session_id"].as_str().map(str::to_string);
        }
        let p = json!({ "session_id": self.session, "command": line, "timeout_ms": self.timeout_ms });
        let v = self.client.call_stream("session.exec.stream", p, |s, d| print_chunk(s, d.as_bytes())).map_err(client_err)?;
        let r = Remote::result(v)?;
        if r.session_closed {
            self.session = None;
        }
        Ok(r)
    }

    fn close(&mut self) {
        if let Some(s) = self.session.take() {
            let _ = self.client.call("session.close", json!({ "session_id": s }));
        }
        if self.created {
            if self.keep {
                eprintln!("osh: sandbox mantida: {}", self.sandbox);
            } else if let Err(e) = self.client.call("sandbox.destroy", json!({ "sandbox_id": self.sandbox })) {
                eprintln!("osh: não deu pra destruir a sandbox {}: {e}", self.sandbox);
            }
        }
    }
}

struct TermSink;

impl OutputSink for TermSink {
    fn output(&self, stream: Stream, data: &[u8]) {
        print_chunk(if stream == Stream::Stderr { "stderr" } else { "stdout" }, data);
    }
}

struct Local {
    sb: Arc<dyn Sandbox>,
    session: Option<Session>,
    limits: ExecLimits,
    env: Vec<String>,
    workdir: String,
}

fn outcome_to_result(o: &host::exec::ExecOutcome) -> ExecResult {
    ExecResult {
        exit_code: o.exit_code,
        signal: o.signal,
        status: o.status(),
        timed_out: o.timed_out,
        cancelled: o.cancelled,
        duration_ms: o.duration_ms,
        streamed: true,
        ..Default::default()
    }
}

impl Target for Local {
    fn run(&mut self, command: Option<&str>, argv: Option<&[String]>, stdin: StdinFeed) -> Result<ExecResult, String> {
        let req = host::worker::spawn_request(&*self.sb, command, argv, &self.workdir, &self.env).map_err(|e| e.message)?;
        let o = host::exec::run_feed(&*self.sb, req, stdin, self.limits, Some(&TermSink), &Cancel::default()).map_err(|e| e.to_string())?;
        Ok(outcome_to_result(&o))
    }

    fn session(&mut self, line: &str) -> Result<ExecResult, String> {
        if self.session.is_none() {
            let state = ShellState {
                cwd: self.workdir.clone().into_bytes(),
                env: self.env.iter().map(|e| e.clone().into_bytes()).collect(),
                dump: Vec::new(),
            };
            let id = host::ids::random_id("ss");
            self.session = Some(Session::open(self.sb.clone(), &id, state, self.workdir.as_bytes()).map_err(|e| e.to_string())?);
        }
        let s = self.session.as_ref().expect("aberta acima");
        let o = s.exec(line, b"", self.limits, Some(&TermSink), &Cancel::default()).map_err(|e| e.to_string())?;
        let mut r = outcome_to_result(&o.exec);
        r.cwd = o.cwd;
        r.session_reset = o.reset;
        r.session_closed = o.closed;
        if o.closed {
            self.session = None;
        }
        Ok(r)
    }

    fn close(&mut self) {
        if let Some(s) = self.session.take() {
            s.close();
        }
        self.sb.destroy();
    }
}

fn read_key(cli: &Cli) -> Result<String, String> {
    if let Some(k) = &cli.key {
        return Ok(k.trim().to_string());
    }
    if let Some(p) = &cli.key_file {
        return std::fs::read_to_string(p).map(|s| s.trim().to_string()).map_err(|e| format!("{}: {e}", p.display()));
    }
    Err("faltou a chave: use --key-file, OSH_KEY ou --key".into())
}

fn connect(cli: &Cli, url: &str, timeout_ms: u64) -> Result<Box<dyn Target>, String> {
    let key = read_key(cli)?;
    let client = Client::new(url, &key);
    let (sandbox, created) = match &cli.sandbox {
        Some(s) => (s.clone(), false),
        None => {
            let mut p = json!({ "labels": { "client": "osh" } });
            if let Some(w) = &cli.workdir {
                p["workdir"] = json!(w);
            }
            let v = client.call("sandbox.create", p).map_err(client_err)?;
            (v["sandbox_id"].as_str().unwrap_or_default().to_string(), true)
        }
    };
    Ok(Box::new(Remote { client, url: url.to_string(), key, sandbox, created, keep: cli.keep, session: None, timeout_ms, cwd: cli.workdir.clone() }))
}

fn local(cli: &Cli, timeout: Duration) -> Result<Box<dyn Target>, String> {
    let mirror_dir = std::env::var_os("PL_PYPI_MIRROR").map(std::path::PathBuf::from);
    let isolation = IsolationConfig::default();
    let opts = host::worker::KernelOptions { isolation: &isolation, cpus: 2, pypi_mirror: mirror_dir.as_deref() };
    let backend = host::worker::make_backend(&cli.backend, &opts)
        .map_err(|e| format!("modo local indisponível: {e}; use --remote"))?;
    let limits = host::api::SandboxLimits {
        mem_bytes: 1 << 30,
        max_procs: 512,
        fs_bytes: 1 << 30,
        nofile: 1024,
        cpu_weight: 100,
        cpu_max: None,
    };
    let user = host::backend::UserSched { user: "local".into(), cpu_weight: 100, cpu_max: None };
    backend.ensure_user(&user).map_err(|e| e.to_string())?;
    let spec = host::backend::SandboxSpec { image: "default".into(), hostname: host::ids::random_hostname(), limits };
    let sb = backend.create_sandbox(&host::ids::random_id("sb"), "local", &spec).map_err(|e| e.to_string())?;
    let mut env: BTreeMap<String, String> =
        host::methods::DEFAULT_ENV.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    if let Ok(term) = std::env::var("TERM") {
        env.insert("TERM".into(), term);
    }
    Ok(Box::new(Local {
        sb,
        session: None,
        limits: ExecLimits {
            timeout,
            output_limit: u64::MAX,
            max_discard: 0,
            drain_grace: Duration::from_millis(100),
        },
        env: env.into_iter().map(|(k, v)| format!("{k}={v}")).collect(),
        workdir: cli.workdir.clone().unwrap_or_else(|| "/root".into()),
    }))
}

/// O stdin do osh vira o fd 0 do comando, lido sob demanda: um pipe aberto e sem dados não segura o
/// comando (como no `bash -c`), e um terminal fica de fora.
fn stdin_if_piped() -> StdinFeed {
    if std::io::stdin().is_terminal() {
        StdinFeed::Bytes(Vec::new())
    } else {
        StdinFeed::Stream(Box::new(std::io::stdin()))
    }
}

fn report(r: &ExecResult) -> u8 {
    if r.timed_out {
        eprintln!("osh: tempo esgotado; o comando foi morto");
    }
    if r.stdout_truncated || r.stderr_truncated {
        eprintln!("osh: saída truncada pelo limite do daemon");
    }
    if r.session_reset {
        eprintln!("osh: a sessão foi reiniciada (cwd e variáveis exportadas mantidos)");
    }
    (r.status & 0xff) as u8
}

/// A última linha termina em `\` sem escape (número ímpar de barras), que junta a linha seguinte.
fn ends_with_line_continuation(buf: &str) -> bool {
    let body = buf.strip_suffix('\n').unwrap_or(buf);
    let last = body.rsplit('\n').next().unwrap_or("");
    last.len() - last.trim_end_matches('\\').len() & 1 == 1
}

fn interactive(t: &mut dyn Target) -> u8 {
    let stdin = std::io::stdin();
    let tty = stdin.is_terminal();
    let mut last: u8 = 0;
    let mut cwd = String::from("~");
    let mut buf = String::new();
    // Em terminal, o rustyline cuida da edição da linha (setas, Home/End, histórico, Ctrl-R, Ctrl-A/E...).
    let mut editor = if tty { rustyline::DefaultEditor::new().ok() } else { None };
    loop {
        let mut line = String::new();
        if let Some(ed) = editor.as_mut() {
            let prompt = if buf.is_empty() { format!("osh:{cwd}# ") } else { "> ".to_string() };
            match ed.readline(&prompt) {
                Ok(l) => {
                    line = l;
                    line.push('\n');
                }
                Err(rustyline::error::ReadlineError::Interrupted) => {
                    buf.clear();
                    continue;
                }
                Err(rustyline::error::ReadlineError::Eof) => break,
                Err(e) => {
                    eprintln!("osh: {e}");
                    break;
                }
            }
        } else {
            if tty {
                if buf.is_empty() {
                    eprint!("osh:{cwd}# ");
                } else {
                    eprint!("> ");
                }
                let _ = std::io::stderr().flush();
            }
            match stdin.lock().read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    eprintln!("osh: {e}");
                    break;
                }
            }
        }
        let trimmed = line.trim_end_matches(['\n', '\r']);
        buf.push_str(trimmed);
        buf.push('\n');
        // Comando incompleto (here-document, aspas, if/for/while abertos, `\` ou `|` no fim): pede mais linhas.
        if ends_with_line_continuation(&buf) || shell_parser::entry::needs_more_input(&buf) {
            continue;
        }
        let cmd = std::mem::take(&mut buf);
        if cmd.trim().is_empty() {
            continue;
        }
        if let Some(ed) = editor.as_mut() {
            let _ = ed.add_history_entry(cmd.trim_end());
        }
        match t.session(&cmd) {
            Ok(r) => {
                last = report(&r);
                if let Some(c) = r.cwd {
                    cwd = c;
                }
                if r.session_closed {
                    return last;
                }
            }
            Err(e) => {
                eprintln!("osh: {e}");
                last = 1;
            }
        }
    }
    last
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let timeout = match host::timeutil::parse_duration(&cli.timeout) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("osh: --timeout: {e}");
            return ExitCode::from(2);
        }
    };
    let target = match &cli.remote {
        Some(url) => connect(&cli, url, timeout.as_millis() as u64),
        None => local(&cli, timeout),
    };
    let mut t = match target {
        Ok(t) => t,
        Err(e) => {
            eprintln!("osh: {e}");
            return ExitCode::from(2);
        }
    };
    let code = if let Some(cmd) = &cli.command {
        match t.run(Some(cmd), None, stdin_if_piped()) {
            Ok(r) => report(&r),
            Err(e) => {
                eprintln!("osh: {e}");
                1
            }
        }
    } else if let Some(script) = cli.script.first() {
        match std::fs::read(script) {
            Ok(data) => {
                // `bash -c` com o texto do script, sem arquivo no sandbox: um `ls -a /tmp` do script
                // não pode ver nada que a bancada pôs lá.
                let mut argv = vec!["bash".to_string(), "-c".to_string(), String::from_utf8_lossy(&data).into_owned(), "bash".to_string()];
                argv.extend(cli.script[1..].iter().cloned());
                match t.run(None, Some(&argv), stdin_if_piped()) {
                    Ok(r) => report(&r),
                    Err(e) => {
                        eprintln!("osh: {e}");
                        1
                    }
                }
            }
            Err(e) => {
                eprintln!("osh: {script}: {}", host::config::io_msg(&e));
                127
            }
        }
    } else {
        interactive(&mut *t)
    };
    t.close();
    ExitCode::from(code)
}
