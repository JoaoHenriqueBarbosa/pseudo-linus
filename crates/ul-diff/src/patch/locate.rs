//! Onde um hunk casa no arquivo, com o comportamento observável do GNU patch 2.8 (especificação
//! levantada no F08 e conferida no oráculo): procura a partir da posição prevista, primeiro pra frente
//! e depois pra trás em distâncias crescentes; com fuzz, ignora linhas de contexto das pontas; um hunk
//! com menos contexto antes do que depois só casa no começo do arquivo, e com menos depois só casa no
//! fim; a janela pra trás não entra no trecho já consumido por hunks anteriores.

use super::hunk::Hunk;

/// Comparação de linhas: exata, ou com `-l` (sequências de brancos casam entre si, brancos no fim
/// da linha não contam).
#[derive(Clone, Copy, Debug)]
pub struct Matcher {
    pub ignore_whitespace: bool,
}

fn is_blank(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\x0b' | b'\x0c' | b'\r')
}

fn trim_end_blank(s: &[u8]) -> &[u8] {
    let s = s.strip_suffix(b"\n").unwrap_or(s);
    let mut end = s.len();
    while end > 0 && is_blank(s[end - 1]) {
        end -= 1;
    }
    &s[..end]
}

impl Matcher {
    pub fn eq(&self, pattern: &[u8], input: &[u8]) -> bool {
        if !self.ignore_whitespace {
            return pattern == input;
        }
        let (p, q) = (trim_end_blank(pattern), trim_end_blank(input));
        let (mut i, mut j) = (0, 0);
        while i < p.len() && j < q.len() {
            if is_blank(p[i]) {
                if !is_blank(q[j]) {
                    return false;
                }
                while i < p.len() && is_blank(p[i]) {
                    i += 1;
                }
                while j < q.len() && is_blank(q[j]) {
                    j += 1;
                }
            } else if p[i] == q[j] {
                i += 1;
                j += 1;
            } else {
                return false;
            }
        }
        i == p.len() && j == q.len()
    }
}

/// Estado da procura num arquivo: quantas linhas da entrada já foram consumidas por hunks anteriores
/// e o deslocamento acumulado (a previsão do próximo hunk usa o último deslocamento achado).
#[derive(Clone, Copy, Debug, Default)]
pub struct Cursor {
    pub frozen: usize,
    pub in_offset: isize,
}

/// Procura com exatamente `fuzz` linhas de contexto ignoradas. Devolve a linha (base 1) da entrada
/// onde a primeira linha do padrão fica.
pub fn locate_level(input: &[&[u8]], hunk: &Hunk, cur: &Cursor, fuzz: usize, m: &Matcher) -> Option<usize> {
    let pattern: Vec<&[u8]> = hunk.old.iter().map(|l| l.text.as_slice()).collect();
    let n = pattern.len() as isize;
    let guess = hunk.old_first as isize + cur.in_offset;
    if n == 0 {
        return (fuzz == 0).then_some(guess.max(1) as usize);
    }
    let len = input.len() as isize;
    let prefix = hunk.prefix_context() as isize;
    let suffix = hunk.suffix_context() as isize;
    let context = prefix.max(suffix);
    let fuzz = fuzz as isize;
    if fuzz > context {
        return None;
    }
    let mut pf = fuzz + prefix - context;
    let sf = fuzz + suffix - context;
    let frozen = cur.frozen as isize;
    let matches = |pos: isize, pf: isize, sf: isize| -> bool {
        if pos < 1 || pos - 1 + n - sf > len {
            return false;
        }
        (pf..n - sf).all(|k| m.eq(pattern[k as usize], input[(pos - 1 + k) as usize]))
    };
    if pf < 0 && hunk.old_first <= 1 {
        // Só pode casar no começo do arquivo (e, sem contexto depois, no arquivo inteiro).
        if sf < 0 && n != len {
            return None;
        }
        return (frozen == 0 && matches(1, 0, sf.max(0))).then_some(1);
    }
    if pf < 0 {
        pf = 0;
    }
    if sf < 0 {
        // Só pode casar no fim do arquivo.
        let pos = len - n + 1;
        return (pos >= 1 && pos > frozen && matches(pos, pf, 0)).then_some(pos as usize);
    }
    let max_where = len - (n - sf) + 1;
    let min_where = frozen + 1 - (prefix - pf);
    let max_pos = max_where - guess;
    let mut max_neg = guess - min_where;
    if guess <= max_neg {
        max_neg = guess - 1;
    }
    let max_off = max_pos.max(max_neg);
    let mut off = 0isize;
    while off <= max_off {
        if off % 256 == 0 {
            sysabi::sys::checkpoint();
        }
        if off <= max_pos && matches(guess + off, pf, sf) {
            return Some((guess + off) as usize);
        }
        if off > 0 && off <= max_neg && matches(guess - off, pf, sf) {
            return Some((guess - off) as usize);
        }
        off += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::hunk::{Format, PLine};
    use super::*;

    fn hunk(old_first: usize, lines: &[(u8, &str)]) -> Hunk {
        let l: Vec<PLine> = lines.iter().map(|(k, t)| PLine::new(*k, t.as_bytes())).collect();
        let (old, new) = super::super::hunk::sections_from_unified(&l);
        Hunk { format: Format::Unified, old_first, old, new_first: old_first, new, func: Vec::new(), normal_cmd: 0 }
    }

    fn input(s: &str) -> Vec<&[u8]> {
        s.as_bytes().split_inclusive(|&c| c == b'\n').collect()
    }

    #[test]
    fn forward_offset_wins_ties() {
        let inp = input("x\ny\nz\nm\nx\ny\nz\n");
        let h = hunk(3, &[(b' ', "x\n"), (b'-', "y\n"), (b'+', "Y\n"), (b' ', "z\n")]);
        let m = Matcher { ignore_whitespace: false };
        assert_eq!(locate_level(&inp, &h, &Cursor::default(), 0, &m), Some(5));
    }

    #[test]
    fn fuzz_ignores_outer_context() {
        let inp = input("a\nB\nc\nd\ne\nf\ng\n");
        let h = hunk(2, &[(b' ', "b\n"), (b' ', "c\n"), (b'-', "d\n"), (b'+', "D\n"), (b' ', "e\n"), (b' ', "f\n")]);
        let m = Matcher { ignore_whitespace: false };
        assert_eq!(locate_level(&inp, &h, &Cursor::default(), 0, &m), None);
        assert_eq!(locate_level(&inp, &h, &Cursor::default(), 1, &m), Some(2));
        let _ = PLine::new(b' ', b"");
    }

    #[test]
    fn loose_whitespace() {
        let m = Matcher { ignore_whitespace: true };
        assert!(m.eq(b"x\n", b"x  \n"));
        assert!(m.eq(b"a b\n", b"a \t b\n"));
        assert!(!m.eq(b"z\n", b" z\n"));
        assert!(!m.eq(b"ab\n", b"a b\n"));
    }
}
