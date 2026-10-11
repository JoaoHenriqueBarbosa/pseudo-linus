//! Golden de `DOMException` contra o JavaScriptCore do bun: `tests/golden/dom_exception_bun.tsv` sai de
//! `scripts/gen-dom-exception-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava. Cobre o descritor do global, o construtor e as 25 constantes, o protótipo
//! e seus acessores, instâncias, argumentos padrão e coerções, `cause`, a tabela de códigos por nome, chamada
//! sem `new`, `this` inválido nos getters, subclasse e `Reflect.construct`, a atribuição aos acessores e as
//! propriedades próprias da exceção lançada por função nativa (sem `sourceURL` nem a cauda da `stack`, que
//! dependem do hospedeiro). As divergências conhecidas estão em `src/runtime/js_dom_exception.rs`.
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/dom_exception_bun.tsv");

/// Linha 214: dois `atob('*')` lançados na mesma linha do programa têm `stack` igual no bun (`true`) e diferente no motor (`false`): o
/// `stack` do DOMException lançado por função nativa leva a posição do chamador (coluna), que no motor difere entre as duas chamadas (as colunas
/// dos dois `atob('*')` na mesma linha); é texto de pilha, sem vazamento. Causa exata não isolada: o programa roda por script nomeado,
/// sem o transpilador no caminho.
const KNOWN: &[common::KnownCase] = &[common::KnownCase { label: "214", cause: "stack do DOMException de dois atob('*') na mesma linha: igual no bun, diferente no motor (coluna do chamador)" }];

#[test]
fn dom_exception_matches_bun() {
    common::check_with_meta_known_lines(GOLDEN, common::NO_PRELUDES, 150, &[], KNOWN, |source, _| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(source, "dom_exception_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
