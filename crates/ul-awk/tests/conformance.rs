//! Conformidade do awk contra o golden da bancada (gawk 5.2.1 do Debian 13), sobre o testkit:
//!
//! - `agent_cases`: os casos de agente de `testbench/corpus/cases/awk`;
//! - `gawk_suite`: o subconjunto portável da suíte do gawk 5.2.1 que o F04 selecionou (352 testes),
//!   montado a partir de `testbench/corpus/upstream/gawk` (gitignored, GPLv3, só leitura) com o golden do
//!   oráculo em cache (`testbench/scratch/f04-awk-jq/gawk-oracle-cache.json`);
//! - `adhoc`: roda um programa avulso (`AWK_ARGS` separado por `\x1f`, `AWK_STDIN`) pra depuração.
//!
//! Rode com `--nocapture` pra ver o placar; o detalhe caso a caso vai pra `target/agents/awk/report-*.txt`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use harness::{Case, Candidate, FileSpec, Outcome};
use pl_testing::{KernelCandidate, TestkitCandidate};

/// O awk deste crate primeiro, depois o resto do sistema (bash, sort, cat...).
fn all_programs() -> Vec<sysabi::Program> {
    let mut p = ul_awk::programs();
    let mine: Vec<String> = p.iter().map(|x| x.path()).collect();
    p.extend(userland::all_programs().into_iter().filter(|x| !mine.contains(&x.path())));
    p
}

fn candidate() -> TestkitCandidate {
    TestkitCandidate::new("ul-awk (testkit)", all_programs())
}

fn kernel_candidate() -> KernelCandidate {
    KernelCandidate::new("ul-awk (kernel)", all_programs())
}

fn report_dir() -> PathBuf {
    let dir = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    std::fs::create_dir_all(&dir).ok();
    dir
}

fn filter_ids() -> Option<Vec<String>> {
    std::env::var("AWK_CASES").ok().map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
}

fn write_report(name: &str, comps: &[harness::CaseComparison]) {
    let mut s = String::new();
    for c in comps.iter().filter(|c| !c.strict) {
        s.push_str(&format!("FALHA {}\n", c.id));
        for d in &c.detail {
            s.push_str(&format!("    {d}\n"));
        }
    }
    let path = report_dir().join(format!("report-{name}.txt"));
    std::fs::write(&path, s).ok();
    eprintln!("detalhes em {}", path.display());
}

fn run_agent_cases(cand: &dyn Candidate, label: &str) {
    let (cases, missing) = harness::paths::load_tool("awk").expect("casos");
    let cases: Vec<_> = match filter_ids() {
        Some(ids) => cases.into_iter().filter(|(c, _)| ids.contains(&c.id)).collect(),
        None => cases,
    };
    let (conf, comps) = harness::score(cand, &cases);
    eprintln!(
        "awk (agente, {label}): {}/{} estrito ({:.1}%), {}/{} leniente, {} sem golden",
        conf.strict_pass,
        conf.total,
        100.0 * conf.strict_rate(),
        conf.lenient_pass,
        conf.total,
        missing
    );
    write_report(&format!("agent-{label}"), &comps);
}

#[test]
fn agent_cases() {
    run_agent_cases(&candidate(), "testkit");
}

#[test]
fn agent_cases_kernel() {
    run_agent_cases(&kernel_candidate(), "kernel");
}

// ------------------------------------------------------------------ suíte do gawk

fn gawk_test_dir() -> PathBuf {
    harness::paths::corpus_dir().join("upstream/gawk/gawk-5.2.1/test")
}

fn list_vars(makefile: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in makefile.lines() {
        if let Some(name) = &current {
            let cont = line.trim_end().ends_with('\\');
            let body = line.trim_end().trim_end_matches('\\');
            out.get_mut(name).expect("var").extend(body.split_whitespace().map(str::to_string));
            if !cont {
                current = None;
            }
            continue;
        }
        if let Some((name, rest)) = line.split_once('=') {
            let name = name.trim();
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                let cont = rest.trim_end().ends_with('\\');
                let body = rest.trim_end().trim_end_matches('\\');
                out.insert(name.to_string(), body.split_whitespace().map(str::to_string).collect());
                if cont {
                    current = Some(name.to_string());
                }
            }
        }
    }
    out
}

fn explicit_targets(makefile: &str) -> BTreeSet<String> {
    makefile
        .lines()
        .filter_map(|l| {
            let (name, _) = l.split_once(':')?;
            (!name.is_empty() && !l.starts_with('\t') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                .then(|| name.to_string())
        })
        .collect()
}

/// A mesma seleção do F04 (`testbench/experiments/f04-awk-jq/src/gawk_suite.rs`).
fn select(dir: &Path) -> Vec<String> {
    let makefile = std::fs::read_to_string(dir.join("Makefile.am")).expect("Makefile.am");
    let vars = list_vars(&makefile);
    let targets = explicit_targets(&makefile);
    let mut flagged = BTreeSet::new();
    for (var, names) in &vars {
        if var.starts_with("NEED_") || var == "RUN_SHELL" {
            flagged.extend(names.iter().cloned());
        }
    }
    let mut sel = Vec::new();
    for list in ["BASIC_TESTS", "UNIX_TESTS", "GAWK_EXT_TESTS"] {
        for name in vars.get(list).into_iter().flatten() {
            if flagged.contains(name)
                || targets.contains(name)
                || dir.join(format!("{name}.sh")).exists()
                || !dir.join(format!("{name}.awk")).exists()
                || !dir.join(format!("{name}.ok")).exists()
            {
                continue;
            }
            sel.push(name.clone());
        }
    }
    sel
}

fn b64(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn spec_of(data: &[u8]) -> FileSpec {
    match std::str::from_utf8(data) {
        Ok(s) => FileSpec::Text(s.to_string()),
        Err(_) => FileSpec::Table(harness::case::FileTable { content_b64: Some(b64(data)), ..Default::default() }),
    }
}

fn build_cases(dir: &Path, names: &[String]) -> Vec<Case> {
    let all_files: BTreeSet<String> =
        std::fs::read_dir(dir).expect("dir").filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    let mut cases = Vec::new();
    for name in names {
        let program = std::fs::read(dir.join(format!("{name}.awk"))).expect("awk");
        let text = String::from_utf8_lossy(&program).into_owned();
        let mut files = BTreeMap::new();
        files.insert(format!("{name}.awk"), spec_of(&program));
        let mut size = program.len();
        let mut refs = BTreeSet::new();
        for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')) {
            if token.is_empty() || token == format!("{name}.awk") || token.ends_with(".ok") {
                continue;
            }
            if all_files.contains(token) && dir.join(token).is_file() {
                refs.insert(token.to_string());
            }
        }
        for f in refs {
            let data = std::fs::read(dir.join(&f)).expect("ref");
            size += data.len();
            files.insert(f, spec_of(&data));
        }
        if size > 2 << 20 {
            continue;
        }
        let stdin_path = dir.join(format!("{name}.in"));
        let stdin = stdin_path.exists().then(|| std::fs::read(&stdin_path).expect("in"));
        let mut env = BTreeMap::new();
        env.insert("AWKPATH".to_string(), ".".to_string());
        cases.push(Case {
            id: format!("gawk-{name}"),
            argv: vec!["gawk".into(), "-f".into(), format!("{name}.awk")],
            script: None,
            stdin: None,
            stdin_b64: stdin.map(|s| b64(&s)),
            files,
            env,
            tags: vec!["upstream:gawk".into()],
            faketime: None,
            timeout_ms: Some(10_000),
        });
    }
    cases
}

fn merged(o: &Outcome) -> Vec<u8> {
    let mut m = o.stdout.0.clone();
    m.extend_from_slice(&o.stderr.0);
    match o.exit {
        Some(0) => {}
        Some(code) => m.extend_from_slice(format!("EXIT CODE: {code}\n").as_bytes()),
        None => m.extend_from_slice(format!("EXIT CODE: {}\n", 128 + o.signal.unwrap_or(0)).as_bytes()),
    }
    m
}

#[test]
fn gawk_suite() {
    run_gawk_suite(&candidate(), "testkit");
}

#[test]
fn gawk_suite_kernel() {
    run_gawk_suite(&kernel_candidate(), "kernel");
}

fn run_gawk_suite(cand: &dyn Candidate, label: &str) {
    let dir = gawk_test_dir();
    if !dir.join("Makefile.am").exists() {
        eprintln!("suíte do gawk ausente em {}; pulando", dir.display());
        return;
    }
    let names = select(&dir);
    let cases = build_cases(&dir, &names);
    let cache_path = harness::paths::root().join("scratch/f04-awk-jq/gawk-oracle-cache.json");
    let cache: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&cache_path).expect("cache do oráculo")).expect("json");
    let outcomes: Vec<Outcome> = serde_json::from_value(cache["outcomes"].clone()).expect("outcomes");
    assert_eq!(outcomes.len(), cases.len(), "cache do oráculo não corresponde à seleção");
    let mut valid = Vec::new();
    for (case, outcome) in cases.into_iter().zip(outcomes) {
        let name = case.id.trim_start_matches("gawk-").to_string();
        let expected = std::fs::read(dir.join(format!("{name}.ok"))).expect("ok");
        if merged(&outcome) == expected {
            valid.push((case, outcome));
        }
    }
    let valid: Vec<_> = match filter_ids() {
        Some(ids) => valid.into_iter().filter(|(c, _)| ids.contains(&c.id)).collect(),
        None => valid,
    };
    let (conf, comps) = harness::score(cand, &valid);
    eprintln!(
        "gawk (suíte, {label}): {}/{} estrito ({:.1}%), {}/{} leniente",
        conf.strict_pass,
        conf.total,
        100.0 * conf.strict_rate(),
        conf.lenient_pass,
        conf.total
    );
    write_report(&format!("gawk-{label}"), &comps);
}

#[test]
fn adhoc() {
    let Ok(args) = std::env::var("AWK_ARGS") else { return };
    let argv: Vec<String> = args.split('\x1f').map(str::to_string).collect();
    let stdin = std::env::var("AWK_STDIN").unwrap_or_default();
    let mut inv = harness::Invocation {
        case_id: "adhoc".into(),
        argv,
        script: None,
        stdin: stdin.into_bytes(),
        files: harness::MemTree::new(),
        env: BTreeMap::new(),
        faketime: None,
    };
    if let Ok(files) = std::env::var("AWK_FILES") {
        for spec in files.split('\x1e') {
            if let Some((name, data)) = spec.split_once('=') {
                inv.files.insert(name, harness::Entry::file(data.as_bytes().to_vec(), 0o644));
            }
        }
    }
    let out = if std::env::var("AWK_KERNEL").is_ok() { kernel_candidate().run(&inv) } else { candidate().run(&inv) };
    eprintln!("--- stdout\n{}--- stderr\n{}--- exit {:?} signal {:?}", String::from_utf8_lossy(&out.stdout.0), String::from_utf8_lossy(&out.stderr.0), out.exit, out.signal);
}
