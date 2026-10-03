//! Métricas do F01 a partir das comparações caso a caso: concordância por aspecto (casa/não casa,
//! span, submatch), por fonte, por classe de construção, por regex inteira, ponderada pela frequência
//! real nos transcripts, e combinações de motores roteadas por características estáticas da regex.

use std::collections::{BTreeMap, HashMap};

use harness::{Case, CaseComparison, Outcome};
use serde::Serialize;

use crate::probe::ProbeKind;

/// O que se sabe de um caso sem rodar motor nenhum.
#[derive(Clone, Debug)]
pub struct CaseInfo {
    pub id: String,
    pub regex_id: String,
    pub kind: ProbeKind,
    pub source: String,
    pub tags: Vec<String>,
    pub weight: u64,
    pub golden_error: bool,
}

impl CaseInfo {
    pub fn has(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    pub fn committed(&self) -> bool {
        !self.source.starts_with("mined")
    }
}

pub fn case_info(case: &Case, golden: &Outcome, weights: &HashMap<String, u64>) -> CaseInfo {
    let kind = case
        .tags
        .iter()
        .find_map(|t| t.strip_prefix("probe:"))
        .and_then(|k| serde_json::from_str::<ProbeKind>(&format!("\"{k}\"")).ok())
        .unwrap_or(ProbeKind::GrepO);
    let suffix = format!("-{}", kind.label());
    let regex_id = case.id.strip_suffix(&suffix).unwrap_or(&case.id).to_string();
    let source = case.tags.iter().find_map(|t| t.strip_prefix("src:")).unwrap_or("unknown").to_string();
    let golden_error = match kind {
        ProbeKind::GrepN | ProbeKind::GrepO | ProbeKind::Gawk => golden.exit == Some(2),
        _ => golden.exit == Some(1) || golden.exit == Some(4),
    };
    let weight = weights.get(&regex_id).copied().unwrap_or(1);
    CaseInfo { id: case.id.clone(), regex_id, kind, source, tags: case.tags.clone(), weight, golden_error }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Ratio {
    pub pass: u64,
    pub total: u64,
}

impl Ratio {
    pub fn add(&mut self, ok: bool, w: u64) {
        self.pass += ok as u64 * w;
        self.total += w;
    }

    pub fn rate(&self) -> f64 {
        if self.total == 0 { 0.0 } else { self.pass as f64 / self.total as f64 }
    }
}

/// Concordância de um conjunto de sondas.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Rates {
    /// Casa ou não casa: `grep -n`, `sed -n` e o exit do `grep -ob` das suítes upstream.
    pub r#match: Ratio,
    /// Spans: `grep -ob` e `sed s///g`.
    pub span: Ratio,
    /// Grupos: `sed s//\1/` e `gawk match()`.
    pub submatch: Ratio,
    /// Padrões que o GNU recusa (exit de erro): o candidato também recusou?
    pub syntax_errors: Ratio,
    /// Sondas que o candidato não conseguiu rodar (construção sem suporte, timeout, crash).
    pub unsupported: Ratio,
    /// Regex inteira: todas as sondas dela iguais ao GNU.
    pub regex: Ratio,
}

/// `pass(case)` diz se a sonda concorda (já roteada pro motor certo, no caso das combinações).
pub fn rates<'a>(
    infos: impl Iterator<Item = &'a CaseInfo>,
    weighted: bool,
    cmp: &dyn Fn(&CaseInfo) -> Option<&'a CaseComparison>,
) -> Rates {
    let mut r = Rates::default();
    let mut per_regex: BTreeMap<&str, (bool, u64)> = BTreeMap::new();
    for info in infos {
        let Some(c) = cmp(info) else { continue };
        let w = if weighted { info.weight } else { 1 };
        let upstream = info.source.starts_with("upstream");
        match info.kind.aspect() {
            "match" => r.r#match.add(c.lenient, w),
            "span" => {
                r.span.add(c.lenient, w);
                if upstream {
                    r.r#match.add(c.exit_ok && c.unsupported.is_none(), w);
                }
            }
            _ => r.submatch.add(c.lenient, w),
        }
        if info.golden_error {
            r.syntax_errors.add(c.lenient, w);
        }
        r.unsupported.add(c.unsupported.is_some(), w);
        let slot = per_regex.entry(info.regex_id.as_str()).or_insert((true, w));
        slot.0 &= c.lenient;
    }
    for (_, (ok, w)) in per_regex {
        r.regex.add(ok, w);
    }
    r
}

/// Concordância por classe de construção (`feat:*`), no nível da regex.
pub fn by_feature<'a>(
    infos: &'a [CaseInfo],
    cmp: &dyn Fn(&CaseInfo) -> Option<&'a CaseComparison>,
) -> BTreeMap<String, Ratio> {
    let mut regex_ok: BTreeMap<&str, (bool, &CaseInfo)> = BTreeMap::new();
    for info in infos {
        let Some(c) = cmp(info) else { continue };
        let slot = regex_ok.entry(info.regex_id.as_str()).or_insert((true, info));
        slot.0 &= c.lenient;
    }
    let mut out: BTreeMap<String, Ratio> = BTreeMap::new();
    for (_, (ok, info)) in regex_ok {
        for t in info.tags.iter().filter_map(|t| t.strip_prefix("feat:")) {
            out.entry(t.to_string()).or_default().add(ok, 1);
        }
        let dialect = info.tags.iter().find_map(|t| t.strip_prefix("dialect:")).unwrap_or("?");
        out.entry(format!("dialect-{dialect}")).or_default().add(ok, 1);
        if info.has("icase") {
            out.entry("icase".into()).or_default().add(ok, 1);
        }
    }
    out
}

/// Uma combinação de motores: a regra escolhe o motor por características da regex e pelo aspecto.
pub struct Combo {
    pub name: &'static str,
    pub description: &'static str,
    pub route: fn(&CaseInfo) -> &'static str,
}

fn needs_backtracking(i: &CaseInfo) -> bool {
    i.has("feat:backref")
}

fn posix_inexpressible(i: &CaseInfo) -> bool {
    i.has("feat:backref") || i.has("feat:word-boundary")
}

pub fn combos() -> Vec<Combo> {
    vec![
        Combo {
            name: "combo: regex-automata-longest + ferroni-longest (backref)",
            description: "DFA de tempo linear (montagem leftmost-longest sobre regex-automata) pra tudo, ferroni só quando há backref",
            route: |i| if needs_backtracking(i) { "ferroni-longest" } else { "regex-automata-longest" },
        },
        Combo {
            name: "combo: regex-automata-longest (span) + ferroni-longest (grupos, backref)",
            description: "regex-automata decide casa/não casa e spans; ferroni dá os grupos e cobre backref",
            route: |i| {
                if needs_backtracking(i) || i.kind.aspect() == "submatch" { "ferroni-longest" } else { "regex-automata-longest" }
            },
        },
        Combo {
            name: "combo: revera + ferroni-longest (backref, \\b)",
            description: "revera (ERE POSIX, subexpressões POSIX) onde o padrão cabe em ERE pura; ferroni no resto",
            route: |i| if posix_inexpressible(i) { "ferroni-longest" } else { "revera" },
        },
        Combo {
            name: "combo: resharp + ferroni-longest (backref)",
            description: "resharp (derivadas, leftmost-longest) pra tudo, ferroni quando há backref",
            route: |i| if needs_backtracking(i) { "ferroni-longest" } else { "resharp" },
        },
    ]
}
