//! Candidato: `alloc-track` 0.4 (Protryon). Conta por thread num vetor estático de 1024 entradas e
//! guarda a thread de origem de cada ponteiro num `DashMap`, então a liberação feita por outra thread
//! é debitada de quem alocou.
//!
//! A API só entrega o número via `thread_report()`, que varre 1024 x 1024 contadores e monta um
//! `BTreeMap` de `String`. Pra não poluir a thread medida, uma thread auxiliar faz o relatório e
//! devolve um retrato num vetor pré-alocado (sincronização por `Mutex` e `Condvar`, sem alocação na
//! thread que pede). A crate também não expõe o índice da thread: o adaptador o descobre alocando um
//! marcador de tamanho único e vendo qual entrada cresceu.

use std::alloc::System;
use std::cell::Cell;
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};

use alloc_track::{AllocTrack, BacktraceMode};
use e07_mem_accounting::{Accounting, Capabilities, Pid, cli, demos};
use serde_json::json;

#[global_allocator]
static GLOBAL: AllocTrack<System> = AllocTrack::new(System, BacktraceMode::None);

const MAX_THREADS: usize = 1024;
const MARKER: usize = 3_333_331;

#[derive(Debug)]
struct Snapshot {
    pending: bool,
    total_alloc: Vec<u64>,
    current_used: Vec<u64>,
}

#[derive(Debug)]
struct Reporter {
    gate: Mutex<()>,
    state: Mutex<Snapshot>,
    req: Condvar,
    resp: Condvar,
}

static REPORTER: OnceLock<Reporter> = OnceLock::new();
static START_USED: [AtomicU64; MAX_THREADS] = [const { AtomicU64::new(0) }; MAX_THREADS];

fn reporter() -> &'static Reporter {
    REPORTER.get_or_init(|| {
        let r = Reporter {
            gate: Mutex::new(()),
            state: Mutex::new(Snapshot {
                pending: false,
                total_alloc: vec![0; MAX_THREADS],
                current_used: vec![0; MAX_THREADS],
            }),
            req: Condvar::new(),
            resp: Condvar::new(),
        };
        std::thread::Builder::new()
            .name("alloc-track-reporter".into())
            .spawn(|| reporter_loop(REPORTER.wait()))
            .expect("thread do relatório");
        r
    })
}

fn reporter_loop(r: &Reporter) {
    let mut st = r.state.lock().expect("estado");
    loop {
        while !st.pending {
            st = r.req.wait(st).expect("estado");
        }
        drop(st);
        let rep = alloc_track::thread_report();
        let mut alloc = [0u64; MAX_THREADS];
        let mut used = [0u64; MAX_THREADS];
        for (name, m) in &rep.0 {
            if let Ok(i) = name.parse::<usize>()
                && i < MAX_THREADS
            {
                alloc[i] = m.total_alloc;
                used[i] = m.current_used;
            }
        }
        drop(rep);
        st = r.state.lock().expect("estado");
        st.total_alloc.copy_from_slice(&alloc);
        st.current_used.copy_from_slice(&used);
        st.pending = false;
        r.resp.notify_all();
    }
}

/// Pede um retrato e copia as duas colunas pra arrays na pilha de quem pediu.
fn snapshot(alloc: &mut [u64; MAX_THREADS], used: &mut [u64; MAX_THREADS]) {
    let r = reporter();
    let _g = r.gate.lock().expect("gate");
    let mut st = r.state.lock().expect("estado");
    st.pending = true;
    r.req.notify_one();
    while st.pending {
        st = r.resp.wait(st).expect("estado");
    }
    alloc.copy_from_slice(&st.total_alloc);
    used.copy_from_slice(&st.current_used);
}

fn used_of(i: usize) -> u64 {
    let mut a = [0u64; MAX_THREADS];
    let mut u = [0u64; MAX_THREADS];
    snapshot(&mut a, &mut u);
    u[i]
}

thread_local! {
    static INDEX: Cell<Option<usize>> = const { Cell::new(None) };
    static LAST: Cell<Option<i64>> = const { Cell::new(None) };
}

/// Serializa a descoberta: com duas threads descobrindo ao mesmo tempo, os dois marcadores aparecem no
/// mesmo retrato e cada uma poderia pegar o índice da outra.
static DISCOVERY: Mutex<()> = Mutex::new(());

/// Descobre o índice que a alloc-track deu a esta thread.
fn my_index() -> usize {
    if let Some(i) = INDEX.with(Cell::get) {
        return i;
    }
    let _serial = DISCOVERY.lock().expect("descoberta");
    let mut a0 = [0u64; MAX_THREADS];
    let mut u0 = [0u64; MAX_THREADS];
    snapshot(&mut a0, &mut u0);
    let marker: Vec<u8> = Vec::with_capacity(MARKER);
    black_box(&marker);
    let mut a1 = [0u64; MAX_THREADS];
    let mut u1 = [0u64; MAX_THREADS];
    snapshot(&mut a1, &mut u1);
    drop(marker);
    // Entre os dois retratos esta thread só alocou o marcador, então o crescimento dela é o marcador
    // (com folga pequena). A thread do relatório cresce muito mais (1M Strings por relatório) e as
    // outras threads não alocam exatamente esse tamanho.
    let i = (0..MAX_THREADS)
        .find(|&i| (MARKER as u64..=MARKER as u64 + 4096).contains(&a1[i].saturating_sub(a0[i])))
        .expect("o marcador não apareceu em nenhuma thread");
    INDEX.with(|c| c.set(Some(i)));
    i
}

struct AllocTrackAdapter;

impl Accounting for AllocTrackAdapter {
    fn name(&self) -> &'static str {
        "alloc-track"
    }

    fn caps(&self) -> Capabilities {
        Capabilities { per_group: true, self_read: true, remote_read: true, ..Capabilities::default() }
    }

    fn run_process<R>(&self, _limit: Option<i64>, body: impl FnOnce(Pid) -> R) -> R {
        let i = my_index();
        let start = used_of(i);
        START_USED[i].store(start, Ordering::SeqCst);
        let out = body(Pid(i as u64));
        LAST.with(|c| c.set(Some(used_of(i) as i64 - start as i64)));
        out
    }

    fn self_live(&self) -> Option<i64> {
        let i = INDEX.with(Cell::get)?;
        Some(used_of(i) as i64 - START_USED[i].load(Ordering::SeqCst) as i64)
    }

    fn remote_live(&self, pid: Pid) -> Option<i64> {
        let i = pid.0 as usize;
        Some(used_of(i) as i64 - START_USED[i].load(Ordering::SeqCst) as i64)
    }

    fn last_process_net(&self) -> Option<i64> {
        LAST.with(Cell::get)
    }
}

/// Cria threads em sequência (cada uma aloca e morre). A alloc-track nunca reaproveita índice e faz
/// `assert!` dentro do allocator quando passa de 1024.
fn thread_limit(total: usize) -> serde_json::Value {
    for n in 0..total {
        std::thread::spawn(move || {
            let v = vec![n as u8; 64];
            black_box(&v);
        })
        .join()
        .expect("thread");
        if n % 128 == 0 {
            eprintln!("e07-demo: {n} threads criadas e encerradas");
        }
    }
    json!({"survived": true, "threads": total})
}

fn main() {
    reporter();
    cli::run(&AllocTrackAdapter, |a| match a.cmd.as_str() {
        "thread-limit" => Some(thread_limit(a.get("threads", 1200))),
        "huge-alloc" => Some(demos::huge_alloc(false)),
        _ => None,
    });
}
