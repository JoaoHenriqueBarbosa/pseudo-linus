//! Medições no kernel do host (H13).
//!
//! Todas as threads do cenário ficam fixadas numa mesma CPU (`sched_setaffinity`) e recebem o nice com
//! `setpriority(PRIO_PROCESS, tid)`, que no Linux vale só pra thread. A thread que coordena fica em
//! outra CPU. O tempo de CPU de cada thread vem do primeiro campo de `/proc/self/task/<tid>/schedstat`
//! (o `sum_exec_runtime` do escalonador, em ns).
//!
//! - **Divisão de CPU**: threads em laço de CPU com nices diferentes; depois de um aquecimento, a
//!   divisão é a fração do tempo de CPU de cada uma numa janela.
//! - **Latência de wakeup**: uma thread dorme 1 ms (`nanosleep`) em laço, competindo com laços de CPU
//!   na mesma CPU. A latência de cada volta é `tempo dormido - 1 ms`, e inclui a folga do timer
//!   (`timer_slack_ns`, 50 µs por padrão), a interrupção, o wakeup e a espera pela CPU.

use std::hint::black_box;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use rustix::process::{Pid, setpriority_process};
use rustix::thread::{current_timer_slack, gettid, set_current_timer_slack};
use serde::Serialize;

use crate::cpusel::pin_current_thread;

/// `sum_exec_runtime` de uma thread do processo.
pub fn schedstat_runtime(tid: i32) -> Result<u64> {
    let path = format!("/proc/self/task/{tid}/schedstat");
    let text = std::fs::read_to_string(&path).with_context(|| format!("ler {path}"))?;
    let first = text.split_whitespace().next().ok_or_else(|| anyhow!("{path} vazio"))?;
    first.parse().with_context(|| format!("campo de {path}"))
}

/// Fixa a thread que chama e põe o nice dela. Devolve o tid.
fn setup_thread(cpu: usize, nice: i32) -> Result<i32> {
    pin_current_thread(cpu)?;
    let tid: Pid = gettid();
    setpriority_process(Some(tid), nice).with_context(|| format!("setpriority({nice}) no tid {}", tid.as_raw_pid()))?;
    Ok(tid.as_raw_pid())
}

/// Portão de largada: as threads se configuram, esperam `go` e param com `stop`.
#[derive(Debug, Default)]
struct Gate {
    go: AtomicBool,
    stop: AtomicBool,
}

impl Gate {
    /// Espera a largada; devolve `false` se mandaram parar antes.
    fn wait(&self) -> bool {
        while !self.go.load(Ordering::Acquire) {
            if self.stop.load(Ordering::Relaxed) {
                return false;
            }
            std::thread::sleep(Duration::from_micros(50));
        }
        !self.stop.load(Ordering::Relaxed)
    }
}

/// Laço de CPU até `stop`.
fn hog_loop(gate: &Gate) {
    let mut x = 0x1234_5678u64;
    while !gate.stop.load(Ordering::Relaxed) {
        for _ in 0..4096 {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        }
        black_box(x);
    }
}

struct Hogs {
    gate: Arc<Gate>,
    handles: Vec<JoinHandle<()>>,
    tids: Vec<i32>,
}

impl Hogs {
    /// Cria as threads de laço e espera todas se configurarem. Elas só começam a gastar CPU quando o
    /// portão abre.
    fn spawn(cpu: usize, nices: &[i32], gate: &Arc<Gate>) -> Result<Hogs> {
        let (tx, rx) = mpsc::channel::<(usize, Result<i32>)>();
        let mut hogs = Hogs { gate: Arc::clone(gate), handles: Vec::new(), tids: vec![0; nices.len()] };
        for (i, &nice) in nices.iter().enumerate() {
            let tx = tx.clone();
            let gate = Arc::clone(gate);
            let spawned = std::thread::Builder::new().name(format!("hog{i}")).spawn(move || {
                let setup = setup_thread(cpu, nice);
                let ok = setup.is_ok();
                let _ = tx.send((i, setup));
                if ok && gate.wait() {
                    hog_loop(&gate);
                }
            });
            match spawned {
                Ok(h) => hogs.handles.push(h),
                Err(e) => {
                    hogs.finish();
                    return Err(anyhow!(e).context("criar thread de laço"));
                }
            }
        }
        drop(tx);
        let mut error = None;
        for (i, r) in rx.iter().take(nices.len()) {
            match r {
                Ok(t) => hogs.tids[i] = t,
                Err(e) => error = Some(e),
            }
        }
        if let Some(e) = error {
            hogs.finish();
            return Err(e.context("configurar thread de laço"));
        }
        Ok(hogs)
    }

    fn finish(self) {
        self.gate.stop.store(true, Ordering::Relaxed);
        for h in self.handles {
            let _ = h.join();
        }
    }
}

/// Uma medição de divisão de CPU.
#[derive(Clone, Debug, Serialize)]
pub struct ShareRun {
    pub nices: Vec<i32>,
    pub cpu_ns: Vec<u64>,
    pub shares: Vec<f64>,
    pub window_ns: u64,
    /// Fração da janela em que a CPU rodou as nossas threads (1,0 = ninguém mais usou a CPU).
    pub ours_fraction: f64,
}

/// Mede a divisão de CPU entre laços com os nices dados, todos na CPU `cpu`.
pub fn measure_shares(cpu: usize, nices: &[i32], warmup: Duration, window: Duration) -> Result<ShareRun> {
    let gate = Arc::new(Gate::default());
    let hogs = Hogs::spawn(cpu, nices, &gate)?;
    gate.go.store(true, Ordering::Release);
    std::thread::sleep(warmup);
    let measured = (|| -> Result<(Vec<u64>, Vec<u64>, u64)> {
        let before: Vec<u64> = hogs.tids.iter().map(|&t| schedstat_runtime(t)).collect::<Result<_>>()?;
        let t0 = Instant::now();
        std::thread::sleep(window);
        let after: Vec<u64> = hogs.tids.iter().map(|&t| schedstat_runtime(t)).collect::<Result<_>>()?;
        Ok((before, after, t0.elapsed().as_nanos() as u64))
    })();
    hogs.finish();
    let (before, after, wall) = measured?;
    let cpu_ns: Vec<u64> = after.iter().zip(&before).map(|(a, b)| a - b).collect();
    let total: u64 = cpu_ns.iter().sum();
    let shares = cpu_ns.iter().map(|&c| c as f64 / total.max(1) as f64).collect();
    Ok(ShareRun { nices: nices.to_vec(), cpu_ns, shares, window_ns: wall, ours_fraction: total as f64 / wall as f64 })
}

/// Uma medição de latência de wakeup.
#[derive(Clone, Debug, Serialize)]
pub struct LatencyRun {
    pub hogs: usize,
    /// Folga do timer em vigor na thread que dorme.
    pub timer_slack_ns: u64,
    #[serde(skip)]
    pub samples_ns: Vec<u64>,
    /// CPU gasta pela thread que dorme por volta (entre acordar e dormir de novo).
    pub sleeper_cpu_per_iter_ns: f64,
    pub window_ns: u64,
    pub ours_fraction: f64,
}

/// Mede a latência de wakeup de uma thread que dorme 1 ms em laço, com `hogs` laços de CPU (nice 0)
/// na mesma CPU. `slack` troca a folga do timer da thread (`None` deixa a herdada, 50 µs).
pub fn measure_latency(cpu: usize, hogs: usize, slack: Option<u64>, warmup_iters: usize, iters: usize) -> Result<LatencyRun> {
    let gate = Arc::new(Gate::default());
    let hog_set = Hogs::spawn(cpu, &vec![0; hogs], &gate)?;
    let hog_tids = hog_set.tids.clone();
    let sleeper_gate = Arc::clone(&gate);
    let spawned = std::thread::Builder::new().name("sleeper".to_string()).spawn(move || -> Result<LatencyRun> {
        let tid = setup_thread(cpu, 0)?;
        if let Some(s) = slack {
            set_current_timer_slack(NonZeroU64::new(s)).context("PR_SET_TIMERSLACK")?;
        }
        let eff_slack = current_timer_slack().context("PR_GET_TIMERSLACK")?;
        if !sleeper_gate.wait() {
            return Err(anyhow!("cancelado antes da largada"));
        }
        let one_ms = Duration::from_millis(1);
        for _ in 0..warmup_iters {
            std::thread::sleep(one_ms);
        }
        let read_all = || -> Result<(u64, u64)> {
            let own = schedstat_runtime(tid)?;
            let mut hogs_total = 0;
            for &t in &hog_tids {
                hogs_total += schedstat_runtime(t)?;
            }
            Ok((own, hogs_total))
        };
        let (own0, hogs0) = read_all()?;
        let w0 = Instant::now();
        let mut samples = Vec::with_capacity(iters);
        for _ in 0..iters {
            let t0 = Instant::now();
            std::thread::sleep(one_ms);
            let slept = t0.elapsed().as_nanos() as u64;
            samples.push(slept.saturating_sub(1_000_000));
        }
        let wall = w0.elapsed().as_nanos() as u64;
        let (own1, hogs1) = read_all()?;
        let ours = (own1 - own0) + (hogs1 - hogs0);
        Ok(LatencyRun {
            hogs: hog_tids.len(),
            timer_slack_ns: eff_slack,
            samples_ns: samples,
            sleeper_cpu_per_iter_ns: (own1 - own0) as f64 / iters as f64,
            window_ns: wall,
            ours_fraction: ours as f64 / wall as f64,
        })
    });
    let sleeper = match spawned {
        Ok(h) => h,
        Err(e) => {
            hog_set.finish();
            return Err(anyhow!(e).context("criar thread que dorme"));
        }
    };
    gate.go.store(true, Ordering::Release);
    let run = sleeper.join().map_err(|_| anyhow!("thread que dorme entrou em pânico"));
    hog_set.finish();
    run?
}
