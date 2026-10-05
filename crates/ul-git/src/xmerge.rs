//! Mesclagem de três vias de arquivos de texto, linha a linha (o `xdl_merge` do xdiff): as mudanças
//! da base para cada lado, a união das que não se tocam, os marcadores de conflito
//! (`<<<<<<< nome`, `|||||||`, `=======`, `>>>>>>> nome`), o refinamento "zealous" dos conflitos e a
//! preferência `ours`/`theirs`/`union`.

use crate::diff::text::{self, Change};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Favor {
    None,
    Ours,
    Theirs,
    Union,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Style {
    Merge,
    Diff3,
    ZealousDiff3,
}

pub struct Params<'a> {
    pub name1: &'a str,
    pub name2: &'a str,
    pub ancestor: &'a str,
    pub favor: Favor,
    pub style: Style,
    pub marker_size: usize,
}

impl Default for Params<'_> {
    fn default() -> Self {
        Params { name1: "", name2: "", ancestor: "", favor: Favor::None, style: Style::Merge, marker_size: 7 }
    }
}

pub struct Merged {
    pub data: Vec<u8>,
    /// Quantos conflitos ficaram no resultado.
    pub conflicts: usize,
}

/// Um trecho do resultado (o `xdmerge_t`): `mode` 0 é conflito, 1 pega o lado 1, 2 o lado 2, 3 os
/// dois e 4 é conflito que o refinamento viu ser idêntico dos dois lados.
#[derive(Clone, Copy, Debug)]
struct Hunk {
    mode: u8,
    i0: i64,
    chg0: i64,
    i1: i64,
    chg1: i64,
    i2: i64,
    chg2: i64,
}

const LEVEL_MINIMAL: u8 = 0;
const LEVEL_EAGER: u8 = 1;
const LEVEL_ZEALOUS: u8 = 2;

fn append_merge(list: &mut Vec<Hunk>, mode: u8, h: (i64, i64, i64, i64, i64, i64)) {
    let (i0, chg0, i1, chg1, i2, chg2) = h;
    if let Some(m) = list.last_mut()
        && (i1 <= m.i1 + m.chg1 || i2 <= m.i2 + m.chg2)
    {
        if mode != m.mode {
            m.mode = 0;
        }
        m.chg0 = i0 + chg0 - m.i0;
        m.chg1 = i1 + chg1 - m.i1;
        m.chg2 = i2 + chg2 - m.i2;
        return;
    }
    list.push(Hunk { mode, i0, chg0, i1, chg1, i2, chg2 });
}

fn recs_copy(out: &mut Vec<u8>, recs: &[&[u8]], i: i64, count: i64, needs_cr: bool, add_nl: bool) {
    if count < 1 {
        return;
    }
    let (i, count) = (i as usize, count as usize);
    for r in &recs[i..i + count] {
        out.extend_from_slice(r);
    }
    if add_nl {
        let last = recs[i + count - 1];
        if last.last() != Some(&b'\n') {
            if needs_cr {
                out.push(b'\r');
            }
            out.push(b'\n');
        }
    }
}

/// 1 se a linha `i` termina em CR/LF, 0 se em LF só, -1 se não dá pra saber.
fn is_eol_crlf(file: &[&[u8]], i: i64) -> i32 {
    let nrec = file.len() as i64;
    if i < nrec - 1 {
        let r = file[i as usize];
        return i32::from(r.len() > 1 && r[r.len() - 2] == b'\r');
    }
    if nrec == 0 {
        return -1;
    }
    let r = file[i as usize];
    if r.last() == Some(&b'\n') {
        return i32::from(r.len() > 1 && r[r.len() - 2] == b'\r');
    }
    if i == 0 {
        return -1;
    }
    let r = file[i as usize - 1];
    i32::from(r.len() > 1 && r[r.len() - 2] == b'\r')
}

fn is_cr_needed(l0: &[&[u8]], l1: &[&[u8]], l2: &[&[u8]], m: &Hunk) -> bool {
    let mut n = is_eol_crlf(l1, if m.i1 != 0 { m.i1 - 1 } else { 0 });
    if n != 0 {
        n = is_eol_crlf(l2, if m.i2 != 0 { m.i2 - 1 } else { 0 });
    }
    if n != 0 {
        n = is_eol_crlf(l0, 0);
    }
    n > 0
}

fn marker(out: &mut Vec<u8>, c: u8, size: usize, name: Option<&str>, needs_cr: bool) {
    out.extend(std::iter::repeat_n(c, size));
    if let Some(n) = name {
        out.push(b' ');
        out.extend_from_slice(n.as_bytes());
    }
    if needs_cr {
        out.push(b'\r');
    }
    out.push(b'\n');
}

struct Files<'a> {
    l0: &'a [&'a [u8]],
    l1: &'a [&'a [u8]],
    l2: &'a [&'a [u8]],
}

fn fill_conflict_hunk(out: &mut Vec<u8>, f: &Files<'_>, p: &Params<'_>, i: i64, m: &Hunk) {
    let needs_cr = is_cr_needed(f.l0, f.l1, f.l2, m);
    let size = if p.marker_size == 0 { 7 } else { p.marker_size };
    recs_copy(out, f.l1, i, m.i1 - i, false, false);
    marker(out, b'<', size, Some(p.name1).filter(|n| !n.is_empty()), needs_cr);
    recs_copy(out, f.l1, m.i1, m.chg1, needs_cr, true);
    if matches!(p.style, Style::Diff3 | Style::ZealousDiff3) {
        marker(out, b'|', size, Some(p.ancestor).filter(|n| !n.is_empty()), needs_cr);
        recs_copy(out, f.l0, m.i0, m.chg0, needs_cr, true);
    }
    marker(out, b'=', size, None, needs_cr);
    recs_copy(out, f.l2, m.i2, m.chg2, needs_cr, true);
    marker(out, b'>', size, Some(p.name2).filter(|n| !n.is_empty()), needs_cr);
}

fn fill_merge_buffer(f: &Files<'_>, p: &Params<'_>, list: &mut [Hunk]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i: i64 = 0;
    for m in list.iter_mut() {
        if p.favor != Favor::None && m.mode == 0 {
            m.mode = match p.favor {
                Favor::Ours => 1,
                Favor::Theirs => 2,
                _ => 3,
            };
        }
        if m.mode == 0 {
            fill_conflict_hunk(&mut out, f, p, i, m);
        } else if m.mode & 3 != 0 {
            recs_copy(&mut out, f.l1, i, m.i1 - i, false, false);
            if m.mode & 1 != 0 {
                let needs_cr = is_cr_needed(f.l0, f.l1, f.l2, m);
                recs_copy(&mut out, f.l1, m.i1, m.chg1, needs_cr, m.mode & 2 != 0);
            }
            if m.mode & 2 != 0 {
                recs_copy(&mut out, f.l2, m.i2, m.chg2, false, false);
            }
        } else {
            continue;
        }
        i = m.i1 + m.chg1;
    }
    recs_copy(&mut out, f.l1, i, f.l1.len() as i64 - i, false, false);
    out
}

/// Tira do começo e do fim do conflito as linhas que os dois lados têm iguais (`zdiff3`).
fn refine_zdiff3_conflicts(f: &Files<'_>, list: &mut [Hunk]) {
    for m in list.iter_mut() {
        if m.mode != 0 {
            continue;
        }
        while m.chg1 != 0 && m.chg2 != 0 && f.l1[m.i1 as usize] == f.l2[m.i2 as usize] {
            m.chg1 -= 1;
            m.chg2 -= 1;
            m.i1 += 1;
            m.i2 += 1;
        }
        while m.chg1 != 0 && m.chg2 != 0 && f.l1[(m.i1 + m.chg1 - 1) as usize] == f.l2[(m.i2 + m.chg2 - 1) as usize] {
            m.chg1 -= 1;
            m.chg2 -= 1;
        }
    }
}

/// Mudanças parecidas mas não idênticas: o conflito encolhe pras linhas que de fato diferem.
fn refine_conflicts(f: &Files<'_>, list: Vec<Hunk>) -> Vec<Hunk> {
    let mut out: Vec<Hunk> = Vec::with_capacity(list.len());
    for m in list {
        if m.mode != 0 || m.chg1 == 0 || m.chg2 == 0 {
            out.push(m);
            continue;
        }
        let a = &f.l1[m.i1 as usize..(m.i1 + m.chg1) as usize];
        let b = &f.l2[m.i2 as usize..(m.i2 + m.chg2) as usize];
        let script = text::changes_plain(a, b);
        if script.is_empty() {
            // As mudanças são idênticas.
            out.push(Hunk { mode: 4, ..m });
            continue;
        }
        for (k, x) in script.iter().enumerate() {
            let mut h = if k == 0 { m } else { Hunk { mode: 0, ..m } };
            h.mode = 0;
            h.i1 = x.i1 as i64 + m.i1;
            h.chg1 = x.chg1 as i64;
            h.i2 = x.i2 as i64 + m.i2;
            h.chg2 = x.chg2 as i64;
            out.push(h);
        }
    }
    out
}

fn line_has_alnum(l: &[u8]) -> bool {
    l.iter().any(|c| c.is_ascii_alphanumeric())
}

/// Com menos de 3 linhas intactas entre dois conflitos, elas entram nos conflitos.
fn simplify_non_conflicts(f: &Files<'_>, list: &mut Vec<Hunk>, simplify_if_no_alnum: bool) {
    let mut k = 0;
    while k + 1 < list.len() {
        let (m, next) = (list[k], list[k + 1]);
        let begin = m.i1 + m.chg1;
        let end = next.i1;
        let keep = m.mode != 0
            || next.mode != 0
            || (end - begin > 3 && (!simplify_if_no_alnum || (begin..end).any(|i| line_has_alnum(f.l1[i as usize]))));
        if keep {
            k += 1;
        } else {
            list[k].chg1 = next.i1 + next.chg1 - m.i1;
            list[k].chg2 = next.i2 + next.chg2 - m.i2;
            list.remove(k + 1);
        }
    }
}

fn do_merge(f: &Files<'_>, s1: &[Change], s2: &[Change], p: &Params<'_>) -> Merged {
    let mut level = LEVEL_ZEALOUS;
    if matches!(p.style, Style::Diff3 | Style::ZealousDiff3) {
        level = level.min(LEVEL_EAGER);
    }
    let (n0, n1, n2) = (f.l0.len() as i64, f.l1.len() as i64, f.l2.len() as i64);
    let cv = |c: &Change| (c.i1 as i64, c.chg1 as i64, c.i2 as i64, c.chg2 as i64);
    let mut changes: Vec<Hunk> = Vec::new();
    let (mut a, mut b) = (0usize, 0usize);
    while a < s1.len() && b < s2.len() {
        let (x1i1, x1c1, x1i2, x1c2) = cv(&s1[a]);
        let (x2i1, x2c1, x2i2, x2c2) = cv(&s2[b]);
        if x1i1 + x1c1 < x2i1 {
            append_merge(&mut changes, 1, (x1i1, x1c1, x1i2, x1c2, x2i2 - x2i1 + x1i1, x1c1));
            a += 1;
            continue;
        }
        if x2i1 + x2c1 < x1i1 {
            append_merge(&mut changes, 2, (x2i1, x2c1, x1i2 - x1i1 + x2i1, x2c1, x2i2, x2c2));
            b += 1;
            continue;
        }
        let same_text = x1c2 == x2c2 && (0..x1c2).all(|k| f.l1[(x1i2 + k) as usize] == f.l2[(x2i2 + k) as usize]);
        if level == LEVEL_MINIMAL || x1i1 != x2i1 || x1c1 != x2c1 || x1c2 != x2c2 || !same_text {
            // Conflito.
            let off = x1i1 - x2i1;
            let ffo = off + x1c1 - x2c1;
            let mut i0 = x1i1;
            let mut i1 = x1i2;
            let mut i2 = x2i2;
            if off > 0 {
                i0 -= off;
                i1 -= off;
            } else {
                i2 += off;
            }
            let mut chg0 = x1i1 + x1c1 - i0;
            let mut chg1 = x1i2 + x1c2 - i1;
            let mut chg2 = x2i2 + x2c2 - i2;
            if ffo < 0 {
                chg0 -= ffo;
                chg1 -= ffo;
            } else {
                chg2 += ffo;
            }
            append_merge(&mut changes, 0, (i0, chg0, i1, chg1, i2, chg2));
        }
        let e1 = x1i1 + x1c1;
        let e2 = x2i1 + x2c1;
        if e1 >= e2 {
            b += 1;
        }
        if e2 >= e1 {
            a += 1;
        }
    }
    while a < s1.len() {
        let (i1_, c1, i2_, c2) = cv(&s1[a]);
        append_merge(&mut changes, 1, (i1_, c1, i2_, c2, i1_ + n2 - n0, c1));
        a += 1;
    }
    while b < s2.len() {
        let (i1_, c1, i2_, c2) = cv(&s2[b]);
        append_merge(&mut changes, 2, (i1_, c1, i1_ + n1 - n0, c1, i2_, c2));
        b += 1;
    }
    if p.style == Style::ZealousDiff3 {
        refine_zdiff3_conflicts(f, &mut changes);
    } else if level >= LEVEL_ZEALOUS {
        changes = refine_conflicts(f, changes);
        simplify_non_conflicts(f, &mut changes, level > LEVEL_ZEALOUS);
    }
    let data = fill_merge_buffer(f, p, &mut changes);
    let conflicts = changes.iter().filter(|m| m.mode == 0).count();
    Merged { data, conflicts }
}

/// `xdl_merge`: `orig` é a base, `mf1` o nosso lado e `mf2` o deles.
pub fn merge(orig: &[u8], mf1: &[u8], mf2: &[u8], p: &Params<'_>) -> Merged {
    let l0 = text::split_lines(orig);
    let l1 = text::split_lines(mf1);
    let l2 = text::split_lines(mf2);
    let s1 = text::changes_plain(&l0, &l1);
    let s2 = text::changes_plain(&l0, &l2);
    if s1.is_empty() {
        return Merged { data: mf2.to_vec(), conflicts: 0 };
    }
    if s2.is_empty() {
        return Merged { data: mf1.to_vec(), conflicts: 0 };
    }
    let f = Files { l0: &l0, l1: &l1, l2: &l2 };
    do_merge(&f, &s1, &s2, p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(a: &str, b: &str, c: &str) -> (String, usize) {
        let p = Params { name1: "HEAD", name2: "other", ..Params::default() };
        let m = merge(a.as_bytes(), b.as_bytes(), c.as_bytes(), &p);
        (String::from_utf8(m.data).unwrap(), m.conflicts)
    }

    #[test]
    fn clean_and_conflict() {
        assert_eq!(run("a\nb\nc\n", "A\nb\nc\n", "a\nb\nC\n"), ("A\nb\nC\n".to_string(), 0));
        assert_eq!(
            run("a\nb\nc\n", "a\nX\nc\n", "a\nY\nc\n"),
            ("a\n<<<<<<< HEAD\nX\n=======\nY\n>>>>>>> other\nc\n".to_string(), 1)
        );
    }
}
