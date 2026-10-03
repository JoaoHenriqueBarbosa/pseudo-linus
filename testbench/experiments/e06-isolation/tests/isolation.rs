//! Testes do próprio experimento: só falham por bug nosso (sonda, parser, subprocesso), nunca porque
//! uma hipótese foi refutada. Os controles de cada modo do subprocesso precisam passar; o resultado
//! das sondas de critério vai pro JSON, não pra cá.

use std::path::Path;

use e06_isolation::child::{BENCH_CONFIGS, BenchReport, LandlockReport, SeccompReport};
use e06_isolation::h21::run_child;
use e06_isolation::probe::{Expect, Role, mismatches};

const EXE: &str = env!("CARGO_BIN_EXE_e06-isolation");

#[test]
fn landlock_child_reports_and_unrestricted_threads_stay_free() {
    let r: LandlockReport = run_child(Path::new(EXE), &["landlock"]).expect("subprocesso landlock");
    assert!(r.applied.is_some() || r.restrict_error.is_some());
    // Controle: a thread principal do subprocesso nunca é restrita, então lê tudo.
    let main: Vec<_> = r.probes.iter().filter(|p| p.thread == "main").collect();
    assert!(!main.is_empty());
    assert!(main.iter().all(|p| p.result.ok), "{main:?}");
    // Coerência das sondas: toda expectativa é Allowed ou um errno conhecido.
    for p in &r.probes {
        if let Expect::Errno(code) = p.expect {
            assert_eq!(code, libc::EACCES, "{p:?}");
        }
    }
}

#[test]
fn seccomp_child_reports_and_controls_hold() {
    let r: SeccompReport = run_child(Path::new(EXE), &["seccomp"]).expect("subprocesso seccomp");
    assert!(r.deny_instructions > 0 && r.clone3_instructions > 0);
    // Controle: leitura de arquivo continua funcionando em qualquer thread, filtrada ou não.
    assert!(mismatches(&r.probes, Role::Control).is_empty(), "{:?}", mismatches(&r.probes, Role::Control));
    // Controle: a thread principal cria socket e processo.
    assert!(r.probes.iter().filter(|p| p.thread == "main").all(|p| p.result.ok));
}

#[test]
fn bench_child_quick_produces_numbers_for_every_config() {
    let r: BenchReport = run_child(Path::new(EXE), &["bench", "quick"]).expect("subprocesso bench");
    assert_eq!(r.configs.len(), BENCH_CONFIGS.len());
    for c in &r.configs {
        assert!(c.getppid_ns.median > 0.0, "{c:?}");
        assert!(c.pread_1b_ns.median > 0.0, "{c:?}");
        assert!(c.open_close_ns.median > 0.0, "{c:?}");
        assert!(c.spawn_restrict_join_us.median > 0.0, "{c:?}");
    }
    assert!(r.inherited_spawn_us.median > 0.0 && r.ruleset_build_us.median > 0.0);
    // Controle: cada configuração estava ativa durante a medição, e a filha da spawner herdou as duas.
    assert!(mismatches(&r.sanity, Role::Control).is_empty(), "{:?}", mismatches(&r.sanity, Role::Control));
    assert!(mismatches(&r.inherited_sanity, Role::Control).is_empty(), "{:?}", r.inherited_sanity);
}

#[test]
fn unknown_child_mode_fails() {
    let r: anyhow::Result<serde_json::Value> = run_child(Path::new(EXE), &["nope"]);
    assert!(r.is_err());
}
