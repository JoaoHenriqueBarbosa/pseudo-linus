//! `cargo run -p runner` (ou `runner report`): valida `results/` contra `hypotheses.toml` e escreve
//! `../docs/bench-report.md`. Sai com erro se alguma hipótese ficar sem veredito.
//!
//! `cargo run -p runner -- run [--only e01-exec-models,f04-awk-jq]`: roda os experimentos em sequência
//! (cada um no seu workspace, em release, com a saída aparecendo ao vivo) e depois escreve o relatório.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use harness::{ExperimentResult, Fit, paths};
use serde::Deserialize;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Só agrega results/ e escreve o relatório.
    Report,
    /// Roda os experimentos (todos ou os de --only) e depois escreve o relatório.
    Run {
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
    },
}

#[derive(Deserialize)]
struct Registry {
    hypothesis: Vec<Hypothesis>,
}

#[derive(Deserialize)]
struct Hypothesis {
    id: String,
    source: String,
    experiment: String,
    statement: String,
    criterion: String,
}

fn main() -> Result<()> {
    match Cli::parse().cmd.unwrap_or(Cmd::Report) {
        Cmd::Report => report(),
        Cmd::Run { only } => {
            run(&only)?;
            report()
        }
    }
}

fn experiments() -> Result<Vec<String>> {
    let dir = paths::root().join("experiments");
    let mut out: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("Cargo.toml").exists())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    Ok(out)
}

fn run(only: &[String]) -> Result<()> {
    let mut failed = Vec::new();
    for exp in experiments()? {
        if !only.is_empty() && !only.contains(&exp) {
            continue;
        }
        let manifest = paths::root().join("experiments").join(&exp).join("Cargo.toml");
        println!("\n=== {exp} ===");
        let started = std::time::Instant::now();
        let status = Command::new("cargo")
            .args(["run", "--release", "--manifest-path"])
            .arg(&manifest)
            .status()
            .with_context(|| format!("cargo run {exp}"))?;
        println!("=== {exp}: {} em {:.0}s ===", if status.success() { "ok" } else { "FALHOU" }, started.elapsed().as_secs_f64());
        if !status.success() {
            failed.push(exp);
        }
    }
    if !failed.is_empty() {
        eprintln!("experimentos que falharam: {}", failed.join(", "));
    }
    Ok(())
}

fn fit_pt(fit: Fit) -> &'static str {
    match fit {
        Fit::Fits => "encaixa",
        Fit::FitsWithWork => "encaixa com trabalho",
        Fit::DoesNotFit => "não encaixa",
        Fit::Reference => "referência",
    }
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn pct(a: usize, b: usize) -> String {
    if b == 0 { "-".into() } else { format!("{:.1}% ({a}/{b})", 100.0 * a as f64 / b as f64) }
}

fn report() -> Result<()> {
    let registry: Registry = toml::from_str(&std::fs::read_to_string(paths::root().join("hypotheses.toml"))?)?;
    let results = ExperimentResult::load_all()?;
    let by_exp: BTreeMap<&str, &ExperimentResult> = results.iter().map(|r| (r.experiment.as_str(), r)).collect();

    let mut missing = Vec::new();
    let mut doc = String::new();
    writeln!(doc, "# Relatório da bancada")?;
    writeln!(doc)?;
    writeln!(
        doc,
        "Gerado por `cargo run -p runner` a partir de `testbench/results/`. Cada linha aponta pro experimento que a decidiu; o README de cada experimento tem método e detalhes."
    )?;
    writeln!(doc)?;
    if let Some(r) = results.first() {
        writeln!(doc, "Host: kernel {}, {} CPUs, {}.", r.host.kernel, r.host.cpus, r.host.rustc)?;
        writeln!(doc)?;
    }

    writeln!(doc, "## Hipóteses")?;
    writeln!(doc)?;
    writeln!(doc, "| Id | Origem | Hipótese | Experimento | Veredito | Por quê |")?;
    writeln!(doc, "|---|---|---|---|---|---|")?;
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for h in &registry.hypothesis {
        let found = by_exp.get(h.experiment.as_str()).and_then(|r| r.hypotheses.iter().find(|v| v.id == h.id));
        let (verdict, summary) = match found {
            Some(v) => (v.verdict.label_pt(), v.summary.clone()),
            None => {
                missing.push(format!("{} ({})", h.id, h.experiment));
                ("sem veredito", format!("Critério: {}", h.criterion))
            }
        };
        *counts.entry(verdict).or_default() += 1;
        writeln!(
            doc,
            "| {} | {} | {} | [{}](../testbench/experiments/{}/README.md) | **{}** | {} |",
            h.id,
            h.source,
            cell(&h.statement),
            h.experiment,
            h.experiment,
            verdict,
            cell(&summary)
        )?;
    }
    writeln!(doc)?;
    let tally: Vec<String> = counts.iter().map(|(k, v)| format!("{v} {k}")).collect();
    writeln!(doc, "Total: {}.", tally.join(", "))?;
    writeln!(doc)?;

    writeln!(doc, "## Candidatos por papel")?;
    writeln!(doc)?;
    writeln!(doc, "Categoria do depscan: (a) não toca o host, (b) toca o host em Rust, (c) tem C. Estrito: stdout, stderr, exit e arquivos iguais ao Debian; leniente: stderr pode divergir.")?;
    writeln!(doc)?;
    writeln!(doc, "| Papel | Candidato | Versão | Cat. | Estrito | Leniente | Encaixe | Notas |")?;
    writeln!(doc, "|---|---|---|---|---|---|---|---|")?;
    let mut candidates: Vec<(&str, &harness::CandidateResult)> =
        results.iter().flat_map(|r| r.candidates.iter().map(move |c| (r.experiment.as_str(), c))).collect();
    candidates.sort_by(|a, b| a.1.role.cmp(&b.1.role).then(a.1.name.cmp(&b.1.name)));
    for (_, c) in &candidates {
        let (strict, lenient) = match &c.conformance {
            Some(conf) => (pct(conf.strict_pass, conf.total), pct(conf.lenient_pass, conf.total)),
            None => ("-".into(), "-".into()),
        };
        writeln!(
            doc,
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            cell(&c.role),
            cell(&c.name),
            cell(&c.version),
            c.category.as_deref().unwrap_or("-"),
            strict,
            lenient,
            fit_pt(c.fit),
            cell(&c.notes)
        )?;
    }
    writeln!(doc)?;

    writeln!(doc, "## Notas por experimento")?;
    writeln!(doc)?;
    for exp in experiments()? {
        match by_exp.get(exp.as_str()) {
            Some(r) => {
                writeln!(doc, "### {} ({})", r.title, r.experiment)?;
                writeln!(doc)?;
                writeln!(doc, "Rodado em {}. [README](../testbench/experiments/{}/README.md).", r.generated_at, r.experiment)?;
                writeln!(doc)?;
                for n in &r.notes {
                    writeln!(doc, "- {}", n.replace('\n', " "))?;
                }
                if !r.notes.is_empty() {
                    writeln!(doc)?;
                }
            }
            None => {
                writeln!(doc, "### {exp}")?;
                writeln!(doc)?;
                writeln!(doc, "Sem `results/{exp}.json` ainda.")?;
                writeln!(doc)?;
            }
        }
    }

    if !missing.is_empty() {
        writeln!(doc, "## Pendências")?;
        writeln!(doc)?;
        for m in &missing {
            writeln!(doc, "- {m} sem veredito")?;
        }
        writeln!(doc)?;
    }

    let out = paths::repo_root().join("docs/bench-report.md");
    std::fs::write(&out, doc)?;
    println!("relatório: {}", out.display());
    if !missing.is_empty() {
        bail!("{} hipóteses sem veredito: {}", missing.len(), missing.join(", "));
    }
    Ok(())
}
