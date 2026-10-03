//! `cargo run -p oracle -- build`: monta a imagem do oráculo e compila o `oracle-agent`.
//! `cargo run -p oracle -- gen [--tool grep]`: roda os casos do corpus no oráculo e grava o golden.
//! `cargo run -p oracle -- check`: confere se todo arquivo de casos tem golden atualizado.

use std::collections::BTreeMap;
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use harness::{CaseFile, Oracle, Outcome, paths};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Monta a imagem Docker e compila o oracle-agent em release.
    Build,
    /// Gera golden pros casos do corpus.
    Gen {
        /// Só esta ferramenta (diretório em corpus/cases).
        #[arg(long)]
        tool: Option<String>,
        /// Só regera arquivos cujo golden não existe ou está desatualizado em relação aos casos.
        #[arg(long)]
        missing_only: bool,
    },
    /// Lista versões dos pacotes do oráculo.
    Versions,
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Build => build(),
        Cmd::Gen { tool, missing_only } => generate(tool, missing_only),
        Cmd::Versions => {
            println!("{}", versions()?);
            Ok(())
        }
    }
}

fn build() -> Result<()> {
    let tag = Oracle::image_tag()?;
    let dir = Oracle::dockerfile_dir();
    let status = Command::new("docker").args(["build", "-t", &tag]).arg(&dir).status()?;
    if !status.success() {
        bail!("docker build falhou");
    }
    let status = Command::new("cargo")
        .args(["build", "--release", "-p", "oracle-agent", "--target-dir"])
        .arg(paths::root().join("target"))
        .current_dir(paths::root())
        .status()?;
    if !status.success() {
        bail!("cargo build do oracle-agent falhou");
    }
    println!("imagem {tag}, agente {}", Oracle::agent_path().display());
    Ok(())
}

fn versions() -> Result<String> {
    let tag = Oracle::image_tag()?;
    let out = Command::new("docker")
        .args(["run", "--rm", &tag, "bash", "-c", "dpkg-query -W -f '${Package}\\t${Version}\\n' | sort"])
        .output()?;
    if !out.status.success() {
        bail!("dpkg-query falhou: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8(out.stdout)?)
}

fn generate(only: Option<String>, missing_only: bool) -> Result<()> {
    let oracle = Oracle::locate()?;
    let tools = match only {
        Some(t) => vec![t],
        None => paths::tools()?,
    };
    std::fs::create_dir_all(paths::root().join("golden"))?;
    std::fs::write(paths::root().join("golden/oracle-versions.txt"), versions()?)?;
    for tool in tools {
        for case_path in paths::case_files(&tool)? {
            let golden_path = paths::golden_for(&case_path);
            if missing_only && is_fresh(&case_path, &golden_path)? {
                continue;
            }
            let file = CaseFile::load(&case_path)?;
            let started = std::time::Instant::now();
            let outcomes = oracle
                .run(&file.cases)
                .with_context(|| format!("rodando {}", case_path.display()))?;
            let mut golden: BTreeMap<String, Outcome> = BTreeMap::new();
            let mut broken = 0;
            for (case, outcome) in file.cases.iter().zip(outcomes) {
                if let Some(why) = &outcome.unsupported {
                    eprintln!("  AVISO {}: {why}", case.id);
                    broken += 1;
                }
                if outcome.timed_out {
                    eprintln!("  AVISO {}: timeout no oráculo", case.id);
                }
                golden.insert(case.id.clone(), outcome);
            }
            std::fs::create_dir_all(golden_path.parent().expect("pai"))?;
            std::fs::write(&golden_path, serde_json::to_string_pretty(&golden)? + "\n")?;
            println!(
                "{tool}/{}: {} casos, {broken} com erro do agente, {:.1}s",
                case_path.file_name().expect("nome").to_string_lossy(),
                golden.len(),
                started.elapsed().as_secs_f64()
            );
        }
    }
    Ok(())
}

/// Golden existe, é mais novo que o arquivo de casos e tem todos os ids.
fn is_fresh(case_path: &std::path::Path, golden_path: &std::path::Path) -> Result<bool> {
    if !golden_path.exists() {
        return Ok(false);
    }
    let case_m = std::fs::metadata(case_path)?.modified()?;
    let golden_m = std::fs::metadata(golden_path)?.modified()?;
    if golden_m < case_m {
        return Ok(false);
    }
    let file = CaseFile::load(case_path)?;
    let golden: BTreeMap<String, serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(golden_path)?)?;
    Ok(file.cases.iter().all(|c| golden.contains_key(&c.id)))
}
