//! Leitura cruzada do IndexedDB com o Chromium 154 do oráculo (`testbench/oracle/measure-idb-cross.sh`): as páginas
//! `idb-basic`, `idb-types`, `idb-twobyte` e `idb-extra` rodam num perfil novo e `idb-dump` lê todos os bancos da
//! origem sem escrever. A leitura do oráculo sobre o próprio banco (`cross/oracle-dump.txt`) é a referência:
//!
//! - o plinus, depois das mesmas páginas, lê o mesmo texto;
//! - o plinus lê o mesmo texto no banco que o oráculo gravou (`cross/oracle-db`);
//! - o oráculo lê o mesmo texto no banco que o plinus gravou (`cross/ours-db`, medido com
//!   `SEED=tests/fixtures/idb/cross/ours-db`; a leitura dele fica em `cross/oracle-reads-ours.txt`), e o banco que o plinus
//!   grava hoje é byte a byte o que o oráculo leu.
//!
//! `IDB_CROSS_WRITE=1` regrava `cross/ours-db` com o banco de agora (depois, medir de novo a leitura do oráculo).

mod common;

use std::rc::Rc;
use std::sync::Arc;

use ul_chromium::idb::disk::IndexedDbStore;
use ul_chromium::session::{Profile, Tab};
use ul_leveldb::env::{Env, MemEnv};

const ORIGIN_DB: &str = "/IndexedDB/http_127.0.0.1_8000.indexeddb.leveldb";
const PAGES: [&str; 4] = ["idb-basic.html", "idb-types.html", "idb-twobyte.html", "idb-extra.html"];

fn path(relative: &str) -> String {
    format!("{}/{relative}", env!("CARGO_MANIFEST_DIR"))
}

fn corpus(page: &str) -> String {
    std::fs::read_to_string(path(&format!("../../testbench/corpus/headless-storage/{page}"))).unwrap()
}

fn fixture_text(name: &str) -> String {
    std::fs::read_to_string(path(&format!("tests/fixtures/idb/cross/{name}"))).unwrap()
}

fn profile_on(env: &MemEnv) -> Arc<Profile> {
    Arc::new(Profile::on_disk(None, Some(Arc::new(IndexedDbStore::new(Arc::new(env.clone()), "/IndexedDB", 1)))))
}

fn run_page(profile: &Arc<Profile>, page: &str) -> String {
    let body = corpus(page);
    let route = format!("/{page}");
    common::run_in_tab(&format!("http://127.0.0.1:8000{route}"), &[(route.as_str(), body.as_str())], Rc::new(Tab::new(profile.clone())))
}

/// O que `idb-dump` escreve no console: a última linha.
fn dump(profile: &Arc<Profile>) -> String {
    let log = run_page(profile, "idb-dump.html");
    format!("{}\n", log.lines().last().unwrap_or_default())
}

/// O banco da origem como (nome do arquivo, bytes), só o que o Chromium lê: CURRENT, MANIFEST, logs e tabelas.
fn origin_files(env: &MemEnv) -> Vec<(String, Vec<u8>)> {
    let prefix = format!("{ORIGIN_DB}/");
    env.snapshot()
        .into_iter()
        .filter_map(|(file, bytes)| {
            let name = file.strip_prefix(&prefix)?.to_owned();
            (name == "CURRENT" || name.starts_with("MANIFEST-") || name.ends_with(".log") || name.ends_with(".ldb")).then_some((name, bytes))
        })
        .collect()
}

/// As páginas num perfil novo, cada uma numa execução do navegador (o oráculo roda um processo por página, e cada
/// abertura de um banco que já existia grava o lote vazio do Chromium).
fn pages_on_fresh_profile() -> MemEnv {
    let env = MemEnv::new();
    for page in PAGES {
        run_page(&profile_on(&env), page);
    }
    env
}

#[test]
fn after_the_same_pages_plinus_reads_what_the_oracle_reads() {
    let env = pages_on_fresh_profile();
    assert_eq!(dump(&profile_on(&env)), fixture_text("oracle-dump.txt"));
}

/// O log da origem depois das páginas e da leitura, byte a byte igual ao do oráculo. Diverge só nos dois registros de
/// `Error` de `idb-types`: o `stack` serializado sai `at http://…/idb-types.html:28:13` e o do V8 é
/// `at request.onupgradeneeded (http://…/idb-types.html:31:13)` (o nome que o V8 infere da atribuição e a linha do
/// script inline contada desde o começo do documento), que são do formato de pilha do zjsc, não do IndexedDB.
#[test]
#[ignore = "Error.stack no formato do V8: nome inferido da atribuição e linha do script inline no documento"]
fn the_log_is_the_oracle_log_byte_for_byte() {
    let env = pages_on_fresh_profile();
    dump(&profile_on(&env));
    let ours = env.read_file(&format!("{ORIGIN_DB}/000003.log")).unwrap();
    assert!(ours == std::fs::read(path("tests/fixtures/idb/cross/oracle-db/000003.log")).unwrap(), "o log difere do oráculo");
}

#[test]
fn plinus_reads_the_database_the_oracle_wrote() {
    let env = MemEnv::new();
    env.create_dir_all(ORIGIN_DB).unwrap();
    for name in ["CURRENT", "MANIFEST-000001", "000003.log"] {
        env.write_file(&format!("{ORIGIN_DB}/{name}"), &std::fs::read(path(&format!("tests/fixtures/idb/cross/oracle-db/{name}"))).unwrap()).unwrap();
    }
    assert_eq!(dump(&profile_on(&env)), fixture_text("oracle-dump.txt"));
}

#[test]
fn the_oracle_reads_the_database_plinus_writes() {
    let env = pages_on_fresh_profile();
    let ours = origin_files(&env);
    let dir = path("tests/fixtures/idb/cross/ours-db");
    if std::env::var_os("IDB_CROSS_WRITE").is_some() {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in &ours {
            std::fs::write(format!("{dir}/{name}"), bytes).unwrap();
        }
    }
    let mut saved: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
    saved.sort();
    let names: Vec<String> = ours.iter().map(|(name, _)| name.clone()).collect();
    assert_eq!(names, saved, "os arquivos do banco que o oráculo leu");
    for (name, bytes) in &ours {
        assert!(&std::fs::read(format!("{dir}/{name}")).unwrap() == bytes, "{name} mudou desde a leitura do oráculo: medir de novo");
    }
    assert_eq!(fixture_text("oracle-reads-ours.txt"), fixture_text("oracle-dump.txt"));
}
