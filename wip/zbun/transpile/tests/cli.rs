//! A CLI `zbun build --no-bundle` contra o `bun build --no-bundle` do bun 1.4.2, medido no oráculo: o texto no stdout
//! (o mesmo corpus do `golden_runtime`, sem `--target=bun`), a saída 0, o arquivo ausente (`Error resolving ...
//! ModuleNotFound` no stderr, saída 0) e o erro de sintaxe (saída 1, nada no stdout, o diagnóstico com o trecho do
//! fonte, o circunflexo, `error: texto`, `at arquivo:linha:coluna` e as notas, byte a byte como o bun).
use std::path::Path;
use std::process::Command;

fn zbun(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_zbun")).args(args).output().expect("rodar o zbun")
}

/// O corpus do `golden_runtime` (esperado de `--target=bun --minify-syntax`) pela CLI.
#[test]
fn build_with_target_bun_and_minify_syntax_matches_the_runtime_corpus() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/golden");
    for name in ["arrows.js", "classes.js", "asi.js", "comments.js"] {
        let out = zbun(&["build", "--no-bundle", "--target=bun", "--minify-syntax", dir.join(name).to_str().unwrap()]);
        assert_eq!(out.status.code(), Some(0), "{name}");
        assert_eq!(out.stdout, std::fs::read(dir.join(format!("{name}.expected.runtime"))).unwrap(), "{name}");
    }
}

/// Os quatro modos (`bun build --no-bundle` com e sem `--minify-syntax` e `--target=bun`) sobre `tests/cli`, esperados
/// do oráculo (`scripts/gen_cli_golden.sh`): dobra, inlining de `const` (número, template, negação), `let`, `var`, laço,
/// closure, sombra e bloco.
#[test]
fn build_modes_match_bun() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/cli");
    let modes: [(&str, &[&str]); 4] = [
        ("build", &[]),
        ("minify", &["--minify-syntax"]),
        ("target", &["--target=bun"]),
        ("both", &["--target=bun", "--minify-syntax"]),
    ];
    let mut failures = Vec::new();
    let mut total = 0;
    for entry in std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".js") {
            continue;
        }
        for (suffix, flags) in modes {
            let mut args = vec!["build", "--no-bundle"];
            args.extend_from_slice(flags);
            args.push(dir.join(&name).to_str().unwrap().to_owned().leak());
            let out = zbun(&args);
            let expected = std::fs::read_to_string(dir.join(format!("{name}.expected.{suffix}"))).unwrap();
            total += 1;
            if String::from_utf8_lossy(&out.stdout) != expected {
                failures.push(format!("{name} [{suffix}]\n--- esperado\n{expected}--- obtido\n{}", String::from_utf8_lossy(&out.stdout)));
            }
        }
    }
    assert!(total >= 8, "o corpus da CLI tem só {total} combinações");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}

#[test]
fn build_writes_outfile_and_nothing_to_stdout() {
    let dir = std::env::temp_dir().join("zbun-cli-test");
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("b1.ts");
    std::fs::write(&input, "const q: number = 1; console.log(q)\n").unwrap();
    let outfile = dir.join("o.js");
    let out = zbun(&["build", "--no-bundle", input.to_str().unwrap(), "--outfile", outfile.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    // Medido no oráculo: `const q = 1;\nconsole.log(q);\n` sem --minify-syntax.
    assert_eq!(std::fs::read_to_string(&outfile).unwrap(), "const q = 1;\nconsole.log(q);\n");
}

#[test]
fn missing_file_is_reported_on_stderr_with_exit_zero() {
    let out = zbun(&["build", "--no-bundle", "/nonexistent/none.ts"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stderr), "Error resolving \"/nonexistent/none.ts\": ModuleNotFound\n");
}

#[test]
fn syntax_error_exits_one_with_empty_stdout() {
    let dir = std::env::temp_dir().join("zbun-cli-test");
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("bad.ts");
    std::fs::write(&input, "const = 1\n").unwrap();
    let out = zbun(&["build", "--no-bundle", input.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("Expected identifier but found \"=\""));
}

/// Os diagnósticos de sintaxe sobre `tests/cli_errors`, esperados do oráculo (`scripts/gen_cli_errors_golden.sh`, que
/// roda cada arquivo em `/w/`): identificador faltando (três erros), redeclaração e const reatribuído (com nota),
/// parêntese aberto, string sem fim e fim de arquivo. O stderr e a saída 1 batem byte a byte; o caminho absoluto do
/// oráculo (`/w/`) é o do diretório da bancada aqui.
#[test]
fn syntax_diagnostics_match_bun() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/cli_errors").canonicalize().unwrap();
    let mut total = 0;
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".js") {
            continue;
        }
        let out = zbun(&["build", "--no-bundle", dir.join(&name).to_str().unwrap()]);
        let expected = std::fs::read_to_string(dir.join(format!("{name}.expected.stderr"))).unwrap().replace("/w/", &format!("{}/", dir.display()));
        let code: i32 = std::fs::read_to_string(dir.join(format!("{name}.expected.code"))).unwrap().trim().parse().unwrap();
        total += 1;
        if out.status.code() != Some(code) || !out.stdout.is_empty() || String::from_utf8_lossy(&out.stderr) != expected {
            failures.push(format!("{name}\n--- esperado (saída {code})\n{expected}--- obtido (saída {:?})\n{}", out.status.code(), String::from_utf8_lossy(&out.stderr)));
        }
    }
    assert!(total >= 6, "o corpus de erros tem só {total} entradas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
