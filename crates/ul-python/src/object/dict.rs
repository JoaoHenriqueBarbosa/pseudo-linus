//! `dict` (`Objects/dictobject.c`): ordem de inserção, igualdade de chaves pelo protocolo do Python
//! (`1`, `1.0` e `True` são a mesma chave) e o `repr` com a proteção de recursão.
//!
//! A ordem de iteração de um `dict` é a de inserção e não depende do hash, então a tabela interna é
//! própria: um vetor de entradas na ordem de inserção (removidas viram buraco até a compactação) e um
//! índice de hash para posições. Só a ordem é observável; a disposição da tabela do CPython não é.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{hash, is, py_eq, repr_into, ObjError, ReprStack, Value};

/// Contador global das mutações: cada `set`/`remove` dá ao dict um número novo, que as visões vivas
/// das globais (`globalsview`) comparam para saber se houve escrita desde a última sincronização.
static GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Default)]
pub struct Dict {
    /// Número da última mutação (ver `GENERATION`); `0` num dict que nunca foi alterado.
    pub generation: u64,
    /// Entradas na ordem de inserção: hash, chave e valor. `None` é entrada removida.
    entries: Vec<Option<(i64, Value, Value)>>,
    /// Hash da chave para as posições em `entries` que têm esse hash.
    index: HashMap<i64, Vec<usize>>,
    len: usize,
}

/// O valor atual do contador global das mutações: igual ao de uma leitura anterior quer dizer que nenhum
/// dict foi alterado nesse intervalo (as visões vivas das globais usam isso para não varrer nada).
pub fn generation_now() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

impl Dict {
    /// `d.clear()`: esvazia e conta como mutação, para as visões vivas perceberem.
    pub fn clear(&mut self) {
        *self = Dict::default();
        self.generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Posição da chave em `entries`, se presente.
    fn find(&self, h: i64, key: &Value) -> Option<usize> {
        self.index.get(&h)?.iter().copied().find(|&i| match &self.entries[i] {
            Some((_, k, _)) => is(k, key) || py_eq(k, key),
            None => false,
        })
    }

    /// `d[key]`; `None` quando ausente (o `KeyError` é do chamador). Chave sem hash é `TypeError`.
    pub fn get(&self, key: &Value) -> Result<Option<Value>, ObjError> {
        let h = hash(key)?;
        Ok(self.find(h, key).and_then(|i| self.entries[i].as_ref().map(|(_, _, v)| v.clone())))
    }

    pub fn contains(&self, key: &Value) -> Result<bool, ObjError> {
        let h = hash(key)?;
        Ok(self.find(h, key).is_some())
    }

    /// `d[key] = value`. Chave já presente mantém o objeto-chave original e a posição, como o
    /// `insertdict` do CPython (`{1: 'a'}` seguido de `d[True] = 'b'` imprime `{1: 'b'}`).
    pub fn set(&mut self, key: Value, value: Value) -> Result<(), ObjError> {
        let h = hash(&key)?;
        self.generation = GENERATION.fetch_add(1, Ordering::Relaxed);
        if let Some(i) = self.find(h, &key) {
            if let Some((_, _, v)) = &mut self.entries[i] {
                *v = value;
            }
            return Ok(());
        }
        self.index.entry(h).or_default().push(self.entries.len());
        self.entries.push(Some((h, key, value)));
        self.len += 1;
        Ok(())
    }

    /// `del d[key]` / `d.pop(key)`: devolve o valor removido, ou `None` se a chave não existia.
    pub fn remove(&mut self, key: &Value) -> Result<Option<Value>, ObjError> {
        let h = hash(key)?;
        let Some(i) = self.find(h, key) else { return Ok(None) };
        self.generation = GENERATION.fetch_add(1, Ordering::Relaxed);
        let removed = self.entries[i].take().map(|(_, _, v)| v);
        if let Some(slots) = self.index.get_mut(&h) {
            slots.retain(|&s| s != i);
            if slots.is_empty() {
                self.index.remove(&h);
            }
        }
        self.len -= 1;
        if self.entries.len() > 8 && self.entries.len() > 2 * self.len {
            self.compact();
        }
        Ok(removed)
    }

    /// Descarta os buracos de entradas removidas e refaz o índice, preservando a ordem.
    fn compact(&mut self) {
        self.entries.retain(Option::is_some);
        self.index.clear();
        for (i, entry) in self.entries.iter().enumerate() {
            if let Some((h, _, _)) = entry {
                self.index.entry(*h).or_default().push(i);
            }
        }
    }

    /// Pares chave e valor na ordem de inserção.
    pub fn iter(&self) -> impl Iterator<Item = (&Value, &Value)> {
        self.entries.iter().flatten().map(|(_, k, v)| (k, v))
    }

    /// Como [`Dict::iter`], com o hash que cada chave tinha ao entrar (a imagem do heap o leva junto: uma
    /// chave cujo `__hash__` é código Python, como um membro de `IntEnum`, não se rehasheia sem uma `Vm`).
    pub fn iter_hashed(&self) -> impl Iterator<Item = (i64, &Value, &Value)> {
        self.entries.iter().flatten().map(|(h, k, v)| (*h, k, v))
    }

    /// Refaz um dict da lista de [`Dict::iter_hashed`]: as chaves já são distintas, entram com o hash dado.
    pub fn from_hashed(entries: impl IntoIterator<Item = (i64, Value, Value)>) -> Dict {
        let mut dict = Dict::default();
        for (h, k, v) in entries {
            dict.index.entry(h).or_default().push(dict.entries.len());
            dict.entries.push(Some((h, k, v)));
            dict.len += 1;
        }
        dict.generation = GENERATION.fetch_add(1, Ordering::Relaxed);
        dict
    }

    pub fn keys(&self) -> impl Iterator<Item = &Value> {
        self.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.iter().map(|(_, v)| v)
    }
}

/// `dict_repr`: `{}` vazio, `{...}` na recursão, `{k: v, ...}` com `repr` das chaves e valores.
pub(super) fn dict_repr(d: &Dict, id: usize, out: &mut String, stack: &mut ReprStack) {
    if d.is_empty() {
        out.push_str("{}");
        return;
    }
    if !stack.enter(id) {
        out.push_str("{...}");
        return;
    }
    out.push('{');
    for (i, (k, v)) in d.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        repr_into(k, out, stack);
        out.push_str(": ");
        repr_into(v, out, stack);
    }
    out.push('}');
    stack.leave(id);
}

/// `dict_equal`: mesmo tamanho e, para cada chave de `a`, o valor em `b` idêntico ou igual. A ordem
/// não conta.
pub(super) fn dict_eq(a: &Dict, b: &Dict) -> bool {
    a.len() == b.len()
        && a.iter().all(|(k, v)| match b.get(k) {
            Ok(Some(w)) => is(v, &w) || py_eq(v, &w),
            _ => false,
        })
}
