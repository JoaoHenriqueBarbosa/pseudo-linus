//! O rastreio de mudanças de uma área de armazenamento do navegador (o que o `StorageAreaImpl` do Chromium guarda
//! entre um commit e outro), igual no Local Storage e no Session Storage: o mapa que a página vê, as chaves que
//! mudaram desde o último commit e se um `clear()` precisa apagar antes o que o banco já tem.
//!
//! O que difere entre os dois é a codificação de chaves e valores e o formato do lote de commit; isso fica com cada
//! banco. Aqui as mudanças já chegam codificadas.

use std::collections::{BTreeMap, BTreeSet};

/// Uma mudança que a página fez na área da origem dela.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change<'a> {
    Set(&'a str, &'a str),
    Remove(&'a str),
    Clear,
}

impl Change<'_> {
    /// A mudança na codificação do banco: cada um tem a sua de chave e de valor.
    pub fn encode(self, key: impl Fn(&str) -> Vec<u8>, value: impl Fn(&str) -> Vec<u8>) -> Encoded {
        match self {
            Change::Set(k, v) => Encoded::Set(key(k), value(v)),
            Change::Remove(k) => Encoded::Remove(key(k)),
            Change::Clear => Encoded::Clear,
        }
    }
}

/// O banco que guarda as áreas de um contexto (o Local Storage do perfil, ou o Session Storage de uma aba): o que ele
/// tem de uma origem ao abri-la e o que a página muda depois.
pub trait AreaStore: std::fmt::Debug + Send + Sync {
    /// Abre a área da origem: as entradas que o banco tem, na ordem de chave (a ordem em que o Blink monta a tabela da
    /// página). Abrir conta como acesso.
    fn open_area(&self, origin: &str) -> Vec<(String, String)>;

    /// Anota uma mudança da página; ela vai ao disco na próxima gravação. O que não muda nada (gravar o valor que já
    /// está lá, remover o que não existe, limpar uma área vazia) não é mudança: o Blink nem avisa o navegador.
    fn record(&self, origin: &str, change: Change);
}

/// Uma mudança da página já codificada para o banco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Encoded {
    Set(Vec<u8>, Vec<u8>),
    Remove(Vec<u8>),
    Clear,
}

/// O estado de uma área: o mapa em ordem de chave (a ordem em que o banco o entrega ao carregar) e o que falta gravar.
#[derive(Debug, Default)]
pub struct Tracked {
    pub map: BTreeMap<Vec<u8>, Vec<u8>>,
    /// As chaves alteradas desde o último commit (as que ainda existem viram `put`, as outras `delete`).
    pub changed: BTreeSet<Vec<u8>>,
    /// Um `clear()` aconteceu desde o último commit: o commit apaga primeiro o que o banco tem da área.
    pub clear_all_first: bool,
}

impl Tracked {
    /// Há algo a gravar no próximo commit.
    pub fn has_changes(&self) -> bool {
        self.clear_all_first || !self.changed.is_empty()
    }

    /// Aplica a mudança da página; devolve `false` quando ela não muda nada (gravar o valor que já está lá, remover o
    /// que não existe, limpar uma área vazia): o Blink nem avisa o navegador.
    pub fn apply(&mut self, change: Encoded) -> bool {
        match change {
            Encoded::Set(key, value) => {
                if self.map.get(&key) == Some(&value) {
                    return false;
                }
                self.map.insert(key.clone(), value);
                self.changed.insert(key);
            }
            Encoded::Remove(key) => {
                if self.map.remove(&key).is_none() {
                    return false;
                }
                self.changed.insert(key);
            }
            Encoded::Clear => {
                if self.map.is_empty() {
                    return false;
                }
                self.map.clear();
                self.changed.clear();
                self.clear_all_first = true;
            }
        }
        true
    }

    /// O commit gravou o que estava pendente.
    pub fn settle(&mut self) {
        self.changed.clear();
        self.clear_all_first = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(key: &str, value: &str) -> Encoded {
        Encoded::Set(key.as_bytes().to_vec(), value.as_bytes().to_vec())
    }

    #[test]
    fn a_change_that_changes_nothing_is_not_a_change() {
        let mut area = Tracked::default();
        assert!(!area.apply(Encoded::Clear));
        assert!(!area.apply(Encoded::Remove(b"k".to_vec())));
        assert!(area.apply(set("k", "v")));
        assert!(!area.apply(set("k", "v")));
        assert!(area.apply(set("k", "w")));
    }

    #[test]
    fn clear_drops_what_was_changed_and_asks_the_commit_to_wipe_first() {
        let mut area = Tracked::default();
        area.apply(set("a", "1"));
        area.apply(Encoded::Clear);
        area.apply(set("b", "2"));
        assert!(area.clear_all_first);
        assert_eq!(area.changed.iter().map(Vec::as_slice).collect::<Vec<_>>(), [b"b".as_slice()]);
        area.settle();
        assert!(!area.has_changes());
    }
}
