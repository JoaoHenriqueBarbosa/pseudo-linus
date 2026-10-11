//! Golden de borda da família Error contra o JavaScriptCore do bun: `tests/golden/error_edge_bun.tsv` sai de
//! `scripts/gen-error-edge-golden.js`, rodado no bun 1.4.2 (um bun filho novo por programa). Cada linha é um
//! programa (JSON) e o texto da variável global `R` que ele grava. Cobre o que os goldens de erro, pilha e dispose
//! ainda não cobriam: AggregateError (errors iterável, ordem de acesso, cause), `cause` em todos os construtores
//! nativos, SuppressedError e as pilhas descartáveis, `Error.captureStackTrace` com constructorOpt,
//! `Error.stackTraceLimit`, `Error.prepareStackTrace` com cada método de CallSite, e as mensagens de
//! TypeError/RangeError/SyntaxError de APIs comuns com o sufixo `(evaluating ...)`. O nome do arquivo do script é
//! `x.js`; os programas normalizam linha e coluna sozinhos quando imprimem texto de pilha.
mod common;

use zjsc::api::eval::evaluate_run_in_this_context_with_caller;

/// O corredor de `scripts/gen-error-edge-golden.js`: o programa roda por `vm.runInThisContext` chamado de um arquivo CJS, então
/// a pilha de cada programa termina em dois frames do corredor (`runInThisContext` nativo e o arquivo chamador).
const CALLER: &str = "try { require(\"node:vm\").runInThisContext(require(\"node:fs\").readFileSync(\"case_source.js\", \"utf8\"){options}) } catch (e) {}\n";

const GOLDEN: &str = include_str!("golden/error_edge_bun.tsv");
const PRELUDES: &str = include_str!("golden/error_edge.preludes.json");

/// O golden foi gerado numa máquina com fuso a oeste de UTC (o `new Date(0)` caiu em 31/12/1969), e o teste roda em UTC: `JSON.parse(new Date(0))` cita o primeiro
/// identificador do texto da data (`Wed Dec 31 1969` contra `Thu Jan 01 1970`). Não é divergência do motor.
const KNOWN: &[common::KnownDivergence] = &[common::KnownDivergence {
    cause: "golden gerado com fuso a oeste de UTC, teste em UTC (texto de new Date(0))",
    expected: "SyntaxError: JSON Parse error: Unexpected identifier \"Wed\"",
    got: "SyntaxError: JSON Parse error: Unexpected identifier \"Thu\"",
    count: 1,
}];

#[test]
fn error_edge_matches_bun() {
    // Um programa que não grava `R` (ou falha na compilação) deixa `R` indefinido, e o golden registra isso.
    common::run_factored_big_stack_known(GOLDEN, PRELUDES, 1000, KNOWN, |source| evaluate_run_in_this_context_with_caller(source, Some("x.js"), "R", CALLER));
}
