//! Padrões reais de agentes (`corpus/agent/patterns.jsonl`, minerado pelo E08 de 54653 chamadas
//! Bash). O arquivo guarda padrão e contagem; as flags (`-E`, `-F`, `-P`, `-i`, `sed -E`) são
//! recuperadas aqui lendo `corpus/agent/commands.jsonl` (só parse, nada é executado).
//!
//! Nada daqui vira arquivo commitado: os casos e o golden ficam em `scratch/`, e o resultado só leva
//! agregados.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::corpus::RegexEntry;
use crate::parse::{Dialect, parse};
use crate::sample::{Rng, samples, seed_of};
use crate::shell::{parse_grep, parse_sed, sed_script_regexes, simple_commands};

#[derive(Deserialize)]
struct PatternLine {
    tool: String,
    pattern: String,
    count: u64,
}

#[derive(Deserialize)]
struct CommandLine {
    command: String,
    count: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MinedStats {
    pub grep_unique: usize,
    pub grep_occurrences: u64,
    /// Modo do grep por padrão único e por ocorrência: G, E, F, P.
    pub grep_mode_unique: BTreeMap<String, usize>,
    pub grep_mode_weighted: BTreeMap<String, u64>,
    /// Padrões cujo modo saiu das linhas de comando (o resto foi inferido pela sintaxe).
    pub grep_flags_from_commands: usize,
    pub grep_icase_unique: usize,
    pub sed_scripts_unique: usize,
    pub sed_scripts_occurrences: u64,
    pub sed_scripts_with_regex: usize,
    pub sed_scripts_with_regex_occurrences: u64,
    pub sed_ere_scripts: usize,
    pub sed_regex_unique: usize,
    pub regex_entries_unique: usize,
    pub sampled: BTreeMap<String, usize>,
    pub sampled_weight: u64,
    pub total_weight: u64,
}

#[derive(Clone, Debug)]
pub struct MinedEntry {
    pub entry: RegexEntry,
    pub weight: u64,
}

pub fn agent_dir() -> std::path::PathBuf {
    harness::paths::corpus_dir().join("agent")
}

fn infer_grep_mode(p: &str) -> char {
    if p.contains("\\|") || p.contains("\\(") || p.contains("\\{") || p.contains("\\+") {
        return 'G';
    }
    let mut prev = ' ';
    for c in p.chars() {
        if prev != '\\' && "|(+?{".contains(c) {
            return 'E';
        }
        prev = c;
    }
    'G'
}

/// Lê os padrões e as flags; devolve as regexes (BRE/ERE) com o peso de cada uma.
pub fn load(dir: &Path) -> Result<(Vec<MinedEntry>, MinedStats)> {
    let mut stats = MinedStats::default();
    let patterns_text = std::fs::read_to_string(dir.join("patterns.jsonl")).context("lendo patterns.jsonl")?;
    let mut grep_patterns: Vec<(String, u64)> = Vec::new();
    let mut sed_scripts: Vec<(String, u64)> = Vec::new();
    for line in patterns_text.lines().filter(|l| !l.trim().is_empty()) {
        let p: PatternLine = serde_json::from_str(line)?;
        match p.tool.as_str() {
            "grep" => grep_patterns.push((p.pattern, p.count)),
            "sed" => sed_scripts.push((p.pattern, p.count)),
            _ => {}
        }
    }
    // Flags observadas nas linhas de comando.
    let mut grep_modes: HashMap<String, BTreeMap<char, u64>> = HashMap::new();
    let mut grep_icase: HashMap<String, (u64, u64)> = HashMap::new();
    let mut sed_ere: HashMap<String, (u64, u64)> = HashMap::new();
    let commands_text = std::fs::read_to_string(dir.join("commands.jsonl")).context("lendo commands.jsonl")?;
    for line in commands_text.lines().filter(|l| !l.trim().is_empty()) {
        let c: CommandLine = serde_json::from_str(line)?;
        for words in simple_commands(&c.command) {
            if let Some(g) = parse_grep(&words) {
                for p in g.patterns {
                    *grep_modes.entry(p.clone()).or_default().entry(g.mode).or_default() += c.count;
                    let e = grep_icase.entry(p).or_default();
                    e.0 += c.count * g.icase as u64;
                    e.1 += c.count;
                }
            } else if let Some(s) = parse_sed(&words) {
                for script in s.scripts {
                    let e = sed_ere.entry(script).or_default();
                    e.0 += c.count * s.ere as u64;
                    e.1 += c.count;
                }
            }
        }
    }

    let mut merged: BTreeMap<(Dialect, bool, String), u64> = BTreeMap::new();
    stats.grep_unique = grep_patterns.len();
    for (p, count) in &grep_patterns {
        stats.grep_occurrences += count;
        let (mode, icase) = match grep_modes.get(p) {
            Some(m) => {
                stats.grep_flags_from_commands += 1;
                let mode = m.iter().max_by_key(|(_, n)| **n).map(|(c, _)| *c).unwrap_or('G');
                let (yes, total) = grep_icase.get(p).copied().unwrap_or((0, 1));
                (mode, yes * 2 > total)
            }
            None => (infer_grep_mode(p), false),
        };
        *stats.grep_mode_unique.entry(mode.to_string()).or_default() += 1;
        *stats.grep_mode_weighted.entry(mode.to_string()).or_default() += count;
        stats.grep_icase_unique += icase as usize;
        let dialect = match mode {
            'G' => Dialect::GrepBre,
            'E' => Dialect::GrepEre,
            _ => continue,
        };
        *merged.entry((dialect, icase, p.clone())).or_default() += count;
    }
    stats.sed_scripts_unique = sed_scripts.len();
    for (script, count) in &sed_scripts {
        stats.sed_scripts_occurrences += count;
        let ere = sed_ere.get(script).map(|(y, t)| y * 2 > *t).unwrap_or(false);
        stats.sed_ere_scripts += ere as usize;
        let found = sed_script_regexes(script);
        if found.is_empty() {
            continue;
        }
        stats.sed_scripts_with_regex += 1;
        stats.sed_scripts_with_regex_occurrences += count;
        let dialect = if ere { Dialect::SedEre } else { Dialect::SedBre };
        for r in found {
            *merged.entry((dialect, r.icase, r.pattern)).or_default() += count;
        }
    }
    stats.sed_regex_unique = merged.keys().filter(|(d, _, _)| d.sed()).count();
    stats.regex_entries_unique = merged.len();
    stats.total_weight = merged.values().sum();

    let entries = merged
        .into_iter()
        .filter(|((_, _, p), _)| !p.is_empty() && !p.contains(['\n', '\0', '\u{1}', '\u{2}', '\u{3}', '\u{4}']) && p.len() <= 400)
        .map(|((dialect, icase, pattern), weight)| MinedEntry {
            entry: RegexEntry { id: String::new(), pattern, dialect, icase, subjects: Vec::new(), tags: Vec::new() },
            weight,
        })
        .collect();
    Ok((entries, stats))
}

/// Os `top` mais frequentes de cada ferramenta mais `random` sorteados (semente fixa) do resto.
pub fn sample(entries: Vec<MinedEntry>, top: usize, random: usize, seed: u64, stats: &mut MinedStats) -> Vec<MinedEntry> {
    let mut out = Vec::new();
    for sed in [false, true] {
        let mut group: Vec<MinedEntry> = entries.iter().filter(|e| e.entry.dialect.sed() == sed).cloned().collect();
        group.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.entry.pattern.cmp(&b.entry.pattern)));
        let (top_part, rest) = group.split_at(group.len().min(if sed { top / 2 } else { top }));
        let tool = if sed { "sed" } else { "grep" };
        for (i, e) in top_part.iter().enumerate() {
            let mut e = e.clone();
            e.entry.id = format!("mined-{tool}-top-{i:05}");
            e.entry.tags = vec![format!("src:mined-{tool}"), "rank:top".into()];
            out.push(e);
        }
        let mut rest: Vec<MinedEntry> = rest.to_vec();
        let mut rng = Rng::new(seed ^ sed as u64);
        // Fisher-Yates parcial.
        let take = rest.len().min(if sed { random / 2 } else { random });
        for i in 0..take {
            let j = i + rng.below((rest.len() - i) as u64) as usize;
            rest.swap(i, j);
        }
        for (i, e) in rest.into_iter().take(take).enumerate() {
            let mut e = e;
            e.entry.id = format!("mined-{tool}-rand-{i:05}");
            e.entry.tags = vec![format!("src:mined-{tool}"), "rank:random".into()];
            out.push(e);
        }
        stats.sampled.insert(tool.into(), top_part.len() + take);
    }
    stats.sampled_weight = out.iter().map(|e| e.weight).sum();
    out
}

/// Linhas fixas pra todos os padrões minerados: as linhas do corpus escrito à mão (logs, código,
/// caminhos, rede, bordas, UTF-8) e um trecho do design.
pub fn haystack(spec_subjects: &[String]) -> Vec<String> {
    let mut out: Vec<String> = spec_subjects.to_vec();
    let design = harness::paths::repo_root().join("docs/design.md");
    if let Ok(text) = std::fs::read_to_string(design) {
        out.extend(
            text.lines()
                .filter(|l| !l.trim().is_empty())
                .step_by(9)
                .take(30)
                .map(|l| l.chars().take(160).collect::<String>()),
        );
    }
    out.retain(|l| !l.contains(['\0', '\u{1}', '\u{2}', '\u{3}', '\u{4}']));
    out.dedup();
    out
}

/// Preenche as linhas de cada entrada: o palheiro fixo mais amostras geradas pela própria regex.
pub fn attach_subjects(entries: &mut [MinedEntry], hay: &[String]) {
    for e in entries {
        let mut subjects = hay.to_vec();
        if let Ok(re) = parse(&e.entry.pattern, e.entry.dialect) {
            for s in samples(&re, e.entry.icase, seed_of(&e.entry.pattern), 6) {
                if !s.contains(['\u{1}', '\u{2}', '\u{3}', '\u{4}']) && !subjects.contains(&s) {
                    subjects.push(s);
                }
            }
        }
        e.entry.subjects = subjects;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_mode_from_syntax() {
        assert_eq!(infer_grep_mode("a\\|b"), 'G');
        assert_eq!(infer_grep_mode("a|b"), 'E');
        assert_eq!(infer_grep_mode("foo\\.bar"), 'G');
        assert_eq!(infer_grep_mode("x+"), 'E');
    }
}
