//! `Error` e `stack` do programa ORIGINAL contra o `bun run` do bun 1.4.2, com o transpilador do zbun instalado: o fonte original
//! passa pelo impressor e as posições saem do mapa dele, como no bun. O golden (`tests/golden/error_originals_bun.tsv`, de
//! `ORIGINALS=1 bun scripts/gen-error-golden.js`) guarda o programa e o valor da global `R`.
mod common;
mod zbun_transpiler;

const GOLDEN: &str = include_str!("golden/error_originals_bun.tsv");

/// Linhas (1-based) que ainda divergem do bun com o transpilador instalado, cada uma com a causa medida.
const KNOWN: &[common::KnownCase] = &[
    common::KnownCase { label: "689", cause: "o frame de `new X()` do principal transpilado sai na coluna do callee, 4 colunas (`new `) depois da do bun" },
    common::KnownCase { label: "698", cause: "o frame de `new X()` do principal transpilado sai na coluna do callee, 4 colunas (`new `) depois da do bun" },
    common::KnownCase { label: "941", cause: "o frame de `new X()` do principal transpilado sai na coluna do callee, 4 colunas (`new `) depois da do bun" },
    common::KnownCase { label: "950", cause: "o frame de `new X()` do principal transpilado sai na coluna do callee, 4 colunas (`new `) depois da do bun" },
    common::KnownCase { label: "1309", cause: "o frame de `new X()` do principal transpilado sai na coluna do callee, 4 colunas (`new `) depois da do bun" },
    common::KnownCase { label: "2620", cause: "o texto do TDZ `Cannot access 'X'` o JSC tira do fonte executado na posição da variável: no bun é `globalThis` (o preload do gerador fica noutro lugar do texto que o do harness, que cai em `process.on(...)`); a mensagem depende do texto do preload, não do motor" },
];

#[test]
fn error_of_original_programs_matches_bun() {
    common::run_with_stack(256 * 1024 * 1024, || {
        zjsc::runtime::vm::VM::set_thread_stack_budget(240 * 1024 * 1024);
        zbun_transpiler::install();
        common::originals::check_original_programs(GOLDEN, "error_case.js", 1500, KNOWN);
    });
}
