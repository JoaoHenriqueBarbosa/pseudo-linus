//! Linhas, normalização das comparações (`-i`, `-E`, `-Z`, `-b`, `-w`, `--strip-trailing-cr`) e
//! internação em classes de equivalência.
//!
//! Regras observadas no GNU diffutils 3.10: com `-b`, `-w` e `-Z` a falta de newline no fim do arquivo
//! não conta como diferença (o newline é espaço em branco no fim da linha); com `-i` e `-E` conta. O
//! `--strip-trailing-cr` age na leitura: o CR some também da saída.

use std::collections::HashMap;

/// Linhas com o terminador incluído; a última pode não ter `\n` (arquivo sem newline final).
pub fn split_lines(data: &[u8]) -> Vec<&[u8]> {
    data.split_inclusive(|&b| b == b'\n').collect()
}

/// Remove o CR antes do `\n` de cada linha (`--strip-trailing-cr`).
pub fn strip_trailing_cr(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\r' && data.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(data[i]);
        i += 1;
    }
    out
}

/// Como o GNU compara linhas.
#[derive(Clone, Copy, Debug)]
pub struct Normalize {
    pub ignore_case: bool,
    pub ignore_all_space: bool,
    pub ignore_space_change: bool,
    pub ignore_trailing_space: bool,
    pub ignore_tab_expansion: bool,
    pub tabsize: usize,
}

impl Default for Normalize {
    fn default() -> Self {
        Normalize {
            ignore_case: false,
            ignore_all_space: false,
            ignore_space_change: false,
            ignore_trailing_space: false,
            ignore_tab_expansion: false,
            tabsize: 8,
        }
    }
}

impl Normalize {
    pub fn is_identity(&self) -> bool {
        !(self.ignore_case
            || self.ignore_all_space
            || self.ignore_space_change
            || self.ignore_trailing_space
            || self.ignore_tab_expansion)
    }

    /// Se o newline no fim da linha é tratado como espaço em branco (e a falta dele deixa de contar).
    fn newline_is_space(&self) -> bool {
        self.ignore_all_space || self.ignore_space_change || self.ignore_trailing_space
    }
}

/// `isspace` do locale C, sem o `\n` (que é o terminador).
pub fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\x0b' | b'\x0c' | b'\r')
}

fn expand_tabs(body: &[u8], tabsize: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut col = 0usize;
    for &b in body {
        match b {
            b'\t' => {
                let n = tabsize - col % tabsize;
                out.extend(std::iter::repeat_n(b' ', n));
                col += n;
            }
            b'\x08' => {
                out.push(b);
                col = col.saturating_sub(1);
            }
            b'\r' => {
                out.push(b);
                col = 0;
            }
            _ => {
                out.push(b);
                col += 1;
            }
        }
    }
    out
}

/// Chave de comparação da linha sob a normalização. A presença do `\n` final faz parte da chave
/// quando a normalização não trata o newline como espaço.
pub fn key(line: &[u8], n: &Normalize) -> Vec<u8> {
    let (body, newline) = match line.strip_suffix(b"\n") {
        Some(b) => (b, true),
        None => (line, false),
    };
    if n.is_identity() {
        let mut out = body.to_vec();
        out.push(newline as u8);
        return out;
    }
    let mut out: Vec<u8> = if n.ignore_tab_expansion && !n.ignore_all_space && !n.ignore_space_change {
        expand_tabs(body, n.tabsize.max(1))
    } else {
        body.to_vec()
    };
    if n.ignore_all_space {
        out.retain(|b| !is_space(*b));
    } else if n.ignore_space_change {
        let mut collapsed = Vec::with_capacity(out.len());
        let mut in_space = false;
        for &b in &out {
            if is_space(b) {
                in_space = true;
            } else {
                if in_space {
                    collapsed.push(b' ');
                }
                in_space = false;
                collapsed.push(b);
            }
        }
        out = collapsed;
    } else if n.ignore_trailing_space {
        while out.last().is_some_and(|b| is_space(*b)) {
            out.pop();
        }
    }
    if n.ignore_case {
        out.make_ascii_lowercase();
    }
    if !n.newline_is_space() {
        out.push(newline as u8);
    } else {
        out.push(2);
    }
    out
}

/// Classes de equivalência dos dois arquivos (mesma classe = linhas iguais sob a normalização).
pub struct Interned {
    pub a: Vec<u32>,
    pub b: Vec<u32>,
    pub classes: usize,
}

pub fn intern(a: &[&[u8]], b: &[&[u8]], n: &Normalize) -> Interned {
    let mut map: HashMap<Vec<u8>, u32> = HashMap::new();
    let mut next = 0u32;
    let mut ids = |lines: &[&[u8]], map: &mut HashMap<Vec<u8>, u32>| -> Vec<u32> {
        let mut v = Vec::with_capacity(lines.len());
        for (i, l) in lines.iter().enumerate() {
            if i % 4096 == 0 {
                sysabi::sys::checkpoint();
            }
            let k = key(l, n);
            let id = *map.entry(k).or_insert_with(|| {
                next += 1;
                next - 1
            });
            v.push(id);
        }
        v
    };
    let ia = ids(a, &mut map);
    let ib = ids(b, &mut map);
    Interned { a: ia, b: ib, classes: next as usize }
}

/// Classes de igualdade exata (byte a byte, com o terminador), usadas pra achar o prefixo e o sufixo
/// comuns como o GNU faz antes da comparação.
pub fn exact_equal(a: &[u8], b: &[u8]) -> bool {
    a == b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newline_rules_match_gnu() {
        let n = Normalize::default();
        assert_ne!(key(b"b", &n), key(b"b\n", &n));
        let w = Normalize { ignore_all_space: true, ..Normalize::default() };
        assert_eq!(key(b"b", &w), key(b"b\n", &w));
        let i = Normalize { ignore_case: true, ..Normalize::default() };
        assert_ne!(key(b"b", &i), key(b"B\n", &i));
    }

    #[test]
    fn space_change_matches_gnu_examples() {
        let n = Normalize { ignore_space_change: true, ..Normalize::default() };
        assert_eq!(key(b"a  b\n", &n), key(b"a b\n", &n));
        assert_eq!(key(b"c \n", &n), key(b"c\n", &n));
        assert_ne!(key(b"e f\n", &n), key(b"ef\n", &n));
        assert_ne!(key(b" a\n", &n), key(b"a\n", &n));
        assert_eq!(key(b"x\r\n", &n), key(b"x\n", &n));
    }

    #[test]
    fn tab_expansion() {
        let n = Normalize { ignore_tab_expansion: true, ..Normalize::default() };
        assert_eq!(key(b"a\tb\n", &n), key(b"a       b\n", &n));
        assert_ne!(key(b"a\tb\n", &n), key(b"a b\n", &n));
    }

    #[test]
    fn strip_cr() {
        assert_eq!(strip_trailing_cr(b"a\r\nb\rc\r\n"), b"a\nb\rc\n");
    }
}
