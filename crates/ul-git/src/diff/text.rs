//! Diff de linhas no formato do git: o Myers com pós-processamento de indentação do
//! `gix-imara-diff` dá as mudanças; agrupamento em hunks, cabeçalho `@@ -a,b +c,d @@ função`,
//! `\ No newline at end of file` e contagem de linhas seguem o `xemit.c`/`diff.c`.

use std::collections::HashMap;

use gix_imara_diff::{Algorithm, Diff, InternedInput, Token};

/// Como comparar linhas.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Ws {
    /// `-w`
    pub all: bool,
    /// `-b`
    pub change: bool,
    /// `--ignore-space-at-eol`
    pub at_eol: bool,
    /// `--ignore-cr-at-eol`
    pub cr_at_eol: bool,
}

impl Ws {
    fn active(&self) -> bool {
        self.all || self.change || self.at_eol || self.cr_at_eol
    }

    /// Chave de comparação de uma linha.
    fn key(&self, line: &[u8]) -> Vec<u8> {
        let body = line.strip_suffix(b"\n").unwrap_or(line);
        let mut out = Vec::with_capacity(body.len());
        if self.all {
            out.extend(body.iter().copied().filter(|c| !c.is_ascii_whitespace()));
        } else if self.change {
            let mut in_ws = false;
            for &c in body {
                if c.is_ascii_whitespace() {
                    in_ws = true;
                } else {
                    if in_ws && !out.is_empty() {
                        out.push(b' ');
                    }
                    in_ws = false;
                    out.push(c);
                }
            }
        } else if self.at_eol {
            let end = body.iter().rposition(|c| !c.is_ascii_whitespace()).map(|e| e + 1).unwrap_or(0);
            out.extend_from_slice(&body[..end]);
        } else {
            out.extend_from_slice(body.strip_suffix(b"\r").unwrap_or(body));
        }
        // A falta de `\n` no fim só conta quando o espaço não é ignorado.
        if !self.all && !self.change && !self.at_eol && line.ends_with(b"\n") {
            out.push(b'\n');
        }
        out
    }
}

/// Uma mudança (o `xdchange_t`): linhas `old[i1..i1+chg1]` viram `new[i2..i2+chg2]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub i1: usize,
    pub chg1: usize,
    pub i2: usize,
    pub chg2: usize,
}

pub fn split_lines(data: &[u8]) -> Vec<&[u8]> {
    data.split_inclusive(|c| *c == b'\n').collect()
}

/// Diff de linhas: as mudanças em ordem.
pub fn changes(old: &[&[u8]], new: &[&[u8]], ws: Ws) -> Vec<Change> {
    let mut input: InternedInput<Vec<u8>> = InternedInput::default();
    let mut map: HashMap<Vec<u8>, Token> = HashMap::new();
    let mut intern = |line: &[u8], input: &mut InternedInput<Vec<u8>>| -> Token {
        let key = if ws.active() { ws.key(line) } else { line.to_vec() };
        if let Some(t) = map.get(&key) {
            return *t;
        }
        let t = input.interner.intern(key.clone());
        map.insert(key, t);
        t
    };
    for l in old {
        let t = intern(l, &mut input);
        input.before.push(t);
    }
    for l in new {
        let t = intern(l, &mut input);
        input.after.push(t);
    }
    let mut diff = Diff::compute(Algorithm::Myers, &input);
    diff.postprocess_lines(&input);
    diff.hunks()
        .map(|h| Change {
            i1: h.before.start as usize,
            chg1: (h.before.end - h.before.start) as usize,
            i2: h.after.start as usize,
            chg2: (h.after.end - h.after.start) as usize,
        })
        .collect()
}

/// `(adicionadas, removidas)`.
pub fn count(old: &[u8], new: &[u8], ws: Ws) -> (usize, usize) {
    let a = split_lines(old);
    let b = split_lines(new);
    let ch = changes(&a, &b, ws);
    (ch.iter().map(|c| c.chg2).sum(), ch.iter().map(|c| c.chg1).sum())
}

/// O `buffer_is_binary` do git: NUL nos primeiros 8000 bytes.
pub fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(8000)].contains(&0)
}

/// Regra padrão de cabeçalho de hunk (gitattributes(5), "Defining a custom hunk-header"): linha
/// que começa com letra ASCII, `_` ou `$`. O texto vai cortado em 80 bytes e sem espaço no fim
/// (medido no oráculo).
fn default_funcname(line: &[u8]) -> Option<Vec<u8>> {
    let first = *line.first()?;
    if !(first.is_ascii_alphabetic() || first == b'_' || first == b'$') {
        return None;
    }
    Some(clip_funcname(line))
}

fn clip_funcname(line: &[u8]) -> Vec<u8> {
    let body = line.strip_suffix(b"\n").unwrap_or(line);
    let cut = &body[..body.len().min(80)];
    let keep = cut.iter().rposition(|c| !c.is_ascii_whitespace()).map_or(0, |p| p + 1);
    cut[..keep].to_vec()
}

/// Opções de saída de hunks.
#[derive(Clone, Debug)]
pub struct HunkOpts {
    pub context: usize,
    pub interhunk: usize,
    pub funcnames: bool,
    /// Regex de linha de função (diff driver); `None` usa a regra padrão.
    pub funcname_re: Option<std::rc::Rc<regex::bytes::Regex>>,
}

impl Default for HunkOpts {
    fn default() -> Self {
        HunkOpts { context: 3, interhunk: 0, funcnames: true, funcname_re: None }
    }
}

/// Um hunk pronto pra imprimir.
#[derive(Clone, Debug)]
pub struct Hunk<'a> {
    pub header: Vec<u8>,
    /// (`' '`, `'-'` ou `'+'`, linha com o `\n` se houver)
    pub lines: Vec<(u8, &'a [u8])>,
}

/// Procura de linha de função de baixo pra cima, lembrando até onde já olhou (os hunks vêm em
/// ordem, então cada linha do arquivo antigo é examinada uma vez só).
struct FuncFinder<'o> {
    re: Option<&'o regex::bytes::Regex>,
    /// Próxima linha ainda não examinada, de cima pra baixo.
    scanned: usize,
    best: Option<Vec<u8>>,
}

impl FuncFinder<'_> {
    /// Linha de função mais próxima acima de `limit` (exclusive).
    fn above(&mut self, old: &[&[u8]], limit: usize) -> Option<Vec<u8>> {
        while self.scanned < limit.min(old.len()) {
            let line = old[self.scanned];
            let hit = match self.re {
                None => default_funcname(line),
                Some(re) => {
                    let body = line.strip_suffix(b"\n").unwrap_or(line);
                    re.is_match(body).then(|| clip_funcname(line))
                }
            };
            if hit.is_some() {
                self.best = hit;
            }
            self.scanned += 1;
        }
        self.best.clone()
    }
}

/// Monta os hunks: cada mudança leva `context` linhas iguais antes e depois, e mudanças cujo vão
/// de linhas iguais não passa de `2*context + interhunk` ficam no mesmo hunk (git-diff(1),
/// `--unified` e `--inter-hunk-context`).
pub fn hunks<'a>(old: &[&'a [u8]], new: &[&'a [u8]], ch: &[Change], o: &HunkOpts) -> Vec<Hunk<'a>> {
    let mut out = Vec::new();
    let ctx = o.context;
    let join_gap = 2 * ctx + o.interhunk;
    let mut finder = FuncFinder { re: o.funcname_re.as_deref(), scanned: 0, best: None };
    let mut groups: Vec<&[Change]> = Vec::new();
    let mut start = 0;
    for k in 1..=ch.len() {
        let split = k == ch.len() || ch[k].i1 - (ch[k - 1].i1 + ch[k - 1].chg1) > join_gap;
        if split && start < k {
            groups.push(&ch[start..k]);
            start = k;
        }
    }
    for g in groups {
        let (first, last) = (g[0], g[g.len() - 1]);
        let old_from = first.i1.saturating_sub(ctx);
        let new_from = first.i2.saturating_sub(ctx);
        let old_to = (last.i1 + last.chg1 + ctx).min(old.len());
        let new_to = (last.i2 + last.chg2 + ctx).min(new.len());
        let mut header = format!("@@ -{} +{} @@", hunk_range(old_from, old_to - old_from), hunk_range(new_from, new_to - new_from)).into_bytes();
        if o.funcnames
            && let Some(f) = finder.above(old, old_from)
            && !f.is_empty()
        {
            header.push(b' ');
            header.extend_from_slice(&f);
        }
        let mut lines: Vec<(u8, &[u8])> = Vec::new();
        // As linhas iguais saem do arquivo novo (com -w é a versão nova que aparece).
        let mut at_new = new_from;
        for c in g {
            while at_new < c.i2 {
                lines.push((b' ', new[at_new]));
                at_new += 1;
            }
            lines.extend(old[c.i1..c.i1 + c.chg1].iter().map(|l| (b'-', *l)));
            lines.extend(new[c.i2..c.i2 + c.chg2].iter().map(|l| (b'+', *l)));
            at_new = c.i2 + c.chg2;
        }
        while at_new < new_to {
            lines.push((b' ', new[at_new]));
            at_new += 1;
        }
        out.push(Hunk { header, lines });
    }
    out
}

/// `início,quantidade` do cabeçalho: quantidade 1 é omitida; com zero linhas o início é a linha
/// anterior (formato unificado).
fn hunk_range(from0: usize, count: usize) -> String {
    match count {
        0 => format!("{from0},0"),
        1 => format!("{}", from0 + 1),
        n => format!("{},{n}", from0 + 1),
    }
}

/// Escreve os hunks (sem os cabeçalhos de arquivo).
pub fn write_hunks(out: &mut Vec<u8>, hunks: &[Hunk<'_>]) {
    for h in hunks {
        out.extend_from_slice(&h.header);
        out.push(b'\n');
        for (k, line) in &h.lines {
            out.push(*k);
            out.extend_from_slice(line);
            if !line.ends_with(b"\n") {
                out.extend_from_slice(b"\n\\ No newline at end of file\n");
            }
        }
    }
}

/// Atalho: hunks de dois buffers no formato do git.
pub fn unified(old: &[u8], new: &[u8], o: &HunkOpts, ws: Ws) -> Vec<u8> {
    let a = split_lines(old);
    let b = split_lines(new);
    let ch = changes(&a, &b, ws);
    let hs = hunks(&a, &b, &ch, o);
    let mut out = Vec::new();
    write_hunks(&mut out, &hs);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_style_hunks() {
        let o = HunkOpts::default();
        let old = b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nN\n";
        let new = b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\n";
        assert_eq!(String::from_utf8(unified(old, new, &o, Ws::default())).unwrap(), "@@ -11,4 +11,4 @@ j\n k\n l\n m\n-N\n+n\n");
        let h = unified(b"no newline\n", b"no newline now", &o, Ws::default());
        assert_eq!(String::from_utf8(h).unwrap(), "@@ -1 +1 @@\n-no newline\n+no newline now\n\\ No newline at end of file\n");
        assert_eq!(String::from_utf8(unified(b"", b"x\n", &o, Ws::default())).unwrap(), "@@ -0,0 +1 @@\n+x\n");
        assert_eq!(count(b"one\n", b"one\ntwo\nthree\n", Ws::default()), (2, 0));
    }

    #[test]
    fn whitespace_modes() {
        let ws = Ws { all: true, ..Ws::default() };
        assert_eq!(count(b"a b\n", b"ab\n", ws), (0, 0));
        assert_eq!(count(b"a b\n", b"ab\n", Ws::default()), (1, 1));
    }
}
