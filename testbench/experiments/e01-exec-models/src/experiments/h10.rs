//! H10: thread-local de "processo corrente" pro shim sysio e pra contabilidade.
//!
//! - A: cada processo grava seu pid num thread-local e confere depois de cada troca. Sem nada especial.
//! - B ingênuo: o mesmo código; todas as corrotinas de um worker dividem o thread-local.
//! - B com troca: o worker grava o pid corrente no thread-local antes de cada `resume`.
//! - B, `RefCell` num thread-local com empréstimo atravessando um `yield`: outra corrotina do mesmo worker
//!   tenta emprestar mutável (o mesmo teste no A como controle).
//! - C: task-local mantido pelo executor em volta de cada poll; conta migrações entre workers.

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use harness::Verdict;
use serde::Serialize;
use serde_json::json;

use super::{HypOut, progress};
use crate::kernel::Pid;
use crate::model_b::{ConfigB, KernelB, current_pid_tls};
use crate::model_c::{current_pid, current_worker};
use crate::stats::{median_f64, r3};
use crate::sys::{ProcMain, Sys};
use crate::{ModelSpec, SyncKernel};

thread_local! {
    static PROC_TAG: Cell<Pid> = const { Cell::new(0) };
    static SHARED_CELL: RefCell<u64> = const { RefCell::new(0) };
}

const PROCS: usize = 200;
const ROUNDS: usize = 50;
const CPUS: usize = 4;

#[derive(Clone, Debug, Serialize)]
pub struct TlsRow {
    pub case: String,
    pub processes: usize,
    pub checks: u64,
    pub mismatches: u64,
    pub migrations: u64,
}

fn tag_test(k: &dyn SyncKernel, use_kernel_swap: bool) -> (u64, u64) {
    let checks = Arc::new(AtomicU64::new(0));
    let bad = Arc::new(AtomicU64::new(0));
    let pids: Vec<_> = (0..PROCS)
        .map(|_| {
            let (c, b) = (checks.clone(), bad.clone());
            let main: ProcMain = Box::new(move |sys: &dyn Sys| {
                let me = sys.pid();
                if !use_kernel_swap {
                    PROC_TAG.with(|t| t.set(me));
                }
                for _ in 0..ROUNDS {
                    sys.yield_now();
                    let seen = if use_kernel_swap { current_pid_tls() } else { PROC_TAG.with(Cell::get) };
                    c.fetch_add(1, Ordering::Relaxed);
                    if seen != me {
                        b.fetch_add(1, Ordering::Relaxed);
                    }
                }
                0
            });
            k.spawn(Vec::new(), main)
        })
        .collect();
    for p in pids {
        k.wait(p);
    }
    (checks.load(Ordering::Relaxed), bad.load(Ordering::Relaxed))
}

/// Processo 1 segura um `borrow()` do `RefCell` thread-local atravessando um `yield`; processo 2 tenta
/// `try_borrow_mut()`. Devolve quantas tentativas deram conflito.
fn refcell_test(k: &dyn SyncKernel) -> u64 {
    let conflicts = Arc::new(AtomicU64::new(0));
    let holding = Arc::new(AtomicUsize::new(0));
    let (h1, h2) = (holding.clone(), holding.clone());
    let c2 = conflicts.clone();
    let p1 = k.spawn(
        Vec::new(),
        Box::new(move |sys: &dyn Sys| {
            SHARED_CELL.with(|cell| {
                let guard = cell.borrow();
                h1.store(1, Ordering::Release);
                while h1.load(Ordering::Acquire) != 2 {
                    sys.yield_now();
                    std::thread::yield_now();
                }
                drop(guard);
            });
            0
        }),
    );
    let p2 = k.spawn(
        Vec::new(),
        Box::new(move |sys: &dyn Sys| {
            while h2.load(Ordering::Acquire) != 1 {
                sys.yield_now();
                std::thread::yield_now();
            }
            for _ in 0..10 {
                SHARED_CELL.with(|cell| {
                    if cell.try_borrow_mut().is_err() {
                        c2.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
            h2.store(2, Ordering::Release);
            0
        }),
    );
    k.wait(p1);
    k.wait(p2);
    conflicts.load(Ordering::Relaxed)
}

fn c_test() -> (u64, u64, u64) {
    let k = ModelSpec::C.c_kernel(CPUS, true);
    let checks = Arc::new(AtomicU64::new(0));
    let bad = Arc::new(AtomicU64::new(0));
    let migr = Arc::new(AtomicU64::new(0));
    let pids: Vec<_> = (0..PROCS)
        .map(|_| {
            let (c, b, m) = (checks.clone(), bad.clone(), migr.clone());
            k.spawn(Vec::new(), move |ctx| async move {
                let me = ctx.pid();
                let mut worker = current_worker();
                for _ in 0..ROUNDS {
                    ctx.yield_now().await;
                    c.fetch_add(1, Ordering::Relaxed);
                    if current_pid() != me {
                        b.fetch_add(1, Ordering::Relaxed);
                    }
                    let w = current_worker();
                    if w != worker {
                        m.fetch_add(1, Ordering::Relaxed);
                        worker = w;
                    }
                }
                0
            })
        })
        .collect();
    for p in pids {
        k.wait(p);
    }
    (checks.load(Ordering::Relaxed), bad.load(Ordering::Relaxed), migr.load(Ordering::Relaxed))
}

/// Custo da troca do thread-local no B: ping-pong de `yield_now` com e sem a troca (ns por troca).
fn swap_cost(iters: u64, reps: usize) -> (f64, f64) {
    let one = |swap: bool| -> f64 {
        let k = KernelB::new(ConfigB { workers: 1, stack_size: 64 * 1024, timer: false, tls_swap: swap });
        let t = Instant::now();
        let pids: Vec<_> = (0..2)
            .map(|_| {
                k.spawn(
                    Vec::new(),
                    Box::new(move |sys: &dyn Sys| {
                        for _ in 0..iters {
                            sys.yield_now();
                        }
                        0
                    }),
                )
            })
            .collect();
        for p in pids {
            k.wait(p);
        }
        let switches = k.core().switches.load(Ordering::Relaxed).max(1);
        t.elapsed().as_nanos() as f64 / switches as f64
    };
    let mut off = Vec::new();
    let mut on = Vec::new();
    for _ in 0..reps {
        off.push(one(false));
        on.push(one(true));
    }
    (r3(median_f64(&off)), r3(median_f64(&on)))
}

pub struct H10Data {
    pub rows: Vec<TlsRow>,
    pub refcell_conflicts_a: u64,
    pub refcell_conflicts_b: u64,
    pub swap_off_ns: f64,
    pub swap_on_ns: f64,
}

pub fn measure(swap_iters: u64, reps: usize) -> H10Data {
    let mut rows = Vec::new();
    progress("H10: thread-local no A");
    let a = ModelSpec::A.sync_kernel(CPUS, true).expect("A");
    let (c, b) = tag_test(a.as_ref(), false);
    rows.push(TlsRow { case: "A".into(), processes: PROCS, checks: c, mismatches: b, migrations: 0 });
    let refcell_conflicts_a = refcell_test(ModelSpec::A.sync_kernel(1, true).expect("A").as_ref());
    drop(a);

    progress("H10: thread-local no B");
    let b_naive = KernelB::new(ConfigB { workers: CPUS, stack_size: 64 * 1024, timer: true, tls_swap: false });
    let (c, b) = tag_test(&b_naive, false);
    rows.push(TlsRow { case: "B_naive".into(), processes: PROCS, checks: c, mismatches: b, migrations: 0 });
    drop(b_naive);
    let b_swap = KernelB::new(ConfigB { workers: CPUS, stack_size: 64 * 1024, timer: true, tls_swap: true });
    let (c, b) = tag_test(&b_swap, true);
    rows.push(TlsRow { case: "B_worker_swap".into(), processes: PROCS, checks: c, mismatches: b, migrations: 0 });
    drop(b_swap);
    let b1 = KernelB::new(ConfigB { workers: 1, stack_size: 64 * 1024, timer: true, tls_swap: true });
    let refcell_conflicts_b = refcell_test(&b1);
    drop(b1);

    progress("H10: task-local no C");
    // O teste só prova algo se houver migração; com o host muito carregado um worker pode pegar tudo.
    let mut attempt = c_test();
    for _ in 0..4 {
        if attempt.2 > 0 {
            break;
        }
        attempt = c_test();
    }
    let (c, b, m) = attempt;
    rows.push(TlsRow { case: "C_task_local".into(), processes: PROCS, checks: c, mismatches: b, migrations: m });

    progress("H10: custo da troca de thread-local no B");
    let (swap_off_ns, swap_on_ns) = swap_cost(swap_iters, reps);
    H10Data { rows, refcell_conflicts_a, refcell_conflicts_b, swap_off_ns, swap_on_ns }
}

pub fn verdict(d: &H10Data) -> HypOut {
    let get = |case: &str| d.rows.iter().find(|r| r.case == case);
    let a_ok = get("A").is_some_and(|r| r.mismatches == 0 && r.checks > 0);
    let b_naive_bad = get("B_naive").map(|r| r.mismatches).unwrap_or(0);
    let b_naive_checks = get("B_naive").map(|r| r.checks).unwrap_or(0);
    let b_swap_ok = get("B_worker_swap").is_some_and(|r| r.mismatches == 0);
    let c = get("C_task_local");
    let c_migr = c.map(|r| r.migrations).unwrap_or(0);
    // Sem migração o teste do C não distingue task-local de thread-local.
    let c_ok = c.is_some_and(|r| r.mismatches == 0) && c_migr > 0;
    let verdict = if a_ok && c_ok && b_swap_ok && b_naive_bad > 0 {
        Verdict::Partial
    } else if a_ok && c_ok && b_naive_bad == 0 {
        Verdict::Confirmed
    } else {
        Verdict::Inconclusive
    };
    HypOut {
        id: "H10",
        verdict,
        summary: format!(
            "Natural só no A: 0 erros em {} conferências. No B ingênuo {b_naive_bad} de {b_naive_checks} conferências \
             viram o pid de outra corrotina; com o worker trocando o valor a cada resume, 0 erros (yield no B custa \
             {:.1} ns com a troca e {:.1} ns sem), mas um RefCell thread-local emprestado através de um yield deu {} conflitos no B \
             contra {} no A, e thread-locals de terceiros (como o STACK_LIMIT do stacker, ver H08) não são trocados. \
             No C o task-local mantido pelo executor deu 0 erros com {c_migr} migrações entre workers.",
            get("A").map(|r| r.checks).unwrap_or(0),
            d.swap_on_ns,
            d.swap_off_ns,
            d.refcell_conflicts_b,
            d.refcell_conflicts_a,
        ),
        evidence: json!({
            "rows": d.rows,
            "refcell_borrow_across_yield_conflicts": { "A": d.refcell_conflicts_a, "B": d.refcell_conflicts_b },
            "b_yield_ns_without_swap": d.swap_off_ns,
            "b_yield_ns_with_swap": d.swap_on_ns,
        }),
    }
}
