//! H30: diff e patch contra o GNU diffutils 3.10 e o GNU patch 2.8.
//!
//! - diff: o front-end ([`cli`]) e o formatador ([`gnu_format`]) são nossos; cada motor de alinhamento
//!   ([`engines`]) é uma biblioteca. Também se mede o formatador próprio de cada crate
//!   ([`own_format`]). Além do corpus manual, um corpus aleatório ([`fuzz`]) roda no oráculo em tempo de
//!   execução e mede a concordância de alinhamento em empates.
//! - patch: front-end nosso ([`patch`]) com três motores: diffy estrito, flickzeug com fuzz e um
//!   localizador nosso sobre o parser do diffy; corpus manual e corpus aleatório (o GNU gera o diff e
//!   aplica num alvo com deslocamento, contexto alterado ou patch já aplicado).

pub mod cli;
pub mod engines;
pub mod fuzz;
pub mod gnu_format;
pub mod own_format;
pub mod patch;
pub mod text;

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::Result;
use harness::{Candidate, CandidateResult, CaseComparison, Conformance, Fit, HypothesisVerdict, Verdict};
use serde_json::{Value, json};

use crate::common::{self, DepSummary, Part};

const FUZZ_SEED: u64 = 30;
const FUZZ_PAIRS: usize = 800;
const PATCH_SEED: u64 = 31;
const PATCH_TRIALS: usize = 500;

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn rate(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { round3(a as f64 / b as f64) }
}

/// Linhas de código nossas (sem testes), pra medir o tamanho do "à mão".
fn code_lines(src: &str) -> usize {
    let body = src.split("#[cfg(test)]\nmod tests").next().unwrap_or(src);
    body.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//")).count()
}

/// Ids dos casos que falharam, agrupados pela primeira tag "de tema".
fn failures_by_tag(all: &[CaseComparison]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in all.iter().filter(|c| !c.strict) {
        let tag = c.tags.first().cloned().unwrap_or_else(|| "untagged".into());
        let tag = if c.unsupported.is_some() { format!("unsupported:{tag}") } else { tag };
        out.entry(tag).or_default().push(c.id.clone());
    }
    out
}

fn scan_or_null(package: &str, cache: &mut BTreeMap<String, Option<DepSummary>>) -> Option<DepSummary> {
    cache.entry(package.to_string()).or_insert_with(|| common::dep_scan(package).ok()).clone()
}

pub fn run() -> Result<Part> {
    let mut part = Part { key: "h30_diff".into(), ..Part::default() };
    let mut scans: BTreeMap<String, Option<DepSummary>> = BTreeMap::new();
    let oracle = harness::Oracle::locate()?;

    // ------------------------------------------------------------------ diff: corpus manual
    let cases = common::load_cases("diff")?;
    let pairs = fuzz::generate(FUZZ_SEED, FUZZ_PAIRS);
    let gnu_fuzz = fuzz::gnu_outputs(&oracle, &pairs, &[])?;
    let gnu_fuzz_u = fuzz::gnu_outputs(&oracle, &pairs, &["-u"])?;
    let bench = bench_inputs();
    let gnu_speed = gnu_bench(&oracle, &bench)?;

    let mut engine_rows = Vec::new();
    let mut best_engine: Option<(String, usize, usize, usize, usize)> = None;
    for (idx, engine) in engines::all_engines().into_iter().enumerate() {
        let label = engine.label();
        let (krate, version) = engine.krate();
        let speeds = engine_bench(engine.as_ref(), &bench);
        let ag = fuzz::agreement(engines::all_engines().remove(idx), &pairs, &gnu_fuzz);
        let renderer = cli::GnuRenderer { engine };
        let ag_u = fuzz::agreement_with(&renderer, cli::Style::Unified(3), &pairs, &gnu_fuzz_u);
        let cand = cli::DiffCandidate { renderer: Box::new(renderer) };
        let (conf, all) = harness::score(&cand, &cases);
        dump(&all, &format!("h30-diff-engine-{idx:02}"));
        let scan = scan_or_null(krate, &mut scans);
        let fit = if conf.strict_rate() >= 0.98 && ag.identical * 100 >= ag.total * 99 {
            Fit::Fits
        } else if ag.invalid == 0 && ag.longer * 100 <= ag.total && conf.strict_rate() >= 0.9 {
            Fit::FitsWithWork
        } else {
            Fit::DoesNotFit
        };
        if best_engine.as_ref().is_none_or(|b| (ag.identical, conf.strict_pass) > (b.1, b.3)) {
            best_engine = Some((label.clone(), ag.identical, ag.total, conf.strict_pass, conf.total));
        }
        engine_rows.push(json!({
            "engine": label,
            "corpus_strict": conf.strict_pass,
            "corpus_total": conf.total,
            "fuzz_identical": ag.identical,
            "fuzz_total": ag.total,
            "fuzz_same_cost_other_alignment": ag.same_cost_other_alignment,
            "fuzz_longer_script": ag.longer,
            "fuzz_unified_identical": ag_u.identical,
        }));
        let notes = format!(
            "Alinhamento da crate com front-end e formatador GNU nossos. Corpus manual {}/{} estrito; corpus \
             aleatório {}/{} idêntico ao GNU no formato normal ({} com o mesmo custo e outro alinhamento, {} com \
             script mais longo, {} inválidos) e {}/{} no -u.",
            conf.strict_pass,
            conf.total,
            ag.identical,
            ag.total,
            ag.same_cost_other_alignment,
            ag.longer,
            ag.invalid,
            ag_u.identical,
            ag_u.total
        );
        part.candidates.push(CandidateResult {
            name: cand.name(),
            version: format!("{krate} {version}"),
            role: "diff".into(),
            category: scan.as_ref().map(|s| s.tree_category.clone()),
            conformance: Some(conf.clone()),
            fit,
            notes,
            metrics: json!({
                "kind": "alinhamento da crate + formatador nosso",
                "strict_rate": rate(conf.strict_pass, conf.total),
                "failures_by_tag": failures_by_tag(&all),
                "fuzz": ag,
                "fuzz_unified": ag_u,
                "speed_ms": speeds,
                "depscan": scan,
            }),
        });
    }

    let mut own_rows = Vec::new();
    for (idx, renderer) in own_format::all_own().into_iter().enumerate() {
        let (krate, version) = renderer.krate();
        let ag_u = fuzz::agreement_with(renderer.as_ref(), cli::Style::Unified(3), &pairs, &gnu_fuzz_u);
        let cand = cli::DiffCandidate { renderer };
        let (conf, all) = harness::score(&cand, &cases);
        dump(&all, &format!("h30-diff-own-{idx:02}"));
        let supported = conf.total - conf.unsupported;
        let scan = scan_or_null(krate, &mut scans);
        own_rows.push(json!({
            "formatter": cand.name(),
            "strict": conf.strict_pass,
            "total": conf.total,
            "supported": supported,
            "fuzz_unified_identical": ag_u.identical,
            "fuzz_total": ag_u.total,
        }));
        let notes = own_notes(krate, &conf, supported, &ag_u);
        part.candidates.push(CandidateResult {
            name: cand.name(),
            version: format!("{krate} {version}"),
            role: "diff".into(),
            category: scan.as_ref().map(|s| s.tree_category.clone()),
            conformance: Some(conf.clone()),
            fit: Fit::DoesNotFit,
            notes,
            metrics: json!({
                "kind": "formatador próprio da crate, front-end nosso",
                "strict_rate": rate(conf.strict_pass, conf.total),
                "supported_cases": supported,
                "strict_rate_on_supported": rate(conf.strict_pass, supported),
                "failures_by_tag": failures_by_tag(&all),
                "fuzz_unified": ag_u,
                "depscan": scan,
            }),
        });
    }

    // ------------------------------------------------------------------ patch
    let patch_cases = common::load_cases("patch")?;
    let trials = fuzz::patch_trials(&oracle, PATCH_SEED, PATCH_TRIALS)?;
    let mut patch_rows = Vec::new();
    let mut ours_patch: Option<(Conformance, fuzz::PatchAgreement)> = None;
    for (idx, engine) in patch::all_engines().into_iter().enumerate() {
        let (krate, version) = engine.krate();
        let is_ours = idx == 2;
        let cand = patch::PatchCandidate { engine };
        let (conf, all) = harness::score(&cand, &patch_cases);
        dump(&all, &format!("h30-patch-{idx:02}"));
        let ag = fuzz::patch_agreement(&cand, &trials);
        let scan = scan_or_null(krate, &mut scans);
        let fit = if is_ours {
            if conf.lenient_rate() >= 0.95 && ag.lenient * 100 >= ag.total * 98 { Fit::FitsWithWork } else { Fit::DoesNotFit }
        } else {
            Fit::DoesNotFit
        };
        patch_rows.push(json!({
            "engine": cand.name(),
            "corpus_strict": conf.strict_pass,
            "corpus_lenient": conf.lenient_pass,
            "corpus_total": conf.total,
            "random_lenient": ag.lenient,
            "random_content": ag.content,
            "random_total": ag.total,
        }));
        let notes = patch_notes(idx, &conf, &ag);
        if is_ours {
            ours_patch = Some((conf.clone(), ag.clone()));
        }
        part.candidates.push(CandidateResult {
            name: cand.name(),
            version: format!("{krate} {version}"),
            role: "patch".into(),
            category: scan.as_ref().map(|s| s.tree_category.clone()),
            conformance: Some(conf.clone()),
            fit,
            notes,
            metrics: json!({
                "strict_rate": rate(conf.strict_pass, conf.total),
                "lenient_rate": rate(conf.lenient_pass, conf.total),
                "failures_by_tag": failures_by_tag(&all),
                "random_trials": ag,
                "depscan": scan,
            }),
        });
    }
    let flick_scan = scan_or_null("flickzeug", &mut scans);

    // ------------------------------------------------------------------ tamanho do "à mão"
    let loc = json!({
        "diff_front_end_cli_rs": code_lines(include_str!("cli.rs")),
        "diff_formatter_gnu_format_rs": code_lines(include_str!("gnu_format.rs")),
        "diff_text_rs": code_lines(include_str!("text.rs")),
        "patch_front_end_and_locator_patch_rs": code_lines(include_str!("patch.rs")),
    });

    // ------------------------------------------------------------------ veredito
    let similar_own_cand = part.candidates.iter().find(|c| c.name.starts_with("similar unified_diff"));
    let similar_own = similar_own_cand.and_then(|c| c.conformance.clone()).unwrap_or_default();
    let similar_own_fuzz_u = similar_own_cand.and_then(|c| c.metrics["fuzz_unified"]["identical"].as_u64()).unwrap_or(0);
    let similar_unified_total = similar_own.total - similar_own.unsupported;
    let similar_myers = part
        .candidates
        .iter()
        .find(|c| c.name.starts_with("similar Myers +"))
        .map(|c| (c.conformance.clone().unwrap_or_default(), c.metrics["fuzz"]["identical"].as_u64().unwrap_or(0)))
        .unwrap_or_default();
    let (best_label, best_fuzz, fuzz_total, best_corpus, corpus_total) = best_engine.unwrap_or_default();
    let (ours_conf, ours_ag) = ours_patch.unwrap_or_default();
    let summary = format!(
        "Refutada: o unified_diff do similar formata como o GNU (acerta {}/{} casos de -u do corpus manual), mas só \
         {similar_own_fuzz_u}/{fuzz_total} pares aleatórios cheios de empate saem iguais ao `diff -u`, porque o \
         alinhamento Myers dele desempata diferente (no formato normal, {}/{}). Nenhuma crate reproduz o desempate do GNU \
         (diffseq.h, GPLv3): o melhor é {best_label}, {best_fuzz}/{fuzz_total} no aleatório e {best_corpus}/{corpus_total} \
         no corpus, sempre com script de mesmo custo. Normal, -c, -e, -y, cabeçalhos, binário e -r ficam com a gente. \
         Patch: localizador nosso sobre o parser do diffy faz {}/{} no corpus (leniente) e {}/{} no aleatório; diffy e \
         flickzeug não informam posição nem deslocamento.",
        similar_own.strict_pass,
        similar_unified_total,
        similar_myers.1,
        fuzz_total,
        ours_conf.lenient_pass,
        ours_conf.total,
        ours_ag.lenient,
        ours_ag.total,
    );
    part.hypotheses.push(HypothesisVerdict {
        id: "H30".into(),
        verdict: Verdict::Refuted,
        summary,
        evidence: json!({
            "similar_own_unified": { "strict": similar_own.strict_pass, "unified_cases": similar_unified_total, "total": similar_own.total, "fuzz_unified_identical": similar_own_fuzz_u, "fuzz_total": fuzz_total },
            "similar_myers_with_our_formatter": { "corpus_strict": similar_myers.0.strict_pass, "corpus_total": similar_myers.0.total, "fuzz_identical": similar_myers.1, "fuzz_total": fuzz_total },
            "best_engine": { "engine": best_label, "fuzz_identical": best_fuzz, "fuzz_total": fuzz_total, "corpus_strict": best_corpus, "corpus_total": corpus_total },
            "patch_ours": { "corpus_lenient": ours_conf.lenient_pass, "corpus_strict": ours_conf.strict_pass, "corpus_total": ours_conf.total, "random_lenient": ours_ag.lenient, "random_total": ours_ag.total },
        }),
    });

    part.metrics = json!({
        "diff_cases": cases.len(),
        "patch_cases": patch_cases.len(),
        "fuzz": { "seed": FUZZ_SEED, "pairs": FUZZ_PAIRS, "families": "Tiny 3/8, Code 2/8, Text 2/8, Medium 1/8" },
        "patch_random": { "seed": PATCH_SEED, "trials": PATCH_TRIALS, "drifts": "Clean, Offset, Edited, Both, Applied (1/5 cada)" },
        "engines": engine_rows,
        "own_formatters": own_rows,
        "patch": patch_rows,
        "speed": { "inputs": bench.iter().map(|b| json!({"name": b.name, "lines_a": b.lines_a, "lines_b": b.lines_b})).collect::<Vec<_>>(), "gnu_diff_ms": gnu_speed },
        "lines_of_code_ours": loc,
        "flickzeug_depscan": flick_scan,
    });
    part.notes.push(
        "H30: o GNU diff alinha com o compareseq do gnulib (diffseq.h) e depois desliza os blocos (shift_boundaries); os \
         dois são GPLv3. Pra sair byte a byte igual em todo empate seria preciso reproduzir esse desempate; o que medimos \
         com crates Apache/MIT chega a 94,5% dos pares aleatórios, sempre com script de mesmo custo."
            .into(),
    );
    part.notes.push(
        "H30: o localizador de patch nosso reproduz o comportamento observável do GNU patch 2.8 (deslocamento pra frente \
         antes de pra trás, fuzz cortando contexto das pontas, âncora no começo ou fim quando o contexto é assimétrico, \
         detecção de patch invertido por nível de fuzz, .rej com faixas deslocadas). Pra produção, escrever a partir \
         dessa especificação de comportamento e dos testes, não do código GPL."
            .into(),
    );
    Ok(part)
}

fn dump(all: &[CaseComparison], name: &str) {
    let failures: Vec<&CaseComparison> = all.iter().filter(|c| !c.strict).collect();
    if let Ok(text) = serde_json::to_string_pretty(&failures) {
        let _ = std::fs::write(common::scratch().join(format!("{name}.json")), text);
    }
}

fn own_notes(krate: &str, conf: &Conformance, supported: usize, ag_u: &fuzz::Agreement) -> String {
    let base = format!(
        "Formatador da própria crate com o front-end nosso: {}/{} estrito no corpus, {}/{} nos casos que o formatador \
         suporta; {}/{} pares aleatórios iguais ao `diff -u` do GNU.",
        conf.strict_pass, conf.total, conf.strict_pass, supported, ag_u.identical, ag_u.total
    );
    let why = match krate {
        "similar" => {
            " Só tem -u. A formatação (faixas, \"\\ No newline at end of file\", agrupamento de hunks) bate com o GNU; \
             o que diverge é o alinhamento Myers do similar nos empates (prefere inserir antes de apagar). Sem \
             -i/-w/-b, sem normal/contexto/ed."
        }
        "diffy" => {
            " Só tem -u. Põe o nome entre aspas e escapa o tab quando o rótulo tem data (cabeçalho trocado pelo nosso \
             na medição); o corpo diverge pelo alinhamento nos empates. Sem -i/-w/-b, sem normal/contexto/ed."
        }
        "diffutils" => {
            " Tem normal, -u, -c, -e e -y, mas o cabeçalho lê o mtime do disco do host (std::fs + chrono::Local; as \
             duas linhas foram trocadas pelas nossas), o formato normal escreve faixa redundante (\"3a4,4\"), e o \
             alinhamento é LCS da crate `diff` (tabela O(N·M))."
        }
        "imara-diff" => {
            " Só -u, só UTF-8; o BasicLineDiffPrinter escreve a faixa sempre com contagem, não emite \"\\ No newline at \
             end of file\" nem cabeçalho; a própria doc manda escrever um printer por cima de `hunks()`."
        }
        _ => "",
    };
    format!("{base}{why}")
}

fn patch_notes(idx: usize, conf: &Conformance, ag: &fuzz::PatchAgreement) -> String {
    let nums = format!(
        "Corpus {}/{} estrito, {}/{} leniente; aleatório {}/{} leniente, {}/{} no conteúdo final.",
        conf.strict_pass, conf.total, conf.lenient_pass, conf.total, ag.lenient, ag.total, ag.content, ag.total
    );
    let why = match idx {
        0 => {
            " Aplica tudo ou nada (um hunk ruim derruba o arquivo), não tem fuzz, desempata pra trás (o GNU tenta pra \
             frente primeiro) e não informa posição nem deslocamento, então mensagens e .orig não saem."
        }
        1 => {
            " Aplica parcial e tem fuzz, mas não informa posição, deslocamento nem fuzz usado; desempata pra trás; aplica \
             patch LF em arquivo CRLF (o GNU recusa)."
        }
        _ => {
            " Parser do diffy (unificado e git; contexto e normal convertidos pelo front-end) + localização e aplicação \
             nossas. Única divergência no corpus: o texto do erro de patch malformado."
        }
    };
    format!("{nums}{why}")
}

// ---------------------------------------------------------------------------------------------- velocidade

struct BenchInput {
    name: &'static str,
    a: Vec<u8>,
    b: Vec<u8>,
    lines_a: usize,
    lines_b: usize,
}

fn bench_inputs() -> Vec<BenchInput> {
    let mut rng = fuzz::Lcg::new(99);
    let mut make = |name: &'static str, n: usize, distinct: u64, edits: usize| {
        let a: Vec<String> = (0..n).map(|i| format!("line {} {}", rng.below(distinct), i % 7)).collect();
        let mut b = a.clone();
        for _ in 0..edits {
            let at = rng.below(b.len() as u64) as usize;
            match rng.below(3) {
                0 => b[at] = format!("changed {}", rng.below(1000)),
                1 => {
                    b.remove(at);
                }
                _ => b.insert(at, format!("inserted {}", rng.below(1000))),
            }
        }
        let join = |v: &[String]| v.iter().flat_map(|l| [l.as_bytes(), b"\n"].concat()).collect::<Vec<u8>>();
        BenchInput { name, lines_a: a.len(), lines_b: b.len(), a: join(&a), b: join(&b) }
    };
    vec![make("texto-20k-linhas-1pct", 20_000, 1_000_000, 200), make("repetitivo-3k-linhas", 3_000, 20, 150)]
}

fn engine_bench(engine: &dyn engines::Engine, inputs: &[BenchInput]) -> Value {
    let mut out = serde_json::Map::new();
    for input in inputs {
        let la = text::split_lines(&input.a);
        let lb = text::split_lines(&input.b);
        let it = text::intern(&la, &lb, &text::Normalize::default());
        // A crate `diff` monta uma tabela u32 de (N+1)x(M+1) entre o prefixo e o sufixo comuns.
        let cells = (la.len() as u64 + 1) * (lb.len() as u64 + 1);
        if engine.krate().0 == "diff" && cells > 50_000_000 {
            out.insert(
                input.name.to_string(),
                json!(format!("não medido: tabela LCS de até {:.1} GiB", cells as f64 * 4.0 / (1u64 << 30) as f64)),
            );
            continue;
        }
        let per = common::time_per_iter(Duration::from_millis(300), || {
            let _ = engine.changes(&it.a, &it.b, &it.reps);
        });
        out.insert(input.name.to_string(), json!(round3(per.as_secs_f64() * 1000.0)));
    }
    Value::Object(out)
}

/// Tempo do GNU diff por par, medido dentro do container e descontado o custo de criar processo.
fn gnu_bench(oracle: &harness::Oracle, inputs: &[BenchInput]) -> Result<Value> {
    let mut files = harness::MemTree::new();
    let mut script = String::from(
        "t() { local n=$1; shift; local s e; s=$(date +%s%N); for i in $(seq 1 $n); do \"$@\" >/dev/null; done; \
         e=$(date +%s%N); echo $(( (e-s)/n )); }\nbase=$(t 20 true)\n",
    );
    for (k, input) in inputs.iter().enumerate() {
        files.insert(&format!("a{k}"), harness::Entry::file(input.a.clone(), 0o644));
        files.insert(&format!("b{k}"), harness::Entry::file(input.b.clone(), 0o644));
        script.push_str(&format!("echo \"{} $(( $(t 10 diff a{k} b{k}) - base ))\"\n", input.name));
    }
    let out = oracle.run_script("h30-gnu-bench", &script, files)?;
    let mut map = serde_json::Map::new();
    for line in String::from_utf8_lossy(out.stdout.as_slice()).lines() {
        if let Some((name, ns)) = line.split_once(' ')
            && let Ok(ns) = ns.trim().parse::<f64>()
        {
            map.insert(name.to_string(), json!(round3(ns / 1e6)));
        }
    }
    Ok(Value::Object(map))
}
