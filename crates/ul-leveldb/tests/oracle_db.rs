//! O banco contra o que o leveldb do Chromium 154 do oráculo escreveu: abre o diretório de uma execução, refaz as
//! escritas da execução seguinte e confere que os arquivos resultantes são os mesmos, byte a byte (o `LOG` só difere
//! na hora e no id da thread). Os diretórios em `tests/fixtures` vêm de `testbench/oracle/measure-reopen.sh`.

use std::rc::Rc;

use ul_leveldb::batch::{Op, WriteBatch};
use ul_leveldb::env::{Env, MemEnv};
use ul_leveldb::log::read_records;
use ul_leveldb::{Db, Options};

const DIR: &str = "/tmp/profile/Default/Local Storage/leveldb";

fn fixture(dir: &str, name: &str) -> Option<Vec<u8>> {
    std::fs::read(format!("{}/tests/fixtures/{dir}/{name}", env!("CARGO_MANIFEST_DIR"))).ok()
}

fn names(dir: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(format!("{}/tests/fixtures/{dir}", env!("CARGO_MANIFEST_DIR"))).unwrap().map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    names
}

/// O diretório do banco como estava ao fim da execução anterior.
fn seed(dir: &str) -> MemEnv {
    let env = MemEnv::new();
    // 2026-10-10 21:00:00 UTC: o relógio do `LOG` novo.
    env.set_clock(1_791_666_000_000_000);
    for name in names(dir) {
        env.write_file(&format!("{DIR}/{name}"), &fixture(dir, &name).unwrap()).unwrap();
    }
    env
}

/// O último log de um diretório de fixture (o que recebe as escritas).
fn last_log(dir: &str) -> Option<String> {
    names(dir).into_iter().filter(|name| name.ends_with(".log")).last()
}

/// As escritas que a execução seguinte fez: os registros do log dela que o log anterior não tinha (o mesmo arquivo,
/// quando o banco reaproveitou o log da execução anterior; senão, um log novo, todo dela).
fn new_batches(previous: &str, next: &str) -> Vec<WriteBatch> {
    let log_name = last_log(next).unwrap();
    let skip = match last_log(previous) {
        Some(old) if old == log_name => read_records(&fixture(previous, &old).unwrap()).unwrap().len(),
        _ => 0,
    };
    let records = read_records(&fixture(next, &log_name).unwrap()).unwrap();
    records.iter().skip(skip).map(|record| WriteBatch::from_contents(record).unwrap()).collect()
}

fn replay(db: &mut Db, batches: &[WriteBatch]) {
    for source in batches {
        let mut batch = WriteBatch::new();
        for op in source.ops().unwrap() {
            match op {
                Op::Put { key, value } => batch.put(&key, &value),
                Op::Delete { key } => batch.delete(&key),
            }
        }
        db.write(batch).unwrap();
    }
}

/// O texto do `LOG` sem a hora e o id da thread de cada linha. As linhas `Delete` seguidas saem na ordem em que o
/// sistema de arquivos lista o diretório (a do oráculo é a do overlayfs dele), então cada sequência é ordenada.
fn log_messages(log: &[u8]) -> Vec<String> {
    let mut messages: Vec<String> = String::from_utf8_lossy(log).lines().map(|line| line.splitn(3, ' ').nth(2).unwrap_or("").to_owned()).collect();
    let mut start = 0;
    while start < messages.len() {
        let mut end = start;
        while end < messages.len() && messages[end].starts_with("Delete type=") {
            end += 1;
        }
        messages[start..end].sort();
        start = end.max(start + 1);
    }
    messages
}

fn assert_same_directory(env: &MemEnv, expected: &str) {
    let actual = env.snapshot();
    let actual_names: Vec<String> = actual.keys().filter_map(|path| path.strip_prefix(&format!("{DIR}/")).map(str::to_owned)).collect();
    assert_eq!(actual_names, names(expected), "os arquivos do diretório");
    for name in names(expected) {
        let want = fixture(expected, &name).unwrap();
        let got = &actual[&format!("{DIR}/{name}")];
        if name == "LOG" {
            assert_eq!(log_messages(got), log_messages(&want), "texto do LOG");
            // Cada linha tem a hora no formato do leveldb (a largura da hora e do id entram no tamanho do arquivo).
            assert!(String::from_utf8_lossy(got).lines().all(|line| line.len() > 28 && &line[4..5] == "/" && &line[19..20] == "."), "formato da hora do LOG");
        } else {
            assert_eq!(got, &want, "{name}");
        }
    }
}

fn reopen(previous: &str, next: &str) -> MemEnv {
    let env = seed(previous);
    let mut options = Options::chromium(0x4e);
    options.create_if_missing = false;
    {
        let mut db = Db::open(Rc::new(env.clone()), DIR, options).unwrap();
        replay(&mut db, &new_batches(previous, next));
    }
    env
}

#[test]
fn a_small_log_is_reused_and_appended_to() {
    assert_same_directory(&reopen("reopen/run1", "reopen/run2"), "reopen/run2");
    assert_same_directory(&reopen("reopen/run2", "reopen/run3"), "reopen/run3");
}

#[test]
fn a_log_just_under_the_write_buffer_is_still_reused() {
    assert_same_directory(&reopen("fill60/run1", "fill60/run2"), "fill60/run2");
}

#[test]
fn a_log_over_the_write_buffer_becomes_a_table_on_recovery() {
    assert_same_directory(&reopen("fill80/run1", "fill80/run2"), "fill80/run2");
}

/// Valores de formas diferentes (JSON, palavras, base64, repetições): a tabela de 60 KB que o oráculo escreveu a
/// partir deles tem os mesmos bytes que a nossa, o que só acontece se o compressor snappy decide igual em cada bloco.
#[test]
fn a_table_of_varied_data_has_the_bytes_of_the_oracles_snappy() {
    assert_same_directory(&reopen("mixed/run1", "mixed/run2"), "mixed/run2");
}

/// Valores de 20, 40, 70 e 130 mil caracteres: cada um ocupa um bloco de dados inteiro e passa de um ou de dois
/// fragmentos de 64 KiB do compressor, que escolhe o tamanho da tabela de hash pelo tamanho do fragmento.
#[test]
fn a_table_of_large_values_has_the_bytes_of_the_oracles_snappy() {
    assert_same_directory(&reopen("large/run1", "large/run2"), "large/run2");
}

#[test]
fn the_fourth_level_zero_table_triggers_a_compaction_when_the_database_opens() {
    // Quatro tabelas de nível 0 com as mesmas cinco chaves viram uma só no nível 1: as versões encobertas somem, e o
    // texto do LOG, os números dos arquivos, o MANIFEST e a tabela de saída têm os bytes do oráculo.
    assert_same_directory(&reopen("compact/run4", "compact/run5"), "compact/run5");
}

#[test]
fn a_database_whose_tables_were_compacted_reopens_like_the_oracles() {
    assert_same_directory(&reopen("compact/run5", "compact/run6"), "compact/run6");
}

#[test]
fn a_new_database_matches_the_first_run_of_the_oracle() {
    let env = MemEnv::new();
    env.set_clock(1_791_666_000_000_000);
    {
        let mut db = Db::open(Rc::new(env.clone()), DIR, Options::chromium(0x5a)).unwrap();
        replay(&mut db, &new_batches("", "reopen/run1"));
    }
    assert_same_directory(&env, "reopen/run1");
}
