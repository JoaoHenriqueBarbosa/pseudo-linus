//! H33 (parte 1): bc e file.
//!
//! - bc: nenhuma crate do crates.io implementa a linguagem como biblioteca. Os candidatos reais são o bc
//!   do posixutils-rs e o bc_clone_rs (motor `bc_core` no_std), clonados numa revisão fixa e compilados
//!   como binário ([`bc`]); a crate `bc` (chama o executável do sistema) e o bc do bashkit (f64) são
//!   descartados com evidência do fonte.
//! - file: front-end nosso sobre `pure-magic` + `magic-db`, `libmagic-rs` (regras embutidas e magdir) e
//!   dois detectores só de MIME ([`file`]); uma camada `ascmagic` nossa mede quanto da distância é texto.

pub mod bc;
pub mod file;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::Instant;

use anyhow::Result;
use harness::{Candidate, CandidateResult, Case, CaseComparison, Conformance, Fit, Invocation, Outcome};
use serde_json::{Value, json};

use crate::common::{self, DepSummary, Part, RoleSummary};

pub fn run() -> Result<Part> {
    let mut part = Part { key: "h33_bc_file".into(), ..Part::default() };
    let t = Instant::now();
    let bc_metrics = run_bc(&mut part)?;
    eprintln!("h33 bc: {:.1}s", t.elapsed().as_secs_f64());
    let t = Instant::now();
    let file_metrics = run_file(&mut part)?;
    eprintln!("h33 file: {:.1}s", t.elapsed().as_secs_f64());
    part.metrics = json!({ "bc": bc_metrics, "file": file_metrics });
    Ok(part)
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn ratio(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { a as f64 / b as f64 }
}

/// (strict, lenient, total) dos casos com uma tag.
fn tag_counts(conf: &Conformance, tag: &str) -> (usize, usize, usize) {
    conf.by_tag.get(tag).copied().unwrap_or((0, 0, 0))
}

/// (strict, lenient, total) num subconjunto das comparações.
fn subset(all: &[CaseComparison], keep: impl Fn(&CaseComparison) -> bool) -> (usize, usize, usize) {
    let sel: Vec<&CaseComparison> = all.iter().filter(|c| keep(c)).collect();
    (sel.iter().filter(|c| c.strict).count(), sel.iter().filter(|c| c.lenient).count(), sel.len())
}

fn has_tag(c: &CaseComparison, tags: &[&str]) -> bool {
    c.tags.iter().any(|t| tags.contains(&t.as_str()))
}

/// Candidato que guarda o que produziu em cada caso (pra classificar divergências depois).
struct Recording<'a> {
    inner: &'a dyn Candidate,
    seen: RefCell<BTreeMap<String, Outcome>>,
}

impl Candidate for Recording<'_> {
    fn name(&self) -> String {
        self.inner.name()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        let out = self.inner.run(inv);
        self.seen.borrow_mut().insert(inv.case_id.clone(), out.clone());
        out
    }
}

/// Placar, comparações caso a caso e saídas do candidato por id de caso.
type Recorded = (Conformance, Vec<CaseComparison>, BTreeMap<String, Outcome>);

/// Pontua, grava as falhas no scratch e devolve placar, comparações e saídas do candidato.
fn score_recorded(candidate: &dyn Candidate, cases: &[(Case, Outcome)], dump: &str) -> Recorded {
    let rec = Recording { inner: candidate, seen: RefCell::new(BTreeMap::new()) };
    let (conf, all) = harness::score(&rec, cases);
    let failures: Vec<&CaseComparison> = all.iter().filter(|c| !c.strict).collect();
    if let Ok(text) = serde_json::to_string_pretty(&failures) {
        let _ = std::fs::write(common::scratch().join(format!("{dump}.json")), text);
    }
    (conf, all, rec.seen.into_inner())
}

// ---------------------------------------------------------------------------------------------- bc

/// Tags do núcleo numérico: aritmética, scale, bases e math library, sem extensão GNU, erro, CLI.
const BC_NUMERIC: &[&str] = &["arith", "scale", "base", "mathlib"];
const BC_NOT_NUMERIC: &[&str] = &["gnu-ext", "error", "env", "file", "quit"];

/// Tira o zero antes do ponto ("0.5" e "-0.5"), que o GNU bc não imprime.
fn strip_leading_zero(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            if let Some(rest) = line.strip_prefix("-0.") {
                format!("-.{rest}")
            } else if let Some(rest) = line.strip_prefix("0.") {
                format!(".{rest}")
            } else {
                line.to_string()
            }
        })
        .collect()
}

/// Classe de uma divergência do bc (a primeira regra que bate).
fn classify_bc(cmp: &CaseComparison, golden: &Outcome, actual: &Outcome) -> &'static str {
    let out = String::from_utf8_lossy(actual.stdout.as_slice());
    let err = String::from_utf8_lossy(actual.stderr.as_slice());
    if actual.unsupported.is_some() {
        "unsupported"
    } else if !cmp.stdout_ok && strip_leading_zero(&out).as_bytes() == golden.stdout.as_slice() {
        "zero à esquerda (0.5 em vez de .5)"
    } else if has_tag(cmp, &["env"]) {
        "BC_LINE_LENGTH ignorado"
    } else if err.contains("unexpected argument") || err.contains("invalid option") {
        "opção de CLI ausente (-q, -s, -w)"
    } else if has_tag(cmp, &["gnu-ext"]) {
        "extensão GNU ausente ou diferente"
    } else if cmp.lenient {
        "só a mensagem de erro difere"
    } else if has_tag(cmp, &["error"]) {
        "erro: exit ou recuperação diferente"
    } else {
        "semântica"
    }
}

fn bc_fit(conf: &Conformance, numeric: (usize, usize, usize), engine_host_touch: usize) -> Fit {
    if conf.strict_rate() >= 0.98 {
        Fit::Fits
    } else if ratio(numeric.0, numeric.2) >= 0.9 && engine_host_touch == 0 {
        Fit::FitsWithWork
    } else {
        Fit::DoesNotFit
    }
}

fn run_bc(part: &mut Part) -> Result<Value> {
    let cases = common::load_cases("bc")?;
    let golden: BTreeMap<&str, &Outcome> = cases.iter().map(|(c, g)| (c.id.as_str(), g)).collect();
    let mut rows = Vec::new();
    // (nome, placar, núcleo numérico com zero corrigido, encaixe, classes, chave de escolha, panics)
    type Best = (String, Conformance, (usize, usize, usize), Fit, BTreeMap<&'static str, usize>, (usize, usize), usize);
    let mut best: Option<Best> = None;
    for up in [&bc::POSIXUTILS, &bc::BC_CLONE] {
        eprintln!("h33 bc: {}", up.label);
        let prepared = bc::ensure_checkout(up).and_then(|dir| bc::ensure_built(up, &dir).map(|bin| (dir, bin)));
        let (dir, bin) = match prepared {
            Ok(p) => p,
            Err(e) => {
                part.candidates.push(CandidateResult {
                    name: up.label.to_string(),
                    version: format!("git {}", &up.rev[..12]),
                    role: "bc".into(),
                    category: None,
                    conformance: None,
                    fit: Fit::DoesNotFit,
                    notes: format!("não foi possível clonar ou compilar: {e:#}"),
                    metrics: Value::Null,
                });
                continue;
            }
        };
        let cand = bc::ExternalBc { name: format!("{} @{}", up.label, &up.rev[..12]), path: bin };
        let (conf, all, seen) = score_recorded(&cand, &cases, &format!("bc-{}", up.dir));
        let upstream = subset(&all, |c| has_tag(c, &["upstream"]));
        let agent = subset(&all, |c| !has_tag(c, &["upstream"]));
        let numeric = subset(&all, |c| has_tag(c, BC_NUMERIC) && !has_tag(c, BC_NOT_NUMERIC));
        let gnu_ext = subset(&all, |c| has_tag(c, &["gnu-ext"]));
        let errors = subset(&all, |c| has_tag(c, &["error"]));
        let mut classes: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut panics = 0usize;
        // Casos em que a única diferença é o zero antes do ponto (stderr, exit e arquivos iguais).
        let mut zero_only = 0usize;
        let mut zero_only_numeric = 0usize;
        for cmp in all.iter().filter(|c| !c.strict) {
            let actual = seen.get(&cmp.id).cloned().unwrap_or_default();
            let class = classify_bc(cmp, golden[cmp.id.as_str()], &actual);
            *classes.entry(class).or_default() += 1;
            panics += String::from_utf8_lossy(actual.stderr.as_slice()).contains("panicked at") as usize;
            if class.starts_with("zero") && cmp.stderr_ok && cmp.exit_ok && cmp.files_ok {
                zero_only += 1;
                zero_only_numeric += (has_tag(cmp, BC_NUMERIC) && !has_tag(cmp, BC_NOT_NUMERIC)) as usize;
            }
        }
        let strict_zero_fixed = conf.strict_pass + zero_only;
        let numeric_zero_fixed = (numeric.0 + zero_only_numeric, numeric.1, numeric.2);
        let scan = depscan::scan(&dir.join("Cargo.toml"), up.package)?;
        let engine_scan = match up.engine_package {
            Some(p) => Some(depscan::scan(&dir.join("Cargo.toml"), p)?),
            None => None,
        };
        let engine = bc::source_stats(&dir, up.engine_files);
        let cli = bc::source_stats(&dir, up.cli_files);
        // O encaixe usa o núcleo numérico com o zero à esquerda corrigido: é uma linha no formatador de
        // saída do fork, e sem isso a medição confunde formato com aritmética.
        let fit = bc_fit(&conf, numeric_zero_fixed, engine.host_touch);
        let engine_category = engine_scan.as_ref().map(|s| s.tree_category.letter().to_string());
        let metrics = json!({
            "rev": up.rev,
            "strict_rate": round3(conf.strict_rate()),
            "lenient_rate": round3(conf.lenient_rate()),
            "agent_style_strict_lenient_total": agent,
            "upstream_posixutils_suite_strict_lenient_total": upstream,
            "numeric_core_strict_lenient_total": numeric,
            "numeric_core_if_leading_zero_fixed": numeric_zero_fixed,
            "strict_if_leading_zero_fixed": strict_zero_fixed,
            "gnu_extensions_strict_lenient_total": gnu_ext,
            "error_cases_strict_lenient_total": errors,
            "divergence_classes": classes,
            "cases_with_rust_panic": panics,
            "package_depscan": {
                "package": up.package,
                "own_category": scan.root.category.letter(),
                "tree_category": scan.tree_category.letter(),
                "c_deps": scan.c_deps,
                "tree_deps": scan.deps.len(),
                "host_touching_deps": scan.host_touching_deps,
            },
            "engine_package_category": engine_category,
            "engine_source": engine,
            "cli_source": cli,
        });
        let notes = bc_notes(up, &conf, numeric, numeric_zero_fixed, strict_zero_fixed, &classes, panics, &engine, engine_category.as_deref());
        rows.push(json!({
            "candidate": cand.name(),
            "strict": conf.strict_pass,
            "lenient": conf.lenient_pass,
            "total": conf.total,
            "strict_if_leading_zero_fixed": strict_zero_fixed,
            "numeric_core": numeric,
            "numeric_core_if_leading_zero_fixed": numeric_zero_fixed,
            "rust_panics": panics,
            "agent_style": agent,
            "upstream_suite": upstream,
        }));
        // Melhor base de fork: mais casos estritos com o zero corrigido; empate decide quem não entra em panic.
        let key = (strict_zero_fixed, usize::MAX - panics);
        let better = best.as_ref().is_none_or(|b| key > b.5);
        if better {
            best = Some((cand.name(), conf.clone(), numeric_zero_fixed, fit, classes.clone(), key, panics));
        }
        part.candidates.push(CandidateResult {
            name: cand.name(),
            version: format!("git {}", &up.rev[..12]),
            role: "bc".into(),
            category: Some(scan.tree_category.letter().to_string()),
            conformance: Some(conf),
            fit,
            notes,
            metrics,
        });
    }

    // Descartados por evidência de fonte (não há o que rodar contra o golden).
    part.candidates.push(CandidateResult {
        name: "bc (crates.io)".into(),
        version: "0.1.17".into(),
        role: "bc".into(),
        category: Some("b".into()),
        conformance: None,
        fit: Fit::DoesNotFit,
        notes: "Não implementa bc: src/lib.rs monta um Command do executável `bc` do sistema (crate execute, \
                `command.execute_input_output`, linhas 87 e 118) e devolve a saída dele. Num pseudo-linus sem bc \
                no host não há o que chamar."
            .into(),
        metrics: json!({ "evidence": "bc-0.1.17/src/lib.rs:41 use execute::{command_args, Execute}; :87 command.execute_input_output(...)" }),
    });
    part.candidates.push(CandidateResult {
        name: "bashkit builtin bc".into(),
        version: "0.18.2".into(),
        role: "bc".into(),
        category: None,
        conformance: None,
        fit: Fit::DoesNotFit,
        notes: "Builtin de 707 linhas sobre f64 (BcState.variables: HashMap<String, f64>, src/builtins/bc.rs:98), \
                quebra a entrada por linha e por ';' sem gramática: sem precisão arbitrária, sem define/auto, \
                sem if/while/for, sem ibase/obase. Não é bc; não vale fork."
            .into(),
        metrics: json!({ "evidence": "bashkit-0.18.2/src/builtins/bc.rs:96-98 struct BcState { variables: HashMap<String, f64> }" }),
    });
    part.notes.push(
        "bc: implementações Rust triadas no GitHub e descartadas sem rodar por tamanho e estado (WillKirkmanM/bc \
         e tincochan/bc-rust com 5 KB, AdrianColaianni/bcr \"WIP\", akgvn/bc parado desde 2021); kz80_bc gera \
         código Z80; eva/evar/excalc são calculadoras com outra linguagem."
            .into(),
    );

    let role = match &best {
        Some((name, conf, numeric, fit, classes, key, panics)) => {
            let top: Vec<String> = {
                let mut v: Vec<(&&str, &usize)> = classes.iter().collect();
                v.sort_by(|a, b| b.1.cmp(a.1));
                v.iter().take(3).map(|(k, n)| format!("{k} ({n})")).collect()
            };
            RoleSummary {
                role: "bc".into(),
                best: match fit {
                    Fit::Fits => name.clone(),
                    Fit::FitsWithWork => {
                        format!("fork do motor de {name}, com extensões GNU, mensagens de erro e front-end nossos")
                    }
                    _ => "fazer à mão".into(),
                },
                fit: *fit,
                summary: format!(
                    "melhor base de fork ({name}): {}/{} estrito contra o GNU bc 1.07.1 ({} com o zero à esquerda \
                     corrigido), núcleo numérico {}/{}, {panics} casos em panic; o resto é {}; nenhuma \
                     implementação existe como biblioteca no crates.io.",
                    conf.strict_pass,
                    conf.total,
                    key.0,
                    numeric.0,
                    numeric.2,
                    top.join(", ")
                ),
            }
        }
        None => RoleSummary {
            role: "bc".into(),
            best: "fazer à mão".into(),
            fit: Fit::DoesNotFit,
            summary: "nenhum candidato compilou.".into(),
        },
    };
    part.roles.push(role);
    Ok(json!({ "cases": cases.len(), "candidates": rows }))
}

#[allow(clippy::too_many_arguments)]
fn bc_notes(
    up: &bc::Upstream,
    conf: &Conformance,
    numeric: (usize, usize, usize),
    numeric_zero_fixed: (usize, usize, usize),
    strict_zero_fixed: usize,
    classes: &BTreeMap<&'static str, usize>,
    panics: usize,
    engine: &bc::SourceStats,
    engine_category: Option<&str>,
) -> String {
    let mut parts = vec![format!(
        "{}/{} estrito, {}/{} leniente; núcleo numérico {}/{}.",
        conf.strict_pass, conf.total, conf.lenient_pass, conf.total, numeric.0, numeric.2
    )];
    if strict_zero_fixed != conf.strict_pass {
        parts.push(format!(
            "Só com o zero à esquerda corrigido: {strict_zero_fixed}/{} estrito, núcleo numérico {}/{}.",
            conf.total, numeric_zero_fixed.0, numeric_zero_fixed.2
        ));
    }
    let cls: Vec<String> = classes.iter().map(|(k, n)| format!("{k}: {n}")).collect();
    parts.push(format!("Divergências: {}.", cls.join("; ")));
    if panics > 0 {
        parts.push(format!("{panics} casos terminam em panic do Rust no stderr."));
    }
    parts.push(format!(
        "Motor: {} linhas em {} arquivos ({} de teste), {} pontos de host, {} unsafe, {} panic!.",
        engine.lines, engine.files, engine.test_lines, engine.host_touch, engine.unsafe_sites, engine.panic_sites
    ));
    if let Some(cat) = engine_category {
        parts.push(format!("Motor em pacote separado (bc_core, no_std + alloc, categoria {cat})."));
    }
    if up.dir == "posixutils-rs" {
        parts.push(
            "bc POSIX estrito: imprime 0 antes do ponto, não tem as extensões GNU (print, else, !, &&, ||, last, #, \
             halt, continue, void) nem -q/-s/-w. O motor (calc/bc_util) escreve num Write genérico e não toca o \
             host; o binário depende de plib, que compila C."
                .into(),
        );
    } else {
        parts.push(
            "Erros de sintaxe e de execução viram panic! dentro do bc_core (o CLI deles faz catch_unwind): num host \
             com panic=abort isso derruba tudo, então o fork precisa trocar panic por Result."
                .into(),
        );
    }
    parts.join(" ")
}

// -------------------------------------------------------------------------------------------- file

/// Encaixe do `file`: MIME é o que mais importa (scripts decidem por ele); a descrição tolera os
/// detalhes que mudam entre versões de regra.
fn file_fit(describe: (usize, usize, usize), mime: (usize, usize, usize)) -> Fit {
    if ratio(describe.0, describe.2) >= 0.95 && ratio(mime.0, mime.2) >= 0.95 {
        Fit::Fits
    } else if ratio(describe.0, describe.2) >= 0.8 && ratio(mime.0, mime.2) >= 0.9 {
        Fit::FitsWithWork
    } else {
        Fit::DoesNotFit
    }
}

/// Rodadas do corpus do `file` por candidato (detecta motor não determinístico).
const FILE_RUNS: usize = 3;

/// Crates com C que o depscan acha na árvore mas que não entram no binário Linux x86_64 (conferido com
/// `cargo tree -e normal -i <crate>` e com os objetos em target/release/build).
const C_NOT_LINKED: &[(&str, &str)] = &[
    ("blake3", "só no proc-macro magic-embed (via fs-walk), em tempo de build; compila assembly SIMD com cc mas não vai pro binário"),
    ("iana-time-zone-haiku", "só no alvo Haiku"),
    ("wasm-bindgen-shared", "só no alvo wasm32"),
];

struct FileSpec {
    candidate: Box<dyn Candidate>,
    key: &'static str,
    version: &'static str,
    packages: &'static [&'static str],
    mime_only: bool,
    /// Tem a camada ascmagic nossa por cima.
    layered: bool,
}

/// Classe de uma divergência do `file`.
fn classify_file(cmp: &CaseComparison, golden: &Outcome, actual: &Outcome) -> &'static str {
    let exp = String::from_utf8_lossy(golden.stdout.as_slice());
    let got = String::from_utf8_lossy(actual.stdout.as_slice());
    if let Some(why) = &actual.unsupported {
        if why.contains("só MIME") { "sem descrição (motor só de MIME)" } else { "opção não suportada (-z)" }
    } else if got.contains("(sem MIME)") {
        "motor sem MIME"
    } else if exp.contains("last modified: ") && exp.contains(" 2026") && got.contains("2026-01-15") {
        "data no formato errado (2026-01-15 em vez de Thu Jan 15 ... 2026)"
    } else if has_tag(cmp, &["text", "script", "source"]) || exp.contains(" text") {
        "texto: codificação, terminadores ou ', ASCII text'"
    } else {
        "regra de magic diferente"
    }
}

fn run_file(part: &mut Part) -> Result<Value> {
    let cases = common::load_cases("file")?;
    let golden: BTreeMap<&str, &Outcome> = cases.iter().map(|(c, g)| (c.id.as_str(), g)).collect();
    let t = Instant::now();
    let magic_db_scan = depscan::scan(&common::manifest_path(), "magic-db")?;
    let magdir = magic_db_scan.root.manifest_dir.join("src").join("magdir");
    let magdir_files = std::fs::read_dir(&magdir).map(|d| d.count()).unwrap_or(0);
    let t_load = Instant::now();
    let libmagic_dir = file::LibmagicRs::new(file::LibmagicRules::Dir(magdir.clone()));
    let libmagic_load_s = t_load.elapsed().as_secs_f64();
    let libmagic_dir_error = libmagic_dir.load_error().map(str::to_string);
    let libmagic_dir_layered = file::WithAscmagic(file::LibmagicRs::new(file::LibmagicRules::Dir(magdir.clone())));
    let t_load = Instant::now();
    let pure = file::PureMagic::new(false);
    let pure_load_s = t_load.elapsed().as_secs_f64();
    eprintln!("h33 file: bancos carregados em {:.1}s", t.elapsed().as_secs_f64());

    const PURE: &[&str] = &["pure-magic", "magic-db"];
    const LIBMAGIC: &[&str] = &["libmagic-rs"];
    let spec = |candidate: Box<dyn Candidate>, key, version, packages, mime_only, layered| FileSpec {
        candidate,
        key,
        version,
        packages,
        mime_only,
        layered,
    };
    let specs: Vec<FileSpec> = vec![
        spec(Box::new(file::FileCli { engine: pure }), "pure-magic-first", "0.4.1", PURE, false, false),
        spec(Box::new(file::FileCli { engine: file::PureMagic::new(true) }), "pure-magic-best", "0.4.1", PURE, false, false),
        spec(
            Box::new(file::FileCli { engine: file::WithAscmagic(file::PureMagic::new(false)) }),
            "pure-magic-ascmagic",
            "0.4.1",
            PURE,
            false,
            true,
        ),
        spec(
            Box::new(file::FileCli { engine: file::LibmagicRs::new(file::LibmagicRules::Builtin) }),
            "libmagic-rs-builtin",
            "0.12.6",
            LIBMAGIC,
            false,
            false,
        ),
        spec(Box::new(file::FileCli { engine: libmagic_dir }), "libmagic-rs-magdir", "0.12.6", LIBMAGIC, false, false),
        spec(
            Box::new(file::FileCli { engine: libmagic_dir_layered }),
            "libmagic-rs-magdir-ascmagic",
            "0.12.6",
            LIBMAGIC,
            false,
            true,
        ),
        spec(Box::new(file::FileCli { engine: file::Infer }), "infer", "0.22", &["infer"], true, false),
        spec(Box::new(file::FileCli { engine: file::FileFormat }), "file-format", "0.29", &["file-format"], true, false),
    ];

    let mut scans: BTreeMap<&str, DepSummary> = BTreeMap::new();
    for spec in &specs {
        for p in spec.packages {
            if !scans.contains_key(p) {
                scans.insert(p, common::dep_scan(p)?);
            }
        }
    }

    let mut rows = Vec::new();
    // (nome, descrição, MIME, encaixe, tem a camada ascmagic)
    type FileBest = (String, (usize, usize, usize), (usize, usize, usize), Fit, bool);
    let mut best: Option<FileBest> = None;
    for spec in &specs {
        let t = Instant::now();
        // Três rodadas por candidato no mesmo processo: pega motor não determinístico dentro do processo
        // (o do libmagic-rs é entre processos, ver a nota dele). Reporta a pior e lista os que oscilam.
        let n_runs = FILE_RUNS;
        let mut runs: Vec<Recorded> = (0..n_runs)
            .map(|_| score_recorded(spec.candidate.as_ref(), &cases, &format!("file-{}", spec.key)))
            .collect();
        let mut flapping: Vec<String> = Vec::new();
        for (i, cmp) in runs[0].1.iter().enumerate() {
            if runs.iter().any(|r| r.1[i].strict != cmp.strict) {
                flapping.push(cmp.id.clone());
            }
        }
        runs.sort_by_key(|r| r.0.strict_pass);
        let (conf, all, seen) = runs.swap_remove(0);
        let describe = tag_counts(&conf, "describe");
        let mime = tag_counts(&conf, "mime");
        let charset = tag_counts(&conf, "mime-charset");
        let detection = subset(&all, |c| has_tag(c, &["describe"]) && !has_tag(c, &["special"]));
        let mut classes: BTreeMap<&'static str, usize> = BTreeMap::new();
        for cmp in all.iter().filter(|c| !c.strict) {
            let actual = seen.get(&cmp.id).cloned().unwrap_or_default();
            *classes.entry(classify_file(cmp, golden[cmp.id.as_str()], &actual)).or_default() += 1;
        }
        let mut category = "a".to_string();
        let mut c_deps: Vec<String> = Vec::new();
        let mut scan_json = serde_json::Map::new();
        for p in spec.packages {
            let s = &scans[p];
            category = category.max(s.tree_category.clone());
            c_deps.extend(s.c_deps.iter().cloned());
            scan_json.insert(p.to_string(), serde_json::to_value(s)?);
        }
        c_deps.sort();
        c_deps.dedup();
        let c_linked: Vec<String> =
            c_deps.iter().filter(|d| !C_NOT_LINKED.iter().any(|(n, _)| d.starts_with(n))).cloned().collect();
        let fit = if spec.mime_only || (spec.key.starts_with("libmagic-rs-magdir") && libmagic_dir_error.is_some()) {
            Fit::DoesNotFit
        } else {
            file_fit(describe, mime)
        };
        eprintln!("h33 file: {} em {:.1}s", spec.candidate.name(), t.elapsed().as_secs_f64());
        let mut notes = file_notes(spec.key, describe, mime, charset, detection, &classes, libmagic_dir_error.as_deref());
        if !c_deps.is_empty() {
            let linked = if c_linked.is_empty() { "nenhuma linkada no binário Linux".to_string() } else { format!("linkadas: {}", c_linked.join(", ")) };
            notes.push_str(&format!(" depscan acha C na árvore ({}); {linked}.", c_deps.join(", ")));
        }
        if !flapping.is_empty() {
            notes.push_str(&format!(
                " Não determinístico: {} oscilou entre {n_runs} cargas do mesmo banco no mesmo binário (placar é o da pior).",
                flapping.join(", ")
            ));
        }
        rows.push(json!({
            "candidate": spec.candidate.name(),
            "strict": conf.strict_pass,
            "total": conf.total,
            "describe": describe,
            "describe_content_only": detection,
            "mime": mime,
            "mime_charset": charset,
            "divergence_classes": classes,
        }));
        let score = describe.0 + mime.0;
        if !spec.mime_only && best.as_ref().is_none_or(|b| score > b.1.0 + b.2.0) {
            best = Some((spec.candidate.name(), describe, mime, fit, spec.layered));
        }
        part.candidates.push(CandidateResult {
            name: spec.candidate.name(),
            version: spec.version.to_string(),
            role: "file".into(),
            category: Some(category),
            conformance: Some(conf.clone()),
            fit,
            notes,
            metrics: json!({
                "strict_rate": round3(conf.strict_rate()),
                "describe_strict_lenient_total": describe,
                "describe_content_only_strict_lenient_total": detection,
                "mime_type_strict_lenient_total": mime,
                "mime_charset_strict_lenient_total": charset,
                "divergence_classes": classes,
                "our_ascmagic_layer": spec.layered,
                "runs": n_runs,
                "nondeterministic_cases": flapping,
                "c_deps": c_deps,
                "c_deps_linked_on_linux": c_linked,
                "c_deps_not_linked_reason": C_NOT_LINKED
                    .iter()
                    .filter(|(n, _)| c_deps.iter().any(|d| d.starts_with(n)))
                    .map(|(n, why)| json!({ "crate": n, "why": why }))
                    .collect::<Vec<_>>(),
                "depscan": scan_json,
                "libmagic_rs_magdir_load_error": if spec.key.starts_with("libmagic-rs-magdir") { json!(libmagic_dir_error) } else { Value::Null },
            }),
        });
    }
    part.notes.push(
        "file: o magdir do magic-db 0.6.0 vem do repositório do file de 2026 (images,v 1.276, compress,v 1.100, \
         archive,v 1.223, commands,v 1.82), mais novo que o file 5.46 do oráculo (images,v 1.263, compress,v 1.96, \
         archive,v 1.207, commands,v 1.77): parte das divergências de descrição pode ser regra nova, não motor."
            .into(),
    );
    part.notes.push(
        "file: o `file` do posixutils-rs foi descartado sem rodar: é o file do POSIX (\"commands text\", \
         \"c program text\"), lê o magic em texto de /usr/share/file/magic/magic do host e não tem --mime-type."
            .into(),
    );
    let role = match best {
        Some((name, describe, mime, fit, layered)) => RoleSummary {
            role: "file".into(),
            best: match fit {
                Fit::Fits => name.clone(),
                Fit::FitsWithWork if layered => format!("{name}, com front-end nosso (opções, symlink, charset)"),
                Fit::FitsWithWork => format!("{name} com front-end e ascmagic nossos"),
                _ => "fazer à mão".into(),
            },
            fit,
            summary: format!(
                "{name}: descrição {}/{} e --mime-type {}/{} byte a byte contra o file 5.46.",
                describe.0, describe.2, mime.0, mime.2
            ),
        },
        None => RoleSummary { role: "file".into(), best: "fazer à mão".into(), fit: Fit::DoesNotFit, summary: "sem candidato".into() },
    };
    part.roles.push(role);
    Ok(json!({
        "cases": cases.len(),
        "magdir": magdir.display().to_string(),
        "magdir_files": magdir_files,
        "load_seconds": { "magic_db_embedded": round3(pure_load_s), "libmagic_rs_magdir_text": round3(libmagic_load_s) },
        "candidates": rows,
    }))
}

fn file_notes(
    key: &str,
    describe: (usize, usize, usize),
    mime: (usize, usize, usize),
    charset: (usize, usize, usize),
    detection: (usize, usize, usize),
    classes: &BTreeMap<&'static str, usize>,
    libmagic_dir_error: Option<&str>,
) -> String {
    let cls: Vec<String> = classes.iter().map(|(k, n)| format!("{k}: {n}")).collect();
    let base = format!(
        "descrição {}/{} (só conteúdo de arquivo regular: {}/{}), --mime-type {}/{}, -i/--mime-encoding {}/{}. \
         Divergências: {}.",
        describe.0,
        describe.2,
        detection.0,
        detection.2,
        mime.0,
        mime.2,
        charset.0,
        charset.2,
        cls.join("; ")
    );
    let extra = match key {
        "pure-magic-first" | "pure-magic-best" => {
            "Banco embutido no binário (magic_embed), sem ler o host. Texto: só ASCII/UTF-8/Unknown (TextEncoding), \
             sem CRLF, linhas longas, ISO-8859 ou UTF-16, e escreve \"S text\" onde o libmagic escreve \"S, ASCII \
             text\"; datas de gzip e zip saem em ISO em vez do formato do ctime."
        }
        "pure-magic-ascmagic" => {
            "Com a camada ascmagic nossa (~150 linhas: classe de codificação, terminadores, linhas longas, \
             reescrita de \"S text\") por cima do motor. Sobra: data no formato ISO no gzip e no zip (bug do \
             pure-magic, fork ou patch), -z não implementado no front-end, e CSV/PDF que mudaram nas regras de \
             2026."
        }
        "libmagic-rs-magdir-ascmagic" => {
            "Com a camada ascmagic nossa por cima do libmagic-rs com magdir; o MIME continua vindo da tabela de \
             palavras-chave em HashMap estático (ordem muda por processo, ver a variante sem a camada)."
        }
        "libmagic-rs-builtin" => "Regras embutidas cobrem 10 formatos (ELF, PE, ZIP, TAR, GZIP, JPEG, PNG, GIF, BMP, PDF).",
        "libmagic-rs-magdir" => match libmagic_dir_error {
            Some(_) => "O parser não carregou o magdir do file (ver libmagic_rs_magdir_load_error).",
            None => {
                "Mesmo magdir do magic-db carregado como magic em texto; !:mime não é lido. O MIME vem de \
                 MimeMapper::get_mime_type, que itera um HashMap estático (src/mime.rs:158) e devolve a primeira \
                 palavra-chave contida na descrição: a ordem muda de um processo pro outro, e o --mime-type do HTML \
                 oscilou entre text/html e text/plain entre execuções (placar varia em 1 caso)."
            }
        },
        _ => "Só MIME; descrição indisponível (casos de descrição contam como unsupported).",
    };
    format!("{base} {extra}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_zero_is_stripped_per_line() {
        assert_eq!(strip_leading_zero("0.5\n-0.25\n10.5\n0\n"), ".5\n-.25\n10.5\n0\n");
    }

    #[test]
    fn bc_fit_needs_numeric_core_and_host_free_engine() {
        let conf = Conformance { total: 10, strict_pass: 7, lenient_pass: 8, ..Conformance::default() };
        assert_eq!(bc_fit(&conf, (9, 9, 10), 0), Fit::FitsWithWork);
        assert_eq!(bc_fit(&conf, (9, 9, 10), 3), Fit::DoesNotFit);
        assert_eq!(bc_fit(&conf, (5, 5, 10), 0), Fit::DoesNotFit);
    }

    #[test]
    fn file_fit_thresholds() {
        assert_eq!(file_fit((47, 47, 47), (28, 28, 28)), Fit::Fits);
        assert_eq!(file_fit((40, 40, 47), (27, 27, 28)), Fit::FitsWithWork);
        assert_eq!(file_fit((27, 27, 47), (27, 27, 28)), Fit::DoesNotFit);
    }
}
