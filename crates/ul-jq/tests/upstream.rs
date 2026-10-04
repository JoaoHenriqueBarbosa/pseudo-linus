//! Suítes do jq 1.7.1 (`tests/*.test` da tag jq-1.7.1, em `testbench/corpus/upstream/jq/`), com o
//! formato e a regra de aprovação do `src/jq_test.c`: programa numa linha, entrada na seguinte, saídas
//! esperadas até linha em branco ou comentário; `%%FAIL` exige erro de compilação e a mesma primeira
//! linha de mensagem (salvo `%%FAIL IGNORE MSG`); comparação por valor (`jv_equal`); erro depois das
//! saídas esperadas não reprova; saída a mais reprova. Cada teste roda como `jq -c PROGRAMA` com a
//! entrada no stdin, sobre o testkit.
//!
//! Os testes que o próprio jq 1.7.1 do Debian não passa (`jq --run-tests` no oráculo) ficam de fora,
//! como no F04: 723 testes.
//!
//! `cargo test -p ul-jq --test upstream -- --nocapture` mostra o placar e as falhas;
//! `JQ_SUITE_FILTER=onig` restringe a um arquivo, `JQ_SUITE_LINE=123` a uma linha.

use std::path::PathBuf;

use jaq_json::jqparse;
use sysabi::testkit::TestKit;

/// Linhas que falham no jq do oráculo (`jq --run-tests`, cache do F04).
const ORACLE_FAILING: &[(&str, u32)] = &[
    ("jq.test", 1584),
    ("jq.test", 1588),
    ("jq.test", 1592),
    ("jq.test", 1596),
    ("jq.test", 1601),
    ("jq.test", 1605),
    ("jq.test", 1609),
    ("jq.test", 1613),
    ("jq.test", 1641),
    ("jq.test", 1645),
    ("jq.test", 1649),
    ("jq.test", 1661),
    ("jq.test", 1942),
    ("man.test", 661),
    ("man.test", 665),
];

const FILES: &[&str] = &["jq.test", "man.test", "onig.test", "manonig.test", "base64.test", "optional.test"];

struct JqTest {
    file: String,
    line: u32,
    program: String,
    input: String,
    expected: Vec<String>,
    /// `Some(None)` = `%%FAIL IGNORE MSG`; `Some(Some(msg))` = `%%FAIL` com mensagem.
    fail: Option<Option<String>>,
}

fn skipline(l: &str) -> bool {
    let t = l.trim_start_matches([' ', '\t']);
    t.is_empty() || t.starts_with('#') || t == "\n"
}

fn parse(file: &str, text: &str) -> Vec<JqTest> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut i = 0;
    let mut out = Vec::new();
    let mut must_fail: Option<bool> = None;
    let strip = |l: &str| l.strip_suffix('\n').unwrap_or(l).to_string();
    while i < lines.len() {
        let raw = lines[i];
        i += 1;
        if skipline(raw) {
            continue;
        }
        if raw == "%%FAIL\n" || raw == "%%FAIL IGNORE MSG\n" {
            must_fail = Some(raw == "%%FAIL\n");
            continue;
        }
        let line = i as u32;
        let program = strip(raw);
        if let Some(check) = must_fail.take() {
            let Some(msg) = lines.get(i) else { break };
            i += 1;
            out.push(JqTest {
                file: file.into(),
                line,
                program,
                input: String::new(),
                expected: Vec::new(),
                fail: Some(check.then(|| strip(msg))),
            });
            continue;
        }
        let Some(input) = lines.get(i) else { break };
        i += 1;
        let mut expected = Vec::new();
        while i < lines.len() {
            let l = lines[i];
            i += 1;
            if skipline(l) {
                break;
            }
            expected.push(strip(l));
        }
        out.push(JqTest { file: file.into(), line, program, input: strip(input), expected, fail: None });
    }
    out
}

fn suite_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testbench/corpus/upstream/jq")
}

/// Resultado de um teste: passou, ou o motivo.
fn run_test(t: &JqTest) -> Result<(), String> {
    // Programa começando com '-' viraria opção; espaço na frente não muda o programa jq.
    let program = if t.program.starts_with('-') { format!(" {}", t.program) } else { t.program.clone() };
    let kit = TestKit::new().programs(ul_jq::programs()).dir("/work", 0o755).cwd("/work");
    let stdin = if t.fail.is_some() { b"null".to_vec() } else { t.input.as_bytes().to_vec() };
    let r = kit.run(&["jq", "-c", &program], &stdin);
    let stderr = r.stderr_str();
    if let Some(fail) = &t.fail {
        let failed_compile = r.code() != 0 && r.stdout.is_empty();
        let first_line = stderr.lines().next().unwrap_or("").to_string();
        if !failed_compile {
            return Err(format!("compilou (exit {}, stdout {:?})", r.code(), r.stdout_str()));
        }
        if let Some(msg) = fail {
            if first_line != *msg {
                return Err(format!("mensagem: {first_line:?}, esperada {msg:?}"));
            }
        }
        return Ok(());
    }
    let mut parser = jqparse::Parser::new(false);
    parser.set_buf(&r.stdout, false);
    let mut actual = Vec::new();
    loop {
        match parser.next() {
            jqparse::Next::Value(v) => actual.push(v),
            jqparse::Next::Error(e) => return Err(format!("saída não é JSON ({e}): {:?}", r.stdout_str())),
            jqparse::Next::None => break,
        }
    }
    let mut expected = Vec::new();
    for e in &t.expected {
        match jqparse::parse_single(e.as_bytes()) {
            Ok(v) => expected.push(v),
            Err(err) => return Err(format!("esperado inválido {e:?}: {err}")),
        }
    }
    let ok = actual.len() == expected.len() && actual.iter().zip(&expected).all(|(a, e)| a == e);
    if ok {
        Ok(())
    } else {
        let got: Vec<String> = actual.iter().take(6).map(|v| v.to_json()).collect();
        Err(format!("esperado {:?}, obtido {:?} (exit {}, stderr {:?})", t.expected, got, r.code(), stderr.chars().take(200).collect::<String>()))
    }
}

#[test]
fn jq_upstream_suites() {
    let filter = std::env::var("JQ_SUITE_FILTER").ok();
    let only_line: Option<u32> = std::env::var("JQ_SUITE_LINE").ok().and_then(|l| l.parse().ok());
    let mut total = 0;
    let mut passed = 0;
    let mut failures = Vec::new();
    for file in FILES {
        if filter.as_deref().is_some_and(|f| !file.starts_with(f)) {
            continue;
        }
        let text = std::fs::read_to_string(suite_dir().join(file)).expect("suíte do jq em testbench/corpus/upstream/jq");
        for t in parse(file, &text) {
            if ORACLE_FAILING.contains(&(file, t.line)) {
                continue;
            }
            if only_line.is_some_and(|l| l != t.line) {
                continue;
            }
            total += 1;
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_test(&t)));
            match r {
                Ok(Ok(())) => passed += 1,
                Ok(Err(why)) => failures.push(format!("{}:{} {:?}: {why}", t.file, t.line, t.program)),
                Err(_) => failures.push(format!("{}:{} {:?}: panic", t.file, t.line, t.program)),
            }
        }
    }
    for f in &failures {
        eprintln!("FALHA {f}");
    }
    eprintln!("suítes do jq 1.7.1: {passed}/{total} ({:.1}%)", 100.0 * passed as f64 / total.max(1) as f64);
    assert!(total > 0);
}
