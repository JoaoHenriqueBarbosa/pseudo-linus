//! F08 files-formats: diff/patch (H30), compressão e arquivos (H31), date (H32) e bc/file/yq/csv (H33).
//!
//! Roda todas as partes em sequência (as medições de velocidade da H31 não podem disputar CPU com as
//! outras), compõe o veredito da H33 a partir dos papéis e grava `results/f08-files-formats.json`.

mod common;
mod h30_diff;
mod h31_archive;
mod h32_date;
mod h33_bc_file;
mod h33_yq_csv;

use std::time::Instant;

use anyhow::Result;
use harness::{ExperimentResult, Fit, Verdict};
use serde_json::{Map, Value, json};

use common::{Part, RoleSummary};

type PartFn = fn() -> Result<Part>;

fn main() -> Result<()> {
    let started = Instant::now();
    let mut res = ExperimentResult::new(common::EXPERIMENT, "Diff/patch, compressão e arquivos, date, bc/file/yq/csv");
    let parts: [(&str, PartFn); 5] = [
        ("h30_diff", h30_diff::run),
        ("h31_archive", h31_archive::run),
        ("h32_date", h32_date::run),
        ("h33_bc_file", h33_bc_file::run),
        ("h33_yq_csv", h33_yq_csv::run),
    ];
    let mut metrics = Map::new();
    let mut roles: Vec<RoleSummary> = Vec::new();
    let mut timings = Map::new();
    for (key, run) in parts {
        let t = Instant::now();
        let part = run().map_err(|e| e.context(format!("parte {key}")))?;
        timings.insert(key.to_string(), json!((t.elapsed().as_secs_f64() * 10.0).round() / 10.0));
        common::print_part(&part);
        res.hypotheses.extend(part.hypotheses);
        res.candidates.extend(part.candidates);
        roles.extend(part.roles.iter().cloned());
        res.notes.extend(part.notes);
        metrics.insert(part.key.clone(), part.metrics);
    }
    compose_h33(&mut res, &roles);
    metrics.insert("roles".into(), serde_json::to_value(&roles)?);
    metrics.insert("elapsed_s".into(), Value::Object(timings));
    res.metrics = Value::Object(metrics);
    let path = res.write()?;
    println!("gravado {} em {:.1}s", path.display(), started.elapsed().as_secs_f64());
    Ok(())
}

/// H33: "bc, file, yq e csv têm crate que encaixa". Confirmada se os quatro papéis têm crate que encaixa
/// do jeito que está (`Fits`); refutada se nenhum papel tem candidato que sirva nem com trabalho; parcial
/// quando algum papel só fecha com fork ou camada nossa relevante (`FitsWithWork`) ou não fecha.
fn compose_h33(res: &mut ExperimentResult, roles: &[RoleSummary]) {
    let wanted = ["bc", "file", "yq", "csv"];
    let mut direct = Vec::new();
    let mut with_work = Vec::new();
    let mut not_fitting = Vec::new();
    let mut missing = Vec::new();
    for role in wanted {
        match roles.iter().find(|r| r.role == role) {
            Some(r) if r.fit == Fit::Fits => direct.push(role),
            Some(r) if r.fit == Fit::FitsWithWork => with_work.push(role),
            Some(_) => not_fitting.push(role),
            None => missing.push(role),
        }
    }
    let verdict = if !missing.is_empty() {
        Verdict::Inconclusive
    } else if direct.len() == wanted.len() {
        Verdict::Confirmed
    } else if direct.is_empty() && with_work.is_empty() {
        Verdict::Refuted
    } else {
        Verdict::Partial
    };
    let list = |v: &[&str]| if v.is_empty() { "nenhum".to_string() } else { v.join(", ") };
    let mut summary: Vec<String> = vec![format!(
        "{}: encaixa do jeito que está: {}; só com fork ou camada nossa relevante: {}; não encaixa: {}.",
        verdict.label_pt(),
        list(&direct),
        list(&with_work),
        list(&not_fitting)
    )];
    for r in roles.iter().filter(|r| wanted.contains(&r.role.as_str())) {
        summary.push(format!("{}: {}", r.role, r.summary));
    }
    if !missing.is_empty() {
        summary.push(format!("sem resultado: {}", missing.join(", ")));
    }
    let evidence = json!({
        "roles": roles.iter().filter(|r| wanted.contains(&r.role.as_str())).collect::<Vec<_>>(),
    });
    res.hypothesis("H33", verdict, summary.join(" "), evidence);
}
