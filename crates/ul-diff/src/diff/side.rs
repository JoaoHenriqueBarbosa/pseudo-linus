//! Formato lado a lado (`diff -y`, e a saída do `sdiff`), com as larguras, tabulações e separadores do
//! GNU diffutils 3.10: `-W`, `--left-column`, `--suppress-common-lines`, `-t`, `--tabsize`, separadores
//! ` `, `|`, `<`, `>`, `(`, `)`, `/` e `\` (quando só um dos lados termina em newline), e larguras de
//! caractere do C.UTF-8 (UTF-8 inválido e controle ocupam zero colunas).

use unicode_width::UnicodeWidthChar;

use super::format::Change;

/// Calha mínima entre as colunas.
const GUTTER: usize = 3;

/// Geometria das colunas.
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub half: usize,
    pub col2: usize,
    pub tabsize: usize,
    pub expand_tabs: bool,
}

impl Geometry {
    pub fn new(width: usize, tabsize: usize, expand_tabs: bool) -> Geometry {
        let tabsize = tabsize.max(1);
        let t = if expand_tabs { 1 } else { tabsize };
        let off = (width + t + GUTTER) / (2 * t) * t;
        let half = off.saturating_sub(GUTTER).min(width.saturating_sub(off));
        let col2 = if half > 0 { off } else { width };
        Geometry { half, col2, tabsize, expand_tabs }
    }

    /// Avança de `from` até `to` com tabs (enquanto couber e sem `-t`) e espaços.
    fn tab_from_to(&self, out: &mut Vec<u8>, mut from: usize, to: usize) -> usize {
        let tab = self.tabsize;
        if !self.expand_tabs {
            loop {
                let stop = from + tab - from % tab;
                if stop > to {
                    break;
                }
                out.push(b'\t');
                from = stop;
            }
        }
        while from < to {
            out.push(b' ');
            from += 1;
        }
        to.max(from)
    }

    /// Imprime até `bound` colunas da linha (sem o `\n`), contando tabs a partir de `indent`. Devolve a
    /// coluna em que a saída parou.
    fn half_line(&self, out: &mut Vec<u8>, line: &[u8], indent: usize, bound: usize) -> usize {
        let (mut inpos, mut outpos) = (0usize, 0usize);
        let body = line.strip_suffix(b"\n").unwrap_or(line);
        let mut k = 0usize;
        while k < body.len() {
            let c = body[k];
            match c {
                b'\t' => {
                    // As paradas de tab contam a partir do começo da meia linha (sem `-t` a coluna da
                    // direita já começa numa parada, então dá no mesmo).
                    let spaces = self.tabsize - (inpos % self.tabsize);
                    if inpos == outpos {
                        let mut stop = outpos + spaces;
                        if self.expand_tabs {
                            if bound < stop {
                                stop = bound;
                            }
                            while outpos < stop {
                                out.push(b' ');
                                outpos += 1;
                            }
                        } else if stop < bound {
                            outpos = stop;
                            out.push(b'\t');
                        }
                    }
                    inpos += spaces;
                    k += 1;
                }
                b'\r' => {
                    out.push(b'\r');
                    self.tab_from_to(out, 0, indent);
                    inpos = 0;
                    outpos = 0;
                    k += 1;
                }
                0x08 => {
                    if inpos != 0 {
                        inpos -= 1;
                        if inpos < bound {
                            if outpos <= inpos {
                                while outpos < inpos {
                                    out.push(b' ');
                                    outpos += 1;
                                }
                            } else {
                                outpos = inpos;
                                out.push(c);
                            }
                        }
                    }
                    k += 1;
                }
                _ => match decode_utf8(&body[k..]) {
                    Some((ch, len)) => {
                        let w = ch.width().unwrap_or(0);
                        inpos += w;
                        if inpos <= bound {
                            outpos = inpos;
                            out.extend_from_slice(&body[k..k + len]);
                        }
                        k += len;
                    }
                    None => {
                        // Byte inválido: sai sem ocupar coluna, enquanto houver espaço.
                        if inpos < bound {
                            out.push(c);
                        }
                        k += 1;
                    }
                },
            }
        }
        outpos
    }

    /// Uma linha da saída: lado esquerdo, separador, lado direito.
    pub fn line(&self, out: &mut Vec<u8>, left: Option<&[u8]>, sep: u8, right: Option<&[u8]>) {
        let (hw, c2o) = (self.half, self.col2);
        let mut col = 0usize;
        let mut put_newline = false;
        if let Some(l) = left {
            put_newline |= l.ends_with(b"\n");
            col = self.half_line(out, l, 0, hw);
        }
        let mut sep = sep;
        if sep != b' ' {
            col = self.tab_from_to(out, col, (hw + c2o).saturating_sub(1) / 2) + 1;
            if let (b'|', Some(r)) = (sep, right)
                && put_newline != r.ends_with(b"\n")
            {
                sep = if put_newline { b'/' } else { b'\\' };
            }
            out.push(sep);
        }
        if let Some(r) = right {
            put_newline |= r.ends_with(b"\n");
            if r != b"\n" && !r.is_empty() {
                col = self.tab_from_to(out, col, c2o);
                self.half_line(out, r, col, hw);
            }
        }
        if put_newline {
            out.push(b'\n');
        }
    }
}

/// Decodifica um caractere UTF-8 no começo de `s`.
fn decode_utf8(s: &[u8]) -> Option<(char, usize)> {
    let first = *s.first()?;
    let len = match first {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    let chunk = s.get(..len)?;
    let st = std::str::from_utf8(chunk).ok()?;
    st.chars().next().map(|c| (c, len))
}

/// Opções do lado a lado.
#[derive(Clone, Copy, Debug)]
pub struct SideOptions {
    pub width: usize,
    pub tabsize: usize,
    pub expand_tabs: bool,
    pub left_column: bool,
    pub suppress_common: bool,
}

/// Saída lado a lado de um par inteiro. `changes` já tem as mudanças ignoráveis marcadas: elas saem
/// como linhas comuns emparelhadas (e as sobras com `)` e `(`), como o GNU.
pub fn format_side_by_side(out: &mut Vec<u8>, changes: &[Change], a: &[&[u8]], b: &[&[u8]], o: &SideOptions) {
    let g = Geometry::new(o.width, o.tabsize, o.expand_tabs);
    let (mut next0, mut next1) = (0usize, 0usize);
    let common = |out: &mut Vec<u8>, next0: &mut usize, next1: &mut usize, lim0: usize, lim1: usize| {
        let (mut i0, mut i1) = (*next0, *next1);
        if !o.suppress_common && (i0 != lim0 || i1 != lim1) {
            while i0 != lim0 && i1 != lim1 {
                if o.left_column {
                    g.line(out, Some(a[i0]), b'(', None);
                } else {
                    g.line(out, Some(a[i0]), b' ', Some(b[i1]));
                }
                i0 += 1;
                i1 += 1;
            }
            while i1 != lim1 {
                g.line(out, None, b')', Some(b[i1]));
                i1 += 1;
            }
            while i0 != lim0 {
                g.line(out, Some(a[i0]), b'(', None);
                i0 += 1;
            }
        }
        *next0 = lim0;
        *next1 = lim1;
    };
    for (k, ch) in changes.iter().enumerate() {
        if k % 256 == 0 {
            sysabi::sys::checkpoint();
        }
        if ch.ignore || (ch.deleted == 0 && ch.inserted == 0) {
            continue;
        }
        common(out, &mut next0, &mut next1, ch.line0, ch.line1);
        let (mut i, mut j) = (ch.line0, ch.line1);
        let (end0, end1) = (ch.line0 + ch.deleted, ch.line1 + ch.inserted);
        if ch.deleted > 0 && ch.inserted > 0 {
            while i < end0 && j < end1 {
                g.line(out, Some(a[i]), b'|', Some(b[j]));
                i += 1;
                j += 1;
            }
        }
        while j < end1 {
            g.line(out, None, b'>', Some(b[j]));
            j += 1;
        }
        while i < end0 {
            g.line(out, Some(a[i]), b'<', None);
            i += 1;
        }
        next0 = end0;
        next1 = end1;
    }
    common(out, &mut next0, &mut next1, a.len(), b.len());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::format::build_script;

    fn lines(s: &str) -> Vec<&[u8]> {
        s.as_bytes().split_inclusive(|&c| c == b'\n').collect()
    }

    fn opts(width: usize) -> SideOptions {
        SideOptions { width, tabsize: 8, expand_tabs: false, left_column: false, suppress_common: false }
    }

    #[test]
    fn side_by_side_matches_gnu_columns() {
        let a = lines("apple\nbanana\ncherry\ndate\n");
        let b = lines("apple\nblueberry\ncherry\nelderberry\n");
        let s = build_script(&[false, true, false, true], &[false, true, false, true]);
        let mut out = Vec::new();
        format_side_by_side(&mut out, &s, &a, &b, &opts(40));
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "apple\t\t\tapple\nbanana\t\t   |\tblueberry\ncherry\t\t\tcherry\ndate\t\t   |\telderberry\n"
        );
    }

    #[test]
    fn wide_characters_and_left_column() {
        let a = lines("日本語の文字列はとても長いです\n");
        let b = lines("x\n");
        let s = build_script(&[true], &[true]);
        let mut out = Vec::new();
        format_side_by_side(&mut out, &s, &a, &b, &opts(30));
        assert_eq!(String::from_utf8(out).unwrap(), "日本語の文字  |\tx\n");
        let a = lines("a\nb\n");
        let b = lines("a\nB\n");
        let s = build_script(&[false, true], &[false, true]);
        let mut out = Vec::new();
        let o = SideOptions { left_column: true, ..opts(30) };
        format_side_by_side(&mut out, &s, &a, &b, &o);
        assert_eq!(String::from_utf8(out).unwrap(), "a\t      (\nb\t      |\tB\n");
    }
}
