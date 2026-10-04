//! Algoritmos sobre o grafo de commits: bases comuns (pela definição do git-merge-base(1):
//! ancestrais comuns que não são ancestrais de outro ancestral comum), ancestralidade,
//! independentes e contagem à frente/atrás.

use std::collections::{BinaryHeap, HashMap, HashSet};

use crate::error::R;
use crate::hash::Oid;
use crate::repo::Repo;

/// Fila de commits por data de commit (mais novo primeiro); empates saem na ordem de entrada.
pub struct DateQueue {
    heap: BinaryHeap<(i64, std::cmp::Reverse<u64>, Oid)>,
    ctr: u64,
}

impl Default for DateQueue {
    fn default() -> Self {
        DateQueue::new()
    }
}

impl DateQueue {
    pub fn new() -> DateQueue {
        DateQueue { heap: BinaryHeap::new(), ctr: 0 }
    }

    pub fn push(&mut self, date: i64, id: Oid) {
        self.ctr += 1;
        self.heap.push((date, std::cmp::Reverse(self.ctr), id));
    }

    pub fn pop(&mut self) -> Option<(i64, Oid)> {
        self.heap.pop().map(|(d, _, id)| (d, id))
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }
}

/// Cache de datas e pais dos commits.
pub struct Graph<'a> {
    pub repo: &'a Repo,
    info: HashMap<Oid, (i64, Vec<Oid>)>,
}

impl<'a> Graph<'a> {
    pub fn new(repo: &'a Repo) -> Graph<'a> {
        Graph { repo, info: HashMap::new() }
    }

    pub fn info(&mut self, id: &Oid) -> R<(i64, Vec<Oid>)> {
        if let Some(i) = self.info.get(id) {
            return Ok(i.clone());
        }
        let c = self.repo.read_commit(id)?;
        let v = (c.commit_date(), c.parents);
        self.info.insert(*id, v.clone());
        Ok(v)
    }

    pub fn date(&mut self, id: &Oid) -> R<i64> {
        Ok(self.info(id)?.0)
    }

    pub fn parents(&mut self, id: &Oid) -> R<Vec<Oid>> {
        Ok(self.info(id)?.1)
    }

    /// Commits alcançáveis de `from` (inclusive).
    pub fn reachable(&mut self, from: &[Oid]) -> R<HashSet<Oid>> {
        let mut seen: HashSet<Oid> = HashSet::new();
        let mut stack: Vec<Oid> = from.to_vec();
        while let Some(c) = stack.pop() {
            if !seen.insert(c) {
                continue;
            }
            for p in self.parents(&c)? {
                if !seen.contains(&p) {
                    stack.push(p);
                }
            }
        }
        Ok(seen)
    }

    /// Melhores ancestrais comuns de `one` e do conjunto `twos` (como um merge hipotético deles),
    /// do mais novo pro mais antigo.
    pub fn merge_bases(&mut self, one: &Oid, twos: &[Oid]) -> R<Vec<Oid>> {
        if twos.contains(one) {
            return Ok(vec![*one]);
        }
        let a = self.reachable(&[*one])?;
        let b = self.reachable(twos)?;
        let common: HashSet<Oid> = a.intersection(&b).copied().collect();
        if common.is_empty() {
            return Ok(Vec::new());
        }
        // Os ancestrais próprios de algum comum são redundantes.
        let mut parents_of_common = Vec::new();
        for c in &common {
            parents_of_common.extend(self.parents(c)?);
        }
        let redundant = self.reachable(&parents_of_common)?;
        let mut best: Vec<(i64, Oid)> = Vec::new();
        for c in common {
            if !redundant.contains(&c) {
                best.push((self.date(&c)?, c));
            }
        }
        best.sort_by(|x, y| y.0.cmp(&x.0).then(x.1.cmp(&y.1)));
        Ok(best.into_iter().map(|(_, c)| c).collect())
    }

    /// Tira da lista quem é ancestral de outro da lista.
    pub fn remove_redundant(&mut self, list: &[Oid]) -> R<Vec<Oid>> {
        let mut out = Vec::new();
        for (i, c) in list.iter().enumerate() {
            let mut covered = false;
            for (j, d) in list.iter().enumerate() {
                if i != j && c != d && self.is_ancestor(c, d)? {
                    covered = true;
                    break;
                }
            }
            if !covered && !out.contains(c) {
                out.push(*c);
            }
        }
        Ok(out)
    }

    /// `a` é ancestral de `b` (ou igual)?
    pub fn is_ancestor(&mut self, a: &Oid, b: &Oid) -> R<bool> {
        if a == b {
            return Ok(true);
        }
        let mut seen = HashSet::new();
        let mut stack = vec![*b];
        while let Some(c) = stack.pop() {
            if c == *a {
                return Ok(true);
            }
            if !seen.insert(c) {
                continue;
            }
            for p in self.parents(&c)? {
                if !seen.contains(&p) {
                    stack.push(p);
                }
            }
        }
        Ok(false)
    }

    /// `(à frente, atrás)`: commits em `a` que não estão em `b`, e vice-versa.
    pub fn ahead_behind(&mut self, a: &Oid, b: &Oid) -> R<(usize, usize)> {
        let ra = self.reachable(&[*a])?;
        let rb = self.reachable(&[*b])?;
        Ok((ra.difference(&rb).count(), rb.difference(&ra).count()))
    }

    /// Os commits da lista que nenhum outro alcança (`merge-base --independent`).
    pub fn independent(&mut self, list: &[Oid]) -> R<Vec<Oid>> {
        let mut uniq: Vec<Oid> = Vec::new();
        for c in list {
            if !uniq.contains(c) {
                uniq.push(*c);
            }
        }
        self.remove_redundant(&uniq)
    }
}
