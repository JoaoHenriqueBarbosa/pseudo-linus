//! Gerador de linhas de teste a partir do AST: um passeio aleatório (semente fixa) que produz textos
//! que provavelmente casam, mais variações com prefixo e sufixo. Quem decide se casa é o oráculo.

use crate::ast::{Node, PosixClass, Regex, Set, SetItem};

/// Gerador congruencial simples (determinístico, sem dependência).
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    pub fn next_u64(&mut self) -> u64 {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next_u64() % n }
    }
}

pub fn seed_of(text: &str) -> u64 {
    // FNV-1a
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

const PRINTABLE: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 _-./:=,;()[]{}<>\"'#$%&*+?@!|~^`\\";

fn pick_in_set(set: &Set, icase: bool, rng: &mut Rng) -> char {
    let mut candidates: Vec<char> = Vec::new();
    if !set.negated {
        for item in &set.items {
            match *item {
                SetItem::Char(c) => candidates.push(c),
                SetItem::Range(a, b) => {
                    let span = (b as u32).saturating_sub(a as u32) as u64 + 1;
                    let x = a as u32 + rng.below(span.min(5000)) as u32;
                    candidates.push(char::from_u32(x).unwrap_or(a));
                }
                SetItem::Class(k) => {
                    let ranges = k.ascii_ranges();
                    let (a, b) = ranges[rng.below(ranges.len() as u64) as usize];
                    let x = a as u32 + rng.below((b as u32 - a as u32) as u64 + 1) as u32;
                    candidates.push(char::from_u32(x).unwrap_or(a));
                }
            }
        }
    } else {
        for c in PRINTABLE.chars() {
            if set.contains(c, icase) {
                candidates.push(c);
            }
        }
    }
    if candidates.is_empty() {
        return 'x';
    }
    candidates[rng.below(candidates.len() as u64) as usize]
}

fn walk(n: &Node, icase: bool, rng: &mut Rng, groups: &mut Vec<Option<String>>, out: &mut String, budget: &mut usize) {
    if *budget == 0 {
        return;
    }
    *budget -= 1;
    match n {
        Node::Empty | Node::Assert(_) => {}
        Node::Char(c) => {
            if icase && rng.below(3) == 0 {
                out.extend(c.to_uppercase());
            } else {
                out.push(*c);
            }
        }
        Node::Any => {
            let p: Vec<char> = PRINTABLE.chars().collect();
            out.push(p[rng.below(p.len() as u64) as usize]);
        }
        Node::Set(s) => {
            let c = if s.escape && s.items == [SetItem::Class(PosixClass::Space)] && !s.negated {
                ' '
            } else {
                pick_in_set(s, icase, rng)
            };
            out.push(c);
        }
        Node::Group { index, inner } => {
            let start = out.len();
            walk(inner, icase, rng, groups, out, budget);
            if *index <= groups.len() {
                groups[index - 1] = Some(out[start..].to_string());
            }
        }
        Node::Concat(v) => v.iter().for_each(|x| walk(x, icase, rng, groups, out, budget)),
        Node::Alt(v) => {
            let i = rng.below(v.len() as u64) as usize;
            walk(&v[i], icase, rng, groups, out, budget);
        }
        Node::Repeat { inner, min, max } => {
            let extra = match max {
                Some(m) => (*m - *min).min(3),
                None => 3,
            };
            let times = (*min).min(40) + rng.below(extra as u64 + 1) as u32;
            for _ in 0..times {
                walk(inner, icase, rng, groups, out, budget);
            }
        }
        Node::Backref(k) => {
            if let Some(Some(text)) = groups.get(k - 1) {
                let t = text.clone();
                out.push_str(&t);
            }
        }
    }
}

/// `n` linhas geradas a partir da regex, sem `\n` nem NUL, com no máximo 200 caracteres.
pub fn samples(re: &Regex, icase: bool, seed: u64, n: usize) -> Vec<String> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::new();
    let affixes = ["", "x ", "  ", "foo: ", "[", "123"];
    for k in 0..n * 3 {
        if out.len() >= n {
            break;
        }
        let mut s = String::new();
        let mut groups = vec![None; re.groups];
        let mut budget = 400;
        walk(&re.root, icase, &mut rng, &mut groups, &mut s, &mut budget);
        let s = if k % 2 == 1 {
            let pre = affixes[rng.below(affixes.len() as u64) as usize];
            let suf = affixes[rng.below(affixes.len() as u64) as usize];
            format!("{pre}{s}{}", suf.chars().rev().collect::<String>())
        } else {
            s
        };
        let s: String = s.chars().filter(|c| *c != '\n' && *c != '\0' && *c != '\r').take(200).collect();
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Dialect, parse};

    #[test]
    fn samples_are_deterministic_and_plausible() {
        let re = parse("([0-9]{1,3}\\.){3}[0-9]{1,3}", Dialect::GrepEre).unwrap();
        let a = samples(&re, false, 7, 4);
        assert_eq!(a, samples(&re, false, 7, 4));
        assert!(a.iter().any(|s| s.matches('.').count() >= 3), "{a:?}");
        let br = parse("\\(ab\\)\\1", Dialect::GrepBre).unwrap();
        assert!(samples(&br, false, 1, 3).iter().any(|s| s.contains("abab")));
    }
}
