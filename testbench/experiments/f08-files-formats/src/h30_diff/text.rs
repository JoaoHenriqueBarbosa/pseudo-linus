//! Linhas, normalização (-i, -w, -b, --strip-trailing-cr) e internação em ids.

use std::collections::HashMap;

/// Linhas com o terminador incluído; a última pode não ter `\n` (arquivo sem newline final).
pub fn split_lines(data: &[u8]) -> Vec<&[u8]> {
    data.split_inclusive(|&b| b == b'\n').collect()
}

/// Como o GNU compara linhas.
#[derive(Clone, Copy, Debug, Default)]
pub struct Normalize {
    pub ignore_case: bool,
    pub ignore_all_space: bool,
    pub ignore_space_change: bool,
    pub strip_trailing_cr: bool,
}

impl Normalize {
    pub fn is_identity(&self) -> bool {
        !(self.ignore_case || self.ignore_all_space || self.ignore_space_change || self.strip_trailing_cr)
    }
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\x0b' | b'\x0c' | b'\r')
}

/// Chave de comparação da linha. A presença do `\n` final faz parte da chave: pro GNU, "b" sem newline e
/// "b\n" são linhas diferentes.
pub fn key(line: &[u8], n: &Normalize) -> Vec<u8> {
    let (body, newline) = match line.strip_suffix(b"\n") {
        Some(b) => (b, true),
        None => (line, false),
    };
    let mut out: Vec<u8> = body.to_vec();
    if n.strip_trailing_cr && newline && out.last() == Some(&b'\r') {
        out.pop();
    }
    if n.ignore_all_space {
        out.retain(|b| !is_space(*b));
    } else if n.ignore_space_change {
        let mut collapsed = Vec::with_capacity(out.len());
        let mut in_space = false;
        for &b in &out {
            if is_space(b) {
                in_space = true;
            } else {
                if in_space && !collapsed.is_empty() {
                    collapsed.push(b' ');
                }
                // Espaço no começo da linha também conta como "quantidade de espaço": vira um só.
                if in_space && collapsed.is_empty() {
                    collapsed.push(b' ');
                }
                in_space = false;
                collapsed.push(b);
            }
        }
        out = collapsed;
    }
    if n.ignore_case {
        out.make_ascii_lowercase();
    }
    out.push(if newline { 1 } else { 0 });
    out
}

/// Duas sequências de ids internados (mesmo id = linhas iguais sob a normalização) e uma linha
/// representante por id (usada por heurísticas de indentação).
pub struct Interned<'a> {
    pub a: Vec<u32>,
    pub b: Vec<u32>,
    pub reps: Vec<&'a [u8]>,
}

pub fn intern<'a>(a: &[&'a [u8]], b: &[&'a [u8]], n: &Normalize) -> Interned<'a> {
    let mut map: HashMap<Vec<u8>, u32> = HashMap::new();
    let mut reps: Vec<&'a [u8]> = Vec::new();
    let mut ids = |lines: &[&'a [u8]]| -> Vec<u32> {
        lines
            .iter()
            .map(|l| {
                let k = key(l, n);
                *map.entry(k).or_insert_with(|| {
                    reps.push(l);
                    (reps.len() - 1) as u32
                })
            })
            .collect()
    };
    let ia = ids(a);
    let ib = ids(b);
    Interned { a: ia, b: ib, reps }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newline_is_part_of_key() {
        let n = Normalize::default();
        assert_ne!(key(b"b", &n), key(b"b\n", &n));
        assert_eq!(key(b"b\n", &n), key(b"b\n", &n));
    }

    #[test]
    fn space_change_matches_gnu_examples() {
        let n = Normalize { ignore_space_change: true, ..Normalize::default() };
        assert_eq!(key(b"a  b\n", &n), key(b"a b\n", &n));
        assert_eq!(key(b"c \n", &n), key(b"c\n", &n));
        assert_ne!(key(b"e f\n", &n), key(b"ef\n", &n));
    }

    #[test]
    fn interning_shares_ids() {
        let a = split_lines(b"x\ny\nx\n");
        let b = split_lines(b"y\nz\n");
        let i = intern(&a, &b, &Normalize::default());
        assert_eq!(i.a, vec![0, 1, 0]);
        assert_eq!(i.b, vec![1, 2]);
        assert_eq!(i.reps.len(), 3);
    }
}
