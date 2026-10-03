//! Peças compartilhadas pelas quatro partes do experimento (H37 a H40).

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use harness::{CandidateResult, Verdict};

/// Id do experimento (nome do diretório e do `results/<id>.json`).
pub const EXPERIMENT: &str = "f15-shell-tty-interp";

/// Diretório do experimento (`testbench/experiments/f15-shell-tty-interp`).
pub fn exp_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn manifest() -> PathBuf {
    exp_dir().join("Cargo.toml")
}

/// Target do workspace do experimento.
pub fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| exp_dir().join("target"))
}

/// Cache gitignored (fica dentro de `target/`): downloads e resultados intermediários.
pub fn cache_dir() -> PathBuf {
    let dir = target_dir().join("f15-cache");
    std::fs::create_dir_all(&dir).expect("criar cache");
    dir
}

/// Binário release de um membro do workspace.
pub fn bin_path(name: &str) -> PathBuf {
    target_dir().join("release").join(name)
}

/// Compila em release os pacotes pedidos numa invocação só do cargo (paraleliza entre crates).
/// Falha de um pacote é resultado, não erro: devolve o stderr pra registrar.
pub fn build_packages(packages: &[&str]) -> Result<Result<(), String>> {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.arg("build").arg("--release").arg("--manifest-path").arg(manifest());
    for p in packages {
        cmd.arg("-p").arg(p);
    }
    let out = cmd.output().context("rodar cargo build")?;
    if out.status.success() {
        Ok(Ok(()))
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(40).collect();
        Ok(Err(tail.into_iter().rev().collect::<Vec<_>>().join("\n")))
    }
}

/// Resultado de uma parte do experimento, que o `main` junta no `ExperimentResult`.
pub struct Section {
    pub hypothesis: &'static str,
    pub verdict: Verdict,
    pub summary: String,
    pub evidence: serde_json::Value,
    pub candidates: Vec<CandidateResult>,
    pub notes: Vec<String>,
}

/// Roda um comando e devolve stdout; erro se sair com código diferente de zero.
pub fn run_cmd(cmd: &mut Command) -> Result<String> {
    let out = cmd.output().with_context(|| format!("rodar {cmd:?}"))?;
    if !out.status.success() {
        bail!("{cmd:?} saiu com {}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Mediana de uma lista (ordena uma cópia).
pub fn median(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}
