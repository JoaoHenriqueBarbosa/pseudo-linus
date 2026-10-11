//! O IndexedDB em disco contra o Chromium 154 do oráculo: as páginas `testbench/corpus/headless-storage/idb-*.html`
//! rodam no prelúdio sobre um perfil em memória, e o `.log` do leveldb da origem tem de sair igual ao que o oráculo
//! gravou (`tests/fixtures/idb/logs`, medidos por `testbench/oracle/measure-idb.sh`): os lotes, os números de
//! sequência, a ordem das operações e cada chave e valor, inclusive os do `LevelDBScopes`.

mod common;

use std::rc::Rc;
use std::sync::Arc;

use ul_chromium::idb::disk::IndexedDbStore;
use ul_chromium::session::{Profile, Tab};
use ul_leveldb::batch::{Op, WriteBatch};
use ul_leveldb::env::{Env, MemEnv};
use ul_leveldb::log::read_records;

const DIR: &str = "/IndexedDB/http_127.0.0.1_8000.indexeddb.leveldb";

fn corpus(page: &str) -> String {
    std::fs::read_to_string(format!("{}/../../testbench/corpus/headless-storage/{page}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/idb/logs/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// Cada lote do log como texto, com chaves e valores em hexadecimal.
fn describe(log: &[u8]) -> Vec<String> {
    let hex = |bytes: &[u8]| bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    read_records(log)
        .unwrap()
        .iter()
        .map(|record| {
            let batch = WriteBatch::from_contents(record).unwrap();
            let ops: Vec<String> = batch
                .ops()
                .unwrap()
                .into_iter()
                .map(|op| match op {
                    Op::Delete { key } => format!("del {}", hex(&key)),
                    Op::Put { key, value } => format!("put {} {}", hex(&key), hex(&value)),
                })
                .collect();
            format!("seq={}\n  {}", batch.sequence(), ops.join("\n  "))
        })
        .collect()
}

/// Roda as páginas, em ordem, num perfil só; devolve o ambiente com os arquivos do banco.
fn run_pages(pages: &[&str]) -> MemEnv {
    let env = MemEnv::new();
    let store = Arc::new(IndexedDbStore::new(Arc::new(env.clone()), "/IndexedDB", 1));
    let profile = Arc::new(Profile::on_disk(None, Some(store)));
    for page in pages {
        let body = corpus(page);
        let path = format!("/{page}");
        common::run_in_tab(&format!("http://127.0.0.1:8000{path}"), &[(path.as_str(), body.as_str())], Rc::new(Tab::new(profile.clone())));
    }
    env
}

fn assert_same_log(env: &MemEnv, file: &str, fixture_name: &str) {
    let ours = describe(&env.read_file(&format!("{DIR}/{file}")).unwrap());
    let oracle = describe(&fixture(fixture_name));
    for (index, (a, b)) in ours.iter().zip(&oracle).enumerate() {
        assert_eq!(a, b, "lote {index} de {file}");
    }
    assert_eq!(ours.len(), oracle.len(), "número de lotes de {file}");
}

#[test]
fn idb_basic_writes_the_batches_of_the_oracle() {
    let env = run_pages(&["idb-basic.html"]);
    assert_same_log(&env, "000003.log", "basic.log");
}

#[test]
fn renames_clear_and_store_removal_write_the_batches_of_the_oracle() {
    let env = run_pages(&["idb-extra.html"]);
    assert_same_log(&env, "000003.log", "extra.log");
}

#[test]
fn deleting_a_database_compacts_its_range_like_the_oracle() {
    // Até o `deleteDatabase`, o log é o mesmo do oráculo (o nosso e o dele somem na compactação, então a página
    // sem a última etapa confere esse trecho).
    let page = corpus("idb-lifecycle.html");
    let cut = page.replace("await new Promise((resolve) => { const request = indexedDB.deleteDatabase('db3'); request.onsuccess = resolve; });", "");
    assert_ne!(cut, page);
    let env = MemEnv::new();
    let store = Arc::new(IndexedDbStore::new(Arc::new(env.clone()), "/IndexedDB", 1));
    common::run_in_tab("http://127.0.0.1:8000/idb-lifecycle.html", &[("/idb-lifecycle.html", cut.as_str())], Rc::new(Tab::new(Arc::new(Profile::on_disk(None, Some(store))))));
    let ours = describe(&env.read_file(&format!("{DIR}/000003.log")).unwrap());
    let oracle = describe(&fixture("lifecycle-3.log"));
    for (index, (a, b)) in ours.iter().zip(&oracle).enumerate() {
        assert_eq!(a, b, "lote {index} antes do deleteDatabase");
    }
    assert_eq!(ours.len(), 17, "os lotes até a remoção do repositório");

    let env = run_pages(&["idb-lifecycle.html"]);
    assert_same_log(&env, "000004.log", "lifecycle-4.log");
    let log = String::from_utf8(env.read_file(&format!("{DIR}/LOG")).unwrap()).unwrap();
    assert!(
        log.contains(r"Manual compaction at level-0 from '\x00\x01\x00\x00\x00' @ 72057594037927935 : 1 .. '\x00\x02\x00\x00\x00' @ 0 : 0; will stop at (end)"),
        "{log}"
    );
}

/// O que uma execução grava, a seguinte lê: a página de leitura vê os registros e os índices refeitos.
#[test]
fn a_later_run_reads_what_an_earlier_one_wrote() {
    let env = MemEnv::new();
    let store = Arc::new(IndexedDbStore::new(Arc::new(env.clone()), "/IndexedDB", 1));
    let profile = Arc::new(Profile::on_disk(None, Some(store)));
    let basic = corpus("idb-basic.html");
    common::run_in_tab("http://127.0.0.1:8000/idb-basic.html", &[("/idb-basic.html", basic.as_str())], Rc::new(Tab::new(profile)));
    let reader = r#"<script>
const r = indexedDB.open('db1');
r.onsuccess = () => {
  const tx = r.result.transaction('items');
  const all = tx.objectStore('items').getAll();
  const byName = tx.objectStore('items').index('by_name').getAllKeys();
  tx.oncomplete = () => console.log(JSON.stringify([r.result.version, all.result.map((v) => [v.id, v.name, v.when instanceof Date, v.bytes instanceof Uint8Array && [...v.bytes]]), byName.result]));
};
</script>"#;
    let reopened = Arc::new(Profile::on_disk(None, Some(Arc::new(IndexedDbStore::new(Arc::new(env.clone()), "/IndexedDB", 1)))));
    let log = common::run_in_tab("http://127.0.0.1:8000/r", &[("/r", reader)], Rc::new(Tab::new(reopened)));
    assert_eq!(log, "[1,[[1,\"a\",true,false],[2,\"b\",false,[1,2,3]]],[1,2]]\n");
}
