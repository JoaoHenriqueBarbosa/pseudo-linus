//! Testes de semântica do mini-kernel nos três modelos e versões reduzidas das medições. Nenhum teste
//! depende de tempo: só de status, contagens e bytes.
#![forbid(unsafe_code)]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use e01_exec_models::experiments::{h01, h02, h03_h04, h06, h07, h08, h09, h10};
use e01_exec_models::kernel::{ExitStatus, File, SIGPIPE, new_pipe};
use e01_exec_models::sys::Sys;
use e01_exec_models::vm::{MODE_BACKEDGE, MODE_EVERY, MODE_NONE, expected, program, run, run_dyn};
use e01_exec_models::workloads::{WcOut, asyncs, sync};
use e01_exec_models::{ModelKind, ModelSpec};
use harness::Verdict;

const MODELS: [ModelSpec; 4] = [ModelSpec::A, ModelSpec::ASPIN, ModelSpec::B64, ModelSpec::C];

/// Escritor manda `total` bytes e sai; leitor conta até EOF.
fn eof_case(spec: ModelSpec, ncpus: usize, total: u64) -> (ExitStatus, ExitStatus, u64) {
    let (r, w) = new_pipe();
    let out = Arc::new(WcOut::default());
    let sink = File::Sink(Arc::new(AtomicU64::new(0)));
    if spec.kind == ModelKind::C {
        let k = spec.c_kernel(ncpus, true);
        let o2 = out.clone();
        let g = k.spawn(vec![(1, w)], move |c| asyncs::gen_text(c, total));
        let c = k.spawn(vec![(0, r), (1, sink)], move |c| asyncs::wc(c, o2, 4096));
        (k.wait(g), k.wait(c), out.bytes.load(Ordering::Relaxed))
    } else {
        let k = spec.sync_kernel(ncpus, true).unwrap();
        let o2 = out.clone();
        let g = k.spawn(vec![(1, w)], Box::new(move |s: &dyn Sys| sync::gen_text(s, total)));
        let c = k.spawn(vec![(0, r), (1, sink)], Box::new(move |s: &dyn Sys| sync::wc(s, &o2, 4096)));
        (k.wait(g), k.wait(c), out.bytes.load(Ordering::Relaxed))
    }
}

#[test]
fn pipe_delivers_everything_then_eof() {
    for spec in MODELS {
        for ncpus in [1, 3] {
            let total = 1_000_003;
            let (g, c, bytes) = eof_case(spec, ncpus, total);
            assert_eq!(g, ExitStatus::Exited(0), "{}", spec.label());
            assert_eq!(c, ExitStatus::Exited(0), "{}", spec.label());
            assert_eq!(bytes, total, "{} com {ncpus} CPUs", spec.label());
        }
    }
}

#[test]
fn write_without_reader_is_sigpipe() {
    for spec in MODELS {
        let (r, w) = new_pipe();
        drop(r);
        let st = if spec.kind == ModelKind::C {
            let k = spec.c_kernel(1, false);
            let p = k.spawn(vec![(1, w)], |c| async move {
                let _ = c.write(1, b"x").await;
                0
            });
            k.wait(p)
        } else {
            let k = spec.sync_kernel(1, false).unwrap();
            let p = k.spawn(
                vec![(1, w)],
                Box::new(|s: &dyn Sys| {
                    let _ = s.write(1, b"x");
                    0
                }),
            );
            k.wait(p)
        };
        assert_eq!(st, ExitStatus::Signaled(SIGPIPE), "{}", spec.label());
    }
}

#[test]
fn wait_from_inside_a_process() {
    for spec in MODELS {
        let got = Arc::new(AtomicU64::new(0));
        let g2 = got.clone();
        if spec.kind == ModelKind::C {
            let k = spec.c_kernel(2, true);
            let p = k.spawn(Vec::new(), move |c| async move {
                let child = c.spawn(&[], |_c| async { 7 }).unwrap();
                if let Ok(ExitStatus::Exited(code)) = c.wait(child).await {
                    g2.store(code as u64, Ordering::Relaxed);
                }
                0
            });
            assert_eq!(k.wait(p), ExitStatus::Exited(0));
        } else {
            let k = spec.sync_kernel(2, true).unwrap();
            let p = k.spawn(
                Vec::new(),
                Box::new(move |s: &dyn Sys| {
                    let child = s.spawn(&[], Box::new(|_s: &dyn Sys| 7)).unwrap();
                    if let Ok(ExitStatus::Exited(code)) = s.wait(child) {
                        g2.store(code as u64, Ordering::Relaxed);
                    }
                    0
                }),
            );
            assert_eq!(k.wait(p), ExitStatus::Exited(0));
        }
        assert_eq!(got.load(Ordering::Relaxed), 7, "{}", spec.label());
    }
}

struct Bump(Arc<AtomicU64>);

impl Drop for Bump {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn deep_exit_sync(s: &dyn Sys, depth: u32, drops: &Arc<AtomicU64>) -> i32 {
    let _b = Bump(drops.clone());
    if depth == 0 {
        s.exit(42);
    }
    deep_exit_sync(s, depth - 1, drops)
}

#[test]
fn exit_from_deep_stack_runs_drops() {
    for spec in MODELS {
        let drops = Arc::new(AtomicU64::new(0));
        let d2 = drops.clone();
        let st = if spec.kind == ModelKind::C {
            let k = spec.c_kernel(1, false);
            let p = k.spawn(Vec::new(), move |c| async move {
                let _outer = Bump(d2.clone());
                c.yield_now().await;
                let _inner = Bump(d2);
                c.exit(42)
            });
            k.wait(p)
        } else {
            let k = spec.sync_kernel(1, false).unwrap();
            let p = k.spawn(Vec::new(), Box::new(move |s: &dyn Sys| deep_exit_sync(s, 9, &d2)));
            k.wait(p)
        };
        assert_eq!(st, ExitStatus::Exited(42), "{}", spec.label());
        let expected_drops = if spec.kind == ModelKind::C { 2 } else { 10 };
        assert_eq!(drops.load(Ordering::Relaxed), expected_drops, "{}", spec.label());
    }
}

#[test]
fn pipelines_produce_expected_output() {
    let sz = h02::Sizes {
        yes_lines: 300_000,
        bulk_bytes: 3 << 20,
        record_bytes: 256 << 10,
        record_size: 64,
        yield_iters_a: 100,
        yield_iters_fast: 100,
        pipe_iters_a: 100,
        pipe_iters_fast: 100,
        reps: 1,
    };
    for spec in ModelSpec::PERF {
        for ncpus in [1, 4] {
            for w in ["yes_head", "bulk4", "records4"] {
                let row = h02::quick_pipeline(spec, ncpus, w, sz);
                assert!(row.verified, "{} {w} {ncpus}: {:?}", spec.label(), row.statuses);
            }
        }
    }
}

#[test]
fn yield_pingpong_switches() {
    for spec in ModelSpec::PERF {
        let ns = h02::quick_yield_pingpong(spec, 200);
        assert!(ns > 0.0 && ns.is_finite(), "{}", spec.label());
    }
}

#[test]
fn interpreter_is_correct_in_all_modes() {
    let flag = std::sync::atomic::AtomicBool::new(true);
    let prog = program(10_000);
    let mut slow_calls = 0u64;
    let mut slow = || {
        slow_calls += 1;
    };
    assert_eq!(run::<MODE_NONE>(&prog, &flag, &mut slow), expected(10_000));
    assert_eq!(run::<MODE_EVERY>(&prog, &flag, &mut slow), expected(10_000));
    assert_eq!(run::<MODE_BACKEDGE>(&prog, &flag, &mut slow), expected(10_000));
    for mode in [MODE_NONE, MODE_EVERY, MODE_BACKEDGE] {
        assert_eq!(run_dyn(&prog, &flag, &mut slow, mode), expected(10_000));
    }
    assert!(slow_calls > 0);
}

#[test]
fn idle_processes_scale_small() {
    for spec in h03_h04::SCALE_MODELS {
        let rep = h03_h04::scale_child(spec, 200);
        assert!(rep.ok, "{}: {:?}", spec.label(), rep.error);
        assert_eq!(rep.exited_ok, 200);
    }
}

#[test]
fn spawn_latency_runs() {
    for spec in ModelSpec::PERF {
        let s = h03_h04::spawn_latency(spec, 50);
        assert_eq!(s.len(), 50, "{}", spec.label());
    }
}

#[test]
fn kill_runs_drops_and_closes_fds() {
    let rows = h06::measure(5);
    let v = h06::verdict(&rows);
    assert_eq!(v.verdict, Verdict::Confirmed, "{}", v.summary);
}

#[test]
fn loop_without_checkpoint_does_not_yield() {
    let d = h07::measure(std::time::Duration::from_millis(100));
    for r in &d.demos {
        assert_eq!(r.neighbor_progress_during_spin, 0, "{}", r.model);
        assert_eq!(r.killed_status, "signal 9", "{}", r.model);
        assert!(r.neighbor_progress_after > 0, "{}", r.model);
    }
}

#[test]
fn panic_is_isolated() {
    let rows = h09::measure();
    let v = h09::verdict(&rows);
    assert_eq!(v.verdict, Verdict::Confirmed, "{}", v.summary);
    for r in rows.iter().filter(|r| r.variant == "std_unwrap") {
        assert!(r.std_mutex_poisoned);
        assert_eq!(r.others_ok, 0, "lock().unwrap() num Mutex envenenado tem que falhar");
    }
}

#[test]
fn thread_local_per_model() {
    let d = h10::measure(1_000, 1);
    let get = |c: &str| d.rows.iter().find(|r| r.case == c).unwrap();
    assert_eq!(get("A").mismatches, 0);
    assert!(get("B_naive").mismatches > 0);
    assert_eq!(get("B_worker_swap").mismatches, 0);
    assert_eq!(get("C_task_local").mismatches, 0);
    assert_eq!(d.refcell_conflicts_a, 0);
    assert!(d.refcell_conflicts_b > 0);
}

/// Roda as sondas de `probes/h01` com `cargo check` e confere os códigos de erro exatos (o que o
/// rustdoc estável não faz nos doctests `compile_fail`).
#[test]
fn h01_probes_fail_with_expected_codes() {
    let v = h01::run();
    assert_eq!(v.verdict, Verdict::Refuted, "{}", v.summary);
    let probes = v.evidence["probes"].as_array().unwrap();
    assert!(probes.iter().all(|p| p["as_expected"] == true), "{probes:?}");
}

#[test]
fn stack_overflow_kills_host_and_stacker_depends_on_model() {
    let exe = Path::new(env!("CARGO_BIN_EXE_e01-exec-models"));
    let plain = h08::run_child(exe, "A-plain");
    assert!(!plain.host_survived && plain.signal.is_some(), "{plain:?}");
    let a = h08::run_child(exe, "A-stacker");
    assert!(a.host_survived, "{a:?}");
    assert_eq!(a.child.as_ref().unwrap().statuses, vec!["exit 3".to_string()]);
    let a_rem = a.remaining_at_entry.unwrap();
    assert!(a_rem > 0 && a_rem <= 256 * 1024, "na thread do A o stacker vê a pilha certa: {a_rem}");
    let c = h08::run_child(exe, "C-stacker");
    assert!(c.host_survived, "{c:?}");
    // Na corrotina o stacker nunca vê a pilha da corrotina: ou acha que não há nada (0, e cresce na
    // primeira chamada) ou acha que há mais do que a pilha real. Se derruba ou não o host depende do
    // layout de endereços, então o teste só confere o que ele vê.
    for v in ["B64-stacker", "B256-stacker"] {
        let r = h08::run_child(exe, v);
        let rem = r.remaining_at_entry.expect("o filho escreve o valor no stderr antes de recursar");
        let real = r.child.as_ref().map(|c| c.real_stack_bytes).unwrap_or(if v.starts_with("B64") { 65536 } else { 262144 });
        assert!(rem == 0 || rem > real, "{v}: stacker viu {rem} bytes numa pilha de {real}");
    }
}
