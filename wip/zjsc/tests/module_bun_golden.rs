//! Golden de módulos ES contra o JavaScriptCore real: `tests/golden/module_bun.tsv` sai de
//! `scripts/gen-module-golden.js`, rodado no bun (formato em `tests/common/module_golden.rs`): o array global `log`
//! (alimentado por `L(x)`) e o `Nome: mensagem` do erro que rejeitou o import, ou `null`. Os módulos rodam num loader
//! em memória (`zjsc::api::module::evaluate_module_map_until`), cada caso num realm novo, com o transpilador do zbun
//! instalado: o fonte ORIGINAL (o que o gerador entrega ao bun) chega ao motor reimpresso, como no `bun run`.
mod common;
mod zbun_transpiler;

use common::module_golden::check_module_golden_with_known;
use zjsc::api::module::LogPoint;

/// O gerador espera um `setTimeout(0)` depois do `await import(main)` antes de imprimir o `log`.
#[test]
fn module_graphs_match_bun() {
    zbun_transpiler::install();
    check_module_golden_with_known(include_str!("golden/module_bun.tsv"), 200, LogPoint::NextTimeout, KNOWN);
}

/// O bun roda como CommonJS o arquivo sem sintaxe ESM que o `import` puxa (o zbun já o classifica assim): `this` é o
/// `module.exports` (`typeof this` dá `object`) e o texto é sloppy (`with` vale). O carregador ES do porte ainda não
/// importa CommonJS, então o texto reimpresso (a função de wrapper) fica de fora e o arquivo roda como módulo.
const NOT_ESM: &str = "o bun roda como CommonJS o arquivo sem sintaxe ESM (medido: `this` é `module.exports`, `with` vale); o carregador ES do porte ainda não importa CommonJS";

const KNOWN: &[(&str, &str)] = &[
    ("L(typeof this); L(typeof globalThis.x); L(this === undefined);", NOT_ESM),
    ("\"a.mjs\":\"with (x) {}\"", NOT_ESM),
    ("\"a.mjs\":\"'use strict'; L(typeof this);\"", NOT_ESM),
    ("\"a.mjs\":\"L((() => typeof this)());\"", NOT_ESM),
    (
        "L(new Error('x').stack.split('\\\\n')[1].replace(",
        "artefato do harness: o gerador roda os arquivos num diretório temporário e o frame do módulo sai `at /diretório/a.mjs:1:7`, de onde o `replace` tira o diretório; o porte tem a chave relativa `a.mjs` e o frame sai `    at a.mjs:1:7`, sem barra para o `replace` cortar",
    ),
];
