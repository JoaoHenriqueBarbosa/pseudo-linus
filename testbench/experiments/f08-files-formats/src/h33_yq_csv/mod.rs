//! H33 (parte 2): yq e csv.
//!
//! - yq: o oráculo é o yq 3.4.3 do kislyuk (PyYAML + jq 1.7.1). O front-end (argumentos, conversão
//!   YAML -> JSON com a semântica do Python, impressão do jq) é nosso e igual pra todos; mudam a camada
//!   YAML (jaq-fmts, saphyr, serde-saphyr, yaml-rust2, noyalib, e uma camada nossa sobre o
//!   saphyr-parser com o resolvedor do yq) e, num candidato, o motor do filtro (yqr). Métrica
//!   separada: taxa de parse correto de cada parser na YAML test suite.
//! - csv: crate `csv` contra `cut`/`column` (CSV sem aspas) e contra o módulo `csv` do Python 3.13
//!   (RFC 4180), que é a única referência de CSV dentro do oráculo.

pub mod csv_front;
pub mod jqout;
pub mod yaml_layers;
pub mod yaml_suite;
pub mod yq_front;
pub mod yq_resolver;

use std::collections::BTreeMap;

use anyhow::Result;
use harness::{CandidateResult, Case, CaseComparison, Conformance, Fit, Outcome};
use serde_json::{Value, json};

use crate::common::{self, Part, RoleSummary};
use yq_front::{Engine, YqCandidate};

/// Tags de formas escalares em que YAML 1.1 e 1.2 divergem, e quirks do Python.
const YAML11_TAGS: &[&str] = &["yaml11", "python-quirk"];
/// Tags de saída YAML (emissor).
const YAML_OUT_TAGS: &[&str] = &["yaml-out", "yaml-roundtrip"];

pub fn run() -> Result<Part> {
    let mut part = Part { key: "h33_yq_csv".into(), ..Part::default() };
    let mut metrics = serde_json::Map::new();

    let (yq_metrics, yq_role) = run_yq(&mut part)?;
    metrics.insert("yq".into(), yq_metrics);
    part.roles.push(yq_role);

    let (csv_metrics, csv_role) = run_csv(&mut part)?;
    metrics.insert("csv".into(), csv_metrics);
    part.roles.push(csv_role);

    part.notes.push(
        "H33/yq: o carregador do yq 3.4.3 não é YAML 1.1 puro: resolve escalares pelas regex do core schema 1.2 \
         (yes/no/on/off, 0b101, 1_000, 1:20 e datas ficam string), mas constrói inteiro com o construtor 1.1 do PyYAML \
         (012 = 10, 08 dá ValueError) e expande merge keys na ordem do flatten_mapping. Nenhuma crate reproduz essa \
         mistura; o resolvedor nosso sobre o saphyr-parser reproduz."
            .into(),
    );
    part.notes.push(
        "H33/yq: o depscan acusa defmt 1.1.1 como C na árvore do jaq-fmts; é falso positivo (o `links = \"defmt\"` é \
         namespace de linker, não C, e a crate nem entra no build: `cargo tree --target all -i defmt` vazio). jaq-core \
         3.1.1 toca o host em 3 pontos: `unwrap_valr` chama std::process::exit no halt (o front-end trata o halt sem ele) \
         e o loader de módulos usa std::env/std::fs só via `with_std_read` (não usado)."
            .into(),
    );
    part.notes.push(
        "H33/csv: não há ferramenta GNU de CSV no oráculo. Referências: cut e column (util-linux) em CSV sem aspas, e o \
         módulo csv do Python 3.13 (dependência do yq no oráculo) pra RFC 4180. Casos em que cut/column divergem de CSV \
         por definição (CRLF, aspas) ficam com a tag `semantics`."
            .into(),
    );

    part.metrics = Value::Object(metrics);
    Ok(part)
}

/// Pontua e devolve placar + comparações (pra classificar as divergências).
fn score_all(candidate: &dyn harness::Candidate, cases: &[(Case, Outcome)], dump: &str) -> (Conformance, Vec<CaseComparison>) {
    let (conf, all) = harness::score(candidate, cases);
    let failures: Vec<&CaseComparison> = all.iter().filter(|c| !c.strict).collect();
    if let Ok(text) = serde_json::to_string_pretty(&failures) {
        let _ = std::fs::write(common::scratch().join(format!("{dump}.json")), text);
    }
    (conf, all)
}

/// Classe da divergência de um caso que falhou no modo estrito.
fn classify_yq(cmp: &CaseComparison) -> &'static str {
    let has = |set: &[&str]| cmp.tags.iter().any(|t| set.contains(&t.as_str()));
    if cmp.unsupported.is_some() {
        "unsupported"
    } else if has(YAML_OUT_TAGS) {
        "yaml-output"
    } else if cmp.lenient {
        "error-message"
    } else if has(&["jq-engine"]) {
        "jq-engine"
    } else if has(YAML11_TAGS) {
        "yaml11-forms"
    } else if has(&["merge"]) {
        "merge-key"
    } else if has(&["number"]) {
        "number"
    } else if has(&["error"]) {
        "error-exit"
    } else {
        "other"
    }
}

fn rate(pass: usize, total: usize) -> f64 {
    if total == 0 { 0.0 } else { (pass as f64 / total as f64 * 1000.0).round() / 1000.0 }
}

/// (estrito, leniente, total) num subconjunto de casos.
fn subset_rate(all: &[CaseComparison], keep: impl Fn(&CaseComparison) -> bool) -> (usize, usize, usize) {
    let sel: Vec<&CaseComparison> = all.iter().filter(|c| keep(c)).collect();
    (sel.iter().filter(|c| c.strict).count(), sel.iter().filter(|c| c.lenient).count(), sel.len())
}

fn dep_metrics(package: &str) -> (Option<String>, Value) {
    match common::dep_scan(package) {
        Ok(d) => {
            let cat = d.tree_category.clone();
            (Some(cat), serde_json::to_value(&d).unwrap_or(Value::Null))
        }
        Err(e) => (None, json!({"error": format!("{e:#}")})),
    }
}

/// Linhas de código (sem testes, comentários e linhas vazias) do resolvedor nosso: o tamanho do
/// "trabalho" que a camada YAML exige.
fn resolver_code_lines() -> usize {
    let src = include_str!("yq_resolver.rs");
    let code = src.split("#[cfg(test)]").next().unwrap_or(src);
    code.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//")).count()
}

/// Resultado resumido de um candidato yq.
struct YqScore {
    name: String,
    own: bool,
    engine: Engine,
    strict: f64,
    core_lenient: f64,
    yaml11: (usize, usize, usize),
    yaml_out: (usize, usize, usize),
    conf: Conformance,
}

fn run_yq(part: &mut Part) -> Result<(Value, RoleSummary)> {
    let cases = common::load_cases("yq")?;
    let mut candidates: Vec<(YqCandidate, bool)> = yaml_layers::all_layers()
        .into_iter()
        .map(|layer| (YqCandidate { layer, engine: Engine::Jaq }, false))
        .collect();
    candidates.push((YqCandidate { layer: Box::new(yq_resolver::YqResolverLayer), engine: Engine::Jaq }, true));
    candidates.push((YqCandidate { layer: Box::new(yaml_layers::NoyalibLayer), engine: Engine::Yqr }, false));

    let (_, jaq_scan) = dep_metrics("jaq-core");
    let mut rows = Vec::new();
    let mut scores: Vec<YqScore> = Vec::new();
    for (cand, own) in &candidates {
        let name = cand.display_name();
        eprintln!("h33 yq: {name}");
        let dump = format!("yq-{}", slug(&name));
        let (conf, all) = score_all(cand, &cases, &dump);
        let mut classes: BTreeMap<&str, usize> = BTreeMap::new();
        for cmp in all.iter().filter(|c| !c.strict) {
            *classes.entry(classify_yq(cmp)).or_default() += 1;
        }
        let has = |c: &CaseComparison, set: &[&str]| c.tags.iter().any(|t| set.contains(&t.as_str()));
        // "Núcleo": tudo menos formas 1.1/quirks do Python, saída YAML e casos de erro.
        let core = subset_rate(&all, |c| !has(c, YAML11_TAGS) && !has(c, YAML_OUT_TAGS) && !has(c, &["error"]));
        let yaml11 = subset_rate(&all, |c| has(c, YAML11_TAGS));
        let yaml_out = subset_rate(&all, |c| has(c, YAML_OUT_TAGS));
        let errors = subset_rate(&all, |c| has(c, &["error"]));
        let (package, version) = match cand.engine {
            Engine::Jaq => (cand.layer.package(), cand.layer.version()),
            Engine::Yqr => ("yqr", "0.8.1"),
        };
        let (category, scan) = dep_metrics(package);
        let core_lenient = rate(core.1, core.2);
        let failing: Vec<String> = all.iter().filter(|c| !c.strict).map(|c| format!("{} [{}]", c.id, classify_yq(c))).collect();
        let mut metrics = json!({
            "strict_rate": rate(conf.strict_pass, conf.total),
            "lenient_rate": rate(conf.lenient_pass, conf.total),
            "core": {"strict": core.0, "lenient": core.1, "total": core.2},
            "yaml11_forms": {"strict": yaml11.0, "lenient": yaml11.1, "total": yaml11.2},
            "yaml_output": {"strict": yaml_out.0, "lenient": yaml_out.1, "total": yaml_out.2},
            "errors": {"strict": errors.0, "lenient": errors.1, "total": errors.2},
            "divergence_classes": classes,
            "failing": failing,
            "depscan": scan,
        });
        if *own {
            metrics["resolver_code_lines"] = json!(resolver_code_lines());
        }
        let notes = yq_notes(cand, *own, &classes);
        rows.push(json!({
            "candidate": name,
            "strict": conf.strict_pass,
            "lenient": conf.lenient_pass,
            "total": conf.total,
            "core_lenient": core_lenient,
            "yaml11_forms_strict": yaml11.0,
            "yaml_output_strict": yaml_out.0,
        }));
        let fit = match (cand.engine, own) {
            (Engine::Yqr, _) => Fit::DoesNotFit,
            (_, true) => Fit::FitsWithWork,
            _ if core_lenient >= 0.9 => Fit::FitsWithWork,
            _ => Fit::DoesNotFit,
        };
        scores.push(YqScore {
            name: name.clone(),
            own: *own,
            engine: cand.engine,
            strict: rate(conf.strict_pass, conf.total),
            core_lenient,
            yaml11,
            yaml_out,
            conf: conf.clone(),
        });
        part.candidates.push(CandidateResult {
            name,
            version: version.to_string(),
            role: "yq".into(),
            category,
            conformance: Some(conf),
            fit,
            notes,
            metrics,
        });
    }

    let suite = yaml_suite::run_suite();
    let role = yq_role(&scores);
    let metrics = json!({
        "cases": cases.len(),
        "candidates": rows,
        "jaq_core_depscan": jaq_scan,
        "yaml_test_suite": suite,
        "oracle_loader_finding": "o yq 3.4.3 resolve escalares pelas regex do core schema do YAML 1.2 (yes/no, 0b, 1_000, 1:20 e datas ficam string) mas constrói inteiros com o construtor 1.1 do PyYAML (012 = 10, 08 dá ValueError) e expande merge keys na ordem do flatten_mapping",
    });
    Ok((metrics, role))
}

fn yq_role(scores: &[YqScore]) -> RoleSummary {
    let crates: Vec<&YqScore> = scores.iter().filter(|s| !s.own && s.engine == Engine::Jaq).collect();
    let best = crates.iter().max_by(|a, b| {
        (a.core_lenient, a.strict).partial_cmp(&(b.core_lenient, b.strict)).unwrap_or(std::cmp::Ordering::Equal)
    });
    let own = scores.iter().find(|s| s.own);
    let best_out = scores.iter().map(|s| s.yaml_out.0).max().unwrap_or(0);
    let out_total = scores.first().map(|s| s.yaml_out.2).unwrap_or(0);
    match (best, own) {
        (Some(b), Some(o)) if o.core_lenient >= 0.9 => RoleSummary {
            role: "yq".into(),
            best: "jaq 3 + saphyr-parser 0.1 com resolvedor yq nosso; emissor -y nosso".into(),
            fit: Fit::FitsWithWork,
            summary: format!(
                "melhor camada pronta ({}) faz {}/{} estrito e {:.0}% leniente no núcleo; com o resolvedor nosso \
                 ({} linhas) sobre o saphyr-parser sobe pra {}/{} estrito, {}/{} nas formas 1.1/Python; \
                 nenhum emissor de crate reproduz o -y do PyYAML (melhor {}/{}).",
                b.name,
                b.conf.strict_pass,
                b.conf.total,
                b.core_lenient * 100.0,
                resolver_code_lines(),
                o.conf.strict_pass,
                o.conf.total,
                o.yaml11.0,
                o.yaml11.2,
                best_out,
                out_total
            ),
        },
        (Some(b), _) => RoleSummary {
            role: "yq".into(),
            best: "fazer à mão".into(),
            fit: Fit::DoesNotFit,
            summary: format!("nenhuma camada passa de 90% leniente no núcleo (melhor {}: {:.0}%).", b.name, b.core_lenient * 100.0),
        },
        _ => RoleSummary { role: "yq".into(), best: "fazer à mão".into(), fit: Fit::DoesNotFit, summary: "sem candidatos".into() },
    }
}

fn yq_notes(cand: &YqCandidate, own: bool, classes: &BTreeMap<&str, usize>) -> String {
    let cls: Vec<String> = classes.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let base = match (cand.engine, own) {
        (Engine::Yqr, _) => {
            "Motor jq próprio do yqr (subconjunto, sem aritmética nem --arg) sobre noyalib; o front-end imprime JSON \
             (o CLI do yqr emite YAML)."
        }
        (_, true) => {
            "É o trabalho medido: eventos do saphyr-parser + resolvedor/construtor do yq 3.4.3 (core schema 1.2, inteiro do \
             PyYAML, merge key do flatten_mapping, BOM) escrito por nós; -y pelo emissor do saphyr."
        }
        _ => "Filtro jaq-core/std/json 3.x; front-end e impressão jq nossos; camada YAML e emissor -y da crate.",
    };
    format!("{base} Divergências estritas por classe: {}.", if cls.is_empty() { "nenhuma".into() } else { cls.join(", ") })
}

fn slug(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect::<String>()
}

fn run_csv(part: &mut Part) -> Result<(Value, RoleSummary)> {
    let cases = common::load_cases("csv")?;
    let cand = csv_front::CsvCandidate;
    let (conf, all) = score_all(&cand, &cases, "csv-csv");
    let has = |c: &CaseComparison, t: &str| c.tags.iter().any(|x| x == t);
    // "Semântica": casos onde cut/column divergem de CSV de propósito (CRLF, aspas).
    let comparable = subset_rate(&all, |c| !has(c, "semantics"));
    let by_ref = |t: &str| subset_rate(&all, |c| has(c, t));
    let cut = by_ref("cut");
    let column = by_ref("column");
    let read = by_ref("read");
    let write = by_ref("write");
    let failures: Vec<Value> = all
        .iter()
        .filter(|c| !c.strict)
        .map(|c| json!({"id": c.id, "tags": c.tags, "detail": c.detail}))
        .collect();
    let (category, scan) = dep_metrics("csv");
    let metrics = json!({
        "strict_rate": rate(conf.strict_pass, conf.total),
        "comparable": {"strict": comparable.0, "total": comparable.2},
        "cut": {"strict": cut.0, "total": cut.2},
        "column": {"strict": column.0, "total": column.2},
        "python_read": {"strict": read.0, "total": read.2},
        "python_write": {"strict": write.0, "total": write.2},
        "failures": failures,
        "no_gnu_oracle": "não existe ferramenta GNU de CSV; referências usadas: cut e column (util-linux) em CSV sem aspas e o módulo csv do Python 3.13 do oráculo pra RFC 4180",
        "depscan": scan,
    });
    let fit = if rate(comparable.0, comparable.2) >= 0.8 { Fit::Fits } else { Fit::FitsWithWork };
    let notes = format!(
        "Leitor e escritor da crate com has_headers(false), flexible(true), QuoteStyle::Necessary e terminador escolhido. \
         Sem oráculo GNU de CSV: cut/column em CSV sem aspas e Python csv pra RFC 4180. {} divergências estritas.",
        conf.total - conf.strict_pass
    );
    let summary = format!(
        "csv 1.4: {}/{} estrito no corpus ({}/{} fora dos casos em que cut/column divergem de CSV por definição); \
         cut {}/{}, column {}/{}, leitura Python {}/{}, escrita Python {}/{}.",
        conf.strict_pass, conf.total, comparable.0, comparable.2, cut.0, cut.2, column.0, column.2, read.0, read.2, write.0, write.2
    );
    part.candidates.push(CandidateResult {
        name: "csv".into(),
        version: "1.4.0".into(),
        role: "csv".into(),
        category,
        conformance: Some(conf),
        fit,
        notes,
        metrics: metrics.clone(),
    });
    let role = RoleSummary { role: "csv".into(), best: "csv 1.4".into(), fit, summary };
    Ok((metrics, role))
}
