//! Saída `-D NOME` e os formatos de grupo e de linha (`--old-group-format`, `--line-format`...), com a
//! linguagem de diretivas documentada no manual do diffutils: `%<`, `%>`, `%=`, `%%`, `%c'C'`,
//! `%c'\OOO'`, `%(A=B?T:E)`, `%[-][largura][.[precisão]]{doxX}LETRA` (F, L, N, E, M do grupo novo e
//! f, l, n, e, m do velho) e, nos formatos de linha, `%L`, `%l` e `%[...]n`.

use super::format::Change;

/// Os formatos em uso. `None` = padrão.
#[derive(Clone, Debug, Default)]
pub struct Formats {
    pub old_group: Option<Vec<u8>>,
    pub new_group: Option<Vec<u8>>,
    pub changed_group: Option<Vec<u8>>,
    pub unchanged_group: Option<Vec<u8>>,
    pub old_line: Option<Vec<u8>>,
    pub new_line: Option<Vec<u8>>,
    pub unchanged_line: Option<Vec<u8>>,
    /// Nome do `-D`.
    pub ifdef: Option<Vec<u8>>,
}

impl Formats {
    fn group(&self, kind: Kind) -> Vec<u8> {
        let explicit = match kind {
            Kind::Old => &self.old_group,
            Kind::New => &self.new_group,
            Kind::Changed => &self.changed_group,
            Kind::Unchanged => &self.unchanged_group,
        };
        if let Some(f) = explicit {
            return f.clone();
        }
        match (&self.ifdef, kind) {
            (Some(name), Kind::Old) => [b"#ifndef ", name.as_slice(), b"\n%<#endif /* ! ", name, b" */\n"].concat(),
            (Some(name), Kind::New) => [b"#ifdef ", name.as_slice(), b"\n%>#endif /* ", name, b" */\n"].concat(),
            (Some(name), Kind::Changed) => [
                b"#ifndef ",
                name.as_slice(),
                b"\n%<#else /* ",
                name,
                b" */\n%>#endif /* ",
                name,
                b" */\n",
            ]
            .concat(),
            (_, Kind::Unchanged) => b"%=".to_vec(),
            (None, Kind::Old) => b"%<".to_vec(),
            (None, Kind::New) => b"%>".to_vec(),
            (None, Kind::Changed) => b"%<%>".to_vec(),
        }
    }

    fn line(&self, which: Which) -> Vec<u8> {
        let f = match which {
            Which::Old => &self.old_line,
            Which::New => &self.new_line,
            Which::Unchanged => &self.unchanged_line,
        };
        f.clone().unwrap_or_else(|| b"%l\n".to_vec())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Old,
    New,
    Changed,
    Unchanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Which {
    Old,
    New,
    Unchanged,
}

/// Um grupo: faixas (0-based, fim exclusivo) nos dois arquivos.
#[derive(Clone, Copy, Debug)]
struct Group {
    a: (usize, usize),
    b: (usize, usize),
}

struct Ctx<'a> {
    fmts: &'a Formats,
    a: &'a [&'a [u8]],
    b: &'a [&'a [u8]],
}

/// Imprime o arquivo fundido inteiro. Diretiva inválida sai literal (`%Q` imprime `%Q`), como no GNU.
pub fn format_ifdef(out: &mut Vec<u8>, changes: &[Change], a: &[&[u8]], b: &[&[u8]], fmts: &Formats) {
    let ctx = Ctx { fmts, a, b };
    let (mut next0, mut next1) = (0usize, 0usize);
    for (k, ch) in changes.iter().enumerate() {
        if k % 256 == 0 {
            sysabi::sys::checkpoint();
        }
        if ch.ignore {
            continue;
        }
        let (end0, end1) = (ch.line0 + ch.deleted, ch.line1 + ch.inserted);
        if next0 < ch.line0 || next1 < ch.line1 {
            let g = Group { a: (next0, ch.line0), b: (next1, ch.line1) };
            ctx.group_format(out, &fmts.group(Kind::Unchanged), g);
        }
        let kind = match (ch.deleted > 0, ch.inserted > 0) {
            (true, true) => Kind::Changed,
            (true, false) => Kind::Old,
            (false, true) => Kind::New,
            (false, false) => continue,
        };
        let g = Group { a: (ch.line0, end0), b: (ch.line1, end1) };
        ctx.group_format(out, &fmts.group(kind), g);
        next0 = end0;
        next1 = end1;
    }
    if next0 < a.len() || next1 < b.len() {
        let g = Group { a: (next0, a.len()), b: (next1, b.len()) };
        ctx.group_format(out, &fmts.group(Kind::Unchanged), g);
    }
}

/// Lê um especificador printf `[-][largura][.[precisão]]` seguido de `d`, `o`, `x` ou `X`. Devolve o
/// especificador e quantos bytes consumiu.
fn parse_spec(f: &[u8]) -> Option<(Spec, usize)> {
    let mut i = 0;
    let mut spec = Spec::default();
    while i < f.len() && matches!(f[i], b'-' | b'0' | b' ' | b'+' | b'#' | b'\'') {
        match f[i] {
            b'-' => spec.left = true,
            b'0' => spec.zero = true,
            b' ' => spec.space = true,
            b'+' => spec.plus = true,
            b'#' => spec.alt = true,
            _ => {}
        }
        i += 1;
    }
    let mut width = 0usize;
    while i < f.len() && f[i].is_ascii_digit() {
        width = width.saturating_mul(10).saturating_add((f[i] - b'0') as usize);
        i += 1;
    }
    spec.width = width;
    if f.get(i) == Some(&b'.') {
        i += 1;
        let mut p = 0usize;
        while i < f.len() && f[i].is_ascii_digit() {
            p = p.saturating_mul(10).saturating_add((f[i] - b'0') as usize);
            i += 1;
        }
        spec.precision = Some(p);
    }
    let conv = *f.get(i)?;
    if !matches!(conv, b'd' | b'o' | b'x' | b'X') {
        return None;
    }
    spec.conv = conv;
    Some((spec, i + 1))
}

#[derive(Clone, Copy, Debug, Default)]
struct Spec {
    left: bool,
    zero: bool,
    space: bool,
    plus: bool,
    alt: bool,
    width: usize,
    precision: Option<usize>,
    conv: u8,
}

impl Spec {
    fn render(&self, v: i64) -> String {
        let neg = v < 0;
        let mag = v.unsigned_abs();
        let mut digits = match self.conv {
            b'o' => format!("{mag:o}"),
            b'x' => format!("{mag:x}"),
            b'X' => format!("{mag:X}"),
            _ => format!("{mag}"),
        };
        if let Some(p) = self.precision {
            if p == 0 && mag == 0 {
                digits.clear();
            }
            while digits.len() < p {
                digits.insert(0, '0');
            }
        }
        if self.alt {
            match self.conv {
                b'o' if !digits.starts_with('0') => digits.insert(0, '0'),
                b'x' if mag != 0 => digits.insert_str(0, "0x"),
                b'X' if mag != 0 => digits.insert_str(0, "0X"),
                _ => {}
            }
        }
        let sign = if neg {
            "-"
        } else if self.plus && self.conv == b'd' {
            "+"
        } else if self.space && self.conv == b'd' {
            " "
        } else {
            ""
        };
        let body_len = sign.len() + digits.len();
        if body_len >= self.width {
            return format!("{sign}{digits}");
        }
        let pad = self.width - body_len;
        if self.left {
            format!("{sign}{digits}{}", " ".repeat(pad))
        } else if self.zero && self.precision.is_none() {
            format!("{sign}{}{digits}", "0".repeat(pad))
        } else {
            format!("{}{sign}{digits}", " ".repeat(pad))
        }
    }
}

/// `%c'C'` e `%c'\OOO'`: devolve o byte e o comprimento a partir do `'`.
fn parse_char(f: &[u8]) -> Option<(u8, usize)> {
    if f.first() != Some(&b'\'') {
        return None;
    }
    if f.get(1) == Some(&b'\\') {
        let mut i = 2;
        let mut v: u32 = 0;
        let mut n = 0;
        while i < f.len() && n < 3 && (b'0'..=b'7').contains(&f[i]) {
            v = v * 8 + (f[i] - b'0') as u32;
            i += 1;
            n += 1;
        }
        if n == 0 || f.get(i) != Some(&b'\'') {
            return None;
        }
        return Some(((v & 0xff) as u8, i + 1));
    }
    let c = *f.get(1)?;
    if f.get(2) != Some(&b'\'') {
        return None;
    }
    Some((c, 3))
}

impl Ctx<'_> {
    /// Valor de uma letra de número de linha no grupo (1-based, como o manual define).
    fn letter_value(&self, g: Group, c: u8) -> Option<i64> {
        let (range, upper) = match c {
            b'F' | b'L' | b'N' | b'E' | b'M' => (g.b, true),
            b'f' | b'l' | b'n' | b'e' | b'm' => (g.a, false),
            _ => return None,
        };
        let first = range.0 as i64 + 1;
        let last = range.1 as i64;
        let v = match c.to_ascii_uppercase() {
            b'F' => first,
            b'L' => last,
            b'N' => last - first + 1,
            b'E' => first - 1,
            b'M' => last + 1,
            _ => return None,
        };
        let _ = upper;
        Some(v)
    }

    /// Operando de `%(A=B?T:E)`: número decimal ou letra.
    fn operand(&self, f: &[u8], g: Group) -> Option<(i64, usize)> {
        let mut i = 0;
        if f.first().is_some_and(|c| c.is_ascii_digit()) {
            let mut v: i64 = 0;
            while i < f.len() && f[i].is_ascii_digit() {
                v = v.saturating_mul(10).saturating_add((f[i] - b'0') as i64);
                i += 1;
            }
            return Some((v, i));
        }
        let c = *f.first()?;
        self.letter_value(g, c).map(|v| (v, 1))
    }

    /// Acha o fim de um formato aninhado (até `stop` no mesmo nível), respeitando `%(...)` e `%c'...'`.
    fn scan(f: &[u8], stop: &[u8]) -> Option<usize> {
        let mut i = 0;
        while i < f.len() {
            if stop.contains(&f[i]) {
                return Some(i);
            }
            if f[i] == b'%' {
                i += 1;
                match f.get(i) {
                    Some(b'(') => {
                        // pula até o ')' correspondente
                        let mut depth = 1;
                        i += 1;
                        while i < f.len() && depth > 0 {
                            if f[i] == b'%' {
                                i += 1;
                                if f.get(i) == Some(&b'(') {
                                    depth += 1;
                                } else if f.get(i) == Some(&b'c') && f.get(i + 1) == Some(&b'\'')
                                    && let Some((_, n)) = parse_char(&f[i + 1..]) {
                                        i += n;
                                    }
                            } else if f[i] == b')' {
                                depth -= 1;
                            }
                            i += 1;
                        }
                        continue;
                    }
                    Some(b'c') => {
                        if let Some((_, n)) = parse_char(&f[i + 1..]) {
                            i += 1 + n;
                            continue;
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
        }
        None
    }

    /// Uma diretiva de grupo a partir de `f[i]` (o caractere depois do `%`). Devolve quantos bytes ela
    /// ocupa, ou `None` se não é diretiva válida (o GNU então imprime o `%` literal e segue).
    fn group_directive(&self, out: &mut Vec<u8>, f: &[u8], i: usize, g: Group) -> Option<usize> {
        let d = *f.get(i)?;
        match d {
            b'<' => {
                self.lines(out, Which::Old, g.a);
                Some(1)
            }
            b'>' => {
                self.lines(out, Which::New, g.b);
                Some(1)
            }
            b'=' => {
                self.lines(out, Which::Unchanged, g.a);
                Some(1)
            }
            b'%' => {
                out.push(b'%');
                Some(1)
            }
            b'c' => {
                let (ch, n) = parse_char(&f[i + 1..])?;
                out.push(ch);
                Some(1 + n)
            }
            b'(' => {
                let mut k = i + 1;
                let (va, n) = self.operand(&f[k..], g)?;
                k += n;
                if f.get(k) != Some(&b'=') {
                    return None;
                }
                k += 1;
                let (vb, n) = self.operand(&f[k..], g)?;
                k += n;
                if f.get(k) != Some(&b'?') {
                    return None;
                }
                k += 1;
                let then_len = Self::scan(&f[k..], b":")?;
                let then_part = &f[k..k + then_len];
                k += then_len + 1;
                let else_len = Self::scan(&f[k..], b")")?;
                let else_part = &f[k..k + else_len];
                k += else_len + 1;
                let chosen = if va == vb { then_part } else { else_part };
                self.group_format(out, chosen, g);
                Some(k - i)
            }
            _ => {
                let (spec, n) = parse_spec(&f[i..])?;
                let letter = *f.get(i + n)?;
                let v = self.letter_value(g, letter)?;
                out.extend_from_slice(spec.render(v).as_bytes());
                Some(n + 1)
            }
        }
    }

    fn group_format(&self, out: &mut Vec<u8>, f: &[u8], g: Group) {
        let mut i = 0;
        while i < f.len() {
            let c = f[i];
            i += 1;
            if c != b'%' {
                out.push(c);
                continue;
            }
            let mut tmp = Vec::new();
            match self.group_directive(&mut tmp, f, i, g) {
                Some(n) => {
                    out.extend_from_slice(&tmp);
                    i += n;
                }
                None => out.push(b'%'),
            }
        }
    }

    fn lines(&self, out: &mut Vec<u8>, which: Which, range: (usize, usize)) {
        let fmt = self.fmts.line(which);
        let file = if which == Which::New { self.b } else { self.a };
        for (k, line) in file[range.0..range.1].iter().enumerate() {
            self.line_format(out, &fmt, line, range.0 + k + 1);
        }
    }

    fn line_directive(out: &mut Vec<u8>, f: &[u8], i: usize, line: &[u8], number: usize) -> Option<usize> {
        let d = *f.get(i)?;
        match d {
            b'L' => {
                out.extend_from_slice(line);
                Some(1)
            }
            b'l' => {
                out.extend_from_slice(line.strip_suffix(b"\n").unwrap_or(line));
                Some(1)
            }
            b'%' => {
                out.push(b'%');
                Some(1)
            }
            b'c' => {
                let (ch, n) = parse_char(&f[i + 1..])?;
                out.push(ch);
                Some(1 + n)
            }
            _ => {
                let (spec, n) = parse_spec(&f[i..])?;
                if f.get(i + n) != Some(&b'n') {
                    return None;
                }
                out.extend_from_slice(spec.render(number as i64).as_bytes());
                Some(n + 1)
            }
        }
    }

    fn line_format(&self, out: &mut Vec<u8>, f: &[u8], line: &[u8], number: usize) {
        let mut i = 0;
        while i < f.len() {
            let c = f[i];
            i += 1;
            if c != b'%' {
                out.push(c);
                continue;
            }
            match Self::line_directive(out, f, i, line, number) {
                Some(n) => i += n,
                None => out.push(b'%'),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::format::build_script;

    fn lines(s: &str) -> Vec<&[u8]> {
        s.as_bytes().split_inclusive(|&c| c == b'\n').collect()
    }

    #[test]
    fn ifdef_like_gnu() {
        let a = lines("a\nb\nc\nd\ne\nf\n");
        let b = lines("a\nB\nc\ne\nf\ng\nh\n");
        let s = build_script(&[false, true, false, true, false, false], &[false, true, false, false, false, true, true]);
        let fmts = Formats { ifdef: Some(b"FOO".to_vec()), ..Formats::default() };
        let mut out = Vec::new();
        format_ifdef(&mut out, &s, &a, &b, &fmts);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "a\n#ifndef FOO\nb\n#else /* FOO */\nB\n#endif /* FOO */\nc\n#ifndef FOO\nd\n#endif /* ! FOO */\ne\nf\n#ifdef FOO\ng\nh\n#endif /* FOO */\n"
        );
    }

    #[test]
    fn group_directives() {
        let a = lines("a\nb\nc\nd\ne\nf\n");
        let b = lines("a\nB\nc\ne\nf\ng\nh\n");
        let s = build_script(&[false, true, false, true, false, false], &[false, true, false, false, false, true, true]);
        let fmts = Formats {
            old_group: Some(b"OLD %df-%dl (%dN)\n%<".to_vec()),
            new_group: Some(b"NEW %dF-%dL %(N=1?one:many)\n%>".to_vec()),
            changed_group: Some(b"CH %de %dE %dm %dM %c'X'%c'\\101'\n%<--\n%>".to_vec()),
            unchanged_group: Some(Vec::new()),
            ..Formats::default()
        };
        let mut out = Vec::new();
        format_ifdef(&mut out, &s, &a, &b, &fmts);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "CH 1 1 3 3 XA\nb\n--\nB\nOLD 4-4 (0)\nd\nNEW 6-7 many\ng\nh\n"
        );
        let mut out = Vec::new();
        let lf = Formats {
            old_line: Some(b"L%dn:%L".to_vec()),
            new_line: Some(b"L%dn:%L".to_vec()),
            unchanged_line: Some(b"L%dn:%L".to_vec()),
            ..Formats::default()
        };
        format_ifdef(&mut out, &s, &a, &b, &lf);
        assert_eq!(String::from_utf8(out).unwrap(), "L1:a\nL2:b\nL2:B\nL3:c\nL4:d\nL5:e\nL6:f\nL6:g\nL7:h\n");
    }
}
