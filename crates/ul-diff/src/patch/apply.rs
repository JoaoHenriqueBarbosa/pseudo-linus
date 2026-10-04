//! Aplicação dos hunks já localizados, montando o arquivo de saída. As linhas de contexto saem da
//! entrada (não do patch), como no GNU: com fuzz ou `-l`, o que está no arquivo prevalece. A cópia da
//! entrada é preguiçosa: só anda até o ponto da próxima mudança, então o contexto final de um hunk
//! ainda pode ser usado pelo hunk seguinte.

use super::hunk::{Hunk, Op};

/// O hunk cairia antes do trecho já escrito ("misordered hunks! output would be garbled").
#[derive(Debug, PartialEq, Eq)]
pub struct Misordered;

/// Resultado de um grupo de mudanças no `--merge`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeKind {
    NotMerged,
    AlreadyApplied,
    Merged,
}

impl MergeKind {
    pub fn text(self) -> &'static str {
        match self {
            MergeKind::NotMerged => "NOT MERGED",
            MergeKind::AlreadyApplied => "already applied",
            MergeKind::Merged => "merged",
        }
    }
}

/// O que o `--merge` fez num hunk: os grupos por tipo (na ordem em que o primeiro de cada tipo
/// apareceu), cada um com as faixas de linhas na saída.
#[derive(Clone, Debug, Default)]
pub struct MergeReport {
    pub parts: Vec<(MergeKind, Vec<(usize, usize)>)>,
    pub conflict: bool,
    /// Linhas escritas pelo hunk e tamanho da janela consumida da entrada.
    pub out_lines: usize,
    pub window: usize,
}

impl MergeReport {
    fn push(&mut self, kind: MergeKind, range: (usize, usize)) {
        match self.parts.iter_mut().find(|p| p.0 == kind) {
            Some(p) => p.1.push(range),
            None => self.parts.push((kind, vec![range])),
        }
    }
}

/// Item do alinhamento entre a base (linhas antigas do hunk) e a janela do arquivo.
enum Item {
    /// Linha igual nos dois lados (índices na base e na janela).
    Same(usize, usize),
    /// Trecho diferente: faixa da base e faixa da janela.
    Diff { b0: usize, b1: usize, w0: usize, w1: usize },
}

/// Mudança do patch em coordenadas da base.
struct Change<'h> {
    b0: usize,
    b1: usize,
    lines: Vec<&'h [u8]>,
}

/// Alinhamento por maior subsequência comum (tabela de sufixos; as janelas são do tamanho de um
/// hunk).
fn align(base: &[&[u8]], window: &[&[u8]]) -> Vec<Item> {
    let (n, m) = (base.len(), window.len());
    let mut t = vec![0u32; (n + 1) * (m + 1)];
    let idx = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[idx(i, j)] = if base[i] == window[j] {
                t[idx(i + 1, j + 1)] + 1
            } else {
                t[idx(i + 1, j)].max(t[idx(i, j + 1)])
            };
        }
    }
    let mut items = Vec::new();
    let (mut i, mut j) = (0, 0);
    let (mut di, mut dj) = (0, 0);
    let flush = |items: &mut Vec<Item>, di: usize, i: usize, dj: usize, j: usize| {
        if di < i || dj < j {
            items.push(Item::Diff { b0: di, b1: i, w0: dj, w1: j });
        }
    };
    while i < n && j < m {
        if base[i] == window[j] && t[idx(i, j)] == t[idx(i + 1, j + 1)] + 1 {
            flush(&mut items, di, i, dj, j);
            items.push(Item::Same(i, j));
            i += 1;
            j += 1;
            di = i;
            dj = j;
        } else if t[idx(i + 1, j)] >= t[idx(i, j + 1)] {
            i += 1;
        } else {
            j += 1;
        }
    }
    flush(&mut items, di, n, dj, m);
    items
}

pub struct Builder<'a> {
    input: Vec<&'a [u8]>,
    out: Vec<u8>,
    /// Linhas da entrada já copiadas ou descartadas.
    frozen: usize,
    ifdef: Option<Vec<u8>>,
}

impl<'a> Builder<'a> {
    pub fn new(input: &'a [u8], ifdef: Option<Vec<u8>>) -> Builder<'a> {
        Builder { input: input.split_inclusive(|&c| c == b'\n').collect(), out: Vec::new(), frozen: 0, ifdef }
    }

    pub fn lines(&self) -> &[&'a [u8]] {
        &self.input
    }

    pub fn frozen(&self) -> usize {
        self.frozen
    }

    /// Escreve uma linha; se a anterior ficou sem `\n` (fim de arquivo sem newline), quebra antes,
    /// como o GNU faz pra não colar duas linhas.
    fn put(&mut self, line: &[u8]) {
        if !self.out.is_empty() && !self.out.ends_with(b"\n") {
            self.out.push(b'\n');
        }
        self.out.extend_from_slice(line);
    }

    fn copy_till(&mut self, n: usize) -> Result<(), Misordered> {
        if self.frozen > n {
            return Err(Misordered);
        }
        let end = n.min(self.input.len());
        let start = self.frozen.min(end);
        for i in start..end {
            let l = self.input[i];
            self.put(l);
        }
        self.frozen = n;
        Ok(())
    }

    fn lines_written(&self) -> usize {
        let n = self.out.iter().filter(|&&c| c == b'\n').count();
        if self.out.last().is_some_and(|&c| c != b'\n') { n + 1 } else { n }
    }

    /// Aplica `hunk` com a primeira linha do padrão na linha `pos` (base 1) da entrada.
    pub fn apply(&mut self, hunk: &Hunk, pos: usize) -> Result<(), Misordered> {
        let ops = hunk.ops();
        let mut k = pos.saturating_sub(1);
        if let Some(name) = self.ifdef.clone() {
            return self.apply_ifdef(&ops, k, &name);
        }
        for op in ops {
            match op {
                Op::Context(_) => k += 1,
                Op::Delete(_) => {
                    self.copy_till(k)?;
                    self.frozen = k + 1;
                    k += 1;
                }
                Op::Insert(t) => {
                    self.copy_till(k)?;
                    self.put(t);
                }
            }
        }
        Ok(())
    }

    /// `-D NOME`: apagadas entre `#ifndef NOME` e `#endif`, inseridas entre `#ifdef NOME` e
    /// `#endif`, trocadas como `#ifndef`/`#else`/`#endif`.
    fn apply_ifdef(&mut self, ops: &[Op<'_>], mut k: usize, name: &[u8]) -> Result<(), Misordered> {
        let mut i = 0;
        while i < ops.len() {
            if let Op::Context(_) = ops[i] {
                k += 1;
                i += 1;
                continue;
            }
            let mut dels = 0;
            while i < ops.len() && matches!(ops[i], Op::Delete(_)) {
                dels += 1;
                i += 1;
            }
            let mut ins: Vec<&[u8]> = Vec::new();
            while i < ops.len() {
                match ops[i] {
                    Op::Insert(t) => ins.push(t),
                    Op::Delete(_) | Op::Context(_) => break,
                }
                i += 1;
            }
            self.copy_till(k)?;
            if dels > 0 {
                self.put(&[b"#ifndef ", name, b"\n"].concat());
                let end = (k + dels).min(self.input.len());
                for j in k.min(end)..end {
                    let l = self.input[j];
                    self.put(l);
                }
                if !ins.is_empty() {
                    self.put(b"#else\n");
                }
            } else {
                self.put(&[b"#ifdef ", name, b"\n"].concat());
            }
            for t in ins {
                self.put(t);
            }
            self.put(b"#endif\n");
            k += dels;
            self.frozen = k;
        }
        Ok(())
    }

    /// `--merge`: fusão de três vias entre a base (linhas antigas do hunk), o arquivo (a janela que
    /// começa em `pos`) e o patch (linhas novas). Mudanças que se tocam ou se sobrepõem formam um
    /// grupo; grupo só do patch é aplicado ("merged"), igual nos dois lados é "already applied", e o
    /// resto vira conflito com marcadores. As posições informadas seguem a contagem do GNU 2.8, que
    /// soma as linhas apagadas de cada grupo aplicado antes no mesmo hunk.
    pub fn merge(&mut self, hunk: &Hunk, pos: usize, diff3: bool) -> Result<MergeReport, Misordered> {
        let k = pos.saturating_sub(1).min(self.input.len());
        self.copy_till(k)?;
        let base: Vec<&[u8]> = hunk.old.iter().map(|l| l.text.as_slice()).collect();
        let end = (k + base.len()).min(self.input.len());
        let window: Vec<&'a [u8]> = self.input[k..end].to_vec();
        let start_line = self.lines_written();

        let ops = hunk.ops();
        let mut theirs: Vec<Change<'_>> = Vec::new();
        let mut bi = 0usize;
        let mut i = 0;
        while i < ops.len() {
            if let Op::Context(_) = ops[i] {
                bi += 1;
                i += 1;
                continue;
            }
            let b0 = bi;
            let mut lines = Vec::new();
            while i < ops.len() && !matches!(ops[i], Op::Context(_)) {
                match ops[i] {
                    Op::Delete(_) => bi += 1,
                    Op::Insert(t) => lines.push(t),
                    Op::Context(_) => {}
                }
                i += 1;
            }
            theirs.push(Change { b0, b1: bi, lines });
        }
        let items = align(&base, &window);

        // Grupos: faixas da base que se tocam ou se sobrepõem.
        let mut spans: Vec<(usize, usize)> = theirs.iter().map(|c| (c.b0, c.b1)).collect();
        spans.extend(items.iter().filter_map(|it| match it {
            Item::Diff { b0, b1, .. } => Some((*b0, *b1)),
            Item::Same(..) => None,
        }));
        spans.sort();
        let mut clusters: Vec<(usize, usize)> = Vec::new();
        for (b0, b1) in spans {
            match clusters.last_mut() {
                Some(last) if b0 <= last.1 => last.1 = last.1.max(b1),
                _ => clusters.push((b0, b1)),
            }
        }
        let cluster_of = |b0: usize, b1: usize| clusters.iter().position(|&(c0, c1)| c0 <= b0 && b1 <= c1 && (b0 < c1 || b0 == b1));

        let mut report = MergeReport::default();
        let mut skew = 0usize;
        let mut it = 0usize;
        for (ci, &(c0, c1)) in clusters.iter().enumerate() {
            // Linhas iguais antes do grupo.
            while it < items.len() {
                match &items[it] {
                    Item::Same(b, w) if *b < c0 => {
                        let l = window[*w];
                        self.put(l);
                        it += 1;
                    }
                    _ => break,
                }
            }
            // Texto do arquivo no grupo.
            let mut mine: Vec<&[u8]> = Vec::new();
            let mut has_ours = false;
            while it < items.len() {
                match &items[it] {
                    Item::Same(b, w) if *b >= c0 && *b < c1 => mine.push(window[*w]),
                    Item::Diff { b0, b1, w0, w1 } if cluster_of(*b0, *b1) == Some(ci) => {
                        has_ours = true;
                        mine.extend(window[*w0..*w1].iter().copied());
                    }
                    _ => break,
                }
                it += 1;
            }
            // Texto do patch no grupo.
            let mut new: Vec<&[u8]> = Vec::new();
            let mut has_theirs = false;
            let mut b = c0;
            loop {
                if let Some(ch) = theirs.iter().find(|c| c.b0 == b && c.b1 == b) {
                    has_theirs = true;
                    new.extend(ch.lines.iter().copied());
                }
                if b >= c1 {
                    break;
                }
                if let Some(ch) = theirs.iter().find(|c| c.b0 == b && c.b1 > b) {
                    has_theirs = true;
                    new.extend(ch.lines.iter().copied());
                    b = ch.b1;
                    continue;
                }
                new.push(base[b]);
                b += 1;
            }
            let line_now = self.lines_written() + 1;
            let span = |at: usize, len: usize| if len > 1 { (at, at + len - 1) } else { (at, at) };
            if has_theirs && !has_ours {
                for l in &new {
                    self.put(l);
                }
                report.push(MergeKind::Merged, span(line_now + skew, new.len()));
                skew += c1 - c0;
            } else if has_theirs && mine == new {
                for l in &mine {
                    self.put(l);
                }
                report.push(MergeKind::AlreadyApplied, span(line_now + skew, mine.len()));
            } else if has_theirs {
                self.put(b"<<<<<<<\n");
                for l in &mine {
                    self.put(l);
                }
                if diff3 {
                    self.put(b"|||||||\n");
                    for l in &base[c0..c1] {
                        self.put(l);
                    }
                }
                self.put(b"=======\n");
                for l in &new {
                    self.put(l);
                }
                self.put(b">>>>>>>\n");
                let last = self.lines_written();
                report.push(MergeKind::NotMerged, (line_now + skew, last + skew));
                report.conflict = true;
            } else {
                for l in &mine {
                    self.put(l);
                }
            }
        }
        // O que sobrou da janela depois do último grupo.
        while it < items.len() {
            match &items[it] {
                Item::Same(_, w) => {
                    let l = window[*w];
                    self.put(l);
                }
                Item::Diff { w0, w1, .. } => {
                    for w in *w0..*w1 {
                        let l = window[w];
                        self.put(l);
                    }
                }
            }
            it += 1;
        }
        self.frozen = end;
        report.out_lines = self.lines_written() - start_line;
        report.window = window.len();
        Ok(report)
    }

    /// Copia o resto da entrada e devolve o arquivo novo.
    pub fn finish(mut self) -> Vec<u8> {
        let len = self.input.len();
        let _ = self.copy_till(len.max(self.frozen));
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::super::hunk::{Format, sections_from_unified};
    use super::*;

    fn hunk(old_first: usize, lines: &[(u8, &str)]) -> Hunk {
        let l: Vec<(u8, Vec<u8>)> = lines.iter().map(|(k, t)| (*k, t.as_bytes().to_vec())).collect();
        let (old, new) = sections_from_unified(&l);
        Hunk { format: Format::Unified, old_first, old, new_first: old_first, new, func: Vec::new(), normal_cmd: 0 }
    }

    #[test]
    fn applies_and_keeps_input_context() {
        let mut b = Builder::new(b"a\nb\nc\nd\n", None);
        b.apply(&hunk(1, &[(b' ', "A\n"), (b'-', "b\n"), (b'+', "B\n"), (b' ', "c\n")]), 1).unwrap();
        assert_eq!(b.finish(), b"a\nB\nc\nd\n");
    }

    #[test]
    fn misordered_is_detected() {
        let mut b = Builder::new(b"1\n2\n3\n4\n", None);
        b.apply(&hunk(3, &[(b'-', "3\n"), (b'+', "T\n")]), 3).unwrap();
        assert_eq!(b.apply(&hunk(1, &[(b'-', "1\n"), (b'+', "O\n")]), 1), Err(Misordered));
    }

    #[test]
    fn ifdef_output() {
        let mut b = Builder::new(b"1\n2\n3\n", Some(b"FOO".to_vec()));
        b.apply(&hunk(1, &[(b' ', "1\n"), (b'-', "2\n"), (b'+', "TWO\n"), (b' ', "3\n")]), 1).unwrap();
        assert_eq!(b.finish(), b"1\n#ifndef FOO\n2\n#else\nTWO\n#endif\n3\n");
    }

    fn merged(input: &str, h: &Hunk, pos: usize, diff3: bool) -> (String, MergeReport) {
        let mut b = Builder::new(input.as_bytes(), None);
        let r = b.merge(h, pos, diff3).unwrap();
        (String::from_utf8(b.finish()).unwrap(), r)
    }

    #[test]
    fn merge_like_gnu() {
        let h = hunk(3, &[(b' ', "3\n"), (b' ', "4\n"), (b'-', "5\n"), (b'+', "FIVE\n"), (b' ', "6\n"), (b' ', "7\n")]);
        let (out, r) = merged("1\n2\n3\nfour\n5\n6\n7\n8\n", &h, 3, false);
        assert_eq!(out, "1\n2\n3\n<<<<<<<\nfour\n5\n=======\n4\nFIVE\n>>>>>>>\n6\n7\n8\n");
        assert_eq!(r.parts, vec![(MergeKind::NotMerged, vec![(4, 10)])]);
        let (out, r) = merged("1\n2\nthree\n4\n5\n6\nseven\n8\n", &h, 3, false);
        assert_eq!(out, "1\n2\nthree\n4\nFIVE\n6\nseven\n8\n");
        assert_eq!(r.parts, vec![(MergeKind::Merged, vec![(5, 5)])]);
        let (out, _) = merged("1\n2\n3\n4\n6\n7\n8\n9\n", &h, 3, false);
        assert_eq!(out, "1\n2\n3\n4\n<<<<<<<\n=======\nFIVE\n>>>>>>>\n6\n7\n8\n9\n");
        let (out, _) = merged("1\n2\n3\n4\nfive\n6\n7\n", &h, 3, true);
        assert_eq!(out, "1\n2\n3\n4\n<<<<<<<\nfive\n|||||||\n5\n=======\nFIVE\n>>>>>>>\n6\n7\n");
    }
}
