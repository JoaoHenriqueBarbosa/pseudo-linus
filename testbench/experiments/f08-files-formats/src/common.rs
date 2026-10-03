//! Utilitários compartilhados entre as partes do F08 (diff/patch, arquivos, date, diversos).
//!
//! Cada parte é um módulo com `pub fn run() -> anyhow::Result<Part>`. O binário principal roda todas e
//! monta `results/f08-files-formats.json`; os binários `part-*` rodam uma parte só (desenvolvimento) e
//! gravam o resultado parcial em `scratch/f08-files-formats/<parte>.json`.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use harness::{Bytes, Candidate, Case, CaseComparison, Conformance, Fit, HypothesisVerdict, Invocation, MemTree, Outcome};
use serde::{Deserialize, Serialize};

pub const EXPERIMENT: &str = "f08-files-formats";

/// Resultado de uma parte do experimento.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Part {
    /// Chave da parte nas métricas (ex.: "h30_diff").
    pub key: String,
    /// Vereditos de hipótese que a parte decide sozinha (H30, H31, H32). H33 é composta no main.
    pub hypotheses: Vec<HypothesisVerdict>,
    pub candidates: Vec<harness::CandidateResult>,
    /// Resumo por papel, usado pra compor vereditos que juntam várias partes (H33).
    pub roles: Vec<RoleSummary>,
    pub metrics: serde_json::Value,
    pub notes: Vec<String>,
}

/// Melhor resposta encontrada pra um papel (ex.: "bc", "file").
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoleSummary {
    pub role: String,
    /// Candidato escolhido, ou "fazer à mão".
    pub best: String,
    pub fit: Fit,
    /// Uma frase com o número que decidiu.
    pub summary: String,
}

/// Manifesto deste experimento (pro depscan).
pub fn manifest_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

/// Diretório de rascunho do experimento (gitignored).
pub fn scratch() -> PathBuf {
    harness::paths::scratch_dir(EXPERIMENT)
}

/// Resumo do depscan de uma crate, no formato que vai pro `metrics` do candidato.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DepSummary {
    pub package: String,
    pub version: String,
    /// Categoria da própria crate.
    pub own_category: String,
    /// Pior categoria da árvore.
    pub tree_category: String,
    /// Dependências que compilam ou linkam C (vazio = Rust puro).
    pub c_deps: Vec<String>,
    pub own_host_touch: usize,
    pub own_unsafe: usize,
    pub tree_deps: usize,
    pub tree_unsafe: usize,
    pub host_touching_deps: BTreeMap<String, usize>,
}

impl DepSummary {
    pub fn pure_rust(&self) -> bool {
        self.c_deps.is_empty()
    }
}

/// Roda o depscan na crate `package` dentro da árvore deste experimento.
pub fn dep_scan(package: &str) -> Result<DepSummary> {
    let scan = depscan::scan(&manifest_path(), package).with_context(|| format!("depscan {package}"))?;
    Ok(DepSummary {
        package: scan.root.name.clone(),
        version: scan.root.version.clone(),
        own_category: scan.root.category.letter().to_string(),
        tree_category: scan.tree_category.letter().to_string(),
        c_deps: scan.c_deps.clone(),
        own_host_touch: scan.root.counts.host_touch(),
        own_unsafe: scan.root.counts.unsafe_total(),
        tree_deps: scan.deps.len(),
        tree_unsafe: scan.totals.unsafe_total(),
        host_touching_deps: scan.host_touching_deps.clone(),
    })
}

/// Casos de uma ferramenta com golden. Falha se algum caso estiver sem golden (golden desatualizado).
pub fn load_cases(tool: &str) -> Result<Vec<(Case, Outcome)>> {
    let (cases, missing) = harness::paths::load_tool(tool)?;
    if missing > 0 {
        anyhow::bail!(
            "{tool}: {missing} casos sem golden; rode `cargo run -q -p oracle -- gen --tool {tool}` em testbench/"
        );
    }
    Ok(cases)
}

/// Pontua um candidato e grava a comparação caso a caso em `scratch/f08-files-formats/<arquivo>.json`.
pub fn score_and_dump(candidate: &dyn Candidate, cases: &[(Case, Outcome)], dump_name: &str) -> Conformance {
    let (conf, all) = harness::score(candidate, cases);
    let path = scratch().join(format!("{dump_name}.json"));
    let failures: Vec<&CaseComparison> = all.iter().filter(|c| !c.strict).collect();
    if let Ok(text) = serde_json::to_string_pretty(&failures) {
        let _ = std::fs::write(path, text);
    }
    conf
}

/// Tempo médio por iteração, repetindo até somar pelo menos `min_total` (e no mínimo 3 vezes).
pub fn time_per_iter(min_total: Duration, mut f: impl FnMut()) -> Duration {
    f(); // aquecimento
    let start = Instant::now();
    let mut iters = 0u32;
    while iters < 3 || start.elapsed() < min_total {
        f();
        iters += 1;
    }
    start.elapsed() / iters
}

/// MB/s (10^6 bytes) dado o tamanho e o tempo por iteração.
pub fn mb_per_s(bytes: usize, per_iter: Duration) -> f64 {
    let secs = per_iter.as_secs_f64();
    if secs == 0.0 { f64::INFINITY } else { bytes as f64 / 1e6 / secs }
}

/// Lê um arquivo da fixture, resolvendo caminho relativo ao diretório do caso.
pub fn fixture_file<'a>(inv: &'a Invocation, path: &str) -> Option<&'a [u8]> {
    inv.files.read(&relative(path))
}

/// Normaliza um caminho do caso pra chave da MemTree ("./a/b" e "/work/case/a/b" viram "a/b").
pub fn relative(path: &str) -> String {
    let p = path.strip_prefix(harness::CASE_DIR).unwrap_or(path);
    p.split('/').filter(|s| !s.is_empty() && *s != ".").collect::<Vec<_>>().join("/")
}

static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Roda um binário externo (ex.: um fork compilado) num diretório temporário com a fixture do caso,
/// o mesmo ambiente do oráculo e mtime fixo. `argv[0]` do caso é trocado por `program`.
pub fn run_external(program: &Path, inv: &Invocation) -> Outcome {
    match run_external_inner(program, inv) {
        Ok(out) => out,
        Err(e) => Outcome::unsupported(format!("run_external: {e:#}")),
    }
}

fn run_external_inner(program: &Path, inv: &Invocation) -> Result<Outcome> {
    let n = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = scratch().join(format!("run-{}-{n}", std::process::id())).join("case");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(harness::FIXTURE_MTIME);
    inv.files.materialize(&dir, mtime)?;
    let mut cmd = Command::new(program);
    cmd.args(inv.args())
        .current_dir(&dir)
        .env_clear()
        .envs(inv.full_env())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().with_context(|| format!("spawn {}", program.display()))?;
    let mut stdin = child.stdin.take().expect("stdin");
    let input = inv.stdin.clone();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let mut out_pipe = child.stdout.take().expect("stdout");
    let mut err_pipe = child.stderr.take().expect("stderr");
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out_pipe.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err_pipe.read_to_end(&mut buf);
        buf
    });
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(20) {
            timed_out = true;
            let _ = child.kill();
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let _ = writer.join();
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    let files = MemTree::capture(&dir)?;
    let _ = std::fs::remove_dir_all(dir.parent().expect("pai"));
    use std::os::unix::process::ExitStatusExt;
    Ok(Outcome {
        stdout: Bytes(stdout),
        stderr: Bytes(stderr),
        exit: status.code(),
        signal: status.signal(),
        timed_out,
        files,
        unsupported: None,
    })
}

/// Usado pelos binários `part-*`: roda uma parte, imprime o resumo e grava o JSON parcial.
pub fn run_part_bin(run: fn() -> Result<Part>) -> Result<()> {
    let started = Instant::now();
    let part = run()?;
    let path = scratch().join(format!("{}.json", part.key));
    std::fs::write(&path, serde_json::to_string_pretty(&part)? + "\n")?;
    print_part(&part);
    println!("parte {} em {:.1}s, gravada em {}", part.key, started.elapsed().as_secs_f64(), path.display());
    Ok(())
}

pub fn print_part(part: &Part) {
    for h in &part.hypotheses {
        println!("{} {}: {}", h.id, h.verdict.label_pt(), h.summary);
    }
    for c in &part.candidates {
        let conf = c
            .conformance
            .as_ref()
            .map(|c| format!(" strict {}/{} lenient {}/{}", c.strict_pass, c.total, c.lenient_pass, c.total))
            .unwrap_or_default();
        println!("  [{}] {} {} ({:?}){conf}", c.role, c.name, c.version, c.fit);
    }
    for r in &part.roles {
        println!("  papel {}: {} ({:?}) {}", r.role, r.best, r.fit, r.summary);
    }
}
