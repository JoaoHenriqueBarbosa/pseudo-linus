//! Corpus de programas pequenos contra o `bun` 1.4.2 real: cada caso de `tests/corpus` tem `cmd.json` (o argv depois de
//! `bun`), `files/` (o que o oráculo viu em `/app`, também o diretório de trabalho do kernel de teste) e `expected/`
//! (`stdout`, `stderr` e `code` que `scripts/gen-corpus.py` mede no oráculo). O stdout, o stderr (BuildMessage com
//! posição, trecho do fonte e pilha incluídos) e o código de saída têm de bater byte a byte. Uma entrada em `KNOWN`
//! continua rodando e, se passar a bater, o teste pede a remoção da linha.

use std::path::{Path, PathBuf};

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::WaitStatus;

/// `(caso, causa medida no oráculo)`.
const KNOWN: &[(&str, &str)] = &[
    ("fs-utf8-buffer", "o bun 1.4.2 lança TypeError em mergeBuf (Utf8Stream síncrono com contentMode buffer) com stack de fonte interno; o zjsc roda o mesmo código gerado e não reproduz o fonte interno do bun"),
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

/// A ajuda de nível superior sorteia a cada execução o argumento de exemplo de sete comandos: o caso com o arquivo
/// `random-help` compara sem essa coluna (a coluna do exemplo, da 12 à 32, de cada uma das sete linhas).
fn without_examples(text: &str) -> String {
    const COMMANDS: [&str; 7] = ["x", "add", "remove", "update", "info", "why", "create"];
    text.split_inclusive('\n')
        .map(|line| {
            let listed = COMMANDS.iter().any(|command| line.starts_with(&format!("  {command} ")));
            if listed && line.len() > 33 { format!("{}{}", &line[..12], &line[33..]) } else { line.to_string() }
        })
        .collect()
}

fn run_case(case: &Path) -> Result<(), String> {
    let argv: Vec<String> = {
        let text = std::fs::read_to_string(case.join("cmd.json")).expect("cmd.json");
        // Um array JSON de strings; o corpus não usa escape além de `\"`, `\\` e `\n`.
        parse_json_strings(&text)
    };
    let kernel = Kernel::new(KernelConfig::default());
    let sandbox = kernel
        .create_sandbox(SandboxConfig { programs: ul_bun::programs(), cwd: b"/app".to_vec(), ..SandboxConfig::default() })
        .expect("sandbox");
    let fs = sandbox.fs();
    fs.mkdir_all(b"/app", 0o755).expect("/app");
    let mut files = Vec::new();
    collect(&case.join("files"), &case.join("files"), &mut files);
    for (rel, data) in &files {
        if let Some((parent, _)) = rel.rsplit_once('/') {
            fs.mkdir_all(format!("/app/{parent}").as_bytes(), 0o755).expect("diretório");
        }
        fs.write_file(format!("/app/{rel}").as_bytes(), data, 0o644).expect("arquivo");
    }
    let mut full = vec!["bun"];
    full.extend(argv.iter().map(String::as_str));
    let request = RunRequest {
        argv: full.iter().map(|part| part.as_bytes().to_vec()).collect(),
        cwd: Some(b"/app".to_vec()),
        timeout: Some(std::time::Duration::from_secs(120)),
        ..RunRequest::default()
    };
    let result = sandbox.run(request).expect("execução");
    let (stdout, stderr) = (String::from_utf8_lossy(&result.stdout).into_owned(), String::from_utf8_lossy(&result.stderr).into_owned());
    let expected = |name: &str| std::fs::read_to_string(case.join("expected").join(name)).expect("esperado");
    let code: i32 = expected("code").trim().parse().expect("código");
    let mut problems = Vec::new();
    let (stdout, wanted_stdout) = if case.join("random-help").exists() {
        (without_examples(&stdout), without_examples(&expected("stdout")))
    } else {
        (stdout, expected("stdout"))
    };
    if stdout != wanted_stdout {
        problems.push(format!("stdout\n--- esperado\n{wanted_stdout}--- obtido\n{stdout}"));
    }
    if stderr != expected("stderr") {
        problems.push(format!("stderr\n--- esperado\n{}--- obtido\n{}", expected("stderr"), stderr));
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
