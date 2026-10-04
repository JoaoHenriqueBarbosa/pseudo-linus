//! Detecção de renomeação e cópia, desenho próprio (não deriva do `diffcore-rename` do git).
//!
//! - Exatas: criação e remoção (ou, com cópia, modificação) com o mesmo id e o mesmo tipo de
//!   arquivo; prefere a origem com o mesmo nome base.
//! - Por semelhança: pontuação = bytes das linhas do arquivo antigo que o diff de linhas preserva,
//!   divididos pelo maior dos dois tamanhos, numa escala de 0 a 60000 (60000 = 100%). Pares acima
//!   do limiar (`-M`, padrão 50%) são atribuídos do mais parecido pro menos, com empate decidido por
//!   nome base igual e depois pela ordem dos caminhos.
//!
//! A concordância com as porcentagens do git é medida contra o oráculo (ver STATUS.md).

use std::collections::HashMap;
use std::rc::Rc;

use super::{Pair, Side, content_of, text};
use crate::error::R;
use crate::object;
use crate::os;
use crate::repo::Repo;

pub const MAX_SCORE: u32 = 60000;
pub const DEFAULT_RENAME_SCORE: u32 = 30000;

#[derive(Clone, Copy, Debug)]
pub struct RenameOpts {
    pub min_score: u32,
    pub copies: bool,
    /// Máximo de origens x destinos na busca por semelhança (o `diff.renameLimit`).
    pub limit: usize,
}

impl Default for RenameOpts {
    fn default() -> Self {
        RenameOpts { min_score: DEFAULT_RENAME_SCORE, copies: false, limit: 1000 }
    }
}

/// `-M50%`, `-M5` (= 50%), `-M` (padrão), `-M75` etc.
pub fn parse_score(s: &str) -> Option<u32> {
    if s.is_empty() {
        return Some(DEFAULT_RENAME_SCORE);
    }
    if let Some(p) = s.strip_suffix('%') {
        let v: f64 = p.parse().ok()?;
        return Some(((v / 100.0) * MAX_SCORE as f64).min(MAX_SCORE as f64) as u32);
    }
    // Sem `%`: os dígitos são a fração depois da vírgula (`5` = 0.5, `75` = 0.75).
    if !s.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let frac: f64 = format!("0.{s}").parse().ok()?;
    Some((frac * MAX_SCORE as f64) as u32)
}

struct Cache<'a> {
    repo: &'a Repo,
    data: HashMap<(usize, bool), Rc<Vec<u8>>>,
}

impl Cache<'_> {
    fn get(&mut self, i: usize, one: bool, side: &Side) -> R<Rc<Vec<u8>>> {
        if let Some(d) = self.data.get(&(i, one)) {
            return Ok(d.clone());
        }
        let d = content_of(self.repo, side)?;
        self.data.insert((i, one), d.clone());
        Ok(d)
    }
}

/// Pontuação de semelhança entre dois conteúdos.
pub fn similarity(old: &[u8], new: &[u8]) -> u32 {
    let max = old.len().max(new.len());
    if max == 0 {
        return MAX_SCORE;
    }
    let a = text::split_lines(old);
    let b = text::split_lines(new);
    let changes = text::changes(&a, &b, text::Ws::default());
    let removed: usize = changes.iter().flat_map(|c| a[c.i1..c.i1 + c.chg1].iter()).map(|l| l.len()).sum();
    let kept = old.len() - removed;
    (kept as u64 * MAX_SCORE as u64 / max as u64) as u32
}

fn same_basename(a: &[u8], b: &[u8]) -> bool {
    os::basename(a) == os::basename(b)
}

fn same_type(a: u32, b: u32) -> bool {
    (a & 0o170000) == (b & 0o170000)
}

/// Aplica a detecção sobre a fila de pares (em ordem de caminho).
pub fn detect(repo: &Repo, q: Vec<Pair>, o: &RenameOpts) -> R<Vec<Pair>> {
    let dsts: Vec<usize> = (0..q.len()).filter(|&i| q[i].status == b'A').collect();
    let srcs: Vec<usize> = (0..q.len()).filter(|&i| q[i].status == b'D' || (o.copies && q[i].status == b'M')).collect();
    if dsts.is_empty() || srcs.is_empty() {
        return Ok(q);
    }
    let mut assigned: HashMap<usize, (usize, u32)> = HashMap::new();
    let mut uses: HashMap<usize, usize> = HashMap::new();
    let free = |s: usize, uses: &HashMap<usize, usize>| o.copies || !uses.contains_key(&s);

    // Exatas.
    for &d in &dsts {
        let dp = &q[d].two;
        let candidates: Vec<usize> = srcs
            .iter()
            .copied()
            .filter(|&s| q[s].one.oid == dp.oid && same_type(q[s].one.mode, dp.mode) && free(s, &uses))
            .collect();
        let pick = candidates
            .iter()
            .copied()
            .find(|&s| !uses.contains_key(&s) && same_basename(&q[s].one.path, &dp.path))
            .or_else(|| candidates.iter().copied().find(|&s| !uses.contains_key(&s)))
            .or_else(|| candidates.first().copied());
        if let Some(s) = pick {
            assigned.insert(d, (s, MAX_SCORE));
            *uses.entry(s).or_insert(0) += 1;
        }
    }

    // Por semelhança.
    let rem_d: Vec<usize> = dsts.iter().copied().filter(|d| !assigned.contains_key(d) && object::is_reg(q[*d].two.mode)).collect();
    let rem_s: Vec<usize> = srcs.iter().copied().filter(|&s| free(s, &uses) && object::is_reg(q[s].one.mode)).collect();
    if !rem_d.is_empty() && !rem_s.is_empty() && rem_d.len().saturating_mul(rem_s.len()) <= o.limit.saturating_mul(o.limit) {
        let mut cache = Cache { repo, data: HashMap::new() };
        let mut cands: Vec<(u32, bool, usize, usize)> = Vec::new();
        for &d in &rem_d {
            let nd = cache.get(d, false, &q[d].two)?;
            for &s in &rem_s {
                let od = cache.get(s, true, &q[s].one)?;
                let (lo, hi) = (od.len().min(nd.len()) as u64, od.len().max(nd.len()) as u64);
                // Tamanhos tão diferentes que nem tudo igual chegaria ao limiar.
                if hi > 0 && lo * MAX_SCORE as u64 / hi < o.min_score as u64 {
                    continue;
                }
                let score = similarity(&od, &nd);
                if score >= o.min_score {
                    cands.push((score, same_basename(&q[s].one.path, &q[d].two.path), d, s));
                }
            }
        }
        cands.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)).then(a.3.cmp(&b.3)));
        for (score, _, d, s) in cands {
            if assigned.contains_key(&d) || !free(s, &uses) {
                continue;
            }
            assigned.insert(d, (s, score));
            *uses.entry(s).or_insert(0) += 1;
        }
    }

    // Remonta a fila: o par renomeado ou copiado entra no lugar da criação; a remoção usada sai.
    // Quando uma origem removida serve a vários destinos, o último (em ordem de caminho) é a
    // renomeação e os anteriores são cópias.
    let mut last_user: HashMap<usize, usize> = HashMap::new();
    for (&d, &(s, _)) in &assigned {
        let e = last_user.entry(s).or_insert(d);
        if d > *e {
            *e = d;
        }
    }
    let mut out = Vec::with_capacity(q.len());
    for (i, p) in q.iter().enumerate() {
        if let Some(&(s, score)) = assigned.get(&i) {
            let src_deleted = q[s].status == b'D';
            let status = if src_deleted && last_user.get(&s) == Some(&i) { b'R' } else { b'C' };
            out.push(Pair { one: q[s].one.clone(), two: p.two.clone(), status, score });
        } else if p.status == b'D' && uses.contains_key(&i) {
            continue;
        } else {
            out.push(p.clone());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores() {
        assert_eq!(similarity(b"a\nb\nc\n", b"a\nb\nc\n"), MAX_SCORE);
        // 4 de 6 bytes preservados, maior tamanho 6.
        assert_eq!(similarity(b"a\nb\nc\n", b"a\nb\nX\n"), 40000);
        assert_eq!(parse_score("75%"), Some(45000));
        assert_eq!(parse_score("5"), Some(30000));
    }
}
