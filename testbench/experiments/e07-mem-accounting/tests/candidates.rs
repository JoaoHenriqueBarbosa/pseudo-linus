//! Testes de integração: rodam os binários de candidato como subprocesso (o allocator global é um por
//! binário, e as demonstrações abortam o processo).
//!
//! Regra da bancada: hipótese refutada é resultado, não teste quebrado. Então aqui só se verifica o que é
//! nosso: o encanamento (cada binário responde em JSON), que as cargas fazem o mesmo trabalho em todo
//! allocator, que a nossa tabela por grupo conta certo, que as demonstrações de abort são de fato abort,
//! e que a contabilidade explícita do kernel fecha em zero.

use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Output};

use serde_json::Value;

const BINS: &[(&str, &str)] = &[
    ("cand-system", env!("CARGO_BIN_EXE_cand-system")),
    ("cand-mimalloc", env!("CARGO_BIN_EXE_cand-mimalloc")),
    ("cand-tracking-allocator", env!("CARGO_BIN_EXE_cand-tracking-allocator")),
    ("cand-tracking-allocator-mimalloc", env!("CARGO_BIN_EXE_cand-tracking-allocator-mimalloc")),
    ("cand-alloc-track", env!("CARGO_BIN_EXE_cand-alloc-track")),
    ("cand-jqf-resource", env!("CARGO_BIN_EXE_cand-jqf-resource")),
    ("cand-alloc-count", env!("CARGO_BIN_EXE_cand-alloc-count")),
    ("cand-allocation-counter", env!("CARGO_BIN_EXE_cand-allocation-counter")),
    ("cand-jemalloc", env!("CARGO_BIN_EXE_cand-jemalloc")),
    ("cand-cap", env!("CARGO_BIN_EXE_cand-cap")),
    ("cand-stats-alloc", env!("CARGO_BIN_EXE_cand-stats-alloc")),
    ("cand-accounting-allocator", env!("CARGO_BIN_EXE_cand-accounting-allocator")),
];

fn exe(name: &str) -> &'static str {
    BINS.iter().find(|(n, _)| *n == name).map(|(_, p)| *p).expect("binário conhecido")
}

fn run(name: &str, args: &[&str]) -> (Output, Option<Value>) {
    let out = Command::new(exe(name)).args(args).output().expect("subprocesso");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let json = stdout.lines().rev().find(|l| l.starts_with(['{', '['])).and_then(|l| serde_json::from_str(l).ok());
    (out, json)
}

fn ok_json(name: &str, args: &[&str]) -> Value {
    let (out, json) = run(name, args);
    assert!(out.status.success(), "{name} {args:?}: {:?}\n{}", out.status, String::from_utf8_lossy(&out.stderr));
    json.unwrap_or_else(|| panic!("{name} {args:?} não imprimiu JSON"))
}

#[test]
fn every_candidate_answers_info() {
    for (name, _) in BINS {
        let j = ok_json(name, &["info"]);
        assert!(j["name"].is_string(), "{name}");
        assert!(j["caps"].is_object(), "{name}");
    }
}

#[test]
fn every_allocator_does_the_same_sort_and_ring_work() {
    let mut sums = Vec::new();
    let mut rings = Vec::new();
    for (name, _) in BINS {
        let j = ok_json(name, &["sort", "lines=20000", "seed=7"]);
        assert_eq!(j["timed"]["items"], 20000, "{name}");
        sums.push((name, j["timed"]["checksum"].as_u64().expect("checksum")));
        let j = ok_json(name, &["small", "ops=50000", "seed=7"]);
        rings.push((name, j["timed"]["checksum"].as_u64().expect("checksum")));
    }
    assert!(sums.windows(2).all(|w| w[0].1 == w[1].1), "{sums:?}");
    assert!(rings.windows(2).all(|w| w[0].1 == w[1].1), "{rings:?}");
}

#[test]
fn group_table_counts_controlled_cases_exactly() {
    for name in ["cand-tracking-allocator", "cand-tracking-allocator-mimalloc"] {
        let j = ok_json(name, &["controlled"]);
        for case in j.as_array().expect("lista de casos") {
            assert_eq!(case["error"], 0, "{name}: {case}");
        }
        let a = ok_json(name, &["attribution"]);
        assert_eq!(a["free_attributed_to_allocator"], true, "{name}: {a}");
        assert_eq!(a["realloc_moves_ownership"], true, "{name}: {a}");
        let e = ok_json(name, &["exit-residual"]);
        assert_eq!(e["remote_after_join"], 0, "{name}: {e}");
    }
}

#[test]
fn attribution_scenario_runs_for_every_accounting_candidate() {
    for (name, _) in BINS.iter().filter(|(n, _)| !matches!(*n, "cand-system" | "cand-mimalloc")) {
        let j = ok_json(name, &["attribution"]);
        let evidence = j["evidence"].as_str().expect("evidence");
        assert!(["self", "remote", "exit", "none"].contains(&evidence), "{name}: {j}");
        assert_eq!(j["payload_bytes"], 1_016_000, "{name}");
    }
}

#[test]
fn lockstep_protocol_answers_one_line_per_go() {
    use std::io::{BufRead, BufReader, Write};
    for (name, cmd) in [
        ("cand-system", "serve-sort"),
        ("cand-tracking-allocator", "serve-sort"),
        ("cand-tracking-allocator", "serve-sort-header-only"),
    ] {
        let mut child = Command::new(exe(name))
            .args([cmd, "lines=2000"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("subprocesso");
        let mut stdin = child.stdin.take().expect("stdin");
        let mut out = BufReader::new(child.stdout.take().expect("stdout"));
        let mut line = String::new();
        out.read_line(&mut line).expect("pronto");
        assert!(line.contains("ready"), "{name} {cmd}: {line}");
        let mut sums = Vec::new();
        for _ in 0..2 {
            stdin.write_all(b"go\n").expect("go");
            stdin.flush().expect("flush");
            line.clear();
            out.read_line(&mut line).expect("resposta");
            let j: Value = serde_json::from_str(line.trim()).expect("json");
            assert!(j["timed"]["cpu_ns"].as_u64().is_some_and(|c| c > 0), "{name} {cmd}: {j}");
            sums.push(j["timed"]["checksum"].as_u64().expect("checksum"));
        }
        assert_eq!(sums[0], sums[1]);
        drop(stdin);
        assert!(child.wait().expect("fim").success(), "{name} {cmd}");
    }
}

#[test]
fn limit_flag_is_seen_at_the_next_checkpoint() {
    let j = ok_json(
        "cand-tracking-allocator",
        &["overshoot", "detect=flag", "every=1", "chunk=4096", "limit=8388608", "reps=2"],
    );
    for r in j["runs"].as_array().expect("runs") {
        assert_eq!(r["detected"], true, "{r}");
        assert_eq!(r["overshoot_bytes"], 4096, "a flag é vista na alocação seguinte: {r}");
        assert_eq!(r["live_after_kill"], 0, "o kill libera tudo: {r}");
    }
}

fn assert_aborted(name: &str, args: &[&str], needle: &str) {
    let (out, json) = run(name, args);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.signal(), Some(6), "{name} {args:?} devia abortar com SIGABRT: {:?}\n{stderr}", out.status);
    assert!(json.is_none(), "{name} {args:?} não devia chegar a imprimir o resultado");
    assert!(stderr.contains("vizinhos vivos"), "os vizinhos estavam vivos antes do pedido: {stderr}");
    assert!(stderr.contains(needle), "{name} {args:?}: {stderr}");
}

fn assert_survived(name: &str, args: &[&str]) -> Value {
    let j = ok_json(name, args);
    assert_eq!(j["survived"], true, "{name} {args:?}: {j}");
    j
}

#[test]
fn hard_limit_inside_the_allocator_aborts_the_whole_host() {
    assert_aborted("cand-cap", &["hard-limit"], "memory allocation of 67108864 bytes failed");
    let j = assert_survived("cand-cap", &["hard-limit-try"]);
    assert!(j["outcome"]["err"].is_string(), "try_reserve devolve erro em vez de abortar: {j}");
}

#[test]
fn huge_request_aborts_even_without_any_limit() {
    assert_aborted("cand-system", &["huge-alloc"], "memory allocation of 35184372088832 bytes failed");
    let j = assert_survived("cand-system", &["huge-alloc-try"]);
    assert!(j["outcome"]["err"].is_string(), "{j}");
}

#[test]
fn jqf_soft_ceiling_survives_small_and_aborts_past_the_slab() {
    let j = assert_survived("cand-jqf-resource", &["past-ceiling", "ask=4096"]);
    assert!(j["outcome"]["err"].as_str().is_some_and(|e| e.contains("checkpoint recusou")), "{j}");
    assert_aborted("cand-jqf-resource", &["past-ceiling", "ask=4194304"], "memory allocation of 4194304 bytes failed");
}

#[test]
fn explicit_kernel_accounting_balances_to_zero() {
    for name in ["cand-system", "cand-tracking-allocator"] {
        let j = ok_json(name, &["kernel-acct", "reps=1", "pipe_mib=8", "file_mib=8"]);
        for kind in ["pipe_single_cpu", "pipe_two_threads_wall", "file_cpu"] {
            for row in j[kind].as_array().expect("variantes") {
                assert_eq!(row["all_balanced"], true, "{name} {kind}: {row}");
            }
        }
    }
}
