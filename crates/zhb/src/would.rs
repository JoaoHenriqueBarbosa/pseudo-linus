//! `hb_ot_layout_lookup_would_substitute`: se um lookup do GSUB substituiria a sequência de glifos
//! dada, sem buffer nem contexto em volta (`hb_would_apply_context_t`). O shaper índico pergunta
//! isso para decidir as formas de base, rph e pref antes de aplicar as features.

use crate::gsubgpos::resolve_subtable;
use crate::ot::{self, GsubGpos, NOT_COVERED, u16at};

/// `hb_set_digest_t`: três padrões de bits de 64 (deslocamentos 4, 0 e 9) que respondem "talvez
/// tenha" para os glifos das coberturas do lookup, e com certeza "não tem" para os demais.
#[derive(Default, Clone, Copy)]
struct Digest {
    masks: [u64; 3],
}

const SHIFTS: [u32; 3] = [4, 0, 9];

impl Digest {
    fn mask_for(g: u32, shift: u32) -> u64 {
        1u64 << ((g >> shift) & 63)
    }

    fn add(&mut self, g: u32) {
        for (m, s) in self.masks.iter_mut().zip(SHIFTS) {
            *m |= Self::mask_for(g, s);
        }
    }

    /// `hb_set_digest_bits_pattern_t::add_range` de cada padrão.
    fn add_range(&mut self, a: u32, b: u32) {
        for (m, s) in self.masks.iter_mut().zip(SHIFTS) {
            if *m == u64::MAX {
                continue;
            }
            if (b >> s).wrapping_sub(a >> s) >= 63 {
                *m = u64::MAX;
            } else {
                let (ma, mb) = (Self::mask_for(a, s), Self::mask_for(b, s));
                *m |= mb.wrapping_add(mb.wrapping_sub(ma)).wrapping_sub(u64::from(mb < ma));
            }
        }
    }

    fn may_have(&self, g: u32) -> bool {
        self.masks.iter().zip(SHIFTS).all(|(m, s)| m & Self::mask_for(g, s) != 0)
    }

    /// `Coverage::collect_coverage`.
    fn add_coverage(&mut self, cov: Option<&[u8]>) {
        let Some(d) = cov else { return };
        match u16at(d, 0) {
            1 => {
                let n = usize::from(u16at(d, 2));
                if d.len() < 4 + n * 2 {
                    return;
                }
                for i in 0..n {
                    self.add(u32::from(u16at(d, 4 + i * 2)));
                }
            }
            2 => {
                let n = usize::from(u16at(d, 2));
                if d.len() < 4 + n * 6 {
                    return;
                }
                for i in 0..n {
                    let r = 4 + i * 6;
                    let (a, b) = (u32::from(u16at(d, r)), u32::from(u16at(d, r + 2)));
                    if a <= b {
                        self.add_range(a, b);
                    }
                }
            }
            _ => {}
        }
    }
}

/// O `get_coverage` de uma subtabela do GSUB (a primeira cobertura de entrada).
fn subtable_coverage(t: u16, d: &[u8]) -> Option<&[u8]> {
    match (t, u16at(d, 0)) {
        (1..=4, _) | (5 | 6, 1 | 2) => ot::sub(d, 0, 2),
        (5, 3) => ot::sub(d, 0, 6),
        (6, 3) => {
            let bt = usize::from(u16at(d, 2));
            ot::sub(d, 0, 4 + bt * 2 + 2)
        }
        (8, 1) => ot::sub(d, 0, 2),
        _ => None,
    }
}

/// O casamento dos glifos de entrada depois do primeiro (`would_match_input`).
#[derive(Clone, Copy)]
enum Match<'a> {
    Glyph,
    Class(Option<&'a [u8]>),
    Coverage(&'a [u8]),
}

fn matches(m: Match, g: u32, value: u32) -> bool {
    match m {
        Match::Glyph => g == value,
        Match::Class(cd) => ot::class(cd, g) == value,
        Match::Coverage(base) => {
            let off = value as usize;
            let cov = if off == 0 { None } else { base.get(off..) };
            ot::coverage(cov, g) != NOT_COVERED
        }
    }
}

/// `would_match_input`: a contagem inclui o primeiro glifo, que não é casado.
fn would_match_input(glyphs: &[u32], count: usize, d: &[u8], off: usize, m: Match) -> bool {
    if count != glyphs.len() {
        return false;
    }
    (1..count).all(|i| matches(m, glyphs[i], u32::from(u16at(d, off + (i - 1) * 2))))
}

/// `RuleSet::would_apply` do contexto simples.
fn rule_set_would_apply(set: Option<&[u8]>, glyphs: &[u32], m: Match) -> bool {
    let Some(set) = set else { return false };
    let n = usize::from(u16at(set, 0));
    (0..n).any(|i| {
        let Some(r) = ot::sub(set, 0, 2 + i * 2) else { return false };
        would_match_input(glyphs, usize::from(u16at(r, 0)), r, 4, m)
    })
}

/// `ChainRuleSet::would_apply`: com contexto zero, só regras sem backtrack nem lookahead.
fn chain_rule_set_would_apply(set: Option<&[u8]>, glyphs: &[u32], zero_context: bool, m: Match) -> bool {
    let Some(set) = set else { return false };
    let n = usize::from(u16at(set, 0));
    (0..n).any(|i| {
        let Some(r) = ot::sub(set, 0, 2 + i * 2) else { return false };
        let bt = usize::from(u16at(r, 0));
        let o = 2 + bt * 2;
        let input_count = usize::from(u16at(r, o));
        let la = usize::from(u16at(r, o + 2 + input_count.saturating_sub(1) * 2));
        (!zero_context || (bt == 0 && la == 0)) && would_match_input(glyphs, input_count, r, o + 2, m)
    })
}

/// O `ruleSet[índice]` de um array de deslocamentos: fora do array é o conjunto nulo.
fn rule_set_at(d: &[u8], count_at: usize, index: u32) -> Option<&[u8]> {
    if index == NOT_COVERED || index as usize >= usize::from(u16at(d, count_at)) {
        return None;
    }
    ot::sub(d, 0, count_at + 2 + index as usize * 2)
}

/// O `would_apply` de uma subtabela (já sem `Extension`).
fn subtable_would_apply(t: u16, d: &[u8], glyphs: &[u32], zero_context: bool) -> bool {
    let g0 = glyphs[0];
    let covered = |at: usize| ot::coverage(ot::sub(d, 0, at), g0) != NOT_COVERED;
    match (t, u16at(d, 0)) {
        // Simples, múltipla, alternada e encadeada reversa: um glifo coberto.
        (1, 1 | 2) | (2, 1) | (3, 1) | (8, 1) => glyphs.len() == 1 && covered(2),
        (4, 1) => {
            let index = ot::coverage(ot::sub(d, 0, 2), g0);
            let Some(set) = rule_set_at(d, 4, index) else { return false };
            let n = usize::from(u16at(set, 0));
            (0..n).any(|i| {
                let Some(lig) = ot::sub(set, 0, 2 + i * 2) else { return false };
                let comp = usize::from(u16at(lig, 2));
                glyphs.len() == comp && (1..comp).all(|k| glyphs[k] == u32::from(u16at(lig, 4 + (k - 1) * 2)))
            })
        }
        (5, 1) => {
            let index = ot::coverage(ot::sub(d, 0, 2), g0);
            rule_set_would_apply(rule_set_at(d, 4, index), glyphs, Match::Glyph)
        }
        (5, 2) => {
            let cd = ot::sub(d, 0, 4);
            rule_set_would_apply(rule_set_at(d, 6, ot::class(cd, g0)), glyphs, Match::Class(cd))
        }
        (5, 3) => {
            let glyph_count = usize::from(u16at(d, 2));
            would_match_input(glyphs, glyph_count, d, 8, Match::Coverage(d))
        }
        (6, 1) => {
            let index = ot::coverage(ot::sub(d, 0, 2), g0);
            chain_rule_set_would_apply(rule_set_at(d, 4, index), glyphs, zero_context, Match::Glyph)
        }
        (6, 2) => {
            let icd = ot::sub(d, 0, 6);
            chain_rule_set_would_apply(rule_set_at(d, 10, ot::class(icd, g0)), glyphs, zero_context, Match::Class(icd))
        }
        (6, 3) => {
            let bt = usize::from(u16at(d, 2));
            let o = 4 + bt * 2;
            let input_count = usize::from(u16at(d, o));
            let la = usize::from(u16at(d, o + 2 + input_count * 2));
            (!zero_context || (bt == 0 && la == 0)) && would_match_input(glyphs, input_count, d, o + 4, Match::Coverage(d))
        }
        _ => false,
    }
}

/// `hb_ot_layout_lookup_would_substitute` sobre o lookup `index` do GSUB.
pub fn would_substitute(gsub: &GsubGpos, index: u32, glyphs: &[u32], zero_context: bool) -> bool {
    if glyphs.is_empty() {
        return false;
    }
    let Some(l) = gsub.lookup(index as usize) else { return false };
    let subtables: Vec<(u16, &[u8])> = (0..l.subtable_count())
        .filter_map(|i| l.subtable(i))
        .map(|d| resolve_subtable(0, l.lookup_type(), d))
        .collect();
    let mut digest = Digest::default();
    for &(t, d) in &subtables {
        digest.add_coverage(subtable_coverage(t, d));
    }
    if !digest.may_have(glyphs[0]) {
        return false;
    }
    subtables.iter().any(|&(t, d)| subtable_would_apply(t, d, glyphs, zero_context))
}
