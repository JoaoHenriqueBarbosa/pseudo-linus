//! Variáveis do shell: escopos dinâmicos (global, função, ambiente temporário), atributos e arrays.
//!
//! As regras de semântica (inteiro, maiúsculas, nameref, readonly) ficam no interpretador; aqui
//! ficam as estruturas e as primitivas de busca e escrita por escopo.

use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Atributos de variável (os do `declare`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Attrs(pub u16);

impl Attrs {
    pub const EXPORT: Attrs = Attrs(1);
    pub const READONLY: Attrs = Attrs(1 << 1);
    pub const INTEGER: Attrs = Attrs(1 << 2);
    pub const LOWER: Attrs = Attrs(1 << 3);
    pub const UPPER: Attrs = Attrs(1 << 4);
    pub const CAPITALIZE: Attrs = Attrs(1 << 5);
    pub const NAMEREF: Attrs = Attrs(1 << 6);
    pub const INDEXED: Attrs = Attrs(1 << 7);
    pub const ASSOC: Attrs = Attrs(1 << 8);
    pub const TRACE: Attrs = Attrs(1 << 9);

    pub fn has(self, a: Attrs) -> bool {
        self.0 & a.0 != 0
    }

    pub fn set(&mut self, a: Attrs) {
        self.0 |= a.0;
    }

    pub fn clear(&mut self, a: Attrs) {
        self.0 &= !a.0;
    }
}

/// Tabela associativa com a mesma ordem de iteração do bash 5.2 (hashlib.c: FNV-1 de 32 bits,
/// 1024 baldes, inserção no começo do balde, crescimento x4 ao chegar em 2 itens por balde).
#[derive(Clone, Debug)]
pub struct Assoc {
    map: HashMap<Vec<u8>, Vec<u8>>,
    buckets: Vec<Vec<Vec<u8>>>,
}

const ASSOC_BUCKETS: usize = 1024;

pub fn bash_hash(s: &[u8]) -> u32 {
    let mut h: u32 = 2_166_136_261;
    for &c in s {
        h = h.wrapping_mul(16_777_619);
        h ^= c as u32;
    }
    h
}

impl Default for Assoc {
    fn default() -> Self {
        Assoc { map: HashMap::new(), buckets: Vec::new() }
    }
}

impl Assoc {
    pub fn new() -> Assoc {
        Assoc::default()
    }

    fn nbuckets(&self) -> usize {
        if self.buckets.is_empty() { ASSOC_BUCKETS } else { self.buckets.len() }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get(&self, k: &[u8]) -> Option<&Vec<u8>> {
        self.map.get(k)
    }

    pub fn contains(&self, k: &[u8]) -> bool {
        self.map.contains_key(k)
    }

    pub fn insert(&mut self, k: Vec<u8>, v: Vec<u8>) {
        if let Some(slot) = self.map.get_mut(&k) {
            *slot = v;
            return;
        }
        if self.buckets.is_empty() {
            self.buckets = vec![Vec::new(); ASSOC_BUCKETS];
        }
        if self.map.len() >= self.nbuckets() * 2 {
            self.grow();
        }
        let b = bash_hash(&k) as usize & (self.nbuckets() - 1);
        self.buckets[b].insert(0, k.clone());
        self.map.insert(k, v);
    }

    fn grow(&mut self) {
        let n = self.nbuckets() * 4;
        let mut nb: Vec<Vec<Vec<u8>>> = vec![Vec::new(); n];
        for bucket in std::mem::take(&mut self.buckets) {
            for k in bucket {
                let b = bash_hash(&k) as usize & (n - 1);
                nb[b].insert(0, k);
            }
        }
        self.buckets = nb;
    }

    pub fn remove(&mut self, k: &[u8]) -> Option<Vec<u8>> {
        let v = self.map.remove(k)?;
        let b = bash_hash(k) as usize & (self.nbuckets() - 1);
        if let Some(bucket) = self.buckets.get_mut(b) {
            bucket.retain(|x| x != k);
        }
        Some(v)
    }

    /// Chaves na ordem do bash.
    pub fn keys(&self) -> Vec<&Vec<u8>> {
        self.buckets.iter().flat_map(|b| b.iter()).collect()
    }

    /// Pares na ordem do bash.
    pub fn iter(&self) -> impl Iterator<Item = (&Vec<u8>, &Vec<u8>)> {
        self.buckets.iter().flat_map(|b| b.iter()).filter_map(move |k| self.map.get(k).map(|v| (k, v)))
    }
}

/// Valor de uma variável.
#[derive(Clone, Debug)]
pub enum Value {
    /// Declarada sem valor (`local x`, `declare x`, ou desfeita no escopo local).
    Unset,
    Scalar(Vec<u8>),
    Indexed(BTreeMap<i64, Vec<u8>>),
    Assoc(Assoc),
}

#[derive(Clone, Debug)]
pub struct Var {
    pub value: Value,
    pub attrs: Attrs,
}

impl Var {
    pub fn scalar(v: Vec<u8>) -> Var {
        Var { value: Value::Scalar(v), attrs: Attrs::default() }
    }

    pub fn is_set(&self) -> bool {
        !matches!(self.value, Value::Unset)
    }

    pub fn is_array(&self) -> bool {
        matches!(self.value, Value::Indexed(_) | Value::Assoc(_)) || self.attrs.has(Attrs::INDEXED) || self.attrs.has(Attrs::ASSOC)
    }

    /// Valor como escalar (`$x`): elemento 0 de array indexado, `[0]` de associativo.
    pub fn scalar_value(&self) -> Option<&[u8]> {
        match &self.value {
            Value::Unset => None,
            Value::Scalar(v) => Some(v),
            Value::Indexed(m) => m.get(&0).map(|v| v.as_slice()),
            Value::Assoc(a) => a.get(b"0").map(|v| v.as_slice()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    Global,
    Function,
    /// Atribuições na frente de builtin ou função (`X=1 f`).
    Temp,
}

#[derive(Clone, Debug)]
pub struct Scope {
    pub kind: ScopeKind,
    pub map: HashMap<String, Var>,
}

/// O conjunto de escopos.
#[derive(Clone, Debug)]
pub struct Vars {
    scopes: Vec<Scope>,
}

impl Default for Vars {
    fn default() -> Self {
        Vars::new()
    }
}

impl Vars {
    pub fn new() -> Vars {
        Vars { scopes: vec![Scope { kind: ScopeKind::Global, map: HashMap::new() }] }
    }

    pub fn push(&mut self, kind: ScopeKind) {
        self.scopes.push(Scope { kind, map: HashMap::new() });
    }

    pub fn pop(&mut self) -> Option<Scope> {
        if self.scopes.len() > 1 { self.scopes.pop() } else { None }
    }

    pub fn depth(&self) -> usize {
        self.scopes.len()
    }

    /// Profundidade de funções (quantos escopos de função estão abertos).
    pub fn function_depth(&self) -> usize {
        self.scopes.iter().filter(|s| s.kind == ScopeKind::Function).count()
    }

    /// Índice do escopo mais interno que tem `name`.
    pub fn find_scope(&self, name: &str) -> Option<usize> {
        (0..self.scopes.len()).rev().find(|i| self.scopes[*i].map.contains_key(name))
    }

    pub fn get(&self, name: &str) -> Option<&Var> {
        let i = self.find_scope(name)?;
        self.scopes[i].map.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Var> {
        let i = self.find_scope(name)?;
        self.scopes[i].map.get_mut(name)
    }

    /// Variável no escopo `i`.
    pub fn get_in(&self, i: usize, name: &str) -> Option<&Var> {
        self.scopes.get(i)?.map.get(name)
    }

    /// Entrada pra escrita: o escopo mais interno que já tem o nome, ou o global.
    pub fn entry(&mut self, name: &str) -> &mut Var {
        let i = self.find_scope(name).unwrap_or(0);
        self.scopes[i].map.entry(name.to_string()).or_insert(Var { value: Value::Unset, attrs: Attrs::default() })
    }

    /// Entrada no escopo global (`declare -g`).
    pub fn global_entry(&mut self, name: &str) -> &mut Var {
        self.scopes[0].map.entry(name.to_string()).or_insert(Var { value: Value::Unset, attrs: Attrs::default() })
    }

    /// Índice do escopo de função mais interno (ou `None` fora de função).
    pub fn current_function_scope(&self) -> Option<usize> {
        (0..self.scopes.len()).rev().find(|i| self.scopes[*i].kind == ScopeKind::Function)
    }

    /// Entrada local na função corrente (`local`); `None` fora de função.
    pub fn local_entry(&mut self, name: &str) -> Option<(&mut Var, bool)> {
        let i = self.current_function_scope()?;
        let existed = self.scopes[i].map.contains_key(name);
        Some((self.scopes[i].map.entry(name.to_string()).or_insert(Var { value: Value::Unset, attrs: Attrs::default() }), existed))
    }

    /// Entrada no escopo do topo (ambiente temporário).
    pub fn top_entry(&mut self, name: &str) -> &mut Var {
        let i = self.scopes.len() - 1;
        self.scopes[i].map.entry(name.to_string()).or_insert(Var { value: Value::Unset, attrs: Attrs::default() })
    }

    pub fn top_kind(&self) -> ScopeKind {
        self.scopes.last().map_or(ScopeKind::Global, |s| s.kind)
    }

    /// `unset nome`: no escopo local corrente a variável fica declarada sem valor (continua
    /// escondendo as de fora até a função voltar); em escopo de fora ela some de verdade.
    pub fn unset(&mut self, name: &str) {
        let Some(i) = self.find_scope(name) else { return };
        let is_current_local = self.current_function_scope() == Some(i) && i == self.scopes.len() - 1;
        if is_current_local {
            if let Some(v) = self.scopes[i].map.get_mut(name) {
                v.value = Value::Unset;
                v.attrs = Attrs::default();
            }
        } else {
            self.scopes[i].map.remove(name);
        }
    }

    /// Remove de vez do escopo `i`.
    pub fn remove_in(&mut self, i: usize, name: &str) -> Option<Var> {
        self.scopes.get_mut(i)?.map.remove(name)
    }

    /// Nomes visíveis, ordenados.
    pub fn names(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for s in &self.scopes {
            for k in s.map.keys() {
                out.insert(k.clone());
            }
        }
        out
    }

    /// Variáveis visíveis (a mais interna de cada nome), ordenadas por nome.
    pub fn visible(&self) -> Vec<(String, &Var)> {
        self.names().into_iter().filter_map(|n| self.get(&n).map(|v| (n, v))).collect()
    }

    /// Escopos (pra quem precisa iterar, como o ambiente exportado).
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assoc_order_matches_bash() {
        // Golden do caso array-assoc-iteration-order (bash 5.2.37).
        let mut a = Assoc::new();
        for k in ["alfa", "beta", "gama", "delta", "epsilon", "zeta", "eta", "teta"] {
            a.insert(k.as_bytes().to_vec(), b"1".to_vec());
        }
        let keys: Vec<String> = a.keys().iter().map(|k| String::from_utf8_lossy(k).into_owned()).collect();
        assert_eq!(keys.join(" "), "teta alfa eta delta epsilon beta zeta gama");
    }

    #[test]
    fn scopes_and_unset_semantics() {
        let mut v = Vars::new();
        v.entry("x").value = Value::Scalar(b"global".to_vec());
        v.push(ScopeKind::Function);
        let (l, _) = v.local_entry("x").unwrap();
        l.value = Value::Scalar(b"local".to_vec());
        assert_eq!(v.get("x").unwrap().scalar_value(), Some(&b"local"[..]));
        v.unset("x");
        // Ainda declarada (sem valor) no escopo local.
        assert!(!v.get("x").unwrap().is_set());
        v.pop();
        assert_eq!(v.get("x").unwrap().scalar_value(), Some(&b"global"[..]));
        // unset de escopo de fora revela o global.
        v.push(ScopeKind::Function);
        v.local_entry("y").unwrap().0.value = Value::Scalar(b"f".to_vec());
        v.entry("y");
        v.push(ScopeKind::Function);
        v.unset("y");
        assert!(v.get("y").is_none());
    }
}
