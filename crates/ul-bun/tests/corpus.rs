//! Corpus de programas pequenos contra o `bun` 1.4.2 real: cada caso de `tests/corpus` tem `cmd.json` (o argv depois de
//! `bun`), `files/` (o que o oráculo viu em `/app`, também o diretório de trabalho do kernel de teste) e `expected/`
//! (`stdout`, `stderr` e `code` que `scripts/gen-corpus.py` mede no oráculo). O stdout, o stderr (BuildMessage com
//! posição, trecho do fonte e pilha incluídos) e o código de saída têm de bater byte a byte. Uma entrada em `KNOWN`
//! continua rodando e, se passar a bater, o teste pede a remoção da linha (um nome com `*` no fim vale para o grupo com esse
//! prefixo e só sai inteiro).

mod common;

use std::path::{Path, PathBuf};

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::WaitStatus;

/// A causa dos casos de `-p`: o valor de conclusão do programa e o escopo CommonJS (ver o PLAN, seção do `-p`).
const CAUSE_PRINT: &str = "-p precisa do valor de conclusão do ECMAScript no motor (programa com escopo de função CommonJS, impresso depois do laço de eventos), ainda não portado";

/// `(caso, causa medida no oráculo)`; o nome que termina em `*` vale para todo caso com esse prefixo.
const KNOWN: &[(&str, &str)] = &[
    ("cli-p-*", CAUSE_PRINT),
    ("cli-p2-*", CAUSE_PRINT),
    ("cli-p3-*", CAUSE_PRINT),
    ("cli-pe", CAUSE_PRINT),
    ("cli-print-long", CAUSE_PRINT),
    ("cli-print-eq", CAUSE_PRINT),
    ("cli-e-and-p", CAUSE_PRINT),
    ("process-stdout-shape", "o process.stdout do porte (process_stdio.rs) não é um fs.WriteStream/tty.WriteStream real: não é instanceof stream.Writable e não tem os métodos de tty.WriteStream (getColorDepth, hasColors, columns)"),
    ("module-crypto-pending", "o node:crypto ainda não porta o que o BoringSSL do bun responde fora dos resumos e dos bytes aleatórios: pbkdf2, scrypt, hkdf, argon2, cifras (createCipheriv/createDecipheriv, getCipherInfo), KeyObject e createSecretKey, geração de chaves, sign/verify, ECDH, primos e publicEncrypt (PLAN, seção do node:crypto)"),
    ("fs-utf8-buffer", "o bun 1.4.2 lança TypeError em mergeBuf (Utf8Stream síncrono com contentMode buffer) com stack de fonte interno; o zjsc roda o mesmo código gerado e não reproduz o fonte interno do bun"),
];

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
    let argv: Vec<String> = serde_json::from_str(&std::fs::read_to_string(case.join("cmd.json")).expect("cmd.json")).expect("cmd.json é um array de strings");
    let kernel = Kernel::new(KernelConfig::default());
    let sandbox = kernel
        .create_sandbox(SandboxConfig { programs: common::programs(), cwd: b"/app".to_vec(), ..SandboxConfig::default() })
        .expect("sandbox");
    common::install(&sandbox.fs(), "/app", &common::read_tree(&case.join("files")));
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
    // `expected/signal` é o sinal que matou o bun no oráculo (0, ou sem o arquivo, se ele saiu sozinho): morrer pelo
    // próprio sinal e sair com 128 + sinal são coisas diferentes para quem espera o processo.
    let signal: i32 = std::fs::read_to_string(case.join("expected").join("signal")).map_or(0, |text| text.trim().parse().expect("sinal"));
    let status_matches = match result.status {
        WaitStatus::Signaled { signal: died, .. } => signal != 0 && died.0 == signal,
        WaitStatus::Exited(exited) => signal == 0 && exited == code,
        _ => false,
    };
    if !status_matches {
        problems.push(format!("saída: esperado código {code} (sinal {signal}), obtido {:?}", result.status));
    }
    if problems.is_empty() { Ok(()) } else { Err(problems.join("\n")) }
}

#[test]
fn corpus_matches_bun() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&root).expect("corpus").map(|e| e.expect("caso").path()).collect();
    cases.sort();
    let mut failures = Vec::new();
    for case in &cases {
        let name = case.file_name().expect("nome").to_string_lossy().into_owned();
        let known = KNOWN.iter().find(|(pattern, _)| pattern.strip_suffix('*').map_or(*pattern == name, |prefix| name.starts_with(prefix)));
        match (run_case(case), known) {
            (Ok(()), None) | (Err(_), Some(_)) => {}
            // Um grupo (`prefixo*`) sai de `KNOWN` inteiro, no commit que porta o que falta; um caso dele que já bate não é erro.
            (Ok(()), Some((pattern, _))) if pattern.ends_with('*') => {}
            (Ok(()), Some((_, cause))) => failures.push(format!("{name}: passou a bater, tire de KNOWN ({cause})")),
            (Err(diff), None) => failures.push(format!("=== {name}\n{diff}")),
        }
    }
    assert!(cases.len() >= 15, "o corpus tem só {} casos", cases.len());
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), cases.len(), failures.join("\n"));
}
