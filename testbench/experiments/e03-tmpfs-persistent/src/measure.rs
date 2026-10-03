//! Cenários de tempo do H17. Rodam no binário principal (allocator do sistema, sem contador).

use std::hint::black_box;
use std::sync::Barrier;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::bench::{Summary, per_iter, summarize, time_ns};
use crate::concurrent::{LockedSandbox, RcuSandbox, SharedSandbox, ShardedSandbox};
use crate::maps::Flavor;
use crate::vfs::Vfs;
use crate::workload::{Image, ImageSpec, Rng, apply_changes, build_big_dir, build_huge_file, build_image};

pub const SIZES: [usize; 3] = [1_000, 10_000, 100_000];
pub const DEPTHS: [usize; 5] = [1, 4, 8, 16, 32];
pub const BIG_DIR: usize = 10_000;
pub const HUGE_FILE: usize = 100 << 20;

/// Coleta amostras de `f` até `max_n` ou até estourar o orçamento (com pelo menos `min_n`).
pub fn sample(max_n: usize, min_n: usize, budget: Duration, mut f: impl FnMut(usize) -> u64) -> Summary {
    let start = Instant::now();
    let mut samples = Vec::with_capacity(max_n);
    for i in 0..max_n {
        samples.push(f(i));
        if samples.len() >= min_n && start.elapsed() > budget {
            break;
        }
    }
    summarize(&mut samples)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn s(x: &Summary) -> Value {
    serde_json::to_value(x).expect("resumo")
}

/// Primeira escrita de 1 byte num arquivo depois de um snapshot (cópia de caminho), e a segunda
/// escrita no mesmo arquivo sem snapshot no meio (no lugar).
fn first_and_second_write<F: Flavor>(vfs: &mut Vfs<F>, paths: &[Vec<u8>], rng: &mut Rng) -> (Summary, Summary) {
    let mut second = Vec::new();
    let first = sample(2000, 30, Duration::from_millis(300), |_| {
        let path = &paths[rng.below(paths.len() as u64) as usize];
        let snap = vfs.snapshot();
        let (t1, r) = time_ns(|| vfs.write(path, 0, b"x"));
        r.expect("escrita");
        let (t2, r) = time_ns(|| vfs.write(path, 0, b"y"));
        r.expect("escrita");
        second.push(t2);
        drop(snap);
        t1
    });
    (first, summarize(&mut second))
}

/// Snapshot, restore, sandbox nova e escrita pós-snapshot em imagens de 1k, 10k e 100k arquivos,
/// mais a escrita em função da profundidade, diretório grande e 100 sandboxes derivadas.
pub fn structure_timing<F: Flavor>() -> Value {
    let mut sizes = Vec::new();
    let mut depth = Vec::new();
    let mut sandboxes = Value::Null;
    for &n in &SIZES {
        let t = Instant::now();
        let img: Image<F> = build_image(&ImageSpec::with_files(n));
        let build = t.elapsed();
        let fs = &img.fs;
        let snapshot = per_iter(25, 20_000, || drop(black_box(fs.clone())));
        let sandbox_new = per_iter(25, 20_000, || drop(black_box(Vfs::from_image(fs))));
        let mut rng = Rng::new(n as u64);
        let mut vfs = Vfs::from_image(fs);
        let base = vfs.snapshot();
        // Frio: 10 arquivos sorteados a cada rodada (caminhos fora do cache, como na vida real).
        let restore = sample(400, 30, Duration::from_millis(300), |_| {
            for _ in 0..10 {
                let p = &img.files[rng.below(img.files.len() as u64) as usize];
                vfs.write(p, 0, b"z").expect("escrita");
            }
            time_ns(|| vfs.restore(&base)).0
        });
        // Quente: sempre o mesmo arquivo e uma escrita só, pra isolar o trabalho por escrita do
        // efeito de cache (com 10 escritas, numa imagem pequena os caminhos se sobrepõem).
        let hot: Vec<Vec<u8>> = (0..10).map(|i| img.files[i * img.files.len() / 10].clone()).collect();
        let restore_hot = sample(2000, 30, Duration::from_millis(200), |_| {
            vfs.write(&hot[0], 0, b"z").expect("escrita");
            time_ns(|| vfs.restore(&base)).0
        });
        let (first_hot, _) = first_and_second_write(&mut vfs, &hot[..1], &mut rng);
        let (first, second) = first_and_second_write(&mut vfs, &img.files, &mut rng);
        let stat = sample(2000, 30, Duration::from_millis(100), |_| {
            let p = &img.files[rng.below(img.files.len() as u64) as usize];
            time_ns(|| vfs.stat(p).expect("stat")).0
        });
        if n == *SIZES.last().expect("tamanhos") {
            for &d in &DEPTHS {
                let path = img.deep_files[d - 1].clone();
                let (first, second) = first_and_second_write(&mut vfs, std::slice::from_ref(&path), &mut rng);
                let stat = sample(2000, 30, Duration::from_millis(50), |_| time_ns(|| vfs.stat(&path).expect("stat")).0);
                depth.push(json!({"depth": d, "first_write": s(&first), "second_write": s(&second), "stat": s(&stat)}));
            }
            let t = Instant::now();
            let mut derived: Vec<Vfs<F>> = Vec::with_capacity(100);
            let mut errors = 0;
            for i in 0..100 {
                let mut v = Vfs::from_image(fs);
                errors += apply_changes(&mut v, &img.files, i, &mut rng).errors;
                derived.push(v);
            }
            let total = t.elapsed();
            sandboxes = json!({"count": 100, "total_ms": ms(total), "per_sandbox_us": total.as_secs_f64() * 1e6 / 100.0, "errors": errors});
            drop(derived);
        }
        drop(vfs);
        let t = Instant::now();
        drop(img);
        let drop_image = t.elapsed();
        sizes.push(json!({
            "files": n,
            "build_ms": ms(build),
            "build_us_per_file": build.as_secs_f64() * 1e6 / n as f64,
            "drop_image_ms": ms(drop_image),
            "snapshot_and_drop_ns": s(&snapshot),
            "sandbox_new_and_drop_ns": s(&sandbox_new),
            "restore_after_10_writes_ns": s(&restore),
            "restore_after_1_write_hot_ns": s(&restore_hot),
            "first_write_after_snapshot_ns": s(&first),
            "first_write_after_snapshot_hot_ns": s(&first_hot),
            "second_write_ns": s(&second),
            "stat_ns": s(&stat),
        }));
    }
    json!({"sizes": sizes, "depth": depth, "sandboxes_100k": sandboxes, "big_dir": big_dir_timing::<F>()})
}

/// Diretório com 10k entradas: criar depois de snapshot, criar no lugar, lookup e readdir.
pub fn big_dir_timing<F: Flavor>() -> Value {
    let t = Instant::now();
    let fs = build_big_dir::<F>(BIG_DIR);
    let build = t.elapsed();
    let mut vfs = Vfs::from_image(&fs);
    let mut k = 0usize;
    let first = sample(1000, 20, Duration::from_millis(300), |_| {
        let snap = vfs.snapshot();
        k += 1;
        let p = format!("/big/new{k}").into_bytes();
        let t = time_ns(|| vfs.create(&p, 0o644).expect("create")).0;
        drop(snap);
        t
    });
    let in_place = sample(1000, 20, Duration::from_millis(100), |_| {
        k += 1;
        let p = format!("/big/new{k}").into_bytes();
        time_ns(|| vfs.create(&p, 0o644).expect("create")).0
    });
    let mut rng = Rng::new(10);
    let lookup = sample(2000, 30, Duration::from_millis(100), |_| {
        let p = format!("/big/e{}", rng.below(BIG_DIR as u64)).into_bytes();
        time_ns(|| vfs.stat(&p).expect("stat")).0
    });
    let readdir = sample(50, 5, Duration::from_millis(200), |_| time_ns(|| vfs.readdir(b"/big").expect("readdir").len()).0);
    json!({
        "entries": BIG_DIR,
        "build_ms": ms(build),
        "create_after_snapshot_ns": s(&first),
        "create_in_place_ns": s(&in_place),
        "lookup_ns": s(&lookup),
        "readdir_sorted_ns": s(&readdir),
    })
}

/// Arquivo de 100 MiB: 1 byte escrito depois de snapshot, no lugar, e leitura de 4 KiB.
pub fn huge_file_timing<F: Flavor>() -> Value {
    let t = Instant::now();
    let fs = build_huge_file::<F>(HUGE_FILE);
    let build = t.elapsed();
    let mut vfs = Vfs::from_image(&fs);
    let mut rng = Rng::new(100);
    let mut second = Vec::new();
    let first = sample(500, 10, Duration::from_millis(1000), |_| {
        let off = rng.below(HUGE_FILE as u64);
        let snap = vfs.snapshot();
        let t1 = time_ns(|| vfs.write(b"/huge", off, b"x").expect("escrita")).0;
        second.push(time_ns(|| vfs.write(b"/huge", off, b"y").expect("escrita")).0);
        drop(snap);
        t1
    });
    let read = sample(2000, 30, Duration::from_millis(100), |_| {
        let off = rng.below(HUGE_FILE as u64 - 4096);
        time_ns(|| vfs.read(b"/huge", off, 4096).expect("leitura").len()).0
    });
    let t = Instant::now();
    let mut sink = 0u64;
    let mut off = 0;
    while off < HUGE_FILE {
        sink += u64::from(vfs.read(b"/huge", off as u64, 1 << 20).expect("leitura")[0]);
        off += 1 << 20;
    }
    black_box(sink);
    let seq = t.elapsed();
    json!({
        "size_bytes": HUGE_FILE,
        "build_ms": ms(build),
        "build_mib_per_s": (HUGE_FILE as f64 / (1 << 20) as f64) / build.as_secs_f64(),
        "write_1_byte_after_snapshot_ns": s(&first),
        "write_1_byte_in_place_ns": s(&summarize(&mut second)),
        "read_4k_ns": s(&read),
        "sequential_read_mib_per_s": (HUGE_FILE as f64 / (1 << 20) as f64) / seq.as_secs_f64(),
    })
}

/// Tempo de montar a imagem de 100k arquivos (conteúdo pesa aqui).
pub fn content_image_timing<F: Flavor>() -> Value {
    let t = Instant::now();
    let img: Image<F> = build_image(&ImageSpec::with_files(100_000));
    let build = t.elapsed();
    json!({"files": 100_000, "content_bytes": img.content_bytes, "build_ms": ms(build)})
}

// ---------------------------------------------------------------------------------------------
// Vazão com escritores concorrentes.

pub const THREADS: [usize; 3] = [1, 4, 16];
pub const OPS_PER_THREAD: usize = 20_000;
pub const FILES_PER_WORKER: usize = 32;
pub const WORKER_FILE: usize = 16 << 10;

/// Imagem pra vazão: 1000 arquivos comuns mais `/w<t>` com 32 arquivos de 16 KiB por thread.
pub fn concurrency_image<F: Flavor>(threads: usize) -> crate::fs::Fs<F> {
    let img: Image<F> = build_image(&ImageSpec::with_files(1000));
    let mut vfs = Vfs::from_image(&img.fs);
    let data = vec![7u8; WORKER_FILE];
    for t in 0..threads {
        vfs.mkdir(format!("/w{t}").as_bytes(), 0o755).expect("mkdir w");
        for f in 0..FILES_PER_WORKER {
            let p = format!("/w{t}/f{f}").into_bytes();
            vfs.create(&p, 0o644).expect("create w");
            vfs.write(&p, 0, &data).expect("write w");
        }
    }
    vfs.into_fs()
}

/// Uma operação do laço de cada thread: 60% escrita de 4 KiB, 20% criação + escrita de 512 B,
/// 20% stat. Devolve quantas chamadas fez.
fn worker_op(rng: &mut Rng, t: usize, k: &mut usize, buf: &[u8], mut call: impl FnMut(WorkerCall<'_>)) -> usize {
    let r = rng.below(10);
    if r < 6 {
        let f = rng.below(FILES_PER_WORKER as u64);
        let off = rng.below((WORKER_FILE / 4096) as u64) * 4096;
        call(WorkerCall::Write(format!("/w{t}/f{f}").into_bytes(), off, buf));
        1
    } else if r < 8 {
        *k += 1;
        let p = format!("/w{t}/n{k}").into_bytes();
        call(WorkerCall::Create(p.clone()));
        call(WorkerCall::Write(p, 0, &buf[..512]));
        2
    } else {
        let f = rng.below(FILES_PER_WORKER as u64);
        call(WorkerCall::Stat(format!("/w{t}/f{f}").into_bytes()));
        1
    }
}

enum WorkerCall<'a> {
    Write(Vec<u8>, u64, &'a [u8]),
    Create(Vec<u8>),
    Stat(Vec<u8>),
}

fn run_shared<F: Flavor, S: SharedSandbox<F>>(image: &S::Image, threads: usize) -> (f64, u64) {
    let sb = S::from_image(image);
    let barrier = Barrier::new(threads + 1);
    let elapsed = std::thread::scope(|scope| {
        for t in 0..threads {
            let sb = &sb;
            let barrier = &barrier;
            scope.spawn(move || {
                let mut rng = Rng::new(t as u64 + 1);
                let buf = vec![t as u8; 4096];
                let mut k = 0;
                barrier.wait();
                let mut done = 0;
                while done < OPS_PER_THREAD {
                    done += worker_op(&mut rng, t, &mut k, &buf, |c| match c {
                        WorkerCall::Write(p, off, d) => {
                            sb.write(&p, off, d).expect("escrita");
                        }
                        WorkerCall::Create(p) => {
                            sb.create(&p).expect("create");
                        }
                        WorkerCall::Stat(p) => {
                            sb.stat(&p).expect("stat");
                        }
                    });
                }
                barrier.wait();
            });
        }
        barrier.wait();
        let t = Instant::now();
        barrier.wait();
        t.elapsed()
    });
    let ops = (threads * OPS_PER_THREAD) as f64;
    (ops / elapsed.as_secs_f64(), sb.retries())
}

/// Linha de base: cada thread com a sua própria sandbox (sem compartilhar nada mutável).
fn run_independent<F: Flavor>(image: &crate::fs::Fs<F>, threads: usize) -> f64 {
    let barrier = Barrier::new(threads + 1);
    let elapsed = std::thread::scope(|scope| {
        for t in 0..threads {
            let barrier = &barrier;
            scope.spawn(move || {
                let mut vfs = Vfs::from_image(image);
                let mut rng = Rng::new(t as u64 + 1);
                let buf = vec![t as u8; 4096];
                let mut k = 0;
                barrier.wait();
                let mut done = 0;
                while done < OPS_PER_THREAD {
                    done += worker_op(&mut rng, t, &mut k, &buf, |c| match c {
                        WorkerCall::Write(p, off, d) => {
                            vfs.write(&p, off, d).expect("escrita");
                        }
                        WorkerCall::Create(p) => {
                            vfs.create(&p, 0o644).expect("create");
                        }
                        WorkerCall::Stat(p) => {
                            vfs.stat(&p).expect("stat");
                        }
                    });
                }
                barrier.wait();
            });
        }
        barrier.wait();
        let t = Instant::now();
        barrier.wait();
        t.elapsed()
    });
    (threads * OPS_PER_THREAD) as f64 / elapsed.as_secs_f64()
}

pub const CONCURRENCY_REPS: usize = 5;

fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    xs[xs.len() / 2]
}

/// Vazão (operações por segundo) de cada estratégia com 1, 4 e 16 threads. As estratégias rodam
/// intercaladas, `CONCURRENCY_REPS` vezes cada, e vale a mediana: a máquina é compartilhada e a
/// carga muda durante a medição.
pub fn concurrency_timing<F: Flavor>() -> Value {
    let max_threads = *THREADS.last().expect("threads");
    let fs = concurrency_image::<F>(max_threads);
    let locked_img = LockedSandbox::<F>::image_from_fs(&fs);
    let rcu_img = RcuSandbox::<F>::image_from_fs(&fs);
    let sharded_img = ShardedSandbox::<F>::image_from_fs(&fs);
    let mut rows = Vec::new();
    for &threads in &THREADS {
        let (mut locked, mut rcu, mut sharded, mut independent) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut rcu_retries, mut sharded_retries) = (Vec::new(), Vec::new());
        for _ in 0..CONCURRENCY_REPS {
            locked.push(run_shared::<F, LockedSandbox<F>>(&locked_img, threads).0);
            let (ops, retries) = run_shared::<F, RcuSandbox<F>>(&rcu_img, threads);
            rcu.push(ops);
            rcu_retries.push(retries as f64);
            let (ops, retries) = run_shared::<F, ShardedSandbox<F>>(&sharded_img, threads);
            sharded.push(ops);
            sharded_retries.push(retries as f64);
            independent.push(run_independent::<F>(&fs, threads));
        }
        rows.push(json!({
            "threads": threads,
            "ops_per_s": {
                LockedSandbox::<F>::LABEL: median(locked),
                RcuSandbox::<F>::LABEL: median(rcu),
                ShardedSandbox::<F>::LABEL: median(sharded),
                "sandbox por thread (referência)": median(independent),
            },
            "retries_median": {
                RcuSandbox::<F>::LABEL: median(rcu_retries),
                ShardedSandbox::<F>::LABEL: median(sharded_retries),
            },
        }));
    }
    json!({"ops_per_thread": OPS_PER_THREAD, "repetitions": CONCURRENCY_REPS, "rows": rows})
}
