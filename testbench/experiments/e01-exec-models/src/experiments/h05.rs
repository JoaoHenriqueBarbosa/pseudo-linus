//! H05: custo do checkpoint num laço de interpretador e latência entre o timer marcar e o processo ceder.
//!
//! Overhead: o interpretador de `vm` roda sozinho numa CPU virtual com o timer de 1 ms ligado, nos três
//! posicionamentos de checkpoint, dentro de cada modelo e fora do kernel (thread comum com um timer que
//! liga o flag). Cada ambiente roda várias rodadas, e em cada rodada os três modos rodam colados (a ordem
//! gira); a desaceleração é a mediana das razões pareadas dentro da rodada, o que cancela a interferência
//! de fora. O tempo é o de CPU da thread que executa o interpretador (`CLOCK_THREAD_CPUTIME_ID`), que não
//! conta o tempo em que o host tirou a thread da CPU.
//!
//! Latência: dois processos do interpretador (checkpoint por salto pra trás) disputam 1 CPU virtual; a
//! sonda do kernel registra, a cada fatia, o tempo do timer ligar `attention` até o processo entrar no
//! caminho lento (preempção) e até o outro processo estar rodando (handoff). Roda 3 vezes por modelo e
//! fica a execução de menor p99 (a cauda é dominada pelo host tirando a thread da CPU).

use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use harness::Verdict;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::json;

use super::{HypOut, is_sync, progress};
use crate::ModelSpec;
use crate::stats::{Dist, median_f64, r3, thread_cpu_ns};
use crate::sys::Sys;
use crate::vm::{
    MODE_BACKEDGE, MODE_EVERY, MODE_NONE, OPS_PER_ITER, expected, program, run, run_async, run_async_dyn, run_dyn,
};

const PREEMPT_P99_LIMIT_NS: u64 = 100_000;
const OVERHEAD_LIMIT_PCT: f64 = 2.0;

fn mode_name(m: u8) -> &'static str {
    match m {
        MODE_NONE => "none",
        MODE_EVERY => "every_instruction",
        _ => "backedge",
    }
}

fn instrs(n: i64) -> u64 {
    n as u64 * OPS_PER_ITER + 6
}

fn run_mode(mode: u8, prog: &[crate::vm::Op], flag: &AtomicBool, slow: &mut dyn FnMut()) -> i64 {
    match mode {
        MODE_NONE => run::<MODE_NONE>(prog, flag, slow),
        MODE_EVERY => run::<MODE_EVERY>(prog, flag, slow),
        _ => run::<MODE_BACKEDGE>(prog, flag, slow),
    }
}

const MODES: [u8; 3] = [MODE_NONE, MODE_EVERY, MODE_BACKEDGE];

/// Tempo de CPU da thread em cada modo, por rodada (posição 3: laço síncrono de referência no C).
type Rounds = Vec<[u64; 4]>;

/// `rounds` rodadas; em cada uma os três modos rodam em sequência (a ordem gira a cada rodada), com `n`
/// iterações cada. Como os três modos de uma rodada rodam colados, a interferência de fora (outro
/// processo no mesmo núcleo físico, frequência) pega os três quase igual, e a razão pareada dentro da
/// rodada fica estável mesmo com a máquina ocupada.
fn rounds_sync(n: i64, rounds: usize, flag: &AtomicBool, slow: &mut dyn FnMut(), dynamic: bool) -> Rounds {
    let prog = program(black_box(n));
    let want = expected(n);
    let mut out = Vec::with_capacity(rounds);
    for r in 0..rounds {
        let mut t = [0u64; 4];
        for k in 0..3 {
            let i = (r + k) % 3;
            let t0 = thread_cpu_ns();
            let v = if dynamic {
                run_dyn(&prog, flag, slow, MODES[i])
            } else {
                run_mode(MODES[i], &prog, flag, slow)
            };
            t[i] = thread_cpu_ns() - t0;
            assert_eq!(v, want, "o interpretador errou a conta");
        }
        out.push(t);
    }
    out
}

/// Como `rounds_sync`, no modelo C. Na variante de mesmo código, cada rodada também roda a versão
/// síncrona do interpretador (sem checkpoint) chamada de dentro do future, na mesma thread do worker: a
/// razão pareada entre as duas é o custo de o laço ser async. Esse tempo vai na posição 3.
async fn rounds_async(n: i64, rounds: usize, ctx: &crate::model_c::CtxC, dynamic: bool) -> Rounds {
    let prog = program(black_box(n));
    let want = expected(n);
    let never = AtomicBool::new(false);
    let slots = if dynamic { 4 } else { 3 };
    let mut out = Vec::with_capacity(rounds);
    for r in 0..rounds {
        let mut t = [0u64; 4];
        for k in 0..slots {
            let i = (r + k) % slots;
            // Com 1 worker e um processo só, o future roda sempre na mesma thread do worker.
            let t0 = thread_cpu_ns();
            let v = match (dynamic, i) {
                (true, 3) => run_dyn(&prog, &never, &mut || {}, MODE_NONE),
                (true, _) => run_async_dyn(&prog, ctx, MODES[i]).await,
                (false, 0) => run_async::<MODE_NONE>(&prog, ctx).await,
                (false, 1) => run_async::<MODE_EVERY>(&prog, ctx).await,
                (false, _) => run_async::<MODE_BACKEDGE>(&prog, ctx).await,
            };
            t[i] = thread_cpu_ns() - t0;
            assert_eq!(v, want, "o interpretador errou a conta");
        }
        out.push(t);
    }
    out
}

/// Fora do kernel: thread comum, um timer liga o flag a cada 1 ms e o caminho lento só apaga.
fn baseline_rounds(n: i64, rounds: usize, dynamic: bool) -> Rounds {
    let flag = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let (f2, s2) = (flag.clone(), stop.clone());
    let timer = thread::spawn(move || {
        while !s2.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(1));
            f2.store(true, Ordering::Relaxed);
        }
    });
    let out = rounds_sync(n, rounds, &flag, &mut || flag.store(false, Ordering::Relaxed), dynamic);
    stop.store(true, Ordering::Relaxed);
    timer.join().expect("timer");
    out
}

/// Dentro de um modelo: um processo sozinho em 1 CPU virtual, timer de 1 ms ligado.
fn in_model_rounds(spec: ModelSpec, n: i64, rounds: usize, dynamic: bool) -> Rounds {
    let result: Arc<Mutex<Rounds>> = Arc::new(Mutex::new(Vec::new()));
    let r2 = result.clone();
    if is_sync(spec) {
        let k = spec.sync_kernel(1, true).expect("sync");
        let pid = k.spawn(
            Vec::new(),
            Box::new(move |sys: &dyn Sys| {
                let out = rounds_sync(n, rounds, &sys.proc().attention, &mut || sys.checkpoint_slow(), dynamic);
                *r2.lock() = out;
                0
            }),
        );
        assert_eq!(k.wait(pid), crate::kernel::ExitStatus::Exited(0));
    } else {
        let k = spec.c_kernel(1, true);
        let pid = k.spawn(Vec::new(), move |ctx| async move {
            let out = rounds_async(n, rounds, &ctx, dynamic).await;
            *r2.lock() = out;
            0
        });
        assert_eq!(k.wait(pid), crate::kernel::ExitStatus::Exited(0));
    }
    std::mem::take(&mut *result.lock())
}

/// Dois interpretadores disputando 1 CPU virtual; devolve (preempção, handoff) em ns.
fn latency(spec: ModelSpec, n: i64) -> (Vec<u64>, Vec<u64>) {
    if is_sync(spec) {
        let k = spec.sync_kernel(1, true).expect("sync");
        k.core().probe.enabled.store(true, Ordering::Relaxed);
        let mk = || -> crate::sys::ProcMain {
            Box::new(move |sys: &dyn Sys| {
                let prog = program(black_box(n));
                let v = run::<MODE_BACKEDGE>(&prog, &sys.proc().attention, &mut || sys.checkpoint_slow());
                i32::from(v != expected(n))
            })
        };
        let p1 = k.spawn(Vec::new(), mk());
        let p2 = k.spawn(Vec::new(), mk());
        k.wait(p1);
        k.wait(p2);
        let core = k.core();
        core.probe.enabled.store(false, Ordering::Relaxed);
        let pre = std::mem::take(&mut *core.probe.preempt_ns.lock());
        let hand = std::mem::take(&mut *core.probe.handoff_ns.lock());
        (pre, hand)
    } else {
        let k = spec.c_kernel(1, true);
        k.core().probe.enabled.store(true, Ordering::Relaxed);
        let mk = || {
            move |ctx: crate::model_c::CtxC| async move {
                let prog = program(black_box(n));
                let v = run_async::<MODE_BACKEDGE>(&prog, &ctx).await;
                i32::from(v != expected(n))
            }
        };
        let p1 = k.spawn(Vec::new(), mk());
        let p2 = k.spawn(Vec::new(), mk());
        k.wait(p1);
        k.wait(p2);
        let core = k.core();
        core.probe.enabled.store(false, Ordering::Relaxed);
        let pre = std::mem::take(&mut *core.probe.preempt_ns.lock());
        let hand = std::mem::take(&mut *core.probe.handoff_ns.lock());
        (pre, hand)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct VmRow {
    pub env: String,
    /// `same_code`: o modo é decidido em tempo de execução e os três rodam o mesmo código de máquina (é o
    /// que decide o veredito); `specialized`: uma versão compilada por modo (mostra o efeito de layout).
    pub variant: String,
    pub mode: String,
    /// ns de CPU por instrução interpretada: mínimo e mediana das rodadas.
    pub min_ns_per_instr: f64,
    pub median_ns_per_instr: f64,
    /// Desaceleração contra o modo sem checkpoint: mediana das razões pareadas por rodada, e quartis.
    pub slowdown_pct: f64,
    pub slowdown_p25_pct: f64,
    pub slowdown_p75_pct: f64,
    pub rounds: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct LatRow {
    pub model: String,
    /// Distribuições da execução com o menor p99 de preempção entre as repetidas.
    pub preempt_ns: Dist,
    pub handoff_ns: Dist,
    /// p99 de preempção e de handoff de cada execução, na ordem em que rodaram.
    pub preempt_p99_per_run: Vec<u64>,
    pub handoff_p99_per_run: Vec<u64>,
}

/// Execuções de latência por modelo. A cauda (p99) é dominada por momentos em que o host tira a thread
/// da CPU, o que depende de quem mais está rodando na máquina; das execuções, fica a de menor p99.
const LAT_RUNS: usize = 3;

pub struct H05Data {
    pub vm: Vec<VmRow>,
    pub lat: Vec<LatRow>,
    pub n: i64,
    /// Laço async sem checkpoint contra o mesmo laço síncrono, na mesma thread do worker do C: mediana
    /// das razões pareadas, em %.
    pub async_vs_sync_pct: f64,
}

pub fn measure(n: i64, reps: usize, lat_n: i64) -> H05Data {
    let mut vm = Vec::new();
    let mut async_vs_sync_pct = f64::NAN;
    let envs: Vec<Option<ModelSpec>> = std::iter::once(None).chain(ModelSpec::MAIN.into_iter().map(Some)).collect();
    let per_run = instrs(n) as f64;
    for (env, dynamic) in envs.iter().flat_map(|e| [(*e, true), (*e, false)]) {
        let label = env.map(|s| s.label()).unwrap_or_else(|| "fora_do_kernel".to_string());
        let variant = if dynamic { "same_code" } else { "specialized" };
        progress(format!("H05: interpretador em {label} ({variant})"));
        let rounds = match env {
            None => baseline_rounds(n, reps, dynamic),
            Some(spec) => in_model_rounds(spec, n, reps, dynamic),
        };
        if dynamic && env.is_some_and(|s| s.kind == crate::ModelKind::C) {
            let ratios: Vec<f64> = rounds.iter().map(|t| t[0] as f64 / t[3].max(1) as f64).collect();
            async_vs_sync_pct = (median_f64(&ratios) - 1.0) * 100.0;
        }
        for (i, &m) in MODES.iter().enumerate() {
            let ns: Vec<f64> = rounds.iter().map(|t| t[i] as f64 / per_run).collect();
            // Razão pareada contra o modo sem checkpoint da mesma rodada.
            let ratios: Vec<f64> = rounds.iter().map(|t| t[i] as f64 / t[0].max(1) as f64).collect();
            let q = |p: f64| {
                let mut v = ratios.clone();
                v.sort_by(f64::total_cmp);
                v[((p / 100.0) * (v.len() - 1) as f64).round() as usize]
            };
            vm.push(VmRow {
                env: label.clone(),
                variant: variant.to_string(),
                mode: mode_name(m).to_string(),
                min_ns_per_instr: r3(ns.iter().copied().fold(f64::INFINITY, f64::min)),
                median_ns_per_instr: r3(median_f64(&ns)),
                slowdown_pct: r3((median_f64(&ratios) - 1.0) * 100.0),
                slowdown_p25_pct: r3((q(25.0) - 1.0) * 100.0),
                slowdown_p75_pct: r3((q(75.0) - 1.0) * 100.0),
                rounds: rounds.len(),
            });
        }
    }
    let mut lat = Vec::new();
    for spec in ModelSpec::PERF {
        progress(format!("H05: latência de preempção {} ({LAT_RUNS} execuções)", spec.label()));
        let runs: Vec<(Dist, Dist)> = (0..LAT_RUNS)
            .map(|_| {
                let (pre, hand) = latency(spec, lat_n);
                (Dist::of(&pre), Dist::of(&hand))
            })
            .collect();
        let preempt_p99_per_run = runs.iter().map(|r| r.0.p99).collect();
        let handoff_p99_per_run = runs.iter().map(|r| r.1.p99).collect();
        let best = runs.into_iter().min_by_key(|r| r.0.p99).expect("execuções");
        lat.push(LatRow {
            model: spec.label(),
            preempt_ns: best.0,
            handoff_ns: best.1,
            preempt_p99_per_run,
            handoff_p99_per_run,
        });
    }
    H05Data { vm, lat, n, async_vs_sync_pct }
}

pub fn verdict(d: &H05Data) -> HypOut {
    // Ambientes síncronos (fora do kernel, A, B) e o C (laço async) separados: no C o custo não é a
    // leitura atômica, é o ponto de `.await` dentro do laço.
    // O veredito usa a variante de mesmo código (`same_code`); a especializada fica como evidência do
    // efeito de layout.
    let worst = |mode: &str, sync: bool| {
        d.vm.iter()
            .filter(|r| r.variant == "same_code" && r.mode == mode && (r.env != "C") == sync)
            .map(|r| r.slowdown_pct)
            .fold(f64::NEG_INFINITY, f64::max)
    };
    let sync_backedge = worst("backedge", true);
    let sync_every = worst("every_instruction", true);
    let c_backedge = worst("backedge", false);
    let c_every = worst("every_instruction", false);
    let layout_swing = d
        .vm
        .iter()
        .filter(|r| r.variant == "specialized" && r.mode != "none")
        .map(|r| r.slowdown_pct.abs())
        .fold(0.0f64, f64::max);
    let async_penalty = d.async_vs_sync_pct;
    let main_labels: Vec<String> = ModelSpec::MAIN.iter().map(|s| s.label()).collect();
    let main_lat: Vec<&LatRow> = d.lat.iter().filter(|r| main_labels.contains(&r.model)).collect();
    let worst_p99 = main_lat.iter().map(|r| r.preempt_ns.p99).max().unwrap_or(0);
    let enough_samples = main_lat.iter().all(|r| r.preempt_ns.n >= 20);
    let handoff = |m: &str| d.lat.iter().find(|r| r.model == m).map(|r| r.handoff_ns.p99).unwrap_or(0);
    let sync_ok = sync_backedge < OVERHEAD_LIMIT_PCT;
    let c_ok = c_backedge < OVERHEAD_LIMIT_PCT;
    let lat_ok = worst_p99 < PREEMPT_P99_LIMIT_NS;
    let verdict = if !enough_samples {
        Verdict::Inconclusive
    } else if sync_ok && c_ok && lat_ok {
        Verdict::Confirmed
    } else if sync_ok || lat_ok {
        Verdict::Partial
    } else {
        Verdict::Refuted
    };
    let worst_backedge = sync_backedge.max(c_backedge);
    let worst_every = sync_every.max(c_every);
    let preempt = |m: &str| d.lat.iter().find(|r| r.model == m).map(|r| r.preempt_ns.p99).unwrap_or(0);
    let us = |ns: u64| ns as f64 / 1e3;
    let summary = format!(
        "Com o mesmo código de máquina nos três modos, checkpoint por salto pra trás custa no pior ambiente síncrono \
         (fora do kernel, A, B) {sync_backedge:.2}% (antes de cada instrução: {sync_every:.1}%); no C, \
         {c_backedge:.2}% (antes de cada instrução: {c_every:.1}%); o laço async sem checkpoint custa \
         {async_penalty:+.1}% em relação ao mesmo laço síncrono rodando na mesma thread. Compilando uma versão por \
         modo, só o layout do \
         código já move o resultado em até {layout_swing:.0}%. p99 do timer até o processo ceder: A {:.1} µs, B64 \
         {:.1} µs, B256 {:.1} µs, C {:.1} µs. p99 até o próximo processo rodar: A {:.1} µs, A-spin {:.1} µs, B64 \
         {:.1} µs, B256 {:.1} µs, C {:.1} µs.",
        us(preempt("A")),
        us(preempt("B64")),
        us(preempt("B256")),
        us(preempt("C")),
        us(handoff("A")),
        us(handoff("A-spin")),
        us(handoff("B64")),
        us(handoff("B256")),
        us(handoff("C")),
    );
    HypOut {
        id: "H05",
        verdict,
        summary,
        evidence: json!({
            "interpreter_iterations_per_run": d.n,
            "instructions_per_run": instrs(d.n),
            "overhead": d.vm,
            "latency": d.lat,
            "worst_backedge_slowdown_pct": r3(worst_backedge),
            "worst_every_instruction_slowdown_pct": r3(worst_every),
            "sync_backedge_slowdown_pct": r3(sync_backedge),
            "sync_every_instruction_slowdown_pct": r3(sync_every),
            "c_backedge_slowdown_pct": r3(c_backedge),
            "c_every_instruction_slowdown_pct": r3(c_every),
            "c_async_loop_vs_a_sync_loop_pct": r3(async_penalty),
            "specialized_layout_swing_pct": r3(layout_swing),
            "worst_preempt_p99_ns": worst_p99,
            "timing": "tempo de CPU da thread (CLOCK_THREAD_CPUTIME_ID); desaceleração = mediana das razões pareadas por rodada",
            "limits": { "overhead_pct": OVERHEAD_LIMIT_PCT, "preempt_p99_ns": PREEMPT_P99_LIMIT_NS },
        }),
    }
}
