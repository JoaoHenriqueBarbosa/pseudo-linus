//! O bytecode que o gerador produz, contra o JavaScriptCore real: `tests/golden/bytecode_eval.txt` sai
//! de `scripts/gen-bytecode-golden.js`, rodado no bun 1.4.2 com `BUN_JSC_dumpGeneratedBytecodes=1`.
//! Cada programa é compilado como eval indireto (`zjsc::api::eval::evaluate_indirect_eval`), o dump do
//! `dataFile()` é capturado e o bloco `<eval>#...` passa pela mesma normalização do script (endereços,
//! `StructureID` e o par `[ptr/id]` das células viram `?`).
use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;

use zjsc::api::eval::evaluate_indirect_eval;
use zjsc::runtime::options::Options;
use zjsc::wtf::data_log::set_data_file;

const GOLDEN: &str = include_str!("golden/bytecode_eval.txt");

/// O `PrintStream` de teste: guarda o que o `dataLog` escreve.
struct Capture(Rc<RefCell<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// `0x` seguido de dígitos hexadecimais vira `0x?`; `StructureID: N` vira `StructureID: ?`; o
/// `[0x?/N` das células vira `[0x?/?`.
fn normalize(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if line[i..].starts_with("0x") {
            let mut end = i + 2;
            while end < bytes.len() && matches!(bytes[end], b'0'..=b'9' | b'a'..=b'f') {
                end += 1;
            }
            if end > i + 2 {
                out.push_str("0x?");
                if line[end..].starts_with('/') && bytes.get(end + 1).is_some_and(u8::is_ascii_digit) {
                    let mut digits = end + 1;
                    while digits < bytes.len() && bytes[digits].is_ascii_digit() {
                        digits += 1;
                    }
                    out.push_str("/?");
                    i = digits;
                } else {
                    i = end;
                }
                continue;
            }
        }
        if line[i..].starts_with("StructureID: ") && bytes.get(i + 13).is_some_and(u8::is_ascii_digit) {
            let mut digits = i + 13;
            while digits < bytes.len() && bytes[digits].is_ascii_digit() {
                digits += 1;
            }
            out.push_str("StructureID: ?");
            i = digits;
            continue;
        }
        let ch = line[i..].chars().next().expect("caractere");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// O bloco `<eval>#...` do dump e os das funções que ele chamou, até o fim, sem as linhas vazias do
/// fim (o script corta antes do `bun:main`, que aqui não existe).
fn eval_block(dump: &str) -> Option<String> {
    let lines: Vec<&str> = dump.split('\n').collect();
    let start = lines.iter().position(|line| line.starts_with("<eval>#"))?;
    let mut end = lines.len();
    while end > start && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    Some(lines[start..end].iter().map(|line| normalize(line)).collect::<Vec<_>>().join("\n"))
}

fn golden_cases() -> Vec<(&'static str, String)> {
    let mut cases = Vec::new();
    // O programa é a linha `=== fonte`; o `===` dentro do fonte (ou do bloco) não separa casos.
    let body = GOLDEN.strip_prefix("=== ").expect("começa pelo primeiro programa");
    for chunk in body.split("\n=== ") {
        let (source, block) = chunk.split_once('\n').expect("programa seguido do bloco");
        cases.push((source, block.trim_end_matches('\n').to_string()));
    }
    cases
}

#[test]
fn generated_bytecode_matches_bun() {
    Options::set_dump_generated_bytecodes(true);
    let mut failures = Vec::new();
    let cases = golden_cases();
    for (source, expected) in &cases {
        let buffer = Rc::new(RefCell::new(Vec::new()));
        set_data_file(Box::new(Capture(Rc::clone(&buffer))));
        let run = std::panic::catch_unwind(|| {
            let _ = evaluate_indirect_eval(source);
        });
        let dump = String::from_utf8_lossy(&buffer.borrow()).into_owned();
        match (run, eval_block(&dump)) {
            (Err(_), _) => failures.push(format!("=== {source}\n(pânico)")),
            (Ok(()), None) => failures.push(format!("=== {source}\n(sem bloco <eval>)")),
            (Ok(()), Some(actual)) if actual != *expected => {
                failures.push(format!("=== {source}\n--- esperado\n{expected}\n--- veio\n{actual}"))
            }
            _ => {}
        }
    }
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), cases.len(), failures.join("\n"));
}

#[test]
fn normalize_matches_script() {
    assert_eq!(normalize("[0x7f3a/12, StructureID: 4321] 0xdead"), "[0x?/?, StructureID: ?] 0x?");
}
