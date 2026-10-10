//! O comportamento do banco ao longo de muitas escritas: as memtables cheias viram tabelas durante a execução, o nível 0
//! passa do limite e é compactado, e o que o usuário lê continua sendo a última versão de cada chave.

use std::sync::Arc;

use ul_leveldb::env::{Env, MemEnv};
use ul_leveldb::table::Table;
use ul_leveldb::{Db, Options};

const DIR: &str = "/db";

/// Texto de `length` caracteres que quase não comprime, determinado pela semente.
fn filler(seed: u32, length: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(12345);
    (0..length)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            b'a' + ((state >> 24) % 26) as u8
        })
        .collect()
}

fn open(env: &MemEnv) -> Db {
    env.set_clock(1_791_666_000_000_000);
    Db::open(Arc::new(env.clone()), DIR, Options::chromium(0x10)).unwrap()
}

fn tables(env: &MemEnv) -> Vec<String> {
    env.snapshot().keys().filter(|path| path.ends_with(".ldb")).cloned().collect()
}

fn log_text(env: &MemEnv) -> String {
    String::from_utf8(env.read_file(&format!("{DIR}/LOG")).unwrap()).unwrap()
}

#[test]
fn the_database_can_be_shared_between_threads_behind_a_mutex() {
    fn assert_send_and_sync<T: Send + Sync>() {}
    assert_send_and_sync::<std::sync::Mutex<Db>>();
}

#[test]
fn overwriting_the_same_keys_keeps_the_latest_value_through_flushes_and_compactions() {
    let env = MemEnv::new();
    let mut db = open(&env);
    for round in 0..40u32 {
        db.put(b"a", &filler(round * 2, 40_000)).unwrap();
        db.put(b"b", &filler(round * 2 + 1, 40_000)).unwrap();
        assert_eq!(db.get(b"a").unwrap().unwrap(), filler(round * 2, 40_000));
    }
    let want = vec![(b"a".to_vec(), filler(78, 40_000)), (b"b".to_vec(), filler(79, 40_000))];
    assert_eq!(db.entries().unwrap(), want);
    let log = log_text(&env);
    // O primeiro flush não sobrepõe nada e vai para o nível 2, o segundo para o 1, os seguintes para o 0; a quarta
    // tabela de nível 0 dispara a compactação, que leva junto a do nível 1 que a faixa sobrepõe.
    assert!(log.contains("Compacting 4@0 + 1@1 files"), "{log}");
    assert!(log.contains("compacted to: files["), "{log}");
    // Cada compactação descarta as versões encobertas: sobram poucas tabelas, não uma por flush.
    assert!(tables(&env).len() <= 6, "{:?}", tables(&env));
    drop(db);
    assert_eq!(open(&env).entries().unwrap(), want);
}

#[test]
fn a_deleted_key_leaves_no_trace_in_the_compacted_tables() {
    let env = MemEnv::new();
    let mut db = open(&env);
    // Três flushes de a e b (níveis 2, 1 e 0) para que o próximo, que também sobrepõe a faixa, entre no nível 0.
    for round in 0..3u32 {
        db.put(b"a", &filler(round, 40_000)).unwrap();
        db.put(b"b", &filler(round + 50, 40_000)).unwrap();
    }
    db.put(b"x", b"value").unwrap();
    db.delete(b"x").unwrap();
    for round in 0..12u32 {
        db.put(b"a", &filler(round + 100, 40_000)).unwrap();
        db.put(b"b", &filler(round + 200, 40_000)).unwrap();
    }
    assert_eq!(db.get(b"x").unwrap(), None);
    assert!(log_text(&env).contains("Compacted 4@0"), "{}", log_text(&env));
    // O put e o delete de `x` estavam nas tabelas de nível 0 compactadas e nada abaixo da saída tem `x` (as tabelas
    // dos níveis de baixo só cobrem a e b): os dois somem.
    for path in tables(&env) {
        let table = Table::open(env.read_file(&path).unwrap()).unwrap();
        for (key, _) in table.entries().unwrap() {
            assert_ne!(&key[..key.len() - 8], b"x", "{path}");
        }
    }
}

#[test]
fn the_file_numbers_and_the_manifest_survive_a_reopen_after_compactions() {
    let env = MemEnv::new();
    {
        let mut db = open(&env);
        for round in 0..24u32 {
            db.put(b"k", &filler(round, 50_000)).unwrap();
            db.put(b"l", &filler(round + 500, 50_000)).unwrap();
        }
    }
    let before = tables(&env);
    let mut db = open(&env);
    // Reabrir não reaproveita número de arquivo: o que for gravado agora tem número novo.
    db.put(b"k", &filler(9999, 50_000)).unwrap();
    db.put(b"l", &filler(9998, 50_000)).unwrap();
    db.put(b"k", &filler(9997, 50_000)).unwrap();
    let newest = |paths: &[String]| paths.iter().filter_map(|path| path.rsplit('/').next()?.strip_suffix(".ldb")?.parse::<u64>().ok()).max();
    assert!(newest(&tables(&env)) >= newest(&before));
    assert_eq!(db.get(b"k").unwrap().unwrap(), filler(9997, 50_000));
    assert_eq!(db.get(b"l").unwrap().unwrap(), filler(9998, 50_000));
}
