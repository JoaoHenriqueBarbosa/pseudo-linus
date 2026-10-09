//! Golden de `crypto`, `Crypto` e `SubtleCrypto` contra o JavaScriptCore do bun: `tests/golden/crypto_bun.tsv` sai de
//! `scripts/gen-crypto-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global `R` que ele
//! grava, lida depois do esvaziamento das promessas. Valores aleatórios entram só por forma. A estática `supports` entra por inteiro
//! (operações × algoritmos, dicionários, terceiro argumento, RSA, ChaCha20-Poly1305, comprimento do `deriveBits`). Lacuna conhecida:
//! a ordem exata de leitura dos membros do dicionário (um `Proxy` conta `name` duas vezes, e o HMAC repete `name,hash,length`), que
//! nenhum caso do golden observa, e `publicExponent` sobre `ArrayBuffer` destacado (o bun aceita) ou redimensionável (o bun recusa).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/crypto_bun.tsv");

#[test]
fn crypto_matches_bun() {
    common::check(GOLDEN, common::NO_PRELUDES, 120, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "crypto_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
