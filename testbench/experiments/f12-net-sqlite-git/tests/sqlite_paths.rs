//! O CLI, o VFS e o IO do turso são nossos: estes testes cobrem a nossa parte.
//! A conformidade de cada motor é resultado (vai pro JSON); aqui só entra o que é bug nosso.

use std::collections::BTreeSet;

use f12_net_sqlite_git::sqlite::cli::Backend;
use f12_net_sqlite_git::sqlite::experiments as sq;
use f12_net_sqlite_git::sqlite::rusq::{PluginBackend, SerializeBackend};
use f12_net_sqlite_git::sqlite::turso_engine::TursoBackend;
use harness::{Entry, MemTree};

/// Divergências conhecidas e explicadas no README pros dois caminhos com o SQLite de verdade.
const KNOWN: &[&str] = &[
    "sqlite-cli-mode-csv",
    "sqlite-cli-mode-json",
    "sqlite-cli-mode-column",
    "sqlite-pragma-journal-mode",
];

#[test]
fn real_sqlite_paths_only_fail_where_documented() {
    let cases = sq::load_cases().unwrap();
    for cand in sq::candidates().into_iter().filter(|c| c.label.starts_with("rusqlite")) {
        let run = sq::run_conformance(&cand, &cases);
        let failing: BTreeSet<String> = run.comparisons.iter().filter(|c| !c.strict).map(|c| c.id.clone()).collect();
        let unexpected: Vec<&String> = failing.iter().filter(|id| !KNOWN.contains(&id.as_str())).collect();
        assert!(unexpected.is_empty(), "{}: falhas fora do esperado: {unexpected:?}", cand.label);
    }
}

fn roundtrip(backend: &mut dyn Backend) -> Vec<String> {
    let mut fs = MemTree::new();
    fs.insert("db.sqlite", Entry::file(Vec::new(), 0o644));
    let mut s = backend.open(&mut fs, Some("db.sqlite"), false).unwrap();
    sq::query_lines(s.as_mut(), "CREATE TABLE t(a, b); INSERT INTO t VALUES (1, 'x'), (2.5, NULL);").unwrap();
    s.close(&mut fs, false).unwrap();
    assert!(fs.read("db.sqlite").unwrap().starts_with(b"SQLite format 3\0"), "o banco tem que voltar pro FS do caso");
    let mut s = backend.open(&mut fs, Some("db.sqlite"), false).unwrap();
    let rows = sq::query_lines(s.as_mut(), "SELECT a, b, typeof(b) FROM t ORDER BY a;").unwrap();
    s.close(&mut fs, false).unwrap();
    rows
}

#[test]
fn each_engine_persists_in_the_case_fs() {
    let want = vec!["1|x|text".to_string(), "2.5||null".to_string()];
    assert_eq!(roundtrip(&mut SerializeBackend), want);
    assert_eq!(roundtrip(&mut PluginBackend), want);
    assert_eq!(roundtrip(&mut TursoBackend { wall: None }), want);
}

#[test]
fn vfs_locks_prevent_lost_updates() {
    let conc = sq::concurrency();
    assert_eq!(conc["rusqlite-sqlite-plugin/counter"]["lost_updates"], 0);
    assert_eq!(conc["rusqlite-sqlite-plugin/visibility"]["concurrent_writer_without_wait"], "database is locked");
    assert_eq!(conc["rusqlite-serialize/counter/whole-file-lock"]["lost_updates"], 0);
}
