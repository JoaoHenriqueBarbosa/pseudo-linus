//! `Function` e `Error` do programa ORIGINAL contra o `bun run` do bun 1.4.2, com o transpilador do zbun instalado: o fonte original
//! passa pelo impressor (o `toString` de função de arquivo e as posições saem como no bun). O golden
//! (`tests/golden/function_error_originals_bun.tsv`, de `ORIGINALS=1 bun scripts/gen-function-error-golden.js`) guarda o programa e
//! o valor da global `R`.
mod common;
mod zbun_transpiler;

const GOLDEN: &str = include_str!("golden/function_error_originals_bun.tsv");

/// Linhas (1-based) que ainda divergem do bun com o transpilador instalado, cada uma com a causa medida.
const KNOWN: &[common::KnownCase] = &[];

#[test]
fn function_error_of_original_programs_matches_bun() {
    common::run_with_stack(256 * 1024 * 1024, || {
        zjsc::runtime::vm::VM::set_thread_stack_budget(240 * 1024 * 1024);
        zbun_transpiler::install();
        common::originals::check_original_programs(GOLDEN, "function_error_case.js", 400, KNOWN);
    });
}
