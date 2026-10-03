use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Candidate, Case, Outcome};

/// Resultado da comparação de um caso.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaseComparison {
    pub id: String,
    pub tags: Vec<String>,
    pub stdout_ok: bool,
    pub stderr_ok: bool,
    pub exit_ok: bool,
    pub files_ok: bool,
    /// Tudo igual, byte a byte.
    pub strict: bool,
    /// stdout, exit e arquivos iguais (stderr pode divergir).
    pub lenient: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail: Vec<String>,
}

pub fn compare_outcome(case: &Case, golden: &Outcome, actual: &Outcome) -> CaseComparison {
    let stdout_ok = golden.stdout == actual.stdout;
    let stderr_ok = golden.stderr == actual.stderr;
    let exit_ok = golden.exit == actual.exit && golden.signal == actual.signal;
    let file_diff = golden.files.diff(&actual.files);
    let files_ok = file_diff.is_empty();
    let unsupported = actual.unsupported.clone();
    let supported = unsupported.is_none();
    let strict = supported && stdout_ok && stderr_ok && exit_ok && files_ok;
    let lenient = supported && stdout_ok && exit_ok && files_ok;
    let mut detail = Vec::new();
    if let Some(why) = &unsupported {
        detail.push(format!("unsupported: {why}"));
    } else {
        if !stdout_ok {
            detail.push(format!("stdout: esperado {} obtido {}", golden.stdout.preview(120), actual.stdout.preview(120)));
        }
        if !stderr_ok {
            detail.push(format!("stderr: esperado {} obtido {}", golden.stderr.preview(120), actual.stderr.preview(120)));
        }
        if !exit_ok {
            detail.push(format!(
                "exit: esperado {:?}/{:?} obtido {:?}/{:?}",
                golden.exit, golden.signal, actual.exit, actual.signal
            ));
        }
        for d in file_diff.into_iter().take(8) {
            detail.push(format!("files: {d}"));
        }
    }
    CaseComparison {
        id: case.id.clone(),
        tags: case.tags.clone(),
        stdout_ok,
        stderr_ok,
        exit_ok,
        files_ok,
        strict,
        lenient,
        unsupported,
        detail,
    }
}

/// Placar de um candidato num conjunto de casos.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Conformance {
    pub candidate: String,
    pub total: usize,
    pub strict_pass: usize,
    pub lenient_pass: usize,
    pub unsupported: usize,
    /// tag -> (strict_pass, lenient_pass, total)
    pub by_tag: BTreeMap<String, (usize, usize, usize)>,
    /// Alguns exemplos de divergência, pro relatório.
    pub sample_failures: Vec<CaseComparison>,
}

impl Conformance {
    pub fn strict_rate(&self) -> f64 {
        ratio(self.strict_pass, self.total)
    }

    pub fn lenient_rate(&self) -> f64 {
        ratio(self.lenient_pass, self.total)
    }
}

fn ratio(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { a as f64 / b as f64 }
}

/// Roda o candidato em cada caso que tem golden e devolve o placar e a comparação caso a caso.
pub fn score(candidate: &dyn Candidate, cases: &[(Case, Outcome)]) -> (Conformance, Vec<CaseComparison>) {
    let mut conf = Conformance { candidate: candidate.name(), ..Conformance::default() };
    let mut all = Vec::with_capacity(cases.len());
    for (case, golden) in cases {
        let inv = match case.invocation() {
            Ok(inv) => inv,
            Err(e) => {
                let actual = Outcome::unsupported(format!("caso inválido: {e}"));
                all.push(compare_outcome(case, golden, &actual));
                continue;
            }
        };
        let actual = run_guarded(candidate, &inv);
        all.push(compare_outcome(case, golden, &actual));
    }
    for cmp in &all {
        conf.total += 1;
        conf.strict_pass += cmp.strict as usize;
        conf.lenient_pass += cmp.lenient as usize;
        conf.unsupported += cmp.unsupported.is_some() as usize;
        let tags: Vec<String> = if cmp.tags.is_empty() { vec!["untagged".into()] } else { cmp.tags.clone() };
        for tag in tags {
            let slot = conf.by_tag.entry(tag).or_default();
            slot.0 += cmp.strict as usize;
            slot.1 += cmp.lenient as usize;
            slot.2 += 1;
        }
    }
    conf.sample_failures = all.iter().filter(|c| !c.strict).take(15).cloned().collect();
    (conf, all)
}

/// Um panic dentro do candidato vira falha do caso, não da bancada.
fn run_guarded(candidate: &dyn Candidate, inv: &crate::Invocation) -> Outcome {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| candidate.run(inv))) {
        Ok(out) => out,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panic sem mensagem".into());
            Outcome::unsupported(format!("panic: {msg}"))
        }
    }
}
