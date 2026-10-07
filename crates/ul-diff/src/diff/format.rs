//! Formatadores de saída com as regras do GNU diffutils 3.10: script de mudanças, agrupamento de
//! hunks, faixas de linhas, "\ No newline at end of file", prefixos com `-T` e
//! `--suppress-blank-empty`, `-t`, cores, e os formatos normal, unificado, contexto, ed (`-e` e `-f`) e
//! RCS (`-n`). O lado a lado está em `side.rs` e o `-D`/formatos de grupo em `ifdef.rs`.
//!
//! Escrito a partir do comportamento documentado e observado no oráculo, sem copiar código do GNU.

use std::ops::{Range, RangeInclusive};

/// Um bloco de mudança: `deleted` linhas a partir de `line0` no primeiro arquivo viram `inserted` linhas a
/// partir de `line1` no segundo (índices a partir de 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub line0: usize,
    pub line1: usize,
    pub deleted: usize,
    pub inserted: usize,
    /// Com `-B`/`-I`: o bloco inteiro é ignorável.
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

/// Agrupa mudanças em hunks: a próxima mudança entra no hunk se a distância até ela é menor que
/// `2 * context + 1` linhas (ou `context`, quando ela é ignorável). Hunks só de mudanças ignoráveis
/// somem.
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
    hunks.retain(|h| changes[h.clone()].iter().any(|c| !c.ignore));
    hunks
}

/// Cores do `--color` (sequências SGR sem o `\x1b[` e o `m`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    pub reset: String,
    pub header: String,
    pub add: String,
    pub delete: String,
    pub line: String,
}

impl Default for Palette {
    fn default() -> Self {
        Palette { reset: "0".into(), header: "1".into(), add: "32".into(), delete: "31".into(), line: "36".into() }
    }
}

impl Palette {
    /// Aplica um `--palette` (`ad=1;32:de=1;31:...`) por cima do padrão. Chaves desconhecidas são
    /// ignoradas, como no GNU.
    pub fn apply(&mut self, spec: &str) {
        for item in spec.split(':') {
            let Some((k, v)) = item.split_once('=') else { continue };
            let slot = match k {
                "rs" => &mut self.reset,
                "hd" => &mut self.header,
                "ad" => &mut self.add,
                "de" => &mut self.delete,
                "ln" => &mut self.line,
                _ => continue,
            };
            *slot = v.to_string();
        }
    }
}

/// Tipo de linha pra cor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    None,
    Header,
    Add,
    Delete,
    Line,
}

/// Opções de apresentação comuns aos formatos.
#[derive(Clone, Debug)]
pub struct Look {
    pub initial_tab: bool,
    pub suppress_blank_empty: bool,
    pub expand_tabs: bool,
    pub tabsize: usize,
    pub color: Option<Palette>,
}

impl Default for Look {
    fn default() -> Self {
        Look { initial_tab: false, suppress_blank_empty: false, expand_tabs: false, tabsize: 8, color: None }
    }
}

/// Prefixos de uma linha: (normal, com `-T`, com `--suppress-blank-empty` numa linha vazia).
#[derive(Clone, Copy)]
pub struct Prefix {
    pub plain: &'static str,
    pub tab: &'static str,
    pub blank: &'static str,
}

pub const NORMAL_OLD: Prefix = Prefix { plain: "< ", tab: "<\t", blank: "<" };
pub const NORMAL_NEW: Prefix = Prefix { plain: "> ", tab: ">\t", blank: ">" };
pub const UNI_CONTEXT: Prefix = Prefix { plain: " ", tab: "\t", blank: "" };
pub const UNI_OLD: Prefix = Prefix { plain: "-", tab: "-\t", blank: "-" };
pub const UNI_NEW: Prefix = Prefix { plain: "+", tab: "+\t", blank: "+" };
pub const CTX_CONTEXT: Prefix = Prefix { plain: "  ", tab: " \t", blank: "" };
pub const CTX_CHANGED: Prefix = Prefix { plain: "! ", tab: "!\t", blank: "!" };
pub const CTX_OLD: Prefix = Prefix { plain: "- ", tab: "-\t", blank: "-" };
pub const CTX_NEW: Prefix = Prefix { plain: "+ ", tab: "+\t", blank: "+" };

/// Escritor de saída de um par de arquivos, com as opções de apresentação.
pub struct Printer<'a> {
    pub out: &'a mut Vec<u8>,
    pub look: &'a Look,
}

impl Printer<'_> {
    fn begin(&mut self, paint: Paint) -> bool {
        let Some(p) = &self.look.color else { return false };
        let code = match paint {
            Paint::None => return false,
            Paint::Header => &p.header,
            Paint::Add => &p.add,
            Paint::Delete => &p.delete,
            Paint::Line => &p.line,
        };
        self.out.extend_from_slice(b"\x1b[");
        self.out.extend_from_slice(code.as_bytes());
        self.out.push(b'm');
        true
    }

    fn end(&mut self) {
        if let Some(p) = &self.look.color {
            self.out.extend_from_slice(b"\x1b[");
            self.out.extend_from_slice(p.reset.as_bytes());
            self.out.push(b'm');
        }
    }

    /// Linha de controle (cabeçalho, faixa), com `\n` no fim e a cor fechada antes dele.
    pub fn control(&mut self, text: &str, paint: Paint) {
        let colored = self.begin(paint);
        self.out.extend_from_slice(text.as_bytes());
        if colored {
            self.end();
        }
        self.out.push(b'\n');
    }

    /// Linha de controle em bytes (rótulos podem não ser UTF-8).
    pub fn control_bytes(&mut self, text: &[u8], paint: Paint) {
        let colored = self.begin(paint);
        self.out.extend_from_slice(text);
        if colored {
            self.end();
        }
        self.out.push(b'\n');
    }

    /// Texto da linha sem o `\n`, com `-t` aplicado.
    fn body(&mut self, line: &[u8]) {
        let body = line.strip_suffix(b"\n").unwrap_or(line);
        if !self.look.expand_tabs {
            self.out.extend_from_slice(body);
            return;
        }
        let ts = self.look.tabsize.max(1);
        let mut col = 0usize;
        for &b in body {
            match b {
                b'\t' => {
                    let n = ts - col % ts;
                    self.out.extend(std::iter::repeat_n(b' ', n));
                    col += n;
                }
                b'\x08' => {
                    self.out.push(b);
                    col = col.saturating_sub(1);
                }
                b'\r' => {
                    self.out.push(b);
                    col = 0;
                }
                _ => {
                    self.out.push(b);
                    col += 1;
                }
            }
        }
    }

    /// Uma linha do arquivo com prefixo. Linha sem `\n` final ganha o aviso do GNU.
    pub fn line(&mut self, prefix: Prefix, line: &[u8], paint: Paint) {
        let empty = line == b"\n" || line.is_empty();
        let pre = if self.look.suppress_blank_empty && empty {
            prefix.blank
        } else if self.look.initial_tab {
            prefix.tab
        } else {
            prefix.plain
        };
        let colored = self.begin(paint);
        self.out.extend_from_slice(pre.as_bytes());
        self.body(line);
        if colored {
            self.end();
        }
        self.out.push(b'\n');
        if !line.ends_with(b"\n") {
            self.out.extend_from_slice(b"\\ No newline at end of file\n");
        }
    }
}

/// Faixa nos formatos normal, ed e de contexto: "a,b" ou só "b" (a linha anterior, quando a faixa
/// é vazia).
pub fn line_range(first: isize, last: isize) -> String {
    let (ta, tb) = (first + 1, last + 1);
    if tb > ta { format!("{ta},{tb}") } else { format!("{tb}") }
}

/// Faixa do formato unificado: "a,n", "a" quando n = 1, "b,0" quando vazia.
pub fn unified_range(first: isize, last: isize) -> String {
    let (ta, tb) = (first + 1, last + 1);
    if tb < ta {
        format!("{tb},0")
    } else if tb == ta {
        format!("{ta}")
    } else {
        format!("{ta},{}", tb - ta + 1)
    }
}

/// Primeira e última linha de cada arquivo cobertas pelas mudanças do hunk, e se há linhas apagadas e
/// inseridas não ignoráveis.
pub fn analyze(hunk: &[Change]) -> (isize, isize, isize, isize, bool, bool) {
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

fn letter(ch: &Change) -> Option<char> {
    match (ch.deleted > 0, ch.inserted > 0) {
        (true, true) => Some('c'),
        (true, false) => Some('d'),
        (false, true) => Some('a'),
        (false, false) => None,
    }
}

pub fn format_normal(p: &mut Printer<'_>, changes: &[Change], a: &[&[u8]], b: &[&[u8]]) {
    for ch in changes.iter().filter(|c| !c.ignore) {
        let Some(letter) = letter(ch) else { continue };
        let (f0, l0) = (ch.line0 as isize, (ch.line0 + ch.deleted) as isize - 1);
        let (f1, l1) = (ch.line1 as isize, (ch.line1 + ch.inserted) as isize - 1);
        p.control(&format!("{}{letter}{}", line_range(f0, l0), line_range(f1, l1)), Paint::Line);
        for line in &a[ch.line0..ch.line0 + ch.deleted] {
            p.line(NORMAL_OLD, line, Paint::Delete);
        }
        if letter == 'c' {
            p.control("---", Paint::None);
        }
        for line in &b[ch.line1..ch.line1 + ch.inserted] {
            p.line(NORMAL_NEW, line, Paint::Add);
        }
    }
}

fn hunk_bounds(hunk: &[Change], na: usize, nb: usize, context: usize) -> (isize, isize, isize, isize) {
    let ctx = context as isize;
    let (first0, last0, first1, last1, _, _) = analyze(hunk);
    let first0c = (first0 - ctx).max(0);
    let first1c = (first1 - ctx).max(0);
    let (len0, len1) = (na as isize, nb as isize);
    let last0c = if last0 < len0 - ctx { last0 + ctx } else { len0 - 1 };
    let last1c = if last1 < len1 - ctx { last1 + ctx } else { len1 - 1 };
    (first0c, last0c, first1c, last1c)
}

/// Corpo do formato unificado (sem as duas linhas de cabeçalho).
pub fn format_unified(p: &mut Printer<'_>, changes: &[Change], a: &[&[u8]], b: &[&[u8]], context: usize) {
    for range in group_hunks(changes, context) {
        let hunk = &changes[range];
        let (first0c, last0c, first1c, last1c) = hunk_bounds(hunk, a.len(), b.len(), context);
        p.control(&format!("@@ -{} +{} @@", unified_range(first0c, last0c), unified_range(first1c, last1c)), Paint::Line);
        let mut next = 0usize;
        let (mut i, mut j) = (first0c, first1c);
        while i <= last0c || j <= last1c {
            let cur = hunk.get(next);
            if cur.is_none_or(|c| i < c.line0 as isize) {
                p.line(UNI_CONTEXT, a[i as usize], Paint::None);
                i += 1;
                j += 1;
            } else {
                let c = cur.expect("mudança");
                for _ in 0..c.deleted {
                    p.line(UNI_OLD, a[i as usize], Paint::Delete);
                    i += 1;
                }
                for _ in 0..c.inserted {
                    p.line(UNI_NEW, b[j as usize], Paint::Add);
                    j += 1;
                }
                next += 1;
            }
        }
    }
}

/// Corpo do formato de contexto (sem as duas linhas de cabeçalho).
pub fn format_context(p: &mut Printer<'_>, changes: &[Change], a: &[&[u8]], b: &[&[u8]], context: usize) {
    for range in group_hunks(changes, context) {
        let hunk = &changes[range];
        let (_, _, _, _, old, new) = analyze(hunk);
        let (first0c, last0c, first1c, last1c) = hunk_bounds(hunk, a.len(), b.len(), context);
        p.control("***************", Paint::None);
        p.control(&format!("*** {} ****", line_range(first0c, last0c)), Paint::Line);
        if old {
            context_side(p, hunk, a, first0c..=last0c, Side::Old);
        }
        p.control(&format!("--- {} ----", line_range(first1c, last1c)), Paint::Line);
        if new {
            context_side(p, hunk, b, first1c..=last1c, Side::New);
        }
    }
}

/// Um dos dois arquivos de um diff de contexto.
#[derive(Clone, Copy)]
enum Side {
    Old,
    New,
}

impl Side {
    /// Onde a mudança começa neste arquivo, quantas linhas ela tem aqui e quantas no outro.
    fn span(self, c: &Change) -> (usize, usize, usize) {
        match self {
            Side::Old => (c.line0, c.deleted, c.inserted),
            Side::New => (c.line1, c.inserted, c.deleted),
        }
    }
}

/// As linhas `range` de um dos lados de um bloco de contexto: `!` na mudança com os dois lados,
/// `-` ou `+` na que só tem este, e o contexto com dois espaços.
fn context_side(p: &mut Printer<'_>, hunk: &[Change], lines: &[&[u8]], range: RangeInclusive<isize>, side: Side) {
    let (own, paint) = match side {
        Side::Old => (CTX_OLD, Paint::Delete),
        Side::New => (CTX_NEW, Paint::Add),
    };
    let mut next = 0usize;
    for i in range {
        while hunk.get(next).is_some_and(|c| {
            let (start, here, _) = side.span(c);
            (start + here) as isize <= i
        }) {
            next += 1;
        }
        let prefix = match hunk.get(next).map(|c| side.span(c)) {
            Some((start, _, other)) if start as isize <= i => {
                if other > 0 {
                    CTX_CHANGED
                } else {
                    own
                }
            }
            _ => CTX_CONTEXT,
        };
        p.line(prefix, lines[i as usize], paint);
    }
}

/// Formato ed (`-e`): mudanças da última pra primeira, comandos a/c/d. Linha nova que é só "." vira
/// ".." e o bloco é seguido de comandos que desfazem a duplicação, como o GNU.
pub fn format_ed(out: &mut Vec<u8>, changes: &[Change], b: &[&[u8]]) {
    for ch in changes.iter().rev().filter(|c| !c.ignore) {
        let Some(letter) = letter(ch) else { continue };
        let (f0, l0) = (ch.line0 as isize, (ch.line0 + ch.deleted) as isize - 1);
        out.extend_from_slice(format!("{}{letter}\n", line_range(f0, l0)).as_bytes());
        if ch.inserted > 0 {
            ed_lines(out, inserted(ch, b));
        }
    }
}

/// As linhas que a mudança insere, do segundo arquivo.
fn inserted<'a>(ch: &Change, b: &'a [&'a [u8]]) -> &'a [&'a [u8]] {
    &b[ch.line1..ch.line1 + ch.inserted]
}

/// A linha com um `\n` só no fim, tenha ela vindo com ou sem.
fn push_line(out: &mut Vec<u8>, line: &[u8]) {
    out.extend_from_slice(line.strip_suffix(b"\n").unwrap_or(line));
    out.push(b'\n');
}

/// Linhas de um comando a/c do ed. Uma linha que é só "." sairia como fim da entrada: vira "..",
/// fecha a entrada, `s/.//` desfaz o ponto extra e, se ainda há linhas, um `a` reabre a entrada.
pub fn ed_lines(out: &mut Vec<u8>, lines: &[&[u8]]) {
    let mut reopen = false;
    let mut last_dot = false;
    for line in lines {
        if reopen {
            out.extend_from_slice(b"a\n");
            reopen = false;
        }
        last_dot = line.strip_suffix(b"\n").unwrap_or(line) == b".";
        if last_dot {
            out.extend_from_slice(b"..\n.\ns/.//\n");
            reopen = true;
        } else {
            push_line(out, line);
        }
    }
    if !last_dot {
        out.extend_from_slice(b".\n");
    }
}

/// Formato ed na ordem direta (`-f`): `c2`, `d4`, `a6`...
pub fn format_forward_ed(out: &mut Vec<u8>, changes: &[Change], b: &[&[u8]]) {
    for ch in changes.iter().filter(|c| !c.ignore) {
        let Some(letter) = letter(ch) else { continue };
        let (f0, l0) = (ch.line0 as isize + 1, (ch.line0 + ch.deleted) as isize);
        let range = if letter == 'a' {
            format!("{}", ch.line0)
        } else if l0 > f0 {
            format!("{f0} {l0}")
        } else {
            format!("{f0}")
        };
        out.extend_from_slice(format!("{letter}{range}\n").as_bytes());
        if ch.inserted > 0 {
            for line in inserted(ch, b) {
                push_line(out, line);
            }
            out.extend_from_slice(b".\n");
        }
    }
}

/// Formato RCS (`-n`): `dL N` e `aL N` seguido das linhas, na ordem direta. Uma troca vira um `d` e um
/// `a`. A última linha sem `\n` sai sem `\n`.
pub fn format_rcs(out: &mut Vec<u8>, changes: &[Change], b: &[&[u8]]) {
    for ch in changes.iter().filter(|c| !c.ignore) {
        if ch.deleted > 0 {
            out.extend_from_slice(format!("d{} {}\n", ch.line0 + 1, ch.deleted).as_bytes());
        }
        if ch.inserted > 0 {
            out.extend_from_slice(format!("a{} {}\n", ch.line0 + ch.deleted, ch.inserted).as_bytes());
            out.extend(inserted(ch, b).concat());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<&[u8]> {
        s.as_bytes().split_inclusive(|&c| c == b'\n').collect()
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
        assert_eq!(line_range(3, 2), "3");
        assert_eq!(line_range(0, -1), "0");
    }

    #[test]
    fn normal_and_unified_match_gnu_shapes() {
        let a = lines("one\ntwo\nthree\n");
        let b = lines("one\nTWO\nthree\n");
        let s = build_script(&[false, true, false], &[false, true, false]);
        let look = Look::default();
        let mut out = Vec::new();
        format_normal(&mut Printer { out: &mut out, look: &look }, &s, &a, &b);
        assert_eq!(String::from_utf8(out).unwrap(), "2c2\n< two\n---\n> TWO\n");
        let mut out = Vec::new();
        format_unified(&mut Printer { out: &mut out, look: &look }, &s, &a, &b, 3);
        assert_eq!(String::from_utf8(out).unwrap(), "@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n");
    }

    #[test]
    fn missing_newline_marker() {
        let a = lines("a\nb");
        let b = lines("a\nb\n");
        let s = build_script(&[false, true], &[false, true]);
        let look = Look::default();
        let mut out = Vec::new();
        format_unified(&mut Printer { out: &mut out, look: &look }, &s, &a, &b, 3);
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

    #[test]
    fn ed_forward_and_rcs() {
        let a = lines("a\nb\nc\nd\ne\nf\n");
        let b = lines("a\nB\nc\ne\nf\ng\nh\n");
        let c0 = [false, true, false, true, false, false];
        let c1 = [false, true, false, false, false, true, true];
        let s = build_script(&c0, &c1);
        let mut out = Vec::new();
        format_ed(&mut out, &s, &b);
        assert_eq!(String::from_utf8(out).unwrap(), "6a\ng\nh\n.\n4d\n2c\nB\n.\n");
        let mut out = Vec::new();
        format_forward_ed(&mut out, &s, &b);
        assert_eq!(String::from_utf8(out).unwrap(), "c2\nB\n.\nd4\na6\ng\nh\n.\n");
        let mut out = Vec::new();
        format_rcs(&mut out, &s, &b);
        assert_eq!(String::from_utf8(out).unwrap(), "d2 1\na2 1\nB\nd4 1\na6 2\ng\nh\n");
        let _ = a;
    }
}
