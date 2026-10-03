//! Suítes de teste do jq 1.7.1 (`tests/*.test`): formato, validação no oráculo e execução nos
//! candidatos.
//!
//! O formato e a regra de aprovação seguem o `src/jq_test.c` da 1.7.1: programa numa linha, entrada na
//! seguinte, saídas esperadas até uma linha em branco ou comentário; blocos `%%FAIL` exigem erro de
//! compilação (e a mensagem, salvo `%%FAIL IGNORE MSG`). A comparação é por valor JSON (`jv_equal`:
//! números como double, objetos sem ordem de chave). Como no `jq_test.c`, um erro depois das saídas
//! esperadas não reprova o teste; saída a mais reprova.
//!
//! Antes de pontuar candidatos, cada arquivo roda no oráculo com `jq --run-tests`; testes que o próprio
//! jq do Debian 13 não passa ficam de fora (e são contados).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use harness::{Candidate, Invocation, MemTree, Oracle};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct JqTest {
    pub file: String,
    /// Linha do programa no arquivo (é como o `--run-tests` identifica o teste).
    pub line: u32,
    pub program: String,
    pub input: String,
    pub expected: Vec<String>,
    /// `Some(None)` = `%%FAIL IGNORE MSG`; `Some(Some(msg))` = `%%FAIL` com mensagem.
    pub fail: Option<Option<String>>,
}

fn skipline(l: &str) -> bool {
    let t = l.trim_start_matches([' ', '\t']);
    t.is_empty() || t.starts_with('#') || t == "\n"
}

pub fn parse(file: &str, text: &str) -> Vec<JqTest> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut i = 0;
    let mut out = Vec::new();
    let mut must_fail: Option<bool> = None;
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
        let prog = raw.strip_suffix('\n').unwrap_or(raw).to_string();
        if let Some(check) = must_fail.take() {
            let Some(msg) = lines.get(i) else { break };
            i += 1;
            let msg = msg.strip_suffix('\n').unwrap_or(msg).to_string();
            out.push(JqTest {
                file: file.into(),
                line,
                program: prog,
                input: String::new(),
                expected: Vec::new(),
                fail: Some(check.then_some(msg)),
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
            expected.push(l.strip_suffix('\n').unwrap_or(l).to_string());
        }
        out.push(JqTest {
            file: file.into(),
            line,
            program: prog,
            input: input.strip_suffix('\n').unwrap_or(input).to_string(),
            expected,
            fail: None,
        });
    }
    out
}

/// Linhas (do programa) dos testes que falham no jq do oráculo, por arquivo.
#[derive(Clone, Debug, Default, Serialize, serde::Deserialize)]
pub struct OracleRun {
    pub failing_lines: BTreeSet<u32>,
    pub summary: String,
}

pub fn parse_run_tests_output(stdout: &str) -> OracleRun {
    let mut failing = BTreeSet::new();
    let mut current: Option<u32> = None;
    let mut summary = String::new();
    for l in stdout.lines() {
        if let Some(rest) = l.strip_prefix("Test #") {
            current = rest.rsplit("at line number ").next().and_then(|n| n.trim().parse().ok());
        } else if l.starts_with("***") {
            if let Some(c) = current {
                failing.insert(c);
            }
        } else if l.contains(" tests passed (") {
            summary = l.to_string();
        }
    }
    OracleRun { failing_lines: failing, summary }
}

/// Roda `jq --run-tests` no oráculo pra cada arquivo (com cache em disco).
pub fn oracle_runs(files: &[(String, String)], cache: &Path) -> Result<BTreeMap<String, OracleRun>> {
    let key = harness::memtree::sha256_hex(serde_json::to_string(files)?.as_bytes());
    if let Ok(text) = std::fs::read_to_string(cache)
        && let Ok((k, v)) = serde_json::from_str::<(String, BTreeMap<String, OracleRun>)>(&text)
        && k == key
    {
        return Ok(v);
    }
    let oracle = Oracle::locate()?;
    let mut out = BTreeMap::new();
    for (name, text) in files {
        let mut tree = MemTree::new();
        tree.insert(name, harness::Entry::file(text.as_bytes().to_vec(), 0o644));
        let outcome = oracle
            .run_script(&format!("run-tests-{name}"), &format!("jq --run-tests {name}"), tree)
            .with_context(|| format!("jq --run-tests {name} no oráculo"))?;
        out.insert(name.clone(), parse_run_tests_output(&String::from_utf8_lossy(outcome.stdout.as_slice())));
    }
    std::fs::write(cache, serde_json::to_string(&(key, &out))?)?;
    Ok(out)
}

/// Igualdade do `jv_equal`: números por valor double, objetos sem ordem.
pub fn jv_equal(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value as V;
    match (a, b) {
        (V::Number(x), V::Number(y)) => num(x) == num(y) || (num(x).is_nan() && num(y).is_nan()),
        (V::Array(x), V::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| jv_equal(p, q)),
        (V::Object(x), V::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| jv_equal(v, w)))
        }
        _ => a == b,
    }
}

fn num(n: &serde_json::Number) -> f64 {
    n.as_str().parse::<f64>().unwrap_or(f64::NAN)
}

/// Valores JSON de uma saída (vazio se não for JSON válido).
pub fn parse_stream(bytes: &[u8]) -> Option<Vec<serde_json::Value>> {
    let mut out = Vec::new();
    for v in serde_json::Deserializer::from_slice(bytes).into_iter::<serde_json::Value>() {
        out.push(v.ok()?);
    }
    Some(out)
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct TestResult {
    pub file: String,
    pub line: u32,
    pub program: String,
    pub pass: bool,
    /// Pra `%%FAIL`: falhou na compilação, mas a mensagem pode não bater.
    pub pass_lenient: bool,
    pub class: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// Executa um teste num candidato através da CLI (`jq -c PROGRAMA` com a entrada no stdin).
pub fn run_test(candidate: &dyn Candidate, t: &JqTest) -> TestResult {
    // Programa começando com '-' viraria opção; espaço na frente não muda o programa jq.
    let program = if t.program.starts_with('-') { format!(" {}", t.program) } else { t.program.clone() };
    let inv = Invocation {
        case_id: format!("{}-{}", t.file, t.line),
        argv: vec!["jq".into(), "-c".into(), program],
        script: None,
        stdin: if t.fail.is_some() { b"null".to_vec() } else { t.input.as_bytes().to_vec() },
        files: MemTree::new(),
        env: Default::default(),
        faketime: None,
    };
    let out = crate::exec::run_guarded(candidate, &inv);
    let class = classify(t);
    let mk = |pass: bool, lenient: bool, detail: String| TestResult {
        file: t.file.clone(),
        line: t.line,
        program: t.program.clone(),
        pass,
        pass_lenient: lenient,
        class: class.to_string(),
        detail,
    };
    if let Some(why) = &out.unsupported {
        return mk(false, false, format!("unsupported: {why}"));
    }
    if out.timed_out {
        return mk(false, false, "timeout".into());
    }
    if let Some(fail) = &t.fail {
        let failed_compile = out.exit != Some(0) && out.stdout.is_empty();
        let first_line = String::from_utf8_lossy(out.stderr.as_slice()).lines().next().unwrap_or("").to_string();
        let msg_ok = match fail {
            None => true,
            Some(msg) => first_line == *msg,
        };
        let detail = if failed_compile && !msg_ok { format!("mensagem: {first_line}") } else if failed_compile { String::new() } else { "compilou".into() };
        return mk(failed_compile && msg_ok, failed_compile, detail);
    }
    let Some(actual) = parse_stream(out.stdout.as_slice()) else {
        return mk(false, false, format!("saída não é JSON: {}", out.stdout.preview(80)));
    };
    let mut expected = Vec::new();
    for e in &t.expected {
        match serde_json::from_str::<serde_json::Value>(e) {
            Ok(v) => expected.push(v),
            // O jq aceita literais que o serde_json não aceita (ex.: nan); compara como texto.
            Err(_) => expected.push(serde_json::Value::String(format!("<literal {e}>"))),
        }
    }
    let ok = actual.len() == expected.len() && actual.iter().zip(&expected).all(|(a, e)| jv_equal(a, e));
    let detail = if ok {
        String::new()
    } else {
        let got: Vec<String> = actual.iter().take(4).map(|v| v.to_string()).collect();
        format!(
            "esperado {:?}, obtido {:?} (exit {:?}, stderr {})",
            t.expected,
            got,
            out.exit,
            out.stderr.preview(100)
        )
    };
    mk(ok, ok, detail)
}

/// Classe de divergência pelo conteúdo do teste (heurística documentada no README).
pub fn classify(t: &JqTest) -> &'static str {
    let p = &t.program;
    let has = |words: &[&str]| words.iter().any(|w| p.contains(w));
    if t.fail.is_some() {
        return "erros/mensagens";
    }
    if has(&["test(", "match(", "capture(", "sub(", "gsub(", "splits(", "scan("]) {
        return "regex";
    }
    if has(&["strftime", "strptime", "mktime", "gmtime", "todate", "fromdate", "dateadd", "datesub", "localtime", "now", "date"]) {
        return "datas";
    }
    if has(&["sort", "group_by", "unique", "min_by", "max_by", "keys", "to_entries"]) {
        return "ordenação";
    }
    let joined = t.expected.join(" ");
    if has(&["error", "try", "catch", "?//", "halt"]) || joined.contains("Cannot") || joined.contains("cannot") {
        return "erros/mensagens";
    }
    if has(&["tostring", "tojson", "@text", "@json", "infinite", "nan", "1e", "E+", "significand", "frexp", "ldexp", "pow", "log", "exp", "%", "/"])
        || joined.contains("e+")
        || joined.contains("E+")
        || joined.contains('.')
    {
        return "números";
    }
    if has(&["input", "$__loc__", "$ENV", "env", "getpath", "path(", "paths", "del(", "setpath", "to_entries", "|=", "+=", "limit", "first", "until", "while", "repeat", "range", "reduce", "foreach", "label"]) {
        return "semântica (caminhos, geradores, atualização)";
    }
    "outros"
}
