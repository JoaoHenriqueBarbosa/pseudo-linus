//! H01: migração de corrotina stackful entre threads em Rust seguro.
//!
//! Roda `cargo check` em cada sonda de `probes/h01` (crate com `forbid(unsafe_code)`) e registra se
//! compilou e com que códigos de erro; depois roda a sonda do `generator`, que compila, pra medir se a
//! migração que ela permite é sound.

use std::path::PathBuf;
use std::process::Command;

use harness::Verdict;
use serde::Serialize;
use serde_json::{Value, json};

use super::{HypOut, progress};

#[derive(Clone, Copy)]
enum Expect {
    Compiles,
    Fails(&'static str),
}

const PROBES: &[(&str, Expect, &str)] = &[
    ("corosensei_same_thread", Expect::Compiles, "controle: corrotina usada só na thread de origem"),
    ("corosensei_move_thread", Expect::Fails("E0277"), "mover Coroutine suspensa pra outra thread"),
    ("corosensei_shared_mutex", Expect::Fails("E0277"), "Arc<Mutex<Coroutine>> compartilhado entre threads"),
    ("corosensei_unsafe_send", Expect::Fails("unsafe_code"), "embrulho com unsafe impl Send"),
    ("may_spawn_safe", Expect::Fails("E0133"), "may::coroutine::spawn sem unsafe"),
    ("may_spawn_unsafe_block", Expect::Fails("unsafe_code"), "may::coroutine::spawn em bloco unsafe"),
    ("generator_migrate", Expect::Compiles, "generator 0.8 migrando entre threads"),
];

#[derive(Serialize)]
struct ProbeResult {
    probe: String,
    description: String,
    compiled: bool,
    error_codes: Vec<String>,
    expected: String,
    as_expected: bool,
}

fn probe_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("probes/h01")
}

fn probe_target() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/h01-probe")
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

fn check(bin: &str) -> anyhow::Result<(bool, Vec<String>)> {
    let out = Command::new(cargo())
        .args(["check", "--release", "--message-format=json", "--bin", bin, "--manifest-path"])
        .arg(probe_dir().join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", probe_target())
        .output()?;
    let mut codes = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v["reason"] != "compiler-message" || v["message"]["level"] != "error" {
            continue;
        }
        let code = v["message"]["code"]["code"].as_str().unwrap_or("sem-código").to_string();
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    if !out.status.success() && codes.is_empty() {
        anyhow::bail!("cargo check falhou sem erro de compilação: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok((out.status.success(), codes))
}

fn run_generator() -> anyhow::Result<Value> {
    let out = Command::new(cargo())
        .args(["run", "--release", "--quiet", "--bin", "generator_migrate", "--manifest-path"])
        .arg(probe_dir().join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", probe_target())
        .output()?;
    if !out.status.success() {
        anyhow::bail!("sonda do generator falhou: {}", String::from_utf8_lossy(&out.stderr));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().last().unwrap_or_default();
    Ok(serde_json::from_str(line)?)
}

pub fn run() -> HypOut {
    let mut results = Vec::new();
    let mut infra_error = None;
    for &(bin, expect, desc) in PROBES {
        progress(format!("H01: cargo check {bin}"));
        match check(bin) {
            Ok((compiled, codes)) => {
                let (expected, ok) = match expect {
                    Expect::Compiles => ("compila".to_string(), compiled),
                    Expect::Fails(code) => (format!("falha com {code}"), !compiled && codes.iter().any(|c| c == code)),
                };
                results.push(ProbeResult {
                    probe: bin.to_string(),
                    description: desc.to_string(),
                    compiled,
                    error_codes: codes,
                    expected,
                    as_expected: ok,
                });
            }
            Err(e) => {
                infra_error = Some(format!("{bin}: {e}"));
                break;
            }
        }
    }
    let generator = if infra_error.is_none() {
        progress("H01: cargo run generator_migrate");
        match run_generator() {
            Ok(v) => Some(v),
            Err(e) => {
                infra_error = Some(e.to_string());
                None
            }
        }
    } else {
        None
    };

    if let Some(err) = infra_error {
        return HypOut {
            id: "H01",
            verdict: Verdict::Inconclusive,
            summary: format!("As sondas de compilação não rodaram: {err}"),
            evidence: json!({ "probes": results }),
        };
    }

    let gen_v = generator.unwrap_or(Value::Null);
    let addr = |k: &str| gen_v[k].as_u64().unwrap_or(0);
    let tls1 = addr("thread1_tls");
    let tls2 = addr("thread2_tls");
    let held2 = addr("held_on_thread2");
    let inline2 = addr("inline_access_on_thread2");
    let migrated = gen_v["coroutine_thread_before"] != gen_v["coroutine_thread_after"];
    // Unsound: depois de migrar, a corrotina roda na thread 2 usando o thread-local da thread 1.
    let generator_unsound = migrated && tls1 != tls2 && held2 == tls1;
    let inline_stale = migrated && inline2 == tls1 && tls1 != tls2;
    let negatives_ok = results.iter().all(|r| r.as_expected);
    let corosensei_blocked = results
        .iter()
        .filter(|r| r.probe.starts_with("corosensei_") && r.probe != "corosensei_same_thread")
        .all(|r| !r.compiled);
    let may_blocked = results.iter().filter(|r| r.probe.starts_with("may_")).all(|r| !r.compiled);

    let (verdict, summary) = if negatives_ok && corosensei_blocked && may_blocked && generator_unsound {
        (
            Verdict::Refuted,
            format!(
                "Nenhuma crate dá migração sound sem unsafe nosso: corosensei falha com E0277 (Coroutine !Send) e com \
                 o lint unsafe_code no embrulho com unsafe impl Send; may::coroutine::spawn exige unsafe (E0133, e o \
                 bloco unsafe é barrado pelo forbid); o generator 0.8 compila a migração mas ela é unsound: depois de \
                 migrar, a corrotina roda na thread 2 usando o thread-local da thread 1 ({held2:#x}; o da thread 2 é \
                 {tls2:#x}){}.",
                if inline_stale {
                    ", e até um acesso novo ao thread-local, inlinado, devolve o endereço da thread 1 porque o \
                     compilador reaproveita o endereço calculado antes da troca"
                } else {
                    ""
                }
            ),
        )
    } else if !corosensei_blocked || !may_blocked {
        (
            Verdict::Confirmed,
            "Alguma sonda de migração compilou sem unsafe nosso; ver evidência.".to_string(),
        )
    } else {
        (
            Verdict::Inconclusive,
            "As sondas não se comportaram como o esperado (ver evidência).".to_string(),
        )
    };
    HypOut {
        id: "H01",
        verdict,
        summary,
        evidence: json!({
            "probes": results,
            "generator_migration": gen_v,
            "generator_unsound": generator_unsound,
            "generator_inline_tls_access_stale": inline_stale,
        }),
    }
}
