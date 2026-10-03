//! E08: o que agentes rodam de verdade.
//!
//! Lê os transcripts locais do Claude Code, extrai os comandos da tool `Bash`, parseia cada um com o
//! `brush-parser` e mede: quais comandos aparecem, com que flags, que recursos de shell, que padrões
//! vão pra grep/sed/awk/jq, e quanto disso o plano v2 cobriria.
//!
//! Privacidade: os comandos completos e os padrões ficam só em `testbench/corpus/agent/` (gitignored).
//! O resultado commitável (`results/e08-agent-corpus.json`) tem só agregados: nomes de comando
//! normalizados, flags sem valor, contagens.

mod catalog;
mod collect;
mod walk;

use std::collections::{BTreeMap, HashMap};
use std::io::Write;

use anyhow::Result;
use harness::{ExperimentResult, Verdict, paths};
use serde_json::json;

fn main() -> Result<()> {
    let root = collect::transcripts_dir();
    let started = std::time::Instant::now();
    let collected = collect::collect(&root)?;
    eprintln!(
        "{} transcripts, {} chamadas Bash em {:.1}s",
        collected.files,
        collected.commands.len(),
        started.elapsed().as_secs_f64()
    );

    // Agrupa por texto exato: o parse é por comando único, a estatística pondera pela contagem.
    let mut unique: HashMap<&str, (usize, std::collections::BTreeSet<&str>)> = HashMap::new();
    for c in &collected.commands {
        let slot = unique.entry(c.command.as_str()).or_default();
        slot.0 += 1;
        slot.1.insert(c.project.as_str());
    }
    let projects: std::collections::BTreeSet<&str> = collected.commands.iter().map(|c| c.project.as_str()).collect();
    let subagent_calls = collected.commands.iter().filter(|c| c.subagent).count();

    let mut unique_list: Vec<(&str, usize, usize)> =
        unique.iter().map(|(cmd, (n, p))| (*cmd, *n, p.len())).collect();
    unique_list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));

    // Corpus local (gitignored) pro F15 e pro F01.
    let agent_dir = paths::corpus_dir().join("agent");
    std::fs::create_dir_all(&agent_dir)?;
    {
        let mut f = std::io::BufWriter::new(std::fs::File::create(agent_dir.join("commands.jsonl"))?);
        for (cmd, count, nproj) in &unique_list {
            serde_json::to_writer(&mut f, &json!({"command": cmd, "count": count, "projects": nproj}))?;
            f.write_all(b"\n")?;
        }
    }

    let analyze_started = std::time::Instant::now();
    let analyses: Vec<(walk::Analysis, usize)> = unique_list
        .iter()
        .map(|(cmd, count, _)| (walk::analyze(cmd), *count))
        .collect();
    eprintln!("{} comandos únicos parseados em {:.1}s", analyses.len(), analyze_started.elapsed().as_secs_f64());

    let total_calls: usize = analyses.iter().map(|(_, n)| n).sum();
    let parsed_unique = analyses.iter().filter(|(a, _)| a.parsed).count();
    let parsed_calls: usize = analyses.iter().filter(|(a, _)| a.parsed).map(|(_, n)| n).sum();
    let mut parse_errors: BTreeMap<String, usize> = BTreeMap::new();
    for (a, _) in &analyses {
        if let Some(e) = &a.parse_error {
            *parse_errors.entry(e.clone()).or_default() += 1;
        }
    }

    // Ocorrências de comando ponderadas pela contagem de chamadas.
    let mut name_occ: HashMap<String, usize> = HashMap::new();
    let mut name_calls: HashMap<String, usize> = HashMap::new();
    let mut flags_by_name: HashMap<String, HashMap<String, usize>> = HashMap::new();
    let mut via_counts: HashMap<String, usize> = HashMap::new();
    let mut in_subst_occ = 0usize;
    let mut feature_calls: BTreeMap<String, usize> = BTreeMap::new();
    let mut pipeline_hist: BTreeMap<usize, usize> = BTreeMap::new();
    let mut runnable_calls = 0usize;
    let mut absent_calls_by_name: HashMap<String, usize> = HashMap::new();
    let mut absent_calls_by_group: BTreeMap<String, usize> = BTreeMap::new();
    let mut occurrences_per_call: Vec<(usize, usize)> = Vec::new();
    let mut patterns: HashMap<(String, String), usize> = HashMap::new();

    for (a, n) in &analyses {
        if !a.parsed {
            continue;
        }
        let mut seen_names = std::collections::BTreeSet::new();
        for o in &a.occurrences {
            *name_occ.entry(o.name.clone()).or_default() += n;
            seen_names.insert(o.name.clone());
            let fl = flags_by_name.entry(o.name.clone()).or_default();
            for f in &o.flags {
                *fl.entry(f.clone()).or_default() += n;
            }
            if let Some(v) = &o.via {
                *via_counts.entry(v.clone()).or_default() += n;
            }
            if o.in_substitution {
                in_subst_occ += n;
            }
        }
        occurrences_per_call.push((a.occurrences.len(), *n));
        for name in &seen_names {
            *name_calls.entry(name.clone()).or_default() += n;
        }
        for f in &a.features {
            *feature_calls.entry(f.clone()).or_default() += n;
        }
        if let Some(max) = a.pipeline_lengths.iter().max() {
            *pipeline_hist.entry(*max).or_default() += n;
        }
        let absent: Vec<&String> = seen_names
            .iter()
            .filter(|s| !catalog::is_planned(s) && !catalog::is_meta(s))
            .collect();
        if absent.is_empty() && !seen_names.iter().any(|s| catalog::is_meta(s)) {
            runnable_calls += n;
        }
        for name in &absent {
            *absent_calls_by_name.entry((*name).clone()).or_default() += n;
        }
        let mut groups: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for name in &absent {
            groups.insert(catalog::absent_group(name));
        }
        for g in groups {
            *absent_calls_by_group.entry(g.to_string()).or_default() += n;
        }
        for (tool, p) in &a.patterns {
            *patterns.entry((tool.clone(), p.clone())).or_default() += n;
        }
    }

    // Padrões (gitignored) pro F01/F04.
    {
        let mut list: Vec<_> = patterns.iter().collect();
        list.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let mut f = std::io::BufWriter::new(std::fs::File::create(agent_dir.join("patterns.jsonl"))?);
        for ((tool, pattern), count) in list {
            serde_json::to_writer(&mut f, &json!({"tool": tool, "pattern": pattern, "count": count}))?;
            f.write_all(b"\n")?;
        }
    }

    // Cobertura: quantos nomes distintos cobrem X% das ocorrências (sem as categorias meta).
    let mut ranked: Vec<(String, usize)> = name_occ.iter().map(|(k, v)| (k.clone(), *v)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let real: Vec<&(String, usize)> = ranked.iter().filter(|(n, _)| !catalog::is_meta(n)).collect();
    let real_total: usize = real.iter().map(|(_, c)| c).sum();
    let coverage = |q: f64| -> usize {
        let mut acc = 0usize;
        for (i, (_, c)) in real.iter().enumerate() {
            acc += c;
            if acc as f64 >= q * real_total as f64 {
                return i + 1;
            }
        }
        real.len()
    };
    let n50 = coverage(0.50);
    let n90 = coverage(0.90);
    let n95 = coverage(0.95);
    let n99 = coverage(0.99);
    let total_occ: usize = ranked.iter().map(|(_, c)| c).sum();

    let safe_name = |s: &str| s.len() <= 40 && s.chars().all(|c| c.is_ascii_alphanumeric() || "._+-<>[:".contains(c));
    let top_commands: Vec<serde_json::Value> = ranked
        .iter()
        .filter(|(n, _)| safe_name(n))
        .take(100)
        .map(|(n, c)| {
            json!({
                "name": n,
                "occurrences": c,
                "calls": name_calls.get(n).copied().unwrap_or(0),
                "planned": catalog::is_planned(n) || catalog::is_meta(n),
            })
        })
        .collect();
    let top_flags: BTreeMap<String, Vec<(String, usize)>> = ranked
        .iter()
        .filter(|(n, _)| safe_name(n) && !catalog::is_meta(n))
        .take(40)
        .map(|(n, _)| {
            let mut fl: Vec<(String, usize)> =
                flags_by_name.get(n).map(|m| m.iter().map(|(k, v)| (k.clone(), *v)).collect()).unwrap_or_default();
            fl.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            fl.truncate(15);
            (n.clone(), fl)
        })
        .collect();
    let mut absent_ranked: Vec<(String, usize)> = absent_calls_by_name
        .into_iter()
        .filter(|(n, _)| safe_name(n))
        .collect();
    absent_ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    absent_ranked.truncate(40);
    let feature_share: BTreeMap<String, serde_json::Value> = feature_calls
        .iter()
        .map(|(f, c)| (f.clone(), json!({"calls": c, "share": round4(*c as f64 / total_calls.max(1) as f64)})))
        .collect();
    let mut patterns_by_tool: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for ((tool, _), c) in &patterns {
        let slot = patterns_by_tool.entry(tool.clone()).or_default();
        slot.0 += 1;
        slot.1 += c;
    }
    let multi_command_calls: usize = occurrences_per_call.iter().filter(|(k, _)| *k > 1).map(|(_, n)| n).sum();

    let parse_rate_calls = parsed_calls as f64 / total_calls.max(1) as f64;
    let parse_rate_unique = parsed_unique as f64 / analyses.len().max(1) as f64;
    let runnable_share = runnable_calls as f64 / parsed_calls.max(1) as f64;
    let sophisticated = ["pipeline", "&&", "$( ) substitution", "heredoc", "for", "while", "if", "[[ ]]", "||"];
    let sophisticated_share: BTreeMap<&str, f64> = sophisticated
        .iter()
        .map(|f| (*f, round4(feature_calls.get(*f).copied().unwrap_or(0) as f64 / total_calls.max(1) as f64)))
        .collect();

    let mut result = ExperimentResult::new("e08-agent-corpus", "Corpus real de comandos de agente");
    result.metrics = json!({
        "transcript_files": collected.files,
        "projects": projects.len(),
        "bash_calls": total_calls,
        "bash_calls_from_subagents": subagent_calls,
        "unique_commands": analyses.len(),
        "bad_transcript_lines": collected.bad_lines,
        "parse": {
            "rate_calls": round4(parse_rate_calls),
            "rate_unique": round4(parse_rate_unique),
            "errors_by_kind_unique": parse_errors,
        },
        "simple_command_occurrences": total_occ,
        "simple_commands_per_call_mean": round4(total_occ as f64 / parsed_calls.max(1) as f64),
        "calls_with_more_than_one_command": round4(multi_command_calls as f64 / parsed_calls.max(1) as f64),
        "occurrences_inside_substitution": in_subst_occ,
        "distinct_command_names": real.len(),
        "coverage_distinct_names": {"p50": n50, "p90": n90, "p95": n95, "p99": n99},
        "top_commands": top_commands,
        "top_flags": top_flags,
        "wrappers": via_counts,
        "features": feature_share,
        "max_pipeline_length_hist": pipeline_hist,
        "runnable_under_plan_share": round4(runnable_share),
        "absent_calls_by_group": absent_calls_by_group,
        "absent_top": absent_ranked,
        "patterns_by_tool": patterns_by_tool.iter().map(|(t, (u, c))| (t.clone(), json!({"unique": u, "occurrences": c}))).collect::<BTreeMap<_, _>>(),
    });

    let concentrated = n95 <= 60;
    let parses = parse_rate_calls >= 0.95;
    let verdict = match (concentrated, parses) {
        (true, true) => Verdict::Confirmed,
        (false, false) => Verdict::Refuted,
        _ => Verdict::Partial,
    };
    let pipeline_share = sophisticated_share.get("pipeline").copied().unwrap_or(0.0);
    let andand_share = sophisticated_share.get("&&").copied().unwrap_or(0.0);
    let subst_share = sophisticated_share.get("$( ) substitution").copied().unwrap_or(0.0);
    let heredoc_share = sophisticated_share.get("heredoc").copied().unwrap_or(0.0);
    result.hypothesis(
        "H22",
        verdict,
        format!(
            "{total_calls} chamadas Bash reais ({} únicas) em {} projetos. {n90} nomes de comando cobrem 90% das ocorrências, {n95} cobrem 95% e {n99} cobrem 99% (de {} distintos). O brush-parser aceita {:.2}% das chamadas. Recursos: pipeline em {:.1}%, && em {:.1}%, $( ) em {:.1}%, heredoc em {:.1}%. {:.1}% das chamadas usariam só ferramentas do plano v2.",
            analyses.len(),
            projects.len(),
            real.len(),
            parse_rate_calls * 100.0,
            pipeline_share * 100.0,
            andand_share * 100.0,
            subst_share * 100.0,
            heredoc_share * 100.0,
            runnable_share * 100.0,
        ),
        json!({
            "coverage_distinct_names": {"p90": n90, "p95": n95, "p99": n99},
            "parse_rate_calls": round4(parse_rate_calls),
            "sophisticated_feature_share": sophisticated_share,
            "runnable_under_plan_share": round4(runnable_share),
        }),
    );
    result.notes.push(
        "Comandos e padrões completos ficam só em testbench/corpus/agent/ (gitignored) e nunca são executados; aqui só entram agregados, com nomes de script local normalizados pra <path-script>.".into(),
    );
    result.notes.push(format!(
        "Critério: confirmada se até 60 nomes cobrem 95% das ocorrências e o brush-parser aceita pelo menos 95% das chamadas. Medido: {n95} nomes e {:.1}%.",
        parse_rate_calls * 100.0
    ));
    let path = result.write()?;
    eprintln!("gravado {}", path.display());
    println!("{}", serde_json::to_string_pretty(&result.hypotheses)?);
    Ok(())
}

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}
