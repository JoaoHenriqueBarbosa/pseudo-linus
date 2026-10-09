//! Marco `1 + 1` e seguintes, ponta a ponta contra o JavaScriptCore real: `tests/golden/e2e_numeric.tsv`
//! sai de `scripts/gen-e2e-golden.js`, rodado no bun 1.4.2. Cada linha é um programa e os bits do
//! `double` do valor de conclusão. O programa passa por `zjsc::api::eval::evaluate_script` (parser,
//! gerador de bytecode, LLInt) e o resultado tem de ter os mesmos bits.
//!
//! Os programas de função (`f()`) e de `var`/`let` só passam quando o laço de despacho cobrir os
//! opcodes deles; a falha mostra o programa e o que veio.
use zjsc::api::eval::{describe_exception, evaluate_script};

const GOLDEN: &str = include_str!("golden/e2e_numeric.tsv");

fn number_bits(value: &zjsc::runtime::js_value::JSValue) -> Option<u64> {
    value.is_number().then(|| value.as_number().to_bits())
}

#[test]
fn numeric_programs_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected_hex) = line.split_once('\t').expect("linha com duas colunas");
        let expected = u64::from_str_radix(expected_hex, 16).expect("bits em hexadecimal");
        total += 1;
        match evaluate_script(source) {
            Ok(value) => match number_bits(&value) {
                Some(bits) if bits == expected => {}
                Some(bits) => failures.push(format!("{source}: esperado {expected:016x}, veio {bits:016x}")),
                None => failures.push(format!("{source}: esperado {expected:016x}, veio valor que não é número")),
            },
            Err(exception) => failures.push(format!("{source}: lançou exceção ({})", describe_exception(&exception))),
        }
    }
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
