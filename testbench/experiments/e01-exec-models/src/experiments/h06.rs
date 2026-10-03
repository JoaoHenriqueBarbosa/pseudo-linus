//! H06: SIGKILL via unwind libera descritores e roda Drops, com o processo bloqueado em `read` de pipe
//! ou num laço com checkpoint.
//!
//! Cada repetição cria um processo que segura um guarda com `Drop` (contador) e o fd de leitura de um
//! pipe cuja ponta de escrita fica com o host. O host espera o processo bloquear (o pipe passa a ter um
//! leitor esperando) ou entrar no laço, manda SIGKILL e cronometra até o `wait` devolver. Depois confere:
//! status = morto por sinal 9, contador de Drops incrementado, pipe sem leitores e escrita do host dando
//! EPIPE (o fd foi fechado pelo término).

use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::{Poll, Waker};
use std::time::{Duration, Instant};

use harness::Verdict;
use serde::Serialize;
use serde_json::json;

use super::{HypOut, is_sync, progress};
use crate::ModelSpec;
use crate::kernel::{ExitStatus, File, SIGKILL, new_pipe};
use crate::pipe::Pipe;
use crate::stats::{Dist, r3};
use crate::sys::Sys;

struct DropCounter(Arc<AtomicU64>);

impl Drop for DropCounter {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct KillRow {
    pub model: String,
    pub scenario: String,
    pub reps: usize,
    pub signaled_9: usize,
    pub drops_ran: u64,
    pub fds_released: usize,
    pub kill_to_reaped_ns: Dist,
}

fn wait_until(cond: impl Fn() -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while !cond() {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_micros(50));
    }
    true
}

fn fd_released(pipe: &Pipe) -> bool {
    pipe.readers() == 0 && matches!(pipe.poll_write(b"x", Waker::noop()), Poll::Ready(Err(_)))
}

fn split(r: &File) -> Arc<Pipe> {
    match r {
        File::Read(pr) => pr.pipe().clone(),
        _ => unreachable!("ponta de leitura"),
    }
}

fn scenario(spec: ModelSpec, blocked: bool, reps: usize) -> KillRow {
    let drops = Arc::new(AtomicU64::new(0));
    let mut lat = Vec::with_capacity(reps);
    let mut signaled = 0;
    let mut released = 0;
    let sync_k = spec.sync_kernel(1, true);
    let c_k = (!is_sync(spec)).then(|| spec.c_kernel(1, true));
    for _ in 0..reps {
        let (r, w) = new_pipe();
        let pipe = split(&r);
        let started = Arc::new(AtomicBool::new(false));
        let (d2, s2) = (drops.clone(), started.clone());
        let pid = match (&sync_k, &c_k) {
            (Some(k), _) => k.spawn(
                vec![(0, r)],
                Box::new(move |sys: &dyn Sys| {
                    let _guard = DropCounter(d2);
                    s2.store(true, Ordering::Release);
                    if blocked {
                        let mut b = [0u8; 64];
                        let _ = sys.read(0, &mut b);
                    } else {
                        let mut x = 0u64;
                        loop {
                            x = black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
                            sys.checkpoint();
                        }
                    }
                    0
                }),
            ),
            (None, Some(k)) => k.spawn(vec![(0, r)], move |ctx| async move {
                let _guard = DropCounter(d2);
                s2.store(true, Ordering::Release);
                if blocked {
                    let mut b = [0u8; 64];
                    let _ = ctx.read(0, &mut b).await;
                } else {
                    let mut x = 0u64;
                    loop {
                        x = black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
                        ctx.checkpoint().await;
                    }
                }
                0
            }),
            _ => unreachable!(),
        };
        let ready = if blocked {
            wait_until(|| pipe.read_waiters() >= 1, Duration::from_secs(5))
        } else {
            wait_until(|| started.load(Ordering::Acquire), Duration::from_secs(5))
                && {
                    std::thread::sleep(Duration::from_millis(1));
                    true
                }
        };
        assert!(ready, "o processo não chegou ao ponto de bloqueio/laço");
        let t = Instant::now();
        let status = match (&sync_k, &c_k) {
            (Some(k), _) => {
                k.kill(pid, SIGKILL);
                k.wait(pid)
            }
            (None, Some(k)) => {
                let _ = k.core().kill(pid, SIGKILL);
                k.wait(pid)
            }
            _ => unreachable!(),
        };
        lat.push(t.elapsed().as_nanos() as u64);
        if status == ExitStatus::Signaled(SIGKILL) {
            signaled += 1;
        }
        if fd_released(&pipe) {
            released += 1;
        }
        drop(w);
    }
    KillRow {
        model: spec.label(),
        scenario: if blocked { "blocked_in_read" } else { "loop_with_checkpoint" }.to_string(),
        reps,
        signaled_9: signaled,
        drops_ran: drops.load(Ordering::Relaxed),
        fds_released: released,
        kill_to_reaped_ns: Dist::of(&lat),
    }
}

pub fn measure(reps: usize) -> Vec<KillRow> {
    let mut rows = Vec::new();
    for spec in ModelSpec::MAIN {
        progress(format!("H06: kill em {}", spec.label()));
        rows.push(scenario(spec, true, reps));
        rows.push(scenario(spec, false, reps));
    }
    rows
}

pub fn verdict(rows: &[KillRow]) -> HypOut {
    let all_ok = rows
        .iter()
        .all(|r| r.signaled_9 == r.reps && r.drops_ran == r.reps as u64 && r.fds_released == r.reps);
    let parts: Vec<String> = rows
        .iter()
        .map(|r| {
            format!(
                "{} {}: p50 {:.1} µs, p99 {:.1} µs",
                r.model,
                if r.scenario == "blocked_in_read" { "bloqueado" } else { "em laço" },
                r.kill_to_reaped_ns.p50 as f64 / 1e3,
                r.kill_to_reaped_ns.p99 as f64 / 1e3
            )
        })
        .collect();
    let total: usize = rows.iter().map(|r| r.reps).sum();
    let verdict = if all_ok { Verdict::Confirmed } else { Verdict::Refuted };
    HypOut {
        id: "H06",
        verdict,
        summary: format!(
            "{} de {total} kills terminaram com sinal 9, Drops rodando e fd fechado (EPIPE pro escritor). Do kill até o \
             wait colher: {}.",
            rows.iter().map(|r| r.signaled_9.min(r.fds_released).min(r.drops_ran as usize)).sum::<usize>(),
            parts.join("; ")
        ),
        evidence: json!({ "rows": rows, "unit": "ns" , "max_p99_us": r3(rows.iter().map(|r| r.kill_to_reaped_ns.p99).max().unwrap_or(0) as f64 / 1e3)}),
    }
}
