//! Golden de BigInt contra o JavaScriptCore do bun: `tests/golden/bigint_bun.tsv` sai de
//! `scripts/gen-bigint-bun-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável
//! global `R` que ele grava: o resultado como string, ou `Nome: mensagem` quando a expressão lança. Cobre literais,
//! aritmética nos tamanhos de borda (1 a 1000 bits), divisão por zero, shifts, bitwise em complemento de dois,
//! comparação com Number e String, mistura de tipos, `BigInt(valor)`, `asIntN`/`asUintN`, `toString(radix)`,
//! `toLocaleString`, conversões para Number, BigInt64Array/BigUint64Array/Atomics/DataView e os limites de tamanho.
//! Resultados com mais de 4000 unidades UTF-16 saem como `#len<N>` nos dois lados.
mod common;

const GOLDEN: &str = include_str!("golden/bigint_bun.tsv");

#[test]
fn bigint_matches_bun() {
    // Mesmo corte do gerador (`gen-bigint-bun-golden.js`): acima de 4000 unidades UTF-16 só o comprimento é comparado.
    common::check(GOLDEN, common::NO_PRELUDES, 1500, |source| {
        common::guarded_units(|| common::EvalMode::IndirectEval.evaluate(source, "bigint_case.js", "R")).map(|result| {
            let units = result.len();
            if units > 4000 { common::Units::from(format!("#len{units}")) } else { result }
        })
    });
}
