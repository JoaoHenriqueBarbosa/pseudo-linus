//! H20: `forbid(unsafe_code)` pega todo unsafe nosso, inclusive gerado por macro?
//!
//! O `forbid-consumer` (sonda) tem `#![forbid(unsafe_code)]` e `[lints.rust] unsafe_code = "forbid"`,
//! e nenhum `unsafe` escrito à mão. Cada caso é uma feature que liga um módulo que só invoca uma macro
//! de outra crate. O E06 compila um caso por vez com `cargo build` e classifica o resultado.

use std::collections::BTreeMap;

use anyhow::Result;
use harness::Verdict;
use serde::{Deserialize, Serialize};

use crate::cargo_probe::{self, CargoRun};
use crate::layout;

/// Feature, descrição e se é o controle positivo (o lint precisa disparar).
pub const CASES: &[(&str, &str, bool)] = &[
    ("macro_rules_block", "(a) macro_rules! de outra crate expandindo pra bloco unsafe", false),
    ("proc_call_site", "(b) proc macro de outra crate, span call_site", false),
    ("proc_mixed_site", "(c) proc macro de outra crate, span mixed_site", false),
    ("proc_located_at_input", "proc macro, contexto call_site com linha/coluna da entrada", false),
    ("macro_rules_unsafe_impl", "(d) macro_rules! gerando unsafe impl Send", false),
    ("derive_bytemuck_pod", "(d) derive Pod/Zeroable do bytemuck (proc macro real, unsafe impl)", false),
    ("intrusive_adapter", "(d) intrusive_collections::intrusive_adapter! (unsafe impl com allow(unsafe_code))", false),
    ("macro_rules_allow", "macro_rules! gerando #[allow(unsafe_code)] e bloco unsafe", false),
    ("macro_rules_no_mangle", "macro_rules! gerando #[unsafe(no_mangle)]", false),
    ("proc_input_span", "controle: proc macro com o span da entrada (unsafe atribuído ao crate)", true),
];

/// Como o build de um caso terminou.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseOutcome {
    /// Compilou: o unsafe gerado passou pelo forbid.
    Compiles,
    /// O lint `unsafe_code` disparou.
    UnsafeCodeLint,
    /// E0453: a macro trouxe `allow(unsafe_code)`, que o forbid não deixa rebaixar.
    ForbidOverridesAllow,
    /// Falhou por outro motivo (bug da sonda).
    OtherError,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaseResult {
    pub feature: String,
    pub description: String,
    pub control: bool,
    pub outcome: CaseOutcome,
    /// Códigos de erro/lint do consumidor (`unsafe_code`, `E0453`...).
    pub codes: Vec<String>,
    pub first_message: Option<String>,
    pub seconds: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct H20Outcome {
    pub baseline: CaseResult,
    pub cases: Vec<CaseResult>,
    /// Unsafe que o depscan conta em cada crate da árvore do consumidor (tokens dentro de macro inclusos).
    pub depscan_unsafe: BTreeMap<String, usize>,
    pub depscan_root_unsafe: usize,
    pub verdict: Verdict,
    pub summary: String,
}

const TARGET: &str = "forbid_consumer";

fn classify(run: &CargoRun) -> (CaseOutcome, Vec<String>, Option<String>) {
    let mut codes: Vec<String> = run.diagnostics_for(TARGET).filter_map(|d| d.code.clone()).collect();
    codes.sort();
    codes.dedup();
    let first_message = run.diagnostics_for(TARGET).find(|d| d.level == "error").map(|d| {
        let place = match (&d.file, d.line) {
            (Some(f), Some(l)) => format!(" ({f}:{l})"),
            _ => String::new(),
        };
        format!("{}{place}", d.message)
    });
    // Feature com nome errado no `cfg` deixaria o módulo de fora e o caso "compilaria" à toa.
    let outcome = if codes.iter().any(|c| c == "unexpected_cfgs") {
        CaseOutcome::OtherError
    } else if run.success() {
        CaseOutcome::Compiles
    } else if codes.iter().any(|c| c == "unsafe_code") {
        CaseOutcome::UnsafeCodeLint
    } else if codes.iter().any(|c| c == "E0453") {
        CaseOutcome::ForbidOverridesAllow
    } else {
        CaseOutcome::OtherError
    };
    (outcome, codes, first_message)
}

fn build_case(feature: Option<&str>, description: &str, control: bool) -> Result<CaseResult> {
    let run = cargo_probe::run(&cargo_probe::probe_args("build", "forbid-consumer", feature))?;
    let (outcome, codes, first_message) = classify(&run);
    let first_message = first_message.or_else(|| (outcome == CaseOutcome::OtherError).then(|| run.stderr_tail.clone()));
    Ok(CaseResult {
        feature: feature.unwrap_or("(nenhuma)").to_string(),
        description: description.to_string(),
        control,
        outcome,
        codes,
        first_message,
        seconds: run.seconds,
    })
}

pub fn run() -> Result<H20Outcome> {
    let baseline = build_case(None, "baseline sem macro", false)?;
    let mut cases = Vec::new();
    for &(feature, description, control) in CASES {
        cases.push(build_case(Some(feature), description, control)?);
    }

    let scan = depscan::scan(&layout::probes_manifest(), "forbid-consumer")?;
    let depscan_unsafe: BTreeMap<String, usize> = scan
        .deps
        .iter()
        .filter(|d| d.counts.unsafe_total() > 0)
        .map(|d| (format!("{} {}", d.name, d.version), d.counts.unsafe_total()))
        .collect();
    let depscan_root_unsafe = scan.root.counts.unsafe_total();

    let control_ok = cases.iter().filter(|c| c.control).all(|c| c.outcome == CaseOutcome::UnsafeCodeLint);
    let bypass: Vec<&CaseResult> = cases.iter().filter(|c| !c.control && c.outcome == CaseOutcome::Compiles).collect();
    let blocked_by_allow: Vec<&CaseResult> =
        cases.iter().filter(|c| c.outcome == CaseOutcome::ForbidOverridesAllow).collect();
    let probes_total = cases.iter().filter(|c| !c.control).count();
    let broken: Vec<&CaseResult> = cases.iter().filter(|c| c.outcome == CaseOutcome::OtherError).collect();

    let (verdict, summary) = if baseline.outcome != CaseOutcome::Compiles || !control_ok || !broken.is_empty() {
        (
            Verdict::Inconclusive,
            format!(
                "Sonda quebrada: baseline {:?}, controle {}, {} casos com erro alheio ao lint.",
                baseline.outcome,
                if control_ok { "ok" } else { "falhou" },
                broken.len()
            ),
        )
    } else if bypass.is_empty() {
        (Verdict::Confirmed, format!("O forbid barrou os {probes_total} casos de unsafe gerado por macro."))
    } else {
        let list = |v: Vec<&str>| if v.is_empty() { "nenhum".to_string() } else { v.join(", ") };
        let names = list(bypass.iter().map(|c| c.feature.as_str()).collect());
        let lint_names = list(
            cases
                .iter()
                .filter(|c| !c.control && c.outcome == CaseOutcome::UnsafeCodeLint)
                .map(|c| c.feature.as_str())
                .collect(),
        );
        let allow_names = list(blocked_by_allow.iter().map(|c| c.feature.as_str()).collect());
        (
            Verdict::Refuted,
            format!(
                "{} de {probes_total} casos de unsafe gerado por macro de outra crate compilaram sob \
                 forbid(unsafe_code): {names}. Pegos pelo lint unsafe_code: {lint_names}. Barrados por E0453 (a \
                 macro emitia allow(unsafe_code) e o forbid não deixa rebaixar, sem detectar o unsafe): \
                 {allow_names}. O controle, com o token unsafe no span do consumidor, foi pego pelo lint.",
                bypass.len(),
            ),
        )
    };

    Ok(H20Outcome { baseline, cases, depscan_unsafe, depscan_root_unsafe, verdict, summary })
}
