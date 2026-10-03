//! Escala da trava da runqueue (H11).
//!
//! Cada worker é uma CPU virtual com a sua [`RunQueue`] de 8 tarefas CPU-bound. Uma operação de
//! escalonamento é o que acontece num tick que preempta: o relógio da runqueue anda 3 ms (mais que a
//! fatia de 2,8 ms), `tick` cobra o corrente e pede reescalonamento, e `schedule` faz o put do anterior
//! (volta pra árvore) e o pick do próximo (`pick_eevdf` + `set_next_entity`).
//!
//! Duas organizações:
//!
//! - **trava global**: todas as runqueues atrás de um único `Mutex` (o desenho inicial do v2);
//! - **trava por runqueue**: cada runqueue com o seu `Mutex`, que só o dono usa.
//!
//! Entre operações, cada worker gasta um tempo de "trabalho" fora da trava (0, 1, 10 e 100 µs),
//! que modela a tarefa rodando entre um evento de escalonamento e outro. A vazão ideal de W workers é
//! a da trava por runqueue com os mesmos W (mesmo hardware, mesma contenção de SMT e cache, sem
//! contenção de trava); a eficiência da trava global é a razão entre as duas.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};

use sched::{Features, ManualClock, RunQueue, Tunables};
use serde::Serialize;

type Rq = RunQueue<ManualClock>;

fn new_rq() -> Rq {
    let clock = ManualClock::new(0);
    let mut rq = RunQueue::new(clock, Tunables::linux_6_12_101(16, 250), Features::default());
    for i in 0..8 {
        let t = rq.create_task([0, 0, 0, 0, 1, 2, -1, 3][i]);
        rq.wake_up_new_task(t);
    }
    rq.schedule(false);
    rq
}

/// Uma operação de escalonamento (tick que preempta: put + pick).
#[inline]
pub fn sched_op(rq: &mut Rq) {
    rq.clock().advance(3_000_000);
    rq.tick();
    if rq.need_resched() {
        rq.schedule(false);
    }
}

fn spin(ns: u64) {
    if ns == 0 {
        return;
    }
    let t0 = Instant::now();
    while (t0.elapsed().as_nanos() as u64) < ns {
        std::hint::spin_loop();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    Global,
    PerWorker,
}

fn run_once(layout: Layout, workers: usize, think_ns: u64, duration: Duration) -> f64 {
    let stop = Arc::new(AtomicBool::new(false));
    let total = Arc::new(AtomicU64::new(0));
    let barrier = Arc::new(Barrier::new(workers + 1));
    let global: Arc<Mutex<Vec<Rq>>> = Arc::new(Mutex::new((0..workers).map(|_| new_rq()).collect()));
    let per: Arc<Vec<Mutex<Rq>>> = Arc::new((0..workers).map(|_| Mutex::new(new_rq())).collect());
    let mut handles = Vec::new();
    for w in 0..workers {
        let stop = Arc::clone(&stop);
        let total = Arc::clone(&total);
        let barrier = Arc::clone(&barrier);
        let global = Arc::clone(&global);
        let per = Arc::clone(&per);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            let mut ops = 0u64;
            while !stop.load(Ordering::Relaxed) {
                match layout {
                    Layout::Global => {
                        let mut g = global.lock().expect("trava global envenenada");
                        sched_op(&mut g[w]);
                    }
                    Layout::PerWorker => {
                        let mut r = per[w].lock().expect("trava da runqueue envenenada");
                        sched_op(&mut r);
                    }
                }
                ops += 1;
                spin(think_ns);
            }
            total.fetch_add(ops, Ordering::Relaxed);
        }));
    }
    barrier.wait();
    let t0 = Instant::now();
    std::thread::sleep(duration);
    stop.store(true, Ordering::Relaxed);
    for h in handles {
        let _ = h.join();
    }
    let elapsed = t0.elapsed().as_secs_f64();
    total.load(Ordering::Relaxed) as f64 / elapsed
}

/// Um ponto da curva.
#[derive(Clone, Debug, Serialize)]
pub struct LockPoint {
    pub workers: usize,
    pub think_ns: u64,
    pub global_ops_per_s: f64,
    pub per_worker_ops_per_s: f64,
    /// Vazão da trava global sobre a da trava por runqueue com os mesmos workers.
    pub global_efficiency: f64,
    /// Vazão da trava por runqueue sobre W vezes a de um worker (efeito do hardware).
    pub per_worker_scaling: f64,
    /// Vazão da trava global sobre W vezes a de um worker.
    pub global_scaling: f64,
}

/// Roda a matriz workers x tempo de trabalho. Cada ponto é a maior vazão de `reps` repetições, com as
/// duas organizações intercaladas (a repetição menos perturbada por outros processos, medida nas
/// mesmas condições pras duas).
pub fn run(workers_list: &[usize], think_list: &[u64], duration: Duration, reps: usize) -> Vec<LockPoint> {
    let mut out = Vec::new();
    for &think in think_list {
        let mut single = None;
        for &w in workers_list {
            let (mut g, mut p) = (0.0f64, 0.0f64);
            for _ in 0..reps {
                g = g.max(run_once(Layout::Global, w, think, duration));
                p = p.max(run_once(Layout::PerWorker, w, think, duration));
            }
            let base = *single.get_or_insert(if w == 1 { p } else { run_once(Layout::PerWorker, 1, think, duration) });
            out.push(LockPoint {
                workers: w,
                think_ns: think,
                global_ops_per_s: g,
                per_worker_ops_per_s: p,
                global_efficiency: g / p,
                per_worker_scaling: p / (w as f64 * base),
                global_scaling: g / (w as f64 * base),
            });
        }
    }
    out
}
