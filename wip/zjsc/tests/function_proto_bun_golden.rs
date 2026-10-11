//! Golden de `Function.prototype` contra o JavaScriptCore do bun: `tests/golden/function_proto_bun.tsv` sai de
//! `scripts/gen-function-proto-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o
//! texto da variável global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre `toString` de todas as
//! categorias, `bind` (name, length, `new`, `instanceof`), `call`/`apply` com array-like, `Symbol.hasInstance`,
//! `name`/`length`, `caller`/`arguments`, `Reflect.construct` com `newTarget` diferente, `instanceof` e a recursão
//! profunda (`RangeError`). O arquivo se chama `function_proto_case.js` dos dois lados.
mod common;

const GOLDEN: &str = include_str!("golden/function_proto_bun.tsv");

const PRELUDES: &str = include_str!("golden/function_proto.preludes.json");

/// Recursão de 5000 níveis por `apply` de Proxy (`p(a[0] - 1)` dentro do trap): o bun sem JIT chega a 5600 níveis (`bun` com
/// `BUN_JSC_useJIT=0`, busca binária) e o zjsc a 2667: o custo por nível dessa recursão (trap `apply` chamando o próprio Proxy)
/// está acima do medido no bun; o `CALL_COST` do `performProxyCall` foi calibrado só com `p()` encadeado. Erro útil (RangeError
/// capturável) e sem vazamento: backlog de calibração da pilha no `PLAN.md`.
const CAUSE: &str = "profundidade da recursão por apply de Proxy: 2667 no zjsc contra 5600 no bun sem JIT";
const KNOWN: &[common::KnownCase] = &[
    common::KnownCase { label: "2885", cause: CAUSE },
    common::KnownCase { label: "2886", cause: CAUSE },
    common::KnownCase { label: "2887", cause: CAUSE },
];

#[test]
fn function_prototype_matches_bun() {
    common::run_mapped_golden_big_stack_known_lines(GOLDEN, PRELUDES, 1200, "function_proto_case.js", KNOWN);
}
