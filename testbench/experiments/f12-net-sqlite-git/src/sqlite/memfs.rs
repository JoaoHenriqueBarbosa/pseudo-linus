//! Arquivos em memória compartilhados entre "processos" (threads), com a trava de arquivo no estilo do
//! SQLite (SHARED, RESERVED, PENDING, EXCLUSIVE). Faz o papel do tmpfs do kernel pros caminhos 2
//! (sqlite-plugin) e 3 (turso): os dois motores leem e escrevem aqui, nunca no disco do host.
//!
//! Cada execução de caso usa um prefixo próprio (`/ns<N>/`), pra que casos em sequência não se vejam.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use harness::{Entry, MemTree};

/// Estado da trava de um arquivo, por id de handle.
#[derive(Debug, Default)]
pub struct LockState {
    pub shared: BTreeSet<u64>,
    pub reserved: Option<u64>,
    pub pending: Option<u64>,
    pub exclusive: Option<u64>,
}

/// Nível de trava, na ordem do SQLite.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    None,
    Shared,
    Reserved,
    Pending,
    Exclusive,
}

#[derive(Debug, Default)]
pub struct Node {
    pub data: RwLock<Vec<u8>>,
    pub locks: Mutex<LockState>,
}

impl Node {
    /// Tenta subir a trava de `id` pra `level`. `false` quando outro handle impede (SQLITE_BUSY).
    pub fn lock(&self, id: u64, current: Level, level: Level) -> bool {
        let mut st = self.locks.lock().expect("trava");
        if level <= current {
            return true;
        }
        let other = |slot: Option<u64>| slot.is_some_and(|h| h != id);
        match level {
            Level::None => true,
            Level::Shared => {
                if other(st.pending) || other(st.exclusive) {
                    return false;
                }
                st.shared.insert(id);
                true
            }
            Level::Reserved => {
                if other(st.reserved) || other(st.pending) || other(st.exclusive) {
                    return false;
                }
                st.reserved = Some(id);
                true
            }
            Level::Pending | Level::Exclusive => {
                if other(st.pending) || other(st.exclusive) {
                    return false;
                }
                st.pending = Some(id);
                if level == Level::Pending {
                    return true;
                }
                if st.shared.iter().any(|&h| h != id) {
                    // Fica com PENDING (barra leitores novos) e devolve BUSY, como o os_unix.c.
                    return false;
                }
                st.exclusive = Some(id);
                true
            }
        }
    }

    /// Desce a trava de `id` pra `level` (SHARED ou NONE).
    pub fn unlock(&self, id: u64, level: Level) {
        let mut guard = self.locks.lock().expect("trava");
        let st = &mut *guard;
        if level <= Level::Shared {
            for slot in [&mut st.reserved, &mut st.pending, &mut st.exclusive] {
                if *slot == Some(id) {
                    *slot = None;
                }
            }
        }
        if level == Level::None {
            st.shared.remove(&id);
        }
    }

    /// Algum handle segura RESERVED ou mais.
    pub fn reserved_held(&self) -> bool {
        let st = self.locks.lock().expect("trava");
        st.reserved.is_some() || st.pending.is_some() || st.exclusive.is_some()
    }

    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let data = self.data.read().expect("dados");
        if offset >= data.len() {
            return 0;
        }
        let n = buf.len().min(data.len() - offset);
        buf[..n].copy_from_slice(&data[offset..offset + n]);
        n
    }

    pub fn write_at(&self, offset: usize, bytes: &[u8]) {
        let mut data = self.data.write().expect("dados");
        let end = offset + bytes.len();
        if data.len() < end {
            data.resize(end, 0);
        }
        data[offset..end].copy_from_slice(bytes);
    }

    pub fn truncate(&self, len: usize) {
        self.data.write().expect("dados").resize(len, 0);
    }

    pub fn len(&self) -> usize {
        self.data.read().expect("dados").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Default)]
pub struct SharedFs {
    files: Mutex<BTreeMap<String, Arc<Node>>>,
}

static GLOBAL: OnceLock<Arc<SharedFs>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Id novo pra handle ou namespace.
pub fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

impl SharedFs {
    /// O FS do processo: é o que o VFS registrado no SQLite enxerga.
    pub fn global() -> Arc<SharedFs> {
        GLOBAL.get_or_init(|| Arc::new(SharedFs::default())).clone()
    }

    pub fn get(&self, path: &str) -> Option<Arc<Node>> {
        self.files.lock().expect("fs").get(path).cloned()
    }

    pub fn get_or_create(&self, path: &str) -> Arc<Node> {
        self.files.lock().expect("fs").entry(path.to_string()).or_default().clone()
    }

    pub fn remove(&self, path: &str) -> bool {
        self.files.lock().expect("fs").remove(path).is_some()
    }

    pub fn exists(&self, path: &str) -> bool {
        self.files.lock().expect("fs").contains_key(path)
    }

    /// Copia os arquivos `names` (relativos ao caso) do `MemTree` pra dentro do namespace.
    pub fn load(&self, ns: &str, tree: &MemTree, names: &[String]) {
        for name in names {
            if let Some(data) = tree.read(name) {
                let node = self.get_or_create(&format!("{ns}/{name}"));
                *node.data.write().expect("dados") = data.to_vec();
            }
        }
    }

    /// Devolve pro `MemTree` o estado dos arquivos `names`: grava os que existem, apaga os que sumiram.
    pub fn store(&self, ns: &str, tree: &mut MemTree, names: &[String]) {
        for name in names {
            match self.get(&format!("{ns}/{name}")) {
                Some(node) => {
                    let data = node.data.read().expect("dados").clone();
                    crate::shell::write_file(tree, name, data);
                }
                None => {
                    if matches!(tree.get(name), Some(Entry::File { .. })) {
                        tree.entries.remove(name);
                    }
                }
            }
        }
    }

    /// Remove tudo abaixo de `ns`.
    pub fn clear(&self, ns: &str) {
        let prefix = format!("{ns}/");
        self.files.lock().expect("fs").retain(|k, _| !k.starts_with(&prefix));
    }

    /// Arquivos abaixo de `ns`, relativos a ele.
    pub fn list(&self, ns: &str) -> Vec<String> {
        let prefix = format!("{ns}/");
        self.files.lock().expect("fs").keys().filter_map(|k| k.strip_prefix(&prefix).map(str::to_string)).collect()
    }
}

/// Os arquivos de um banco que precisam ir e voltar entre o `MemTree` e o FS compartilhado.
pub fn db_family(db: &str) -> Vec<String> {
    vec![db.to_string(), format!("{db}-journal"), format!("{db}-wal"), format!("{db}-shm")]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_protocol() {
        let n = Node::default();
        assert!(n.lock(1, Level::None, Level::Shared));
        assert!(n.lock(2, Level::None, Level::Shared));
        assert!(n.lock(1, Level::Shared, Level::Reserved));
        assert!(!n.lock(2, Level::Shared, Level::Reserved), "só um RESERVED");
        assert!(!n.lock(1, Level::Reserved, Level::Exclusive), "2 ainda lê");
        assert!(!n.lock(3, Level::None, Level::Shared), "PENDING barra leitor novo");
        n.unlock(2, Level::None);
        assert!(n.lock(1, Level::Pending, Level::Exclusive));
        assert!(n.reserved_held());
        n.unlock(1, Level::None);
        assert!(!n.reserved_held());
        assert!(n.lock(3, Level::None, Level::Shared));
    }
}
