//! O IndexedDB de uma origem no leveldb do Chromium (`<perfil>/Default/IndexedDB/<origem>.indexeddb.leveldb`): o
//! que cada transação grava, nos lotes e na ordem que o Chromium usa (medidos no oráculo, ver
//! `testbench/corpus/headless-storage/idb-*.html`), e a carga do que está no disco.
//!
//! Cada transação que escreve roda sobre um *escopo* do `LevelDBScopes`: os metadados do escopo (`G+0x32 0x01 <id>`,
//! os bloqueios em protobuf) entram no primeiro lote, cada escrita deixa uma tarefa de desfazer
//! (`G+0x32 0x02 <id> 0x00 <contador>`, o contador desce a partir de `i64::MAX`) com o valor que a chave tinha, o commit
//! zera os metadados (no último lote, quando ele é de uma requisição, ou num lote só dele) e o lote seguinte apaga as
//! tarefas e os metadados.
//!
//! Dois estilos de lote: o *das requisições* (put de registro, delete, renomear) junta as escritas, depois a marca
//! dos metadados (bloqueios no primeiro, vazia no commit) e por fim as tarefas de desfazer; o *dos apagamentos de
//! esquema* (apagar índice, repositório, `clear`) intercala, para cada chave, a tarefa de desfazer e a remoção.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use ul_leveldb::batch::WriteBatch;
use ul_leveldb::coding::{put_length_prefixed, Input};
use ul_leveldb::env::Env;
use ul_leveldb::{Db, Options, Result};

use super::coding::{
    database, decode_int, encode_int, global, index_data_key, index_data_value, index_field, key_prefix, object_store_data_value, record_key,
    read_string, record_index, store_field, string, units, varint, IdbKey, KeyPath,
};
use super::comparator::IdbComparator;
use ul_leveldb::comparator::Comparator;
use super::js_value::{self, Js, Kind};
use super::v8::{self, Value};

const GLOBAL: [u8; 4] = [0, 0, 0, 0];
const SCOPE_TYPE: u8 = 0x32;
const DATA_VERSION: u64 = 0x10_0000_0015;
const SCHEMA_VERSION: u64 = 5;
const BLOB_KEY_GENERATOR_START: u64 = 2;

fn scopes_version_key() -> Vec<u8> {
    [GLOBAL.as_slice(), &[SCOPE_TYPE, 0]].concat()
}

fn scope_metadata_key(scope: u64) -> Vec<u8> {
    let mut key = [GLOBAL.as_slice(), &[SCOPE_TYPE, 1]].concat();
    varint(&mut key, scope);
    key
}

/// `kind`: 0 as tarefas de desfazer (contador decrescente), 1 as de limpeza (crescente).
fn scope_task_key(scope: u64, kind: u8, counter: u64) -> Vec<u8> {
    let mut key = [GLOBAL.as_slice(), &[SCOPE_TYPE, 2]].concat();
    varint(&mut key, scope);
    key.push(kind);
    key.extend_from_slice(&counter.to_be_bytes());
    key
}

/// Um campo de protobuf de tipo 2: a marca e o corpo com o tamanho na frente.
fn length_prefixed(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    put_length_prefixed(&mut out, body);
    out
}

/// A tarefa de desfazer de uma chave: recolocar o valor antigo, ou apagar a chave que não existia.
fn undo_task(key: &[u8], old: Option<&[u8]>) -> Vec<u8> {
    let key_field = length_prefixed(0x0a, key);
    match old {
        Some(value) => length_prefixed(0x0a, &[key_field, length_prefixed(0x12, value)].concat()),
        None => length_prefixed(0x12, &key_field),
    }
}

/// O bloqueio do banco, pelo nome (UTF-8).
fn database_lock(name: &[u16]) -> Vec<u8> {
    let name = String::from_utf16_lossy(name);
    length_prefixed(0x0a, &length_prefixed(0x12, &length_prefixed(0x0a, name.as_bytes())))
}

/// O bloqueio de um repositório: tipo 1 e o par (banco, repositório) em 8 bytes cada, big-endian.
fn store_lock(database_id: u64, store_id: u64) -> Vec<u8> {
    let range = [database_id.to_be_bytes(), store_id.to_be_bytes()].concat();
    length_prefixed(0x0a, &[vec![0x08, 0x01], length_prefixed(0x12, &length_prefixed(0x0a, &range))].concat())
}

#[derive(Clone, Debug)]
enum Op {
    Put(Vec<u8>, Vec<u8>),
    Del(Vec<u8>),
}

impl Op {
    fn key(&self) -> &[u8] {
        match self {
            Op::Put(key, _) | Op::Del(key) => key,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    VersionChange,
    ReadWrite,
}

/// Uma mutação que a transação fez, na ordem das requisições.
pub enum Entry {
    CreateStore { name: Vec<u16>, key_path: KeyPath, auto_increment: bool },
    DeleteStore(Vec<u16>),
    CreateIndex { store: Vec<u16>, name: Vec<u16>, key_path: KeyPath, unique: bool, multi_entry: bool, records: Vec<(IdbKey, Vec<IdbKey>)> },
    DeleteIndex { store: Vec<u16>, name: Vec<u16> },
    RenameStore { from: Vec<u16>, to: Vec<u16> },
    RenameIndex { store: Vec<u16>, from: Vec<u16>, to: Vec<u16> },
    /// `generated`: a chave veio do gerador (que então anda no lote da existência); uma chave explícita que passa
    /// do gerador o empurra num lote só dele, depois.
    Put { store: Vec<u16>, key: IdbKey, value: Value, indexes: Vec<(Vec<u16>, Vec<IdbKey>)>, generator: f64, generated: bool },
    Delete { store: Vec<u16>, key: IdbKey },
    Clear(Vec<u16>),
}

/// O que o prelúdio devolve ao perfil quando uma transação que escreve termina.
pub struct Commit {
    mode: Mode,
    pub name: Vec<u16>,
    pub version: u64,
    scope: Vec<Vec<u16>>,
    journal: Vec<Entry>,
}

fn text_of(js: &Js) -> Option<Vec<u16>> {
    match js {
        Js::String(text) => Some(text.clone()),
        _ => None,
    }
}

fn items_of(js: &Js) -> Option<Vec<&Js>> {
    match js {
        Js::Node(_, kind) => matches!(&**kind, Kind::Array(..)).then(|| js.items()),
        _ => None,
    }
}

fn keys_of(js: &Js) -> Option<Vec<IdbKey>> {
    items_of(js)?.into_iter().map(js_value::idb_key).collect()
}

fn entry_of(js: &Js) -> Option<Entry> {
    let items = items_of(js)?;
    let kind = items.first()?.string()?;
    let field = |index: usize| items.get(index).copied();
    Some(match kind.as_str() {
        "createStore" => {
            let options = field(1)?;
            Entry::CreateStore { name: text_of(options.field("name")?)?, key_path: js_value::key_path(options.field("keyPath")?)?, auto_increment: options.field("autoIncrement")?.flag() }
        }
        "deleteStore" => Entry::DeleteStore(text_of(field(1)?)?),
        "createIndex" => {
            let options = field(3 - 1)?;
            let records = items_of(field(3)?)?
                .into_iter()
                .map(|record| {
                    let pair = items_of(record)?;
                    Some((js_value::idb_key(pair.first()?)?, keys_of(pair.get(1)?)?))
                })
                .collect::<Option<_>>()?;
            Entry::CreateIndex {
                store: text_of(field(1)?)?,
                name: text_of(options.field("name")?)?,
                key_path: js_value::key_path(options.field("keyPath")?)?,
                unique: options.field("unique")?.flag(),
                multi_entry: options.field("multiEntry")?.flag(),
                records,
            }
        }
        "deleteIndex" => Entry::DeleteIndex { store: text_of(field(1)?)?, name: text_of(field(2)?)? },
        "renameStore" => Entry::RenameStore { from: text_of(field(1)?)?, to: text_of(field(2)?)? },
        "renameIndex" => Entry::RenameIndex { store: text_of(field(1)?)?, from: text_of(field(2)?)?, to: text_of(field(3)?)? },
        "put" => {
            let indexes = items_of(field(4)?)?
                .into_iter()
                .map(|pair| {
                    let pair = items_of(pair)?;
                    Some((text_of(pair.first()?)?, keys_of(pair.get(1)?)?))
                })
                .collect::<Option<_>>()?;
            Entry::Put {
                store: text_of(field(1)?)?,
                key: js_value::idb_key(field(2)?)?,
                value: js_value::to_v8(field(3)?)?,
                indexes,
                generator: field(5)?.number()?,
                generated: field(6)?.flag(),
            }
        }
        "delete" => Entry::Delete { store: text_of(field(1)?)?, key: js_value::idb_key(field(2)?)? },
        "clear" => Entry::Clear(text_of(field(1)?)?),
        _ => return None,
    })
}

impl Commit {
    /// Lê o texto do prelúdio (`commitJournal`); `None` quando ele não é de uma transação que o disco entende.
    pub fn parse(source: &str) -> Option<Commit> {
        let root = js_value::parse(source)?;
        let mode = match root.field("mode")?.string()?.as_str() {
            "versionchange" => Mode::VersionChange,
            "readwrite" => Mode::ReadWrite,
            _ => return None,
        };
        let scope = root.field("scope").map(|scope| scope.items().into_iter().filter_map(text_of).collect()).unwrap_or_default();
        Some(Commit {
            mode,
            name: Vec::new(),
            version: root.field("version")?.number()? as u64,
            scope,
            journal: root.field("journal")?.items().into_iter().map(entry_of).collect::<Option<_>>()?,
        })
    }
}

/// O leveldb de uma origem.
pub struct OriginDisk {
    db: Db,
    origin: Vec<u16>,
    next_scope: u64,
}

fn io(message: impl Into<String>) -> ul_leveldb::Error {
    ul_leveldb::Error::InvalidArgument(message.into())
}

impl OriginDisk {
    /// Abre (ou cria) o banco. Um banco novo recebe a versão dos escopos e as versões do esquema e dos dados; um que já
    /// existia recebe o lote vazio com que o Chromium marca cada abertura.
    pub fn open(env: Arc<dyn Env>, dir: &str, thread_id: u64, origin_key: &str) -> Result<OriginDisk> {
        let mut options = Options::chromium(thread_id);
        options.comparator = Arc::new(IdbComparator);
        let mut db = Db::open(env, dir, options)?;
        if db.get(&scopes_version_key())?.is_none() {
            let mut batch = WriteBatch::new();
            batch.put(&scopes_version_key(), &[0x08, 0x01]);
            db.write(batch)?;
            let mut batch = WriteBatch::new();
            batch.put(&global::schema_version(), &encode_int(SCHEMA_VERSION));
            batch.put(&global::data_version(), &encode_int(DATA_VERSION));
            db.write(batch)?;
        } else {
            db.write(WriteBatch::new())?;
        }
        Ok(OriginDisk { db, origin: units(origin_key), next_scope: 0 })
    }

    fn name_key(&self, name: &[u16]) -> Vec<u8> {
        global::database_name(&self.origin, name)
    }

    fn database_id(&self, name: &[u16]) -> Result<Option<u64>> {
        Ok(self.db.get(&self.name_key(name))?.map(|bytes| decode_int(&bytes)))
    }

    fn int(&self, key: &[u8]) -> Result<Option<u64>> {
        Ok(self.db.get(key)?.map(|bytes| decode_int(&bytes)))
    }

    fn varint_value(&self, key: &[u8]) -> Result<Option<u64>> {
        Ok(self.db.get(key)?.and_then(|bytes| read_varint(&bytes)))
    }

    fn write(&mut self, ops: &[Op]) -> Result<()> {
        let mut batch = WriteBatch::new();
        for op in ops {
            match op {
                Op::Put(key, value) => batch.put(key, value),
                Op::Del(key) => batch.delete(key),
            }
        }
        self.db.write(batch)
    }

    /// O banco novo: o id seguinte, o nome, a versão 0 e o gerador de blobs.
    fn create_database(&mut self, name: &[u16]) -> Result<u64> {
        let id = self.int(&global::max_database_id())?.unwrap_or(0) + 1;
        let mut version = Vec::new();
        varint(&mut version, 0);
        let mut blob = Vec::new();
        varint(&mut blob, BLOB_KEY_GENERATOR_START);
        self.write(&[
            Op::Put(global::max_database_id(), encode_int(id)),
            Op::Put(self.name_key(name), encode_int(id)),
            Op::Put(database::int_version(id), version),
            Op::Put(database::blob_key_generator_current_number(id), blob),
        ])?;
        Ok(id)
    }

    fn store_id(&self, database_id: u64, name: &[u16]) -> Result<u64> {
        self.int(&database::object_store_name(database_id, name))?.ok_or_else(|| io("repositório desconhecido"))
    }

    fn index_id(&self, database_id: u64, store_id: u64, name: &[u16]) -> Result<u64> {
        let wanted = utf16_be(name);
        let last = self.int(&database::object_store_field(database_id, store_id, store_field::MAX_INDEX_ID))?.unwrap_or(0);
        (record_index::MINIMUM_INDEX_ID + 1..=last)
            .find(|id| self.db.get(&database::index_field(database_id, store_id, *id, index_field::NAME)).ok().flatten().as_deref() == Some(wanted.as_slice()))
            .ok_or_else(|| io("índice desconhecido"))
    }

    fn index_ids(&self, database_id: u64, store_id: u64) -> Result<Vec<u64>> {
        let last = self.int(&database::object_store_field(database_id, store_id, store_field::MAX_INDEX_ID))?.unwrap_or(0);
        Ok((record_index::MINIMUM_INDEX_ID + 1..=last).filter(|id| self.db.get(&database::index_field(database_id, store_id, *id, index_field::NAME)).ok().flatten().is_some()).collect())
    }

    /// As entradas do banco cuja chave começa por `prefix`, na ordem do comparador.
    fn scan(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        Ok(self.db.entries()?.into_iter().filter(|(key, _)| key.starts_with(prefix)).collect())
    }

    /// `Commit` de uma transação que escreve: os lotes dela, o commit e a limpeza dos escopos.
    pub fn commit(&mut self, commit: &Commit) -> Result<()> {
        let database_id = match self.database_id(&commit.name)? {
            Some(id) => id,
            None => self.create_database(&commit.name)?,
        };
        if commit.mode == Mode::ReadWrite && commit.journal.is_empty() {
            return Ok(());
        }
        let scope = self.next_scope;
        self.next_scope += 1;
        let mut locks = database_lock(&commit.name);
        if commit.mode == Mode::ReadWrite {
            let mut stores: Vec<u64> = commit.scope.iter().filter_map(|name| self.store_id(database_id, name).ok()).collect();
            stores.sort_unstable();
            for store in stores {
                locks.extend(store_lock(database_id, store));
            }
        }
        let mut tx = Tx { disk: self, scope, locks, database_id, meta_written: false, next_undo: i64::MAX as u64, undo_keys: Vec::new(), pending: None };
        if commit.mode == Mode::VersionChange {
            let mut version = Vec::new();
            varint(&mut version, commit.version);
            tx.direct(vec![Op::Put(database::int_version(database_id), version)])?;
        }
        let mut queued: Vec<&Entry> = Vec::new();
        for entry in &commit.journal {
            match entry {
                Entry::CreateStore { name, key_path, auto_increment } => tx.create_store(name, key_path, *auto_increment)?,
                Entry::CreateIndex { store, name, key_path, unique, multi_entry, .. } => {
                    tx.create_index(store, name, key_path, *unique, *multi_entry)?;
                    queued.push(entry);
                }
                Entry::DeleteIndex { store, name } => tx.delete_index(store, name)?,
                Entry::DeleteStore(name) => tx.delete_store(name)?,
                _ if commit.mode == Mode::VersionChange => queued.push(entry),
                other => tx.queued(other)?,
            }
        }
        let mut renames = Vec::new();
        for entry in queued {
            if matches!(entry, Entry::RenameStore { .. } | Entry::RenameIndex { .. }) {
                renames.push(entry);
                continue;
            }
            tx.rename_batch(&mut renames)?;
            tx.queued(entry)?;
        }
        tx.rename_batch(&mut renames)?;
        tx.finish()
    }

    /// `deleteDatabase`: a remoção do nome sob um escopo com tarefa de limpeza da faixa do banco, o commit, e a limpeza.
    pub fn delete_database(&mut self, name: &[u16]) -> Result<()> {
        let Some(database_id) = self.database_id(name)? else { return Ok(()) };
        let scope = self.next_scope;
        self.next_scope += 1;
        let name_key = self.name_key(name);
        let old_name = self.db.get(&name_key)?.unwrap_or_default();
        let range = [key_prefix(database_id, 0, 0), vec![0]].concat();
        let next_range = [key_prefix(database_id + 1, 0, 0), vec![0]].concat();
        let cleanup_task = [length_prefixed(0x0a, &range), length_prefixed(0x12, &next_range)].concat();
        let undo_key = scope_task_key(scope, 0, i64::MAX as u64);
        self.write(&[
            Op::Put(scope_metadata_key(scope), database_lock(name)),
            Op::Put(scope_task_key(scope, 1, 0), length_prefixed(0x12, &cleanup_task)),
            Op::Del(name_key.clone()),
            Op::Put(undo_key.clone(), undo_task(&name_key, Some(&old_name))),
        ])?;
        let journal_key = [GLOBAL.as_slice(), &[3]].concat();
        let existing = self.db.get(&journal_key)?;
        let mut blob_journal = existing.unwrap_or_else(|| vec![0]);
        blob_journal[0] += 1;
        varint(&mut blob_journal, database_id);
        let blob_undo = scope_task_key(scope, 0, i64::MAX as u64 - 1);
        self.write(&[
            Op::Put(journal_key.clone(), blob_journal),
            Op::Put(scope_metadata_key(scope), Vec::new()),
            Op::Put(blob_undo.clone(), undo_task(&journal_key, None)),
        ])?;
        self.write(&[Op::Del(journal_key)])?;
        let mut cleanup = vec![Op::Del(blob_undo), Op::Del(undo_key)];
        let inside = |key: &[u8]| IdbComparator.compare(key, &range).is_ge() && IdbComparator.compare(key, &next_range).is_lt();
        cleanup.extend(self.db.entries()?.into_iter().filter(|(key, _)| inside(key)).map(|(key, _)| Op::Del(key)));
        self.write(&cleanup)?;
        // A faixa do banco apagado é compactada antes do último lote (o `LOG` do oráculo: a tabela de nível 0, o log
        // velho apagado e a linha `Manual compaction at level-0 from '\x00\x02\x00\x00\x00' ...`).
        self.db.compact_range(&range, &next_range)?;
        self.write(&[Op::Del(scope_task_key(scope, 1, 0)), Op::Del(scope_metadata_key(scope))])
    }

    /// O banco da origem como o texto que o prelúdio lê: `[[nome, texto], ...]` na ordem do comparador.
    pub fn load(&self) -> Result<Vec<(String, String)>> {
        let prefix = [GLOBAL.as_slice(), &[201]].concat();
        let mut loaded = Vec::new();
        for (key, value) in self.scan(&prefix)? {
            let mut input = Input::new(&key[prefix.len()..]);
            let origin = read_string(&mut input).ok_or_else(|| io("nome de banco ilegível"))?;
            if origin != self.origin {
                continue;
            }
            let name = read_string(&mut input).ok_or_else(|| io("nome de banco ilegível"))?;
            let id = decode_int(&value);
            loaded.push((String::from_utf16_lossy(&name), self.read_database(id)?));
        }
        Ok(loaded)
    }

    fn read_database(&self, database_id: u64) -> Result<String> {
        let version = self.varint_value(&database::int_version(database_id))?.unwrap_or(0);
        let mut writer = js_value::Writer::default();
        writer.object(|writer| {
            writer.field(true, "version", |writer| writer.number(version as f64));
            writer.field(false, "stores", |writer| {
                writer.claim();
                writer.raw("[\"M\",[");
                let names = self.scan(&[key_prefix(database_id, 0, 0), vec![200]].concat()).unwrap_or_default();
                for (position, (key, value)) in names.iter().enumerate() {
                    if position > 0 {
                        writer.raw(",");
                    }
                    let name = read_string(&mut Input::new(&key[key_prefix(database_id, 0, 0).len() + 1..])).unwrap_or_default();
                    writer.raw("[");
                    writer.string(&name);
                    writer.raw(",");
                    self.write_store(writer, database_id, decode_int(value), &name);
                    writer.raw("]");
                }
                writer.raw("]]");
            });
        });
        Ok(writer.out)
    }

    fn write_store(&self, writer: &mut js_value::Writer, database_id: u64, store_id: u64, name: &[u16]) {
        let field = |number: u8| self.db.get(&database::object_store_field(database_id, store_id, number)).ok().flatten().unwrap_or_default();
        let key_path = read_key_path(&field(store_field::KEY_PATH));
        let auto_increment = field(store_field::AUTO_INCREMENT).first().copied().unwrap_or(0) != 0;
        let generator = decode_int(&field(store_field::KEY_GENERATOR_CURRENT_NUMBER));
        let records = self.scan(&key_prefix(database_id, store_id, record_index::OBJECT_STORE_DATA)).unwrap_or_default();
        let prefix_len = key_prefix(database_id, store_id, record_index::OBJECT_STORE_DATA).len();
        writer.object(|writer| {
            writer.field(true, "name", |writer| writer.string(name));
            writer.field(false, "keyPath", |writer| write_key_path(writer, &key_path));
            writer.field(false, "autoIncrement", |writer| writer.raw(if auto_increment { "true" } else { "false" }));
            writer.field(false, "generator", |writer| writer.number(generator as f64));
            writer.field(false, "records", |writer| {
                writer.claim();
                writer.raw(&format!("[\"a\",{},[", records.len()));
                for (position, (key, value)) in records.iter().enumerate() {
                    if position > 0 {
                        writer.raw(",");
                    }
                    writer.raw(&format!("[\"{position}\","));
                    writer.object(|writer| {
                        writer.field(true, "key", |writer| write_idb_key(writer, &key[prefix_len..]));
                        writer.field(false, "value", |writer| {
                            let mut input = Input::new(value);
                            let _version = input.varint64();
                            match v8::read_wire_value(input.remaining()) {
                                Some(value) => writer.value(&value, &mut 0, &mut HashMap::new()),
                                None => writer.raw("[\"u\"]"),
                            }
                        });
                    });
                    writer.raw("]");
                }
                writer.raw("]]");
            });
            writer.field(false, "indexes", |writer| {
                writer.claim();
                writer.raw("[\"M\",[");
                let ids = self.index_ids(database_id, store_id).unwrap_or_default();
                for (position, id) in ids.iter().enumerate() {
                    if position > 0 {
                        writer.raw(",");
                    }
                    let read = |number: u8| self.db.get(&database::index_field(database_id, store_id, *id, number)).ok().flatten().unwrap_or_default();
                    let index_name: Vec<u16> = read(index_field::NAME).chunks_exact(2).map(|pair| u16::from_be_bytes([pair[0], pair[1]])).collect();
                    writer.raw("[");
                    writer.string(&index_name);
                    writer.raw(",");
                    writer.object(|writer| {
                        writer.field(true, "name", |writer| writer.string(&index_name));
                        writer.field(false, "keyPath", |writer| write_key_path(writer, &read_key_path(&read(index_field::KEY_PATH))));
                        writer.field(false, "unique", |writer| writer.raw(if read(index_field::UNIQUE).first().copied().unwrap_or(0) != 0 { "true" } else { "false" }));
                        writer.field(false, "multiEntry", |writer| writer.raw(if read(index_field::MULTI_ENTRY).first().copied().unwrap_or(0) != 0 { "true" } else { "false" }));
                        writer.field(false, "entries", |writer| {
                            writer.claim();
                            writer.raw("[\"a\",0,[]]");
                        });
                    });
                    writer.raw("]");
                }
                writer.raw("]]");
            });
            writer.field(false, "deleted", |writer| writer.raw("false"));
        });
    }
}

fn utf16_be(text: &[u16]) -> Vec<u8> {
    let mut out = Vec::new();
    string(&mut out, text);
    out
}

fn read_varint(bytes: &[u8]) -> Option<u64> {
    Input::new(bytes).varint64().ok()
}

fn read_key_path(bytes: &[u8]) -> KeyPath {
    let mut input = Input::new(bytes.get(3..).unwrap_or_default());
    match bytes.get(2) {
        Some(1) => read_string(&mut input).map_or(KeyPath::Null, KeyPath::String),
        Some(2) => {
            let count = input.varint64().unwrap_or(0);
            KeyPath::Array((0..count).filter_map(|_| read_string(&mut input)).collect())
        }
        _ => KeyPath::Null,
    }
}

fn write_key_path(writer: &mut js_value::Writer, path: &KeyPath) {
    match path {
        KeyPath::Null => writer.raw("null"),
        KeyPath::String(text) => writer.string(text),
        KeyPath::Array(paths) => {
            writer.claim();
            writer.raw(&format!("[\"a\",{},[", paths.len()));
            for (position, path) in paths.iter().enumerate() {
                if position > 0 {
                    writer.raw(",");
                }
                writer.raw(&format!("[\"{position}\","));
                writer.string(path);
                writer.raw("]");
            }
            writer.raw("]]");
        }
    }
}

/// A chave do IndexedDB do disco como o objeto `{ type, value }` do prelúdio.
fn write_idb_key(writer: &mut js_value::Writer, bytes: &[u8]) {
    write_idb_key_from(writer, &mut Input::new(bytes));
}

fn write_idb_key_from(writer: &mut js_value::Writer, input: &mut Input) {
    let kind = input.take(1).map_or(3, |byte| byte[0]);
    let (type_number, ()) = match kind {
        3 => (1, ()),
        2 => (2, ()),
        1 => (3, ()),
        6 => (4, ()),
        _ => (5, ()),
    };
    writer.object(|writer| {
        writer.field(true, "type", |writer| writer.raw(&type_number.to_string()));
        writer.field(false, "value", |writer| match kind {
            3 | 2 => writer.number(input.fixed64().map_or(0.0, f64::from_bits)),
            1 => writer.string(&read_string(input).unwrap_or_default()),
            6 => {
                let length = input.varint64().unwrap_or(0) as usize;
                let data = input.take(length).unwrap_or_default().to_vec();
                writer.claim();
                writer.raw("[\"V\",\"Uint8Array\",");
                writer.claim();
                writer.raw("[\"B\",");
                writer.string(&data.iter().map(|byte| u16::from(*byte)).collect::<Vec<_>>());
                writer.raw(&format!("],0,{length}]"));
            }
            _ => {
                let count = input.varint64().unwrap_or(0);
                writer.claim();
                writer.raw(&format!("[\"a\",{count},["));
                for position in 0..count {
                    if position > 0 {
                        writer.raw(",");
                    }
                    writer.raw(&format!("[\"{position}\","));
                    write_idb_key_from(writer, input);
                    writer.raw("]");
                }
                writer.raw("]]");
            }
        });
    });
}

/// Uma transação em curso: o escopo, as tarefas de desfazer e o lote da última requisição, que o commit pode alcançar.
struct Tx<'a> {
    disk: &'a mut OriginDisk,
    scope: u64,
    locks: Vec<u8>,
    database_id: u64,
    meta_written: bool,
    next_undo: u64,
    undo_keys: Vec<Vec<u8>>,
    pending: Option<Vec<Op>>,
}

impl Tx<'_> {
    /// O valor de `key` como a transação o vê: o lote ainda não gravado por cima do banco.
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        if let Some(op) = self.pending.iter().flatten().rev().find(|op| op.key() == key) {
            return Ok(match op {
                Op::Put(_, value) => Some(value.clone()),
                Op::Del(_) => None,
            });
        }
        self.disk.db.get(key)
    }

    fn int(&self, key: &[u8]) -> Result<Option<u64>> {
        Ok(self.get(key)?.map(|bytes| decode_int(&bytes)))
    }

    fn undo_for(&mut self, op: &Op, overlay: &mut HashMap<Vec<u8>, Option<Vec<u8>>>) -> Result<Option<Op>> {
        let key = op.key().to_vec();
        let old = match overlay.get(&key) {
            Some(value) => value.clone(),
            None => self.disk.db.get(&key)?,
        };
        overlay.insert(key.clone(), match op {
            Op::Put(_, value) => Some(value.clone()),
            Op::Del(_) => None,
        });
        let task = match (op, &old) {
            (Op::Del(_), None) => return Ok(None),
            (_, old) => undo_task(&key, old.as_deref()),
        };
        let undo_key = scope_task_key(self.scope, 0, self.next_undo);
        self.next_undo -= 1;
        self.undo_keys.push(undo_key.clone());
        Ok(Some(Op::Put(undo_key, task)))
    }

    /// O lote no estilo das requisições: as escritas, a marca dos metadados e as tarefas de desfazer.
    fn write_requests(&mut self, ops: Vec<Op>, marker: bool) -> Result<()> {
        let mut overlay = HashMap::new();
        let mut undos = Vec::new();
        for op in &ops {
            undos.extend(self.undo_for(op, &mut overlay)?);
        }
        let mut all = ops;
        if marker {
            all.push(Op::Put(scope_metadata_key(self.scope), Vec::new()));
        } else if !self.meta_written {
            all.push(Op::Put(scope_metadata_key(self.scope), self.locks.clone()));
            self.meta_written = true;
        }
        all.extend(undos);
        self.disk.write(&all)
    }

    /// O lote no estilo dos apagamentos: para cada chave, a tarefa de desfazer e a remoção (ou o contrário, quando
    /// `removal_first`). Os metadados do escopo entram na frente se ainda não foram escritos.
    fn write_removals(&mut self, keys: Vec<Vec<u8>>, removal_first: bool) -> Result<()> {
        let mut overlay = HashMap::new();
        let mut all = Vec::new();
        if !self.meta_written {
            all.push(Op::Put(scope_metadata_key(self.scope), self.locks.clone()));
            self.meta_written = true;
        }
        for key in keys {
            let removal = Op::Del(key);
            let undo = self.undo_for(&removal, &mut overlay)?;
            if removal_first {
                all.push(removal);
                all.extend(undo);
            } else {
                all.extend(undo);
                all.push(removal);
            }
        }
        self.disk.write(&all)
    }

    fn flush_pending(&mut self) -> Result<()> {
        match self.pending.take() {
            Some(ops) => self.write_requests(ops, false),
            None => Ok(()),
        }
    }

    /// Um lote que não se funde com o commit.
    fn direct(&mut self, ops: Vec<Op>) -> Result<()> {
        self.flush_pending()?;
        self.write_requests(ops, false)
    }

    /// Um lote de requisição: espera o seguinte, que decide se a marca do commit entra nele.
    fn request(&mut self, ops: Vec<Op>) -> Result<()> {
        self.flush_pending()?;
        self.pending = Some(ops);
        Ok(())
    }

    fn removals(&mut self, keys: Vec<Vec<u8>>, removal_first: bool) -> Result<()> {
        if keys.is_empty() {
            return Ok(());
        }
        self.flush_pending()?;
        self.write_removals(keys, removal_first)
    }

    fn finish(mut self) -> Result<()> {
        match self.pending.take() {
            Some(ops) => self.write_requests(ops, true)?,
            None => self.write_requests(Vec::new(), true)?,
        }
        let mut cleanup: Vec<Op> = self.undo_keys.iter().rev().cloned().map(Op::Del).collect();
        cleanup.push(Op::Del(scope_metadata_key(self.scope)));
        self.disk.write(&cleanup)
    }

    fn store(&self, name: &[u16]) -> Result<u64> {
        self.int(&database::object_store_name(self.database_id, name))?.ok_or_else(|| io("repositório desconhecido"))
    }

    fn create_store(&mut self, name: &[u16], key_path: &KeyPath, auto_increment: bool) -> Result<()> {
        let id = self.disk.int(&database::max_object_store_id(self.database_id))?.unwrap_or(0) + 1;
        let db = self.database_id;
        let flag = |value: bool| vec![u8::from(value)];
        let mut ops = vec![Op::Put(database::max_object_store_id(db), encode_int(id))];
        let fields: [(u8, Vec<u8>); 8] = [
            (store_field::NAME, utf16_be(name)),
            (store_field::KEY_PATH, key_path.encode()),
            (store_field::AUTO_INCREMENT, flag(auto_increment)),
            (store_field::EVICTED, flag(false)),
            (store_field::LAST_VERSION, encode_int(1)),
            (store_field::MAX_INDEX_ID, encode_int(record_index::MINIMUM_INDEX_ID)),
            (store_field::HAS_KEY_PATH, flag(*key_path != KeyPath::Null)),
            (store_field::KEY_GENERATOR_CURRENT_NUMBER, encode_int(1)),
        ];
        ops.extend(fields.into_iter().map(|(number, value)| Op::Put(database::object_store_field(db, id, number), value)));
        ops.push(Op::Put(database::object_store_name(db, name), encode_int(id)));
        self.direct(ops)
    }

    fn create_index(&mut self, store: &[u16], name: &[u16], key_path: &KeyPath, unique: bool, multi_entry: bool) -> Result<()> {
        let (db, store_id) = (self.database_id, self.store(store)?);
        let id = self.disk.int(&database::object_store_field(db, store_id, store_field::MAX_INDEX_ID))?.unwrap_or(record_index::MINIMUM_INDEX_ID) + 1;
        let flag = |value: bool| vec![u8::from(value)];
        self.direct(vec![
            Op::Put(database::object_store_field(db, store_id, store_field::MAX_INDEX_ID), encode_int(id)),
            Op::Put(database::index_field(db, store_id, id, index_field::NAME), utf16_be(name)),
            Op::Put(database::index_field(db, store_id, id, index_field::UNIQUE), flag(unique)),
            Op::Put(database::index_field(db, store_id, id, index_field::KEY_PATH), key_path.encode()),
            Op::Put(database::index_field(db, store_id, id, index_field::MULTI_ENTRY), flag(multi_entry)),
        ])
    }

    fn delete_index(&mut self, store: &[u16], name: &[u16]) -> Result<()> {
        let (db, store_id) = (self.database_id, self.store(store)?);
        let id = self.disk.index_id(db, store_id, name)?;
        let fields = [index_field::NAME, index_field::UNIQUE, index_field::KEY_PATH, index_field::MULTI_ENTRY];
        self.removals(fields.iter().map(|number| database::index_field(db, store_id, id, *number)).collect(), false)?;
        let entries = self.disk.scan(&key_prefix(db, store_id, id))?;
        self.removals(entries.into_iter().map(|(key, _)| key).collect(), false)
    }

    fn delete_store(&mut self, name: &[u16]) -> Result<()> {
        let (db, store_id) = (self.database_id, self.store(name)?);
        // Os índices saem do `MAX_INDEX_ID`, que é o primeiro a ser apagado.
        let ids = self.disk.index_ids(db, store_id)?;
        self.removals((0..=7).map(|number| database::object_store_field(db, store_id, number)).collect(), false)?;
        self.removals(vec![database::object_store_name(db, name)], true)?;
        let fields = [index_field::NAME, index_field::UNIQUE, index_field::KEY_PATH, index_field::MULTI_ENTRY];
        self.removals(ids.iter().flat_map(|id| fields.iter().map(move |number| database::index_field(db, store_id, *id, *number))).collect(), false)?;
        self.record_removals(store_id)?;
        self.index_entry_removals(store_id, &ids)
    }

    /// Os dados e depois as marcas de existência de todos os registros do repositório, num lote só.
    fn record_removals(&mut self, store_id: u64) -> Result<()> {
        let db = self.database_id;
        let mut keys: Vec<Vec<u8>> = self.disk.scan(&key_prefix(db, store_id, record_index::OBJECT_STORE_DATA))?.into_iter().map(|(key, _)| key).collect();
        let exists: Vec<Vec<u8>> = self.disk.scan(&key_prefix(db, store_id, record_index::EXISTS_ENTRY))?.into_iter().map(|(key, _)| key).collect();
        keys.extend(exists);
        self.removals(keys, false)
    }

    fn index_entry_removals(&mut self, store_id: u64, ids: &[u64]) -> Result<()> {
        let db = self.database_id;
        let mut keys = Vec::new();
        for id in ids {
            keys.extend(self.disk.scan(&key_prefix(db, store_id, *id))?.into_iter().map(|(key, _)| key));
        }
        self.removals(keys, false)
    }

    /// As requisições que rodam depois do `upgradeneeded` (ou numa transação de leitura e escrita).
    fn queued(&mut self, entry: &Entry) -> Result<()> {
        let db = self.database_id;
        match entry {
            Entry::CreateIndex { store, name, records, .. } => {
                let store_id = self.store(store)?;
                let id = self.disk.index_id(db, store_id, name)?;
                for (primary, keys) in records {
                    let data = self.get(&record_key(db, store_id, record_index::OBJECT_STORE_DATA, primary))?.unwrap_or_default();
                    let version = read_varint(&data).unwrap_or(1);
                    let ops = keys.iter().map(|key| Op::Put(index_data_key(db, store_id, id, key, 0, primary), index_data_value(version, primary))).collect::<Vec<_>>();
                    if !ops.is_empty() {
                        self.request(ops)?;
                    }
                }
                Ok(())
            }
            Entry::Put { store, key, value, indexes, generator, generated } => {
                let store_id = self.store(store)?;
                let version = self.int(&database::object_store_field(db, store_id, store_field::LAST_VERSION))?.unwrap_or(1) + 1;
                let mut encoded_version = Vec::new();
                varint(&mut encoded_version, version);
                self.request(vec![
                    Op::Put(database::object_store_field(db, store_id, store_field::LAST_VERSION), encode_int(version)),
                    Op::Put(record_key(db, store_id, record_index::OBJECT_STORE_DATA, key), object_store_data_value(version, &v8::wire_value(value))),
                ])?;
                let mut ops = vec![Op::Put(record_key(db, store_id, record_index::EXISTS_ENTRY, key), encoded_version)];
                for (index, keys) in indexes {
                    let index_id = self.disk.index_id(db, store_id, index)?;
                    ops.extend(keys.iter().map(|index_key| Op::Put(index_data_key(db, store_id, index_id, index_key, 0, key), index_data_value(version, key))));
                }
                let auto_increment = self.get(&database::object_store_field(db, store_id, store_field::AUTO_INCREMENT))?.is_some_and(|flag| flag.first() == Some(&1));
                let stored = self.int(&database::object_store_field(db, store_id, store_field::KEY_GENERATOR_CURRENT_NUMBER))?.unwrap_or(1);
                let generator_key = database::object_store_field(db, store_id, store_field::KEY_GENERATOR_CURRENT_NUMBER);
                let bumped = auto_increment && *generator as u64 != stored;
                if bumped && *generated {
                    ops.push(Op::Put(generator_key.clone(), encode_int(*generator as u64)));
                }
                self.request(ops)?;
                // A chave explícita passa pela atualização do gerador, que só grava quando ele sobe; mesmo vazia,
                // ela fecha o lote anterior (o commit não se junta a ele).
                if auto_increment && !*generated {
                    self.flush_pending()?;
                    if bumped {
                        self.request(vec![Op::Put(generator_key, encode_int(*generator as u64))])?;
                    }
                }
                Ok(())
            }
            Entry::Delete { store, key } => {
                let store_id = self.store(store)?;
                self.request(vec![Op::Del(record_key(db, store_id, record_index::OBJECT_STORE_DATA, key)), Op::Del(record_key(db, store_id, record_index::EXISTS_ENTRY, key))])
            }
            Entry::Clear(store) => {
                let store_id = self.store(store)?;
                self.flush_pending()?;
                self.record_pairs_for_clear(store_id)
            }
            Entry::RenameStore { .. } | Entry::RenameIndex { .. } | Entry::CreateStore { .. } | Entry::DeleteStore(_) | Entry::DeleteIndex { .. } => Ok(()),
        }
    }

    /// `clear`: os registros (dados e depois existência) e as entradas dos índices, em dois lotes de remoções.
    fn record_pairs_for_clear(&mut self, store_id: u64) -> Result<()> {
        let ids = self.disk.index_ids(self.database_id, store_id)?;
        self.record_removals(store_id)?;
        self.index_entry_removals(store_id, &ids)
    }

    /// Os renomear que vieram em sequência saem num lote só: o nome do repositório (campo, nome novo, nome velho) e o
    /// nome do índice.
    fn rename_batch(&mut self, renames: &mut Vec<&Entry>) -> Result<()> {
        if renames.is_empty() {
            return Ok(());
        }
        let db = self.database_id;
        let mut ops = Vec::new();
        let mut renamed: Vec<(Vec<u16>, u64)> = Vec::new();
        for entry in renames.drain(..) {
            let known = |name: &[u16]| renamed.iter().rev().find(|(known, _)| known.as_slice() == name).map(|(_, id)| *id);
            match entry {
                Entry::RenameStore { from, to } => {
                    let id = match known(from) {
                        Some(id) => id,
                        None => self.store(from)?,
                    };
                    renamed.push((to.clone(), id));
                    ops.push(Op::Put(database::object_store_field(db, id, store_field::NAME), utf16_be(to)));
                    ops.push(Op::Put(database::object_store_name(db, to), encode_int(id)));
                    ops.push(Op::Del(database::object_store_name(db, from)));
                }
                Entry::RenameIndex { store, to, from } => {
                    let store_id = match known(store) {
                        Some(id) => id,
                        None => self.store(store)?,
                    };
                    let id = self.disk.index_id(db, store_id, from)?;
                    ops.push(Op::Put(database::index_field(db, store_id, id, index_field::NAME), utf16_be(to)));
                }
                _ => {}
            }
        }
        self.request(ops)
    }
}

/// O IndexedDB de um perfil: o leveldb de cada origem em `<raiz>/<origem>.indexeddb.leveldb`, aberto no primeiro uso
/// (é aí que o Chromium o cria ou grava o lote vazio da reabertura).
pub struct IndexedDbStore {
    env: Arc<dyn Env>,
    root: String,
    thread_id: u64,
    origins: std::sync::Mutex<HashMap<String, OriginDisk>>,
    /// O uso de cada origem nesta execução: o QuotaManager o soma ao bucket dela.
    uses: std::sync::Mutex<BTreeMap<String, BucketUse>>,
}

/// O que uma origem fez com o IndexedDB nesta execução (instantes em microssegundos WebKit).
#[derive(Debug, Clone, Copy)]
pub struct BucketUse {
    /// Dois por `indexedDB.open` (o Chromium abre o bucket duas vezes a cada abertura).
    pub use_count: i64,
    pub accessed: i64,
    pub modified: i64,
}

impl std::fmt::Debug for IndexedDbStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("IndexedDbStore").field("root", &self.root).finish_non_exhaustive()
    }
}

impl Default for IndexedDbStore {
    /// Sem `--user-data-dir` (e nos contextos de `Target.createBrowserContext`): os bancos vivem só na memória.
    fn default() -> Self {
        IndexedDbStore::new(Arc::new(ul_leveldb::env::MemEnv::new()), "/IndexedDB", 1)
    }
}

/// O identificador de armazenamento da origem (`http_127.0.0.1_8000`): esquema, host e porta, com a porta 0 quando é
/// a padrão do esquema.
pub fn origin_identifier(origin: &str) -> Option<String> {
    let (scheme, rest) = origin.split_once("://")?;
    let (host, port) = match rest.rsplit_once(':') {
        Some((host, port)) if !rest.ends_with(']') => (host, port),
        _ => (rest, "0"),
    };
    Some(format!("{scheme}_{}_{port}", host.replace(':', "_")))
}

impl IndexedDbStore {
    pub fn new(env: Arc<dyn Env>, root: &str, thread_id: u64) -> IndexedDbStore {
        IndexedDbStore { env, root: root.to_owned(), thread_id, origins: std::sync::Mutex::default(), uses: std::sync::Mutex::default() }
    }

    fn with_origin<T>(&self, origin: &str, work: impl FnOnce(&mut OriginDisk) -> Result<T>) -> Result<T> {
        let identifier = origin_identifier(origin).ok_or_else(|| io("origem sem IndexedDB"))?;
        let mut origins = self.origins.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !origins.contains_key(&identifier) {
            let dir = format!("{}/{identifier}.indexeddb.leveldb", self.root);
            let disk = OriginDisk::open(self.env.clone(), &dir, self.thread_id, &format!("{identifier}@1"))?;
            origins.insert(identifier.clone(), disk);
        }
        work(origins.get_mut(&identifier).expect("aberto acima"))
    }

    /// Anota o uso da origem: qualquer chamada do IndexedDB cria o bucket; `opens` soma as aberturas e `wrote` o instante
    /// da última gravação.
    fn note(&self, origin: &str, opens: i64, wrote: bool) {
        let now = ul_common::time::webkit_now_micros();
        let mut uses = self.uses.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = uses.entry(origin.to_owned()).or_insert(BucketUse { use_count: 0, accessed: now, modified: now });
        entry.use_count += 2 * opens;
        if opens > 0 {
            entry.accessed = now;
        }
        if wrote {
            entry.modified = now;
        }
    }

    /// Uma chamada de `indexedDB.open`.
    pub fn opened(&self, origin: &str) {
        self.note(origin, 1, false);
    }

    /// Os buckets que as origens usaram nesta execução, em ordem de origem.
    pub fn bucket_uses(&self) -> Vec<(String, BucketUse)> {
        self.uses.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().map(|(origin, usage)| (origin.clone(), *usage)).collect()
    }

    /// Os bancos da origem, no texto que o prelúdio lê. Um disco ilegível vale como nenhum banco.
    pub fn load(&self, origin: &str) -> Vec<(String, String)> {
        self.note(origin, 0, false);
        self.with_origin(origin, |disk| disk.load()).unwrap_or_default()
    }

    /// Grava o diário de uma transação que terminou.
    pub fn commit(&self, origin: &str, name: &str, text: &str) -> Result<()> {
        let mut commit = Commit::parse(text).ok_or_else(|| io("diário ilegível"))?;
        commit.name = units(name);
        self.note(origin, 0, true);
        self.with_origin(origin, |disk| disk.commit(&commit))
    }

    pub fn delete(&self, origin: &str, name: &str) -> Result<()> {
        self.with_origin(origin, |disk| disk.delete_database(&units(name)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_tasks_match_the_oracle_encoding() {
        // `0a 0a 0a 05 <chave 00 01 00 00 04> 12 01 00`: recolocar o valor 0 da versão (lote seq=8 de idb-basic).
        assert_eq!(undo_task(&[0, 1, 0, 0, 4], Some(&[0])), [0x0a, 0x0a, 0x0a, 0x05, 0, 1, 0, 0, 4, 0x12, 0x01, 0]);
        // `12 07 0a 05 <chave>`: apagar a chave que não existia (lote seq=11).
        assert_eq!(undo_task(&[0, 1, 0, 0, 3], None), [0x12, 0x07, 0x0a, 0x05, 0, 1, 0, 0, 3]);
    }

    #[test]
    fn scope_locks_match_the_oracle_encoding() {
        assert_eq!(database_lock(&units("db1")), [0x0a, 0x07, 0x12, 0x05, 0x0a, 0x03, b'd', b'b', b'1']);
        assert_eq!(store_lock(1, 1), [vec![0x0a, 0x16, 0x08, 0x01, 0x12, 0x12, 0x0a, 0x10], 1u64.to_be_bytes().to_vec(), 1u64.to_be_bytes().to_vec()].concat());
    }

    #[test]
    fn scope_keys_match_the_oracle() {
        assert_eq!(scope_metadata_key(0), [0, 0, 0, 0, 0x32, 1, 0]);
        assert_eq!(scope_task_key(2, 0, i64::MAX as u64), [vec![0, 0, 0, 0, 0x32, 2, 2, 0], 0x7fff_ffff_ffff_ffffu64.to_be_bytes().to_vec()].concat());
        assert_eq!(scope_task_key(2, 1, 0), [0, 0, 0, 0, 0x32, 2, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    }
}
