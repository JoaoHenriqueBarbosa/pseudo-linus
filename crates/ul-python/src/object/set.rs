//! `set` mínimo (`Objects/setobject.c`): inserção, pertinência, remoção, iteração e `repr`.
//!
//! Diferente do `dict`, a ordem de iteração de um `set` é a ordem das posições na tabela de hash, e
//! `print({8, 1, 100})` a expõe. Por isso a tabela é port fiel do CPython: sondagem linear de até 9
//! vizinhos (`LINEAR_PROBES`), perturbação `i = i*5 + 1 + perturb` com `perturb >>= 5`, entradas
//! removidas como marcador (`dummy`) e o redimensionamento do `set_table_resize`.

use super::{hash, is, py_eq, repr_into, ObjError, ReprStack, Value};

const MIN_SIZE: usize = 8;
const LINEAR_PROBES: usize = 9;
const PERTURB_SHIFT: u32 = 5;

#[derive(Clone)]
enum Slot {
    Empty,
    /// Entrada removida: não encerra a sondagem e não é reaproveitada na inserção.
    Dummy,
    Active(i64, Value),
}

#[derive(Clone)]
pub struct Set {
    table: Vec<Slot>,
    /// Posições ocupadas por entradas ativas ou removidas.
    fill: usize,
    /// Entradas ativas.
    used: usize,
    /// `frozenset`: imutável e com hash.
    frozen: bool,
}

impl Default for Set {
    fn default() -> Set {
        Set::new()
    }
}

impl Set {
    pub fn new() -> Set {
        Set { table: vec![Slot::Empty; MIN_SIZE], fill: 0, used: 0, frozen: false }
    }

    pub fn len(&self) -> usize {
        self.used
    }

    pub fn is_empty(&self) -> bool {
        self.used == 0
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// O mesmo conteúdo como `frozenset` (ou como `set`, com `false`).
    pub fn with_frozen(mut self, frozen: bool) -> Set {
        self.frozen = frozen;
        self
    }

    /// `frozenset_hash`: combina os hashes dos elementos sem depender da ordem (`Objects/setobject.c`).
    pub fn frozen_hash(&self) -> i64 {
        fn shuffle(h: u64) -> u64 {
            ((h ^ 89_869_747) ^ (h << 16)).wrapping_mul(3_644_798_167)
        }
        let mut acc: u64 = 0;
        for slot in &self.table {
            if let Slot::Active(h, _) = slot {
                acc ^= shuffle(*h as u64);
            }
        }
        acc ^= ((self.used as u64) + 1).wrapping_mul(1_927_868_237);
        acc ^= (acc >> 11) ^ (acc >> 25);
        acc = acc.wrapping_mul(69_069).wrapping_add(907_133_923);
        let h = acc as i64;
        if h == -1 {
            590_923_713
        } else {
            h
        }
    }

    fn mask(&self) -> usize {
        self.table.len() - 1
    }

    /// `set_lookkey`/`set_add_entry`: posição da chave, ou `Err` com a primeira posição vazia
    /// encontrada na sequência de sondagem.
    fn probe(&self, h: i64, key: &Value) -> Result<usize, usize> {
        let mask = self.mask();
        let mut perturb = h as u64 as usize;
        let mut i = h as u64 as usize & mask;
        loop {
            let probes = if i + LINEAR_PROBES <= mask { LINEAR_PROBES } else { 0 };
            for j in i..=i + probes {
                match &self.table[j] {
                    Slot::Empty => return Err(j),
                    Slot::Active(eh, k) if *eh == h && (is(k, key) || py_eq(k, key)) => return Ok(j),
                    _ => {}
                }
            }
            perturb >>= PERTURB_SHIFT;
            i = (i.wrapping_mul(5).wrapping_add(1).wrapping_add(perturb)) & mask;
        }
    }

    /// A primeira posição `Dummy` da sequência de sondagem de `h` até a posição vazia `end`, se houver.
    fn first_dummy(&self, h: i64, end: usize) -> Option<usize> {
        let mask = self.mask();
        let mut perturb = h as u64 as usize;
        let mut i = h as u64 as usize & mask;
        loop {
            let probes = if i + LINEAR_PROBES <= mask { LINEAR_PROBES } else { 0 };
            for j in i..=i + probes {
                if j == end {
                    return None;
                }
                if matches!(self.table[j], Slot::Dummy) {
                    return Some(j);
                }
            }
            perturb >>= PERTURB_SHIFT;
            i = (i.wrapping_mul(5).wrapping_add(1).wrapping_add(perturb)) & mask;
        }
    }

    pub fn contains(&self, key: &Value) -> Result<bool, ObjError> {
        let h = hash(key)?;
        Ok(self.probe(h, key).is_ok())
    }

    /// `set.add`.
    pub fn add(&mut self, key: Value) -> Result<(), ObjError> {
        let h = hash(&key)?;
        let Err(slot) = self.probe(h, &key) else { return Ok(()) };
        // `set_add_entry`: a primeira posição removida da sequência de sondagem é reaproveitada.
        if let Some(free) = self.first_dummy(h, slot) {
            self.table[free] = Slot::Active(h, key);
            self.used += 1;
            return Ok(());
        }
        self.table[slot] = Slot::Active(h, key);
        self.fill += 1;
        self.used += 1;
        if self.fill * 5 >= self.mask() * 3 {
            self.resize(if self.used > 50_000 { self.used * 2 } else { self.used * 4 });
        }
        Ok(())
    }

    /// `set.discard`: devolve se a chave estava presente.
    pub fn discard(&mut self, key: &Value) -> Result<bool, ObjError> {
        let h = hash(key)?;
        match self.probe(h, key) {
            Ok(slot) => {
                self.table[slot] = Slot::Dummy;
                self.used -= 1;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    /// `set_table_resize`: menor potência de 2 maior que `minused`, reinserindo na ordem antiga.
    fn resize(&mut self, minused: usize) {
        let mut size = MIN_SIZE;
        while size <= minused {
            size <<= 1;
        }
        let old = std::mem::replace(&mut self.table, vec![Slot::Empty; size]);
        let mask = size - 1;
        for slot in old {
            if let Slot::Active(h, key) = slot {
                // `set_insert_clean`: a tabela nova não tem removidas nem chaves repetidas.
                let mut perturb = h as u64 as usize;
                let mut i = h as u64 as usize & mask;
                let target = 'search: loop {
                    let probes = if i + LINEAR_PROBES <= mask { LINEAR_PROBES } else { 0 };
                    for j in i..=i + probes {
                        if matches!(self.table[j], Slot::Empty) {
                            break 'search j;
                        }
                    }
                    perturb >>= PERTURB_SHIFT;
                    i = (i.wrapping_mul(5).wrapping_add(1).wrapping_add(perturb)) & mask;
                };
                self.table[target] = Slot::Active(h, key);
            }
        }
        self.fill = self.used;
    }

    /// Elementos na ordem da tabela, que é a ordem de iteração do CPython.
    pub fn iter(&self) -> impl Iterator<Item = &Value> {
        self.table.iter().filter_map(|slot| match slot {
            Slot::Active(_, k) => Some(k),
            _ => None,
        })
    }
}

/// `set_repr`: `set()` vazio, `set(...)` na recursão, `{a, b}` no resto (`frozenset({a, b})` se congelado).
pub(super) fn set_repr(s: &Set, id: usize, out: &mut String, stack: &mut ReprStack) {
    let name = if s.frozen { "frozenset" } else { "set" };
    if s.is_empty() {
        out.push_str(name);
        out.push_str("()");
        return;
    }
    if !stack.enter(id) {
        out.push_str(name);
        out.push_str("(...)");
        return;
    }
    if s.frozen {
        out.push_str("frozenset(");
    }
    out.push('{');
    for (i, item) in s.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        repr_into(item, out, stack);
    }
    out.push('}');
    if s.frozen {
        out.push(')');
    }
    stack.leave(id);
}

/// Igualdade de conjuntos: mesmo tamanho e todo elemento de `a` presente em `b`.
pub(super) fn set_eq(a: &Set, b: &Set) -> bool {
    a.len() == b.len() && a.iter().all(|k| b.contains(k).unwrap_or(false))
}
