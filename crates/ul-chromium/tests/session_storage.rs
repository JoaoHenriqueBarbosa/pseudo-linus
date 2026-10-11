//! O Session Storage em disco contra o que o Chromium 154 do oráculo gravou: cada fixture é o `000003.log` de uma
//! execução de `testbench/corpus/headless-storage/ss-*.html` (medida por `testbench/oracle/measure-session-storage.sh`),
//! e o log do leveldb que o `SessionStorageStore` deixa ao fazer o mesmo tem de sair igual: lotes (inclusive os vazios
//! que o desligamento grava), ordem das operações, números de sequência, formato de chaves e valores. Só o GUID do
//! `namespace` muda de uma execução para outra, e os dois lados o trocam por um nome fixo.

use std::sync::Arc;

use ul_chromium::session_storage::{Exit, SessionStorageStore};
use ul_chromium::storage_state::{AreaStore, Change};
use ul_leveldb::batch::{Op, WriteBatch};
use ul_leveldb::env::{Env, MemEnv};
use ul_leveldb::log::read_records;

const DIR: &str = "/profile/Default/Session Storage";
const ORIGIN: &str = "http://127.0.0.1:8000";
const OTHER_ORIGIN: &str = "http://localhost:8000";
const START_US: u64 = 1_791_666_000_000_000;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/session_storage/{name}.log", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// A chave com o GUID do `namespace` trocado por `GUID`.
fn anonymous(key: &[u8]) -> String {
    match key.strip_prefix(b"namespace-") {
        Some(rest) => {
            let dash = rest.iter().position(|&byte| byte == b'-').unwrap();
            format!("namespace-GUID{}", rest[dash..].escape_ascii())
        }
        None => key.escape_ascii().to_string(),
    }
}

/// Cada lote do log como texto: a sequência e as operações.
fn describe(log: &[u8]) -> Vec<String> {
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
                    Op::Delete { key } => format!("delete {}", anonymous(&key)),
                    Op::Put { key, value } => format!("put {} => {}", anonymous(&key), value.escape_ascii()),
                })
                .collect();
            format!("seq={} {}", batch.sequence(), ops.join(" | "))
        })
        .collect()
}

/// Uma execução do navegador: abre o banco, deixa a página agir na aba e sai da forma pedida.
fn run(exit: Exit, page: impl FnOnce(&dyn AreaStore)) -> Vec<String> {
    let env = MemEnv::new();
    env.set_clock(START_US);
    let store = Arc::new(SessionStorageStore::open(Arc::new(env.clone()), DIR, 0x20).unwrap());
    page(&*store.namespace());
    store.finish(exit).unwrap();
    describe(&env.read_file(&format!("{DIR}/000003.log")).unwrap())
}

fn check(name: &str, exit: Exit, page: impl FnOnce(&dyn AreaStore)) {
    assert_eq!(run(exit, page), describe(&fixture(name)), "{name}");
}

fn write_page(tab: &dyn AreaStore) {
    tab.open_area(ORIGIN);
    tab.record(ORIGIN, Change::Set("a", "1"));
    tab.record(ORIGIN, Change::Set("b", "dois"));
    tab.record(ORIGIN, Change::Set("日", "ü"));
    tab.record(ORIGIN, Change::Set("gone", "x"));
    tab.record(ORIGIN, Change::Remove("gone"));
}

fn quick_page(tab: &dyn AreaStore) {
    tab.open_area(ORIGIN);
    tab.record(ORIGIN, Change::Set("a", "1"));
    tab.record(ORIGIN, Change::Set("日", "ü"));
}

/// `ss-two.html`: grava no `sessionStorage` de uma origem e navega para outra na mesma aba.
fn two_origins_page(tab: &dyn AreaStore) {
    tab.open_area(ORIGIN);
    tab.record(ORIGIN, Change::Set("top", "1"));
    tab.open_area(OTHER_ORIGIN);
    tab.record(OTHER_ORIGIN, Change::Set("frame", "2"));
}

#[test]
fn a_browser_that_stored_nothing_leaves_the_batches_of_its_exit() {
    check("signal-empty", Exit::Signal, |_| {});
    check("dump-empty", Exit::DumpDom, |_| {});
}

#[test]
fn closing_through_devtools_writes_the_changes_and_two_empty_batches() {
    check("close-write", Exit::Close, write_page);
    check("close-two", Exit::Close, two_origins_page);
}

#[test]
fn a_signal_writes_only_the_pending_changes() {
    check("signal-quick", Exit::Signal, quick_page);
}

#[test]
fn the_dump_removes_every_namespace_and_then_every_map() {
    check("dump-two", Exit::DumpDom, two_origins_page);
}

#[test]
fn a_key_changed_and_removed_is_a_delete_under_the_registered_namespace() {
    check("signal-empty-map", Exit::Signal, |tab| {
        tab.open_area(ORIGIN);
        tab.record(ORIGIN, Change::Set("k", "v"));
        tab.record(ORIGIN, Change::Remove("k"));
    });
}

/// `ss-edge.html`: o mesmo valor duas vezes não é mudança, e o `clear()` esquece o que foi alterado antes dele (o
/// banco nem tem o que apagar).
#[test]
fn clear_forgets_what_was_changed_before_it() {
    check("signal-edge", Exit::Signal, |tab| {
        tab.open_area(ORIGIN);
        tab.record(ORIGIN, Change::Set("k", "v"));
        tab.record(ORIGIN, Change::Remove("k"));
        tab.record(ORIGIN, Change::Set("same", "1"));
        tab.record(ORIGIN, Change::Set("same", "1"));
        tab.record(ORIGIN, Change::Set("x", "y"));
        tab.record(ORIGIN, Change::Set("z", "w"));
        tab.record(ORIGIN, Change::Clear);
        tab.record(ORIGIN, Change::Set("after", "clear"));
    });
}

/// `ss-read-nav.html`: ler já dá à origem o id de mapa 0, mesmo sem gravar nada; a origem seguinte fica com o 1.
#[test]
fn reading_an_area_takes_the_next_map_id() {
    check("signal-read-nav", Exit::Signal, |tab| {
        assert!(tab.open_area(ORIGIN).is_empty());
        tab.open_area(OTHER_ORIGIN);
        tab.record(OTHER_ORIGIN, Change::Set("frame", "2"));
    });
}

#[test]
fn a_change_goes_to_disk_five_seconds_after_it_was_made() {
    let env = MemEnv::new();
    env.set_clock(START_US);
    let store = Arc::new(SessionStorageStore::open(Arc::new(env.clone()), DIR, 0x20).unwrap());
    let tab = store.namespace();
    tab.open_area(ORIGIN);
    tab.record(ORIGIN, Change::Set("k", "v"));
    store.flush_due().unwrap();
    let log = || describe(&env.read_file(&format!("{DIR}/000003.log")).unwrap());
    assert_eq!(log().len(), 2, "o version e o lote vazio: a mudança ainda está dentro do atraso");
    env.set_clock(START_US + 6_000_000);
    store.flush_due().unwrap();
    assert_eq!(log().len(), 3);
    // O que já foi gravado não é gravado de novo na saída, e o namespace só entra uma vez.
    tab.record(ORIGIN, Change::Set("k", "w"));
    store.finish(Exit::Signal).unwrap();
    let batches = log();
    assert_eq!(batches.len(), 4);
    assert_eq!(batches[3], "seq=4 put map-0-k => w\\x00", "{}", batches[3]);
}

#[test]
fn a_tabs_area_reads_back_what_the_page_wrote() {
    let env = MemEnv::new();
    let store = Arc::new(SessionStorageStore::open(Arc::new(env), DIR, 0x20).unwrap());
    let tab = store.namespace();
    tab.open_area(ORIGIN);
    tab.record(ORIGIN, Change::Set("日", "ü"));
    assert_eq!(tab.open_area(ORIGIN), [("日".to_owned(), "ü".to_owned())]);
}
