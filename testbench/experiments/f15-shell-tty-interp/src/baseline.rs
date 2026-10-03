//! H40: linhas de base. Projetos que já fazem "bash virtual sobre FS em memória" (bashkit, rust-bash,
//! kaish, wasmsh) rodando os casos do golden de TODAS as ferramentas.
//!
//! Cada linha de base é um binário separado (`baseline/<nome>`), que fala o protocolo de
//! `baseline/common`: um `Case` em JSON por linha no stdin, um `Outcome` em JSON por linha no stdout.
//! Aqui cada binário sobe como processo filho (com `prlimit --as` limitando a memória), caso a caso,
//! com timeout; se travar ou morrer, o caso conta como timeout ou crash e um processo novo assume o
//! caso seguinte. A comparação é a do harness (`compare_outcome`), agregada por diretório de
//! ferramenta e por área.
//!
//! Dois conjuntos são medidos:
//!
//! - **script**: os casos `script` do golden, como pede o critério do H40 (é o que um agente manda);
//! - **argv**: os casos `argv`, convertidos em linha de shell com cada argumento citado
//!   (`f15_baseline_common::argv_script`). Dá a taxa por área de userland (coreutils, awk, jq, sqlite)
//!   com muito mais casos do que o conjunto script tem.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use anyhow::Result;
use harness::{
    CandidateResult, Case, CaseComparison, Conformance, Fit, Outcome, Verdict, compare_outcome, paths,
};
use serde_json::json;

use crate::common::{Section, bin_path, build_packages, manifest};

/// Linhas de base medidas: (binário, crate, versão).
const BASELINES: &[(&str, &str, &str)] = &[
    ("f15-baseline-bashkit", "bashkit", "0.18.2"),
    ("f15-baseline-rust-bash", "rust-bash", "0.3.0"),
    ("f15-baseline-kaish", "kaish-kernel", "0.17.2"),
    ("f15-baseline-wasmsh", "wasmsh-runtime", "0.9.0"),
];

/// Timeout por caso, salvo quando o caso pede mais (`timeout_ms` + folga).
const CASE_TIMEOUT: Duration = Duration::from_secs(10);
/// Teto de memória virtual de cada processo filho.
const CHILD_AS_LIMIT: u64 = 4 << 30;
/// Linha de resposta maior que isso é tratada como falha (proteção contra saída sem fim).
const MAX_LINE: usize = 64 << 20;
/// Processos filhos por linha de base (as quatro rodam em paralelo).
const WORKERS: usize = 3;

/// Área do plano a que cada diretório de ferramenta pertence (critério do H40).
pub fn area_of(tool: &str) -> &'static str {
    match tool {
        "shell" | "smoke" => "shell",
        "coreutils" | "find" | "xargs" | "diff" | "patch" | "date" | "files" | "archive" | "compress" => {
            "coreutils"
        }
        "grep" | "sed" | "awk" | "regex" => "grep/sed/awk",
        "jq" | "yq" => "jq",
        _ => "outros",
    }
}

/// Como um caso terminou do lado do processo filho.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunStatus {
    Answered,
    Timeout,
    Crash,
}

/// Um processo filho de linha de base, reiniciado sob demanda.
struct ChildRunner {
    bin: PathBuf,
    child: Option<(Child, ChildStdin, Receiver<Result<String, String>>)>,
    restarts: usize,
    /// Variáveis extras no ambiente do processo filho (só as sondas de vazamento usam).
    extra_env: Vec<(String, String)>,
}

impl ChildRunner {
    fn new(bin: PathBuf) -> ChildRunner {
        ChildRunner { bin, child: None, restarts: 0, extra_env: Vec::new() }
    }

    fn start(&mut self) -> std::io::Result<()> {
        let mut cmd = Command::new("prlimit");
        cmd.envs(self.extra_env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        cmd.arg(format!("--as={CHILD_AS_LIMIT}"))
            .arg("--")
            .arg(&self.bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("stdin do filho");
        let stdout = child.stdout.take().expect("stdout do filho");
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_line_limited(&mut reader) {
                    Ok(Some(line)) => {
                        if tx.send(Ok(line)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => return,
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                }
            }
        });
        self.child = Some((child, stdin, rx));
        Ok(())
    }

    fn kill(&mut self) {
        if let Some((mut child, stdin, _rx)) = self.child.take() {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Roda um caso; em timeout ou crash, mata o filho (o próximo caso sobe outro).
    fn run(&mut self, case: &Case) -> (Outcome, RunStatus) {
        if self.child.is_none() {
            if let Err(e) = self.start() {
                return (Outcome::unsupported(format!("crash: não subiu o processo: {e}")), RunStatus::Crash);
            }
            self.restarts += 1;
        }
        let line = serde_json::to_string(case).expect("caso serializável") + "\n";
        let timeout = case.timeout_ms.map(|ms| Duration::from_millis(ms) + Duration::from_secs(5)).unwrap_or(CASE_TIMEOUT);
        let (_, stdin, rx) = self.child.as_mut().expect("filho vivo");
        if stdin.write_all(line.as_bytes()).and_then(|_| stdin.flush()).is_err() {
            self.kill();
            return (Outcome::unsupported("crash: processo morreu antes do caso"), RunStatus::Crash);
        }
        match rx.recv_timeout(timeout) {
            Ok(Ok(text)) => match serde_json::from_str::<Outcome>(&text) {
                Ok(o) => (o, RunStatus::Answered),
                Err(e) => {
                    self.kill();
                    (Outcome::unsupported(format!("crash: resposta inválida: {e}")), RunStatus::Crash)
                }
            },
            Ok(Err(e)) => {
                self.kill();
                (Outcome::unsupported(format!("crash: {e}")), RunStatus::Crash)
            }
            Err(RecvTimeoutError::Timeout) => {
                self.kill();
                (Outcome::unsupported(format!("timeout: sem resposta em {} s", timeout.as_secs())), RunStatus::Timeout)
            }
            Err(RecvTimeoutError::Disconnected) => {
                let status = self.child.as_mut().and_then(|(c, _, _)| c.wait().ok());
                self.kill();
                let why = status.map(|s| s.to_string()).unwrap_or_else(|| "sem status".into());
                (Outcome::unsupported(format!("crash: processo morreu ({why})")), RunStatus::Crash)
            }
        }
    }
}

impl Drop for ChildRunner {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Lê uma linha (sem o `\n`) com teto de tamanho. `Ok(None)` no EOF.
fn read_line_limited(reader: &mut impl BufRead) -> Result<Option<String>, String> {
    let mut buf = Vec::new();
    loop {
        let chunk = reader.fill_buf().map_err(|e| e.to_string())?;
        if chunk.is_empty() {
            return if buf.is_empty() { Ok(None) } else { Ok(Some(String::from_utf8_lossy(&buf).into_owned())) };
        }
        if let Some(pos) = chunk.iter().position(|&b| b == b'\n') {
            buf.extend_from_slice(&chunk[..pos]);
            reader.consume(pos + 1);
            return Ok(Some(String::from_utf8_lossy(&buf).into_owned()));
        }
        let n = chunk.len();
        buf.extend_from_slice(chunk);
        reader.consume(n);
        if buf.len() > MAX_LINE {
            // Esvazia o resto pra não travar o filho, mas desiste da resposta.
            let _ = reader.take(u64::MAX).read_to_end(&mut Vec::new());
            return Err(format!("resposta maior que {MAX_LINE} bytes"));
        }
    }
}

/// Um caso do golden com a ferramenta (diretório) de onde veio.
#[derive(Clone)]
struct GoldenCase {
    tool: String,
    case: Case,
    golden: Outcome,
}

/// Resultado de um caso numa linha de base.
#[derive(Clone)]
struct CaseRun {
    cmp: CaseComparison,
    status: RunStatus,
    elapsed: Duration,
}

/// Placar de um conjunto de casos (por ferramenta ou área).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Tally {
    pub total: usize,
    pub strict: usize,
    pub lenient: usize,
    /// stdout e exit iguais, ignorando stderr e arquivos (mede o comportamento do shell mesmo quando
    /// o VFS do candidato não modela modos ou symlinks).
    pub stdout_exit: usize,
    pub unsupported: usize,
    pub timeouts: usize,
    pub crashes: usize,
}

impl Tally {
    fn add(&mut self, cmp: &CaseComparison, status: RunStatus) {
        self.total += 1;
        self.strict += cmp.strict as usize;
        self.lenient += cmp.lenient as usize;
        self.stdout_exit += (cmp.unsupported.is_none() && cmp.stdout_ok && cmp.exit_ok) as usize;
        self.unsupported += cmp.unsupported.is_some() as usize;
        self.timeouts += (status == RunStatus::Timeout) as usize;
        self.crashes += (status == RunStatus::Crash) as usize;
    }

    pub fn rate(n: usize, total: usize) -> f64 {
        if total == 0 { 0.0 } else { n as f64 / total as f64 }
    }

    fn to_json(&self) -> serde_json::Value {
        json!({
            "total": self.total,
            "strict": self.strict,
            "lenient": self.lenient,
            "stdout_exit": self.stdout_exit,
            "unsupported": self.unsupported,
            "timeouts": self.timeouts,
            "crashes": self.crashes,
            "strict_rate": round3(Tally::rate(self.strict, self.total)),
            "lenient_rate": round3(Tally::rate(self.lenient, self.total)),
            "stdout_exit_rate": round3(Tally::rate(self.stdout_exit, self.total)),
        })
    }
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// Agrega por chave (ferramenta ou área).
fn tally_by<'a>(
    runs: impl Iterator<Item = (&'a str, &'a CaseRun)>,
    key: impl Fn(&str) -> String,
) -> BTreeMap<String, Tally> {
    let mut out: BTreeMap<String, Tally> = BTreeMap::new();
    for (tool, r) in runs {
        out.entry(key(tool)).or_default().add(&r.cmp, r.status);
    }
    out
}

/// Troca travessão e meia-risca (que aparecem em mensagens dos candidatos) por hífen, pra que o JSON
/// de resultados siga a regra de texto do projeto.
fn sanitize(s: &str) -> String {
    s.replace(['\u{2014}', '\u{2013}'], "-")
}

fn sanitize_cmp(mut c: CaseComparison) -> CaseComparison {
    c.detail = c.detail.iter().map(|d| sanitize(d)).collect();
    c.unsupported = c.unsupported.map(|u| sanitize(&u));
    c
}

fn load_golden() -> Result<(Vec<GoldenCase>, usize)> {
    let mut out = Vec::new();
    let mut missing = 0;
    for tool in paths::tools()? {
        let (cases, miss) = paths::load_tool(&tool)?;
        missing += miss;
        for (case, golden) in cases {
            out.push(GoldenCase { tool: tool.clone(), case, golden });
        }
    }
    Ok((out, missing))
}

/// Roda todos os casos numa linha de base, com `WORKERS` processos filhos em paralelo.
fn run_baseline(bin: &PathBuf, cases: &[GoldenCase]) -> (Vec<CaseRun>, usize) {
    let mut slots: Vec<Option<CaseRun>> = vec![None; cases.len()];
    let mut restarts = 0;
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..WORKERS)
            .map(|w| {
                let bin = bin.clone();
                s.spawn(move || {
                    let mut runner = ChildRunner::new(bin);
                    let mut done = Vec::new();
                    for (i, gc) in cases.iter().enumerate().filter(|(i, _)| i % WORKERS == w) {
                        let start = Instant::now();
                        let (outcome, status) = runner.run(&gc.case);
                        let elapsed = start.elapsed();
                        let cmp = compare_outcome(&gc.case, &gc.golden, &outcome);
                        done.push((i, CaseRun { cmp, status, elapsed }));
                    }
                    (done, runner.restarts)
                })
            })
            .collect();
        for h in handles {
            let (done, r) = h.join().expect("worker");
            restarts += r;
            for (i, run) in done {
                slots[i] = Some(run);
            }
        }
    });
    (slots.into_iter().map(|r| r.expect("todo caso rodou")).collect(), restarts)
}

/// Exemplos de falha pra registrar: até `n`, variando a ferramenta.
fn failure_examples(cases: &[GoldenCase], runs: &[CaseRun], n: usize) -> Vec<String> {
    let mut seen_tools: BTreeMap<&str, usize> = BTreeMap::new();
    let mut out = Vec::new();
    for (gc, r) in cases.iter().zip(runs) {
        if r.cmp.lenient {
            continue;
        }
        let k = seen_tools.entry(gc.tool.as_str()).or_default();
        if *k >= 2 {
            continue;
        }
        *k += 1;
        let first = r.cmp.detail.first().map(|d| sanitize(d)).unwrap_or_default();
        let first: String = first.chars().take(160).collect();
        out.push(format!("{}/{}: {first}", gc.tool, gc.case.id));
        if out.len() >= n {
            break;
        }
    }
    out
}

/// Marca posta só no ambiente do processo host do candidato (nunca no ambiente do caso).
const LEAK_ENV: &str = "F15_HOST_LEAK_PROBE";

/// Sondas de isolamento: o candidato deixa o script ver o ambiente ou um arquivo do host? Cada sonda
/// é um caso comum; "vazou" quando a saída contém a marca do host.
fn leak_probes(bin: &PathBuf) -> serde_json::Value {
    let marker = format!("vazou-{}", std::process::id());
    let machine_id = std::fs::read_to_string("/etc/machine-id").map(|s| s.trim().to_string()).unwrap_or_default();
    let probes: [(&str, String, &str); 3] = [
        ("env-shell", format!("echo \"${{{LEAK_ENV}:-ausente}}\""), "env"),
        ("env-jq", format!("jq -n -r 'env.{LEAK_ENV} // \"ausente\"'"), "env"),
        ("host-file", "cat /etc/machine-id 2>/dev/null || echo ausente".to_string(), "file"),
    ];
    let mut runner = ChildRunner::new(bin.clone());
    runner.extra_env.push((LEAK_ENV.into(), marker.clone()));
    let mut out = serde_json::Map::new();
    for (id, script, kind) in probes {
        let case: Case = match toml::from_str(&format!("id = {id:?}\nscript = {script:?}\n")) {
            Ok(c) => c,
            Err(e) => {
                out.insert(id.into(), json!({ "error": e.to_string() }));
                continue;
            }
        };
        let (o, status) = runner.run(&case);
        let stdout = String::from_utf8_lossy(o.stdout.as_slice()).into_owned();
        let needle = if kind == "env" { marker.as_str() } else { machine_id.as_str() };
        let leaked = !needle.is_empty() && stdout.contains(needle);
        // O conteúdo do arquivo do host (machine-id) não vai pro JSON: só o fato de ter vazado.
        let shown = if kind == "env" { sanitize(&stdout.chars().take(120).collect::<String>()) } else { String::new() };
        out.insert(
            id.into(),
            json!({
                "script": script,
                "leaked": leaked,
                "status": format!("{status:?}"),
                "stdout": shown,
            }),
        );
    }
    serde_json::Value::Object(out)
}

/// Notas fixas por linha de base: o que a API oferece e as limitações (com arquivo e linha).
fn api_notes(krate: &str) -> Vec<String> {
    match krate {
        "bashkit" => vec![
            "API: BashBuilder (env, cwd, limits, fixed_epoch, username, hostname) + Bash::exec_with_options (stdin) + Bash::fs() (InMemoryFs com chmod, symlink, set_modified_time). Tudo que a bancada precisa existe na API pública.".into(),
            "Rodou com ExecutionLimits::cli() (os limites do CLI do bashkit) e as features jq, git e sqlite (Turso). faketime vira fixed_epoch.".into(),
            "O jq do bashkit troca o halt do jaq por um erro (bashkit src/builtins/jq/mod.rs, linhas 245 a 260, \"halt is disabled in the bashkit sandbox\"): halt_error não derruba o processo.".into(),
        ],
        "rust-bash" => vec![
            "API: RustBashBuilder (env, cwd, execution_limits) + RustBash::exec + RustBash::fs() (VirtualFs com chmod, utimes, symlink, lstat). Sem as features cli, network e native-fs.".into(),
            "stdin: exec_with_overrides só aceita &str e cola um here-doc no fim do script (rust-bash src/api.rs, linhas 176 a 183), o que alimenta só o último comando. Redirecionamento em comando composto ({ ...; } < f, ( ... ) < f) não chega ao stdin dos comandos de dentro (medido: { read x; } < f lê vazio); a bancada grava o stdin no VFS e roda cat arquivo | { script; }.".into(),
            "Relógio do host sem injeção: date usa chrono::Local::now() (rust-bash src/commands/utils.rs, linha 294); casos com faketime ficam unsupported.".into(),
            "jq halt/halt_error derruba o processo inteiro: o embutidor chama jaq_core::unwrap_valr, que faz std::process::exit no halt (jaq-core 3.1.1 src/val.rs, linhas 38 a 46).".into(),
            "Usa brush-parser 0.3 como parser (dependência), o que é evidência a favor do H37.".into(),
        ],
        "kaish-kernel" => vec![
            "kaish não é bash: é uma linguagem própria parecida com Bourne (variáveis tipadas, JSON nativo). Nome de comando entre aspas é erro de parse, por isso o argv é citado só quando precisa.".into(),
            "API: Kernel::new(KernelConfig::isolated()) (VFS só em memória, sem comando externo) + execute_with_options (vars, cwd, stdin) + Kernel::vfs() (trait Filesystem). Sem features localfs, overlay e subprocess.".into(),
            "MemoryFs sem modelo de permissão nem chmod: reporta 0666 pra arquivo e 0777 pra diretório (kaish-vfs src/memory.rs, linhas 200 a 216); symlink só com alvo relativo (linha 700). Sem relógio injetável.".into(),
            "jq halt/halt_error derruba o processo inteiro (jaq_core::unwrap_valr faz std::process::exit: jaq-core 3.1.1 src/val.rs, linhas 38 a 46).".into(),
        ],
        "wasmsh-runtime" => vec![
            "API: só o protocolo do WorkerRuntime (HostCommand::Init/Run/WriteFile/ReadFile/ListDir); o VFS é campo privado. Ambiente, cwd, diretórios, modos e symlinks entram por comandos do próprio wasmsh; stdin vai num arquivo do VFS e o script roda em cat arquivo | { script; }, porque redirecionamento em comando composto ({ ...; } < f) não chega ao stdin dos comandos de dentro (medido).".into(),
            "Sem symlink (ln copia: wasmsh-utils src/file_ops.rs, linha 814), sem mtime (touch -d/-t é no-op: linhas 615 a 617) e stat com modo fixo 644/755 (linhas 921 e 922): o retrato usa esses modos.".into(),
            "jq halt/halt_error derruba o processo inteiro: o jaq-std 2.1.2 que o wasmsh-utils usa chama std::process::exit no halt (jaq-std 2.1.2 src/lib.rs, linhas 446 e 454).".into(),
        ],
        _ => Vec::new(),
    }
}

fn depscan_metrics(krate: &str) -> (Option<String>, serde_json::Value) {
    match depscan::scan(&manifest(), krate) {
        Ok(scan) => {
            let own = scan.root.category.letter().to_string();
            let v = json!({
                "own_category": own,
                "tree_category": scan.tree_category.letter(),
                "own_host_touch": scan.root.counts.host_touch(),
                "own_unsafe": scan.root.counts.unsafe_total(),
                "tree_deps": scan.deps.len(),
                "tree_host_touch": scan.totals.host_touch(),
                "tree_unsafe": scan.totals.unsafe_total(),
                "c_deps": scan.c_deps,
                "host_touching_deps": scan.host_touching_deps.len(),
            });
            (Some(own), v)
        }
        Err(e) => (None, json!({ "error": e.to_string() })),
    }
}

/// Uma linha de base medida.
struct Measured {
    krate: &'static str,
    version: &'static str,
    bin: &'static str,
    runs: Option<Vec<CaseRun>>,
    restarts: usize,
    elapsed: Duration,
    build_error: Option<String>,
    leaks: serde_json::Value,
}

impl Measured {
    fn leaked(&self) -> Vec<String> {
        match &self.leaks {
            serde_json::Value::Object(m) => m
                .iter()
                .filter(|(_, v)| v.get("leaked").and_then(|b| b.as_bool()).unwrap_or(false))
                .map(|(k, _)| k.clone())
                .collect(),
            _ => Vec::new(),
        }
    }
}

pub fn run() -> Result<Section> {
    let started = Instant::now();
    let packages: Vec<&str> = BASELINES.iter().map(|(b, _, _)| *b).collect();
    let build = build_packages(&packages)?;
    let build_error = build.err();

    let (cases, missing_golden) = load_golden()?;
    // O depscan (syn sobre a árvore toda) é a parte lenta: roda em paralelo com as linhas de base.
    let mut deps: BTreeMap<&str, (Option<String>, serde_json::Value)> = BTreeMap::new();
    let measured: Vec<Measured> = std::thread::scope(|s| {
        let dep_handles: Vec<_> = BASELINES
            .iter()
            .map(|&(_, krate, _)| (krate, s.spawn(move || depscan_metrics(krate))))
            .collect();
        let handles: Vec<_> = BASELINES
            .iter()
            .map(|&(bin, krate, version)| {
                let cases = &cases;
                let build_error = build_error.clone();
                s.spawn(move || {
                    let path = bin_path(bin);
                    if !path.exists() {
                        return Measured {
                            krate,
                            version,
                            bin,
                            runs: None,
                            restarts: 0,
                            elapsed: Duration::ZERO,
                            build_error: Some(build_error.unwrap_or_else(|| "binário não gerado".into())),
                            leaks: serde_json::Value::Null,
                        };
                    }
                    let t = Instant::now();
                    let (runs, restarts) = run_baseline(&path, cases);
                    let elapsed = t.elapsed();
                    let leaks = leak_probes(&path);
                    Measured { krate, version, bin, runs: Some(runs), restarts, elapsed, build_error: None, leaks }
                })
            })
            .collect();
        let measured = handles.into_iter().map(|h| h.join().expect("linha de base")).collect();
        for (krate, h) in dep_handles {
            deps.insert(krate, h.join().expect("depscan"));
        }
        measured
    });

    let mut candidates = Vec::new();
    let mut table = serde_json::Map::new();
    let mut best: Option<(f64, String, String, usize)> = None; // (lenient, candidato, área, n)
    let mut strong_areas: Vec<String> = Vec::new();
    let mut partial_areas: Vec<String> = Vec::new();
    let mut leak_lines: Vec<String> = Vec::new();
    let tools_present: Vec<String> = {
        let mut t: Vec<String> = cases.iter().map(|c| c.tool.clone()).collect();
        t.dedup();
        t
    };

    for m in &measured {
        let (category, dep_metrics) = deps.remove(m.krate).unwrap_or((None, json!({ "error": "depscan não rodou" })));
        let size = std::fs::metadata(bin_path(m.bin)).map(|x| x.len()).ok();
        let mut notes = api_notes(m.krate);
        let Some(runs) = &m.runs else {
            notes.push(format!("Não compilou ou não gerou binário: {}", sanitize(m.build_error.as_deref().unwrap_or("?"))));
            candidates.push(CandidateResult {
                name: m.krate.into(),
                version: m.version.into(),
                role: "baseline".into(),
                category,
                conformance: None,
                fit: Fit::Reference,
                notes: notes.join(" "),
                metrics: json!({ "depscan": dep_metrics, "build_error": m.build_error.as_deref().map(sanitize) }),
            });
            continue;
        };
        let pairs = || cases.iter().zip(runs.iter());
        let script_pairs = || pairs().filter(|(gc, _)| gc.case.script.is_some());
        let argv_pairs = || pairs().filter(|(gc, _)| gc.case.script.is_none());
        let by_tool_script = tally_by(script_pairs().map(|(gc, r)| (gc.tool.as_str(), r)), |t| t.to_string());
        let by_tool_argv = tally_by(argv_pairs().map(|(gc, r)| (gc.tool.as_str(), r)), |t| t.to_string());
        let by_area_all = tally_by(pairs().map(|(gc, r)| (gc.tool.as_str(), r)), |t| area_of(t).to_string());
        let by_area_script = tally_by(script_pairs().map(|(gc, r)| (gc.tool.as_str(), r)), |t| area_of(t).to_string());
        let mut total_script = Tally::default();
        for (_, r) in script_pairs() {
            total_script.add(&r.cmp, r.status);
        }
        // Recurso de shell (tag do caso) no corpus de shell: mostra o que a linha de base já cobre.
        let mut shell_by_tag: BTreeMap<String, Tally> = BTreeMap::new();
        for (gc, r) in pairs().filter(|(gc, _)| gc.tool == "shell") {
            for tag in &gc.case.tags {
                shell_by_tag.entry(tag.clone()).or_default().add(&r.cmp, r.status);
            }
        }
        let mut total_all = Tally::default();
        for (_, r) in pairs() {
            total_all.add(&r.cmp, r.status);
        }
        for (area, t) in &by_area_all {
            let rate = Tally::rate(t.lenient, t.total);
            if t.total >= 20 {
                if best.as_ref().is_none_or(|b| rate > b.0) {
                    best = Some((rate, m.krate.to_string(), area.clone(), t.total));
                }
                // 80%: dá pra adotar como base daquela área. 40%: cobre parte, vale estudar ou
                // reaproveitar pedaços, mas não muda a estratégia sozinho.
                if rate >= 0.8 {
                    strong_areas.push(format!("{} em {} ({:.0}% leniente, n={})", m.krate, area, rate * 100.0, t.total));
                } else if rate >= 0.4 {
                    partial_areas.push(format!("{} em {} ({:.0}% leniente, n={})", m.krate, area, rate * 100.0, t.total));
                }
            }
        }
        // Conformance no formato do harness, sobre o conjunto script (o do critério), by_tag = ferramenta.
        let conformance = Conformance {
            candidate: format!("{} {}", m.krate, m.version),
            total: total_script.total,
            strict_pass: total_script.strict,
            lenient_pass: total_script.lenient,
            unsupported: total_script.unsupported,
            by_tag: by_tool_script.iter().map(|(k, t)| (k.clone(), (t.strict, t.lenient, t.total))).collect(),
            sample_failures: script_pairs()
                .filter(|(_, r)| !r.cmp.strict)
                .take(15)
                .map(|(_, r)| sanitize_cmp(r.cmp.clone()))
                .collect(),
        };
        let examples = failure_examples(&cases, runs, 5);
        notes.push(format!("Falhas típicas: {}", examples.join(" | ")));
        let incidents = |want: RunStatus| -> Vec<String> {
            cases
                .iter()
                .zip(runs.iter())
                .filter(|(_, r)| r.status == want)
                .map(|(gc, r)| format!("{}/{}: {}", gc.tool, gc.case.id, sanitize(r.cmp.unsupported.as_deref().unwrap_or(""))))
                .take(20)
                .collect()
        };
        let crash_cases = incidents(RunStatus::Crash);
        let timeout_cases = incidents(RunStatus::Timeout);
        let leaked = m.leaked();
        if leaked.is_empty() {
            notes.push("Isolamento: nenhuma sonda viu o ambiente nem arquivo do host (env no shell, env no jq, /etc/machine-id).".into());
        } else {
            notes.push(format!(
                "Isolamento: o script viu o host nas sondas {} (variável posta só no ambiente do processo host, ou /etc/machine-id do host).",
                leaked.join(", ")
            ));
            leak_lines.push(format!("{} ({})", m.krate, leaked.join(", ")));
        }
        if !crash_cases.is_empty() {
            notes.push(format!(
                "{} caso(s) derrubaram o processo do candidato (o embutidor junto): {}",
                runs.iter().filter(|r| r.status == RunStatus::Crash).count(),
                crash_cases.iter().take(5).cloned().collect::<Vec<_>>().join(" | ")
            ));
        }
        let slowest = runs.iter().map(|r| r.elapsed).max().unwrap_or_default();
        let tool_json = |m: &BTreeMap<String, Tally>| -> serde_json::Value {
            serde_json::Value::Object(m.iter().map(|(k, t)| (k.clone(), t.to_json())).collect())
        };
        table.insert(
            m.krate.to_string(),
            json!({
                "script_by_tool": tool_json(&by_tool_script),
                "argv_as_script_by_tool": tool_json(&by_tool_argv),
                "by_area_all": tool_json(&by_area_all),
                "by_area_script": tool_json(&by_area_script),
                "script_total": total_script.to_json(),
                "all_total": total_all.to_json(),
            }),
        );
        candidates.push(CandidateResult {
            name: m.krate.into(),
            version: m.version.into(),
            role: "baseline".into(),
            category,
            conformance: Some(conformance),
            fit: Fit::Reference,
            notes: notes.iter().map(|n| sanitize(n)).collect::<Vec<_>>().join(" "),
            metrics: json!({
                "depscan": dep_metrics,
                "binary_bytes": size,
                "elapsed_s": round3(m.elapsed.as_secs_f64()),
                "slowest_case_s": round3(slowest.as_secs_f64()),
                "process_starts": m.restarts,
                "crash_cases": crash_cases,
                "timeout_cases": timeout_cases,
                "host_leak_probes": m.leaks.clone(),
                "script_by_tool": tool_json(&by_tool_script),
                "argv_as_script_by_tool": tool_json(&by_tool_argv),
                "by_area_all": tool_json(&by_area_all),
                "shell_corpus_by_tag": tool_json(&shell_by_tag),
                "script_total": total_script.to_json(),
                "all_total": total_all.to_json(),
            }),
        });
    }

    let n_script = cases.iter().filter(|c| c.case.script.is_some()).count();
    let n_argv = cases.len() - n_script;
    let verdict = if !strong_areas.is_empty() {
        Verdict::Confirmed
    } else if !partial_areas.is_empty() {
        Verdict::Partial
    } else if measured.iter().all(|m| m.runs.is_none()) {
        Verdict::Inconclusive
    } else {
        Verdict::Refuted
    };
    let script_line = measured
        .iter()
        .filter_map(|m| {
            let runs = m.runs.as_ref()?;
            let mut t = Tally::default();
            for (gc, r) in cases.iter().zip(runs) {
                if gc.case.script.is_some() {
                    t.add(&r.cmp, r.status);
                }
            }
            Some(format!(
                "{} {:.0}%/{:.0}%",
                m.krate,
                Tally::rate(t.strict, t.total) * 100.0,
                Tally::rate(t.lenient, t.total) * 100.0
            ))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let best_line = best
        .as_ref()
        .map(|(r, c, a, n)| format!("melhor área: {c} em {a} com {:.0}% leniente (n={n})", r * 100.0))
        .unwrap_or_else(|| "nenhuma área com 20 casos ou mais".into());
    let summary = format!(
        "{n_script} casos script e {n_argv} argv (como linha de shell) de {} diretórios de golden. Estrito/leniente no conjunto script: {script_line}. {best_line}. Regra: Confirmada se alguma linha de base passa de 80% leniente numa área com 20 casos ou mais (adotável como base); Parcial entre 40% e 80% (cobre parte). Áreas acima de 80%: {}. Áreas entre 40% e 80%: {}. Vazamento do host nas sondas de isolamento: {}.",
        tools_present.len(),
        if strong_areas.is_empty() { "nenhuma".to_string() } else { strong_areas.join("; ") },
        if partial_areas.is_empty() { "nenhuma".to_string() } else { partial_areas.join("; ") },
        if leak_lines.is_empty() { "nenhum".to_string() } else { leak_lines.join("; ") }
    );
    let evidence = json!({
        "tools": tools_present,
        "cases_script": n_script,
        "cases_argv": n_argv,
        "golden_missing_cases": missing_golden,
        "areas": {
            "shell": ["shell", "smoke"],
            "coreutils": ["coreutils", "find", "xargs", "diff", "patch", "date"],
            "grep/sed/awk": ["grep", "sed", "awk"],
            "jq": ["jq", "yq"],
            "outros": "demais (sqlite, git...)"
        },
        "strong_areas": strong_areas,
        "partial_areas": partial_areas,
        "host_leaks": leak_lines,
        "table": table,
        "timeout_s": CASE_TIMEOUT.as_secs(),
        "workers_per_baseline": WORKERS,
        "elapsed_s": round3(started.elapsed().as_secs_f64()),
    });
    let mut notes = vec![
        format!(
            "H40: {} linhas de base, {} casos cada ({} script + {} argv como linha de shell), {:.0} s no total.",
            measured.len(),
            cases.len(),
            n_script,
            n_argv,
            started.elapsed().as_secs_f64()
        ),
        "stdout_exit = stdout e exit iguais ao golden, ignorando stderr e arquivos: mede o shell mesmo quando o VFS do candidato não modela modo, mtime ou symlink.".into(),
    ];
    if let Some(e) = &build_error {
        notes.push(format!("Falha de build das linhas de base: {}", sanitize(e)));
    }
    Ok(Section { hypothesis: "H40", verdict, summary: sanitize(&summary), evidence, candidates, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmp(strict: bool, lenient: bool, stdout_ok: bool, exit_ok: bool, unsupported: bool) -> CaseComparison {
        CaseComparison {
            id: "x".into(),
            tags: Vec::new(),
            stdout_ok,
            stderr_ok: strict,
            exit_ok,
            files_ok: lenient,
            strict,
            lenient,
            unsupported: unsupported.then(|| "timeout".to_string()),
            detail: vec!["stderr: a \u{2014} b".into()],
        }
    }

    #[test]
    fn tally_aggregates_by_tool_and_area() {
        let runs = [
            ("awk", CaseRun { cmp: cmp(true, true, true, true, false), status: RunStatus::Answered, elapsed: Duration::ZERO }),
            ("awk", CaseRun { cmp: cmp(false, true, true, true, false), status: RunStatus::Answered, elapsed: Duration::ZERO }),
            ("sed", CaseRun { cmp: cmp(false, false, true, true, false), status: RunStatus::Answered, elapsed: Duration::ZERO }),
            ("jq", CaseRun { cmp: cmp(false, false, false, false, true), status: RunStatus::Timeout, elapsed: Duration::ZERO }),
        ];
        let by_tool = tally_by(runs.iter().map(|(t, r)| (*t, r)), |t| t.to_string());
        assert_eq!(by_tool["awk"], Tally { total: 2, strict: 1, lenient: 2, stdout_exit: 2, ..Tally::default() });
        assert_eq!(by_tool["jq"].timeouts, 1);
        assert_eq!(by_tool["jq"].unsupported, 1);
        let by_area = tally_by(runs.iter().map(|(t, r)| (*t, r)), |t| area_of(t).to_string());
        assert_eq!(by_area["grep/sed/awk"].total, 3);
        assert_eq!(by_area["grep/sed/awk"].stdout_exit, 3);
        assert_eq!(by_area["jq"].total, 1);
    }

    #[test]
    fn sanitize_removes_dashes() {
        let c = sanitize_cmp(cmp(false, false, false, false, false));
        assert_eq!(c.detail[0], "stderr: a - b");
        assert!(!sanitize("x\u{2013}y\u{2014}z").contains(['\u{2013}', '\u{2014}']));
    }

    #[test]
    fn read_line_limited_splits_lines_and_eof() {
        let mut r = BufReader::new(&b"um\ndois\ntres"[..]);
        assert_eq!(read_line_limited(&mut r).unwrap().as_deref(), Some("um"));
        assert_eq!(read_line_limited(&mut r).unwrap().as_deref(), Some("dois"));
        assert_eq!(read_line_limited(&mut r).unwrap().as_deref(), Some("tres"));
        assert_eq!(read_line_limited(&mut r).unwrap(), None);
    }

    /// Fumaça dos binários (só quando já foram compilados: o teste não chama o cargo).
    #[test]
    fn baseline_binaries_answer_echo() {
        let case: Case = toml::from_str("id = \"echo\"\nscript = \"echo oi\"\n").unwrap();
        for (bin, _, _) in BASELINES {
            let path = bin_path(bin);
            if !path.exists() {
                continue;
            }
            let mut runner = ChildRunner::new(path);
            let (o, status) = runner.run(&case);
            assert_eq!(status, RunStatus::Answered, "{bin}: {:?}", o.unsupported);
            if *bin != "f15-baseline-kaish" {
                assert_eq!(o.stdout.as_slice(), b"oi\n", "{bin}");
                assert_eq!(o.exit, Some(0), "{bin}");
            }
        }
    }
}
