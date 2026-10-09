//! Golden do `fetch` de rede e do quirk de `file:` contra o JavaScriptCore do bun: `tests/golden/fetch_network_bun.tsv` sai
//! de `scripts/gen-fetch-network-golden.js` (bun 1.4.2, medido com `unshare -rn`, numa máquina sem rede). Cada linha é
//! `JSON(programa)<TAB>JSON(valor de R)`. O programa roda como `/app/main.js` sobre um `ModuleFs` em que só a raiz existe
//! (um diretório), o que basta: o quirk abre a raiz (`EISDIR`) e os demais caminhos do golden não existem (`ENOENT`).
mod common;

use std::rc::Rc;

use common::{guarded, json_string};
use zjsc::api::eval::evaluate_cjs_program_with_fs;
use zjsc::api::module_probe::{EntryKind, ModuleFs};

const GOLDEN: &str = include_str!("golden/fetch_network_bun.tsv");

/// Um sistema de arquivos em que só `/` existe, e é diretório.
#[derive(Debug)]
struct RootOnlyFs;

impl ModuleFs for RootOnlyFs {
    fn read_file(&self, _path: &str) -> Option<String> {
        None
    }

    fn stat(&self, path: &str) -> Option<EntryKind> {
        (path == "/").then_some(EntryKind::Directory)
    }

    fn realpath(&self, path: &str) -> Option<String> {
        self.stat(path).map(|_| path.to_owned())
    }
}

#[test]
fn fetch_network_and_file_quirk_match_bun() {
    let mut failures = Vec::new();
    let mut total = 0;
    for (row, line) in GOLDEN.lines().filter(|line| !line.is_empty()).enumerate() {
        let mut columns = line.splitn(2, '\t');
        let source = json_string(columns.next().expect("fonte"));
        let expected = json_string(columns.next().expect("resultado"));
        total += 1;
        let units: Vec<u16> = source.encode_utf16().collect();
        match guarded(|| evaluate_cjs_program_with_fs(Rc::new(RootOnlyFs), &units, "/app/main.js", "R", &[])) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("linha {row}\n    esperado {expected:?}\n    veio     {actual:?}")),
            Err(reason) => failures.push(format!("linha {row}\n    esperado {expected:?}\n    {reason}")),
        }
    }
    assert!(total >= 60, "só {total} casos");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
