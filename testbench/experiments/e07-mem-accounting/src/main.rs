//! Orquestrador do E07: compila os binários de candidato, roda cada um como subprocesso (o allocator
//! global é um por binário), junta os números e grava `results/e07-mem-accounting.json`.
//!
//! Variáveis de ambiente:
//! - `E07_NO_BUILD=1`: não chama `cargo build` (os binários já estão ao lado deste).
//! - `E07_QUICK=1`: menos rodadas e cargas menores (só pra conferir o encanamento; não grava resultado
//!   a menos que `E07_WRITE=1`).

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use e07_mem_accounting::rng::SplitMix64;
use harness::{CandidateResult, ExperimentResult, Fit, Verdict};
use serde_json::{Value, json};

const EXPERIMENT: &str = "e07-mem-accounting";

/// Um binário de candidato.
#[derive(Debug, Clone, Copy)]
struct Cand {
    bin: &'static str,
    /// Nome no relatório.
    name: &'static str,
    /// Crate que o depscan varre e de onde vem a versão.
    krate: Option<&'static str>,
    /// Allocator interno de referência pra comparar o custo só da contabilidade.
    inner_baseline: Option<&'static str>,
}

const CANDS: &[Cand] = &[
    Cand { bin: "cand-system", name: "system", krate: None, inner_baseline: None },
    Cand { bin: "cand-mimalloc", name: "mimalloc", krate: Some("mimalloc"), inner_baseline: None },
    Cand {
        bin: "cand-tracking-allocator",
        name: "tracking-allocator",
        krate: Some("tracking-allocator"),
        inner_baseline: Some("cand-system"),
    },
    Cand {
        bin: "cand-tracking-allocator-mimalloc",
        name: "tracking-allocator+mimalloc",
        krate: Some("tracking-allocator"),
        inner_baseline: Some("cand-mimalloc"),
    },
    Cand { bin: "cand-alloc-track", name: "alloc-track", krate: Some("alloc-track"), inner_baseline: Some("cand-system") },
    Cand { bin: "cand-jqf-resource", name: "jqf-resource", krate: Some("jqf-resource"), inner_baseline: Some("cand-system") },
    Cand { bin: "cand-alloc-count", name: "alloc_count", krate: Some("alloc_count"), inner_baseline: Some("cand-system") },
    Cand {
        bin: "cand-allocation-counter",
        name: "allocation-counter",
        krate: Some("allocation-counter"),
        inner_baseline: Some("cand-system"),
    },
    Cand { bin: "cand-jemalloc", name: "jemalloc-thread-counters", krate: Some("tikv-jemalloc-ctl"), inner_baseline: None },
    Cand { bin: "cand-cap", name: "cap", krate: Some("cap"), inner_baseline: Some("cand-system") },
    Cand { bin: "cand-stats-alloc", name: "stats_alloc", krate: Some("stats_alloc"), inner_baseline: Some("cand-system") },
    Cand {
        bin: "cand-accounting-allocator",
        name: "accounting-allocator",
        krate: Some("accounting-allocator"),
        inner_baseline: Some("cand-system"),
    },
];

/// Candidatos avaliados só pela leitura do código (o motivo que os elimina não precisa de medida).
const INSPECTED: &[(&str, &str, &str)] = &[
    (
        "staging-tracking-allocator",
        "2.0.0",
        "Allocator da Parity (Polkadot): um contador do processo inteiro atrás de um spinlock global, sem grupos; \
         o limite se arma por `start_tracking`, que é `unsafe fn` (chamar exige bloco unsafe, proibido aqui). \
         Licença GPL-3.0-only, incompatível com o MIT do projeto.",
    ),
    (
        "mod-alloc",
        "1.0.0",
        "Profiler (substituto do dhat): contadores e pico do processo inteiro e agrupamento por ponto de chamada \
         (backtrace); não tem grupo por thread nem leitura por processo.",
    ),
    (
        "rallo",
        "0.5.2",
        "Profiler com rastreamento de pilha e flamegraph; contadores globais, sem grupos por thread.",
    ),
    (
        "re_memory",
        "0.38.1",
        "AccountingAllocator do Rerun: contagem global e rastreio de alocações grandes por callstack; sem grupos \
         por thread.",
    ),
    (
        "peakmem-alloc",
        "0.3.0",
        "Wrapper que mede o pico de memória do processo inteiro; sem grupos.",
    ),
];

// ---------------------------------------------------------------------------------------------
// Execução de subprocessos.
// ---------------------------------------------------------------------------------------------

#[derive(Debug)]
struct Out {
    code: Option<i32>,
    signal: Option<i32>,
    stdout: String,
    stderr: String,
    json: Option<Value>,
    timed_out: bool,
    wall_ms: u64,
}

impl Out {
    fn summary(&self) -> Value {
        json!({
            "exit_code": self.code,
            "signal": self.signal,
            "timed_out": self.timed_out,
            "wall_ms": self.wall_ms,
            "stderr_tail": tail(&self.stderr, 6),
            "stdout_tail": if self.json.is_none() { tail(&self.stdout, 3) } else { Vec::new() },
        })
    }
}

fn tail(s: &str, n: usize) -> Vec<String> {
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].iter().map(|l| l.to_string()).collect()
}

fn run(bin_dir: &Path, bin: &str, args: &[String], timeout: Duration) -> Out {
    let t0 = Instant::now();
    let mut child = match Command::new(bin_dir.join(bin))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return Out {
                code: None,
                signal: None,
                stdout: String::new(),
                stderr: format!("falha ao iniciar {bin}: {e}"),
                json: None,
                timed_out: false,
                wall_ms: 0,
            };
        }
    };
    let mut so = child.stdout.take().expect("stdout");
    let mut se = child.stderr.take().expect("stderr");
    let ho = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = so.read_to_string(&mut s);
        s
    });
    let he = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = se.read_to_string(&mut s);
        s
    });
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if t0.elapsed() > timeout {
                    let _ = child.kill();
                    timed_out = true;
                    break child.wait().ok();
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => break None,
        }
    };
    let stdout = ho.join().unwrap_or_default();
    let stderr = he.join().unwrap_or_default();
    let json = stdout.lines().rev().find(|l| l.trim_start().starts_with(['{', '['])).and_then(|l| serde_json::from_str(l).ok());
    Out {
        code: status.and_then(|s| s.code()),
        signal: status.and_then(|s| s.signal()),
        stdout,
        stderr,
        json,
        timed_out,
        wall_ms: t0.elapsed().as_millis() as u64,
    }
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------------------------------------
// Estatística.
// ---------------------------------------------------------------------------------------------

fn median_f(v: &[f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let n = s.len();
    Some(if n % 2 == 1 { s[n / 2] } else { (s[n / 2 - 1] + s[n / 2]) / 2.0 })
}

fn min_f(v: &[f64]) -> Option<f64> {
    v.iter().copied().reduce(f64::min)
}

fn max_f(v: &[f64]) -> Option<f64> {
    v.iter().copied().reduce(f64::max)
}

fn pct(ratio: f64) -> f64 {
    (ratio - 1.0) * 100.0
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

// ---------------------------------------------------------------------------------------------
// Cargas: rodadas intercaladas, razão pareada contra a linha de base de cada rodada.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct WorkloadData {
    /// bin -> tempos de parede (ns) por rodada (None = rodada falhou).
    times: BTreeMap<&'static str, Vec<Option<f64>>>,
    /// bin -> tempo de CPU das threads da carga (ns) por rodada.
    cpu: BTreeMap<&'static str, Vec<Option<f64>>>,
    rss: BTreeMap<&'static str, Vec<f64>>,
    checksums: BTreeMap<&'static str, Vec<u64>>,
    failures: BTreeMap<&'static str, Vec<Value>>,
}

fn at(j: &Value, path: &[&str]) -> Option<f64> {
    let mut v = j;
    for k in path {
        v = v.get(*k)?;
    }
    v.as_f64()
}

#[allow(clippy::too_many_arguments)]
fn run_workload(
    bin_dir: &Path,
    cands: &[Cand],
    cmd: &str,
    extra: &[String],
    rounds: u32,
    wall_path: &[&str],
    cpu_path: &[&str],
    seed: u64,
) -> WorkloadData {
    let mut data = WorkloadData::default();
    let mut rng = SplitMix64::new(seed);
    for round in 0..rounds {
        let order = rng.permutation(cands.len());
        for &i in &order {
            let c = cands[i];
            let mut a = vec![cmd.to_string()];
            a.extend_from_slice(extra);
            let out = run(bin_dir, c.bin, &a, Duration::from_secs(180));
            let t = out.json.as_ref().and_then(|j| at(j, wall_path));
            let cpu = out.json.as_ref().and_then(|j| at(j, cpu_path)).filter(|x| *x > 0.0);
            data.times.entry(c.bin).or_default().push(t);
            data.cpu.entry(c.bin).or_default().push(cpu);
            if let Some(j) = &out.json {
                if let Some(r) = j.get("peak_rss_kib").and_then(Value::as_f64) {
                    data.rss.entry(c.bin).or_default().push(r);
                }
                if let Some(ck) = j.pointer("/timed/checksum").and_then(Value::as_u64) {
                    data.checksums.entry(c.bin).or_default().push(ck);
                }
            }
            if t.is_none() {
                data.failures.entry(c.bin).or_default().push(out.summary());
            }
        }
        eprintln!("e07: {cmd} rodada {}/{rounds}", round + 1);
    }
    data
}

#[derive(Debug, Clone)]
struct Overhead {
    median_ns: Option<f64>,
    min_ns: Option<f64>,
    /// Mediana das razões pareadas por rodada contra a linha de base (System), em %.
    paired_pct: Option<f64>,
    /// Razão dos mínimos, em %.
    min_pct: Option<f64>,
    /// O mesmo contra o allocator interno do candidato.
    inner_paired_pct: Option<f64>,
    rss_median_kib: Option<f64>,
    runs: usize,
    failed: usize,
}

/// Overhead pela série de CPU (`cpu = true`) ou de parede.
fn overhead_of(data: &WorkloadData, bin: &str, base: &str, inner: Option<&str>, cpu: bool) -> Overhead {
    let series = if cpu { &data.cpu } else { &data.times };
    let times = series.get(bin).cloned().unwrap_or_default();
    let ok: Vec<f64> = times.iter().flatten().copied().collect();
    let paired = |against: &str| -> Option<f64> {
        let b = series.get(against)?;
        let ratios: Vec<f64> = times
            .iter()
            .zip(b)
            .filter_map(|(x, y)| Some(x.as_ref()? / y.as_ref()?))
            .collect();
        median_f(&ratios).map(pct)
    };
    let base_ok: Vec<f64> = series.get(base).map(|v| v.iter().flatten().copied().collect()).unwrap_or_default();
    Overhead {
        median_ns: median_f(&ok),
        min_ns: min_f(&ok),
        paired_pct: paired(base),
        min_pct: match (min_f(&ok), min_f(&base_ok)) {
            (Some(a), Some(b)) => Some(pct(a / b)),
            _ => None,
        },
        inner_paired_pct: inner.and_then(paired),
        rss_median_kib: data.rss.get(bin).and_then(|v| median_f(v)),
        runs: ok.len(),
        failed: times.len() - ok.len(),
    }
}

fn overhead_json(o: &Overhead) -> Value {
    json!({
        "median_ms": o.median_ns.map(|x| round2(x / 1e6)),
        "min_ms": o.min_ns.map(|x| round2(x / 1e6)),
        "overhead_vs_system_pct": o.paired_pct.map(round2),
        "overhead_vs_system_min_pct": o.min_pct.map(round2),
        "overhead_vs_inner_allocator_pct": o.inner_paired_pct.map(round2),
        "peak_rss_median_mib": o.rss_median_kib.map(|k| round2(k / 1024.0)),
        "runs": o.runs,
        "failed": o.failed,
    })
}

// ---------------------------------------------------------------------------------------------
// Fase A/B do critério da H16: os binários do par rodam ao mesmo tempo, em passo travado, e a razão é
// tirada iteração a iteração.
// ---------------------------------------------------------------------------------------------

/// Um participante do passo travado: rótulo, binário e comando.
type AbEntry = (&'static str, &'static str, &'static str);

const SYS: &str = "system";
const TRK: &str = "tracking-allocator";
const TRK_HDR: &str = "tracking-allocator (rastreamento desligado: só cabeçalho e realloc)";
const MI: &str = "mimalloc";
const TRK_MI: &str = "tracking-allocator+mimalloc";
const TRK_MI_HDR: &str = "tracking-allocator+mimalloc (rastreamento desligado)";

/// O conjunto que decide a H16, com a decomposição do custo.
const AB_MAIN: &[AbEntry] = &[
    (SYS, "cand-system", "serve-sort"),
    (TRK, "cand-tracking-allocator", "serve-sort"),
    (TRK_HDR, "cand-tracking-allocator", "serve-sort-header-only"),
];
/// A variante com mimalloc por baixo, comparada ao System e ao mimalloc puro.
const AB_MIMALLOC: &[AbEntry] = &[
    (SYS, "cand-system", "serve-sort"),
    (MI, "cand-mimalloc", "serve-sort"),
    (TRK_MI, "cand-tracking-allocator-mimalloc", "serve-sort"),
    (TRK_MI_HDR, "cand-tracking-allocator-mimalloc", "serve-sort-header-only"),
];

#[derive(Debug, Default)]
struct AbData {
    bins: Vec<&'static str>,
    /// rótulo -> (CPU, parede) por iteração.
    samples: BTreeMap<&'static str, Vec<Option<(f64, f64)>>>,
}

/// Sobe um processo `serve-sort` por participante, espera todos ficarem prontos e roda `iters`
/// iterações. Dois modos:
/// - `simultaneous = true` (passo travado): "go" pra todos de uma vez; a mesma iteração de cada um
///   acontece ao mesmo tempo e sofre a mesma carga dos vizinhos, mas eles disputam banda de memória
///   entre si, o que infla a base e dilui custos fixos.
/// - `simultaneous = false` (alternado): um por vez, em ordem sorteada a cada iteração; sem disputa
///   mútua, com os pares a ~1 s de distância.
fn ab_phase(bin_dir: &Path, entries: &[AbEntry], iters: u32, lines: &str, simultaneous: bool, seed: u64) -> AbData {
    use std::io::{BufRead, BufReader, Write};
    let bins: Vec<&'static str> = entries.iter().map(|e| e.0).collect();
    let mut data = AbData { bins: bins.clone(), ..AbData::default() };
    let mut kids = Vec::new();
    for &(label, bin, cmd) in entries {
        let child = Command::new(bin_dir.join(bin))
            .args([cmd, lines])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn();
        match child {
            Ok(mut c) => {
                let stdin = c.stdin.take().expect("stdin");
                let stdout = BufReader::new(c.stdout.take().expect("stdout"));
                kids.push((label, c, Some(stdin), stdout));
            }
            Err(e) => eprintln!("e07: falha ao iniciar {bin}: {e}"),
        }
    }
    let read_json = |r: &mut BufReader<std::process::ChildStdout>| -> Option<Value> {
        let mut line = String::new();
        r.read_line(&mut line).ok()?;
        serde_json::from_str(line.trim()).ok()
    };
    for (_, _, _, out) in kids.iter_mut() {
        let _ = read_json(out);
    }
    let parse = |j: Option<Value>| j.and_then(|j| Some((at(&j, &["timed", "cpu_ns"])?, at(&j, &["timed", "elapsed_ns"])?)));
    let mut rng = SplitMix64::new(seed);
    for i in 0..iters {
        if simultaneous {
            for (_, _, stdin, _) in kids.iter_mut() {
                if let Some(s) = stdin.as_mut() {
                    let _ = s.write_all(b"go\n").and_then(|()| s.flush());
                }
            }
            for (label, _, _, out) in kids.iter_mut() {
                let s = parse(read_json(out));
                data.samples.entry(*label).or_default().push(s);
            }
        } else {
            for k in rng.permutation(kids.len()) {
                let (label, _, stdin, out) = &mut kids[k];
                if let Some(s) = stdin.as_mut() {
                    let _ = s.write_all(b"go\n").and_then(|()| s.flush());
                }
                let s = parse(read_json(out));
                data.samples.entry(*label).or_default().push(s);
            }
        }
        if (i + 1) % 10 == 0 {
            let mode = if simultaneous { "simultâneo" } else { "alternado" };
            eprintln!("e07: pareado {mode} {} participantes, iteração {}/{iters}", bins.len(), i + 1);
        }
    }
    for (_, mut c, stdin, _) in kids {
        drop(stdin);
        let _ = c.wait();
    }
    data
}

fn quantile(v: &[f64], q: f64) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let pos = q * (s.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    Some(s[lo] + (s[hi] - s[lo]) * (pos - lo as f64))
}

/// Razões por bloco de `bin` contra `against` (CPU, ou parede com `wall = true`), em %.
fn ab_ratios(d: &AbData, bin: &str, against: &str, wall: bool) -> Vec<f64> {
    let (Some(a), Some(b)) = (d.samples.get(bin), d.samples.get(against)) else {
        return Vec::new();
    };
    a.iter()
        .zip(b)
        .filter_map(|(x, y)| {
            let (x, y) = (x.as_ref()?, y.as_ref()?);
            Some(if wall { pct(x.1 / y.1) } else { pct(x.0 / y.0) })
        })
        .collect()
}

fn dist(v: &[f64]) -> Value {
    json!({
        "median": median_f(v).map(round2),
        "p25": quantile(v, 0.25).map(round2),
        "p75": quantile(v, 0.75).map(round2),
        "min": min_f(v).map(round2),
        "max": max_f(v).map(round2),
        "blocks": v.len(),
    })
}

/// Distribuições de todos os pares do conjunto (cada binário contra cada um que vem antes dele).
fn ab_json(d: &AbData, iters: u32) -> Value {
    let mut cpu_ms = serde_json::Map::new();
    for bin in &d.bins {
        let v: Vec<f64> = d.samples.get(bin).map(|s| s.iter().flatten().map(|x| x.0 / 1e6).collect()).unwrap_or_default();
        cpu_ms.insert(bin.to_string(), json!({"median": median_f(&v).map(round2), "min": min_f(&v).map(round2)}));
    }
    let mut pairs = serde_json::Map::new();
    for (j, bin) in d.bins.iter().enumerate() {
        for against in &d.bins[..j] {
            pairs.insert(
                format!("{bin} vs {against}"),
                json!({
                    "cpu_pct": dist(&ab_ratios(d, bin, against, false)),
                    "wall_pct": dist(&ab_ratios(d, bin, against, true)),
                }),
            );
        }
    }
    json!({"bins": d.bins, "iters_per_run": iters, "cpu_ms": cpu_ms, "pairs": pairs})
}

// ---------------------------------------------------------------------------------------------
// Estouro de limite.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct OvCfg {
    detect: &'static str,
    every: u64,
    period_us: u64,
    chunk: u64,
}

const LIMIT: u64 = 64 << 20;

fn overshoot_configs(caps: &Value) -> Vec<OvCfg> {
    let flag = |k: &str| caps.get(k).and_then(Value::as_bool).unwrap_or(false);
    let mut out = Vec::new();
    for chunk in [64u64, 65536] {
        if flag("self_read") {
            for every in [1u64, 64, 1024] {
                out.push(OvCfg { detect: "self_poll", every, period_us: 0, chunk });
            }
        }
        if flag("limit_flag") {
            for every in [1u64, 64, 1024] {
                out.push(OvCfg { detect: "flag", every, period_us: 0, chunk });
            }
        }
        if flag("remote_read") {
            for period_us in [100u64, 1000] {
                out.push(OvCfg { detect: "watcher", every: 64, period_us, chunk });
            }
        }
    }
    out
}

fn summarize_runs(runs: &[Value]) -> Value {
    let get = |k: &str| -> Vec<f64> { runs.iter().filter_map(|r| r.get(k).and_then(Value::as_f64)).collect() };
    let detected = runs.iter().filter(|r| r.get("detected").and_then(Value::as_bool) == Some(true)).count();
    // Detectado antes de a verdade cruzar o teto: a conta do candidato é maior que o pedido (ex.: classe
    // de tamanho do jemalloc).
    let early = runs
        .iter()
        .filter(|r| {
            r.get("detected").and_then(Value::as_bool) == Some(true)
                && r.get("overshoot_bytes").and_then(Value::as_i64).is_some_and(|o| o <= 0)
        })
        .count();
    let over = get("overshoot_bytes");
    let lat = get("latency_ns");
    let wlat: Vec<f64> = runs.iter().filter_map(|r| r.get("watcher_latency_ns").and_then(Value::as_f64)).collect();
    let after = get("live_after_kill");
    json!({
        "reps": runs.len(),
        "detected": detected,
        "detected_before_crossing": early,
        "overshoot_kib_median": median_f(&over).map(|x| round2(x / 1024.0)),
        "overshoot_kib_max": max_f(&over).map(|x| round2(x / 1024.0)),
        "latency_us_median": median_f(&lat).map(|x| round2(x / 1e3)),
        "latency_us_max": max_f(&lat).map(|x| round2(x / 1e3)),
        "watcher_latency_us_median": median_f(&wlat).map(|x| round2(x / 1e3)),
        "live_after_kill_max": max_f(&after),
    })
}

// ---------------------------------------------------------------------------------------------
// Principal.
// ---------------------------------------------------------------------------------------------

fn build(manifest: &Path) -> Result<()> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let st = Command::new(cargo)
        .args(["build", "--release", "--bins", "--manifest-path"])
        .arg(manifest)
        .status()
        .context("cargo build")?;
    if !st.success() {
        bail!("cargo build dos candidatos falhou");
    }
    Ok(())
}

fn depscan_of(manifest: &Path, krate: &str) -> Value {
    match depscan::scan(manifest, krate) {
        Ok(t) => json!({
            "version": t.root.version,
            "category": t.tree_category.letter(),
            "root_unsafe": t.root.counts.unsafe_total(),
            "tree_unsafe": t.totals.unsafe_total(),
            "c_deps": t.c_deps,
            "deps": t.deps.len(),
        }),
        Err(e) => json!({"error": e.to_string()}),
    }
}

fn main() -> Result<()> {
    let quick = std::env::var_os("E07_QUICK").is_some();
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = manifest_dir.join("Cargo.toml");
    if std::env::var_os("E07_NO_BUILD").is_none() {
        build(&manifest)?;
    }
    let bin_dir = std::env::current_exe()?.parent().context("diretório dos binários")?.to_path_buf();
    let t_start = Instant::now();

    // 1. Capacidades declaradas e depscan.
    let mut caps: BTreeMap<&str, Value> = BTreeMap::new();
    let mut overcommit = Value::Null;
    for c in CANDS {
        let out = run(&bin_dir, c.bin, &args(&["info"]), Duration::from_secs(30));
        let j = out.json.clone().unwrap_or(Value::Null);
        if overcommit.is_null() {
            overcommit = j.get("overcommit").cloned().unwrap_or(Value::Null);
        }
        caps.insert(c.bin, j.get("caps").cloned().unwrap_or(Value::Null));
    }
    let mut scans: BTreeMap<&str, Value> = BTreeMap::new();
    for c in CANDS {
        if let Some(k) = c.krate {
            scans.entry(k).or_insert_with(|| depscan_of(&manifest, k));
        }
    }
    for k in ["tikv-jemallocator", "tracking-allocator"] {
        scans.entry(k).or_insert_with(|| depscan_of(&manifest, k));
    }
    eprintln!("e07: capacidades e depscan prontos ({:.1}s)", t_start.elapsed().as_secs_f64());

    // 2. Cargas.
    // Rodadas dimensionadas pra o experimento inteiro caber em ~10 minutos numa máquina carregada; o
    // veredito usa o tempo de CPU da thread, que é bem menos sensível aos vizinhos que o de parede.
    let (r_sort, r_borrow, r_small, r_mt) = if quick { (2, 1, 1, 1) } else { (7, 3, 5, 3) };
    let lines = if quick { "lines=100000" } else { "lines=1000000" };
    let ops = if quick { "ops=1000000" } else { "ops=10000000" };
    let mt_ops = if quick { "ops=200000" } else { "ops=2000000" };
    let (wall, cpu) = (&["timed", "elapsed_ns"][..], &["timed", "cpu_ns"][..]);
    let sort = run_workload(&bin_dir, CANDS, "sort", &args(&[lines]), r_sort, wall, cpu, 11);
    let sort_b = run_workload(&bin_dir, CANDS, "sort-borrowed", &args(&[lines]), r_borrow, wall, cpu, 12);
    let small = run_workload(&bin_dir, CANDS, "small", &args(&[ops]), r_small, wall, cpu, 13);
    let small_mt = run_workload(
        &bin_dir,
        CANDS,
        "small-mt",
        &args(&["threads=16", mt_ops]),
        r_mt,
        &["timed", "wall_ns"],
        &["timed", "cpu_ns_sum"],
        14,
    );
    eprintln!("e07: cargas prontas ({:.1}s)", t_start.elapsed().as_secs_f64());

    // Passo travado: o par roda ao mesmo tempo, uma iteração de cada por vez.
    // Os dois modos de pareamento (simultâneo e alternado) com os mesmos processos vivos; o veredito usa
    // a pior das duas medianas.
    let (ab_blocks, ab_mi_blocks, ab_iters) = if quick { (3, 3, 1) } else { (30, 20, 1) };
    let ab = ab_phase(&bin_dir, AB_MAIN, ab_blocks, lines, true, 15);
    let ab_alt = ab_phase(&bin_dir, AB_MAIN, ab_blocks, lines, false, 17);
    let ab_mi = ab_phase(&bin_dir, AB_MIMALLOC, ab_mi_blocks, lines, true, 16);
    let ab_mi_alt = ab_phase(&bin_dir, AB_MIMALLOC, ab_mi_blocks, lines, false, 18);
    eprintln!("e07: A/B do sort pronto ({:.1}s)", t_start.elapsed().as_secs_f64());

    let checksums_agree = [&sort, &sort_b, &small, &small_mt].iter().all(|d| {
        let all: Vec<u64> = d.checksums.values().flatten().copied().collect();
        all.windows(2).all(|w| w[0] == w[1])
    });

    // 3. Cenários de corretude e custos fixos.
    let mut scen: BTreeMap<&str, BTreeMap<&str, Value>> = BTreeMap::new();
    for c in CANDS {
        if c.bin == "cand-system" || c.bin == "cand-mimalloc" {
            continue;
        }
        for cmd in ["attribution", "controlled", "exit-residual", "costs"] {
            let out = run(&bin_dir, c.bin, &args(&[cmd]), Duration::from_secs(300));
            let v = out.json.clone().unwrap_or_else(|| json!({"failed": out.summary()}));
            scen.entry(c.bin).or_default().insert(cmd, v);
        }
    }
    eprintln!("e07: cenários prontos ({:.1}s)", t_start.elapsed().as_secs_f64());

    // 4. Estouro de limite (só quem lê os bytes de um processo).
    let ov_cands = ["cand-tracking-allocator", "cand-alloc-track", "cand-jqf-resource", "cand-alloc-count", "cand-jemalloc"];
    let reps = if quick { "reps=2" } else { "reps=5" };
    let mut overshoot: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    for bin in ov_cands {
        let cfgs = overshoot_configs(caps.get(bin).unwrap_or(&Value::Null));
        let read_ns = scen
            .get(bin)
            .and_then(|m| m.get("costs"))
            .map(|c| {
                let s = c.get("self_read_ns").and_then(Value::as_f64).unwrap_or(0.0);
                let r = c.get("remote_read_ns").and_then(Value::as_f64).unwrap_or(0.0);
                (s, r)
            })
            .unwrap_or((0.0, 0.0));
        for cfg in cfgs {
            // Pula configuração cujo custo de leitura sozinho passaria de ~5 s por repetição.
            let reads = (LIMIT / cfg.chunk + cfg.every) as f64 / cfg.every as f64;
            if cfg.detect == "self_poll" && reads * read_ns.0 > 5e9 {
                overshoot.entry(bin).or_default().push(json!({
                    "detect": cfg.detect, "every": cfg.every, "chunk": cfg.chunk, "period_us": cfg.period_us,
                    "skipped": format!("cada leitura custa {:.1} ms; {reads:.0} leituras por repetição", read_ns.0 / 1e6),
                }));
                continue;
            }
            let a = vec![
                "overshoot".to_string(),
                format!("detect={}", cfg.detect),
                format!("every={}", cfg.every),
                format!("period_us={}", cfg.period_us),
                format!("chunk={}", cfg.chunk),
                format!("limit={LIMIT}"),
                reps.to_string(),
            ];
            let out = run(&bin_dir, bin, &a, Duration::from_secs(120));
            let mut row = json!({"detect": cfg.detect, "every": cfg.every, "chunk": cfg.chunk, "period_us": cfg.period_us});
            match out.json.as_ref().and_then(|j| j.get("runs")).and_then(Value::as_array) {
                Some(runs) => {
                    row["summary"] = summarize_runs(runs);
                }
                None => {
                    row["process_died"] = out.summary();
                }
            }
            overshoot.entry(bin).or_default().push(row);
        }
    }
    eprintln!("e07: estouro pronto ({:.1}s)", t_start.elapsed().as_secs_f64());

    // 5. Demonstrações que abortam (sempre em subprocesso).
    let demo_list: &[(&str, &str, &[&str], bool)] = &[
        ("hard_limit_cap", "cand-cap", &["hard-limit"], true),
        ("hard_limit_cap_try_reserve", "cand-cap", &["hard-limit-try"], false),
        ("huge_alloc_no_limit", "cand-system", &["huge-alloc"], true),
        ("huge_alloc_try_reserve", "cand-system", &["huge-alloc-try"], false),
        ("jqf_past_ceiling_4kib", "cand-jqf-resource", &["past-ceiling", "ask=4096"], false),
        ("jqf_past_ceiling_4mib", "cand-jqf-resource", &["past-ceiling", "ask=4194304"], true),
        ("alloc_track_thread_limit", "cand-alloc-track", &["thread-limit", "threads=1200"], true),
    ];
    let mut demos = serde_json::Map::new();
    for (name, bin, a, expect_abort) in demo_list {
        let out = run(&bin_dir, bin, &args(a), Duration::from_secs(120));
        let aborted = out.signal == Some(6);
        demos.insert(
            name.to_string(),
            json!({
                "bin": bin,
                "args": a,
                "expected_abort": expect_abort,
                "aborted_sigabrt": aborted,
                "matches_expectation": aborted == *expect_abort,
                "process": out.summary(),
                "stdout": out.json,
            }),
        );
    }
    eprintln!("e07: demonstrações prontas ({:.1}s)", t_start.elapsed().as_secs_f64());

    // 6. Contabilidade explícita do kernel.
    let kreps = if quick { "reps=2" } else { "reps=7" };
    let kpipe = if quick { "pipe_mib=64" } else { "pipe_mib=512" };
    let kfile = if quick { "file_mib=32" } else { "file_mib=256" };
    let mut kernel = serde_json::Map::new();
    for bin in ["cand-system", "cand-tracking-allocator"] {
        let out = run(&bin_dir, bin, &args(&["kernel-acct", kreps, kpipe, kfile]), Duration::from_secs(600));
        kernel.insert(bin.to_string(), out.json.clone().unwrap_or_else(|| json!({"failed": out.summary()})));
    }
    eprintln!("e07: contabilidade do kernel pronta ({:.1}s)", t_start.elapsed().as_secs_f64());
    let kernel_summary = kernel_summary_of(&kernel);

    // 7. Avaliação.
    let mut res = ExperimentResult::new(EXPERIMENT, "Contabilidade de memória por pseudo-processo");
    let base = "cand-system";
    let mut table = serde_json::Map::new();
    // `ovh` guarda o overhead por tempo de CPU (o que decide); `ovh_wall`, o de parede (registro).
    let mut ovh: BTreeMap<&str, (Overhead, Overhead, Overhead, Overhead)> = BTreeMap::new();
    let mut ovh_wall: BTreeMap<&str, (Overhead, Overhead, Overhead, Overhead)> = BTreeMap::new();
    for c in CANDS {
        let both = |d: &WorkloadData| {
            (overhead_of(d, c.bin, base, c.inner_baseline, true), overhead_of(d, c.bin, base, c.inner_baseline, false))
        };
        let (s, s_w) = both(&sort);
        let (sb, sb_w) = both(&sort_b);
        let (sm, sm_w) = both(&small);
        let (mt, mt_w) = both(&small_mt);
        table.insert(
            c.name.to_string(),
            json!({
                "sort_1m_owned": {"cpu": overhead_json(&s), "wall": overhead_json(&s_w)},
                "sort_1m_borrowed": {"cpu": overhead_json(&sb), "wall": overhead_json(&sb_w)},
                "small_allocs_1_thread": {"cpu": overhead_json(&sm), "wall": overhead_json(&sm_w)},
                "small_allocs_16_threads": {"cpu": overhead_json(&mt), "wall": overhead_json(&mt_w)},
                "failures": {
                    "sort": sort.failures.get(c.bin),
                    "small": small.failures.get(c.bin),
                    "small_mt": small_mt.failures.get(c.bin),
                },
            }),
        );
        ovh.insert(c.bin, (s, sb, sm, mt));
        ovh_wall.insert(c.bin, (s_w, sb_w, sm_w, mt_w));
    }

    let attr_ok = |bin: &str| -> Option<bool> {
        scen.get(bin)?.get("attribution")?.get("free_attributed_to_allocator")?.as_bool()
    };
    let realloc_ok = |bin: &str| -> Option<bool> {
        scen.get(bin)?.get("attribution")?.get("realloc_moves_ownership")?.as_bool()
    };
    let controlled_ok = |bin: &str| -> (usize, usize) {
        let Some(cases) = scen.get(bin).and_then(|m| m.get("controlled")).and_then(Value::as_array) else {
            return (0, 0);
        };
        let ok = cases.iter().filter(|c| c.get("ok").and_then(Value::as_bool) == Some(true)).count();
        (ok, cases.len())
    };
    let ov_summary = |bin: &str, detect: &str, every: u64, chunk: u64| -> Option<Value> {
        overshoot
            .get(bin)?
            .iter()
            .find(|r| {
                r.get("detect").and_then(Value::as_str) == Some(detect)
                    && r.get("every").and_then(Value::as_u64) == Some(every)
                    && r.get("chunk").and_then(Value::as_u64) == Some(chunk)
            })
            .cloned()
    };

    // H16: precisa de grupo por processo, atribuição certa, leitura pro checkpoint e overhead < 15%.
    let primary = "cand-tracking-allocator";
    let p_sort = ovh.get(primary).map(|o| o.0.clone());
    let p_attr = attr_ok(primary);
    let p_realloc = realloc_ok(primary);
    let p_ctrl = controlled_ok(primary);
    let flag64 = ov_summary(primary, "flag", 64, 65536);
    let flag1 = ov_summary(primary, "flag", 1, 64);
    let watch1ms = ov_summary(primary, "watcher", 64, 65536).or_else(|| ov_summary(primary, "watcher", 64, 64));
    // O critério usa as duas medidas pareadas (CPU): simultânea e alternada. Vale a pior das duas
    // medianas, porque cada modo tem um viés conhecido (a simultânea dilui custo fixo com a disputa de
    // memória; a alternada pareia a ~1 s de distância). A tabela geral sequencial fica como contexto.
    let med = |d: &AbData, a: &str, b: &str| median_f(&ab_ratios(d, a, b, false));
    let worst = |x: Option<f64>, y: Option<f64>| match (x, y) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    let sim_cpu = ab_ratios(&ab, TRK, SYS, false);
    let alt_cpu = ab_ratios(&ab_alt, TRK, SYS, false);
    let sort_sim = median_f(&sim_cpu);
    let sort_alt = median_f(&alt_cpu);
    let sort_pct = worst(sort_sim, sort_alt).or(p_sort.as_ref().and_then(|o| o.paired_pct));
    let q = |v: &[f64]| (quantile(v, 0.25), quantile(v, 0.75));
    let (sim_p25, sim_p75) = q(&sim_cpu);
    let (alt_p25, alt_p75) = q(&alt_cpu);
    // Decomposição: cabeçalho + realloc sem override (rastreamento desligado) e o rastreamento em si.
    let hdr_sim = med(&ab, TRK_HDR, SYS);
    let hdr_alt = med(&ab_alt, TRK_HDR, SYS);
    let trk_only_sim = med(&ab, TRK, TRK_HDR);
    let trk_only_alt = med(&ab_alt, TRK, TRK_HDR);
    let tam_sim = med(&ab_mi, TRK_MI, SYS);
    let tam_alt = med(&ab_mi_alt, TRK_MI, SYS);
    let tam_sort = worst(tam_sim, tam_alt);
    let tam_inner_sim = med(&ab_mi, TRK_MI, MI);
    let tam_inner_alt = med(&ab_mi_alt, TRK_MI, MI);
    let mi_sim = med(&ab_mi, MI, SYS);
    let mi_alt = med(&ab_mi_alt, MI, SYS);
    let detection_measured = flag64.as_ref().and_then(|r| r.pointer("/summary/latency_us_median")).is_some();
    // O critério compara contra o allocator que o host usaria sem contabilidade (o System). Vale o melhor
    // arranjo pronto com grupos por thread: tracking-allocator sobre System ou sobre mimalloc.
    let best = match (sort_pct, tam_sort) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let correct = p_attr == Some(true) && p_ctrl.0 == p_ctrl.1 && p_ctrl.1 > 0;
    let verdict = match best {
        Some(p) if correct && p < 15.0 && detection_measured => Verdict::Confirmed,
        Some(p) if correct && p < 30.0 => Verdict::Partial,
        Some(_) => Verdict::Refuted,
        None => Verdict::Inconclusive,
    };
    let lat64 = flag64.as_ref().and_then(|r| r.pointer("/summary/latency_us_median")).and_then(Value::as_f64);
    let over64 = flag64.as_ref().and_then(|r| r.pointer("/summary/overshoot_kib_max")).and_then(Value::as_f64);
    let lat1 = flag1.as_ref().and_then(|r| r.pointer("/summary/latency_us_median")).and_then(Value::as_f64);
    let small_pct = ovh.get(primary).and_then(|o| o.2.paired_pct);
    let mt_pct = ovh.get(primary).and_then(|o| o.3.paired_pct);
    let system_passes = sort_pct.is_some_and(|p| p < 15.0);
    let which = if system_passes {
        "o arranjo sobre o System já passa"
    } else if tam_sort.is_some_and(|p| p < 15.0) {
        "sobre o System não passa; passa com o mimalloc por baixo"
    } else {
        "nenhum arranjo passa"
    };
    let summary = format!(
        "{}: no sort de 1M linhas, o tracking-allocator 0.4 com a nossa tabela por grupo custa {} de CPU contra o \
         System na medida pareada alternada (quartis {} a {}) e {} na simultânea (quartis {} a {}). Desse \
         custo, o cabeçalho de 8 bytes e o realloc sem override respondem por {} e {} \
         (rastreamento desligado), e o rastreamento em si por {} e {}. Sobre o mimalloc o arranjo fica {} e {} \
         contra o System ({} e {} contra o mimalloc puro, que sozinho faz {} e {}). Critério < 15% contra o \
         System, pela pior das duas medidas: {}. A atribuição está certa (A aloca e B libera: {}; B realoca: {}; \
         {}/{} cenários de bytes vivos exatos), e o limite marcado pelo próprio allocator é visto no checkpoint \
         seguinte: a cada alocação, {}; a cada 64 alocações de 64 KiB, mediana {} e estouro máximo {}.",
        verdict.label_pt(),
        fmt_pct(sort_alt),
        fmt_pct(alt_p25),
        fmt_pct(alt_p75),
        fmt_pct(sort_sim),
        fmt_pct(sim_p25),
        fmt_pct(sim_p75),
        fmt_pct(hdr_alt),
        fmt_pct(hdr_sim),
        fmt_pct(trk_only_alt),
        fmt_pct(trk_only_sim),
        fmt_pct(tam_alt),
        fmt_pct(tam_sim),
        fmt_pct(tam_inner_alt),
        fmt_pct(tam_inner_sim),
        fmt_pct(mi_alt),
        fmt_pct(mi_sim),
        which,
        yes_no(p_attr),
        yes_no(p_realloc),
        p_ctrl.0,
        p_ctrl.1,
        fmt_us(lat1),
        fmt_us(lat64),
        fmt_kib(over64),
    );
    let pair = |sim: Option<f64>, alt: Option<f64>| json!({"alternating": alt.map(round2), "simultaneous": sim.map(round2)});
    res.hypothesis(
        "H16",
        verdict,
        summary,
        json!({
            "primary_candidate": "tracking-allocator 0.4.0 + GroupTable (tabela de contadores por grupo, nossa)",
            "measure": "tempo de CPU da thread (CLOCK_THREAD_CPUTIME_ID); processos vivos pareados, simultâneo e alternado; vale a pior mediana",
            "criterion_arrangement": which,
            "sort_1m_overhead_vs_system_pct": sort_pct.map(round2),
            "sort_1m_tracking_vs_system_pct": pair(sort_sim, sort_alt),
            "sort_1m_tracking_vs_system_quartiles_pct": {
                "alternating": [alt_p25.map(round2), alt_p75.map(round2)],
                "simultaneous": [sim_p25.map(round2), sim_p75.map(round2)],
            },
            "sort_1m_header_and_realloc_only_vs_system_pct": pair(hdr_sim, hdr_alt),
            "sort_1m_tracking_callbacks_only_pct": pair(trk_only_sim, trk_only_alt),
            "sort_1m_tracking_over_mimalloc_vs_system_pct": pair(tam_sim, tam_alt),
            "sort_1m_tracking_over_mimalloc_vs_mimalloc_pct": pair(tam_inner_sim, tam_inner_alt),
            "sort_1m_mimalloc_vs_system_pct": pair(mi_sim, mi_alt),
            "sort_1m_overhead_vs_system_general_table_pct": p_sort.as_ref().and_then(|o| o.paired_pct).map(round2),
            "small_allocs_overhead_pct": small_pct.map(round2),
            "small_allocs_16_threads_overhead_pct": mt_pct.map(round2),
            "cross_thread_free_attributed_to_allocator": p_attr,
            "cross_thread_realloc_moves_ownership": p_realloc,
            "controlled_cases_ok": format!("{}/{}", p_ctrl.0, p_ctrl.1),
            "detection_flag_every_64_chunk_64k": flag64,
            "detection_flag_every_1_chunk_64": flag1,
            "detection_watcher": watch1ms,
            "unsafe_in_our_code": "nenhum: unsafe_code = forbid compila com #[global_allocator] de tipo de crate",
        }),
    );

    // Candidatos.
    for c in CANDS {
        let scan = c.krate.and_then(|k| scans.get(k)).cloned().unwrap_or(Value::Null);
        let version = scan.get("version").and_then(Value::as_str).unwrap_or("-").to_string();
        let category = scan.get("category").and_then(Value::as_str).map(str::to_string);
        let (s, sb, sm, mt) = ovh.get(c.bin).cloned().expect("overhead");
        let (s_w, sb_w, sm_w, mt_w) = ovh_wall.get(c.bin).cloned().expect("overhead");
        let cp = caps.get(c.bin).cloned().unwrap_or(Value::Null);
        let attr = attr_ok(c.bin);
        let ctrl = controlled_ok(c.bin);
        let paired_note = match c.bin {
            "cand-tracking-allocator" => Some(format!(
                "{} (alternado) e {} (simultâneo) contra o System, dos quais {} e {} com o rastreamento desligado \
                 (cabeçalho e realloc sem override)",
                fmt_pct(sort_alt),
                fmt_pct(sort_sim),
                fmt_pct(hdr_alt),
                fmt_pct(hdr_sim),
            )),
            "cand-tracking-allocator-mimalloc" => Some(format!(
                "{} (alternado) e {} (simultâneo) contra o System; {} e {} contra o mimalloc puro",
                fmt_pct(tam_alt),
                fmt_pct(tam_sim),
                fmt_pct(tam_inner_alt),
                fmt_pct(tam_inner_sim),
            )),
            _ => None,
        };
        let (fit, notes) =
            judge(c, &cp, attr, realloc_ok(c.bin), ctrl, &s, &sm, &mt, scen.get(c.bin), &demos, paired_note.as_deref());
        let metrics = json!({
            "capabilities": cp,
            "depscan": scan,
            "sort_1m_owned": {"cpu": overhead_json(&s), "wall": overhead_json(&s_w)},
            "sort_1m_borrowed": {"cpu": overhead_json(&sb), "wall": overhead_json(&sb_w)},
            "small_allocs_1_thread": {"cpu": overhead_json(&sm), "wall": overhead_json(&sm_w)},
            "small_allocs_16_threads": {"cpu": overhead_json(&mt), "wall": overhead_json(&mt_w)},
            "attribution": scen.get(c.bin).and_then(|m| m.get("attribution")),
            "controlled_ok": format!("{}/{}", ctrl.0, ctrl.1),
            "exit_residual": scen.get(c.bin).and_then(|m| m.get("exit-residual")),
            "costs": scen.get(c.bin).and_then(|m| m.get("costs")),
            "overshoot": overshoot.get(c.bin),
        });
        res.candidates.push(CandidateResult {
            name: c.name.to_string(),
            version,
            role: "mem-accounting".to_string(),
            category,
            conformance: None,
            fit,
            notes,
            metrics,
        });
    }
    for (name, version, why) in INSPECTED {
        res.candidates.push(CandidateResult {
            name: name.to_string(),
            version: version.to_string(),
            role: "mem-accounting".to_string(),
            category: None,
            conformance: None,
            fit: Fit::DoesNotFit,
            notes: format!("Avaliado pela leitura do código-fonte, sem binário: {why}"),
            metrics: json!({"measured": false}),
        });
    }

    res.metrics = json!({
        "host_overcommit_memory": overcommit,
        "quick": quick,
        "limit_bytes_overshoot": LIMIT,
        "rounds": {
            "sort": r_sort, "sort_borrowed": r_borrow, "small": r_small, "small_mt": r_mt,
            "paired_sort_iterations": ab_blocks, "paired_sort_mimalloc_iterations": ab_mi_blocks,
        },
        "paired_sort_simultaneous": ab_json(&ab, ab_iters),
        "paired_sort_alternating": ab_json(&ab_alt, ab_iters),
        "paired_sort_mimalloc_simultaneous": ab_json(&ab_mi, ab_iters),
        "paired_sort_mimalloc_alternating": ab_json(&ab_mi_alt, ab_iters),
        "checksums_agree": checksums_agree,
        "workloads": table,
        "scenarios": scen,
        "overshoot": overshoot,
        "demos": demos,
        "kernel_accounting_summary": kernel_summary,
        "kernel_accounting": kernel,
        "elapsed_s": round2(t_start.elapsed().as_secs_f64()),
    });
    res.notes = vec![
        "Cada candidato é um binário próprio (o allocator global é um por binário) e roda em subprocesso. Todo \
         tempo é de CPU das threads da carga (CLOCK_THREAD_CPUTIME_ID), que não conta a espera na fila do \
         escalonador; o de parede fica registrado ao lado. A máquina é compartilhada e a carga dos vizinhos oscila \
         (load de 5 a 750 durante as rodadas), por isso o critério da H16 usa processos vivos pareados: System, \
         tracking e tracking com o rastreamento desligado rodam juntos, uma iteração do sort por vez, nos modos \
         simultâneo e alternado, com razão por iteração; vale a pior das duas medianas. A tabela geral \
         (workloads), com todos os candidatos em rodadas sequenciais de ordem sorteada, é contexto: com poucas \
         rodadas, um pico de carga distorce a mediana das razões, e as notas usam a razão dos mínimos."
            .to_string(),
        "Bytes vivos são os bytes pedidos (Layout::size). O tracking-allocator acrescenta 8 bytes de cabeçalho por \
         alocação (mais o alinhamento), que aparecem no pico de RSS, não na conta."
            .to_string(),
        "No estouro, o teto é múltiplo do tamanho do pedaço e o cruzamento cai logo depois de um checkpoint: é o pior \
         caso de fase (estouro de every x chunk)."
            .to_string(),
    ];

    let write = !quick || std::env::var_os("E07_WRITE").is_some();
    if write {
        let path = res.write()?;
        eprintln!("e07: gravado {}", path.display());
    }
    println!("{}", serde_json::to_string_pretty(&res.hypotheses)?);
    Ok(())
}

/// Resumo da contabilidade explícita do kernel: custo do contador isolado (microbenchmark preciso), o
/// custo esperado por operação de pipe (contadores x custo do par / ns da operação) e o que a medida de
/// ponta a ponta mostrou (dentro ou fora do ruído).
fn kernel_summary_of(kernel: &serde_json::Map<String, Value>) -> Value {
    let mut out = serde_json::Map::new();
    for (bin, k) in kernel {
        let f = |key: &str| k.get(key).and_then(Value::as_f64);
        let row = |table: &str, mode: &str, scope: bool| -> Option<Value> {
            k.get(table)?
                .as_array()?
                .iter()
                .find(|r| r.get("mode").and_then(Value::as_str) == Some(mode) && r.get("kernel_scope").and_then(Value::as_bool) == Some(scope))
                .cloned()
        };
        let base_pipe = row("pipe_single_cpu", "none", false).and_then(|r| r.get("min_ns_per_op").and_then(Value::as_f64));
        let base_file = row("file_cpu", "none", false).and_then(|r| r.get("min_ns_per_op").and_then(Value::as_f64));
        let pair = f("atomic_pair_ns_single");
        let expected = |base: Option<f64>, counters: f64| -> Option<f64> { Some(round2(pair? * counters / base? * 100.0)) };
        let pick = |table: &str| -> Value {
            let mut m = serde_json::Map::new();
            for (mode, scope) in [("process", false), ("process_and_sandbox", false), ("process_and_sandbox", true)] {
                if let Some(r) = row(table, mode, scope) {
                    m.insert(
                        format!("{mode}{}", if scope { "+kernel_scope" } else { "" }),
                        json!({
                            "overhead_median_pct": r.get("overhead_pct").and_then(Value::as_f64).map(round2),
                            "overhead_min_pct": r.get("overhead_min_pct").and_then(Value::as_f64).map(round2),
                        }),
                    );
                }
            }
            Value::Object(m)
        };
        out.insert(
            bin.clone(),
            json!({
                "atomic_pair_ns_single": f("atomic_pair_ns_single").map(round2),
                "atomic_pair_ns_16_threads_own_counter": f("atomic_pair_ns_16_own").map(round2),
                "atomic_pair_ns_16_threads_shared_counter": f("atomic_pair_ns_16_shared").map(round2),
                "atomic_pair_ns_16_threads_shared_batched_64k": f("atomic_pair_ns_16_shared_batched_64k").map(round2),
                "pipe_op_cpu_ns_min": base_pipe.map(round2),
                "file_append_cpu_ns_min": base_file.map(round2),
                "expected_pipe_overhead_pct_process": expected(base_pipe, 1.0),
                "expected_pipe_overhead_pct_process_and_sandbox": expected(base_pipe, 2.0),
                "expected_file_overhead_pct_process_and_sandbox": expected(base_file, 1.0),
                "measured_pipe_single": pick("pipe_single_cpu"),
                "measured_pipe_two_threads": pick("pipe_two_threads_wall"),
                "measured_file": pick("file_cpu"),
            }),
        );
    }
    Value::Object(out)
}

fn fmt_pct(v: Option<f64>) -> String {
    v.map_or("n/d".to_string(), |x| format!("{x:.1}%"))
}

fn fmt_us(v: Option<f64>) -> String {
    v.map_or("n/d".to_string(), |x| {
        if x >= 1000.0 {
            format!("{:.2} ms", x / 1000.0)
        } else if x >= 1.0 {
            format!("{x:.1} µs")
        } else {
            format!("{:.0} ns", x * 1000.0)
        }
    })
}

fn fmt_kib(v: Option<f64>) -> String {
    v.map_or("n/d".to_string(), |x| if x >= 1024.0 { format!("{:.1} MiB", x / 1024.0) } else { format!("{x:.1} KiB") })
}

fn yes_no(v: Option<bool>) -> &'static str {
    match v {
        Some(true) => "sim",
        Some(false) => "não",
        None => "não se aplica",
    }
}

/// Veredito de encaixe de um candidato a partir das medidas.
#[allow(clippy::too_many_arguments)]
fn judge(
    c: &Cand,
    caps: &Value,
    attr: Option<bool>,
    realloc: Option<bool>,
    ctrl: (usize, usize),
    sort: &Overhead,
    small: &Overhead,
    mt: &Overhead,
    scen: Option<&BTreeMap<&str, Value>>,
    demos: &serde_json::Map<String, Value>,
    paired_sort: Option<&str>,
) -> (Fit, String) {
    let flag = |k: &str| caps.get(k).and_then(Value::as_bool).unwrap_or(false);
    // Nas notas vale a razão dos mínimos de CPU da tabela sequencial: com poucas rodadas, um pico de
    // carga numa rodada do System distorce a mediana das razões pareadas e o mínimo não.
    let sort_s = format!("{} (razão dos mínimos de CPU)", fmt_pct(sort.min_pct));
    let small_s = fmt_pct(small.min_pct);
    let mt_s = fmt_pct(mt.min_pct);
    let costs = scen.and_then(|m| m.get("costs"));
    let read_ns = costs.and_then(|c| c.get("self_read_ns").or(c.get("remote_read_ns"))).and_then(Value::as_f64);
    let demo_aborted = |n: &str| demos.get(n).and_then(|d| d.get("aborted_sigabrt")).and_then(Value::as_bool);
    match c.bin {
        "cand-system" => (Fit::Reference, "Linha de base: malloc da glibc, sem contabilidade.".to_string()),
        "cand-mimalloc" => (
            Fit::Reference,
            format!("Linha de base do allocator interno alternativo (C, categoria c): sort {sort_s} e alocações pequenas {small_s} contra o System."),
        ),
        "cand-tracking-allocator" | "cand-tracking-allocator-mimalloc" => {
            // Encaixa com trabalho nosso (a tabela por grupo e a disciplina do escopo do kernel) quando a
            // atribuição entre threads está certa; o limiar de overhead é julgado na H16.
            let fit = if attr == Some(true) && ctrl.0 == ctrl.1 { Fit::FitsWithWork } else { Fit::DoesNotFit };
            (
                fit,
                format!(
                    "Grupo por pseudo-processo via AllocationGroupToken; a liberação é debitada do grupo de origem (cabeçalho \
                     de 8 bytes por alocação), então A aloca e B libera dá certo ({}), e realocação feita por B transfere a \
                     memória pra B ({}). Cenários controlados {}/{}. Sort de 1M linhas, medida pareada: {}. Alocações \
                     pequenas {small_s} contra o System. Leitura do contador ~{} ns. Trabalho nosso: \
                     o AllocationTracker com a tabela de slots por grupo e a flag de limite (src/group_table.rs), \
                     AllocationRegistry::untracked em volta das alocações do kernel, reciclagem de slot na morte do \
                     processo. Crate parada desde 2022 (MPL-2.0), com unsafe interno; o realloc não é sobrescrito \
                     (sempre aloca e copia).",
                    yes_no(attr),
                    yes_no(realloc),
                    ctrl.0,
                    ctrl.1,
                    paired_sort.unwrap_or("n/d"),
                    read_ns.map_or("n/d".to_string(), |x| format!("{x:.1}")),
                ),
            )
        }
        "cand-alloc-track" => (
            Fit::DoesNotFit,
            format!(
                "Atribui a liberação à thread de origem (A aloca e B libera: {}), mas: índice de thread nunca reaproveitado \
                 e limite fixo de 1024 threads (a 1025a thread aborta o host: {}), um DashMap por ponteiro em toda \
                 alocação (sort {sort_s}, alocações pequenas {small_s}) e a única leitura é thread_report(), que varre \
                 1024 x 1024 contadores e aloca 1M Strings (~{} ms por leitura). A crate também não compila sem a feature \
                 backtrace nem com a feature fs (dependência libc atrás de cfg(linux)).",
                yes_no(attr),
                match demo_aborted("alloc_track_thread_limit") {
                    Some(true) => "abortou com SIGABRT",
                    Some(false) => "não abortou",
                    None => "n/d",
                },
                read_ns.map_or("n/d".to_string(), |x| format!("{:.0}", x / 1e6)),
            ),
        ),
        "cand-jqf-resource" => (
            Fit::DoesNotFit,
            format!(
                "Conta por thread (RequestAccount !Send instalada na thread) e já implementa o desenho limite + recusa \
                 cooperativa (slab de emergência de 1 MiB e cooperative_refusal no checkpoint), mas a liberação é \
                 descontada de quem libera, com piso em zero (A aloca e B libera: {}; A fica com a carga pra sempre), não \
                 há leitura de fora pro kernel, e um pedido acima do slab depois do teto aborta o host (demonstração de \
                 4 MiB: {}). Sort {sort_s}, alocações pequenas {small_s}.",
                yes_no(attr),
                match demo_aborted("jqf_past_ceiling_4mib") {
                    Some(true) => "SIGABRT",
                    Some(false) => "sobreviveu",
                    None => "n/d",
                },
            ),
        ),
        "cand-alloc-count" | "cand-allocation-counter" => (
            Fit::DoesNotFit,
            format!(
                "Contadores em thread_local: a liberação feita por outra thread conta pra quem libera (A aloca e B libera: \
                 {}; A fica positivo e B negativo), {}. Sort {sort_s}, alocações pequenas {small_s} numa thread e \
                 {mt_s} em 16 (contadores globais atômicos atualizados a cada evento, quando a crate os tem).",
                yes_no(attr),
                if flag("self_read") {
                    "só a própria thread lê (sem leitura de fora); o escopo de exclusão é API escondida (IgnoreGuard)"
                } else {
                    "e o número só sai quando o measure() termina, então não serve pra checkpoint nem pra vigia"
                },
            ),
        ),
        "cand-jemalloc" => (
            Fit::DoesNotFit,
            format!(
                "Contadores por thread que o próprio jemalloc já mantém (thread.allocatedp/deallocatedp): custo zero no \
                 caminho da alocação (sort {sort_s}, alocações pequenas {small_s} contra o System, efeito do jemalloc em \
                 si), mas a liberação conta pra quem libera (A aloca e B libera: {}), o ponteiro do contador é !Send (só a \
                 própria thread lê) e os números são do tamanho de classe, não do pedido. Os contadores por arena, que \
                 atribuiriam a liberação certa, só existem na API raw (unsafe) e o jemalloc limita a 4095 arenas.",
                yes_no(attr),
            ),
        ),
        "cand-cap" => (
            Fit::DoesNotFit,
            format!(
                "Só contador e teto globais (do host inteiro). Serve de demonstração do limite duro: com o teto armado, o \
                 pedido acima dele vira handle_alloc_error e o host inteiro morre ({}), vizinhos inclusive; só \
                 try_reserve sobrevive ({}). Sort {sort_s}; alocações pequenas em 16 threads {mt_s} (o contador \
                 global vira disputa de linha de cache).",
                match demo_aborted("hard_limit_cap") {
                    Some(true) => "SIGABRT",
                    Some(false) => "não abortou",
                    None => "n/d",
                },
                match demo_aborted("hard_limit_cap_try_reserve") {
                    Some(false) => "sobreviveu",
                    Some(true) => "abortou",
                    None => "n/d",
                },
            ),
        ),
        _ => (
            Fit::DoesNotFit,
            format!(
                "Só contador global, sem grupos por thread. Sort {sort_s}; alocações pequenas {small_s} numa thread e \
                 {mt_s} em 16 (o custo de um contador global compartilhado aparece com várias threads)."
            ),
        ),
    }
}
