//! Golden de ordem de chaves próprias contra o JavaScriptCore do bun: `tests/golden/key_order_bun.tsv` sai de
//! `scripts/gen-key-order-golden.js`, rodado no bun 1.4.2. Cada linha é um objeto (globais do ECMAScript e do JSC,
//! seus `.prototype`, classes de Intl e Temporal): a expressão e a string de `ser` (Reflect.ownKeys com símbolos pela
//! descrição, flags writable/enumerable/configurable, tipo get/set/valor, length e name das funções). Cada objeto roda
//! como um programa em realm novo; o texto de `tests/golden/key_order_serializer.js` é o mesmo dos dois lados.
mod common;

use common::json_string;
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_indirect_eval;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/key_order_bun.tsv");
const SERIALIZER: &str = include_str!("golden/key_order_serializer.js");

/// Roda a expressão em realm novo e devolve a string serializada, ou o motivo de não ter devolvido.
fn run(expression: &str) -> Result<String, String> {
    let program = format!("{SERIALIZER}\n{expression}");
    match catch_unwind(AssertUnwindSafe(|| evaluate_indirect_eval(&program))) {
        Ok(Ok(value)) => {
            let bytes = value.to_wtf_string().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Err(_)) => Err("o programa lançou exceção".to_string()),
        Err(_) => Err("pânico".to_string()),
    }
}

/// Primeira chave em que as duas listas divergem, para o relatório caber numa linha.
fn first_difference(expected: &str, actual: &str) -> String {
    let (expected, actual): (Vec<&str>, Vec<&str>) = (expected.split(';').collect(), actual.split(';').collect());
    for index in 0..expected.len().max(actual.len()) {
        let (left, right) = (expected.get(index), actual.get(index));
        if left != right {
            return format!("posição {index}: esperado {left:?}, veio {right:?}");
        }
    }
    "iguais".to_string()
}

#[test]
fn own_key_order_matches_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (expression, expected) = line.split_once('\t').expect("expressão e resultado");
        let (expression, expected) = (json_string(expression), json_string(expected));
        total += 1;
        match run(&expression) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{expression}: {}", first_difference(&expected, &actual))),
            Err(reason) => failures.push(format!("{expression}: {reason}")),
        }
    }
    assert!(total >= 100, "golden com só {total} objetos");
    assert!(failures.is_empty(), "{} de {} objetos divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
