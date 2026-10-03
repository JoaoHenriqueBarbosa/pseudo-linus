//! F15/F16/F17: shell (brush), edição de linha (termwiz, noline) e interpretadores embutidos, mais as
//! linhas de base (bashkit e afins). Responde H37, H38, H39 e H40 e grava
//! `results/f15-shell-tty-interp.json`.
//!
//! Uso:
//!
//! ```sh
//! cargo run --release --manifest-path experiments/f15-shell-tty-interp/Cargo.toml
//! cargo run --release ... -- --only h37,h38     # só algumas partes (grava em target/f15-cache)
//! cargo run --release ... -- --fresh            # ignora o cache do bash -n
//! ```
//!
//! O binário compila sozinho os crates auxiliares (interpretadores, linhas de base, sonda de tty) numa
//! invocação do cargo, roda tudo e grava o resultado.

mod baseline;
mod common;
mod evidence;
mod interp;
mod lineedit;
mod parse;

use std::collections::BTreeSet;
use std::time::Instant;

use anyhow::Result;
use harness::{ExperimentResult, Verdict};
use serde_json::json;

use common::Section;

const TITLE: &str = "Shell (brush), edição de linha sobre pty em memória, interpretadores embutidos e linhas de base";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("line-edit-child") {
        return lineedit::child_main();
    }
    let fresh = args.iter().any(|a| a == "--fresh");
    let only: Option<BTreeSet<String>> = args
        .iter()
        .position(|a| a == "--only")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.split(',').map(|p| p.trim().to_lowercase()).collect());
    let wanted = |h: &str| only.as_ref().is_none_or(|set| set.contains(h));

    let started = Instant::now();
    let mut result = ExperimentResult::new(common::EXPERIMENT, TITLE);
    let mut timings = serde_json::Map::new();
    let mut evidence = serde_json::Map::new();

    type Part = (&'static str, &'static str, Box<dyn Fn() -> Result<Section>>);
    let parts: Vec<Part> = vec![
        ("h37", "H37", Box::new(move || parse::run(fresh))),
        ("h38", "H38", Box::new(lineedit::run)),
        ("h39", "H39", Box::new(interp::run)),
        ("h40", "H40", Box::new(baseline::run)),
    ];
    for (key, id, run) in parts {
        if !wanted(key) {
            continue;
        }
        let t = Instant::now();
        eprintln!("[{id}] rodando...");
        match run() {
            Ok(section) => {
                eprintln!("[{id}] {:?}: {}", section.verdict, section.summary);
                result.hypothesis(section.hypothesis, section.verdict, section.summary, section.evidence.clone());
                evidence.insert(id.to_string(), section.evidence);
                result.candidates.extend(section.candidates);
                result.notes.extend(section.notes);
            }
            Err(e) => {
                let msg = format!("{e:#}");
                eprintln!("[{id}] ERRO: {msg}");
                result.hypothesis(
                    id,
                    Verdict::Inconclusive,
                    format!("Parte do experimento falhou ao rodar: {}", msg.chars().take(400).collect::<String>()),
                    json!({ "error": msg }),
                );
            }
        }
        timings.insert(id.to_string(), json!(t.elapsed().as_secs_f64()));
    }
    timings.insert("total".into(), json!(started.elapsed().as_secs_f64()));
    result.metrics = json!({ "timings_s": timings, "by_hypothesis": evidence });

    if let Some(set) = &only {
        // Um arquivo por combinação de partes, pra execuções parciais em paralelo não se atropelarem.
        let tag = set.iter().cloned().collect::<Vec<_>>().join("-");
        let path = common::cache_dir().join(format!("partial-{tag}.json"));
        std::fs::write(&path, serde_json::to_string_pretty(&result)? + "\n")?;
        eprintln!("resultado parcial em {}", path.display());
    } else {
        let path = result.write()?;
        eprintln!("resultado em {}", path.display());
    }
    eprintln!("tempo total: {:.1}s", started.elapsed().as_secs_f64());
    Ok(())
}
