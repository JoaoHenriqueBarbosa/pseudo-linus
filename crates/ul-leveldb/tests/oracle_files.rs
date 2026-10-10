//! Os arquivos que o leveldb do Chromium 154 do oráculo escreveu para o Local Storage de `http://127.0.0.1:8000` (a
//! página `testbench/corpus/headless-storage/probe.html`, medida por `testbench/oracle/measure-storage.sh`): esta
//! crate os lê e, escrevendo os mesmos dados, produz os mesmos bytes.

use ul_leveldb::batch::{Op, WriteBatch};
use ul_leveldb::log::{read_records, Writer};
use ul_leveldb::version_edit::VersionEdit;

const LOG: &[u8] = include_bytes!("fixtures/local_storage/000003.log");
const MANIFEST: &[u8] = include_bytes!("fixtures/local_storage/MANIFEST-000001");

/// Escreve de volta os registros lidos e confere que sai o arquivo inteiro.
fn rewrite(records: &[Vec<u8>]) -> Vec<u8> {
    let mut writer = Writer::new();
    records.iter().flat_map(|record| writer.add_record(record)).collect()
}

#[test]
fn the_oracle_manifest_reads_and_rewrites_byte_for_byte() {
    let records = read_records(MANIFEST).unwrap();
    assert_eq!(records.len(), 1);
    let edit = VersionEdit::decode(&records[0]).unwrap();
    assert_eq!(edit.comparator.as_deref(), Some(&b"leveldb.BytewiseComparator"[..]));
    assert_eq!((edit.log_number, edit.next_file_number, edit.last_sequence), (Some(0), Some(2), Some(0)));
    assert_eq!(edit.encode(), records[0]);
    assert_eq!(rewrite(&records), MANIFEST);
}

#[test]
fn the_oracle_log_reads_and_rewrites_byte_for_byte() {
    let records = read_records(LOG).unwrap();
    assert_eq!(records.len(), 2);
    let version = WriteBatch::from_contents(&records[0]).unwrap();
    assert_eq!((version.sequence(), version.count()), (1, 1));
    assert_eq!(version.ops().unwrap(), vec![Op::Put { key: b"VERSION".to_vec(), value: b"1".to_vec() }]);
    let data = WriteBatch::from_contents(&records[1]).unwrap();
    assert_eq!((data.sequence(), data.count()), (2, 4));
    let ops = data.ops().unwrap();
    assert!(matches!(&ops[0], Op::Put { key, value } if key == b"_http://127.0.0.1:8000\0\x01k1" && value == b"\x01v1"));
    assert!(matches!(&ops[1], Op::Put { key, value } if key == b"_http://127.0.0.1:8000\0\x01key2" && value == b"\x01valor dois"));
    assert!(matches!(&ops[2], Op::Put { key, .. } if key == b"METAACCESS:http://127.0.0.1:8000"));
    assert!(matches!(&ops[3], Op::Put { key, .. } if key == b"META:http://127.0.0.1:8000"));
    let rewritten: Vec<Vec<u8>> = [&version, &data].iter().map(|batch| batch.contents().to_vec()).collect();
    assert_eq!(rewrite(&rewritten), LOG);
}
