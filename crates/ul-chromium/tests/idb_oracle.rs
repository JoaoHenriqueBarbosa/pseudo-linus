//! O leveldb do IndexedDB que o Chromium do oráculo escreveu (`tests/fixtures/idb/oracle-db`, o perfil depois de
//! `idb-basic`, `idb-twobyte`, `idb-later`, `idb-upgrade`, `idb-drop` e `idb-reopen`, ver `testbench/oracle/measure-idb.sh`)
//! abre com o `idb_cmp1` do `ul-chromium`: o nome do comparador do MANIFEST confere, a tabela já vem na ordem dele e as
//! leituras atravessam a tabela e o log.

use std::cmp::Ordering;
use std::sync::Arc;

use ul_chromium::idb::coding::{database, global, units};
use ul_chromium::idb::comparator::IdbComparator;
use ul_leveldb::comparator::Comparator;
use ul_leveldb::env::{Env, MemEnv};
use ul_leveldb::{Db, Options};

const FILES: [(&str, &[u8]); 4] = [
    ("000004.log", include_bytes!("fixtures/idb/oracle-db/000004.log")),
    ("000005.ldb", include_bytes!("fixtures/idb/oracle-db/000005.ldb")),
    ("CURRENT", include_bytes!("fixtures/idb/oracle-db/CURRENT")),
    ("MANIFEST-000001", include_bytes!("fixtures/idb/oracle-db/MANIFEST-000001")),
];

fn open_oracle_db() -> Db {
    let env = MemEnv::new();
    env.create_dir_all("/db").expect("diretório");
    for (name, bytes) in FILES {
        env.write_file(&format!("/db/{name}"), bytes).expect("arquivo");
    }
    let mut options = Options::chromium(1);
    options.comparator = Arc::new(IdbComparator);
    Db::open(Arc::new(env), "/db", options).expect("o comparador idb_cmp1 do MANIFEST confere")
}

#[test]
fn the_oracle_table_is_sorted_by_the_idb_comparator_and_reads_see_table_and_log() {
    let db = open_oracle_db();
    let entries = db.entries().expect("entradas");
    assert!(entries.len() >= 10, "o banco do oráculo tem os metadados e os registros: {}", entries.len());
    for pair in entries.windows(2) {
        assert_eq!(IdbComparator.compare(&pair[0].0, &pair[1].0), Ordering::Less, "{:02x?} antes de {:02x?}", pair[0].0, pair[1].0);
    }
    // db1 chegou à versão 3 (idb-drop) e o banco db1 existe com o id 1; o db2 foi apagado por idb-drop.
    assert_eq!(db.get(&database::int_version(1)).expect("leitura"), Some(vec![3]));
    let name = |text: &str| global::database_name(&units("http_127.0.0.1_8000@1"), &units(text));
    assert_eq!(db.get(&name("db1")).expect("leitura"), Some(vec![1]));
    assert_eq!(db.get(&name("db2")).expect("leitura"), None);
}
