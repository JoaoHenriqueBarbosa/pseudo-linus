//! Golden de recursos recentes do ECMAScript contra o JavaScriptCore do bun: `tests/golden/recent_features_bun.tsv` sai
//! de `scripts/gen-recent-features-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da
//! variável global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre `typeof`, `name`/`length`, descritores
//! e mensagens de erro exatas de Uint8Array base64/hex, Math.sumPrecise, Error.isError, Promise.try, Array.fromAsync,
//! Object.groupBy, Iterator.concat/zip e helpers, cópias de Array, isWellFormed, Atomics.waitAsync, RegExp.escape,
//! JSON.rawJSON, Float16, Intl.Locale, DisposableStack, AsyncDisposableStack, SuppressedError, `using`, Temporal.Instant
//! e ArrayBuffer transfer/resize/detached.
mod common;

const GOLDEN: &str = include_str!("golden/recent_features_bun.tsv");

const PRELUDES: &str = include_str!("golden/recent_features.preludes.json");

#[test]
fn recent_features_match_bun() {
    common::run_mapped_golden(GOLDEN, PRELUDES, 500, "recent_features_case.js");
}
