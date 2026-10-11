//! Corpus de programas pequenos contra o `bun` 1.4.2 real: cada caso de `tests/corpus` tem `cmd.json` (o argv depois de
//! `bun`), `files/` (o que o oráculo viu em `/app`, também o diretório de trabalho do kernel de teste) e `expected/`
//! (`stdout`, `stderr` e `code` que `scripts/gen-corpus.py` mede no oráculo). O stdout, o stderr (BuildMessage com
//! posição, trecho do fonte e pilha incluídos) e o código de saída têm de bater byte a byte. Uma entrada em `KNOWN`
//! continua rodando e, se passar a bater, o teste pede a remoção da linha.

use std::path::{Path, PathBuf};

use sysabi::testkit::TestKit;
use sysabi::WaitStatus;

/// `(caso, causa medida no oráculo)`.
const KNOWN: &[(&str, &str)] = &[
    ("fs-read", "o zjsc ainda não tem o módulo node:fs (readFileSync, existsSync sobre o VFS do sandbox)"),
    ("http-server", "o zjsc ainda não tem Bun.serve nem a rede do fetch local"),
];

fn collect(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for entry in std::fs::read_dir(dir).expect("diretório do caso") {
        let path: PathBuf = entry.expect("entrada").path();
        if path.is_dir() {
            collect(&path, base, out);
        } else {
            let rel = path.strip_prefix(base).expect("relativo").to_string_lossy().into_owned();
            out.push((rel, std::fs::read(&path).expect("arquivo")));
        }
    }
}

fn run_case(case: &Path) -> Result<(), String> {
    let argv: Vec<String> = {
        let text = std::fs::read_to_string(case.join("cmd.json")).expect("cmd.json");
        // Um array JSON de strings; o corpus não usa escape além de `\"`, `\\` e `\n`.
        parse_json_strings(&text)
    };
    let mut kit = TestKit::new().programs(ul_bun::programs()).dir("/app", 0o755).cwd("/app");
    let mut files = Vec::new();
    collect(&case.join("files"), &case.join("files"), &mut files);
    for (rel, data) in &files {
        if let Some((parent, _)) = rel.rsplit_once('/') {
            kit = kit.dir(&format!("/app/{parent}"), 0o755);
        }
        kit = kit.file(&format!("/app/{rel}"), data, 0o644);
    }
    let mut full = vec!["bun"];
    full.extend(argv.iter().map(String::as_str));
    let result = kit.run(&full, b"");
    let expected = |name: &str| std::fs::read_to_string(case.join("expected").join(name)).expect("esperado");
    let code: i32 = expected("code").trim().parse().expect("código");
    let mut problems = Vec::new();
    if result.stdout_str() != expected("stdout") {
        problems.push(format!("stdout\n--- esperado\n{}--- obtido\n{}", expected("stdout"), result.stdout_str()));
    }
    if result.stderr_str() != expected("stderr") {
        problems.push(format!("stderr\n--- esperado\n{}--- obtido\n{}", expected("stderr"), result.stderr_str()));
    }
    if result.status != WaitStatus::Exited(code) {
        problems.push(format!("saída: esperado {code}, obtido {:?}", result.status));
    }
    if problems.is_empty() { Ok(()) } else { Err(problems.join("\n")) }
}

fn parse_json_strings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.trim().chars();
    assert_eq!(chars.next(), Some('['));
    loop {
        match chars.next().expect("array incompleto") {
            ']' => break,
            '"' => {
                let mut item = String::new();
                loop {
                    match chars.next().expect("string incompleta") {
                        '"' => break,
                        '\\' => match chars.next().expect("escape") {
                            'n' => item.push('\n'),
                            't' => item.push('\t'),
                            other => item.push(other),
                        },
                        other => item.push(other),
                    }
                }
                out.push(item);
            }
            _ => {}
        }
    }
    out
}

#[test]
fn corpus_matches_bun() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&root).expect("corpus").map(|e| e.expect("caso").path()).collect();
    cases.sort();
    let mut failures = Vec::new();
    for case in &cases {
        let name = case.file_name().expect("nome").to_string_lossy().into_owned();
        let known = KNOWN.iter().find(|(n, _)| *n == name);
        match (run_case(case), known) {
            (Ok(()), None) | (Err(_), Some(_)) => {}
            (Ok(()), Some((_, cause))) => failures.push(format!("{name}: passou a bater, tire de KNOWN ({cause})")),
            (Err(diff), None) => failures.push(format!("=== {name}\n{diff}")),
        }
    }
    assert!(cases.len() >= 15, "o corpus tem só {} casos", cases.len());
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), cases.len(), failures.join("\n"));
}
