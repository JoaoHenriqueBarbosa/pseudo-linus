//! Golden do `exec` do interpretador do Yarr contra o bun 1.4.2 (`scripts/gen-regexp-exec-golden.js`):
//! índice do casamento e o par `[início,fim]` de cada grupo (`-` para grupo que não participou).

use zjsc::wtf::text::string_view::StringView;
use zjsc::wtf::text::wtf_string::String as WtfString;
use zjsc::yarr::yarr::{ExecutionMode, OFFSET_NO_MATCH};
use zjsc::yarr::yarr_error_code::ErrorCode;
use zjsc::yarr::yarr_flags::parse_flags;
use zjsc::yarr::yarr_interpreter::{byte_compile, interpret};
use zjsc::yarr::yarr_pattern::YarrPattern;

fn unquote(quoted: &str) -> Vec<u16> {
    let inner: Vec<u16> = quoted[1..quoted.len() - 1].encode_utf16().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < inner.len() {
        if inner[i] != b'\\' as u16 {
            out.push(inner[i]);
            i += 1;
            continue;
        }
        i += 1;
        match char::from_u32(inner[i] as u32).unwrap() {
            'n' => out.push(10),
            't' => out.push(9),
            'b' => out.push(8),
            'r' => out.push(13),
            'u' => {
                let hex: String = inner[i + 1..i + 5].iter().map(|&c| c as u8 as char).collect();
                out.push(u16::from_str_radix(&hex, 16).unwrap());
                i += 4;
            }
            other => out.push(other as u16),
        }
        i += 1;
    }
    out
}

fn exec(pattern: &[u16], flags: &str, input: &[u16]) -> String {
    let flag_set = parse_flags(flags.as_bytes()).unwrap();
    let mut error = ErrorCode::NoError;
    let mut yarr = YarrPattern::new(&WtfString::from_utf16(pattern), flag_set, &mut error, ExecutionMode::IncludeSubpatterns);
    assert_eq!(error, ErrorCode::NoError);
    let groups = yarr.num_subpatterns as usize + 1;
    let bytecode = byte_compile(&mut yarr, &mut error).expect("byte_compile");
    let mut output = vec![OFFSET_NO_MATCH; groups * 2];
    let result = interpret(&bytecode, StringView::from(input), 0, &mut output);
    if result == OFFSET_NO_MATCH {
        return "null".to_string();
    }
    let mut parts = vec![output[0].to_string()];
    for g in 0..groups {
        let (start, end) = (output[2 * g], output[2 * g + 1]);
        parts.push(if start == OFFSET_NO_MATCH { "-".to_string() } else { format!("[{start},{end}]") });
    }
    parts.join(" ")
}

#[test]
fn regexp_exec_matches_bun() {
    let golden = include_str!("golden/regexp-exec.tsv");
    let mut failures = Vec::new();
    for (line_number, line) in golden.lines().enumerate() {
        let f: Vec<&str> = line.split('\t').collect();
        let (pattern, input) = (unquote(f[0]), unquote(f[2]));
        if std::env::var_os("YARR_TRACE").is_some() { eprintln!("{}", line_number + 1); }
        let got = exec(&pattern, f[1], &input);
        if got != f[3] {
            failures.push(format!("linha {}: {} /{}/{}: esperado {}, obtido {}", line_number + 1, f[2], f[0], f[1], f[3], got));
        }
    }
    assert!(failures.is_empty(), "{} divergências:\n{}", failures.len(), failures.join("\n"));
}
