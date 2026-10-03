//! H03 (memória por processo ocioso) e H04 (custo de criação e quantos cabem).
//!
//! Escala: um subprocesso por (modelo, quantidade), dentro de
//! `systemd-run --user --scope -p TasksMax=40000 -p MemoryMax=8G`, cria N processos que bloqueiam todos
//! lendo o mesmo pipe vazio. Com todos bloqueados, mede VmRSS do host e a memória do cgroup do scope
//! (que inclui pilha de kernel por thread e tabelas de página, que o RSS não mostra). Depois fecha o
//! pipe (EOF pra todos) e espera todos saírem.
//!
//! Latência: um processo "shell" cria um filho trivial e espera por ele, repetidas vezes; cada iteração
//! é cronometrada dentro do processo.

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use harness::Verdict;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{HypOut, is_sync, progress};
use crate::kernel::{ExitStatus, File, new_pipe};
use crate::stats::{CgroupMem, Dist, cgroup_mem, r3, rss_kib};
use crate::sys::Sys;
use crate::{KIB, ModelKind, ModelSpec};

pub const COUNTS: [usize; 3] = [1_000, 10_000, 30_000];
pub const SCALE_MODELS: [ModelSpec; 5] = [ModelSpec::A, ModelSpec::A64, ModelSpec::B64, ModelSpec::B256, ModelSpec::C];
const SCALE_CPUS: usize = 4;
const H03_LIMIT_BYTES: f64 = 64.0 * 1024.0;
const H04_LIMIT_US: f64 = 50.0;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ScaleReport {
    pub model: String,
    pub count: usize,
    pub ok: bool,
    pub blocked: usize,
    pub failures: u64,
    pub spawn_total_ms: f64,
    pub spawn_us_per_proc: f64,
    pub rss_before_kib: u64,
    pub rss_after_kib: u64,
    pub rss_per_proc_bytes: f64,
    pub cgroup_before: Option<CgroupMem>,
    pub cgroup_after: Option<CgroupMem>,
    pub cgroup_per_proc_bytes: Option<f64>,
    pub kernel_stack_per_proc_bytes: Option<f64>,
    pub pagetables_per_proc_bytes: Option<f64>,
    pub teardown_ms: f64,
    pub exited_ok: usize,
    pub error: Option<String>,
}

/// Subcomando `scale <modelo> <n>`: roda dentro do scope e devolve o relatório.
pub fn scale_child(spec: ModelSpec, count: usize) -> ScaleReport {
    let mut rep = ScaleReport { model: spec.label(), count, ..Default::default() };
    let ready = Arc::new(AtomicUsize::new(0));
    let (r, w) = new_pipe();
    let pipe = match &r {
        File::Read(pr) => pr.pipe().clone(),
        _ => unreachable!("ponta de leitura"),
    };
    // O kernel é criado antes da linha de base de memória.
    let sync_k = spec.sync_kernel(SCALE_CPUS, false);
    let c_k = (spec.kind == ModelKind::C).then(|| spec.c_kernel(SCALE_CPUS, false));
    std::thread::sleep(Duration::from_millis(50));
    rep.rss_before_kib = rss_kib();
    rep.cgroup_before = cgroup_mem();

    let t0 = Instant::now();
    let mut pids = Vec::with_capacity(count);
    for _ in 0..count {
        let ready = ready.clone();
        let files = vec![(0, r.clone())];
        let pid = match (&sync_k, &c_k) {
            (Some(k), _) => k.spawn(
                files,
                Box::new(move |sys: &dyn Sys| {
                    ready.fetch_add(1, Ordering::Relaxed);
                    let mut b = [0u8; 1];
                    let _ = sys.read(0, &mut b);
                    0
                }),
            ),
            (None, Some(k)) => k.spawn(files, move |ctx| async move {
                ready.fetch_add(1, Ordering::Relaxed);
                let mut b = [0u8; 1];
                let _ = ctx.read(0, &mut b).await;
                0
            }),
            _ => unreachable!(),
        };
        pids.push(pid);
    }
    drop(r);
    let spawn_el = t0.elapsed();
    rep.spawn_total_ms = r3(spawn_el.as_secs_f64() * 1e3);
    rep.spawn_us_per_proc = r3(spawn_el.as_secs_f64() * 1e6 / count as f64);

    // Falhas de criação (thread ou pilha) aparecem como processos já terminados.
    let failures = || -> u64 {
        match (&sync_k, &c_k) {
            (Some(k), _) => {
                let core = k.core();
                pids.iter()
                    .filter(|&&p| core.lookup(p).and_then(|x| x.exit_status()).is_some_and(|s| s.is_abnormal()))
                    .count() as u64
            }
            _ => 0,
        }
    };
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let blocked = pipe.read_waiters();
        let f = failures();
        if blocked as u64 + f >= count as u64 {
            rep.blocked = blocked;
            rep.failures = f;
            break;
        }
        if Instant::now() > deadline {
            rep.blocked = blocked;
            rep.failures = f;
            rep.error = Some(format!("prazo: só {blocked} bloqueados de {count}"));
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(200));
    rep.rss_after_kib = rss_kib();
    rep.cgroup_after = cgroup_mem();
    let alive = rep.blocked.max(1) as f64;
    rep.rss_per_proc_bytes = r3((rep.rss_after_kib.saturating_sub(rep.rss_before_kib) * 1024) as f64 / alive);
    if let (Some(a), Some(b)) = (&rep.cgroup_before, &rep.cgroup_after) {
        rep.cgroup_per_proc_bytes = Some(r3(b.current.saturating_sub(a.current) as f64 / alive));
        rep.kernel_stack_per_proc_bytes = Some(r3(b.kernel_stack.saturating_sub(a.kernel_stack) as f64 / alive));
        rep.pagetables_per_proc_bytes = Some(r3(b.pagetables.saturating_sub(a.pagetables) as f64 / alive));
    }

    let t1 = Instant::now();
    drop(w);
    let mut ok = 0;
    for pid in pids {
        let st = match (&sync_k, &c_k) {
            (Some(k), _) => k.core().wait_host_timeout(pid, Duration::from_secs(120)),
            (None, Some(k)) => k.core().wait_host_timeout(pid, Duration::from_secs(120)),
            _ => unreachable!(),
        };
        if matches!(st, Ok(Some(ExitStatus::Exited(0)))) {
            ok += 1;
        }
    }
    rep.teardown_ms = r3(t1.elapsed().as_secs_f64() * 1e3);
    rep.exited_ok = ok;
    rep.ok = rep.error.is_none() && rep.failures == 0 && rep.blocked == count && ok == count;
    rep
}

/// Roda `scale` num subprocesso dentro de um scope do systemd com TasksMax e MemoryMax.
pub fn run_scale_in_scope(spec: ModelSpec, count: usize) -> ScaleReport {
    let fail = |e: String| ScaleReport { model: spec.label(), count, error: Some(e), ..Default::default() };
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => return fail(e.to_string()),
    };
    let out = Command::new("systemd-run")
        .args(["--user", "--scope", "--quiet", "-p", "TasksMax=40000", "-p", "MemoryMax=8G", "--"])
        .arg(exe)
        .args(["scale", &spec.label(), &count.to_string()])
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) => return fail(format!("systemd-run indisponível: {e}")),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    match text.lines().last().map(serde_json::from_str::<ScaleReport>) {
        Some(Ok(r)) => r,
        _ => fail(format!(
            "subprocesso terminou com {:?} sem relatório; stderr: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).chars().take(400).collect::<String>()
        )),
    }
}

/// Latência de spawn+exit+wait, cronometrada dentro de um processo "shell".
pub fn spawn_latency(spec: ModelSpec, iters: usize) -> Vec<u64> {
    let samples = Arc::new(Mutex::new(Vec::with_capacity(iters)));
    if is_sync(spec) {
        let k = spec.sync_kernel(1, true).expect("sync");
        let s2 = samples.clone();
        let shell = k.spawn(
            Vec::new(),
            Box::new(move |sys: &dyn Sys| {
                let mut local = Vec::with_capacity(iters);
                for _ in 0..iters {
                    let t = Instant::now();
                    let pid = sys.spawn(&[], Box::new(|_s: &dyn Sys| 0)).expect("spawn");
                    let st = sys.wait(pid).expect("wait");
                    local.push(t.elapsed().as_nanos() as u64);
                    assert_eq!(st, ExitStatus::Exited(0));
                }
                *s2.lock() = local;
                0
            }),
        );
        k.wait(shell);
    } else {
        let k = spec.c_kernel(1, true);
        let s2 = samples.clone();
        let shell = k.spawn(Vec::new(), move |ctx| async move {
            let mut local = Vec::with_capacity(iters);
            for _ in 0..iters {
                let t = Instant::now();
                let pid = ctx.spawn(&[], |_c| async { 0 }).expect("spawn");
                let st = ctx.wait(pid).await.expect("wait");
                local.push(t.elapsed().as_nanos() as u64);
                assert_eq!(st, ExitStatus::Exited(0));
            }
            *s2.lock() = local;
            0
        });
        k.wait(shell);
    }
    std::mem::take(&mut *samples.lock())
}

pub struct H03H04Data {
    pub scale: Vec<ScaleReport>,
    pub latency: Vec<(String, Dist)>,
}

pub fn measure(counts: &[usize], lat_iters_a: usize, lat_iters_fast: usize) -> H03H04Data {
    let mut scale = Vec::new();
    for &n in counts {
        for spec in SCALE_MODELS {
            progress(format!("H03/H04: escala {} com {n} processos (subprocesso em scope)", spec.label()));
            scale.push(run_scale_in_scope(spec, n));
        }
    }
    let mut latency = Vec::new();
    for spec in ModelSpec::PERF {
        progress(format!("H04: latência de spawn+exit {}", spec.label()));
        let iters = if spec.kind == ModelKind::A { lat_iters_a } else { lat_iters_fast };
        let s = spawn_latency(spec, iters);
        latency.push((spec.label(), Dist::of(&s)));
    }
    H03H04Data { scale, latency }
}

fn at<'a>(d: &'a H03H04Data, model: &str, count: usize) -> Option<&'a ScaleReport> {
    d.scale.iter().find(|r| r.model == model && r.count == count)
}

pub fn verdicts(d: &H03H04Data) -> (HypOut, HypOut) {
    let max_count = d.scale.iter().map(|r| r.count).max().unwrap_or(0);
    let any_error = d.scale.iter().find_map(|r| r.error.clone());

    // H03: RSS por processo ocioso abaixo de 64 KiB em todas as quantidades.
    let mut per_model = Vec::new();
    for spec in SCALE_MODELS {
        let rows: Vec<&ScaleReport> = d.scale.iter().filter(|r| r.model == spec.label()).collect();
        let all_ok = !rows.is_empty() && rows.iter().all(|r| r.ok);
        let max_rss = rows.iter().map(|r| r.rss_per_proc_bytes).fold(0.0, f64::max);
        let at_max = at(d, &spec.label(), max_count);
        per_model.push(json!({
            "model": spec.label(),
            "stack_kib": spec.stack / KIB,
            "all_counts_ok": all_ok,
            "max_rss_per_proc_bytes": max_rss,
            "under_64k": all_ok && max_rss < H03_LIMIT_BYTES,
            "at_max_count": at_max.map(|r| json!({
                "count": r.count,
                "rss_per_proc_bytes": r.rss_per_proc_bytes,
                "cgroup_per_proc_bytes": r.cgroup_per_proc_bytes,
                "kernel_stack_per_proc_bytes": r.kernel_stack_per_proc_bytes,
                "pagetables_per_proc_bytes": r.pagetables_per_proc_bytes,
            })),
        }));
    }
    let under: Vec<bool> = per_model.iter().map(|m| m["under_64k"].as_bool().unwrap_or(false)).collect();
    let fmt_model = |name: &str| -> String {
        at(d, name, max_count)
            .map(|r| {
                format!(
                    "{name} {:.1} KiB de RSS ({} KiB no cgroup, {} KiB de pilha de kernel)",
                    r.rss_per_proc_bytes / 1024.0,
                    r.cgroup_per_proc_bytes.map(|x| format!("{:.1}", x / 1024.0)).unwrap_or("?".into()),
                    r.kernel_stack_per_proc_bytes.map(|x| format!("{:.1}", x / 1024.0)).unwrap_or("?".into()),
                )
            })
            .unwrap_or_else(|| format!("{name} sem dado"))
    };
    let h03_verdict = if d.scale.is_empty() || any_error.is_some() && under.iter().all(|u| !u) {
        Verdict::Inconclusive
    } else if under.iter().all(|&u| u) {
        Verdict::Confirmed
    } else if under.iter().any(|&u| u) {
        Verdict::Partial
    } else {
        Verdict::Refuted
    };
    let h03 = HypOut {
        id: "H03",
        verdict: h03_verdict,
        summary: format!(
            "Com {max_count} processos bloqueados, por processo: {}; {}; {}; {}.",
            fmt_model("A"),
            fmt_model("B64"),
            fmt_model("B256"),
            fmt_model("C")
        ),
        evidence: json!({ "per_model": per_model, "scale_runs": d.scale, "limit_bytes": H03_LIMIT_BYTES }),
    };

    // H04: mediana de spawn+exit < 50 µs e todos os processos simultâneos no maior N.
    let mut rows = Vec::new();
    let mut all = true;
    let mut some = false;
    for (label, dist) in &d.latency {
        let p50_us = dist.p50 as f64 / 1e3;
        let scale_label = if label == "A-spin" { "A".to_string() } else { label.clone() };
        let sustained = at(d, &scale_label, max_count).is_some_and(|r| r.ok);
        let pass = p50_us < H04_LIMIT_US && sustained;
        all &= pass;
        some |= pass;
        rows.push(json!({
            "model": label, "spawn_exit_p50_us": r3(p50_us), "spawn_exit_p99_us": r3(dist.p99 as f64 / 1e3),
            "samples": dist.n, "sustained_max_count": sustained, "max_count": max_count, "pass": pass,
        }));
    }
    let lat = |m: &str| {
        d.latency.iter().find(|(l, _)| l == m).map(|(_, x)| x.p50 as f64 / 1e3).unwrap_or(f64::NAN)
    };
    let h04_verdict = if d.latency.is_empty() {
        Verdict::Inconclusive
    } else if all {
        Verdict::Confirmed
    } else if some {
        Verdict::Partial
    } else {
        Verdict::Refuted
    };
    let sustained_list: Vec<String> = SCALE_MODELS
        .iter()
        .map(|s| {
            let ok = at(d, &s.label(), max_count).is_some_and(|r| r.ok);
            format!("{} {}", s.label(), if ok { "sim" } else { "não" })
        })
        .collect();
    let h04 = HypOut {
        id: "H04",
        verdict: h04_verdict,
        summary: format!(
            "Mediana de spawn+exit+wait: A {:.1} µs, A-spin {:.1} µs, B64 {:.1} µs, B256 {:.1} µs, C {:.2} µs. \
             {max_count} processos simultâneos: {}.",
            lat("A"),
            lat("A-spin"),
            lat("B64"),
            lat("B256"),
            lat("C"),
            sustained_list.join(", ")
        ),
        evidence: json!({ "per_model": rows, "limit_us": H04_LIMIT_US, "scale_cpus": SCALE_CPUS }),
    };
    (h03, h04)
}
