//! O Session Storage do navegador sobre o leveldb: o lado do navegador do Chromium (`SessionStorageImpl`), com o formato
//! de chaves e valores e o ciclo de vida que ele tem em `<perfil>/Default/Session Storage` (medido no oráculo, ver
//! `testbench/oracle/measure-session-storage.sh` e `testbench/corpus/headless-storage/ss-*.html`):
//!
//! - `version` => `1`, gravado quando o banco nasce; o banco é apagado e recriado a cada início do navegador;
//! - `namespace-<guid>-<origem>/` => `<id do mapa>` (em ASCII), com o GUID da aba (`-` vira `_`);
//! - `map-<id>-<chave>` => `<valor>`, a chave em UTF-8 e o valor em UTF-16 little endian puro, sem o byte de formato do
//!   Local Storage.
//!
//! O id do mapa é o próximo número da execução e nasce na abertura da área (ler já o consome); as mudanças acumulam e o
//! commit as grava num lote só: por área, na ordem de abertura, o `namespace` (na primeira vez), as chaves alteradas que
//! existem e as apagadas. Com o navegador de pé o commit sai 5 segundos depois da primeira mudança; senão, no
//! desligamento, que ainda grava lotes vazios (e, no `--dump-dom`, apaga tudo) conforme a forma de sair.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ul_leveldb::batch::WriteBatch;
use ul_leveldb::env::Env;
use ul_leveldb::{Db, Options, Result};

use crate::storage_state::{AreaStore, Change, Tracked};

const SCHEMA_VERSION_KEY: &[u8] = b"version";
const SCHEMA_VERSION: &[u8] = b"1";
const NAMESPACE_PREFIX: &[u8] = b"namespace-";
const MAP_PREFIX: &[u8] = b"map-";
/// O atraso entre a primeira mudança e o commit (`kCommitDefaultDelaySecs` do Session Storage).
const COMMIT_DELAY_US: u64 = 5_000_000;

/// Como o navegador sai, o que decide os lotes que o Session Storage ainda grava (medidos no oráculo com e sem dados).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// SIGTERM, SIGINT ou SIGHUP: só o commit do que está pendente.
    Signal,
    /// `Browser.close`: o commit e mais dois lotes vazios.
    Close,
    /// O `--dump-dom`: o commit, três lotes vazios e a remoção de tudo que o banco tem (ou um lote vazio, sem nada).
    DumpDom,
}

impl Exit {
    /// Os lotes vazios que sobram depois do commit.
    fn empty_batches(self) -> usize {
        match self {
            Exit::Signal => 0,
            Exit::Close => 2,
            Exit::DumpDom => 3,
        }
    }
}

fn namespace_key(guid: &str, origin: &str) -> Vec<u8> {
    [NAMESPACE_PREFIX, guid.as_bytes(), b"-", origin.as_bytes(), b"/"].concat()
}

fn map_prefix(id: u64) -> Vec<u8> {
    [MAP_PREFIX, id.to_string().as_bytes(), b"-"].concat()
}

fn encode_value(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn decode_value(bytes: &[u8]) -> Option<String> {
    (bytes.len() % 2 == 0).then(|| String::from_utf16_lossy(&bytes.chunks(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect::<Vec<u16>>()))
}

/// A área de uma origem numa aba: o que a página vê e o que ainda não foi ao disco.
struct Area {
    guid: String,
    origin: String,
    id: u64,
    /// Chave (UTF-8) e valor (UTF-16) sem o prefixo do mapa, em ordem de chave.
    tracked: Tracked,
    /// O `namespace` desta área já está no banco.
    registered: bool,
}

struct Inner {
    db: Db,
    next_map_id: u64,
    /// As áreas na ordem de abertura: a dos commits.
    areas: Vec<Area>,
    index: HashMap<(String, String), usize>,
    /// Desde quando há mudança pendente: o atraso de commit conta a partir daí.
    pending_since: Option<u64>,
}

impl Inner {
    fn area(&mut self, guid: &str, origin: &str) -> &mut Area {
        let key = (guid.to_owned(), origin.to_owned());
        let slot = match self.index.get(&key) {
            Some(&slot) => slot,
            None => {
                let id = self.next_map_id;
                self.next_map_id += 1;
                self.areas.push(Area { guid: key.0.clone(), origin: key.1.clone(), id, tracked: Tracked::default(), registered: false });
                self.index.insert(key, self.areas.len() - 1);
                self.areas.len() - 1
            }
        };
        &mut self.areas[slot]
    }

    /// O lote de commit de todas as áreas com mudança, ou `None` quando não há nada a gravar.
    fn commit_batch(&mut self) -> Option<WriteBatch> {
        self.pending_since = None;
        if !self.areas.iter().any(|area| area.tracked.has_changes()) {
            return None;
        }
        let on_disk: Vec<Vec<u8>> = if self.areas.iter().any(|area| area.tracked.clear_all_first) { self.db.entries().unwrap_or_default().into_iter().map(|(key, _)| key).collect() } else { Vec::new() };
        let mut batch = WriteBatch::new();
        for area in self.areas.iter_mut().filter(|area| area.tracked.has_changes()) {
            let prefix = map_prefix(area.id);
            if area.tracked.clear_all_first {
                for key in on_disk.iter().filter(|key| key.starts_with(&prefix)) {
                    batch.delete(key);
                }
            }
            if !area.registered {
                batch.put(&namespace_key(&area.guid.replace('-', "_"), &area.origin), area.id.to_string().as_bytes());
                area.registered = true;
            }
            let Tracked { map, changed, .. } = &area.tracked;
            let full = |key: &[u8]| [prefix.as_slice(), key].concat();
            for key in changed {
                if let Some(value) = map.get(key) {
                    batch.put(&full(key), value);
                }
            }
            for key in changed.iter().filter(|key| !map.contains_key(*key)) {
                batch.delete(&full(key));
            }
            area.tracked.settle();
        }
        Some(batch)
    }

    fn commit(&mut self) -> Result<()> {
        match self.commit_batch() {
            Some(batch) => self.db.write(batch),
            None => Ok(()),
        }
    }

    /// A remoção que o `--dump-dom` faz ao sair: os `namespace` e depois os mapas, no que o banco tem.
    fn wipe_batch(&self) -> WriteBatch {
        let keys: Vec<Vec<u8>> = self.db.entries().unwrap_or_default().into_iter().map(|(key, _)| key).collect();
        let mut batch = WriteBatch::new();
        for prefix in [NAMESPACE_PREFIX, MAP_PREFIX] {
            for key in keys.iter().filter(|key| key.starts_with(prefix)) {
                batch.delete(key);
            }
        }
        batch
    }
}

/// O banco do Session Storage de um perfil, compartilhado pelas abas (cada uma com o seu `namespace`).
pub struct SessionStorageStore {
    env: Arc<dyn Env>,
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for SessionStorageStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("SessionStorageStore").finish_non_exhaustive()
    }
}

impl SessionStorageStore {
    /// Cria o banco em `dir` (quem chama já apagou o da execução anterior): o `version` e o lote vazio que o Chromium
    /// grava ao abrir.
    pub fn open(env: Arc<dyn Env>, dir: &str, thread_id: u64) -> Result<SessionStorageStore> {
        let mut db = Db::open(env.clone(), dir, Options::chromium(thread_id))?;
        db.put(SCHEMA_VERSION_KEY, SCHEMA_VERSION)?;
        db.write(WriteBatch::new())?;
        Ok(SessionStorageStore { env, inner: Mutex::new(Inner { db, next_map_id: 0, areas: Vec::new(), index: HashMap::new(), pending_since: None }) })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// O `namespace` de uma aba nova (GUID aleatório).
    pub fn namespace(self: &Arc<Self>) -> Arc<SessionNamespace> {
        Arc::new(SessionNamespace { store: self.clone(), guid: crate::devtools::uuid() })
    }

    /// O commit das áreas cujo atraso de 5 segundos já passou: o que o navegador faz sozinho enquanto está de pé.
    pub fn flush_due(&self) -> Result<()> {
        let now = self.env.now_micros();
        let mut inner = self.lock();
        match inner.pending_since {
            Some(since) if now.saturating_sub(since) >= COMMIT_DELAY_US => inner.commit(),
            _ => Ok(()),
        }
    }

    /// A saída do navegador: o commit do que está pendente e o que a forma de sair ainda grava.
    pub fn finish(&self, exit: Exit) -> Result<()> {
        let mut inner = self.lock();
        inner.commit()?;
        for _ in 0..exit.empty_batches() {
            inner.db.write(WriteBatch::new())?;
        }
        if exit == Exit::DumpDom {
            let batch = inner.wipe_batch();
            inner.db.write(batch)?;
        }
        Ok(())
    }
}

/// O Session Storage de uma aba: as áreas dela no banco compartilhado.
pub struct SessionNamespace {
    store: Arc<SessionStorageStore>,
    guid: String,
}

impl std::fmt::Debug for SessionNamespace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("SessionNamespace").finish_non_exhaustive()
    }
}

impl AreaStore for SessionNamespace {
    /// Abrir a área (ler já conta) dá a ela o próximo id de mapa; uma área nova não tem o que carregar.
    fn open_area(&self, origin: &str) -> Vec<(String, String)> {
        let mut inner = self.store.lock();
        let area = inner.area(&self.guid, origin);
        area.tracked.map.iter().filter_map(|(key, value)| Some((String::from_utf8(key.clone()).ok()?, decode_value(value)?))).collect()
    }

    fn record(&self, origin: &str, change: Change) {
        let now = self.store.env.now_micros();
        let mut inner = self.store.lock();
        let change = change.encode(|key| key.as_bytes().to_vec(), encode_value);
        if inner.area(&self.guid, origin).tracked.apply(change) {
            inner.pending_since.get_or_insert(now);
        }
    }
}
