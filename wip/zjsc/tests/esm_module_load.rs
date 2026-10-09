//! Golden do carregamento de `import` estático e `import()` dinâmico sobre o `ModuleFs` contra o bun 1.4.2:
//! `tests/golden/esm_module_load_bun.tsv` sai de `scripts/gen-esm-module-load-golden.js`. Cada linha é
//! `nome<TAB>JSON(main.mjs)<TAB>JSON(log)`. A árvore do gerador está em `tree()` com raiz `/app`
//! (o gerador tira o prefixo da pasta temporária das strings), e `main.mjs` roda como `/app/main.mjs`. Os arquivos
//! com `file://` escrevem `/app` onde o gerador usa `__ROOT__`.
mod common;

use std::rc::Rc;

use common::json_string;
use zjsc::api::module::evaluate_module_with_fs;
use zjsc::api::module_probe::MemoryFs;

const GOLDEN: &str = include_str!("golden/esm_module_load_bun.tsv");

fn tree() -> MemoryFs {
    MemoryFs::new(&[
        ("/app/a.mjs", "export const n = 1; export default 'A';"),
        ("/app/b.js", "export const b = 2;"),
        ("/app/dir/index.mjs", "export const d = 3;"),
        ("/app/bad_static.mjs", "import './nope';"),
        ("/app/bad_static_bare.mjs", "import 'nopkg';"),
        ("/app/sub/s.mjs", "import { t } from './t'; export const s = t + 1; export const url = import.meta.url;"),
        ("/app/sub/t.ts", "export const t = 10;"),
        ("/app/sub/bare.mjs", "import { x } from 'pk'; export { x };"),
        ("/app/node_modules/pk/package.json", r#"{"main":"i.js"}"#),
        ("/app/node_modules/pk/i.js", "export const x = 'pk-main';"),
        ("/app/node_modules/pk/lib/l.js", "export const l = 'pk-lib';"),
        ("/app/node_modules/@s/p/index.js", "export const sp = 'scoped';"),
        ("/app/real/x.mjs", "export const url = import.meta.url;"),
        ("/app/sp ace/x y.mjs", "export const url = import.meta.url;"),
        ("/app/c.cjs", "module.exports = { n: 1 };"),
        (
            "/app/file_static.mjs",
            "import { n } from 'file:///app/a.mjs?x=1'; import { url } from 'file://localhost/app/sp%20ace/x%20y.mjs#h'; export const r = [n, url];",
        ),
        ("/app/bad_file_static.mjs", "import 'file:///app/nope%20x.mjs?q#h';"),
        (
            "/app/req_file.cjs",
            "const D = __dirname; const t = (s) => { try { return JSON.stringify(require(s)); } catch (e) { return e.message.split(D).join(''); } };\n\
             module.exports = [t('file://' + D + '/c.cjs'), t('file://' + D + '/c.cjs?x=1#h'), t('file://localhost' + D + '/c.cjs'), t('file://' + D + '/nope%20c.cjs'), t('file:./c.cjs')];",
        ),
    ])
    .with_link("/app/ln", "/app/real")
}

#[test]
fn esm_module_load_matches_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let mut columns = line.splitn(3, '\t');
        let name = columns.next().expect("nome do caso");
        let source = json_string(columns.next().expect("fonte"));
        let expected = columns.next().expect("log").to_owned();
        total += 1;
        let outcome = evaluate_module_with_fs(Rc::new(tree()), &source, "/app/main.mjs");
        match outcome.error {
            None if outcome.log_json == expected => {}
            None => failures.push(format!("{name}\n    esperado {expected}\n    veio     {}", outcome.log_json)),
            Some(error) => failures.push(format!("{name}\n    esperado {expected}\n    rejeitou {error}")),
        }
    }
    assert!(total >= 20, "golden com só {total} casos");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
