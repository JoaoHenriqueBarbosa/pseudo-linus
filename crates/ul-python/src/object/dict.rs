//! `dict` (`Objects/dictobject.c`): ordem de inserção, igualdade de chaves pelo protocolo do Python
//! (`1`, `1.0` e `True` são a mesma chave) e o `repr` com a proteção de recursão.
//!
//! A ordem de iteração de um `dict` é a de inserção e não depende do hash, então a tabela interna é
//! própria: um vetor de entradas na ordem de inserção (removidas viram buraco até a compactação) e um
//! índice de hash para posições. Só a ordem é observável; a disposição da tabela do CPython não é.

use std::collections::HashMap;

use super::{hash, is, py_eq, repr_into, ObjError, ReprStack, Value};

#[derive(Clone, Default)]
pub struct Dict {
    /// Entradas na ordem de inserção: hash, chave e valor. `None` é entrada removida.
    entries: Vec<Option<(i64, Value, Value)>>,
    /// Hash da chave para as posições em `entries` que têm esse hash.
    index: HashMap<i64, Vec<usize>>,
    len: usize,
}

impl Dict {
    pub fn new() -> Dict {
        Dict::default()
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
