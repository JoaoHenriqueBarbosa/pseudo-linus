//! H07: laço de CPU sem checkpoint não cede nem morre; no modelo A, rebaixar a thread host mitiga.
//!
//! Demonstração (cada modelo, 1 CPU virtual, timer ligado): o processo L gira 300 ms sem checkpoint
//! (relógio do host, sem passar pelo kernel) e só no fim chama `checkpoint`. O vizinho N, pronto na mesma
//! CPU virtual, conta iterações com checkpoint. Aos 50 ms o host manda SIGKILL pra L. Mede: quanto N
//! progrediu enquanto L girava (L lê o contador de N no fim do giro) e quanto L demorou pra morrer depois
//! do kill.
//!
//! Mitigação (só A, 2 CPUs virtuais): L e N rodam ao mesmo tempo, com as duas threads do host fixadas no
//! mesmo núcleo físico. Com L em nice 0 e com o watchdog ligado (que rebaixa L pra nice 19 quando percebe
//! que ele ignora `attention` e está consumindo CPU), mede a fração da CPU que N recebe do total que N e L
//! dividem (schedstat das duas threads), e a taxa de iterações de N contra N sozinho.

use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use harness::Verdict;
use serde::Serialize;
use serde_json::json;

use super::{HypOut, is_sync, progress, status_str};
use crate::kernel::{SIGKILL, SIGTERM};
use crate::model_a::{ConfigA, KernelA, WatchdogStats, thread_cpu_ns};
use crate::stats::r3;
use crate::sys::Sys;
use crate::{KIB, ModelSpec, SyncKernel};

const SPIN: Duration = Duration::from_millis(300);
const KILL_AT: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, Serialize)]
pub struct DemoRow {
    pub model: String,
    pub neighbor_progress_during_spin: u64,
    pub neighbor_progress_after: u64,
    pub killed_status: String,
    pub kill_to_death_ms: f64,
    pub spin_ms: f64,
}

fn spin_no_checkpoint(d: Duration) {
    let t = Instant::now();
    let mut x = 1u64;
    while t.elapsed() < d {
        for _ in 0..1000 {
            x = black_box(x.wrapping_mul(2862933555777941757).wrapping_add(3037000493));
        }
    }
    black_box(x);
}

fn demo(spec: ModelSpec) -> DemoRow {
    let counter = Arc::new(AtomicU64::new(0));
    let seen_during = Arc::new(AtomicU64::new(u64::MAX));
    let started = Arc::new(AtomicBool::new(false));
    let (c_l, seen_l, st_l) = (counter.clone(), seen_during.clone(), started.clone());
    let c_n = counter.clone();
    let (killed_status, kill_to_death, n_after) = if is_sync(spec) {
        let k = spec.sync_kernel(1, true).expect("sync");
        let l = k.spawn(
            Vec::new(),
            Box::new(move |sys: &dyn Sys| {
                st_l.store(true, Ordering::Release);
                spin_no_checkpoint(SPIN);
                seen_l.store(c_l.load(Ordering::Relaxed), Ordering::Release);
                sys.checkpoint();
                0
            }),
        );
        while !started.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        let n = k.spawn(
            Vec::new(),
            Box::new(move |sys: &dyn Sys| {
                loop {
                    c_n.fetch_add(1, Ordering::Relaxed);
                    sys.checkpoint();
                }
            }),
        );
        std::thread::sleep(KILL_AT);
        let t = Instant::now();
        k.kill(l, SIGKILL);
        let st = k.wait(l);
        let el = t.elapsed();
        std::thread::sleep(Duration::from_millis(20));
        let after = counter.load(Ordering::Relaxed);
        k.kill(n, SIGTERM);
        k.wait(n);
        (status_str(&st), el, after)
    } else {
        let k = spec.c_kernel(1, true);
        let l = k.spawn(Vec::new(), move |ctx| async move {
            st_l.store(true, Ordering::Release);
            spin_no_checkpoint(SPIN);
            seen_l.store(c_l.load(Ordering::Relaxed), Ordering::Release);
            ctx.checkpoint().await;
            0
        });
        while !started.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        let n = k.spawn(Vec::new(), move |ctx| async move {
            loop {
                if c_n.fetch_add(1, Ordering::Relaxed) == u64::MAX {
                    break 0;
                }
                ctx.checkpoint().await;
            }
        });
        std::thread::sleep(KILL_AT);
        let t = Instant::now();
        let _ = k.core().kill(l, SIGKILL);
        let st = k.wait(l);
        let el = t.elapsed();
        std::thread::sleep(Duration::from_millis(20));
        let after = counter.load(Ordering::Relaxed);
        let _ = k.core().kill(n, SIGTERM);
        k.wait(n);
        (status_str(&st), el, after)
    };
    DemoRow {
        model: spec.label(),
        neighbor_progress_during_spin: seen_during.load(Ordering::Acquire),
        neighbor_progress_after: n_after,
        killed_status,
        kill_to_death_ms: r3(kill_to_death.as_secs_f64() * 1e3),
        spin_ms: SPIN.as_secs_f64() * 1e3,
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MitigationRow {
    pub config: String,
    pub neighbor_iters_per_s: f64,
    /// Taxa do vizinho dividida pela taxa dele sozinho (sensível a outros processos no mesmo núcleo).
    pub share_vs_alone: f64,
    /// CPU que o vizinho recebeu dividida pela soma da CPU do vizinho e do laço na janela (lida de
    /// `/proc/self/task/<tid>/schedstat`). Não depende de quanto outros processos do host usaram o
    /// núcleo, porque o escalonador do host divide entre os dois pelo peso de cada um.
    pub neighbor_cpu_share_vs_spinner: Option<f64>,
    /// Fração da janela em que o vizinho esteve na CPU (mostra a disputa com o resto do host).
    pub neighbor_cpu_utilization: f64,
    pub spinner_nice_during_window: Option<i32>,
    pub watchdog: WatchdogStats,
}

/// Mede a taxa do vizinho N com as duas threads fixadas no mesmo núcleo. `with_spinner`: L presente;
/// `watchdog`: rebaixamento ligado.
fn mitigation_run(with_spinner: bool, watchdog: bool, window: Duration, host_cpu: usize) -> MitigationRow {
    let k = KernelA::new(ConfigA { ncpus: 2, stack_size: 256 * KIB, timer: true, spin: Duration::ZERO });
    k.set_watchdog(watchdog);
    let counter = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let spin_total = window + Duration::from_millis(250);
    let l = with_spinner.then(|| {
        k.spawn(
            Vec::new(),
            Box::new(move |sys: &dyn Sys| {
                spin_no_checkpoint(spin_total);
                sys.checkpoint();
                0
            }),
        )
    });
    let (c2, s2) = (counter.clone(), stop.clone());
    let n = k.spawn(
        Vec::new(),
        Box::new(move |sys: &dyn Sys| {
            let mut x = 1u64;
            while !s2.load(Ordering::Relaxed) {
                for _ in 0..200 {
                    x = black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
                }
                c2.fetch_add(1, Ordering::Relaxed);
                sys.checkpoint();
            }
            0
        }),
    );
    let mut set = rustix::thread::CpuSet::new();
    set.set(host_cpu);
    let pids: Vec<_> = l.iter().copied().chain(std::iter::once(n)).collect();
    for &p in &pids {
        while k.host_tid(p) == 0 {
            std::thread::yield_now();
        }
        let tid = rustix::process::Pid::from_raw(k.host_tid(p));
        let _ = rustix::thread::sched_setaffinity(tid, &set);
    }
    std::thread::sleep(Duration::from_millis(100));
    let nice = l.and_then(|p| rustix::process::getpriority_process(rustix::process::Pid::from_raw(k.host_tid(p))).ok());
    let tid_n = k.host_tid(n);
    let tid_l = l.map(|p| k.host_tid(p));
    let cpu = |tid: i32| thread_cpu_ns(tid).unwrap_or(0);
    let (n0, l0) = (cpu(tid_n), tid_l.map(cpu));
    let c0 = counter.load(Ordering::Relaxed);
    let t0 = Instant::now();
    std::thread::sleep(window);
    let c1 = counter.load(Ordering::Relaxed);
    let (n1, l1) = (cpu(tid_n), tid_l.map(cpu));
    let el = t0.elapsed();
    let rate = (c1 - c0) as f64 / el.as_secs_f64();
    let dn = n1.saturating_sub(n0) as f64;
    let share = match (l0, l1) {
        (Some(a), Some(b)) => {
            let dl = b.saturating_sub(a) as f64;
            (dn + dl > 0.0).then(|| r3(dn / (dn + dl)))
        }
        _ => None,
    };
    let utilization = r3(dn / el.as_nanos() as f64);
    stop.store(true, Ordering::Relaxed);
    k.wait(n);
    if let Some(p) = l {
        k.wait(p);
    }
    let wd = k.shared().watchdog_stats.lock().clone();
    MitigationRow {
        config: match (with_spinner, watchdog) {
            (false, _) => "neighbor_alone",
            (true, false) => "spinner_nice0",
            (true, true) => "spinner_watchdog_nice19",
        }
        .to_string(),
        neighbor_iters_per_s: r3(rate),
        share_vs_alone: 0.0,
        neighbor_cpu_share_vs_spinner: share,
        neighbor_cpu_utilization: utilization,
        spinner_nice_during_window: nice,
        watchdog: wd,
    }
}

pub struct H07Data {
    pub demos: Vec<DemoRow>,
    pub mitigation: Vec<MitigationRow>,
    pub host_cpu: usize,
}

pub fn measure(window: Duration) -> H07Data {
    let mut demos = Vec::new();
    for spec in ModelSpec::MAIN {
        progress(format!("H07: laço sem checkpoint em {}", spec.label()));
        demos.push(demo(spec));
    }
    progress("H07: mitigação por nice 19 no modelo A");
    let host_cpu = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) - 1;
    let mut mitigation = vec![
        mitigation_run(false, false, window, host_cpu),
        mitigation_run(true, false, window, host_cpu),
        mitigation_run(true, true, window, host_cpu),
    ];
    let alone = mitigation[0].neighbor_iters_per_s.max(1.0);
    for m in &mut mitigation {
        m.share_vs_alone = r3(m.neighbor_iters_per_s / alone);
    }
    H07Data { demos, mitigation, host_cpu }
}

pub fn verdict(d: &H07Data) -> HypOut {
    let no_yield = d.demos.iter().all(|r| r.neighbor_progress_during_spin == 0);
    let late_death = d.demos.iter().all(|r| r.kill_to_death_ms >= (SPIN - KILL_AT).as_secs_f64() * 1e3 * 0.8);
    // Decide pela divisão de CPU entre vizinho e laço (robusta a outros processos no mesmo núcleo); a taxa
    // contra o vizinho sozinho vai junto.
    let plain = d.mitigation.iter().find(|m| m.config == "spinner_nice0");
    let mitig = d.mitigation.iter().find(|m| m.config == "spinner_watchdog_nice19");
    let shared = plain.and_then(|m| m.neighbor_cpu_share_vs_spinner).unwrap_or(0.0);
    let mitigated = mitig.and_then(|m| m.neighbor_cpu_share_vs_spinner).unwrap_or(0.0);
    let shared_rate = plain.map(|m| m.share_vs_alone).unwrap_or(0.0);
    let mitigated_rate = mitig.map(|m| m.share_vs_alone).unwrap_or(0.0);
    let reniced = mitig.map(|m| m.watchdog.reniced).unwrap_or(0);
    let detect_ms = mitig.and_then(|m| m.watchdog.detect_ns.first().copied()).map(|n| n as f64 / 1e6);
    let restore_err = mitig.and_then(|m| m.watchdog.restore_errors.first().cloned());
    let helps = reniced > 0 && mitigated > shared + 0.2 && mitigated > 0.9;
    let verdict = if no_yield && late_death && helps {
        Verdict::Confirmed
    } else if no_yield && late_death {
        Verdict::Partial
    } else {
        Verdict::Refuted
    };
    let deaths: Vec<String> = d.demos.iter().map(|r| format!("{} {:.0} ms", r.model, r.kill_to_death_ms)).collect();
    HypOut {
        id: "H07",
        verdict,
        summary: format!(
            "Nos 4 modelos o vizinho não andou nada ({}) durante os {} ms de laço sem checkpoint, e o SIGKILL só matou \
             no fim do laço ({}). No A, com as threads do vizinho e do laço fixadas no mesmo núcleo, o vizinho fica \
             com {:.0}% da CPU que os dois dividem; com o watchdog rebaixando o laço pra nice 19 (detectado em {} ms) \
             fica com {:.0}% (taxa de iterações: {:.0}% e {:.0}% da taxa dele sozinho). Voltar pra nice 0 falha sem \
             privilégio ({}), então o rebaixamento é de mão única.",
            if no_yield { "0 iterações" } else { "houve progresso" },
            SPIN.as_millis(),
            deaths.join(", "),
            shared * 100.0,
            detect_ms.map(|x| format!("{x:.1}")).unwrap_or("?".into()),
            mitigated * 100.0,
            shared_rate * 100.0,
            mitigated_rate * 100.0,
            restore_err.unwrap_or_else(|| "sem tentativa registrada".into()),
        ),
        evidence: json!({
            "demos": d.demos,
            "mitigation_model_a": d.mitigation,
            "pinned_host_cpu": d.host_cpu,
            "spin_ms": SPIN.as_millis() as u64,
            "kill_at_ms": KILL_AT.as_millis() as u64,
        }),
    }
}
