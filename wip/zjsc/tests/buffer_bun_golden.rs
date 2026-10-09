//! Golden do global `Buffer` contra o JavaScriptCore do bun: `tests/golden/buffer_bun.tsv` sai de
//! `scripts/gen-buffer-golden.js` rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global `R`
//! que ele grava. Só os casos de [`IN_SCOPE`] (índice da linha no TSV, base 0, faixas inclusivas) rodam; o resto entra
//! com as fatias seguintes de `wip/notes/buffer-plan.md`.
//! Ficam de fora, por LACUNA: as chaves próprias de `Buffer` e de `Buffer.prototype` (99 e 100), que dependem de
//! todo o protótipo (`xxxSlice`/`xxxWrite`, `copy`, `fill`, `read*`/`write*`, `swap*`) e da ordem `alloc ... isEncoding,
//! length, name, prototype, poolSize` (fatias 7 a 9).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_running_timers_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/buffer_bun.tsv");

const IN_SCOPE: &[(usize, usize)] = &[
    // Forma do global, `from` de string em cada codificação, `toString`, `from` de array/array-like/ArrayBuffer/visão e as
    // mensagens de `ERR_INVALID_ARG_TYPE` (symbol, bigint, função), `alloc`/`allocUnsafe`, `isBuffer`, `isEncoding`,
    // `byteLength` e `Buffer(n)`/`new Buffer(n)`.
    (0, 98),
    // `from` de ArrayBuffer compartilhando memória, preenchimento inválido, mensagens de size, `new Buffer(n)`, `compare`,
    // `concat`, `copyBytesFrom`, `equals`, `compare`, `indexOf`, `lastIndexOf`, `includes`, `slice`, `subarray`, `toJSON` e
    // `write`. Exige o TSV regenerado com `bun scripts/gen-buffer-golden.js`.
    (101, 170),
    // Ordem das chaves do construtor, `name` vazio, `read*`/`write*` (inteiros, BigInt, float, double, `byteLength`),
    // `fill`, `copy`, `swap*`, `xxxSlice`/`xxxWrite`, `toLocaleString`, `inspect` e os erros `ERR_OUT_OF_RANGE`,
    // `ERR_BUFFER_OUT_OF_BOUNDS`. Exige o TSV regenerado (273 linhas).
    (171, 272),
    // `Symbol.toStringTag`, `Symbol.species` e o `inspect` custom do protótipo, acessores `offset`/`parent`, `Bun.inspect`
    // com propriedades extras e os erros de receptor de `equals`/`compare`/`indexOf`. Exige o TSV regenerado (293 linhas).
    (273, 292),
    // base64 leniente (byte baixo da unidade, `=` encerra), `byteLength` de hex/base64 por tamanho, hex ímpar e inválido,
    // escrita parcial em latin1/ascii/utf16le/utf8, surrogate solto, utf8 inválido em `toString` (U+FFFD por sequência)
    // e `readIntBE`/`writeIntBE` com `byteLength` variável (offset obrigatório). Exige o TSV regenerado (384 linhas).
    (293, 383),
];

fn in_scope(line: usize) -> bool {
    IN_SCOPE.iter().any(|&(first, last)| (first..=last).contains(&line))
}

#[test]
fn buffer_match_bun() {
    let scoped: String = GOLDEN.lines().enumerate().filter(|&(number, _)| in_scope(number)).map(|(_, line)| format!("{line}\n")).collect();
    let expected: usize = IN_SCOPE.iter().map(|&(first, last)| last - first + 1).sum();
    assert_eq!(scoped.lines().count(), expected, "faixa de IN_SCOPE fora do golden");
    common::check(&scoped, common::NO_PRELUDES, expected, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_running_timers_reporting_uncaught(source, "buffer_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
