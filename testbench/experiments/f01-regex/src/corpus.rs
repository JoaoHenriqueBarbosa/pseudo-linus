//! Fontes de regex do F01 e geração das sondas de cada uma.
//!
//! - suítes do GNU grep 3.11 (`spencer1.tests`, `bre.tests`, `ere.tests`; a antiga `spencer2` foi
//!   dividida nessas duas em 2009), baixadas pra `corpus/upstream/grep/`;
//! - regexes no estilo de agente escritas à mão em `data/agent_style.toml`;
//! - padrões reais minerados dos transcripts (ver [`crate::mined`]), que nunca viram arquivo commitado.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::ast;
use crate::parse::{Dialect, parse};
use crate::probe::{Probe, ProbeKind, gawk_safe};

/// Uma regex com as linhas de teste dela.
#[derive(Clone, Debug)]
pub struct RegexEntry {
    pub id: String,
    pub pattern: String,
    pub dialect: Dialect,
    pub icase: bool,
    pub subjects: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeSet {
    /// Todas as sondas que se aplicam.
    Full,
    /// Suítes upstream: `grep -ob` (o exit dá o casa/não casa) e as de grupo.
    Upstream,
}

pub fn sed_dialect(d: Dialect) -> Dialect {
    match d {
        Dialect::GrepBre | Dialect::SedBre => Dialect::SedBre,
        Dialect::GrepEre | Dialect::SedEre => Dialect::SedEre,
    }
}

/// Sondas de uma regex, cada uma com id e tags.
pub fn probes_for(entry: &RegexEntry, set: ProbeSet) -> Vec<(String, Probe, Vec<String>)> {
    let parsed = parse(&entry.pattern, entry.dialect);
    let mut tags = entry.tags.clone();
    tags.push(format!("dialect:{}", entry.dialect.label()));
    if entry.icase {
        tags.push("icase".into());
    }
    match &parsed {
        Ok(re) => tags.extend(ast::features(re).into_iter().map(|f| format!("feat:{f}"))),
        Err(_) => tags.push("feat:syntax-error".into()),
    }
    let groups_main = parsed.as_ref().map(|r| r.groups).unwrap_or(0);
    let sed_d = sed_dialect(entry.dialect);
    let sed_groups = parse(&entry.pattern, sed_d).map(|r| r.groups).unwrap_or(groups_main);

    let mut kinds: Vec<(ProbeKind, Dialect, usize)> = Vec::new();
    let grep_side = !entry.dialect.sed();
    match (set, grep_side) {
        (ProbeSet::Full, true) => {
            kinds.push((ProbeKind::GrepN, entry.dialect, 0));
            kinds.push((ProbeKind::GrepO, entry.dialect, 0));
        }
        (ProbeSet::Full, false) => {
            kinds.push((ProbeKind::SedN, entry.dialect, 0));
            kinds.push((ProbeKind::SedG, entry.dialect, 0));
        }
        (ProbeSet::Upstream, _) => kinds.push((ProbeKind::GrepO, entry.dialect, 0)),
    }
    if sed_groups > 0 {
        kinds.push((ProbeKind::SedSub, sed_d, sed_groups));
    }
    if let Ok(re) = &parsed
        && gawk_safe(&entry.pattern, re, entry.dialect)
    {
        kinds.push((ProbeKind::Gawk, Dialect::GrepEre, re.groups));
    }
    kinds
        .into_iter()
        .map(|(kind, dialect, groups)| {
            let probe = Probe {
                kind,
                dialect,
                icase: entry.icase,
                pattern: entry.pattern.clone(),
                groups,
                subjects: entry.subjects.clone(),
            };
            let mut t = tags.clone();
            t.push(format!("probe:{}", kind.label()));
            t.push(format!("aspect:{}", kind.aspect()));
            (format!("{}-{}", entry.id, kind.label()), probe, t)
        })
        .collect()
}

/// Lê uma suíte `exit@padrão@texto[@nota]` do GNU grep.
pub fn load_upstream(path: &Path, suite: &str, dialect: Dialect) -> Result<Vec<RegexEntry>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("lendo {}", path.display()))?;
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('@').collect();
        if fields.len() < 3 || fields.len() > 4 {
            continue;
        }
        let mut tags = vec![format!("src:{suite}"), format!("expect:{}", fields[0])];
        if fields.len() == 4 {
            tags.push("upstream-todo".into());
        }
        out.push(RegexEntry {
            id: format!("{suite}-{:03}", n + 1),
            pattern: fields[1].to_string(),
            dialect,
            icase: false,
            subjects: vec![fields[2].to_string()],
            tags,
        });
    }
    Ok(out)
}

#[derive(Deserialize)]
struct Spec {
    subjects: BTreeMap<String, Vec<String>>,
    corpus: SpecCorpus,
}

#[derive(Deserialize)]
struct SpecCorpus {
    regex: Vec<SpecEntry>,
}

#[derive(Deserialize)]
struct SpecEntry {
    id: String,
    re: String,
    d: Dialect,
    #[serde(default)]
    on: Vec<String>,
    #[serde(default)]
    s: Vec<String>,
    #[serde(default)]
    i: bool,
}

pub fn load_agent_style(path: &Path) -> Result<Vec<RegexEntry>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("lendo {}", path.display()))?;
    let spec: Spec = toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for e in spec.corpus.regex {
        anyhow::ensure!(seen.insert(e.id.clone()), "id repetido: {}", e.id);
        let mut subjects = Vec::new();
        for group in &e.on {
            subjects.extend(spec.subjects.get(group).with_context(|| format!("{}: grupo {group}", e.id))?.iter().cloned());
        }
        subjects.extend(e.s.iter().cloned());
        anyhow::ensure!(!subjects.is_empty(), "{}: sem linhas", e.id);
        out.push(RegexEntry {
            id: format!("agent-{}", e.id),
            pattern: e.re,
            dialect: e.d,
            icase: e.i,
            subjects,
            tags: vec!["src:agent-style".into()],
        });
    }
    Ok(out)
}

/// Todas as linhas de `[subjects]` da especificação (usadas como palheiro dos padrões minerados).
pub fn spec_subjects(path: &Path) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("lendo {}", path.display()))?;
    let spec: Spec = toml::from_str(&text)?;
    let mut out: Vec<String> = Vec::new();
    for lines in spec.subjects.values() {
        for l in lines {
            if !out.contains(l) {
                out.push(l.clone());
            }
        }
    }
    Ok(out)
}

pub fn spec_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("data/agent_style.toml")
}

pub fn upstream_dir() -> std::path::PathBuf {
    harness::paths::corpus_dir().join("upstream/grep")
}

/// As três suítes do grep, na ordem.
pub fn upstream_suites() -> [(&'static str, &'static str, Dialect); 3] {
    [
        ("spencer1", "spencer1.tests", Dialect::GrepEre),
        ("bre", "bre.tests", Dialect::GrepBre),
        ("ere", "ere.tests", Dialect::GrepEre),
    ]
}
