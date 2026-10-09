//! Golden do campo `exports` do `package.json` contra o bun 1.4.2: `tests/golden/package_exports_bun.tsv` sai de
//! `scripts/gen-package-exports-golden.js`. Cada linha é `JSON(exports)<TAB>JSON(subcaminho)<TAB>require.resolve
//! <TAB>import.meta.resolve`, com caminhos relativos à raiz da árvore e erros como `ERR <código>: <mensagem>`.
//! A árvore não é emitida pelo gerador: ela é a mesma para todo caso (a lista `FILES` do gerador, cada arquivo com
//! o texto `1`, mais o `package.json` com `{"main":"main.js","exports":<exports>}`), então o runner a monta por caso
//! em `node_modules/pkg`, o nome que o gerador normaliza no golden. Importadores: `/t.cjs` e `/t.mjs`.
//!
//! PONTO DE INTEGRAÇÃO: `src/api/package_exports.rs` ainda não existia quando este runner foi escrito. Ele supõe
//! `package_exports::resolve_package(fs, importer_dir, specifier, esm) -> Result<String, String>`, onde `esm`
//! escolhe as condições (`import` em vez de `require`), `Ok` é o caminho absoluto resolvido e `Err` é
//! `<código>: <mensagem>` (a primeira linha da mensagem). Ajustar só o `resolve` abaixo.
mod common;

use common::json_string;
use zjsc::api::module_probe::MemoryFs;
use zjsc::api::package_exports::resolve_package;

const GOLDEN: &str = include_str!("golden/package_exports_bun.tsv");

const FILES: &[&str] = &[
    "index.js", "main.js", "a.js", "b.js", "c.js", "d.js", "e.js", "i.mjs", "r.cjs", "bun.js", "node.js", "def.js",
    "types.js", "src/x.js", "src/y/z.js", "lib/x.js", "lib/y/z.js", "lib/x/g.js", "lib/y/z/g.js", "lib/x.js.js",
    "lib/q/index.js", "package.json.js",
];

fn tree(exports_json: &str) -> MemoryFs {
    let mut files: Vec<(String, String)> =
        FILES.iter().map(|file| (format!("/node_modules/pkg/{file}"), "1".to_string())).collect();
    files.push(("/node_modules/pkg/package.json".to_string(), format!("{{\"main\":\"main.js\",\"exports\":{exports_json}}}")));
    files.push(("/t.cjs".to_string(), String::new()));
    files.push(("/t.mjs".to_string(), String::new()));
    let refs: Vec<(&str, &str)> = files.iter().map(|(path, text)| (path.as_str(), text.as_str())).collect();
    MemoryFs::new(&refs)
}

fn resolve(fs: &MemoryFs, importer: &str, specifier: &str, esm: bool) -> String {
    let dir = &importer[..importer.rfind('/').expect("diretório do importador")];
    match resolve_package(fs, dir, specifier, esm) {
        Ok(path) => path.trim_start_matches('/').to_string(),
        Err(message) => format!("ERR {message}").replace('\n', "\\n"),
    }
}

#[test]
fn package_exports_matches_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let mut columns = line.splitn(4, '\t');
        let exports_json = json_string(columns.next().expect("exports"));
        let sub = json_string(columns.next().expect("subcaminho"));
        let expected_require = json_string(columns.next().expect("require.resolve"));
        let expected_import = json_string(columns.next().expect("import.meta.resolve"));
        let specifier = if sub == "." { "pkg".to_string() } else { format!("pkg{}", &sub[1..]) };
        let fs = tree(&exports_json);
        let actual_require = resolve(&fs, "/t.cjs", &specifier, false);
        let actual_import = resolve(&fs, "/t.mjs", &specifier, true);
        total += 1;
        if actual_require != expected_require || actual_import != expected_import {
            failures.push(format!(
                "exports {exports_json} {sub:?}\n    esperado {expected_require:?} | {expected_import:?}\n    veio     {actual_require:?} | {actual_import:?}"
            ));
        }
    }
    assert_eq!(total, 115, "o golden mudou de tamanho");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
