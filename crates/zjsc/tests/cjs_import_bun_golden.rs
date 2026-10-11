//! Golden do `import` de arquivos CommonJS contra o bun 1.4.2: `tests/golden/cjs_import_bun.tsv` sai de
//! `scripts/gen-cjs-import-golden.js` (formato de `tests/common/module_golden.rs`: mapa de arquivos, saída
//! `{"log":[...],"error":...}`). Cobre o interop de `__esModule` (o `default` do namespace e a lista de nomeados), as chaves
//! dinâmicas de `module.exports`, o CommonJS que roda na carga (antes dos corpos ES), o cache dividido entre `import` e
//! `require`, vários CommonJS no mesmo programa e o erro lançado na carga. Os arquivos moram numa árvore em memória
//! (`/app`) e o transpilador do zbun está instalado: é ele que diz ao carregador qual arquivo é CommonJS.
mod common;
mod zbun_transpiler;

use std::rc::Rc;

use common::module_golden::parse_files;
use zjsc::api::module::evaluate_module_with_fs;
use zjsc::api::module_probe::MemoryFs;

const GOLDEN: &str = include_str!("golden/cjs_import_bun.tsv");

#[test]
fn commonjs_imports_match_bun() {
    zbun_transpiler::install();
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let mut columns = line.split('\t');
        let name = columns.next().expect("nome do caso");
        let files = parse_files(columns.next().expect("arquivos"));
        let expected = columns.next().expect("saída");
        total += 1;
        let paths: Vec<(String, &str)> = files.iter().map(|(file, text)| (format!("/app/{file}"), text.as_str())).collect();
        let refs: Vec<(&str, &str)> = paths.iter().map(|(path, text)| (path.as_str(), *text)).collect();
        let main = files.iter().find(|(file, _)| file == "main.mjs").map(|(_, text)| text.as_str()).expect("main.mjs");
        let outcome = evaluate_module_with_fs(Rc::new(MemoryFs::new(&refs)), main, "/app/main.mjs");
        let actual = match outcome.error {
            None => format!("{{\"log\":{},\"error\":null}}", outcome.log_json),
            Some(error) => format!("{{\"log\":{},\"error\":{error:?}}}", outcome.log_json),
        };
        if actual != expected {
            failures.push(format!("{name}\n    esperado {expected}\n    veio     {actual}"));
        }
    }
    assert!(total >= 13, "o golden tem só {total} casos");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
