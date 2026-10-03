//! Formatador nosso, com as regras de saída do GNU diffutils 3.10 (script de mudanças, agrupamento de
//! hunks, faixas de linhas, "\ No newline at end of file"). Recebe o alinhamento de qualquer motor.
//!
//! Escrito a partir do comportamento documentado e observado no oráculo (formatos normal, unificado,
//! contexto e ed), sem copiar código do GNU.

use std::ops::Range;

/// Um bloco de mudança: `deleted` linhas a partir de `line0` no primeiro arquivo viram `inserted` linhas a
/// partir de `line1` no segundo (índices a partir de 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub line0: usize,
    pub line1: usize,
    pub deleted: usize,
    pub inserted: usize,
    /// Com -B: todas as linhas do bloco são em branco.
    pub ignore: bool,
}

/// Monta o script de mudanças a partir dos vetores `changed`, andando nos dois arquivos em paralelo.
pub fn build_script(c0: &[bool], c1: &[bool]) -> Vec<Change> {
    let (n0, n1) = (c0.len(), c1.len());
    let (mut i0, mut i1) = (0usize, 0usize);
    let mut out = Vec::new();
    while i0 < n0 || i1 < n1 {
        let ch0 = i0 < n0 && c0[i0];
        let ch1 = i1 < n1 && c1[i1];
        if ch0 || ch1 {
            let (l0, l1) = (i0, i1);
            while i0 < n0 && c0[i0] {
                i0 += 1;
            }
            while i1 < n1 && c1[i1] {
                i1 += 1;
            }
            out.push(Change { line0: l0, line1: l1, deleted: i0 - l0, inserted: i1 - l1, ignore: false });
        }
        i0 += 1;
        i1 += 1;
    }
    out
}

fn is_blank(line: &[u8]) -> bool {
    line == b"\n" || line.is_empty()
}

/// -B: marca como ignoráveis os blocos em que toda linha apagada e inserida é vazia.
pub fn mark_blank_changes(changes: &mut [Change], a: &[&[u8]], b: &[&[u8]]) {
    for ch in changes.iter_mut() {
        let del = &a[ch.line0..ch.line0 + ch.deleted];
        let ins = &b[ch.line1..ch.line1 + ch.inserted];
        ch.ignore = del.iter().all(|l| is_blank(l)) && ins.iter().all(|l| is_blank(l));
    }
}

/// Agrupa mudanças em hunks: a próxima mudança entra no hunk se a distância até ela é menor que
/// `2 * context + 1` linhas (ou `context`, quando ela é ignorável).
pub fn group_hunks(changes: &[Change], context: usize) -> Vec<Range<usize>> {
    let mut hunks = Vec::new();
    let mut start = 0;
    while start < changes.len() {
        let mut end = start;
        loop {
            let cur = &changes[end];
            let top0 = cur.line0 + cur.deleted;
            match changes.get(end + 1) {
                Some(next) => {
                    let thresh = if next.ignore { context } else { 2 * context + 1 };
                    if next.line0 - top0 < thresh {
                        end += 1;
                    } else {
                        break;
                    }
                }
                None => break,
            }
        }
        hunks.push(start..end + 1);
        start = end + 1;
    }
    // Hunk só com mudanças ignoráveis não é impresso.
    hunks.retain(|h| changes[h.clone()].iter().any(|c| !c.ignore));
    hunks
}

/// Imprime uma linha com prefixo; linha sem `\n` final ganha o aviso do GNU.
fn put_line(out: &mut Vec<u8>, prefix: &[u8], line: &[u8]) {
    out.extend_from_slice(prefix);
    out.extend_from_slice(line);
    if !line.ends_with(b"\n") {
        out.extend_from_slice(b"\n\\ No newline at end of file\n");
    }
}

/// Faixa no formato normal e ed: "a,b" ou só "b" (linha anterior, quando a faixa é vazia).
fn normal_range(first: isize, last: isize) -> String {
    let (ta, tb) = (first + 1, last + 1);
    if tb > ta { format!("{ta},{tb}") } else { format!("{tb}") }
}

/// Faixa do formato unificado: "a,n", "a" quando n = 1, "b,0" quando vazia.
fn unified_range(first: isize, last: isize) -> String {
    let (ta, tb) = (first + 1, last + 1);
    if tb < ta {
        format!("{tb},0")
    } else if tb == ta {
        format!("{ta}")
    } else {
        format!("{ta},{}", tb - ta + 1)
    }
}

/// Faixa do formato de contexto: "a,b" ou só "b".
fn context_range(first: isize, last: isize) -> String {
    let (ta, tb) = (first + 1, last + 1);
    if tb <= ta { format!("{tb}") } else { format!("{ta},{tb}") }
}

/// Primeira e última linha de cada arquivo cobertas pelas mudanças do hunk, e se há linhas apagadas
/// (OLD) e inseridas (NEW) não ignoráveis.
fn analyze(hunk: &[Change]) -> (isize, isize, isize, isize, bool, bool) {
    let first = hunk[0];
    let last = hunk[hunk.len() - 1];
    let first0 = first.line0 as isize;
    let first1 = first.line1 as isize;
    let last0 = (last.line0 + last.deleted) as isize - 1;
    let last1 = (last.line1 + last.inserted) as isize - 1;
    let old = hunk.iter().any(|c| c.deleted > 0 && !c.ignore);
    let new = hunk.iter().any(|c| c.inserted > 0 && !c.ignore);
    (first0, last0, first1, last1, old, new)
}

pub fn format_normal(out: &mut Vec<u8>, changes: &[Change], a: &[&[u8]], b: &[&[u8]]) {
    for ch in changes.iter().filter(|c| !c.ignore) {
        let (f0, l0) = (ch.line0 as isize, (ch.line0 + ch.deleted) as isize - 1);
        let (f1, l1) = (ch.line1 as isize, (ch.line1 + ch.inserted) as isize - 1);
        let letter = match (ch.deleted > 0, ch.inserted > 0) {
            (true, true) => 'c',
            (true, false) => 'd',
            (false, true) => 'a',
            (false, false) => continue,
        };
        out.extend_from_slice(format!("{}{letter}{}\n", normal_range(f0, l0), normal_range(f1, l1)).as_bytes());
        for line in &a[ch.line0..ch.line0 + ch.deleted] {
            put_line(out, b"< ", line);
        }
        if letter == 'c' {
            out.extend_from_slice(b"---\n");
        }
        for line in &b[ch.line1..ch.line1 + ch.inserted] {
            put_line(out, b"> ", line);
        }
    }
}

/// Corpo do formato unificado (sem as duas linhas de cabeçalho).
pub fn format_unified(out: &mut Vec<u8>, changes: &[Change], a: &[&[u8]], b: &[&[u8]], context: usize) {
    let ctx = context as isize;
    for range in group_hunks(changes, context) {
        let hunk = &changes[range];
        let (first0, last0, first1, last1, _, _) = analyze(hunk);
        let first0c = (first0 - ctx).max(0);
        let first1c = (first1 - ctx).max(0);
        let len0 = a.len() as isize;
        let len1 = b.len() as isize;
        let last0c = if last0 < len0 - ctx { last0 + ctx } else { len0 - 1 };
        let last1c = if last1 < len1 - ctx { last1 + ctx } else { len1 - 1 };
        out.extend_from_slice(
            format!("@@ -{} +{} @@\n", unified_range(first0c, last0c), unified_range(first1c, last1c)).as_bytes(),
        );
        let mut next = 0usize;
        let (mut i, mut j) = (first0c, first1c);
        while i <= last0c || j <= last1c {
            let cur = hunk.get(next);
            if cur.is_none_or(|c| i < c.line0 as isize) {
                put_line(out, b" ", a[i as usize]);
                i += 1;
                j += 1;
            } else {
                let c = cur.expect("mudança");
                for _ in 0..c.deleted {
                    put_line(out, b"-", a[i as usize]);
                    i += 1;
                }
                for _ in 0..c.inserted {
                    put_line(out, b"+", b[j as usize]);
                    j += 1;
                }
                next += 1;
            }
        }
    }
}

/// Corpo do formato de contexto (sem as duas linhas de cabeçalho).
pub fn format_context(out: &mut Vec<u8>, changes: &[Change], a: &[&[u8]], b: &[&[u8]], context: usize) {
    let ctx = context as isize;
    for range in group_hunks(changes, context) {
        let hunk = &changes[range];
        let (first0, last0, first1, last1, old, new) = analyze(hunk);
        let first0c = (first0 - ctx).max(0);
        let first1c = (first1 - ctx).max(0);
        let len0 = a.len() as isize;
        let len1 = b.len() as isize;
        let last0c = if last0 < len0 - ctx { last0 + ctx } else { len0 - 1 };
        let last1c = if last1 < len1 - ctx { last1 + ctx } else { len1 - 1 };
        out.extend_from_slice(b"***************\n");
        out.extend_from_slice(format!("*** {} ****\n", context_range(first0c, last0c)).as_bytes());
        if old {
            let mut next = 0usize;
            for i in first0c..=last0c {
                while next < hunk.len() && (hunk[next].line0 + hunk[next].deleted) as isize <= i {
                    next += 1;
                }
                let prefix: &[u8] = match hunk.get(next) {
                    Some(c) if c.line0 as isize <= i => {
                        if c.inserted > 0 { b"! " } else { b"- " }
                    }
                    _ => b"  ",
                };
                put_line(out, prefix, a[i as usize]);
            }
        }
        out.extend_from_slice(format!("--- {} ----\n", context_range(first1c, last1c)).as_bytes());
        if new {
            let mut next = 0usize;
            for j in first1c..=last1c {
                while next < hunk.len() && (hunk[next].line1 + hunk[next].inserted) as isize <= j {
                    next += 1;
                }
                let prefix: &[u8] = match hunk.get(next) {
                    Some(c) if c.line1 as isize <= j => {
                        if c.deleted > 0 { b"! " } else { b"+ " }
                    }
                    _ => b"  ",
                };
                put_line(out, prefix, b[j as usize]);
            }
        }
    }
}

/// Formato ed (-e): mudanças da última pra primeira, comandos a/c/d.
pub fn format_ed(out: &mut Vec<u8>, changes: &[Change], b: &[&[u8]]) {
    for ch in changes.iter().rev().filter(|c| !c.ignore) {
        let (f0, l0) = (ch.line0 as isize, (ch.line0 + ch.deleted) as isize - 1);
        let letter = match (ch.deleted > 0, ch.inserted > 0) {
            (true, true) => 'c',
            (true, false) => 'd',
            (false, true) => 'a',
            (false, false) => continue,
        };
        out.extend_from_slice(format!("{}{letter}\n", normal_range(f0, l0)).as_bytes());
        if ch.inserted > 0 {
            for line in &b[ch.line1..ch.line1 + ch.inserted] {
                out.extend_from_slice(line);
                if !line.ends_with(b"\n") {
                    out.push(b'\n');
                }
            }
            out.extend_from_slice(b".\n");
        }
    }
}

/// Larguras do side-by-side: meia coluna e início da coluna da direita, com tab de 8 e calha mínima de 3.
fn sdiff_columns(width: usize) -> (usize, usize) {
    let t = 8usize;
    let off = (width + t + 3) / (2 * t) * t;
    let half = off.saturating_sub(3).min(width.saturating_sub(off));
    let col2 = if half < off { off } else { width };
    (half, col2)
}

/// Avança de `from` até `to` com tabs (enquanto couber) e espaços.
fn tab_from_to(out: &mut Vec<u8>, mut from: usize, to: usize) -> usize {
    let tab = 8usize;
    let mut stop = from + tab - from % tab;
    while stop <= to {
        out.push(b'\t');
        from = stop;
        stop += tab;
    }
    while from < to {
        out.push(b' ');
        from += 1;
    }
    to
}

/// Imprime até `bound` colunas da linha (sem o `\n`), contando tabs a partir de `indent`.
fn half_line(out: &mut Vec<u8>, line: &[u8], indent: usize, bound: usize) -> usize {
    let (mut inpos, mut outpos) = (0usize, 0usize);
    let body = line.strip_suffix(b"\n").unwrap_or(line);
    let text = String::from_utf8_lossy(body);
    let mut bytes_iter = body.iter();
    for ch in text.chars() {
        let len = ch.len_utf8();
        let raw: Vec<u8> = (0..len).filter_map(|_| bytes_iter.next().copied()).collect();
        match ch {
            '\t' => {
                let spaces = 8 - ((inpos + indent) % 8);
                if inpos == outpos {
                    let stop = outpos + spaces;
                    if stop < bound {
                        outpos = stop;
                        out.push(b'\t');
                    }
                }
                inpos += spaces;
            }
            '\r' => {
                out.push(b'\r');
                tab_from_to(out, 0, indent);
                inpos = 0;
                outpos = 0;
            }
            _ => {
                if inpos < bound {
                    inpos += 1;
                    outpos = inpos;
                    out.extend_from_slice(&raw);
                } else {
                    inpos += 1;
                }
            }
        }
    }
    outpos
}

fn sdiff_line(out: &mut Vec<u8>, left: Option<&[u8]>, sep: u8, right: Option<&[u8]>, width: usize) {
    let (hw, c2o) = sdiff_columns(width);
    let mut col = 0usize;
    let mut put_newline = false;
    if let Some(l) = left {
        put_newline |= l.ends_with(b"\n");
        col = half_line(out, l, 0, hw);
    }
    let mut sep = sep;
    if sep != b' ' {
        col = tab_from_to(out, col, (hw + c2o - 1) / 2) + 1;
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
            col = tab_from_to(out, col, c2o);
            half_line(out, r, col, hw);
        }
    }
    if put_newline {
        out.push(b'\n');
    }
}

/// Formato lado a lado (-y), com `--suppress-common-lines` e largura `-W`.
pub fn format_side_by_side(
    out: &mut Vec<u8>,
    changes: &[Change],
    a: &[&[u8]],
    b: &[&[u8]],
    width: usize,
    suppress_common: bool,
) {
    let (mut i, mut j) = (0usize, 0usize);
    let common = |out: &mut Vec<u8>, i: &mut usize, j: &mut usize, upto0: usize, upto1: usize| {
        while *i < upto0 && *j < upto1 {
            if !suppress_common {
                sdiff_line(out, Some(a[*i]), b' ', Some(b[*j]), width);
            }
            *i += 1;
            *j += 1;
        }
    };
    for ch in changes {
        common(out, &mut i, &mut j, ch.line0, ch.line1);
        let (mut x, mut y) = (ch.line0, ch.line1);
        let (end0, end1) = (ch.line0 + ch.deleted, ch.line1 + ch.inserted);
        while x < end0 && y < end1 {
            sdiff_line(out, Some(a[x]), b'|', Some(b[y]), width);
            x += 1;
            y += 1;
        }
        while y < end1 {
            sdiff_line(out, None, b'>', Some(b[y]), width);
            y += 1;
        }
        while x < end0 {
            sdiff_line(out, Some(a[x]), b'<', None, width);
            x += 1;
        }
        i = end0;
        j = end1;
    }
    common(out, &mut i, &mut j, a.len(), b.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_by_side_matches_gnu_columns() {
        let a = lines("apple\nbanana\ncherry\ndate\n");
        let b = lines("apple\nblueberry\ncherry\nelderberry\n");
        let s = build_script(&[false, true, false, true], &[false, true, false, true]);
        let mut out = Vec::new();
        format_side_by_side(&mut out, &s, &a, &b, 40, false);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "apple\t\t\tapple\nbanana\t\t   |\tblueberry\ncherry\t\t\tcherry\ndate\t\t   |\telderberry\n"
        );
    }

    fn lines(s: &str) -> Vec<&[u8]> {
        crate::h30_diff::text::split_lines(s.as_bytes())
    }

    #[test]
    fn script_and_ranges() {
        let c0 = [false, true, false];
        let c1 = [false, true, true, false];
        let s = build_script(&c0, &c1);
        assert_eq!(s, vec![Change { line0: 1, line1: 1, deleted: 1, inserted: 2, ignore: false }]);
        assert_eq!(unified_range(0, -1), "0,0");
        assert_eq!(unified_range(0, 0), "1");
        assert_eq!(unified_range(0, 2), "1,3");
        assert_eq!(normal_range(3, 2), "3");
        assert_eq!(context_range(0, -1), "0");
    }

    #[test]
    fn normal_and_unified_match_gnu_shapes() {
        let a = lines("one\ntwo\nthree\n");
        let b = lines("one\nTWO\nthree\n");
        let s = build_script(&[false, true, false], &[false, true, false]);
        let mut out = Vec::new();
        format_normal(&mut out, &s, &a, &b);
        assert_eq!(String::from_utf8(out).unwrap(), "2c2\n< two\n---\n> TWO\n");
        let mut out = Vec::new();
        format_unified(&mut out, &s, &a, &b, 3);
        assert_eq!(String::from_utf8(out).unwrap(), "@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n");
    }

    #[test]
    fn missing_newline_marker() {
        let a = lines("a\nb");
        let b = lines("a\nb\n");
        let s = build_script(&[false, true], &[false, true]);
        let mut out = Vec::new();
        format_unified(&mut out, &s, &a, &b, 3);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+b\n"
        );
    }

    #[test]
    fn hunks_merge_at_gap_of_twice_context() {
        let s = vec![
            Change { line0: 4, line1: 4, deleted: 1, inserted: 1, ignore: false },
            Change { line0: 11, line1: 11, deleted: 1, inserted: 1, ignore: false },
        ];
        assert_eq!(group_hunks(&s, 3).len(), 1);
        let s2 = vec![s[0], Change { line0: 12, line1: 12, deleted: 1, inserted: 1, ignore: false }];
        assert_eq!(group_hunks(&s2, 3).len(), 2);
    }
}
