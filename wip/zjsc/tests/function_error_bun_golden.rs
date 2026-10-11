//! Golden de Function e Error contra o JavaScriptCore do bun: `tests/golden/function_error_bun.tsv` sai de
//! `scripts/gen-function-error-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre
//! `Function.prototype.toString`, `name`/`length`, `Error` (cause, AggregateError, captureStackTrace,
//! stackTraceLimit, prepareStackTrace, `Error.prototype.toString`) e o formato de `err.stack`. O arquivo se chama
//! `function_error_case.js` dos dois lados, então nome, linha e coluna de cada frame têm de ser idênticos.
mod common;

const GOLDEN: &str = include_str!("golden/function_error_bun.tsv");

const PRELUDES: &str = include_str!("golden/function_error.preludes.json");

/// Linhas do golden (1-based) que divergem do bun só pelo transpilador dele (reimpressão do arquivo: posição de frame, indentação do
/// `toString` de função de arquivo, programa recusado no parse como `'str'.length = 1`), medido no bun 1.4.2. Fecham com o printer
/// do zbun (backlog no `PLAN.md`).
const CAUSE: &str = "reimpressão do transpilador: posição de frame, indentação do toString, ou programa recusado no parse do arquivo";
const KNOWN: &[common::KnownCase] = &[
    common::KnownCase { label: "17", cause: CAUSE },
    common::KnownCase { label: "18", cause: CAUSE },
    common::KnownCase { label: "49", cause: CAUSE },
    common::KnownCase { label: "50", cause: CAUSE },
    common::KnownCase { label: "707", cause: CAUSE },
    common::KnownCase { label: "755", cause: CAUSE },
    common::KnownCase { label: "757", cause: CAUSE },
    common::KnownCase { label: "758", cause: CAUSE },
    common::KnownCase { label: "772", cause: CAUSE },
    common::KnownCase { label: "773", cause: CAUSE },
    common::KnownCase { label: "779", cause: CAUSE },
    common::KnownCase { label: "780", cause: CAUSE },
    common::KnownCase { label: "781", cause: CAUSE },
    common::KnownCase { label: "782", cause: CAUSE },
    common::KnownCase { label: "783", cause: CAUSE },
    common::KnownCase { label: "784", cause: CAUSE },
    common::KnownCase { label: "785", cause: CAUSE },
    common::KnownCase { label: "786", cause: CAUSE },
    common::KnownCase { label: "797", cause: CAUSE },
    common::KnownCase { label: "804", cause: CAUSE },
    common::KnownCase { label: "805", cause: CAUSE },
    common::KnownCase { label: "820", cause: CAUSE },
    common::KnownCase { label: "885", cause: CAUSE },
    common::KnownCase { label: "887", cause: CAUSE },
    common::KnownCase { label: "899", cause: CAUSE },
    common::KnownCase { label: "900", cause: CAUSE },
    common::KnownCase { label: "901", cause: CAUSE },
    common::KnownCase { label: "902", cause: CAUSE },
    common::KnownCase { label: "908", cause: CAUSE },
];

#[test]
fn function_and_error_match_bun() {
    common::run_mapped_golden_big_stack_known_lines(GOLDEN, PRELUDES, 400, "function_error_case.js", KNOWN);
}
