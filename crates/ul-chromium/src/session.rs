//! O que vive fora do documento e sobrevive à troca dele, como o navegador guarda fora do renderer: o perfil (o
//! `localStorage` de cada origem, compartilhado pelas abas do mesmo contexto de navegação, cada uma na sua thread) e a
//! aba (o `sessionStorage` de cada origem, que só ela vê).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ul_common::hash::rapidhash::{wtf_string_hash, DEFAULT_SEED};

use crate::disk_profile::DiskProfile;
use crate::idb::disk::IndexedDbStore;
use crate::local_storage::LocalStorageStore;
use crate::session_storage::SessionStorageStore;
use crate::storage_state::AreaStore;

/// Um balde da tabela.
#[derive(Debug, Clone)]
enum Slot {
    Empty,
    Deleted,
    Full(String, String),
}

/// Uma área de armazenamento (o `Storage` de uma origem), na ordem que a página vê em `key(i)`, `Object.keys` e
/// `for..in`: a dos baldes do `HashMap<String, String>` do `StorageAreaMap` do Blink, medida no oráculo. A chave vai ao
/// balde pelo hash de texto do WTF do Blink (o rapidhash com a semente padrão); a tabela nasce com 8 baldes, dobra
/// quando chaves e apagados chegam à metade (ou só se reorganiza, se as chaves vivas não passam de um sexto do dobro),
/// cai à metade quando as chaves ficam abaixo de um sexto, colide em sondagem triangular e reusa o primeiro balde
/// apagado do caminho.
#[derive(Debug, Default, Clone)]
pub struct StorageArea {
    slots: Vec<Slot>,
    count: usize,
    deleted: usize,
}

const MINIMUM_TABLE_SIZE: usize = 8;
const MAX_LOAD: usize = 2;
const MIN_LOAD: usize = 6;

fn key_hash(key: &str) -> usize {
    wtf_string_hash(DEFAULT_SEED, &key.encode_utf16().collect::<Vec<u16>>()) as usize
}

impl StorageArea {
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn entries(&self) -> impl Iterator<Item = (&String, &String)> {
        self.slots.iter().filter_map(|slot| match slot {
            Slot::Full(key, value) => Some((key, value)),
            _ => None,
        })
    }

    /// Os baldes que a busca de `hash` visita, na ordem da sondagem triangular.
    fn probe(&self, hash: usize) -> impl Iterator<Item = usize> {
        let mask = self.slots.len() - 1;
        (0..self.slots.len()).scan(hash & mask, move |index, step| {
            let current = *index;
            *index = (*index + step + 1) & mask;
            Some(current)
        })
    }

    fn find(&self, key: &str) -> Option<usize> {
        if self.slots.is_empty() {
            return None;
        }
        for index in self.probe(key_hash(key)) {
            match &self.slots[index] {
                Slot::Empty => return None,
                Slot::Full(existing, _) if existing == key => return Some(index),
                _ => {}
            }
        }
        None
    }

    fn rehash(&mut self, size: usize) {
        let old = std::mem::replace(&mut self.slots, vec![Slot::Empty; size]);
        self.deleted = 0;
        for slot in old {
            if let Slot::Full(key, value) = slot {
                let index = self.probe(key_hash(&key)).find(|&index| matches!(self.slots[index], Slot::Empty)).expect("balde livre");
                self.slots[index] = Slot::Full(key, value);
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.find(key).and_then(|index| match &self.slots[index] {
            Slot::Full(_, value) => Some(value.as_str()),
            _ => None,
        })
    }

    pub fn key(&self, index: usize) -> Option<&str> {
        self.entries().nth(index).map(|(key, _)| key.as_str())
    }

    pub fn keys(&self) -> Vec<String> {
        self.entries().map(|(key, _)| key.clone()).collect()
    }

    /// Grava `value` em `key`; a chave que já existia fica no balde dela.
    pub fn set(&mut self, key: &str, value: &str) {
        if let Some(index) = self.find(key) {
            self.slots[index] = Slot::Full(key.to_owned(), value.to_owned());
            return;
        }
        if self.slots.is_empty() {
            self.rehash(MINIMUM_TABLE_SIZE);
        }
        let mut target = None;
        let mut reused = false;
        for index in self.probe(key_hash(key)) {
            match self.slots[index] {
                Slot::Deleted if target.is_none() => {
                    target = Some(index);
                    reused = true;
                }
                Slot::Empty => {
                    target.get_or_insert(index);
                    break;
                }
                _ => {}
            }
        }
        let index = target.expect("balde livre");
        if reused {
            self.deleted -= 1;
        }
        self.slots[index] = Slot::Full(key.to_owned(), value.to_owned());
        self.count += 1;
        if (self.count + self.deleted) * MAX_LOAD >= self.slots.len() {
            let size = if self.count * MIN_LOAD < self.slots.len() * 2 { self.slots.len() } else { self.slots.len() * 2 };
            self.rehash(size);
        }
    }

    pub fn remove(&mut self, key: &str) {
        let Some(index) = self.find(key) else { return };
        self.slots[index] = Slot::Deleted;
        self.count -= 1;
        self.deleted += 1;
        if self.count * MIN_LOAD < self.slots.len() && self.slots.len() > MINIMUM_TABLE_SIZE {
            self.rehash(self.slots.len() / 2);
        }
    }

    /// `clear()` solta a tabela inteira.
    pub fn clear(&mut self) {
        *self = StorageArea::default();
    }
}

/// As áreas de cada origem (serializada, como `location.origin`). No contexto padrão de um navegador com
/// `--user-data-dir`, a primeira vez que uma origem é aberta a tabela é montada com o que o banco do perfil tem, na
/// ordem de chave dele (é nessa ordem que o Blink insere o que o navegador entrega).
#[derive(Debug, Default)]
pub struct Areas {
    map: HashMap<String, StorageArea>,
    store: Option<Arc<dyn AreaStore>>,
}

impl Areas {
    pub fn area(&mut self, origin: &str) -> &mut StorageArea {
        let store = &self.store;
        self.map.entry(origin.to_owned()).or_insert_with(|| {
            let mut area = StorageArea::default();
            if let Some(store) = store {
                for (key, value) in store.open_area(origin) {
                    area.set(&key, &value);
                }
            }
            area
        })
    }

    /// O banco que guarda estas áreas, se o perfil é o de um navegador com `--user-data-dir`.
    pub fn store(&self) -> Option<Arc<dyn AreaStore>> {
        self.store.clone()
    }
}

/// O perfil de um contexto de navegação: o contexto padrão do navegador, ou um criado por
/// `Target.createBrowserContext` (que nasce vazio e não vê o do padrão).
#[derive(Debug, Default)]
pub struct Profile {
    pub local: Mutex<Areas>,
    /// O IndexedDB do contexto: no leveldb de cada origem, como o Chromium o grava.
    pub indexed_db: Arc<IndexedDbStore>,
    local_storage: Option<Arc<LocalStorageStore>>,
    /// O banco do `sessionStorage`: cada aba ganha o `namespace` dela nele.
    session_storage: Option<Arc<SessionStorageStore>>,
    /// O History do navegador de pé: a navegação da aba entra no banco 10 segundos depois ou no `Browser.close`.
    pub history: Option<Arc<crate::history::HistoryLog>>,
    /// Os arquivos de sessão do navegador de pé: a página da linha de comando e o `Tabs_` do `Browser.close`.
    pub sessions: Option<Arc<crate::sessions::SessionsLog>>,
    /// As navegações e respostas que as abas anotam para a saída (History, UKM, cache de disco, engajamento).
    pub visits: Option<Arc<crate::disk_profile::Visits>>,
    /// O pote de cookies do contexto: as abas o leem e o escrevem (`document.cookie`, `Set-Cookie`, `Storage.*Cookies`).
    pub cookies: Mutex<ul_net::net::cookies::Jar>,
    /// O que o DevTools concedeu (`Browser.grantPermissions`): origem (vazia vale para todas) e nome da permissão.
    pub permissions: Mutex<Vec<(String, String)>>,
    /// As origens de que o navegador já pediu o `/favicon.ico` (a resposta, mesmo a 404, fica guardada e ele não a repete).
    pub favicons: Mutex<Vec<String>>,
}

impl Profile {
    /// O perfil do `--user-data-dir`: o `localStorage` e o IndexedDB persistem nos bancos dele.
    pub fn on_disk(local_storage: Option<Arc<LocalStorageStore>>, indexed_db: Option<Arc<IndexedDbStore>>) -> Profile {
        let store = local_storage.clone().map(|store| store as Arc<dyn AreaStore>);
        Profile { local: Mutex::new(Areas { map: HashMap::new(), store }), indexed_db: indexed_db.unwrap_or_default(), local_storage, session_storage: None, history: None, sessions: None, visits: None, cookies: Mutex::default(), permissions: Mutex::default(), favicons: Mutex::default() }
    }

    /// O perfil com todos os bancos do `DiskProfile`, o `sessionStorage` incluído.
    pub fn from_disk(disk: &DiskProfile) -> Profile {
        Profile { session_storage: disk.session_storage(), history: disk.history_log(), sessions: disk.sessions_log(), visits: Some(disk.visits.clone()), ..Profile::on_disk(disk.local_storage(), disk.indexed_db()) }
    }

    /// Algum banco do perfil grava o que as páginas guardam.
    pub fn persists(&self) -> bool {
        self.local_storage.is_some() || self.session_storage.is_some()
    }

    /// Grava o que passou do atraso de commit: o que o navegador faz sozinho enquanto está de pé.
    pub fn flush_due(&self) {
        if let Some(store) = &self.local_storage {
            let _ = store.flush_due();
        }
        if let Some(store) = &self.session_storage {
            let _ = store.flush_due();
        }
    }
}

/// Uma aba: o perfil do contexto dela e o `sessionStorage`, que atravessa as navegações da aba.
#[derive(Debug, Default)]
pub struct Tab {
    pub profile: Arc<Profile>,
    pub session: RefCell<Areas>,
    /// Quem atende `alert`, `confirm` e `prompt` (o DevTools da aba); sem ele os diálogos não aparecem para ninguém.
    pub dialogs: RefCell<Option<DialogHook>>,
}

/// O atendimento de um diálogo do documento: tipo, mensagem e texto padrão; `None` é o diálogo recusado.
pub struct DialogHook(pub std::rc::Rc<dyn Fn(&str, &str, &str) -> Option<String>>);

impl std::fmt::Debug for DialogHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DialogHook")
    }
}

impl Tab {
    pub fn new(profile: Arc<Profile>) -> Tab {
        let store = profile.session_storage.as_ref().map(|store| store.namespace() as Arc<dyn AreaStore>);
        Tab { profile, session: RefCell::new(Areas { map: HashMap::new(), store }), dialogs: RefCell::new(None) }
    }
}

#[cfg(test)]
mod tests {
    use super::StorageArea;

    enum Op<'a> {
        Set(&'a str),
        Remove(&'a str),
    }

    fn order(ops: impl IntoIterator<Item = Op<'static>>) -> String {
        let mut area = StorageArea::default();
        for op in ops {
            match op {
                Op::Set(key) => area.set(key, "v"),
                Op::Remove(key) => area.remove(key),
            }
        }
        area.keys().join(" ")
    }

    fn sets(keys: &[&'static str]) -> Vec<Op<'static>> {
        keys.iter().map(|key| Op::Set(key)).collect()
    }

    fn numbered(prefix: &str, range: std::ops::Range<usize>) -> Vec<&'static str> {
        range.map(|i| &*Box::leak(format!("{prefix}{i}").into_boxed_str())).collect()
    }

    /// `ls-order.html` no oráculo: quarenta chaves gravadas na ordem `(i * 7) % 40` saem de `Object.keys` na ordem da
    /// primeira execução; na seguinte o navegador carrega o banco (chaves em ordem de bytes) e a tabela é montada
    /// inserindo nessa ordem, o que dá outra ordem de `Object.keys`.
    #[test]
    fn a_reloaded_area_is_built_in_the_databases_key_order() {
        let written: Vec<String> = (0..40).map(|i| format!("key{}", (i * 7) % 40)).collect();
        let mut first = StorageArea::default();
        for key in &written {
            first.set(key, "v");
        }
        assert_eq!(
            first.keys().join(","),
            "key21,key24,key11,key31,key38,key14,key7,key12,key19,key0,key2,key16,key26,key23,key18,key35,key6,key39,key17,key10,key22,key8,key36,key33,key32,key3,key34,key15,key5,key25,key20,key9,key28,key27,key4,key1,key30,key29,key13,key37"
        );
        let mut by_bytes = written;
        by_bytes.sort();
        let mut reloaded = StorageArea::default();
        for key in &by_bytes {
            reloaded.set(key, "v");
        }
        assert_eq!(
            reloaded.keys().join(","),
            "key21,key24,key11,key31,key38,key14,key7,key12,key19,key0,key16,key2,key26,key18,key23,key35,key6,key39,key17,key10,key22,key33,key36,key32,key8,key3,key15,key34,key5,key25,key20,key9,key28,key27,key4,key1,key30,key29,key13,key37"
        );
    }

    /// A ordem de `localStorage.key(i)` no oráculo (`storage-order2.js` e `storage-order3.js` da medição): a ordem da
    /// inserção não importa, a regravação mantém o balde, a tabela cresce e encolhe, o balde apagado é reusado, e o
    /// texto fora do Latin1 entra em UTF-16.
    #[test]
    fn key_order_as_the_oracle() {
        assert_eq!(order(sets(&["b", "a", "c", "10", "2", "Z"])), "Z a 10 2 c b");
        assert_eq!(order(sets(&["Z", "2", "10", "c", "a", "b"])), "Z a 10 2 c b");
        assert_eq!(order(sets(&["b", "a", "c", "b"])), "c a b");
        assert_eq!(order(sets(&["b", "a", "c", "d"]).into_iter().chain([Op::Remove("a"), Op::Set("e")])), "c e d b");
        assert_eq!(order(sets(&numbered("k", 0..8))), "k7 k2 k3 k5 k4 k1 k6 k0");
        assert_eq!(order(sets(&numbered("k", 0..20))), "k7 k12 k8 k18 k5 k4 k13 k15 k6 k0 k10 k2 k19 k14 k3 k1 k16 k9 k11 k17");
        assert_eq!(
            order(sets(&numbered("k", 0..40))),
            "k21 k12 k37 k33 k24 k18 k15 k13 k20 k6 k36 k0 k28 k35 k14 k3 k26 k29 k23 k38 k16 k11 k17 k25 k7 k39 k32 k31 k8 k30 k5 k4 k27 k34 k22 k10 k2 k19 k1 k9"
        );
        assert_eq!(order(sets(&["a", "b"])), "a b");
        assert_eq!(order(sets(&["b", "a"])), "a b");
        let removed = || numbered("k", 0..36).into_iter().map(Op::Remove);
        assert_eq!(order(sets(&numbered("k", 0..40)).into_iter().chain(removed())), "k39 k38 k37 k36");
        assert_eq!(order(sets(&numbered("k", 0..40)).into_iter().chain(removed()).chain(sets(&numbered("n", 0..5)))), "n2 k39 n3 k37 n1 k38 n0 k36 n4");
        let odd = numbered("k", 0..10).into_iter().skip(1).step_by(2).map(Op::Remove);
        assert_eq!(order(sets(&numbered("k", 0..10)).into_iter().chain(odd).chain(sets(&numbered("m", 0..6)))), "m1 m2 m0 k2 m4 m5 k8 k4 k6 m3 k0");
        assert_eq!(order(sets(&["k0", "k1", "k2"]).into_iter().chain([Op::Remove("k1"), Op::Set("k1"), Op::Set("z")])), "k1 k2 z k0");
        assert_eq!(order(sets(&["ação", "é", "中文", "😀", "ü", "abc", "Ωmega", "\u{100}x"])), "ação abc Ωmega 😀 中文 ü Āx é");
        let long = ["a", "b", "c", "d", "e", "f", "g"].iter().zip([17, 33, 49, 100, 4, 8, 3]).map(|(c, n)| &*Box::leak(c.repeat(n).into_boxed_str())).collect::<Vec<_>>();
        let expected = [long[5], long[0], long[4], long[3], long[2], long[1], long[6]].join(" ");
        assert_eq!(order(sets(&long)), expected);
        assert_eq!(order(sets(&["", "x", "y"])), " x y");
    }
}
