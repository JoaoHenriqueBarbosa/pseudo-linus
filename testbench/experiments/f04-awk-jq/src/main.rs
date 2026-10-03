//! F04: awk e jq. Ver README.md.
//!
//! Uso:
//! - `cargo run --release` roda tudo e grava `results/f04-awk-jq.json`;
//! - o executável chamado com o nome `jq` (symlink) é o jq nosso sobre o jaq;
//! - `f04-awk-jq jq ARGS...` faz o mesmo sem symlink;
//! - `f04-awk-jq probe NOME` e `f04-awk-jq score-inproc CHAVE` são usados em subprocesso pelo próprio
//!   run (sondagens de checkpoint e candidatos em processo, isolados pra que um abort não derrube a
//!   bancada).

mod awk_inproc;
mod bashkit_cand;
mod checkpoint;
mod exec;
mod gawk_suite;
mod jq;
mod jqtest;
mod scan;
mod tools;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use harness::{Candidate, CandidateResult, Case, CaseComparison, ExperimentResult, Fit, Outcome, Verdict};
use serde::{Deserialize, Serialize};
use serde_json::json;

const EXPERIMENT: &str = "f04-awk-jq";

fn basename(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let argv0 = args.first().map(|a| basename(a).to_string()).unwrap_or_default();
    if argv0 == "jq" {
        return jq::cli::main_std(&args[1..]);
    }
    let r = match args.get(1).map(String::as_str) {
        Some("jq") => return jq::cli::main_std(&args[2..]),
        Some("probe") => return checkpoint::probe_main(&args[2..]),
        Some("score-inproc") => score_inproc_main(args.get(2).map(String::as_str).unwrap_or("")),
        None | Some("run") => run_all(),
        Some(other) => Err(anyhow::anyhow!("subcomando desconhecido: {other}")),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[f04] erro: {e:#}");
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Corpora
// ---------------------------------------------------------------------------------------------

struct Corpora {
    awk: Vec<(Case, Outcome)>,
    jq: Vec<(Case, Outcome)>,
    gawk: Vec<(Case, Outcome)>,
    jq_tests: Vec<jqtest::JqTest>,
    meta: serde_json::Value,
}

const JQ_SUITES: &[&str] = &["jq.test", "man.test", "onig.test", "manonig.test", "base64.test", "optional.test"];

fn jq_upstream_dir() -> PathBuf {
    harness::paths::corpus_dir().join("upstream").join("jq")
}

fn ensure_jq_suites() -> Result<Vec<(String, String)>> {
    let dir = jq_upstream_dir();
    std::fs::create_dir_all(&dir)?;
    let mut out = Vec::new();
    for name in JQ_SUITES {
        let path = dir.join(name);
        if !path.exists() {
            let url = format!("https://raw.githubusercontent.com/jqlang/jq/jq-1.7.1/tests/{name}");
            let st = Command::new("curl").args(["-sSfL", "-o"]).arg(&path).arg(&url).status()?;
            if !st.success() {
                bail!("download de {url} falhou");
            }
        }
        out.push((name.to_string(), std::fs::read_to_string(&path)?));
    }
    Ok(out)
}

fn prepare(scratch: &Path) -> Result<Corpora> {
    let (awk, awk_missing) = harness::paths::load_tool("awk")?;
    let (jq_cases, jq_missing) = harness::paths::load_tool("jq")?;
    if awk.is_empty() || jq_cases.is_empty() {
        bail!("sem golden de awk ou jq; rode `cargo run -q -p oracle -- gen --tool awk` (e jq)");
    }

    let gdir = gawk_suite::ensure_upstream()?;
    let selection = gawk_suite::select(&gdir)?;
    let gcases = gawk_suite::build_cases(&gdir, &selection.selected)?;
    let built = gcases.len();
    let (gawk, gawk_mismatch) = gawk_suite::validate(&gdir, gcases, &scratch.join("gawk-oracle-cache.json"))?;

    let files = ensure_jq_suites()?;
    let runs = jqtest::oracle_runs(&files, &scratch.join("jq-oracle-cache.json"))?;
    let mut jq_tests = Vec::new();
    let mut per_file = BTreeMap::new();
    for (name, text) in &files {
        let parsed = jqtest::parse(name, text);
        let failing = runs.get(name).map(|r| r.failing_lines.clone()).unwrap_or_default();
        let total = parsed.len();
        let mut excluded = 0;
        for t in parsed {
            if failing.contains(&t.line) {
                excluded += 1;
            } else {
                jq_tests.push(t);
            }
        }
        per_file.insert(
            name.clone(),
            json!({"parsed": total, "oracle_failing": excluded, "used": total - excluded,
                   "oracle_summary": runs.get(name).map(|r| r.summary.clone()).unwrap_or_default()}),
        );
    }

    let meta = json!({
        "awk_agent_cases": awk.len(),
        "awk_agent_cases_without_golden": awk_missing,
        "jq_agent_cases": jq_cases.len(),
        "jq_agent_cases_without_golden": jq_missing,
        "gawk_suite": {
            "version": gawk_suite::VERSION,
            "listed": selection.listed,
            "selected_by_rule": selection.selected.len(),
            "built": built,
            "oracle_reproduces_ok": gawk.len(),
            "oracle_mismatch": gawk_mismatch,
            "excluded": selection.excluded.iter().map(|(k, v)| (k.clone(), v.len())).collect::<BTreeMap<_, _>>(),
        },
        "jq_suites": per_file,
        "jq_tests_used": jq_tests.len(),
    });
    Ok(Corpora { awk, jq: jq_cases, gawk, jq_tests, meta })
}

// ---------------------------------------------------------------------------------------------
// Candidatos
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Awk,
    Jq,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::Awk => "awk",
            Role::Jq => "jq",
        }
    }
}

enum How {
    Bin { tool: &'static str, prefix: &'static [&'static str] },
    /// Binário com o nome do programa trocado no stderr (variante de medida, não candidato novo).
    BinRenamed { tool: &'static str, from: &'static str },
    InProc { key: &'static str },
}

struct Desc {
    name: String,
    version: String,
    role: Role,
    how: How,
    /// (manifesto, pacote) pro depscan.
    scan: Option<(PathBuf, String)>,
    baseline: bool,
    blurb: &'static str,
}

fn own_manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

fn descriptors() -> Vec<Desc> {
    let tool_scan = |key: &str| {
        let spec = tools::spec(key);
        tools::manifest_for(spec).map(|m| (m, spec.crate_name.to_string()))
    };
    let own = |pkg: &str| Some((own_manifest(), pkg.to_string()));
    vec![
        Desc {
            name: "uutils/awk".into(),
            version: tools::spec("uutils-awk").version.into(),
            role: Role::Awk,
            how: How::Bin { tool: "uutils-awk", prefix: &[] },
            scan: tool_scan("uutils-awk"),
            baseline: false,
            blurb: "reimplementação do gawk do projeto uutils (não publicada); VM sans-IO que suspende em toda E/S",
        },
        Desc {
            name: "awk-rs".into(),
            version: "0.2.0".into(),
            role: Role::Awk,
            how: How::Bin { tool: "awk-rs", prefix: &[] },
            scan: tool_scan("awk-rs"),
            baseline: false,
            blurb: "interpretador de árvore; biblioteca recebe BufRead/Write, mas arquivos e pipes vão direto pro std::fs/process",
        },
        Desc {
            name: "awkrs".into(),
            version: "0.5.6".into(),
            role: Role::Awk,
            how: How::Bin { tool: "awkrs", prefix: &[] },
            scan: tool_scan("awkrs"),
            baseline: false,
            blurb: "VM com JIT Cranelift, GMP/MPFR, LSP, DAP, reedline; mira compatibilidade com o gawk",
        },
        Desc {
            name: "frawk (-B interp)".into(),
            version: "0.4.8".into(),
            role: Role::Awk,
            how: How::Bin { tool: "frawk", prefix: &["-B", "interp"] },
            scan: tool_scan("frawk"),
            baseline: false,
            blurb: "linguagem \"awk-like\" com inferência de tipos; backend de bytecode",
        },
        Desc {
            name: "frawk (cranelift, padrão)".into(),
            version: "0.4.8".into(),
            role: Role::Awk,
            how: How::Bin { tool: "frawk", prefix: &[] },
            scan: None,
            baseline: false,
            blurb: "mesmo frawk com o backend padrão (JIT Cranelift 0.93)",
        },
        Desc {
            name: "zawk".into(),
            version: "0.5.25".into(),
            role: Role::Awk,
            how: How::Bin { tool: "zawk", prefix: &[] },
            scan: tool_scan("zawk"),
            baseline: false,
            blurb: "fork do frawk com biblioteca padrão enorme (MySQL, NATS, MQTT, S3...)",
        },
        Desc {
            name: "rawk-core".into(),
            version: "0.6.0".into(),
            role: Role::Awk,
            how: How::InProc { key: "rawk-core" },
            scan: own("rawk-core"),
            baseline: false,
            blurb: "biblioteca pura: recebe linhas, devolve linhas; sem -v, sem vários arquivos, sem saída parcial",
        },
        Desc {
            name: "bashkit awk".into(),
            version: "0.18.2".into(),
            role: Role::Awk,
            how: How::InProc { key: "bashkit-awk" },
            scan: own("bashkit"),
            baseline: true,
            blurb: "awk próprio do bashkit sobre VFS em memória (linha de base)",
        },
        Desc {
            name: "jaq-core + jaq-std + jaq-json com camada nossa".into(),
            version: "3.1.1 / 3.0.3 / 2.0.3".into(),
            role: Role::Jq,
            how: How::InProc { key: "jaq-ours" },
            scan: own("jaq-core"),
            baseline: false,
            blurb: "núcleo do jaq com CLI, leitor/escritor JSON, números, mensagens, defs do jq e checkpoint nossos",
        },
        Desc {
            name: "jaq (binário)".into(),
            version: "3.1.1".into(),
            role: Role::Jq,
            how: How::Bin { tool: "jaq", prefix: &[] },
            scan: tool_scan("jaq"),
            baseline: true,
            blurb: "o CLI oficial do jaq, como referência do que o projeto entrega sem camada nossa",
        },
        Desc {
            name: "xq".into(),
            version: "0.5.0".into(),
            role: Role::Jq,
            how: How::Bin { tool: "xq", prefix: &[] },
            scan: tool_scan("xq"),
            baseline: false,
            blurb: "reimplementação do jq em VM própria, regex via Oniguruma (C)",
        },
        Desc {
            name: "qj".into(),
            version: "0.2.1".into(),
            role: Role::Jq,
            how: How::Bin { tool: "qj", prefix: &[] },
            scan: tool_scan("qj"),
            baseline: false,
            blurb: "processador \"compatível com jq\" sobre simdjson (C++)",
        },
        Desc {
            name: "qj (nome do programa normalizado)".into(),
            version: "0.2.1".into(),
            role: Role::Jq,
            how: How::BinRenamed { tool: "qj", from: "qj" },
            scan: None,
            baseline: true,
            blurb: "o mesmo binário do qj com \"qj:\" trocado por \"jq:\" no stderr, pra separar nome de programa de comportamento",
        },
        Desc {
            name: "tq (modo JSON)".into(),
            version: "0.3.0".into(),
            role: Role::Jq,
            how: How::Bin {
                tool: "tq",
                prefix: &["-i", "json", "-o", "json", "--allow-environment", "--allow-platform"],
            },
            scan: tool_scan("tq"),
            baseline: false,
            blurb: "processador de TOON \"compatível com jq\"; forçado a ler e escrever JSON",
        },
        Desc {
            name: "bashkit jq".into(),
            version: "0.18.2".into(),
            role: Role::Jq,
            how: How::InProc { key: "bashkit-jq" },
            scan: own("bashkit"),
            baseline: true,
            blurb: "jq do bashkit: jaq-core com a camada de compatibilidade dele (linha de base)",
        },
    ]
}

fn inproc_candidate(key: &str, scratch: &Path) -> Result<(Role, Box<dyn Candidate + Sync>)> {
    Ok(match key {
        "rawk-core" => (Role::Awk, Box::new(awk_inproc::RawkCore)),
        "bashkit-awk" => (Role::Awk, Box::new(bashkit_cand::Bashkit::new("awk"))),
        "bashkit-jq" => (Role::Jq, Box::new(bashkit_cand::Bashkit::new("jq"))),
        "jaq-ours" => (Role::Jq, Box::new(jq::JaqOurs::new("jaq-ours", scratch)?)),
        other => bail!("candidato em processo desconhecido: {other}"),
    })
}

#[derive(Default, Serialize, Deserialize)]
struct CandScore {
    agent: Vec<CaseComparison>,
    upstream: Vec<CaseComparison>,
    jqtest: Vec<jqtest::TestResult>,
    elapsed_ms: f64,
}

fn score_candidate(cand: &(dyn Candidate + Sync), role: Role, c: &Corpora) -> CandScore {
    let start = Instant::now();
    let threads = exec::threads();
    let mut s = CandScore::default();
    match role {
        Role::Awk => {
            s.agent = exec::score_parallel(cand, &c.awk, threads).1;
            s.upstream = exec::score_parallel(cand, &c.gawk, threads).1;
        }
        Role::Jq => {
            s.agent = exec::score_parallel(cand, &c.jq, threads).1;
            s.jqtest = exec::par_map(&c.jq_tests, threads, |t| jqtest::run_test(cand, t));
        }
    }
    s.elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    s
}

/// Filho: pontua um candidato em processo e imprime o JSON.
fn score_inproc_main(key: &str) -> Result<()> {
    let scratch = harness::paths::scratch_dir(EXPERIMENT);
    let corpora = prepare(&scratch)?;
    let (role, cand) = inproc_candidate(key, &scratch)?;
    let s = score_candidate(cand.as_ref(), role, &corpora);
    let path = scratch.join(format!("score-{key}.json"));
    std::fs::write(&path, serde_json::to_vec(&s)?)?;
    println!("{}", path.display());
    Ok(())
}

/// Pai: roda o filho e lê o resultado; um abort do candidato vira resultado, não queda da bancada.
fn score_inproc_isolated(key: &str) -> Result<CandScore, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(exe);
    cmd.arg("score-inproc").arg(key).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let out = exec::spawn_and_wait(cmd, &[], Duration::from_secs(900), Path::new("."))
        .map_err(|e| format!("{e:#}"))?;
    if out.timed_out {
        return Err("o processo de pontuação estourou 15 minutos".into());
    }
    if out.exit != Some(0) {
        return Err(format!(
            "o processo de pontuação morreu (exit {:?}, sinal {:?}): {}",
            out.exit,
            out.signal,
            out.stderr.preview(300)
        ));
    }
    let path = String::from_utf8_lossy(out.stdout.as_slice()).trim().to_string();
    let data = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_slice(&data).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------------------------
// Agregação
// ---------------------------------------------------------------------------------------------

fn rate(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { a as f64 / b as f64 }
}

fn pct(x: f64) -> String {
    format!("{:.1}%", x * 100.0)
}

/// Classe de divergência de um caso de agente, pelas tags.
fn agent_class(role: Role, tags: &[String]) -> &'static str {
    let has = |t: &str| tags.iter().any(|x| x == t);
    match role {
        Role::Jq => {
            if has("errors") || has("compile") {
                "erros/mensagens"
            } else if has("numbers") || has("math") {
                "números"
            } else if has("regex") {
                "regex"
            } else if has("dates") {
                "datas"
            } else if has("sort") {
                "ordenação"
            } else if has("cli") || has("format") || has("vars") || has("exit") || has("inputs") || has("input") || has("io") {
                "CLI"
            } else if has("builtins-set") {
                "conjunto de builtins"
            } else {
                "semântica (caminhos, geradores, atualização)"
            }
        }
        Role::Awk => {
            if has("errors") {
                "erros/mensagens"
            } else if has("posix-semantics") || has("regex") {
                "regex"
            } else if has("printf") || has("numeric") || has("strnum") || has("output-format") {
                "números e printf"
            } else if has("gawk-ext") || has("time") {
                "extensões gawk"
            } else if has("io") || has("getline") || has("pipes") || has("pipeline") {
                "E/S e pipes"
            } else if has("cli") {
                "CLI"
            } else if has("for-in-order") || has("arrays") {
                "arrays"
            } else {
                "campos, registros e controle"
            }
        }
    }
}

fn class_failures(role: Role, cmps: &[CaseComparison]) -> BTreeMap<String, (usize, usize)> {
    let mut m: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for c in cmps {
        let slot = m.entry(agent_class(role, &c.tags).to_string()).or_default();
        slot.1 += 1;
        if !c.lenient {
            slot.0 += 1;
        }
    }
    m
}

struct Aggregated {
    result: CandidateResult,
    /// Taxa estrita combinada (awk: agente + gawk; jq: agente).
    strict: f64,
    lenient: f64,
    jqtest_rate: Option<f64>,
}

fn aggregate(d: &Desc, score: &CandScore, scan: Option<&scan::ScanSummary>, scan_err: Option<&str>) -> Aggregated {
    let agent = exec::conformance_of(&d.name, &score.agent);
    let mut metrics = serde_json::Map::new();
    metrics.insert("elapsed_ms".into(), json!(score.elapsed_ms));
    metrics.insert(
        "agent_cases".into(),
        json!({"total": agent.total, "strict": agent.strict_pass, "lenient": agent.lenient_pass,
               "unsupported": agent.unsupported, "strict_rate": agent.strict_rate(), "lenient_rate": agent.lenient_rate()}),
    );
    metrics.insert("agent_failures_by_class".into(), json!(class_failures(d.role, &score.agent)));
    let mut timeouts = 0;
    let mut panics = 0;
    for c in score.agent.iter().chain(&score.upstream) {
        if c.detail.iter().any(|x| x.contains("panic")) {
            panics += 1;
        }
        if c.unsupported.as_deref().is_some_and(|u| u.contains("timeout")) {
            timeouts += 1;
        }
    }
    metrics.insert("panics".into(), json!(panics));
    let _ = timeouts;
    let (conf, strict, lenient, jqtest_rate);
    match d.role {
        Role::Awk => {
            let up = exec::conformance_of(&d.name, &score.upstream);
            let output_only: Vec<&CaseComparison> =
                score.upstream.iter().filter(|c| c.tags.iter().any(|t| t == "output-only")).collect();
            let oo_pass = output_only.iter().filter(|c| c.strict).count();
            metrics.insert(
                "gawk_suite".into(),
                json!({"total": up.total, "strict": up.strict_pass, "lenient": up.lenient_pass,
                       "strict_rate": up.strict_rate(), "lenient_rate": up.lenient_rate(),
                       "output_only_total": output_only.len(), "output_only_strict": oo_pass}),
            );
            let mut all = score.agent.clone();
            all.extend(score.upstream.iter().cloned());
            let combined = exec::conformance_of(&d.name, &all);
            strict = combined.strict_rate();
            lenient = combined.lenient_rate();
            metrics.insert(
                "combined".into(),
                json!({"total": combined.total, "strict": combined.strict_pass, "lenient": combined.lenient_pass,
                       "strict_rate": strict, "lenient_rate": lenient}),
            );
            conf = combined;
            jqtest_rate = None;
        }
        Role::Jq => {
            let total = score.jqtest.len();
            let pass = score.jqtest.iter().filter(|t| t.pass).count();
            let len_pass = score.jqtest.iter().filter(|t| t.pass_lenient).count();
            let mut by_file: BTreeMap<String, (usize, usize)> = BTreeMap::new();
            let mut by_class: BTreeMap<String, (usize, usize)> = BTreeMap::new();
            for t in &score.jqtest {
                let f = by_file.entry(t.file.clone()).or_default();
                f.1 += 1;
                f.0 += t.pass as usize;
                let c = by_class.entry(t.class.clone()).or_default();
                c.1 += 1;
                if !t.pass {
                    c.0 += 1;
                }
            }
            let samples: Vec<serde_json::Value> = score
                .jqtest
                .iter()
                .filter(|t| !t.pass)
                .take(25)
                .map(|t| json!({"file": t.file, "line": t.line, "program": t.program, "class": t.class, "detail": t.detail}))
                .collect();
            jqtest_rate = Some(rate(pass, total));
            metrics.insert(
                "jq_test_suites".into(),
                json!({"total": total, "pass": pass, "pass_rate": rate(pass, total),
                       "pass_lenient": len_pass, "pass_lenient_rate": rate(len_pass, total),
                       "by_file_pass_total": by_file, "failures_by_class_fail_total": by_class,
                       "sample_failures": samples}),
            );
            strict = agent.strict_rate();
            lenient = agent.lenient_rate();
            conf = agent.clone();
        }
    }
    if let Some(s) = scan {
        metrics.insert("depscan".into(), json!(s));
    }
    if let Some(e) = scan_err {
        metrics.insert("depscan_error".into(), json!(e));
    }
    let category = scan.map(|s| s.tree_category_refined.clone());
    let kind = match d.how {
        How::Bin { .. } | How::BinRenamed { .. } => {
            "medido como binário em subprocesso, com a fixture copiada pro scratch"
        }
        How::InProc { .. } => "medido em processo, sobre a fixture em memória",
    };
    let (fit, verdict_note) = decide_fit(d, strict, lenient, jqtest_rate, category.as_deref());
    let notes = format!(
        "{}. {kind}. Casos estritos {} (frouxos {}){}. {verdict_note}",
        d.blurb,
        pct(strict),
        pct(lenient),
        jqtest_rate.map(|r| format!(", suítes do jq {}", pct(r))).unwrap_or_default()
    );
    Aggregated {
        result: CandidateResult {
            name: d.name.clone(),
            version: d.version.clone(),
            role: d.role.as_str().into(),
            category,
            conformance: Some(conf),
            fit,
            notes,
            metrics: serde_json::Value::Object(metrics),
        },
        strict,
        lenient,
        jqtest_rate,
    }
}

/// Regra de encaixe (documentada no README): conformidade e categoria de acoplamento.
fn decide_fit(d: &Desc, strict: f64, lenient: f64, jqtest: Option<f64>, category: Option<&str>) -> (Fit, String) {
    if d.baseline {
        return (Fit::Reference, "Linha de base ou referência, não dependência.".into());
    }
    let cat = category.unwrap_or("?");
    match d.role {
        Role::Awk => {
            if strict >= 0.9 && cat == "a" {
                (Fit::Fits, "Encaixa como está.".into())
            } else if strict >= 0.8 {
                (Fit::FitsWithWork, format!("Passa de 80% estrito, mas é categoria ({cat}): exige fork pro Ctx."))
            } else {
                (Fit::DoesNotFit, format!("Abaixo de 80% estrito ({}), categoria ({cat}).", pct(strict)))
            }
        }
        Role::Jq => {
            let jt = jqtest.unwrap_or(0.0);
            if jt >= 0.95 && lenient >= 0.95 && cat == "a" {
                (Fit::Fits, "Encaixa como está.".into())
            } else if jt >= 0.8 && lenient >= 0.85 {
                (
                    Fit::FitsWithWork,
                    format!(
                        "Suítes do jq {} e CLI frouxo {}: encaixa com trabalho (camada ou fork), categoria ({cat}).",
                        pct(jt),
                        pct(lenient)
                    ),
                )
            } else {
                (Fit::DoesNotFit, format!("Suítes do jq {}, CLI frouxo {}, categoria ({cat}).", pct(jt), pct(lenient)))
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Execução completa
// ---------------------------------------------------------------------------------------------

fn run_all() -> Result<()> {
    let t0 = Instant::now();
    let scratch = harness::paths::scratch_dir(EXPERIMENT);
    let mut notes: Vec<String> = Vec::new();

    eprintln!("[f04] ferramentas");
    let mut bins: BTreeMap<&str, Result<PathBuf, String>> = BTreeMap::new();
    for spec in tools::TOOLS {
        bins.insert(spec.key, tools::ensure(&scratch, spec).map_err(|e| format!("{e:#}")));
    }

    eprintln!("[f04] corpora (golden, suíte do gawk, suítes do jq)");
    let corpora = prepare(&scratch)?;
    eprintln!(
        "[f04] awk: {} casos de agente, {} da suíte do gawk; jq: {} casos de CLI, {} testes das suítes",
        corpora.awk.len(),
        corpora.gawk.len(),
        corpora.jq.len(),
        corpora.jq_tests.len()
    );

    let descs = descriptors();

    eprintln!("[f04] depscan");
    let scan_items: Vec<(String, PathBuf, String)> = descs
        .iter()
        .filter_map(|d| d.scan.clone().map(|(m, p)| (d.name.clone(), m, p)))
        .collect();
    let mut scans = scan::scan_all(scan_items, &scratch.join("depscan-cache.json"));
    // A camada nossa sobre o jaq usa as três crates: a categoria é a pior das três árvores.
    let jaq_parts = scan::scan_all(
        ["jaq-std", "jaq-json"].iter().map(|p| (p.to_string(), own_manifest(), p.to_string())).collect(),
        &scratch.join("depscan-cache.json"),
    );

    let mut aggregated: Vec<(Role, Aggregated)> = Vec::new();
    let mut raw_scores: BTreeMap<String, CandScore> = BTreeMap::new();
    for d in &descs {
        eprintln!("[f04] candidato {} ({})", d.name, d.role.as_str());
        let score: Result<CandScore, String> = match &d.how {
            How::Bin { tool, prefix } => match bins.get(tool).expect("ferramenta") {
                Ok(bin) => {
                    let aliases: &[&str] = if d.role == Role::Awk { &["awk", "gawk"] } else { &["jq"] };
                    match exec::SubprocessCandidate::new(&d.name, bin, prefix, aliases, &scratch) {
                        Ok(c) => Ok(score_candidate(&c, d.role, &corpora)),
                        Err(e) => Err(format!("{e:#}")),
                    }
                }
                Err(e) => Err(format!("não compilou: {e}")),
            },
            How::BinRenamed { tool, from } => match bins.get(tool).expect("ferramenta") {
                Ok(bin) => match exec::SubprocessCandidate::new(&d.name, bin, &[], &["jq"], &scratch) {
                    Ok(mut c) => {
                        c.rename_prog = Some((from.to_string(), "jq".to_string()));
                        Ok(score_candidate(&c, d.role, &corpora))
                    }
                    Err(e) => Err(format!("{e:#}")),
                },
                Err(e) => Err(format!("não compilou: {e}")),
            },
            How::InProc { key } => score_inproc_isolated(key),
        };
        let mut scan_entry = scans.remove(&d.name);
        if matches!(d.how, How::InProc { key: "jaq-ours" })
            && let Some(Ok(core)) = &mut scan_entry
        {
            for part in jaq_parts.values().flatten() {
                if part.tree_category > core.tree_category {
                    core.tree_category = part.tree_category.clone();
                }
                if part.tree_category_refined > core.tree_category_refined {
                    core.tree_category_refined = part.tree_category_refined.clone();
                }
                core.c_deps.extend(part.c_deps.iter().cloned());
                core.c_deps_refined.extend(part.c_deps_refined.iter().cloned());
                core.own_host_touch += part.own_host_touch;
            }
            core.c_deps.sort();
            core.c_deps.dedup();
            core.c_deps_refined.sort();
            core.c_deps_refined.dedup();
        }
        let (scan_ok, scan_err) = match &scan_entry {
            Some(Ok(s)) => (Some(s), None),
            Some(Err(e)) => (None, Some(e.as_str())),
            None => (None, None),
        };
        match score {
            Ok(s) => {
                let a = aggregate(d, &s, scan_ok, scan_err);
                eprintln!(
                    "[f04]   estrito {} frouxo {}{} em {:.1}s",
                    pct(a.strict),
                    pct(a.lenient),
                    a.jqtest_rate.map(|r| format!(", suítes do jq {}", pct(r))).unwrap_or_default(),
                    s.elapsed_ms / 1000.0
                );
                raw_scores.insert(d.name.clone(), s);
                aggregated.push((d.role, a));
            }
            Err(e) => {
                eprintln!("[f04]   falhou: {e}");
                let mut metrics = serde_json::Map::new();
                if let Some(s) = scan_ok {
                    metrics.insert("depscan".into(), json!(s));
                }
                metrics.insert("error".into(), json!(e));
                aggregated.push((
                    d.role,
                    Aggregated {
                        result: CandidateResult {
                            name: d.name.clone(),
                            version: d.version.clone(),
                            role: d.role.as_str().into(),
                            category: scan_ok.map(|s| s.tree_category.clone()),
                            conformance: None,
                            fit: Fit::DoesNotFit,
                            notes: format!("{}. Não foi possível medir: {e}", d.blurb),
                            metrics: serde_json::Value::Object(metrics),
                        },
                        strict: 0.0,
                        lenient: 0.0,
                        jqtest_rate: None,
                    },
                ));
            }
        }
    }

    eprintln!("[f04] sondagens de checkpoint");
    let probes = checkpoint::run_all();
    for p in &probes {
        eprintln!(
            "[f04]   {}: interrompida={} morta={} latência={:?}us",
            p.name, p.interrupted, p.killed, p.latency_us
        );
    }
    let overhead = checkpoint::hook_overhead();
    // Evidência pro H07 (laço de CPU sem checkpoint): quem cede e quem só morre matando o processo.
    let h07: Vec<serde_json::Value> = probes
        .iter()
        .map(|p| {
            let outcome = if p.interrupted {
                "interrompido pelo checkpoint, sem matar o processo"
            } else if p.killed {
                "não cede: só parou matando o processo"
            } else {
                "terminou sozinho (limite do próprio candidato ou laço finito)"
            };
            json!({"probe": p.name, "subject": p.subject, "program": p.program, "outcome": outcome,
                   "latency_us": p.latency_us, "max_gap_us": p.max_gap_us})
        })
        .collect();

    // Builtins visíveis: jq 1.7.1 do oráculo contra jaq puro e jaq com a camada.
    let builtins = builtin_diff(&scratch);

    // Detalhes caso a caso ficam no scratch (gitignored), pra investigação.
    std::fs::write(scratch.join("details.json"), serde_json::to_vec_pretty(&raw_scores)?)?;

    let mut result = ExperimentResult::new(EXPERIMENT, "awk e jq em Rust: conformidade com gawk 5.2.1 e jq 1.7.1, e checkpoint");
    verdicts(&mut result, &aggregated, &probes, &corpora, &raw_scores);
    result.candidates = aggregated.into_iter().map(|(_, a)| a.result).collect();
    result.metrics = json!({
        "corpora": corpora.meta,
        "checkpoint_probes": probes,
        "h07_evidence": h07,
        "jaq_checkpoint_overhead": overhead,
        "jq_builtins": builtins,
        "host_tools": {"faketime_on_host": false},
        "installed_binaries": tools::TOOLS.iter().map(|t| json!({
            "key": t.key, "crate": t.crate_name, "version": t.version, "repo": t.repo,
            "install": format!("cargo install {}", t.install_args.join(" ")),
            "ok": bins.get(t.key).is_some_and(|b| b.is_ok()),
        })).collect::<Vec<_>>(),
        "elapsed_s": t0.elapsed().as_secs_f64(),
    });
    notes.push(
        "Casos com faketime (2 no total) ficam como não suportados nos candidatos binários: o host não tem \
         libfaketime. O jq nosso e o bashkit recebem o relógio injetado."
            .into(),
    );
    notes.push(
        "A suíte do gawk e as do jq ficam em corpus/upstream (gitignored); só os números entram aqui.".into(),
    );
    notes.push(
        "Pontos de host do jaq (depscan b): jaq-core Loader::with_std_read (std::fs, std::env, só pra módulos, \
         não usado pela camada); jaq-std env (std::env::vars), now (SystemTime), stderr/debug (log), e as nativas \
         de fuso local via jiff (TimeZone::system). A camada sobrescreve env, now, stderr, debug, input*; \
         localtime, strflocaltime e mktime ainda leem o fuso do host."
            .into(),
    );
    notes.push(
        "Categoria dos candidatos: a do depscan refinada (crates com links sem código C, como defmt, e -sys de \
         Rust puro, como linux-raw-sys, não contam como C); a bruta fica em metrics.depscan.tree_category."
            .into(),
    );
    notes.push(
        "Alguns candidatos (awk-rs, frawk, xq) variam cerca de 1 ponto entre execuções porque a ordem do for-in \
         (ou de chaves) deles vem de hash com semente aleatória."
            .into(),
    );
    result.notes = notes;
    let path = result.write()?;
    eprintln!("[f04] gravado {} em {:.0}s", path.display(), t0.elapsed().as_secs_f64());
    Ok(())
}

fn builtin_diff(scratch: &Path) -> serde_json::Value {
    let cache = scratch.join("jq-builtins-cache.json");
    let jq_list: Option<BTreeSet<String>> = std::fs::read_to_string(&cache)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .or_else(|| {
            let oracle = harness::Oracle::locate().ok()?;
            let out = oracle.run_script("jq-builtins", "jq -nr 'builtins | .[]'", harness::MemTree::new()).ok()?;
            let set: BTreeSet<String> =
                String::from_utf8_lossy(out.stdout.as_slice()).lines().map(str::to_string).collect();
            let _ = std::fs::write(&cache, serde_json::to_string(&set).ok()?);
            Some(set)
        });
    let Some(jq_list) = jq_list else { return json!({"error": "oráculo indisponível"}) };
    let plain = jq::engine::builtin_names(false);
    let layered = jq::engine::builtin_names(true);
    let diff = |ours: &BTreeSet<String>| {
        json!({
            "missing_vs_jq": jq_list.difference(ours).cloned().collect::<Vec<_>>(),
            "extra_vs_jq": ours.difference(&jq_list).cloned().collect::<Vec<_>>(),
        })
    };
    json!({"jq_1_7_1_count": jq_list.len(), "jaq_plain": diff(&plain), "jaq_with_layer": diff(&layered)})
}

fn verdicts(
    result: &mut ExperimentResult,
    agg: &[(Role, Aggregated)],
    probes: &[checkpoint::ProbeResult],
    c: &Corpora,
    raw: &BTreeMap<String, CandScore>,
) {
    // H26: nenhum awk em Rust passa de 80% estrito (agente + suíte do gawk).
    let awk: Vec<&Aggregated> = agg.iter().filter(|(r, _)| *r == Role::Awk).map(|(_, a)| a).collect();
    let best = awk.iter().max_by(|a, b| a.strict.total_cmp(&b.strict));
    let table: Vec<serde_json::Value> = awk
        .iter()
        .map(|a| {
            // Casos onde leftmost-longest (POSIX) difere de leftmost-first.
            let posix = raw.get(&a.result.name).map(|s| {
                let cs: Vec<&CaseComparison> =
                    s.agent.iter().filter(|x| x.tags.iter().any(|t| t == "posix-semantics")).collect();
                (cs.iter().filter(|x| x.strict).count(), cs.len())
            });
            json!({"candidate": a.result.name, "strict": a.strict, "lenient": a.lenient,
                   "category": a.result.category, "posix_leftmost_longest_pass_total": posix})
        })
        .collect();
    let posix_any = raw
        .iter()
        .filter(|(_, s)| !s.upstream.is_empty())
        .any(|(_, s)| s.agent.iter().any(|x| x.strict && x.tags.iter().any(|t| t == "posix-semantics")));
    let total_cases = c.awk.len() + c.gawk.len();
    match best {
        Some(b) => {
            let verdict = if b.strict > 0.8 { Verdict::Refuted } else { Verdict::Confirmed };
            let summary = format!(
                "Melhor awk em Rust: {} com {} estrito ({} frouxo) em {} casos (agente + suíte do gawk 5.2.1); critério de 80% estrito {}. {}",
                b.result.name,
                pct(b.strict),
                pct(b.lenient),
                total_cases,
                if verdict == Verdict::Confirmed { "não atingido por nenhum" } else { "atingido" },
                if posix_any {
                    "Algum candidato acerta leftmost-longest."
                } else {
                    "Nenhum candidato acerta os casos de leftmost-longest; fazer à mão, com o motor POSIX do F01."
                }
            );
            result.hypothesis(
                "H26",
                verdict,
                summary,
                json!({"candidates": table,
                       "recommendation": "interpretador próprio, categoria (a): regex POSIX do F01, ordem do for-in do gawk, VM sans-IO como a do uutils/awk com checkpoint no laço de despacho; esta bancada como aceitação"}),
            );
        }
        None => result.hypothesis("H26", Verdict::Inconclusive, "nenhum candidato de awk medido", json!({})),
    }

    // H27: jaq tem compatibilidade alta com o jq real.
    let jq: Vec<&Aggregated> = agg.iter().filter(|(r, _)| *r == Role::Jq).map(|(_, a)| a).collect();
    let find = |prefix: &str| jq.iter().find(|a| a.result.name.starts_with(prefix)).copied();
    let ours = find("jaq-core");
    let bin = find("jaq (binário)");
    let table: Vec<serde_json::Value> = jq
        .iter()
        .map(|a| json!({"candidate": a.result.name, "jq_test_suites": a.jqtest_rate, "cli_strict": a.strict, "cli_lenient": a.lenient}))
        .collect();
    let jaq_pass_ok = c.jq_tests.len();
    match (ours, bin) {
        (Some(o), Some(b)) => {
            let jt = o.jqtest_rate.unwrap_or(0.0);
            let verdict = if jt >= 0.9 && o.lenient >= 0.9 {
                Verdict::Confirmed
            } else if jt >= 0.7 {
                Verdict::Partial
            } else {
                Verdict::Refuted
            };
            let qj = jq.iter().find(|a| a.result.name == "qj").copied();
            let summary = format!(
                "Suítes do jq 1.7.1 ({} testes que o jq do Debian passa): jaq-core com a camada nossa {}, binário jaq 3.1.1 {}; casos de CLI de agente: camada nossa {} estrito / {} frouxo, binário jaq {} estrito. Checkpoint no jaq sem fork (DataT::lut). Referência: qj {} nas suítes.",
                jaq_pass_ok,
                pct(jt),
                pct(b.jqtest_rate.unwrap_or(0.0)),
                pct(o.strict),
                pct(o.lenient),
                pct(b.strict),
                qj.and_then(|q| q.jqtest_rate).map(pct).unwrap_or_else(|| "não medido".into())
            );
            let recommendation = "Trade-off: qj (porte do jq 1.8.1, categoria c com Oniguruma) chega a ~99% nas suítes e precisa de patch pequeno pro checkpoint na VM e de fixar comportamentos 1.7.1; jaq + camada nossa é (a), checkpoint sem fork, mas para em ~84% e o resto exige fork espalhado em jaq-core, jaq-json e jaq-std. Recomendado: núcleo do qj com a CLI 1.7.1 desta camada, se o Oniguruma em C for aceito; senão jaq + camada com a lista de divergências como dívida.";
            let classes = o.result.metrics.get("jq_test_suites").and_then(|m| m.get("failures_by_class_fail_total")).cloned();
            let agent_classes = o.result.metrics.get("agent_failures_by_class").cloned();
            let probe_summary: Vec<serde_json::Value> = probes
                .iter()
                .filter(|p| p.name.starts_with("jaq"))
                .map(|p| json!({"probe": p.name, "interrupted": p.interrupted, "killed": p.killed, "latency_us": p.latency_us}))
                .collect();
            result.hypothesis(
                "H27",
                verdict,
                summary,
                json!({"candidates": table, "divergence_classes_jq_test": classes,
                       "divergence_classes_cli": agent_classes, "checkpoint": probe_summary,
                       "recommendation": recommendation}),
            );
        }
        _ => result.hypothesis("H27", Verdict::Inconclusive, "jaq não pôde ser medido", json!({"candidates": table})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jq_test_parser_handles_fail_blocks() {
        let text = "# comentário\n.a\n{\"a\":1}\n1\n\n%%FAIL\n{(0):1}\njq: error: Object keys must be strings at <top-level>, line 1:\n\n.[]\n[1,2]\n1\n2\n";
        let t = jqtest::parse("x.test", text);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].program, ".a");
        assert_eq!(t[0].expected, vec!["1"]);
        assert_eq!(t[0].line, 2);
        assert!(matches!(&t[1].fail, Some(Some(m)) if m.starts_with("jq: error: Object keys")));
        assert_eq!(t[2].expected, vec!["1", "2"]);
    }

    #[test]
    fn run_tests_output_parser() {
        let out = "Test #1: '.a' at line number 2\nTest #2: '.b' at line number 6\n*** Expected 1, but got 2 for test at line number 8: .b\n2 of 3 tests passed (0 malformed, 0 skipped)\n";
        let r = jqtest::parse_run_tests_output(out);
        assert_eq!(r.failing_lines.into_iter().collect::<Vec<_>>(), vec![6]);
        assert!(r.summary.contains("tests passed"));
    }

    #[test]
    fn our_jq_cli_basics() {
        let host = || jq::cli::Host {
            stdin: b"{\"a\": [1, 2.50, 1e2]}\n".to_vec(),
            read_file: Box::new(|_| Err("No such file or directory".into())),
            env: vec![("HOME".into(), "/root".into())],
            now: None,
        };
        let out = jq::cli::run(&["-c".into(), ".a, (.a | add), env.HOME".into()], host(), Default::default());
        assert_eq!(String::from_utf8_lossy(&out.stdout), "[1,2.50,1E+2]\n103.5\n\"/root\"\n");
        assert_eq!(out.exit, 0);
        let out = jq::cli::run(&[".a.b".into()], host(), Default::default());
        assert_eq!(String::from_utf8_lossy(&out.stderr), "jq: error (at <stdin>:1): Cannot index array with string \"b\"\n");
        assert_eq!(out.exit, 5);
        let out = jq::cli::run(&["nosuch(1)".into()], host(), Default::default());
        assert_eq!(out.exit, 3);
        assert!(String::from_utf8_lossy(&out.stderr).starts_with("jq: error: nosuch/1 is not defined at <top-level>, line 1:"));
    }

    #[test]
    fn checkpoint_interrupts_infinite_jaq_loop() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let flag = Arc::new(AtomicBool::new(false));
        let f2 = flag.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            f2.store(true, Ordering::SeqCst);
        });
        let host = jq::cli::Host {
            stdin: Vec::new(),
            read_file: Box::new(|_| Err("x".into())),
            env: Vec::new(),
            now: None,
        };
        let opts = jq::cli::RunOpts { interrupt: Some(flag), ..Default::default() };
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            jq::cli::run(&["-n".into(), "last(range(1e18))".into()], host, opts)
        }));
        let payload = r.expect_err("deveria desenrolar");
        assert!(payload.downcast_ref::<jq::engine::Interrupted>().is_some());
    }
}
