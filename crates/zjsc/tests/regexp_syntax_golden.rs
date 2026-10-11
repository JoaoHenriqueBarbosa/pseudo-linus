//! Golden da compilação de padrões do Yarr contra o bun 1.4.2 (`scripts/gen-regexp-syntax-golden.js`):
//! para cada padrão e flags, `ok` quando o `new RegExp` aceita, ou a mensagem do `SyntaxError` sem o
//! prefixo "Invalid regular expression: ". Flags inválidas saem com `!` e a mensagem do construtor.

use zjsc::wtf::text::wtf_string::String as WtfString;
use zjsc::yarr::yarr::ExecutionMode;
use zjsc::yarr::yarr_error_code::{error_message, ErrorCode};
use zjsc::yarr::yarr_flags::parse_flags;
use zjsc::yarr::yarr_pattern::YarrPattern;

const PREFIX: &str = "Invalid regular expression: ";

fn compile(pattern: &str, flags: &str) -> String {
    let Some(flag_set) = parse_flags(flags.as_bytes()) else {
        return "!Invalid flags supplied to RegExp constructor.".to_string();
    };
    let units: Vec<u16> = pattern.encode_utf16().collect();
    let mut error = ErrorCode::NoError;
    YarrPattern::new(&WtfString::from_utf16(&units), flag_set, &mut error, ExecutionMode::IncludeSubpatterns);
    if error == ErrorCode::NoError {
        return "ok".to_string();
    }
    let message = error_message(error);
    message.strip_prefix(PREFIX).map_or_else(|| format!("!{message}"), str::to_string)
}

#[test]
fn regexp_syntax_matches_bun() {
    let golden = include_str!("golden/regexp-syntax.tsv");
    let mut failures = Vec::new();
    for (line_number, line) in golden.lines().enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        let pattern: String = serde_json_like_unquote(fields[0]);
        let got = compile(&pattern, fields[1]);
        if got != fields[2] {
            failures.push(format!("linha {}: /{}/{}: esperado {:?}, obtido {:?}", line_number + 1, pattern, fields[1], fields[2], got));
        }
    }
    assert!(failures.is_empty(), "{} divergências:\n{}", failures.len(), failures.join("\n"));
}

/// Desfaz o `JSON.stringify` de uma string: aspas nas pontas e os escapes que ele produz.
fn serde_json_like_unquote(quoted: &str) -> String {
    let inner = &quoted[1..quoted.len() - 1];
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}
