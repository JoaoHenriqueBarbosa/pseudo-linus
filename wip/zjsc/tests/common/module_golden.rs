//! O conferidor dos goldens de grafos de módulos ES (`module_bun`, `module_more_bun`, `module_edge_bun`,
//! `module_meta_resolve_bun`). Cada linha do tsv é um mapa de arquivos (JSON, `main.mjs` é o ponto de entrada), a saída
//! esperada `{"log":[...],"error":...}` e, quando o bun deu saídas diferentes entre execuções (a leitura de arquivos
//! dele corre num pool de threads, então duas importações paralelas assentam em ordem variável), uma terceira coluna
//! com o array JSON das outras saídas aceitas (ver `scripts/golden-alternatives.js`).
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::module::{evaluate_module_map_until, LogPoint};

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

/// Lê o array `["...", ...]` de strings JSON (as saídas alternativas).
fn parse_strings(json: &str) -> Vec<String> {
    let mut chars = json.chars().peekable();
    assert_eq!(chars.next(), Some('['));
    let mut strings = Vec::new();
    loop {
        match chars.next().expect("array incompleto") {
            ']' => break,
            ',' => {}
            '"' => strings.push(read_json_string(&mut chars)),
            other => panic!("caractere inesperado {other:?} no array"),
        }
    }
    strings
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

/// O resultado no formato do golden (`{"log":...,"error":...}`), ou o motivo de o porte não ter chegado lá.
fn run(files: &[(String, String)], log_point: LogPoint) -> Result<String, String> {
    match catch_unwind(AssertUnwindSafe(|| evaluate_module_map_until(files, "main.mjs", log_point))) {
        Ok(outcome) => {
            let error = outcome.error.map_or_else(|| "null".to_owned(), |text| json_quote(&text));
            Ok(format!("{{\"log\":{},\"error\":{error}}}", outcome.log_json))
        }
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

/// Confere cada linha de `golden` contra o porte, lendo o `log` no ponto `log_point` do gerador; `minimum` guarda
/// contra um golden truncado.
pub fn check_module_golden(golden: &str, minimum: usize, log_point: LogPoint) {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in golden.lines().filter(|line| !line.is_empty()) {
        let mut columns = line.split('\t');
        let files_json = columns.next().expect("linha com arquivos");
        let expected = columns.next().expect("linha com resultado");
        let alternatives = columns.next().map(parse_strings).unwrap_or_default();
        total += 1;
        match run(&parse_files(files_json), log_point) {
            Ok(actual) if actual == expected || alternatives.contains(&actual) => {}
            Ok(actual) => failures.push(format!("{files_json}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{files_json}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(total >= minimum, "o golden tem só {total} casos");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
