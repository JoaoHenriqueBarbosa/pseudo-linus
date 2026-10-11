//! O Local Storage em disco contra o que o Chromium 154 do oráculo gravou: a mesma sequência de nove execuções de
//! `testbench/corpus/headless-storage/ls-*.html` sobre um `--user-data-dir` só, e o log do leveldb de cada uma tem de
//! sair igual ao dele (lotes, ordem das operações, números de sequência, formato das chaves e valores). Só os
//! carimbos de tempo dos metadados diferem (cada execução tem o seu), então os dois lados são comparados com o tempo
//! trocado pelo que importa: o tamanho que o `META` registra.

use std::sync::Arc;

use ul_chromium::local_storage::LocalStorageStore;
use ul_chromium::storage_state::{AreaStore, Change};
use ul_leveldb::batch::{Op, WriteBatch};
use ul_leveldb::env::{Env, MemEnv};
use ul_leveldb::log::read_records;

const DIR: &str = "/profile/Default/Local Storage/leveldb";
const ORIGIN: &str = "http://127.0.0.1:8000";
const OTHER_ORIGIN: &str = "http://localhost:8000";

fn fixture(run: u32) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/local_storage/run{run}.log", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn varint(bytes: &[u8]) -> (u64, &[u8]) {
    let (mut value, mut shift, mut used) = (0u64, 0, 0);
    for &byte in bytes {
        value |= u64::from(byte & 0x7f) << shift;
        shift += 7;
        used += 1;
        if byte & 0x80 == 0 {
            break;
        }
    }
    (value, &bytes[used..])
}

/// Cada lote do log como texto: a sequência e as operações, com os metadados reduzidos ao que não depende do relógio.
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
                    Op::Delete { key } => format!("delete {}", key.escape_ascii()),
                    Op::Put { key, value: _ } if key.starts_with(b"METAACCESS:") => format!("put {} access", key.escape_ascii()),
                    Op::Put { key, value } if key.starts_with(b"META:") => {
                        assert_eq!(value[0], 0x08, "o campo 1 do META é o last_modified");
                        let (_, rest) = varint(&value[1..]);
                        assert_eq!(rest[0], 0x10, "o campo 2 do META é o size_bytes");
                        format!("put {} modified size={}", key.escape_ascii(), varint(&rest[1..]).0)
                    }
                    Op::Put { key, value } => format!("put {} => {}", key.escape_ascii(), value.escape_ascii()),
                })
                .collect();
            format!("seq={} {}", batch.sequence(), ops.join(" | "))
        })
        .collect()
}

/// Uma execução do navegador: abre o banco do perfil, deixa a página agir e fecha com o commit.
fn run(env: &MemEnv, page: impl FnOnce(&LocalStorageStore)) {
    env.set_clock(1_791_666_000_000_000);
    let store = LocalStorageStore::open(Arc::new(env.clone()), DIR, 0x20).unwrap();
    page(&store);
    store.flush().unwrap();
}

fn log_of(env: &MemEnv) -> Vec<u8> {
    env.read_file(&format!("{DIR}/000003.log")).unwrap()
}

fn check(env: &MemEnv, run_number: u32) {
    assert_eq!(describe(&log_of(env)), describe(&fixture(run_number)), "log depois da execução {run_number}");
}

fn write_three(store: &LocalStorageStore, origin: &str) {
    store.open_area(origin);
    store.record(origin, Change::Set("k1", "um"));
    store.record(origin, Change::Set("k2", "dois"));
    store.record(origin, Change::Set("k3", "tres"));
}

#[test]
fn a_change_goes_to_disk_five_seconds_after_it_was_made() {
    let env = MemEnv::new();
    env.set_clock(1_791_666_000_000_000);
    let store = LocalStorageStore::open(Arc::new(env.clone()), DIR, 0x20).unwrap();
    store.open_area(ORIGIN);
    store.record(ORIGIN, Change::Set("k", "v"));
    store.flush_due().unwrap();
    assert_eq!(describe(&log_of(&env)).len(), 1, "só o VERSION: a mudança ainda está dentro do atraso");
    env.set_clock(1_791_666_000_000_000 + 6_000_000);
    store.flush_due().unwrap();
    let batches = describe(&log_of(&env));
    assert_eq!(batches.len(), 2);
    assert!(batches[1].contains("put _http://127.0.0.1:8000\\x00\\x01k => \\x01v"), "{}", batches[1]);
    // O que já foi gravado não é gravado de novo na saída.
    store.flush().unwrap();
    assert_eq!(describe(&log_of(&env)).len(), 2);
}

#[test]
fn the_nine_runs_of_the_oracle_leave_the_same_log() {
    let env = MemEnv::new();
    // 1. `ls-write.html`
    run(&env, |store| write_three(store, ORIGIN));
    check(&env, 1);
    // 2. `ls-read.html`: só ler atualiza o acesso.
    run(&env, |store| {
        assert_eq!(store.open_area(ORIGIN), [("k1".into(), "um".into()), ("k2".into(), "dois".into()), ("k3".into(), "tres".into())]);
    });
    check(&env, 2);
    // 3. `ls-modify.html`: remove, grava em Latin1, em UTF-16, vazio, e regrava um valor igual (que não é mudança).
    run(&env, |store| {
        store.open_area(ORIGIN);
        store.record(ORIGIN, Change::Remove("k2"));
        store.record(ORIGIN, Change::Set("k4", "ação"));
        store.record(ORIGIN, Change::Set("k5", "日本語"));
        store.record(ORIGIN, Change::Set("chave中", "v"));
        store.record(ORIGIN, Change::Set("e", ""));
        store.record(ORIGIN, Change::Set("k1", "um"));
    });
    check(&env, 3);
    // 4. `ls-read.html`: as entradas voltam na ordem de chave do banco (o formato UTF-16 vem antes do Latin1).
    run(&env, |store| {
        let entries = store.open_area(ORIGIN);
        let keys: Vec<&str> = entries.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, ["chave中", "e", "k1", "k3", "k4", "k5"]);
        assert_eq!(entries[4], ("k4".to_owned(), "ação".to_owned()));
        assert_eq!(entries[5], ("k5".to_owned(), "日本語".to_owned()));
    });
    check(&env, 4);
    // 5. `ls-clear.html`
    run(&env, |store| {
        store.open_area(ORIGIN);
        store.record(ORIGIN, Change::Clear);
    });
    check(&env, 5);
    // 6. `ls-read.html` numa área vazia: nada é gravado.
    run(&env, |store| assert!(store.open_area(ORIGIN).is_empty()));
    check(&env, 6);
    // 7. `ls-write.html` de novo.
    run(&env, |store| write_three(store, ORIGIN));
    check(&env, 7);
    // 8. `ls-write.html` em outra origem: um lote só dela, e a primeira origem nem é aberta.
    run(&env, |store| write_three(store, OTHER_ORIGIN));
    check(&env, 8);
    // 9. `ls-read.html` na primeira origem.
    run(&env, |store| {
        store.open_area(ORIGIN);
    });
    check(&env, 9);
}
