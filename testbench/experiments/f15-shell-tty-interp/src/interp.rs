//! H39: Python, JS e Lua embutíveis com todo I/O passando por nós.
//!
//! Seis engines, cada um num binário próprio (`interp/<engine>`, protocolo em `interp/common`):
//! Monty e RustPython (Python), boa e rquickjs (JS), piccolo e mlua (Lua). Pra cada um:
//!
//! 1. **compila com `forbid(unsafe_code)`**: todo crate do workspace tem o lint; se o binário existe,
//!    compilou;
//! 2. **corpus de one-liners** (`data/interp-corpus.toml`): saída igual à esperada, com o esperado de
//!    Python e JS confirmado no python3/node do host;
//! 3. **negação de I/O de host**: sondas que tentam abrir arquivo do host, listar diretório, ler o
//!    ambiente, criar processo e abrir socket; e o binário inteiro roda sob `strace`, contando toda
//!    syscall de arquivo, rede e processo entre os marcadores de início e fim dos trechos;
//! 4. **custo**: tamanho do binário (release com strip), startup em processo (criar contexto e rodar
//!    um one-liner, mediana) e tempo de processo inteiro (spawn até sair);
//! 5. **depscan** do crate do engine: categoria (a, b, c), unsafe e dependências com C.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use harness::{CandidateResult, Fit, Verdict};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::common::{Section, bin_path, build_packages, cache_dir, exp_dir, manifest, median};

/// Marcadores do protocolo comum (`interp/common`): `stat` em caminho que não existe.
const MARK_BEGIN: &str = "/f15-strace-marker-begin";
const MARK_END: &str = "/f15-strace-marker-end";

/// Iterações da medição de startup em processo.
const STARTUP_ITERATIONS: usize = 40;
/// Repetições da medição de processo inteiro.
const PROCESS_RUNS: usize = 5;
/// Teto de tempo de um processo de engine (o corpus inteiro de uma linguagem cabe com folga).
const ENGINE_TIMEOUT: Duration = Duration::from_secs(180);

struct EngineSpec {
    /// Nome curto (diretório em `interp/`).
    name: &'static str,
    /// Crate do engine (pro depscan).
    krate: &'static str,
    version: &'static str,
    lang: &'static str,
    /// Linguagem/implementação, pra nota.
    what: &'static str,
}

const ENGINES: &[EngineSpec] = &[
    EngineSpec { name: "monty", krate: "monty", version: "1.0.0", lang: "python", what: "subconjunto de Python da pydantic, VM própria em Rust" },
    EngineSpec { name: "rustpython", krate: "rustpython-vm", version: "0.6.0", lang: "python", what: "Python 3 em Rust, sem host_env, stdlib congelada" },
    EngineSpec { name: "boa", krate: "boa_engine", version: "0.22.0", lang: "js", what: "JS em Rust puro" },
    EngineSpec { name: "rquickjs", krate: "rquickjs", version: "0.14.0", lang: "js", what: "QuickJS (C) com binding Rust" },
    EngineSpec { name: "piccolo", krate: "piccolo", version: "0.3.3", lang: "lua", what: "VM Lua stackless em Rust puro" },
    EngineSpec { name: "mlua", krate: "mlua", version: "0.12.1", lang: "lua", what: "Lua 5.4 de referência (C, vendored) com binding Rust" },
];

fn package_of(e: &EngineSpec) -> String {
    format!("f15-interp-{}", e.name)
}

fn startup_code(lang: &str) -> &'static str {
    match lang {
        "js" => "console.log(1 + 1)",
        _ => "print(1 + 1)",
    }
}

// ------------------------------------------------------------------------------------------------
// Corpus
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize)]
pub struct CorpusFile {
    pub snippet: Vec<CorpusSnippet>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CorpusSnippet {
    pub id: String,
    pub lang: String,
    /// "corpus" ou "deny".
    pub kind: String,
    pub category: String,
    pub code: String,
    pub expected: String,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub expected_files: BTreeMap<String, String>,
}

impl CorpusSnippet {
    fn is_deny(&self) -> bool {
        self.kind == "deny"
    }
}

pub fn corpus_path() -> PathBuf {
    exp_dir().join("data").join("interp-corpus.toml")
}

pub fn load_corpus() -> Result<Vec<CorpusSnippet>> {
    let text = std::fs::read_to_string(corpus_path()).context("ler data/interp-corpus.toml")?;
    let file: CorpusFile = toml::from_str(&text).context("parse de data/interp-corpus.toml")?;
    let mut seen = BTreeSet::new();
    for s in &file.snippet {
        if !seen.insert(s.id.clone()) {
            bail!("id repetido no corpus: {}", s.id);
        }
        if !matches!(s.lang.as_str(), "python" | "js" | "lua") {
            bail!("{}: linguagem desconhecida {}", s.id, s.lang);
        }
        if !matches!(s.kind.as_str(), "corpus" | "deny") {
            bail!("{}: kind desconhecido {}", s.id, s.kind);
        }
    }
    Ok(file.snippet)
}

// ------------------------------------------------------------------------------------------------
// Protocolo (espelho de interp/common, que o crate principal não importa)
// ------------------------------------------------------------------------------------------------

#[derive(Serialize)]
struct Request<'a> {
    snippets: Vec<WireSnippet<'a>>,
    startup_iterations: usize,
    startup_code: &'a str,
}

#[derive(Serialize)]
struct WireSnippet<'a> {
    id: &'a str,
    code: &'a str,
    files: &'a BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct SnippetResult {
    id: String,
    stdout: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    files: BTreeMap<String, String>,
    elapsed_us: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct Startup {
    iterations: usize,
    min_us: f64,
    median_us: f64,
    p90_us: f64,
    output: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct Response {
    startup: Startup,
    results: Vec<SnippetResult>,
    #[serde(default)]
    notes: Vec<String>,
}

fn request_json(snippets: &[&CorpusSnippet], startup_iterations: usize, lang: &str) -> String {
    let req = Request {
        snippets: snippets
            .iter()
            .map(|s| WireSnippet { id: &s.id, code: &s.code, files: &s.files })
            .collect(),
        startup_iterations,
        startup_code: startup_code(lang),
    };
    serde_json::to_string(&req).expect("pedido serializável")
}

/// Roda um comando com stdin dado, timeout e saída capturada.
fn run_with_stdin(cmd: &mut Command, input: &str, timeout: Duration) -> Result<(std::process::ExitStatus, String, String, Duration)> {
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let start = Instant::now();
    let mut child = cmd.spawn().with_context(|| format!("spawn {cmd:?}"))?;
    let mut stdin = child.stdin.take().expect("stdin");
    let input = input.to_string();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let mut out_pipe = child.stdout.take().expect("stdout");
    let mut err_pipe = child.stderr.take().expect("stderr");
    let out_reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out_pipe.read_to_string(&mut s);
        s
    });
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err_pipe.read_to_string(&mut s);
        s
    });
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("timeout de {}s", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let elapsed = start.elapsed();
    let _ = writer.join();
    let out = out_reader.join().unwrap_or_default();
    let err = err_reader.join().unwrap_or_default();
    Ok((status, out, err, elapsed))
}

fn call_engine(bin: &Path, input: &str) -> Result<(Response, Duration)> {
    let (status, out, err, elapsed) = run_with_stdin(&mut Command::new(bin), input, ENGINE_TIMEOUT)?;
    if !status.success() {
        bail!("{} saiu com {status}: {}", bin.display(), err.chars().take(2000).collect::<String>());
    }
    let resp: Response = serde_json::from_str(out.trim()).with_context(|| format!("resposta de {}", bin.display()))?;
    Ok((resp, elapsed))
}

// ------------------------------------------------------------------------------------------------
// Conferência do esperado no python3/node do host
// ------------------------------------------------------------------------------------------------

#[derive(Default, Serialize)]
struct HostCheck {
    tool: String,
    available: bool,
    checked: usize,
    confirmed: usize,
    mismatches: Vec<String>,
}

fn host_check(corpus: &[CorpusSnippet], lang: &str, tool: &str) -> HostCheck {
    let mut hc = HostCheck { tool: tool.into(), ..HostCheck::default() };
    let version = Command::new(tool).arg("--version").output();
    hc.available = version.map(|o| o.status.success()).unwrap_or(false);
    if !hc.available {
        return hc;
    }
    let base = cache_dir().join("interp-host");
    for s in corpus.iter().filter(|s| s.lang == lang && !s.is_deny()) {
        let dir = base.join(&s.id);
        let _ = std::fs::remove_dir_all(&dir);
        if std::fs::create_dir_all(&dir).is_err() {
            continue;
        }
        for (p, c) in &s.files {
            let _ = std::fs::write(dir.join(p), c);
        }
        let flag = if lang == "js" { "-e" } else { "-c" };
        let mut cmd = Command::new(tool);
        cmd.arg(flag).arg(&s.code).current_dir(&dir);
        hc.checked += 1;
        match run_with_stdin(&mut cmd, "", Duration::from_secs(30)) {
            Ok((st, out, err, _)) => {
                let files_ok = s
                    .expected_files
                    .iter()
                    .all(|(p, c)| std::fs::read_to_string(dir.join(p)).map(|x| &x == c).unwrap_or(false));
                if st.success() && out == s.expected && files_ok {
                    hc.confirmed += 1;
                } else {
                    hc.mismatches.push(format!(
                        "{}: host deu {:?} (stderr {:?}, arquivos ok: {files_ok}), esperado {:?}",
                        s.id,
                        out,
                        err.chars().take(200).collect::<String>(),
                        s.expected
                    ));
                }
            }
            Err(e) => hc.mismatches.push(format!("{}: {e}", s.id)),
        }
    }
    hc
}

// ------------------------------------------------------------------------------------------------
// strace
// ------------------------------------------------------------------------------------------------

/// Prefixos de caminho que são do runtime do processo (não dos trechos) e ficam fora da contagem,
/// mas registrados: as leituras de cgroup e de `/sys/devices/system/cpu` são do
/// `std::thread::available_parallelism` (std/src/sys/thread/unix.rs, módulo `cgroups`), e
/// `/proc/self/maps` é a guarda de pilha de thread nova do std. Nenhum desses depende do trecho.
/// `getcwd` NÃO é ruído: no boa ele vinha do `realpath(".")` do loader de módulos padrão, e no
/// RustPython do cálculo de `sys.path` (getpath.rs); conta como toque.
const RUNTIME_NOISE: &[&str] = &["/proc/self/maps", "/sys/devices/system/cpu", "/proc/self/cgroup", "/sys/fs/cgroup"];

/// Syscalls que só devolvem informação do processo e ficam fora da contagem (nenhuma por enquanto;
/// a lista existe pra que a decisão fique explícita e testada).
const RUNTIME_NOISE_SYSCALLS: &[&str] = &[];

const NET_SYSCALLS: &[&str] = &[
    "socket", "connect", "bind", "listen", "accept", "accept4", "sendto", "recvfrom", "sendmsg", "recvmsg",
    "getsockopt", "setsockopt", "getsockname", "getpeername", "socketpair", "shutdown",
];
const SPAWN_SYSCALLS: &[&str] = &["execve", "execveat", "fork", "vfork"];

#[derive(Default, Debug, Serialize)]
pub struct StraceReport {
    pub available: bool,
    pub markers_found: bool,
    /// Syscalls de arquivo, rede ou criação de processo entre os marcadores (fora o ruído do runtime).
    pub host_touches: usize,
    /// Amostra das linhas que contaram.
    pub touches: Vec<String>,
    /// Toques por syscall.
    pub by_syscall: BTreeMap<String, usize>,
    /// Caminhos distintos tocados (até 40), com contagem.
    pub paths: BTreeMap<String, usize>,
    /// Ruído de runtime ignorado (caminho -> vezes).
    pub ignored: BTreeMap<String, usize>,
    /// Threads criadas entre os marcadores (clone com CLONE_THREAD): não é I/O de host, mas é registrado.
    pub threads_spawned: usize,
    pub lines_between_markers: usize,
}

fn syscall_name(line: &str) -> Option<&str> {
    // "1234  openat(AT_FDCWD, ...) = 3", "1234  <... openat resumed>...", ou sem pid.
    let rest = line.trim_start();
    let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start();
    if let Some(r) = rest.strip_prefix("<... ") {
        return r.split_whitespace().next();
    }
    let end = rest.find('(')?;
    let name = &rest[..end];
    (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(name)
}

fn first_quoted(line: &str) -> Option<&str> {
    let start = line.find('"')? + 1;
    let len = line[start..].find('"')?;
    Some(&line[start..start + len])
}

pub fn parse_strace(text: &str) -> StraceReport {
    let mut rep = StraceReport { available: true, ..StraceReport::default() };
    let lines: Vec<&str> = text.lines().collect();
    let begin = lines.iter().position(|l| l.contains(MARK_BEGIN));
    let end = lines.iter().rposition(|l| l.contains(MARK_END));
    let (Some(b), Some(e)) = (begin, end) else {
        return rep;
    };
    rep.markers_found = true;
    for line in &lines[b + 1..e] {
        if line.contains(MARK_BEGIN) || line.contains(MARK_END) || line.contains("+++ exited") || line.contains("--- SIG") {
            continue;
        }
        rep.lines_between_markers += 1;
        let Some(name) = syscall_name(line) else { continue };
        if matches!(name, "exit" | "exit_group" | "wait4" | "waitid") {
            continue;
        }
        if matches!(name, "clone" | "clone3") {
            if line.contains("CLONE_THREAD") || line.contains("resumed") {
                rep.threads_spawned += line.contains("CLONE_THREAD") as usize;
                continue;
            }
        }
        if RUNTIME_NOISE_SYSCALLS.contains(&name) {
            *rep.ignored.entry(format!("{name}()")).or_default() += 1;
            continue;
        }
        let is_net = NET_SYSCALLS.contains(&name);
        let is_spawn = SPAWN_SYSCALLS.contains(&name) || matches!(name, "clone" | "clone3");
        // Caminho vazio (statx/fstatat com AT_EMPTY_PATH) é operação num fd já aberto, não acesso novo.
        let path = first_quoted(line).filter(|p| !p.is_empty());
        if let Some(p) = path
            && RUNTIME_NOISE.iter().any(|n| p.starts_with(n))
        {
            *rep.ignored.entry(p.to_string()).or_default() += 1;
            continue;
        }
        // Syscall de arquivo sem caminho (close, fstat de fd) não é acesso novo ao host.
        if !is_net && !is_spawn && path.is_none() {
            continue;
        }
        rep.host_touches += 1;
        *rep.by_syscall.entry(name.to_string()).or_default() += 1;
        if let Some(p) = path
            && (rep.paths.len() < 40 || rep.paths.contains_key(p))
        {
            *rep.paths.entry(p.to_string()).or_default() += 1;
        }
        if rep.touches.len() < 20 {
            rep.touches.push(line.trim().chars().take(200).collect());
        }
    }
    rep
}

/// Roda o binário (com `args`) sob strace e devolve o relatório e a resposta do protocolo.
fn strace_engine(bin: &Path, args: &[&str], input: &str, name: &str) -> (StraceReport, Option<Response>) {
    let available = Command::new("strace").arg("-V").output().map(|o| o.status.success()).unwrap_or(false);
    if !available {
        return (StraceReport::default(), None);
    }
    let trace = cache_dir().join(format!("strace-interp-{name}.txt"));
    let _ = std::fs::remove_file(&trace);
    let mut cmd = Command::new("strace");
    cmd.args(["-f", "-qq", "-e", "trace=%file,%network,%process", "-o"]).arg(&trace).arg(bin).args(args);
    let resp = match run_with_stdin(&mut cmd, input, ENGINE_TIMEOUT) {
        Ok((_, out, _, _)) => serde_json::from_str::<Response>(out.trim()).ok(),
        Err(_) => return (StraceReport { available: true, ..StraceReport::default() }, None),
    };
    let text = std::fs::read_to_string(&trace).unwrap_or_default();
    (parse_strace(&text), resp)
}

/// Configurações "ingênuas" medidas pra mostrar por que a nossa configuração é necessária: o mesmo
/// engine, com o default da biblioteca no ponto que fecha a porta pro host.
const NAIVE_VARIANTS: &[(&str, &str, &str)] = &[
    ("boa", "--default-context", "Context::default() (loader de módulos padrão, SimpleModuleLoader sobre \".\")"),
    ("mlua", "--keep-base-io", "StdLib sem io/os/package, mas mantendo dofile/loadfile da biblioteca base"),
];

#[derive(Serialize)]
struct NaiveReport {
    engine: String,
    flag: String,
    what: String,
    deny_pass: usize,
    deny_total: usize,
    leaks: Vec<String>,
    strace: StraceReport,
}

fn naive_variant(spec: &EngineSpec, flag: &str, what: &str, corpus: &[CorpusSnippet]) -> NaiveReport {
    let deny: Vec<&CorpusSnippet> = corpus.iter().filter(|s| s.lang == spec.lang && s.is_deny()).collect();
    let input = request_json(&deny, 0, spec.lang);
    let (strace, resp) = strace_engine(&bin_path(&package_of(spec)), &[flag], &input, &format!("{}-naive", spec.name));
    let mut rep = NaiveReport {
        engine: spec.name.into(),
        flag: flag.into(),
        what: what.into(),
        deny_pass: 0,
        deny_total: deny.len(),
        leaks: Vec::new(),
        strace,
    };
    if let Some(resp) = resp {
        let by_id: BTreeMap<&str, &SnippetResult> = resp.results.iter().map(|r| (r.id.as_str(), r)).collect();
        for s in &deny {
            let r = by_id.get(s.id.as_str()).copied();
            rep.deny_pass += judge(s, r).pass as usize;
            if r.is_some_and(|r| r.stdout.contains("LEAK")) {
                rep.leaks.push(s.id.clone());
            }
        }
    }
    rep
}

/// Achados com evidência de código (arquivo e linha no código-fonte da versão medida).
fn code_findings(engine: &str) -> &'static str {
    match engine {
        "monty" => {
            "json só tem loads/dumps (monty src/modules/json/mod.rs, linhas 53 a 58), então json.load(f)/json.dump(x, f) \
             dão AttributeError; round(2.675, 2) devolve 2.68 (CPython: 2.67); open é OsCall respondido pelo host, \
             caminho relativo chega normalizado a partir do cwd configurado."
        }
        "rustpython" => {
            "Mesmo sem host_env, todo Interpreter novo chama getpath::init_path_config (rustpython-vm src/vm/interpreter.rs, \
             linha 96; src/getpath.rs, linha 108): lê PYTHONEXECUTABLE/__PYVENV_LAUNCHER__ do ambiente do host, resolve o \
             executável pelo PATH e procura pyvenv.cfg e Lib/os.py subindo pelos diretórios do host (e lê o pyvenv.cfg se \
             achar). Não há opção que desligue; isolar exige fork dessa função. O crate rustpython-host_env continua \
             obrigatório na árvore."
        }
        "boa" => {
            "Context::default() usa SimpleModuleLoader::new(\".\") (boa_engine src/context/mod.rs, linha 1217): realpath do \
             cwd do host em todo contexto e std::fs::read_to_string no import() (src/module/loader/mod.rs, linha 426). \
             Com ContextBuilder::module_loader(IdleModuleLoader) some. Closure nativa com captura só via \
             NativeFunction::from_closure, que é unsafe: o estado fica num thread_local."
        }
        "rquickjs" => {
            "Context::full registra só intrínsecos ECMAScript; o quickjs-libc (std/os do qjs) não é compilado e o loader de \
             módulos só existe com a feature loader. Closures Rust com captura são seguras (Function::new)."
        }
        "piccolo" => {
            "Stdlib mínima (src/stdlib): string só len/lower/upper/reverse/sub, table só pack/unpack, sem tonumber, \
             string.format/find/gsub/gmatch/byte/char/rep, table.insert/concat/sort/remove, nem metatable de string. Além \
             disso, divergências de semântica do Lua 5.4 no corpus: -7 // 2 dá -3, floats inteiros saem sem .0 e \
             (select(2, ...)) não trunca pra um valor."
        }
        "mlua" => {
            "Lua::new_with só recusa as bibliotecas unsafe (debug, ffi); a base vem sempre com print (stdout C), dofile e \
             loadfile (fopen), que precisam ser trocados/removidos à mão; sem package não há require."
        }
        _ => "",
    }
}

// ------------------------------------------------------------------------------------------------
// Avaliação por engine
// ------------------------------------------------------------------------------------------------

#[derive(Default, Serialize)]
struct SnippetVerdict {
    id: String,
    category: String,
    pass: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Default, Serialize)]
struct EngineReport {
    engine: String,
    lang: String,
    built: bool,
    build_error: Option<String>,
    binary_bytes: u64,
    startup_iterations: usize,
    startup_min_us: f64,
    startup_median_us: f64,
    startup_p90_us: f64,
    startup_output_ok: bool,
    /// Soma do tempo de todos os trechos do corpus (cada um com contexto novo), em ms.
    corpus_elapsed_ms: f64,
    process_wall_ms: f64,
    corpus_pass: usize,
    corpus_total: usize,
    corpus_by_category: BTreeMap<String, (usize, usize)>,
    deny_pass: usize,
    deny_total: usize,
    deny_leaks: Vec<String>,
    strace: StraceReport,
    failures: Vec<SnippetVerdict>,
    engine_notes: Vec<String>,
    depscan: serde_json::Value,
    category: Option<String>,
}

fn judge(s: &CorpusSnippet, r: Option<&SnippetResult>) -> SnippetVerdict {
    let mut v = SnippetVerdict { id: s.id.clone(), category: s.category.clone(), ..SnippetVerdict::default() };
    let Some(r) = r else {
        v.detail = Some("sem resultado".into());
        return v;
    };
    let files_ok = s.expected_files.iter().all(|(p, c)| r.files.get(p) == Some(c));
    v.pass = r.error.is_none() && r.stdout == s.expected && files_ok;
    if !v.pass {
        let mut d = Vec::new();
        if let Some(e) = &r.error {
            d.push(format!("erro: {}", e.lines().last().unwrap_or(e).chars().take(240).collect::<String>()));
        }
        if r.stdout != s.expected {
            d.push(format!("stdout {:?} esperado {:?}", r.stdout.chars().take(160).collect::<String>(), s.expected));
        }
        if !files_ok {
            d.push("arquivos escritos diferentes do esperado".into());
        }
        v.detail = Some(d.join("; "));
    }
    v
}

/// Pacotes ("nome versão") da árvore normal de `package` no Linux do host. O depscan lê o `resolve`
/// do `cargo metadata`, que inclui dependências de todas as plataformas (wasm-bindgen, windows-sys,
/// android...); o `cargo tree` filtra pela plataforma do host, e a interseção dá a categoria que vale
/// aqui.
fn linux_tree(package: &str) -> Option<BTreeSet<String>> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .arg("tree")
        .arg("--manifest-path")
        .arg(manifest())
        .args(["-p", package, "-e", "normal", "--prefix", "none", "-f", "{p}"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Some(
        text.lines()
            .filter_map(|l| {
                let mut it = l.split_whitespace();
                let name = it.next()?;
                let ver = it.next()?.trim_start_matches('v');
                Some(format!("{name} {ver}"))
            })
            .collect(),
    )
}

fn depscan_summary(krate: &str, package: &str) -> (serde_json::Value, Option<String>) {
    match depscan::scan(&manifest(), krate) {
        Ok(scan) => {
            let r = &scan.root;
            let host_deps: Vec<String> = scan.host_touching_deps.keys().cloned().collect();
            let tree = linux_tree(package);
            let (linux_c, linux_host, effective) = match &tree {
                Some(t) => {
                    let c: Vec<String> = scan.c_deps.iter().filter(|d| t.contains(*d)).cloned().collect();
                    let h: Vec<String> = host_deps.iter().filter(|d| t.contains(*d)).cloned().collect();
                    let eff = if r.category == depscan::Category::C || !c.is_empty() {
                        "c"
                    } else if r.counts.host_touch() > 0 || !h.is_empty() {
                        "b"
                    } else {
                        "a"
                    };
                    (c, h, eff.to_string())
                }
                None => (scan.c_deps.clone(), host_deps.clone(), scan.tree_category.letter().to_string()),
            };
            (
                json!({
                    "own_category": r.category.letter(),
                    "tree_category_all_platforms": scan.tree_category.letter(),
                    "tree_category_linux": effective,
                    "own_host_touch": r.counts.host_touch(),
                    "own_unsafe": r.counts.unsafe_total(),
                    "tree_deps_all_platforms": scan.deps.len(),
                    "tree_deps_linux": tree.as_ref().map(|t| t.len().saturating_sub(1)),
                    "tree_host_touch_all_platforms": scan.totals.host_touch(),
                    "tree_unsafe_all_platforms": scan.totals.unsafe_total(),
                    "c_deps_all_platforms": scan.c_deps,
                    "c_deps_linux": linux_c,
                    "host_touching_deps_linux": linux_host,
                }),
                Some(effective),
            )
        }
        Err(e) => (json!({ "error": format!("{e:#}") }), None),
    }
}

fn evaluate(spec: &EngineSpec, corpus: &[CorpusSnippet], build_error: Option<String>) -> EngineReport {
    let mut rep = EngineReport { engine: spec.name.into(), lang: spec.lang.into(), ..EngineReport::default() };
    let (dep, cat) = depscan_summary(spec.krate, &package_of(spec));
    rep.depscan = dep;
    rep.category = cat;
    let bin = bin_path(&package_of(spec));
    if !bin.exists() {
        rep.build_error = Some(build_error.unwrap_or_else(|| "binário não encontrado".into()));
        return rep;
    }
    rep.built = true;
    rep.binary_bytes = std::fs::metadata(&bin).map(|m| m.len()).unwrap_or(0);

    let snippets: Vec<&CorpusSnippet> = corpus.iter().filter(|s| s.lang == spec.lang).collect();
    let input = request_json(&snippets, STARTUP_ITERATIONS, spec.lang);
    match call_engine(&bin, &input) {
        Err(e) => {
            rep.build_error = Some(format!("execução: {e:#}"));
            return rep;
        }
        Ok((resp, _)) => {
            rep.startup_iterations = resp.startup.iterations;
            rep.startup_min_us = resp.startup.min_us;
            rep.startup_median_us = resp.startup.median_us;
            rep.startup_p90_us = resp.startup.p90_us;
            rep.corpus_elapsed_ms = resp.results.iter().map(|r| r.elapsed_us as f64).sum::<f64>() / 1e3;
            rep.startup_output_ok = resp.startup.output == "2\n";
            rep.engine_notes = resp.notes.clone();
            let by_id: BTreeMap<&str, &SnippetResult> = resp.results.iter().map(|r| (r.id.as_str(), r)).collect();
            for s in &snippets {
                let r = by_id.get(s.id.as_str()).copied();
                let v = judge(s, r);
                if s.is_deny() {
                    rep.deny_total += 1;
                    rep.deny_pass += v.pass as usize;
                    if r.is_some_and(|r| r.stdout.contains("LEAK")) {
                        rep.deny_leaks.push(s.id.clone());
                    }
                } else {
                    rep.corpus_total += 1;
                    rep.corpus_pass += v.pass as usize;
                    let slot = rep.corpus_by_category.entry(s.category.clone()).or_default();
                    slot.0 += v.pass as usize;
                    slot.1 += 1;
                }
                if !v.pass {
                    rep.failures.push(v);
                }
            }
        }
    }

    // Processo inteiro: spawn, um one-liner, saída.
    let one = CorpusSnippet {
        id: "process".into(),
        lang: spec.lang.into(),
        kind: "corpus".into(),
        category: "startup".into(),
        code: startup_code(spec.lang).into(),
        expected: "2\n".into(),
        files: BTreeMap::new(),
        expected_files: BTreeMap::new(),
    };
    let one_input = request_json(&[&one], 0, spec.lang);
    let mut walls = Vec::new();
    for _ in 0..PROCESS_RUNS {
        if let Ok((_, d)) = call_engine(&bin, &one_input) {
            walls.push(d.as_secs_f64() * 1e3);
        }
    }
    rep.process_wall_ms = median(&walls);

    // strace com o corpus inteiro da linguagem (sondas de negação incluídas).
    let trace_input = request_json(&snippets, 0, spec.lang);
    rep.strace = strace_engine(&bin, &[], &trace_input, spec.name).0;
    rep
}

fn fit_of(rep: &EngineReport) -> Fit {
    if !rep.built {
        return Fit::DoesNotFit;
    }
    let isolated = rep.deny_leaks.is_empty() && rep.strace.markers_found && rep.strace.host_touches == 0;
    let rate = ratio(rep.corpus_pass, rep.corpus_total);
    if !isolated || rate < 0.6 {
        Fit::DoesNotFit
    } else if rate >= 0.9 && rep.category.as_deref() != Some("c") {
        Fit::Fits
    } else {
        Fit::FitsWithWork
    }
}

fn ratio(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { a as f64 / b as f64 }
}

fn notes_pt(spec: &EngineSpec, rep: &EngineReport, fit: Fit) -> String {
    let mut n = vec![format!("{}.", spec.what)];
    if !rep.built {
        n.push(format!("Não rodou: {}.", rep.build_error.clone().unwrap_or_default()));
        return n.join(" ");
    }
    n.push(format!(
        "Corpus {}/{} ({:.0}%), sondas de negação {}/{}, vazamentos {}, syscalls de host entre os marcadores do strace: {}.",
        rep.corpus_pass,
        rep.corpus_total,
        100.0 * ratio(rep.corpus_pass, rep.corpus_total),
        rep.deny_pass,
        rep.deny_total,
        rep.deny_leaks.len(),
        rep.strace.host_touches
    ));
    n.push(format!(
        "Binário {:.1} MiB, startup em processo {:.0} µs (mediana), processo inteiro {:.1} ms.",
        rep.binary_bytes as f64 / (1024.0 * 1024.0),
        rep.startup_median_us,
        rep.process_wall_ms
    ));
    if !rep.failures.is_empty() {
        let fails: Vec<String> = rep
            .failures
            .iter()
            .take(6)
            .map(|f| format!("{} ({})", f.id, f.detail.clone().unwrap_or_default().chars().take(140).collect::<String>()))
            .collect();
        n.push(format!("Falhas: {}.", fails.join("; ")));
    }
    if rep.strace.host_touches > 0 {
        let paths: Vec<&String> = rep.strace.paths.keys().take(6).collect();
        n.push(format!(
            "Toques de host entre os marcadores, por syscall: {:?}; caminhos (amostra): {:?}.",
            rep.strace.by_syscall, paths
        ));
    }
    if !rep.strace.markers_found {
        n.push("O strace não achou os marcadores (o binário não terminou o protocolo), isolamento não verificado.".into());
    }
    if rep.category.as_deref() == Some("c") {
        n.push("Árvore com C no Linux: o depscan não enxerga dentro do C, então a garantia de isolamento é o strace e o fato de não ligarmos as bibliotecas de I/O.".into());
    }
    let finding = code_findings(spec.name);
    if !finding.is_empty() {
        n.push(finding.to_string());
    }
    n.push(format!("Encaixe: {fit:?}."));
    n.join(" ")
}

// ------------------------------------------------------------------------------------------------
// run
// ------------------------------------------------------------------------------------------------

pub fn run() -> Result<Section> {
    let corpus = load_corpus()?;

    // 1. Confere o esperado de Python e JS no host.
    let host_py = host_check(&corpus, "python", "python3");
    let host_js = host_check(&corpus, "js", "node");

    // 2. Compila os seis engines numa invocação; se falhar, um por um pra saber quem quebrou.
    let packages: Vec<String> = ENGINES.iter().map(package_of).collect();
    let refs: Vec<&str> = packages.iter().map(String::as_str).collect();
    let mut build_errors: BTreeMap<String, String> = BTreeMap::new();
    if let Err(all_err) = build_packages(&refs)? {
        for p in &refs {
            if let Err(e) = build_packages(&[p])? {
                build_errors.insert(p.to_string(), e);
            }
        }
        if build_errors.is_empty() {
            build_errors.insert("todos".into(), all_err);
        }
    }

    // 3. Avalia cada engine.
    let mut reports = Vec::new();
    for spec in ENGINES {
        let err = build_errors.get(&package_of(spec)).cloned();
        reports.push((spec, evaluate(spec, &corpus, err)));
    }

    // 4. Candidatos.
    let mut candidates = Vec::new();
    let mut table = Vec::new();
    for (spec, rep) in &reports {
        let fit = fit_of(rep);
        table.push(json!({
            "engine": spec.name,
            "lang": spec.lang,
            "built_with_forbid_unsafe": rep.built,
            "binary_bytes": rep.binary_bytes,
            "startup_median_us": rep.startup_median_us,
            "process_wall_ms": rep.process_wall_ms,
            "corpus": format!("{}/{}", rep.corpus_pass, rep.corpus_total),
            "deny": format!("{}/{}", rep.deny_pass, rep.deny_total),
            "leaks": rep.deny_leaks.len(),
            "strace_host_touches": rep.strace.host_touches,
            "category": rep.category,
            "fit": fit,
        }));
        candidates.push(CandidateResult {
            name: spec.krate.to_string(),
            version: spec.version.to_string(),
            role: spec.lang.to_string(),
            category: rep.category.clone(),
            conformance: None,
            fit,
            notes: notes_pt(spec, rep, fit),
            metrics: serde_json::to_value(rep).unwrap_or(serde_json::Value::Null),
        });
    }

    // 5. Veredito: por linguagem, o melhor engine isolado.
    let mut best: BTreeMap<&str, (&EngineSpec, &EngineReport, Fit)> = BTreeMap::new();
    for (spec, rep) in &reports {
        let fit = fit_of(rep);
        let score = |f: Fit, r: &EngineReport| -> (u8, i64, i64) {
            let f = match f {
                Fit::Fits => 3,
                Fit::FitsWithWork => 2,
                _ => 0,
            };
            (f, (1000.0 * ratio(r.corpus_pass, r.corpus_total)) as i64, -(r.startup_median_us as i64))
        };
        let replace = match best.get(spec.lang) {
            None => true,
            Some((_, r0, f0)) => score(fit, rep) > score(*f0, r0),
        };
        if replace {
            best.insert(spec.lang, (spec, rep, fit));
        }
    }
    // Confirmada: as três linguagens têm engine que encaixa como está (Fits: isolado, >= 90% do corpus,
    // sem C). Parcial: alguma só encaixa com trabalho (C, subconjunto, fork). Refutada: nenhuma.
    let langs_ok: Vec<&str> = best
        .iter()
        .filter(|(_, (_, _, f))| matches!(f, Fit::Fits | Fit::FitsWithWork))
        .map(|(l, _)| *l)
        .collect();
    let langs_fit: Vec<&str> = best.iter().filter(|(_, (_, _, f))| *f == Fit::Fits).map(|(l, _)| *l).collect();
    let verdict = if langs_fit.len() == 3 {
        Verdict::Confirmed
    } else if langs_ok.is_empty() {
        Verdict::Refuted
    } else {
        Verdict::Partial
    };
    let naive: Vec<NaiveReport> = NAIVE_VARIANTS
        .iter()
        .filter_map(|(name, flag, what)| {
            let (spec, rep) = reports.iter().find(|(s, _)| s.name == *name)?;
            rep.built.then(|| naive_variant(spec, flag, what, &corpus))
        })
        .collect();
    let recs: Vec<String> = best
        .iter()
        .map(|(lang, (spec, rep, fit))| {
            format!(
                "{lang}: {} ({}/{} no corpus, {:.0} µs de startup, {:.1} MiB, {fit:?})",
                spec.name,
                rep.corpus_pass,
                rep.corpus_total,
                rep.startup_median_us,
                rep.binary_bytes as f64 / (1024.0 * 1024.0)
            )
        })
        .collect();
    let built = reports.iter().filter(|(_, r)| r.built).count();
    let isolated: Vec<&str> = reports
        .iter()
        .filter(|(_, r)| r.built && r.deny_leaks.is_empty() && r.strace.markers_found && r.strace.host_touches == 0)
        .map(|(s, _)| s.name)
        .collect();
    let not_isolated: Vec<String> = reports
        .iter()
        .filter(|(s, _)| !isolated.contains(&s.name))
        .map(|(s, r)| format!("{} ({} syscalls de host)", s.name, r.strace.host_touches))
        .collect();
    let naive_txt: Vec<String> = naive
        .iter()
        .map(|n| format!("{} {}: {} toque(s) de host e {} vazamento(s)", n.engine, n.flag, n.strace.host_touches, n.leaks.len()))
        .collect();
    let with_work: Vec<&str> = langs_ok.iter().filter(|l| !langs_fit.contains(l)).copied().collect();
    let summary = format!(
        "{built}/6 engines compilam com forbid(unsafe_code) num crate nosso, com print, arquivo e ambiente do nosso lado. \
         Isolados de verdade (zero vazamento nas sondas e zero syscall de host entre os marcadores do strace): {}; não \
         isolados: {}. Configuração ingênua medida: {}. Linguagens com engine que encaixa como está: {}; só com \
         trabalho (C, subconjunto ou fork): {}. Recomendação: {}.",
        isolated.join(", "),
        if not_isolated.is_empty() { "nenhum".to_string() } else { not_isolated.join(", ") },
        naive_txt.join("; "),
        if langs_fit.is_empty() { "nenhuma".to_string() } else { langs_fit.join(", ") },
        if with_work.is_empty() { "nenhuma".to_string() } else { with_work.join(", ") },
        recs.join("; ")
    );
    let evidence = json!({
        "table": table,
        "recommendation": recs,
        "langs_fits": langs_fit,
        "langs_fits_or_with_work": langs_ok,
        "naive_configurations": naive,
        "code_findings": ENGINES.iter().map(|e| (e.name, code_findings(e.name))).collect::<BTreeMap<_, _>>(),
        "host_expected_check": { "python3": host_py, "node": host_js },
        "corpus_file": "experiments/f15-shell-tty-interp/data/interp-corpus.toml",
        "corpus_size": {
            "python": corpus.iter().filter(|s| s.lang == "python" && !s.is_deny()).count(),
            "js": corpus.iter().filter(|s| s.lang == "js" && !s.is_deny()).count(),
            "lua": corpus.iter().filter(|s| s.lang == "lua" && !s.is_deny()).count(),
            "deny": corpus.iter().filter(|s| s.is_deny()).count(),
        },
        "build_errors": build_errors,
        "method": {
            "startup": format!("mediana de {STARTUP_ITERATIONS} criações de contexto + one-liner, em processo"),
            "process_wall": format!("mediana de {PROCESS_RUNS} execuções do binário com um one-liner (spawn até sair)"),
            "strace": "strace -f -e trace=%file,%network,%process; conta syscalls com caminho, de rede ou de criação de processo entre os marcadores (stat em /f15-strace-marker-begin e -end)",
            "lua_expected": "esperado de Lua escrito à mão e conferido contra o Lua 5.4 de referência (C vendored do mlua); o host não tem lua",
        },
    });
    let mut notes = vec![format!(
        "H39: esperado do corpus confirmado no host: python3 {}/{}, node {}/{}.",
        host_py_confirmed(&evidence, "python3").0,
        host_py_confirmed(&evidence, "python3").1,
        host_py_confirmed(&evidence, "node").0,
        host_py_confirmed(&evidence, "node").1
    )];
    for (spec, rep) in &reports {
        if !rep.strace.ignored.is_empty() {
            notes.push(format!("H39: strace de {} ignorou ruído de runtime: {:?}.", spec.name, rep.strace.ignored));
        }
    }
    Ok(Section { hypothesis: "H39", verdict, summary, evidence, candidates, notes })
}

fn host_py_confirmed(evidence: &serde_json::Value, tool: &str) -> (u64, u64) {
    let hc = &evidence["host_expected_check"][tool];
    (hc["confirmed"].as_u64().unwrap_or(0), hc["checked"].as_u64().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_parses_and_is_balanced() {
        let corpus = load_corpus().expect("corpus válido");
        for lang in ["python", "js", "lua"] {
            let n = corpus.iter().filter(|s| s.lang == lang && !s.is_deny()).count();
            let d = corpus.iter().filter(|s| s.lang == lang && s.is_deny()).count();
            assert!(n >= 20, "{lang}: só {n} trechos de corpus");
            assert!(d >= 5, "{lang}: só {d} sondas de negação");
        }
        for s in &corpus {
            assert!(s.expected.ends_with('\n'), "{}: esperado sem quebra de linha final", s.id);
            if s.is_deny() {
                assert!(s.expected.starts_with("DENIED"), "{}: sonda deve esperar DENIED", s.id);
            }
        }
    }

    #[test]
    fn strace_parser_counts_only_between_markers() {
        let text = "\
100 openat(AT_FDCWD, \"/etc/ld.so.cache\", O_RDONLY|O_CLOEXEC) = 3
100 statx(AT_FDCWD, \"/f15-strace-marker-begin\", AT_STATX_SYNC_AS_STAT, STATX_ALL, 0x7ffd) = -1 ENOENT
100 openat(AT_FDCWD, \"/etc/passwd\", O_RDONLY) = 3
100 openat(AT_FDCWD, \"/sys/devices/system/cpu/online\", O_RDONLY) = 3
100 clone3({flags=CLONE_VM|CLONE_FS|CLONE_THREAD, ...}, 88) = 101
101 +++ exited with 0 +++
100 socket(AF_INET, SOCK_STREAM, IPPROTO_IP) = 4
100 statx(3, \"\", AT_STATX_SYNC_AS_STAT|AT_EMPTY_PATH, STATX_ALL, {...}) = 0
100 getcwd(\"/home/x\", 1024) = 8
100 statx(AT_FDCWD, \"/f15-strace-marker-end\", AT_STATX_SYNC_AS_STAT, STATX_ALL, 0x7ffd) = -1 ENOENT
100 openat(AT_FDCWD, \"/etc/hosts\", O_RDONLY) = 3
";
        let r = parse_strace(text);
        assert!(r.markers_found);
        // /etc/passwd, socket e getcwd; o statx em fd (caminho vazio) não conta.
        assert_eq!(r.host_touches, 3, "{r:?}");
        assert_eq!(r.by_syscall.get("getcwd"), Some(&1));
        assert_eq!(r.threads_spawned, 1);
        assert_eq!(r.ignored.get("/sys/devices/system/cpu/online"), Some(&1));
    }

    #[test]
    fn syscall_name_handles_pid_and_resumed() {
        assert_eq!(syscall_name("123 openat(AT_FDCWD, \"x\")"), Some("openat"));
        assert_eq!(syscall_name("123 <... openat resumed>) = 3"), Some("openat"));
        assert_eq!(syscall_name("execve(\"/bin/x\", ...)"), Some("execve"));
    }
}
