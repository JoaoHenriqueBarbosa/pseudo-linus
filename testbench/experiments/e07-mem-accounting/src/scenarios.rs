//! Cenários de corretude e de latência, genéricos sobre o candidato.
//!
//! Regra dos cenários: entre duas leituras que formam uma medida, a thread medida só faz as alocações
//! do próprio cenário (nada de `format!`, `println!` ou canal), pra que a diferença seja exatamente o
//! que o cenário pediu. Sincronização é por `Barrier` e `Mutex` (futex, sem alocação).

use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Barrier, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::accounting::{Accounting, Pid};

/// Tolerância das comparações de bytes vivos (ruído de bookkeeping do runtime, ex.: pilha de grupos).
pub const TOLERANCE: i64 = 4096;

fn close(value: i64, expected: i64) -> bool {
    (value - expected).abs() <= TOLERANCE
}

fn diff(after: Option<i64>, before: Option<i64>) -> Option<i64> {
    Some(after? - before?)
}

// ---------------------------------------------------------------------------------------------
// Atribuição entre threads: A aloca, B libera (e B realoca o que recebeu de A).
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Attribution {
    pub payload_bytes: i64,
    pub grow_from: i64,
    pub grow_to: i64,
    /// Diferenças lidas de dentro (self), relativas à leitura do início de cada processo.
    pub a_self_after_alloc: Option<i64>,
    pub a_self_after_remote_free: Option<i64>,
    pub b_self_after_free: Option<i64>,
    pub a_self_after_remote_grow: Option<i64>,
    pub b_self_after_grow: Option<i64>,
    /// Leituras de fora (thread principal), relativas ao início de cada processo.
    pub a_remote_after_alloc: Option<i64>,
    pub a_remote_after_remote_free: Option<i64>,
    pub b_remote_after_free: Option<i64>,
    pub a_remote_after_remote_grow: Option<i64>,
    pub b_remote_after_grow: Option<i64>,
    /// Bytes que cada processo deixou atribuídos a si ao terminar.
    pub a_net_at_exit: Option<i64>,
    pub b_net_at_exit: Option<i64>,
    /// Diferença do contador global entre antes de A alocar e depois de B liberar.
    pub global_delta_after_free: Option<i64>,
    /// De onde veio o veredito: "self", "remote", "exit" ou "none".
    pub evidence: String,
    /// O grupo de A volta ao que era quando B libera, e o de B não muda.
    pub free_attributed_to_allocator: Option<bool>,
    /// Quando B realoca o buffer de A, a memória passa a ser de B.
    pub realloc_moves_ownership: Option<bool>,
}

pub fn attribution<A: Accounting>(acct: &A) -> Attribution {
    const N: usize = 1000;
    const SIZE: usize = 1000;
    let payload_bytes = (N * SIZE + N * std::mem::size_of::<Box<[u8]>>()) as i64;
    let grow_from: usize = 1 << 20;
    let grow_to: usize = 4 << 20;

    let slot: Mutex<Option<Vec<Box<[u8]>>>> = Mutex::new(None);
    let grow_slot: Mutex<Option<Vec<u8>>> = Mutex::new(None);
    let bar = Barrier::new(3);
    let a_pid = AtomicU64::new(0);
    let b_pid = AtomicU64::new(0);
    let rec = Mutex::new(Attribution {
        payload_bytes,
        grow_from: grow_from as i64,
        grow_to: grow_to as i64,
        ..Attribution::default()
    });

    std::thread::scope(|s| {
        let ha = s.spawn(|| {
            let (base, after_alloc, after_free, after_grow) = acct.run_process(None, |pid| {
                a_pid.store(pid.0, Ordering::SeqCst);
                let base = acct.self_live();
                bar.wait(); // 0: todos com a leitura inicial feita
                bar.wait(); // 0b: principal leu o contador global
                let mut payload: Vec<Box<[u8]>> = Vec::with_capacity(N);
                for _ in 0..N {
                    payload.push(vec![7u8; SIZE].into_boxed_slice());
                }
                let after_alloc = acct.self_live();
                *slot.lock().expect("slot") = Some(payload);
                bar.wait(); // 1: carga publicada
                bar.wait(); // 2: principal leu A de fora
                bar.wait(); // 3: B liberou a carga
                let after_free = acct.self_live();
                bar.wait(); // 4: leituras pós-liberação feitas
                let grow = vec![1u8; grow_from];
                *grow_slot.lock().expect("slot") = Some(grow);
                bar.wait(); // 5: buffer publicado
                bar.wait(); // 6: B cresceu o buffer
                let after_grow = acct.self_live();
                bar.wait(); // 7: leituras pós-realocação feitas
                bar.wait(); // 8: B liberou o buffer crescido
                (base, after_alloc, after_free, after_grow)
            });
            let net = acct.last_process_net();
            let mut r = rec.lock().expect("rec");
            r.a_self_after_alloc = diff(after_alloc, base);
            r.a_self_after_remote_free = diff(after_free, base);
            r.a_self_after_remote_grow = diff(after_grow, base);
            r.a_net_at_exit = net;
        });
        let hb = s.spawn(|| {
            let (base, after_free, after_grow) = acct.run_process(None, |pid| {
                b_pid.store(pid.0, Ordering::SeqCst);
                let base = acct.self_live();
                bar.wait(); // 0
                bar.wait(); // 0b
                bar.wait(); // 1
                bar.wait(); // 2
                let payload = slot.lock().expect("slot").take();
                drop(payload);
                bar.wait(); // 3
                let after_free = acct.self_live();
                bar.wait(); // 4
                bar.wait(); // 5
                let mut g = grow_slot.lock().expect("slot").take().expect("buffer de A");
                g.reserve_exact(grow_to - g.len());
                black_box(&g);
                bar.wait(); // 6
                let after_grow = acct.self_live();
                bar.wait(); // 7
                drop(g);
                bar.wait(); // 8
                (base, after_free, after_grow)
            });
            let net = acct.last_process_net();
            let mut r = rec.lock().expect("rec");
            r.b_self_after_free = diff(after_free, base);
            r.b_self_after_grow = diff(after_grow, base);
            r.b_net_at_exit = net;
        });

        // Thread principal: o "kernel" que lê os processos de fora.
        bar.wait(); // 0
        let g0 = acct.global_live();
        bar.wait(); // 0b
        bar.wait(); // 1
        let a = Pid(a_pid.load(Ordering::SeqCst));
        let b = Pid(b_pid.load(Ordering::SeqCst));
        let a_alloc = acct.remote_live(a);
        bar.wait(); // 2
        bar.wait(); // 3
        let a_free = acct.remote_live(a);
        let b_free = acct.remote_live(b);
        let g1 = acct.global_live();
        bar.wait(); // 4
        bar.wait(); // 5
        bar.wait(); // 6
        let a_grow = acct.remote_live(a);
        let b_grow = acct.remote_live(b);
        bar.wait(); // 7
        bar.wait(); // 8
        ha.join().expect("thread A");
        hb.join().expect("thread B");
        let mut r = rec.lock().expect("rec");
        r.a_remote_after_alloc = a_alloc;
        r.a_remote_after_remote_free = a_free;
        r.b_remote_after_free = b_free;
        r.a_remote_after_remote_grow = a_grow;
        r.b_remote_after_grow = b_grow;
        r.global_delta_after_free = diff(g1, g0);
    });

    let mut r = rec.into_inner().expect("rec");
    let pb = r.payload_bytes;
    let gt = r.grow_to;
    let judge = |alloc: Option<i64>, a_free: Option<i64>, b_free: Option<i64>, a_grow: Option<i64>, b_grow: Option<i64>| {
        let free_ok = match (alloc, a_free, b_free) {
            (Some(al), Some(af), Some(bf)) => Some(close(al, pb) && close(af, 0) && close(bf, 0)),
            _ => None,
        };
        let grow_ok = match (a_grow, b_grow) {
            (Some(ag), Some(bg)) => Some(close(ag, 0) && close(bg, gt)),
            _ => None,
        };
        (free_ok, grow_ok)
    };
    let (free_ok, grow_ok, evidence) = if r.a_self_after_alloc.is_some() && r.b_self_after_free.is_some() {
        let (f, g) = judge(
            r.a_self_after_alloc,
            r.a_self_after_remote_free,
            r.b_self_after_free,
            r.a_self_after_remote_grow,
            r.b_self_after_grow,
        );
        (f, g, "self")
    } else if r.a_remote_after_alloc.is_some() {
        let (f, g) = judge(
            r.a_remote_after_alloc,
            r.a_remote_after_remote_free,
            r.b_remote_after_free,
            r.a_remote_after_remote_grow,
            r.b_remote_after_grow,
        );
        (f, g, "remote")
    } else if let (Some(an), Some(bn)) = (r.a_net_at_exit, r.b_net_at_exit) {
        (Some(close(an, 0) && close(bn, 0)), None, "exit")
    } else {
        (None, None, "none")
    };
    r.free_attributed_to_allocator = free_ok;
    r.realloc_moves_ownership = grow_ok;
    r.evidence = evidence.to_string();
    r
}

// ---------------------------------------------------------------------------------------------
// Bytes vivos em cenários controlados.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct Case {
    pub name: String,
    pub expected: i64,
    pub measured: Option<i64>,
    /// "self", "remote" ou "global".
    pub source: String,
    pub error: Option<i64>,
    pub ok: Option<bool>,
}

fn case(name: &str, expected: i64, measured: Option<i64>, source: &str) -> Case {
    let error = measured.map(|m| m - expected);
    Case { name: name.to_string(), expected, measured, source: source.to_string(), error, ok: error.map(|e| e.abs() <= TOLERANCE) }
}

/// Lê pelo melhor caminho disponível e diz qual foi.
fn read_any<A: Accounting>(acct: &A, pid: Pid) -> (Option<i64>, &'static str) {
    if let Some(v) = acct.self_live() {
        (Some(v), "self")
    } else if let Some(v) = acct.remote_live(pid) {
        (Some(v), "remote")
    } else if let Some(v) = acct.global_live() {
        (Some(v), "global")
    } else {
        (None, "none")
    }
}

pub fn controlled<A: Accounting>(acct: &A) -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    // Os registros são montados fora das janelas de medida: dentro só há i64 e as alocações do caso.
    let single = acct.run_process(None, |pid| {
        let mut rows: [(i64, Option<i64>, &'static str); 7] = [(0, None, "none"); 7];
        let (start, src) = read_any(acct, pid);

        // 1. Vec com capacidade exata de 1 MiB.
        let v: Vec<u8> = Vec::with_capacity(1 << 20);
        black_box(&v);
        let (after, _) = read_any(acct, pid);
        rows[0] = (1 << 20, diff(after, start), src);

        // 2. 10k caixas de 100 bytes num Vec com capacidade exata.
        let (before, _) = read_any(acct, pid);
        let mut boxes: Vec<Box<[u8; 100]>> = Vec::with_capacity(10_000);
        for i in 0..10_000u32 {
            boxes.push(Box::new([i as u8; 100]));
        }
        black_box(&boxes);
        let (after, _) = read_any(acct, pid);
        rows[1] = (10_000 * 8 + 10_000 * 100, diff(after, before), src);

        // 3. Crescimento por push (cadeia de realocações).
        let (before, _) = read_any(acct, pid);
        let mut grow: Vec<u64> = Vec::new();
        for i in 0..100_000u64 {
            grow.push(i);
        }
        black_box(&grow);
        let (after, _) = read_any(acct, pid);
        rows[2] = ((grow.capacity() * 8) as i64, diff(after, before), src);

        // 4. String encolhida (realocação pra baixo).
        let (before, _) = read_any(acct, pid);
        let mut s = String::with_capacity(1 << 20);
        for _ in 0..1000 {
            s.push('x');
        }
        s.shrink_to_fit();
        black_box(&s);
        let (after, _) = read_any(acct, pid);
        rows[3] = (s.capacity() as i64, diff(after, before), src);

        // 5. Tudo liberado: volta ao início.
        drop(v);
        drop(boxes);
        drop(grow);
        drop(s);
        let (after, _) = read_any(acct, pid);
        rows[4] = (0, diff(after, start), src);

        // 6. Alocação do kernel em nome do processo (buffer de pipe): fora da conta se houver escopo.
        let (before, _) = read_any(acct, pid);
        let k = acct.kernel_scope(|| vec![0u8; 2 << 20]);
        black_box(&k);
        let (after, _) = read_any(acct, pid);
        rows[5] = (0, diff(after, before), src);
        acct.kernel_scope(|| drop(k));
        let (after, _) = read_any(acct, pid);
        rows[6] = (0, diff(after, start), src);
        rows
    });
    let names = [
        "vec_1mib",
        "boxes_10k_x100",
        "vec_push_growth",
        "string_shrink_to_fit",
        "all_freed",
        "kernel_scope_2mib_excluded",
        "kernel_scope_freed",
    ];
    for (name, (expected, measured, src)) in names.iter().zip(single) {
        out.push(case(name, expected, measured, src));
    }

    // 7. Dois processos simultâneos com quantidades diferentes: cada um vê só o seu.
    let bar = Barrier::new(3);
    let pids = [AtomicU64::new(0), AtomicU64::new(0)];
    let sizes: [usize; 2] = [3 << 20, 5 << 20];
    let mut selfs: [Option<i64>; 2] = [None, None];
    let mut remotes: [Option<i64>; 2] = [None, None];
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..2)
            .map(|i| {
                let (bar, pids) = (&bar, &pids);
                s.spawn(move || {
                    acct.run_process(None, |pid| {
                        pids[i].store(pid.0, Ordering::SeqCst);
                        let start = acct.self_live();
                        let buf = vec![1u8; sizes[i]];
                        black_box(&buf);
                        let after = acct.self_live();
                        bar.wait(); // 1: os dois segurando a memória
                        bar.wait(); // 2: principal leu de fora
                        drop(buf);
                        diff(after, start)
                    })
                })
            })
            .collect();
        bar.wait();
        for i in 0..2 {
            remotes[i] = acct.remote_live(Pid(pids[i].load(Ordering::SeqCst)));
        }
        bar.wait();
        for (i, h) in handles.into_iter().enumerate() {
            selfs[i] = h.join().expect("processo");
        }
    });
    for i in 0..2 {
        let name = if i == 0 { "two_processes_a_3mib" } else { "two_processes_b_5mib" };
        if selfs[i].is_some() {
            out.push(case(name, sizes[i] as i64, selfs[i], "self"));
        } else {
            out.push(case(name, sizes[i] as i64, remotes[i], "remote"));
        }
        if selfs[i].is_some() && remotes[i].is_some() {
            out.push(case(&format!("{name}_remote"), sizes[i] as i64, remotes[i], "remote"));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Resíduo na saída da thread: o que o grupo ainda conta depois que o processo morre.
// ---------------------------------------------------------------------------------------------

thread_local! {
    static TLS_BUF: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExitResidual {
    pub tls_bytes: i64,
    /// Bytes que o processo ainda tinha ao fim do `run_process` (o TLS continua vivo nesse ponto).
    pub net_at_process_end: Option<i64>,
    /// Leitura de fora depois do `join`: o que sobra depois dos destrutores de TLS.
    pub remote_after_join: Option<i64>,
}

pub fn exit_residual<A: Accounting>(acct: &A) -> ExitResidual {
    const TLS: usize = 1 << 20;
    let pid = AtomicU64::new(0);
    let net = std::thread::scope(|s| {
        s.spawn(|| {
            acct.run_process(None, |p| {
                pid.store(p.0, Ordering::SeqCst);
                TLS_BUF.with(|b| b.borrow_mut().reserve_exact(TLS));
            });
            acct.last_process_net()
        })
        .join()
        .expect("processo")
    });
    let remote = acct.remote_live(Pid(pid.load(Ordering::SeqCst)));
    ExitResidual { tls_bytes: TLS as i64, net_at_process_end: net, remote_after_join: remote }
}

// ---------------------------------------------------------------------------------------------
// Estouro de limite: quanto passa e quanto demora até o kernel perceber.
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Detect {
    /// O processo lê os próprios bytes vivos a cada `every` alocações (checkpoint lendo o contador).
    SelfPoll,
    /// O allocator marca a flag; o checkpoint a cada `every` alocações só lê a flag.
    Flag,
    /// Um vigia lê de fora a cada `period_us` e liga a flag de kill; o checkpoint a cada `every` lê.
    Watcher,
}

impl std::str::FromStr for Detect {
    type Err = String;
    fn from_str(s: &str) -> Result<Detect, String> {
        match s {
            "self_poll" => Ok(Detect::SelfPoll),
            "flag" => Ok(Detect::Flag),
            "watcher" => Ok(Detect::Watcher),
            other => Err(format!("modo de detecção desconhecido: {other}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct OvershootCfg {
    /// Teto de dados do processo, além do vetor de controle do próprio cenário.
    pub limit: i64,
    pub chunk: usize,
    pub detect: Detect,
    pub every: u64,
    pub period_us: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct OvershootRun {
    pub detected: bool,
    pub timed_out: bool,
    /// Bytes alocados de verdade além do teto no momento em que o processo foi parado.
    pub overshoot_bytes: i64,
    /// Do instante em que a verdade passou do teto até o processo parar.
    pub latency_ns: u64,
    /// Do cruzamento até o vigia ligar a flag (só no modo vigia).
    pub watcher_latency_ns: Option<u64>,
    pub chunks: u64,
    pub polls: u64,
    /// Bytes vivos do processo logo depois do "kill" (tudo liberado no unwind).
    pub live_after_kill: Option<i64>,
}

pub fn overshoot_once<A: Accounting>(acct: &A, cfg: &OvershootCfg) -> OvershootRun {
    let chunk = cfg.chunk as i64;
    let max_truth = cfg.limit * 2 + (cfg.every as i64) * chunk * 2 + (16 << 20);
    let max_chunks = (max_truth / chunk) as usize + 2;
    let held_bytes = (max_chunks * std::mem::size_of::<Vec<u8>>()) as i64;
    let limit_total = cfg.limit + held_bytes;
    let deadline = Duration::from_secs(5);

    let kill = AtomicBool::new(false);
    let done = AtomicBool::new(false);
    let ready = AtomicBool::new(false);
    let pid_cell = AtomicU64::new(0);
    let watcher_ns = AtomicU64::new(u64::MAX);
    let polls = AtomicU64::new(0);
    let t0 = Instant::now();

    let mut run = std::thread::scope(|s| {
        let watcher = (cfg.detect == Detect::Watcher).then(|| {
            s.spawn(|| {
                while !ready.load(Ordering::Acquire) && !done.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
                let pid = Pid(pid_cell.load(Ordering::Acquire));
                let period = Duration::from_micros(cfg.period_us);
                while !done.load(Ordering::Acquire) {
                    std::thread::sleep(period);
                    polls.fetch_add(1, Ordering::Relaxed);
                    if acct.remote_live(pid).is_some_and(|live| live > limit_total) {
                        watcher_ns.store(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
                        kill.store(true, Ordering::Release);
                        break;
                    }
                }
            })
        });
        let proc = s.spawn(|| {
            acct.run_process(Some(limit_total), |pid| {
                let mut held: Vec<Vec<u8>> = Vec::with_capacity(max_chunks);
                pid_cell.store(pid.0, Ordering::Release);
                ready.store(true, Ordering::Release);
                let mut truth = held_bytes;
                let mut crossed: Option<u64> = None;
                let mut detected: Option<u64> = None;
                let mut timed_out = false;
                let mut i = 0u64;
                loop {
                    held.push(vec![1u8; cfg.chunk]);
                    truth += chunk;
                    if crossed.is_none() && truth > limit_total {
                        crossed = Some(t0.elapsed().as_nanos() as u64);
                    }
                    i += 1;
                    if i.is_multiple_of(cfg.every) {
                        let hit = match cfg.detect {
                            Detect::SelfPoll => acct.self_live().is_some_and(|l| l > limit_total),
                            Detect::Flag => acct.over_limit(pid).unwrap_or(false),
                            Detect::Watcher => kill.load(Ordering::Acquire),
                        };
                        if hit {
                            detected = Some(t0.elapsed().as_nanos() as u64);
                            break;
                        }
                        if t0.elapsed() > deadline {
                            timed_out = true;
                            break;
                        }
                    }
                    if truth > max_truth {
                        break;
                    }
                }
                let truth_at_stop = truth;
                drop(held);
                let after = acct.self_live();
                let run = OvershootRun {
                    detected: detected.is_some(),
                    timed_out,
                    overshoot_bytes: truth_at_stop - limit_total,
                    latency_ns: match (crossed, detected) {
                        (Some(c), Some(d)) => d.saturating_sub(c),
                        _ => 0,
                    },
                    watcher_latency_ns: None,
                    chunks: i,
                    polls: 0,
                    live_after_kill: after,
                };
                (run, crossed)
            })
        });
        let r = proc.join().expect("processo do estouro");
        done.store(true, Ordering::Release);
        if let Some(w) = watcher {
            w.join().expect("vigia");
        }
        r
    });
    run.0.polls = polls.load(Ordering::Relaxed);
    let w = watcher_ns.load(Ordering::Relaxed);
    if w != u64::MAX {
        run.0.watcher_latency_ns = run.1.map(|c| w.saturating_sub(c));
    }
    run.0
}

// ---------------------------------------------------------------------------------------------
// Custos fixos: criar o grupo de um processo, ler o contador, entrar no escopo do kernel.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Costs {
    /// ns por `run_process` vazio (registrar e entrar no grupo, sem criar thread).
    pub process_setup_ns: f64,
    pub self_read_ns: Option<f64>,
    pub remote_read_ns: Option<f64>,
    pub kernel_scope_ns: f64,
}

fn per_iter(budget: Duration, max_iters: u64, mut f: impl FnMut()) -> f64 {
    let t0 = Instant::now();
    let mut n = 0u64;
    while n < max_iters {
        for _ in 0..16 {
            f();
        }
        n += 16;
        if t0.elapsed() > budget {
            break;
        }
    }
    t0.elapsed().as_nanos() as f64 / n as f64
}

pub fn costs<A: Accounting>(acct: &A) -> Costs {
    let budget = Duration::from_millis(200);
    let process_setup_ns = per_iter(budget, 200_000, || {
        black_box(acct.run_process(None, |p| black_box(p).0));
    });
    let self_read_ns = acct.run_process(None, |_| {
        acct.self_live().map(|_| per_iter(budget, 2_000_000, || {
            black_box(acct.self_live());
        }))
    });
    let kernel_scope_ns = acct.run_process(None, |_| {
        per_iter(budget, 2_000_000, || {
            acct.kernel_scope(|| black_box(1u64));
        })
    });
    // Leitura remota: um processo vivo esperando, a thread principal lendo de fora.
    let bar = Barrier::new(2);
    let pid = AtomicU64::new(0);
    let remote_read_ns = std::thread::scope(|s| {
        s.spawn(|| {
            acct.run_process(None, |p| {
                pid.store(p.0, Ordering::SeqCst);
                bar.wait();
                bar.wait();
            })
        });
        bar.wait();
        let p = Pid(pid.load(Ordering::SeqCst));
        let r = acct.remote_live(p).map(|_| {
            per_iter(budget, 2_000_000, || {
                black_box(acct.remote_live(p));
            })
        });
        bar.wait();
        r
    });
    Costs { process_setup_ns, self_read_ns, remote_read_ns, kernel_scope_ns }
}
