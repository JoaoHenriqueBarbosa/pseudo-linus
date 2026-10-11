//! Golden do JSBigInt contra o bun 1.4.2 (`scripts/gen-bigint-golden.js`): leitura em hexadecimal
//! (`parseInt` com sinal), multiplicação, potência e `toString` nas bases 10, 16, 2, 7 e 36. Os
//! tamanhos cruzam os limiares do Comba fixo, Karatsuba, Toom-3 e FFT. Resultados longos vêm como
//! `#` mais o FNV-1a de 32 bits do texto.

use zjsc::runtime::js_big_int::{ErrorParseMode, ImplResult, JSBigInt, ParseIntMode, ParseIntSign};
use zjsc::runtime::vm::VM;

const RADIXES: [u32; 5] = [10, 16, 2, 7, 36];

/// O sinal fica com o chamador, como no `parseInt(span, errorParseMode)` do C++: ele pula o `-` e
/// passa `Signed`.
fn parse_hex(vm: &VM, text: &str) -> JSBigInt {
    let negative = text.starts_with('-');
    let result = JSBigInt::parse_int_span_with_radix(
        None,
        vm,
        text.as_bytes(),
        negative as u32,
        16,
        ErrorParseMode::IgnoreExceptions,
        if negative { ParseIntSign::Signed } else { ParseIntSign::Unsigned },
        ParseIntMode::DisallowEmptyString,
    );
    heap(result, text)
}

fn heap(result: ImplResult, context: &str) -> JSBigInt {
    match result {
        ImplResult::Heap(value) => value,
        other => panic!("{context}: resultado inesperado {other:?}"),
    }
}

fn fnv(text: &[u8]) -> u32 {
    text.iter().fold(0x811c9dc5u32, |h, &c| (h ^ c as u32).wrapping_mul(0x01000193))
}

fn shown(vm: &VM, value: &JSBigInt, radix: u32, expected: &str) -> String {
    let text = String::from_utf8(JSBigInt::try_get_string(vm, value, radix).ascii()).unwrap();
    if expected.starts_with('#') { format!("#{:x}", fnv(text.as_bytes())) } else { text }
}

#[test]
fn bigint_matches_bun() {
    let vm = VM::new();
    let golden = include_str!("golden/bigint.tsv");
    let mut failures = Vec::new();
    for (line_number, line) in golden.lines().enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        let (op, a, b, expected) = (fields[0], fields[1], fields[2], &fields[3..]);
        let a_value = parse_hex(&vm, a);
        let value = match op {
            "str" => a_value,
            "mul" => heap(JSBigInt::multiply(&a_value, &parse_hex(&vm, b)).unwrap(), line),
            "pow" => heap(JSBigInt::exponentiate(&a_value, &parse_hex(&vm, b)).unwrap(), line),
            _ => unreachable!("{op}"),
        };
        for (radix, want) in RADIXES.iter().zip(expected) {
            let got = shown(&vm, &value, *radix, want);
            if got != *want {
                failures.push(format!("linha {} ({op}, base {radix}): esperado {want:.80}, obtido {got:.80}", line_number + 1));
            }
        }
    }
    assert!(failures.is_empty(), "{} divergências:\n{}", failures.len(), failures.join("\n"));
}
