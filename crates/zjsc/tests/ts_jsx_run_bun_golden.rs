//! `bun run` de arquivos `.ts`, `.tsx` e `.jsx` (`tests/golden/ts_jsx_run_bun.tsv`, de
//! `scripts/gen-ts-jsx-run-golden.js`, rodado no bun 1.4.2): enum (inclusive `const` e mesclado), namespace, `import
//! type`, `satisfies`, parâmetros-propriedade, decorators do TC39, `using`, posições de `Error.stack` depois de apagar
//! os tipos e JSX clássico (pragma) e automático (`jsxDEV` com a fonte). O zjsc roda com o transpilador do zbun
//! instalado, como o bun roda o fonte original reimpresso.
mod common;
mod zbun_transpiler;

use common::module_golden::check_module_golden_with_known;
use zjsc::api::module::LogPoint;

/// `(trecho do mapa de arquivos, causa medida no oráculo)`.
const KNOWN: &[(&str, &str)] = &[];

#[test]
fn ts_and_jsx_run_matches_bun() {
    zbun_transpiler::install();
    check_module_golden_with_known(include_str!("golden/ts_jsx_run_bun.tsv"), 100, LogPoint::EntrySettled, KNOWN);
}
