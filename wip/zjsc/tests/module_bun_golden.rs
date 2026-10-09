//! Golden de módulos ES contra o JavaScriptCore real: `tests/golden/module_bun.tsv` sai de
//! `scripts/gen-module-golden.js`, rodado no bun. Cada linha é um mapa de arquivos (JSON, `main.mjs` é o
//! ponto de entrada) e a saída esperada `{"log":[...],"error":...}`: o array global `log` (alimentado por
//! `L(x)`) depois de esvaziar as microtarefas, e o `Nome: mensagem` do erro que rejeitou o import, ou `null`.
//! Os módulos rodam num loader em memória (`zjsc::api::module::evaluate_module_map`), cada caso num realm novo.
//!
//! Os erros da camada do bun (`BuildMessage`, `ResolveMessage`, `AggregateError`) também são comparados
//! pelo texto exato `Nome: mensagem`: o porte imita essa camada em `src/runtime/js_module_loader.rs`.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::module::evaluate_module_map;

const GOLDEN: &str = include_str!("golden/module_bun.tsv");

/// Lê uma string JSON a partir de `chars` (já depois da aspa de abertura).
fn read_json_string(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut out = String::new();
    let mut pending_high: Option<u32> = None;
    while let Some(c) = chars.next() {
        let unit = match c {
            '"' => break,
            '\\' => match chars.next().expect("escape incompleto") {
                'n' => '\n' as u32,
                't' => '\t' as u32,
                'r' => '\r' as u32,
                'b' => 8,
                'f' => 12,
                'u' => {
                    let hex: String = (0..4).map(|_| chars.next().expect("\\u incompleto")).collect();
                    u32::from_str_radix(&hex, 16).expect("hex inválido")
                }
                other => other as u32,
            },
            other => other as u32,
        };
        match (pending_high.take(), unit) {
            (Some(high), low) if (0xDC00..0xE000).contains(&low) => {
                out.push(char::from_u32(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)).expect("par substituto"));
            }
            (_, high) if (0xD800..0xDC00).contains(&high) => pending_high = Some(high),
            (_, other) => out.push(char::from_u32(other).unwrap_or('\u{fffd}')),
        }
    }
    out
}

/// Lê o mapa `{"nome":"fonte",...}` (só strings) na ordem em que aparece.
fn parse_files(json: &str) -> Vec<(String, String)> {
    let mut chars = json.chars().peekable();
    assert_eq!(chars.next(), Some('{'));
    let mut files = Vec::new();
    loop {
        match chars.next().expect("mapa incompleto") {
            '}' => break,
            ',' => {}
            '"' => {
                let name = read_json_string(&mut chars);
                assert_eq!(chars.next(), Some(':'));
                assert_eq!(chars.next(), Some('"'));
                files.push((name, read_json_string(&mut chars)));
            }
            other => panic!("caractere inesperado {other:?} no mapa"),
        }
    }
    files
}

/// `JSON.stringify(text)`.
fn json_quote(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\u{8}' => quoted.push_str("\\b"),
            '\u{c}' => quoted.push_str("\\f"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            control if (control as u32) < 0x20 => quoted.push_str(&format!("\\u{:04x}", control as u32)),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

/// O resultado no formato do golden, ou o motivo de o porte não ter chegado lá.
fn run(files: &[(String, String)]) -> Result<(String, Option<String>), String> {
    let outcome = catch_unwind(AssertUnwindSafe(|| evaluate_module_map(files, "main.mjs")));
    match outcome {
        Ok(outcome) => Ok((outcome.log_json, outcome.error)),
        Err(panic) => {
            let reason = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
                .unwrap_or_default();
            Err(format!("pânico: {reason}"))
        }
    }
}

/// Separa `{"log":<json>,"error":<null ou string>}` em log (texto JSON) e erro (texto JSON).
fn split_expected(expected: &str) -> (&str, &str) {
    let body = expected.strip_prefix("{\"log\":").expect("formato do esperado");
    let (log, rest) = body.rsplit_once(",\"error\":").expect("formato do esperado");
    (log, rest.strip_suffix('}').expect("formato do esperado"))
}

#[test]
fn module_graphs_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (files_json, expected) = line.split_once('\t').expect("linha com arquivos e resultado");
        let files = parse_files(files_json);
        let (expected_log, expected_error) = split_expected(expected);
        total += 1;
        match run(&files) {
            Ok((log, error)) => {
                let actual_error = error.map_or_else(|| "null".to_owned(), |text| json_quote(&text));
                if log != expected_log || actual_error != expected_error {
                    failures.push(format!(
                        "{files_json}\n    esperado log {expected_log} erro {expected_error}\n    veio     log {log} erro {actual_error}"
                    ));
                }
            }
            Err(reason) => failures.push(format!("{files_json}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(total >= 200, "o golden tem só {total} casos");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
