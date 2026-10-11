//! O Local Storage do navegador sobre o leveldb: o lado do navegador do Chromium (`LocalStorageImpl` e o
//! `StorageAreaImpl` de cada origem), com o formato de chaves e valores que ele grava em
//! `<perfil>/Default/Local Storage/leveldb` (medido no oráculo, ver `testbench/corpus/headless-storage/ls-*.html`):
//!
//! - `VERSION` => `1`, gravado quando o banco nasce;
//! - `_<origem>\0<chave>` => `<valor>`, com o byte de formato na frente de cada texto (1 = Latin1, um byte por
//!   caractere; 0 = UTF-16 little endian, quando algum caractere passa de U+00FF);
//! - `META:<origem>` => `{last_modified, size_bytes}` e `METAACCESS:<origem>` => `{last_accessed}`, mensagens
//!   protobuf com o tempo no formato do WebKit (microssegundos desde 1601) e o tamanho somando chave e valor
//!   codificados de todas as entradas da origem.
//!
//! As mudanças não vão ao disco na hora: acumulam por origem e o commit (`flush`) as grava num lote só, como o
//! `StorageAreaImpl` faz depois do atraso de commit ou ao desligar: primeiro as chaves apagadas por `clear()` (pelo
//! prefixo, no que o banco tem), depois as chaves alteradas que existem (em ordem de chave), as apagadas, e por fim os
//! metadados (`METAACCESS` e `META`, ou a remoção dos dois quando a origem ficou vazia).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ul_leveldb::batch::WriteBatch;
use ul_leveldb::coding::put_varint64;
use ul_leveldb::env::Env;
use ul_leveldb::{Db, Options, Result};

use crate::storage_state::{AreaStore, Change, Tracked};

/// Microssegundos de 1601 (a época do tempo WebKit) até 1970.
const WEBKIT_EPOCH_US: u64 = 11_644_473_600_000_000;
const SCHEMA_VERSION_KEY: &[u8] = b"VERSION";
const SCHEMA_VERSION: &[u8] = b"1";
const META_PREFIX: &[u8] = b"META:";
const META_ACCESS_PREFIX: &[u8] = b"METAACCESS:";
const FORMAT_UTF16: u8 = 0;
const FORMAT_LATIN1: u8 = 1;

/// O texto como o Chromium o grava: o byte de formato e os caracteres (Latin1 se todos cabem em um byte).
fn encode(text: &str) -> Vec<u8> {
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.iter().all(|&unit| unit < 0x100) {
        let mut out = vec![FORMAT_LATIN1];
        out.extend(units.iter().map(|&unit| unit as u8));
        out
    } else {
        let mut out = vec![FORMAT_UTF16];
        out.extend(units.iter().flat_map(|unit| unit.to_le_bytes()));
        out
    }
}

fn decode(bytes: &[u8]) -> Option<String> {
    let (&format, rest) = bytes.split_first()?;
    match format {
        FORMAT_LATIN1 => Some(rest.iter().map(|&byte| char::from(byte)).collect()),
        FORMAT_UTF16 if rest.len() % 2 == 0 => Some(String::from_utf16_lossy(&rest.chunks(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect::<Vec<u16>>())),
        _ => None,
    }
}

/// `_<origem>\0`: o prefixo das chaves de uma origem.
fn area_prefix(origin: &str) -> Vec<u8> {
    [b"_".as_slice(), origin.as_bytes(), b"\0"].concat()
}

fn meta_key(prefix: &[u8], origin: &str) -> Vec<u8> {
    [prefix, origin.as_bytes()].concat()
}

/// A mensagem protobuf com o campo 1 (`last_accessed`) e, quando há, o campo 2 (`size_bytes`).
fn meta_value(time: u64, size: Option<u64>) -> Vec<u8> {
    let mut out = vec![0x08];
    put_varint64(&mut out, time);
    if let Some(size) = size {
        out.push(0x10);
        put_varint64(&mut out, size);
    }
    out
}

/// A origem como o navegador a vê: o que a página tem agora e o que ainda não foi ao disco.
#[derive(Default)]
struct Area {
    /// Chave e valor codificados (sem o prefixo da origem), em ordem de chave: a ordem em que o banco os entrega ao
    /// carregar a origem, e o que falta gravar.
    tracked: Tracked,
    /// A origem foi aberta nesta execução (o commit atualiza `METAACCESS`).
    accessed: bool,
    /// Desde quando há algo a gravar: a abertura da origem, ou a primeira mudança depois dela (o atraso de commit
    /// conta a partir daí).
    pending_since: Option<u64>,
}

/// O atraso entre a primeira mudança de uma origem e o commit dela (`kCommitDefaultDelaySecs` do Local Storage).
const COMMIT_DELAY_US: u64 = 5_000_000;

struct Inner {
    db: Db,
    areas: HashMap<String, Area>,
    /// As origens na ordem do primeiro acesso: a dos commits.
    order: Vec<String>,
}

impl Inner {
    /// A área da origem, carregada do banco na primeira vez (`now` é o instante da abertura).
    fn area(&mut self, origin: &str, now: u64) -> &mut Area {
        if !self.areas.contains_key(origin) {
            let prefix = area_prefix(origin);
            let map = self
                .db
                .entries()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|(key, value)| Some((key.strip_prefix(prefix.as_slice())?.to_vec(), value)))
                .collect();
            self.areas.insert(origin.to_owned(), Area { tracked: Tracked { map, ..Tracked::default() }, accessed: true, pending_since: Some(now), ..Area::default() });
            self.order.push(origin.to_owned());
        }
        self.areas.get_mut(origin).expect("a área foi carregada acima")
    }

    /// O lote de commit de uma origem (`StorageAreaImpl::CommitChanges` mais `PrepareToCommit` do Local Storage), ou
    /// `None` quando não há nada a gravar.
    fn commit_batch(&mut self, origin: &str, now: u64) -> Option<WriteBatch> {
        let prefix = area_prefix(origin);
        let persisted: Vec<Vec<u8>> = match self.areas.get(origin) {
            Some(area) if area.tracked.clear_all_first => self.db.entries().unwrap_or_default().into_iter().map(|(key, _)| key).filter(|key| key.starts_with(&prefix)).collect(),
            _ => Vec::new(),
        };
        let area = self.areas.get_mut(origin)?;
        let has_changes = area.tracked.has_changes();
        let Tracked { map, changed, .. } = &area.tracked;
        area.pending_since = None;
        if !has_changes && (!area.accessed || map.is_empty()) {
            area.accessed = false;
            return None;
        }
        let mut batch = WriteBatch::new();
        for key in &persisted {
            batch.delete(key);
        }
        let full = |key: &[u8]| [prefix.as_slice(), key].concat();
        for key in changed {
            if let Some(value) = map.get(key) {
                batch.put(&full(key), value);
            }
        }
        for key in changed.iter().filter(|key| !map.contains_key(*key)) {
            batch.delete(&full(key));
        }
        let (access_key, modified_key) = (meta_key(META_ACCESS_PREFIX, origin), meta_key(META_PREFIX, origin));
        let time = now + WEBKIT_EPOCH_US;
        if map.is_empty() {
            batch.delete(&access_key);
            batch.delete(&modified_key);
        } else {
            batch.put(&access_key, &meta_value(time, None));
            if has_changes {
                let size: usize = map.iter().map(|(key, value)| key.len() + value.len()).sum();
                batch.put(&modified_key, &meta_value(time, Some(size as u64)));
            }
        }
        area.tracked.settle();
        area.accessed = false;
        Some(batch)
    }
}

/// O banco do Local Storage de um perfil, compartilhado pelas abas.
pub struct LocalStorageStore {
    env: Arc<dyn Env>,
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for LocalStorageStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("LocalStorageStore").finish_non_exhaustive()
    }
}

impl LocalStorageStore {
    /// Abre (ou cria) o banco em `dir`; um banco novo recebe o `VERSION`.
    pub fn open(env: Arc<dyn Env>, dir: &str, thread_id: u64) -> Result<LocalStorageStore> {
        let mut db = Db::open(env.clone(), dir, Options::chromium(thread_id))?;
        if db.get(SCHEMA_VERSION_KEY)?.is_none() {
            db.put(SCHEMA_VERSION_KEY, SCHEMA_VERSION)?;
        }
        Ok(LocalStorageStore { env, inner: Mutex::new(Inner { db, areas: HashMap::new(), order: Vec::new() }) })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// O commit: grava o que está pendente, um lote por origem, na ordem dos primeiros acessos.
    pub fn flush(&self) -> Result<()> {
        self.commit(|_| true)
    }

    /// O commit das origens cujo atraso de 5 segundos já passou: o que o navegador faz sozinho enquanto está de pé.
    pub fn flush_due(&self) -> Result<()> {
        let now = self.env.now_micros();
        self.commit(|since| now.saturating_sub(since) >= COMMIT_DELAY_US)
    }

    fn commit(&self, due: impl Fn(u64) -> bool) -> Result<()> {
        let mut inner = self.lock();
        for origin in inner.order.clone() {
            let ready = inner.areas.get(&origin).and_then(|area| area.pending_since).is_some_and(&due);
            if !ready {
                continue;
            }
            let now = self.env.now_micros();
            if let Some(batch) = inner.commit_batch(&origin, now) {
                inner.db.write(batch)?;
            }
        }
        Ok(())
    }
}

impl AreaStore for LocalStorageStore {
    /// Abrir a área conta como acesso: o commit seguinte atualiza `METAACCESS`.
    fn open_area(&self, origin: &str) -> Vec<(String, String)> {
        let now = self.env.now_micros();
        let mut inner = self.lock();
        let area = inner.area(origin, now);
        area.tracked.map.iter().filter_map(|(key, value)| Some((decode(key)?, decode(value)?))).collect()
    }

    fn record(&self, origin: &str, change: Change) {
        let now = self.env.now_micros();
        let mut inner = self.lock();
        let area = inner.area(origin, now);
        let first_change = !area.tracked.has_changes();
        let change = change.encode(encode, encode);
        if area.tracked.apply(change) && first_change {
            area.pending_since = Some(now);
        }
    }
}
