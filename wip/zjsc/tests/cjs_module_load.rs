//! Golden do carregador de módulos do `require` do CommonJS contra o bun 1.4.2: `tests/golden/cjs_module_load_bun.tsv` sai
//! de `scripts/gen-cjs-module-load-golden.js`. Cada linha é `nome<TAB>JSON(main.js)<TAB>JSON(resultado)`. A árvore do
//! gerador está em `tree()` com raiz `/app` (o gerador tira o prefixo da pasta temporária das strings, e `ROOT` do
//! programa vale `/app` aqui), `main.js` roda como `/app/main.js` e grava a string `R`.
mod common;

use std::rc::Rc;

use common::{guarded, json_string};
use zjsc::api::eval::evaluate_cjs_program_with_fs;
use zjsc::api::module_probe::MemoryFs;

const GOLDEN: &str = include_str!("golden/cjs_module_load_bun.tsv");

fn tree() -> MemoryFs {
    MemoryFs::new(&[
        (
            "/app/a.js",
            "exports.n=1;exports.id=module.id;exports.filename=module.filename;exports.loaded=module.loaded;exports.path=module.path;\
             exports.parentId=module.parent&&module.parent.id;exports.keys=Object.keys(module);exports.children=module.children.length;\
             exports.paths0=module.paths[0];",
        ),
        ("/app/node_modules/pk/package.json", r#"{"main":"i.js"}"#),
        ("/app/node_modules/pk/i.js", "exports.x=2;exports.id=module.id;exports.parentId=module.parent.id;"),
        ("/app/c1.js", "exports.a=1;const o=require('./c2');exports.b=o.seen;"),
        ("/app/c2.js", "const p=require('./c1');exports.seen=JSON.stringify(p);"),
        ("/app/d.json", r#"{"k":[1,2]}"#),
        ("/app/thrower.js", "throw new Error('boom');"),
        ("/app/sub/s.js", "exports.t=require('./t').v;exports.parentFile=module.parent.id;exports.res=require.resolve('./t');"),
        ("/app/sub/t.js", "exports.v=7;"),
        ("/app/real/x.js", "exports.id=module.id;"),
    ])
    .with_link("/app/ln", "/app/real")
}

#[test]
fn cjs_module_load_matches_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let mut columns = line.splitn(3, '\t');
        let name = columns.next().expect("nome do caso");
        let source = json_string(columns.next().expect("fonte"));
        let expected = json_string(columns.next().expect("resultado"));
        total += 1;
        let units: Vec<u16> = source.encode_utf16().collect();
        let outcome = guarded(|| evaluate_cjs_program_with_fs(Rc::new(tree()), &units, "/app/main.js", "R", &[]));
        match outcome {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{name}\n    esperado {expected:?}\n    veio     {actual:?}")),
            Err(reason) => failures.push(format!("{name}\n    esperado {expected:?}\n    {reason}")),
        }
    }
    assert!(total >= 20, "golden com só {total} casos");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
