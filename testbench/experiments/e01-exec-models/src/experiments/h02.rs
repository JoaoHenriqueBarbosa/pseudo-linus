//! H02: custo de troca (ping-pong) e throughput de pipeline nos três modelos.
//!
//! - Ping-pong por `yield_now` com 1 CPU virtual: ns por troca efetiva (contada pelo kernel).
//! - Ping-pong de 1 byte por pipe com 1 e 2 CPUs virtuais: ns por ida e volta.
//! - Linhas de base fora do kernel: `resume`+`suspend` cru do corosensei e handoff cru entre duas threads
//!   do SO com park/unpark.
//! - Pipelines com buffer de 64 KiB: `yes | head -n N`, 4 estágios em bloco (16 KiB por chamada) e 4
//!   estágios em registros de 64 bytes (estresse de trocas), com 1 e 4 CPUs virtuais.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;

use corosensei::Coroutine;
use harness::Verdict;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::json;

use super::{AsyncStage, HypOut, PipelineRun, SyncStage, is_sync, progress, run_pipeline_c, run_pipeline_sync, status_str};
use crate::ModelSpec;
use crate::kernel::{ExitStatus, SIGPIPE, new_pipe};
use crate::stats::{median_f64, r3};
use crate::sys::{ProcMain, Sys};
use crate::workloads::{CHUNK, WcOut, asyncs, sync};

const REPS: usize = 3;

#[derive(Clone, Debug, Serialize)]
pub struct SwitchRow {
    pub model: String,
    pub test: String,
    pub ncpus: usize,
    /// ns por troca (yield) ou por ida e volta (pipe), mediana das repetições.
    pub ns: f64,
    pub samples: Vec<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PipeRow {
    pub model: String,
    pub workload: String,
    pub ncpus: usize,
    pub mb_per_s: f64,
    pub samples: Vec<f64>,
    /// CPU do host gasta por run (todas as threads), mediana, em ms.
    pub cpu_ms: f64,
    /// CPU do host dividida pelo tempo de parede: quantos núcleos o pipeline ocupou em média.
    pub cores_used: f64,
    pub statuses: Vec<String>,
    pub verified: bool,
}

fn iters_for(spec: ModelSpec, slow: u64, fast: u64) -> u64 {
    if spec.kind == crate::ModelKind::A { slow } else { fast }
}

/// Ping-pong por `yield_now` com 1 CPU virtual. Devolve ns por troca efetiva.
fn yield_pingpong(spec: ModelSpec, iters: u64) -> f64 {
    let ready = Arc::new(AtomicUsize::new(0));
    let start: Arc<Mutex<Option<(Instant, u64)>>> = Arc::new(Mutex::new(None));
    if is_sync(spec) {
        let k = spec.sync_kernel(1, false).expect("sync");
        let core = k.core().clone();
        let mk = |ready: Arc<AtomicUsize>, start: Arc<Mutex<Option<(Instant, u64)>>>| -> ProcMain {
            Box::new(move |sys: &dyn Sys| {
                ready.fetch_add(1, Ordering::AcqRel);
                while ready.load(Ordering::Acquire) < 2 {
                    sys.yield_now();
                }
                {
                    let mut s = start.lock();
                    if s.is_none() {
                        *s = Some((Instant::now(), sys.core().switches.load(Ordering::Relaxed)));
                    }
                }
                for _ in 0..iters {
                    sys.yield_now();
                }
                0
            })
        };
        let p1 = k.spawn(Vec::new(), mk(ready.clone(), start.clone()));
        let p2 = k.spawn(Vec::new(), mk(ready.clone(), start.clone()));
        k.wait(p1);
        k.wait(p2);
        let end = Instant::now();
        let (t0, s0) = start.lock().expect("início");
        let switches = core.switches.load(Ordering::Relaxed) - s0;
        (end - t0).as_nanos() as f64 / switches.max(1) as f64
    } else {
        let k = spec.c_kernel(1, false);
        let core = k.core().clone();
        let mk = |ready: Arc<AtomicUsize>, start: Arc<Mutex<Option<(Instant, u64)>>>| {
            move |ctx: crate::model_c::CtxC| async move {
                ready.fetch_add(1, Ordering::AcqRel);
                while ready.load(Ordering::Acquire) < 2 {
                    ctx.yield_now().await;
                }
                {
                    let mut s = start.lock();
                    if s.is_none() {
                        *s = Some((Instant::now(), ctx.core().switches.load(Ordering::Relaxed)));
                    }
                }
                for _ in 0..iters {
                    ctx.yield_now().await;
                }
                0
            }
        };
        let p1 = k.spawn(Vec::new(), mk(ready.clone(), start.clone()));
        let p2 = k.spawn(Vec::new(), mk(ready.clone(), start.clone()));
        k.wait(p1);
        k.wait(p2);
        let end = Instant::now();
        let (t0, s0) = start.lock().expect("início");
        let switches = core.switches.load(Ordering::Relaxed) - s0;
        (end - t0).as_nanos() as f64 / switches.max(1) as f64
    }
}

/// Ping-pong de 1 byte por dois pipes. Devolve ns por ida e volta.
fn pipe_pingpong(spec: ModelSpec, ncpus: usize, iters: u64) -> f64 {
    let (r_ab, w_ab) = new_pipe();
    let (r_ba, w_ba) = new_pipe();
    let t0 = Instant::now();
    if is_sync(spec) {
        let k = spec.sync_kernel(ncpus, false).expect("sync");
        let p1 = k.spawn(
            vec![(0, r_ba), (1, w_ab)],
            Box::new(move |sys: &dyn Sys| {
                let mut b = [0u8; 1];
                for _ in 0..iters {
                    sys.write(1, &[1]).expect("write");
                    sys.read(0, &mut b).expect("read");
                }
                0
            }),
        );
        let p2 = k.spawn(
            vec![(0, r_ab), (1, w_ba)],
            Box::new(move |sys: &dyn Sys| {
                let mut b = [0u8; 1];
                for _ in 0..iters {
                    sys.read(0, &mut b).expect("read");
                    sys.write(1, &b).expect("write");
                }
                0
            }),
        );
        k.wait(p1);
        k.wait(p2);
    } else {
        let k = spec.c_kernel(ncpus, false);
        let p1 = k.spawn(vec![(0, r_ba), (1, w_ab)], move |ctx| async move {
            let mut b = [0u8; 1];
            for _ in 0..iters {
                ctx.write(1, &[1]).await.expect("write");
                ctx.read(0, &mut b).await.expect("read");
            }
            0
        });
        let p2 = k.spawn(vec![(0, r_ab), (1, w_ba)], move |ctx| async move {
            let mut b = [0u8; 1];
            for _ in 0..iters {
                ctx.read(0, &mut b).await.expect("read");
                ctx.write(1, &b).await.expect("write");
            }
            0
        });
        k.wait(p1);
        k.wait(p2);
    }
    t0.elapsed().as_nanos() as f64 / iters as f64
}

/// Linha de base: `resume` + `suspend` cru do corosensei (duas trocas de pilha por iteração).
fn raw_coroutine(iters: u64) -> f64 {
    let mut co: Coroutine<(), (), ()> = Coroutine::new(|y, ()| {
        loop {
            y.suspend(());
        }
    });
    let t0 = Instant::now();
    for _ in 0..iters {
        co.resume(());
    }
    t0.elapsed().as_nanos() as f64 / (2 * iters) as f64
}

/// Linha de base: handoff cru entre duas threads do SO com park/unpark (o mecanismo do modelo A sem o
/// kernel em volta). Devolve ns por handoff.
fn raw_thread_handoff(iters: u64) -> f64 {
    let turn = Arc::new(AtomicU64::new(0));
    let total = 2 * iters;
    let main_t = thread::current();
    let t2 = turn.clone();
    let other = thread::spawn(move || {
        loop {
            let t = t2.load(Ordering::Acquire);
            if t >= total {
                break;
            }
            if t % 2 == 1 {
                t2.store(t + 1, Ordering::Release);
                main_t.unpark();
            } else {
                thread::park();
            }
        }
        main_t.unpark();
    });
    let other_t = other.thread().clone();
    let t0 = Instant::now();
    loop {
        let t = turn.load(Ordering::Acquire);
        if t >= total {
            break;
        }
        if t.is_multiple_of(2) {
            turn.store(t + 1, Ordering::Release);
            other_t.unpark();
        } else {
            thread::park();
        }
    }
    let el = t0.elapsed();
    other.join().expect("thread");
    el.as_nanos() as f64 / total as f64
}

fn sync_yes_head(lines: u64) -> Vec<SyncStage> {
    vec![Box::new(sync::yes), Box::new(move |s: &dyn Sys| sync::head(s, lines))]
}

fn async_yes_head(lines: u64) -> Vec<AsyncStage> {
    vec![
        Box::new(|c| Box::pin(asyncs::yes(c))),
        Box::new(move |c| Box::pin(asyncs::head(c, lines))),
    ]
}

fn sync_four(total: u64, rec: usize, out: Arc<WcOut>) -> Vec<SyncStage> {
    if rec == CHUNK {
        vec![
            Box::new(move |s: &dyn Sys| sync::gen_text(s, total)),
            Box::new(sync::upper),
            Box::new(move |s: &dyn Sys| sync::relay(s, CHUNK)),
            Box::new(move |s: &dyn Sys| sync::wc(s, &out, CHUNK)),
        ]
    } else {
        vec![
            Box::new(move |s: &dyn Sys| sync::gen_records(s, total, rec)),
            Box::new(move |s: &dyn Sys| sync::relay(s, rec)),
            Box::new(move |s: &dyn Sys| sync::relay(s, rec)),
            Box::new(move |s: &dyn Sys| sync::wc(s, &out, rec)),
        ]
    }
}

fn async_four(total: u64, rec: usize, out: Arc<WcOut>) -> Vec<AsyncStage> {
    if rec == CHUNK {
        vec![
            Box::new(move |c| Box::pin(asyncs::gen_text(c, total))),
            Box::new(|c| Box::pin(asyncs::upper(c))),
            Box::new(move |c| Box::pin(asyncs::relay(c, CHUNK))),
            Box::new(move |c| Box::pin(asyncs::wc(c, out, CHUNK))),
        ]
    } else {
        vec![
            Box::new(move |c| Box::pin(asyncs::gen_records(c, total, rec))),
            Box::new(move |c| Box::pin(asyncs::relay(c, rec))),
            Box::new(move |c| Box::pin(asyncs::relay(c, rec))),
            Box::new(move |c| Box::pin(asyncs::wc(c, out, rec))),
        ]
    }
}

/// Parâmetros de tamanho (encolhidos nos testes).
#[derive(Clone, Copy, Debug)]
pub struct Sizes {
    pub yes_lines: u64,
    pub bulk_bytes: u64,
    pub record_bytes: u64,
    pub record_size: usize,
    pub yield_iters_a: u64,
    pub yield_iters_fast: u64,
    pub pipe_iters_a: u64,
    pub pipe_iters_fast: u64,
    pub reps: usize,
}

impl Sizes {
    pub const FULL: Sizes = Sizes {
        yes_lines: 64 << 20,
        bulk_bytes: 128 << 20,
        record_bytes: 4 << 20,
        record_size: 64,
        yield_iters_a: 30_000,
        yield_iters_fast: 500_000,
        pipe_iters_a: 20_000,
        pipe_iters_fast: 200_000,
        reps: REPS,
    };
}

fn pipeline(spec: ModelSpec, ncpus: usize, workload: &str, sz: Sizes) -> PipeRow {
    let mut samples = Vec::new();
    let mut cpu_ms = Vec::new();
    let mut cores = Vec::new();
    let mut statuses = Vec::new();
    let mut verified = true;
    for _ in 0..sz.reps {
        let out = Arc::new(WcOut::default());
        let (bytes, run): (u64, PipelineRun) = match (workload, is_sync(spec)) {
            ("yes_head", true) => (2 * sz.yes_lines, run_pipeline_sync(spec, ncpus, sync_yes_head(sz.yes_lines))),
            ("yes_head", false) => (2 * sz.yes_lines, run_pipeline_c(ncpus, async_yes_head(sz.yes_lines))),
            ("bulk4", true) => (sz.bulk_bytes, run_pipeline_sync(spec, ncpus, sync_four(sz.bulk_bytes, CHUNK, out.clone()))),
            ("bulk4", false) => (sz.bulk_bytes, run_pipeline_c(ncpus, async_four(sz.bulk_bytes, CHUNK, out.clone()))),
            ("records4", true) => (
                sz.record_bytes,
                run_pipeline_sync(spec, ncpus, sync_four(sz.record_bytes, sz.record_size, out.clone())),
            ),
            ("records4", false) => {
                (sz.record_bytes, run_pipeline_c(ncpus, async_four(sz.record_bytes, sz.record_size, out.clone())))
            }
            _ => unreachable!("workload desconhecido"),
        };
        let ok = match workload {
            "yes_head" => {
                run.sink_bytes == bytes
                    && run.statuses[0] == ExitStatus::Signaled(SIGPIPE)
                    && run.statuses[1] == ExitStatus::Exited(0)
            }
            _ => out.bytes.load(Ordering::Relaxed) == bytes && run.statuses.iter().all(|s| *s == ExitStatus::Exited(0)),
        };
        verified &= ok;
        statuses = run.statuses.iter().map(status_str).collect();
        samples.push(r3(bytes as f64 / run.elapsed.as_secs_f64() / 1e6));
        cpu_ms.push(run.cpu.as_secs_f64() * 1e3);
        cores.push(run.cpu.as_secs_f64() / run.elapsed.as_secs_f64());
    }
    PipeRow {
        model: spec.label(),
        workload: workload.to_string(),
        ncpus,
        mb_per_s: r3(median_f64(&samples)),
        samples,
        cpu_ms: r3(median_f64(&cpu_ms)),
        cores_used: r3(median_f64(&cores)),
        statuses,
        verified,
    }
}

pub struct H02Data {
    pub switches: Vec<SwitchRow>,
    pub pipes: Vec<PipeRow>,
    pub raw_coroutine_ns: f64,
    pub raw_thread_handoff_ns: f64,
    /// Referência: os mesmos pipelines com processos de verdade do Linux (coreutils), MB/s.
    pub native: Vec<NativeRow>,
}

#[derive(Clone, Debug, Serialize)]
pub struct NativeRow {
    pub workload: String,
    pub command: String,
    pub mb_per_s: Option<f64>,
    pub samples: Vec<f64>,
}

/// Roda um pipeline de verdade no shell do host e devolve MB/s (bytes lógicos / tempo de parede).
fn native(workload: &str, command: String, bytes: u64, reps: usize) -> NativeRow {
    let mut samples = Vec::new();
    for _ in 0..reps {
        let t = Instant::now();
        let ok = std::process::Command::new("sh")
            .args(["-c", &command])
            .stdout(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            return NativeRow { workload: workload.into(), command, mb_per_s: None, samples };
        }
        samples.push(r3(bytes as f64 / t.elapsed().as_secs_f64() / 1e6));
    }
    NativeRow { workload: workload.into(), command, mb_per_s: Some(r3(median_f64(&samples))), samples }
}

pub fn measure(sz: Sizes) -> H02Data {
    let mut switches = Vec::new();
    for spec in ModelSpec::PERF {
        progress(format!("H02: ping-pong {}", spec.label()));
        let it = iters_for(spec, sz.yield_iters_a, sz.yield_iters_fast);
        let samples: Vec<f64> = (0..sz.reps).map(|_| r3(yield_pingpong(spec, it))).collect();
        switches.push(SwitchRow { model: spec.label(), test: "yield".into(), ncpus: 1, ns: r3(median_f64(&samples)), samples });
        for ncpus in [1, 2] {
            let it = iters_for(spec, sz.pipe_iters_a, sz.pipe_iters_fast);
            let samples: Vec<f64> = (0..sz.reps).map(|_| r3(pipe_pingpong(spec, ncpus, it))).collect();
            switches.push(SwitchRow {
                model: spec.label(),
                test: "pipe_roundtrip".into(),
                ncpus,
                ns: r3(median_f64(&samples)),
                samples,
            });
        }
    }
    progress("H02: linhas de base cruas");
    let raw_coroutine_ns = r3(median_f64(&(0..sz.reps).map(|_| raw_coroutine(sz.yield_iters_fast * 4)).collect::<Vec<_>>()));
    let raw_thread_handoff_ns =
        r3(median_f64(&(0..sz.reps).map(|_| raw_thread_handoff(sz.yield_iters_a)).collect::<Vec<_>>()));

    let mut pipes = Vec::new();
    for workload in ["yes_head", "bulk4", "records4"] {
        for ncpus in [1, 4] {
            for spec in ModelSpec::PERF {
                progress(format!("H02: {workload} {} com {ncpus} CPU(s)", spec.label()));
                pipes.push(pipeline(spec, ncpus, workload, sz));
            }
        }
    }
    progress("H02: os mesmos pipelines com processos reais do Linux");
    let native_rows = vec![
        native(
            "yes_head",
            format!("yes | head -n {} > /dev/null", sz.yes_lines),
            2 * sz.yes_lines,
            sz.reps,
        ),
        native(
            "bulk4",
            format!("head -c {} /dev/zero | tr a-z A-Z | cat | wc -lc > /dev/null", sz.bulk_bytes),
            sz.bulk_bytes,
            sz.reps,
        ),
    ];
    H02Data { switches, pipes, raw_coroutine_ns, raw_thread_handoff_ns, native: native_rows }
}

fn find_switch<'a>(d: &'a H02Data, model: &str, test: &str, ncpus: usize) -> Option<&'a SwitchRow> {
    d.switches.iter().find(|r| r.model == model && r.test == test && r.ncpus == ncpus)
}

pub fn verdict(d: &H02Data) -> HypOut {
    // Razão (melhor variante do A) / (melhor de B64, B256, C) em cada pipeline. A variante com espera
    // ativa é uma implementação legítima do modelo A, então entra; a CPU extra que ela gasta vai junto.
    let mut ratios = Vec::new();
    for workload in ["yes_head", "bulk4", "records4"] {
        for ncpus in [1, 4] {
            let row = |m: &str| d.pipes.iter().find(|r| r.model == m && r.workload == workload && r.ncpus == ncpus);
            let get = |m: &str| row(m).map(|r| r.mb_per_s).unwrap_or(0.0);
            let cores = |m: &str| row(m).map(|r| r.cores_used).unwrap_or(0.0);
            let (a_name, a) = ["A", "A-spin"]
                .iter()
                .map(|m| (*m, get(m)))
                .fold(("", 0.0f64), |acc, x| if x.1 > acc.1 { x } else { acc });
            let (best_name, best) = ["B64", "B256", "C"]
                .iter()
                .map(|m| (*m, get(m)))
                .fold(("", 0.0f64), |acc, x| if x.1 > acc.1 { x } else { acc });
            ratios.push(json!({
                "workload": workload, "ncpus": ncpus,
                "a_park_mb_s": get("A"), "a_spin_mb_s": get("A-spin"),
                "a_park_cores": cores("A"), "a_spin_cores": cores("A-spin"),
                "best_a": a_name, "best_a_mb_s": a,
                "best_other": best_name, "best_other_mb_s": best, "best_other_cores": cores(best_name),
                "a_over_best": r3(if best > 0.0 { a / best } else { 0.0 }),
                "a_park_over_best": r3(if best > 0.0 { get("A") / best } else { 0.0 }),
            }));
        }
    }
    let decisive: Vec<&serde_json::Value> =
        ratios.iter().filter(|r| r["workload"] == "yes_head" || r["workload"] == "bulk4").collect();
    let worst_row = decisive
        .iter()
        .min_by(|a, b| {
            a["a_over_best"].as_f64().unwrap_or(0.0).total_cmp(&b["a_over_best"].as_f64().unwrap_or(0.0))
        })
        .copied();
    let worst_bulk = worst_row.and_then(|r| r["a_over_best"].as_f64()).unwrap_or(0.0);
    let worst_desc = worst_row
        .map(|r| {
            format!(
                "{} com {} CPU(s): {} {:.0} MB/s contra {} {:.0} MB/s",
                r["workload"].as_str().unwrap_or("?"),
                r["ncpus"],
                r["best_a"].as_str().unwrap_or("?"),
                r["best_a_mb_s"].as_f64().unwrap_or(0.0),
                r["best_other"].as_str().unwrap_or("?"),
                r["best_other_mb_s"].as_f64().unwrap_or(0.0)
            )
        })
        .unwrap_or_default();
    let native_of = |w: &str| d.native.iter().find(|n| n.workload == w).and_then(|n| n.mb_per_s);
    let native_desc = format!(
        "Referência com processos reais do Linux: yes|head {} MB/s, 4 estágios {} MB/s.",
        native_of("yes_head").map(|x| format!("{x:.0}")).unwrap_or("?".into()),
        native_of("bulk4").map(|x| format!("{x:.0}")).unwrap_or("?".into()),
    );
    let worst_records = ratios
        .iter()
        .filter(|r| r["workload"] == "records4")
        .map(|r| r["a_over_best"].as_f64().unwrap_or(0.0))
        .fold(f64::INFINITY, f64::min);
    let a_park_yield = find_switch(d, "A", "yield", 1).map(|r| r.ns).unwrap_or(0.0);
    let a_spin_yield = find_switch(d, "A-spin", "yield", 1).map(|r| r.ns).unwrap_or(0.0);
    let a_yield = if a_spin_yield > 0.0 { a_park_yield.min(a_spin_yield) } else { a_park_yield };
    let b_yield = find_switch(d, "B64", "yield", 1).map(|r| r.ns).unwrap_or(0.0);
    let c_yield = find_switch(d, "C", "yield", 1).map(|r| r.ns).unwrap_or(0.0);
    let best_fast = b_yield.min(c_yield).max(0.001);
    let switch_ratio = a_yield / best_fast;
    let all_verified = d.pipes.iter().all(|p| p.verified);

    let switch_desc = format!(
        "Troca por yield com 1 CPU virtual: A {a_park_yield:.0} ns (park/unpark), A-spin {a_spin_yield:.0} ns, B64 \
         {b_yield:.0} ns, C {c_yield:.0} ns; resume+suspend cru do corosensei {:.1} ns, handoff cru entre threads {:.0} ns.",
        d.raw_coroutine_ns, d.raw_thread_handoff_ns
    );
    let (verdict, head) = if !all_verified {
        (Verdict::Inconclusive, "Algum pipeline não produziu a saída esperada.".to_string())
    } else if worst_bulk < 0.8 {
        (
            Verdict::Confirmed,
            format!(
                "A perde {:.0}% de throughput de pipeline em bloco pro melhor de B/C no pior caso ({worst_desc}).",
                (1.0 - worst_bulk) * 100.0
            ),
        )
    } else if switch_ratio > 5.0 {
        (
            Verdict::Partial,
            format!(
                "A troca no A é {switch_ratio:.0}x mais cara que em B/C, mas nos pipelines em bloco (yes|head e 4 \
                 estágios de 16 KiB) A fica em no mínimo {:.0}% do melhor de B/C ({worst_desc}).",
                worst_bulk * 100.0
            ),
        )
    } else {
        (Verdict::Refuted, format!("A troca no A é só {switch_ratio:.1}x a de B/C."))
    };
    let summary = format!(
        "{head} {switch_desc} Com registros de 64 bytes por chamada (estresse de trocas) A fica em {:.0}% do melhor. \
         {native_desc}",
        worst_records * 100.0
    );
    HypOut {
        id: "H02",
        verdict,
        summary,
        evidence: json!({
            "switch_ns": d.switches,
            "raw_coroutine_switch_ns": d.raw_coroutine_ns,
            "raw_thread_handoff_ns": d.raw_thread_handoff_ns,
            "a_over_b_c_yield_cost": r3(switch_ratio),
            "pipelines": d.pipes,
            "a_vs_best_other": ratios,
            "worst_a_over_best_bulk": r3(worst_bulk),
            "worst_a_over_best_records": r3(worst_records),
            "native_linux_pipelines": d.native,
            "pipe_capacity_bytes": crate::pipe::PIPE_CAPACITY,
        }),
    }
}

/// Ping-pong só pra testes (sem estatística).
pub fn quick_yield_pingpong(spec: ModelSpec, iters: u64) -> f64 {
    yield_pingpong(spec, iters)
}

/// Pipeline só pra testes.
pub fn quick_pipeline(spec: ModelSpec, ncpus: usize, workload: &str, sz: Sizes) -> PipeRow {
    pipeline(spec, ncpus, workload, sz)
}
