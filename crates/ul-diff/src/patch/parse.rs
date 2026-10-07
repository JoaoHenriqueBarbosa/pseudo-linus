//! Leitura da entrada do `patch`: divide o texto em "pedaços" (um por arquivo), reconhece o formato
//! (unificado, contexto, normal, script do ed, cabeçalhos do git) e interpreta os hunks com as regras
//! observadas no GNU patch 2.8: contagem pelas faixas do cabeçalho, linha vazia como contexto, linha
//! começando com tab como contexto, "\ No newline at end of file", linhas de contexto em branco
//! completadas no fim da entrada (até três), hunk só de contexto é malformado.

use ul_common::ctype::parse_decimal_usize as parse_num;

use super::hunk::{Format, Hunk, PLine, sections_from_unified};
use super::names::{HeaderName, fetch_name};
use super::opts::ForcedFormat;

/// Erro fatal da leitura (sai com 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fatal {
    /// `malformed patch at line N: <linha>`.
    Malformed { line: usize, text: Vec<u8> },
    /// `unexpected end of file in patch`.
    UnexpectedEof,
}

impl Fatal {
    pub fn message(&self) -> Vec<u8> {
        match self {
            Fatal::Malformed { line, text } => {
                let mut m = format!("malformed patch at line {line}: ").into_bytes();
                m.extend_from_slice(text);
                m
            }
            Fatal::UnexpectedEof => b"unexpected end of file in patch".to_vec(),
        }
    }
}

/// Cabeçalhos estendidos do git.
#[derive(Clone, Debug, Default)]
pub struct GitInfo {
    pub a_name: Option<Vec<u8>>,
    pub b_name: Option<Vec<u8>>,
    pub old_mode: Option<u32>,
    pub new_mode: Option<u32>,
    pub new_file_mode: Option<u32>,
    pub deleted_file_mode: Option<u32>,
    pub rename_from: Option<Vec<u8>>,
    pub rename_to: Option<Vec<u8>>,
    pub copy_from: Option<Vec<u8>>,
    pub copy_to: Option<Vec<u8>>,
}

impl GitInfo {
    fn has_effect(&self) -> bool {
        self.old_mode.is_some()
            || self.new_mode.is_some()
            || self.new_file_mode.is_some()
            || self.deleted_file_mode.is_some()
            || self.rename_from.is_some()
            || self.copy_from.is_some()
    }
}

/// O corpo de um pedaço.
#[derive(Clone, Debug)]
pub enum Body {
    /// Hunks já interpretados; `error` é o erro fatal no hunk seguinte ao último da lista.
    Hunks { format: Format, hunks: Vec<Hunk>, error: Option<Fatal> },
    /// Script do ed (o texto do script).
    Ed(Vec<u8>),
    /// "GIT binary patch".
    GitBinary,
    /// "Binary files a and b differ".
    BinaryDiffer,
    /// Só cabeçalhos do git (renomeação, cópia, modo, arquivo vazio novo ou removido).
    Nothing,
}

/// Um arquivo do patch.
#[derive(Clone, Debug)]
pub struct Chunk {
    pub old: Option<HeaderName>,
    pub new: Option<HeaderName>,
    pub index: Option<Vec<u8>>,
    pub git: Option<GitInfo>,
    /// Linhas desde o fim do pedaço anterior até o começo do corpo.
    pub leading: Vec<Vec<u8>>,
    /// Linha (base 1) que o GNU cita em "can't find file to patch at input line N".
    pub input_line: usize,
    pub body: Body,
}

impl Chunk {
    /// Palavra de uma linha `Prereq:` no texto antes do corpo (o arquivo tem que contê-la).
    pub fn prereq(&self) -> Option<Vec<u8>> {
        self.leading.iter().rev().find_map(|l| {
            let rest = l.strip_prefix(b"Prereq:")?;
            let word: Vec<u8> = rest
                .iter()
                .skip_while(|c| c.is_ascii_whitespace())
                .take_while(|c| !c.is_ascii_whitespace())
                .copied()
                .collect();
            (!word.is_empty()).then_some(word)
        })
    }

    pub fn hunk_count(&self) -> usize {
        match &self.body {
            Body::Hunks { hunks, .. } => hunks.len(),
            _ => 0,
        }
    }

    /// Descrição do formato, pra mensagem "Hmm...  Looks like ... to me..." do `--verbose`.
    pub fn kind_text(&self) -> &'static str {
        match &self.body {
            Body::Hunks { format: Format::Unified, .. } => "a unified diff",
            Body::Hunks { format: Format::Context, .. } => "a new-style context diff",
            Body::Hunks { format: Format::Normal, .. } => "a normal diff",
            Body::Ed(_) => "an ed script",
            _ => "a git diff",
        }
    }
}

/// Divisor da entrada em pedaços.
pub struct Scanner<'a> {
    lines: Vec<&'a [u8]>,
    pos: usize,
    forced: Option<ForcedFormat>,
    /// Foi dado o arquivo alvo na linha de comando (aceita diffs sem nomes).
    has_target: bool,
    /// Sobrou texto depois do último pedaço.
    trailing: bool,
}

/// `a` ou `a,b`.
fn parse_pair(s: &[u8]) -> Option<(usize, Option<usize>)> {
    match s.iter().position(|&c| c == b',') {
        Some(p) => Some((parse_num(&s[..p])?, Some(parse_num(&s[p + 1..])?))),
        None => Some((parse_num(s)?, None)),
    }
}

/// Comando do diff normal: `5c5`, `3a4,5`, `4,5d3`.
pub fn normal_command(line: &[u8]) -> Option<(usize, usize, u8, usize, usize)> {
    let l = super::names::strip_eol(line);
    let p = l.iter().position(|&c| matches!(c, b'a' | b'c' | b'd'))?;
    let (l1, l2) = parse_pair(&l[..p])?;
    let (r1, r2) = parse_pair(&l[p + 1..])?;
    Some((l1, l2.unwrap_or(l1), l[p], r1, r2.unwrap_or(r1)))
}

/// Comando do ed como o `diff -e` escreve: `5c`, `3a`, `4,5d`, `5s/.//`.
fn ed_command(line: &[u8]) -> bool {
    let l = super::names::strip_eol(line);
    let digits_end = l.iter().position(|c| !c.is_ascii_digit() && *c != b',').unwrap_or(l.len());
    if digits_end == 0 || digits_end == l.len() {
        return false;
    }
    let rest = &l[digits_end..];
    matches!(rest, b"a" | b"c" | b"d" | b"i") || rest.starts_with(b"s/")
}

fn context_header(line: &[u8], open: &[u8], close: u8) -> Option<(usize, Option<usize>)> {
    let l = super::names::strip_eol(line);
    let rest = l.strip_prefix(open)?;
    let end = rest.iter().position(|&c| c == b' ' || c == close).unwrap_or(rest.len());
    let tail = &rest[end..];
    if !tail.iter().all(|&c| c == b' ' || c == close) {
        return None;
    }
    parse_pair(&rest[..end])
}

fn is_context_separator(line: &[u8]) -> bool {
    context_header(line, b"--- ", b'-').is_some() && super::names::strip_eol(line).ends_with(b"-")
}

fn is_context_line(line: &[u8]) -> bool {
    line.len() >= 2 && matches!(line[0], b' ' | b'+' | b'-' | b'!') && line[1] == b' '
        || line == b"\n"
        || line.starts_with(b"\\")
}

/// Corpo de uma linha de hunk sem o marcador de duas colunas do formato de contexto.
fn ctx_text(line: &[u8]) -> &[u8] {
    if line.len() >= 2 { &line[2..] } else { b"\n" }
}

/// `\ No newline at end of file`: a última linha lida perde o fim de linha.
fn no_newline_at_end(lines: &mut [PLine]) {
    if let Some(last) = lines.last_mut()
        && last.text.ends_with(b"\n")
    {
        last.text.pop();
    }
}

fn git_names(rest: &[u8]) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let l = super::names::strip_eol(rest);
    if l.first() == Some(&b'"')
        && let Some(a) = fetch_name(l) {
            // Depois do primeiro nome entre aspas vem o segundo.
            let mut quoted_len = 1;
            let mut esc = false;
            for &c in &l[1..] {
                quoted_len += 1;
                if esc {
                    esc = false;
                } else if c == b'\\' {
                    esc = true;
                } else if c == b'"' {
                    break;
                }
            }
            let b = fetch_name(&l[quoted_len.min(l.len())..]).map(|h| h.name);
            return (Some(a.name), b);
        }
    // a/x b/x: corta no meio quando os dois lados têm o mesmo caminho, senão no " b/".
    let n = l.len();
    if n % 2 == 1 {
        let half = n / 2;
        let (a, b) = (&l[..half], &l[half + 1..]);
        if l[half] == b' ' && a.len() > 2 && b.len() > 2 && a[2..] == b[2..] {
            return (Some(a.to_vec()), Some(b.to_vec()));
        }
    }
    if let Some(p) = l.windows(3).position(|w| w == b" b/") {
        return (Some(l[..p].to_vec()), Some(l[p + 1..].to_vec()));
    }
    match l.iter().position(|&c| c == b' ') {
        Some(p) => (Some(l[..p].to_vec()), Some(l[p + 1..].to_vec())),
        None => (Some(l.to_vec()), None),
    }
}

fn octal_mode(rest: &[u8]) -> Option<u32> {
    let s = super::names::strip_eol(rest);
    let s = std::str::from_utf8(s).ok()?.trim();
    u32::from_str_radix(s, 8).ok()
}

fn git_path(rest: &[u8]) -> Vec<u8> {
    let l = super::names::strip_eol(rest);
    if l.first() == Some(&b'"')
        && let Some(h) = fetch_name(l)
    {
        return h.name;
    }
    l.to_vec()
}

impl<'a> Scanner<'a> {
    pub fn new(text: &'a [u8], forced: Option<ForcedFormat>, has_target: bool) -> Scanner<'a> {
        Scanner { lines: text.split_inclusive(|&c| c == b'\n').collect(), pos: 0, forced, has_target, trailing: false }
    }

    /// Sobrou lixo depois do último pedaço (só vale depois de `next_chunk` devolver `None`).
    pub fn has_trailing(&self) -> bool {
        self.trailing
    }

    fn line(&self, i: usize) -> &'a [u8] {
        self.lines.get(i).copied().unwrap_or(b"")
    }

    /// Próximo pedaço, ou `None` no fim da entrada.
    pub fn next_chunk(&mut self) -> Option<Chunk> {
        let start = self.pos;
        let n = self.lines.len();
        let mut index: Option<Vec<u8>> = None;
        let mut git: Option<GitInfo> = None;
        let mut i = start;
        while i < n {
            sysabi::sys::checkpoint();
            let l = self.lines[i];
            if let Some(rest) = l.strip_prefix(b"diff --git ") {
                if let Some(g) = git.take()
                    && g.has_effect()
                {
                    self.pos = i;
                    return Some(self.header_only(start, i, i, index, g));
                }
                let (a, b) = git_names(rest);
                git = Some(GitInfo { a_name: a, b_name: b, ..GitInfo::default() });
                i += 1;
                continue;
            }
            if let Some(g) = git.as_mut() {
                let mut header = true;
                if let Some(r) = l.strip_prefix(b"old mode ") {
                    g.old_mode = octal_mode(r);
                } else if let Some(r) = l.strip_prefix(b"new mode ") {
                    g.new_mode = octal_mode(r);
                } else if let Some(r) = l.strip_prefix(b"new file mode ") {
                    g.new_file_mode = octal_mode(r);
                } else if let Some(r) = l.strip_prefix(b"deleted file mode ") {
                    g.deleted_file_mode = octal_mode(r);
                } else if let Some(r) = l.strip_prefix(b"rename from ") {
                    g.rename_from = Some(git_path(r));
                } else if let Some(r) = l.strip_prefix(b"rename to ") {
                    g.rename_to = Some(git_path(r));
                } else if let Some(r) = l.strip_prefix(b"copy from ") {
                    g.copy_from = Some(git_path(r));
                } else if let Some(r) = l.strip_prefix(b"copy to ") {
                    g.copy_to = Some(git_path(r));
                } else if l.starts_with(b"similarity index ")
                    || l.starts_with(b"dissimilarity index ")
                    || l.starts_with(b"index ")
                {
                } else if l == b"GIT binary patch\n" || l == b"GIT binary patch" {
                    let g = git.take().unwrap_or_default();
                    let mut j = i + 1;
                    // Dois blocos (literal/delta), cada um terminado por uma linha vazia.
                    let mut blocks = 0;
                    while j < n && blocks < 2 {
                        if self.lines[j] == b"\n" {
                            blocks += 1;
                        }
                        j += 1;
                    }
                    self.pos = j;
                    return Some(Chunk {
                        old: None,
                        new: None,
                        index,
                        git: Some(g),
                        leading: self.lines[start..i].iter().map(|x| x.to_vec()).collect(),
                        input_line: i + 1,
                        body: Body::GitBinary,
                    });
                } else if l.starts_with(b"Binary files ") && super::names::strip_eol(l).ends_with(b" differ") {
                    let g = git.take().unwrap_or_default();
                    self.pos = i + 1;
                    return Some(Chunk {
                        old: None,
                        new: None,
                        index,
                        git: Some(g),
                        leading: self.lines[start..=i].iter().map(|x| x.to_vec()).collect(),
                        input_line: i + 1,
                        body: Body::BinaryDiffer,
                    });
                } else {
                    header = false;
                }
                if header {
                    i += 1;
                    continue;
                }
            }
            if let Some(r) = l.strip_prefix(b"Index:") {
                let name = super::names::strip_eol(r);
                let name: Vec<u8> = name.iter().skip_while(|c| **c == b' ' || **c == b'\t').copied().collect();
                if !name.is_empty() {
                    index = Some(name);
                }
            }
            let unified_ok = !matches!(self.forced, Some(ForcedFormat::Context | ForcedFormat::Normal | ForcedFormat::Ed));
            let context_ok = !matches!(self.forced, Some(ForcedFormat::Unified | ForcedFormat::Normal | ForcedFormat::Ed));
            if unified_ok
                && l.starts_with(b"--- ")
                && self.line(i + 1).starts_with(b"+++ ")
                && self.line(i + 2).starts_with(b"@@ -")
            {
                let old = fetch_name(&l[4..]);
                let new = fetch_name(&self.line(i + 1)[4..]);
                let leading = self.lines[start..i + 2].iter().map(|x| x.to_vec()).collect();
                let (hunks, error, end) = self.parse_unified(i + 2);
                self.pos = end;
                return Some(Chunk {
                    old,
                    new,
                    index,
                    git,
                    leading,
                    input_line: i + 3,
                    body: Body::Hunks { format: Format::Unified, hunks, error },
                });
            }
            if context_ok
                && l.starts_with(b"*** ")
                && self.line(i + 1).starts_with(b"--- ")
                && self.line(i + 2).starts_with(b"***************")
            {
                let old = fetch_name(&l[4..]);
                let new = fetch_name(&self.line(i + 1)[4..]);
                let leading = self.lines[start..i + 2].iter().map(|x| x.to_vec()).collect();
                let (hunks, error, end) = self.parse_context(i + 2);
                self.pos = end;
                return Some(Chunk {
                    old,
                    new,
                    index,
                    git,
                    leading,
                    input_line: i + 3,
                    body: Body::Hunks { format: Format::Context, hunks, error },
                });
            }
            let headless_ok = self.has_target || index.is_some() || self.forced.is_some();
            if unified_ok && headless_ok && l.starts_with(b"@@ -") {
                let leading = self.lines[start..i].iter().map(|x| x.to_vec()).collect();
                let (hunks, error, end) = self.parse_unified(i);
                self.pos = end;
                return Some(Chunk {
                    old: None,
                    new: None,
                    index,
                    git,
                    leading,
                    input_line: i + 1,
                    body: Body::Hunks { format: Format::Unified, hunks, error },
                });
            }
            let normal_ok = matches!(self.forced, None | Some(ForcedFormat::Normal));
            if normal_ok && headless_ok && normal_command(l).is_some() {
                let leading = self.lines[start..i].iter().map(|x| x.to_vec()).collect();
                let (hunks, error, end) = self.parse_normal(i);
                self.pos = end;
                return Some(Chunk {
                    old: None,
                    new: None,
                    index,
                    git,
                    leading,
                    input_line: i + 1,
                    body: Body::Hunks { format: Format::Normal, hunks, error },
                });
            }
            let ed_ok = matches!(self.forced, None | Some(ForcedFormat::Ed));
            if ed_ok && headless_ok && ed_command(l) {
                let leading = self.lines[start..i].iter().map(|x| x.to_vec()).collect();
                let mut script = Vec::new();
                for x in &self.lines[i..] {
                    script.extend_from_slice(x);
                }
                self.pos = n;
                return Some(Chunk { old: None, new: None, index, git, leading, input_line: i + 1, body: Body::Ed(script) });
            }
            if let Some(g) = git.take()
                && g.has_effect() {
                    self.pos = i;
                    return Some(self.header_only(start, i, i, index, g));
                }
            i += 1;
        }
        if let Some(g) = git.take()
            && g.has_effect()
        {
            self.pos = n;
            return Some(self.header_only(start, n, n, index, g));
        }
        self.trailing = start < n;
        self.pos = n;
        None
    }

    fn header_only(&self, start: usize, end: usize, line: usize, index: Option<Vec<u8>>, g: GitInfo) -> Chunk {
        Chunk {
            old: None,
            new: None,
            index,
            git: Some(g),
            leading: self.lines[start..end].iter().map(|x| x.to_vec()).collect(),
            input_line: line.max(1),
            body: Body::Nothing,
        }
    }

    /// Hunks unificados a partir da linha `j` (o primeiro `@@`). Devolve os hunks, o erro fatal (se
    /// houver) e onde o pedaço termina.
    fn parse_unified(&self, mut j: usize) -> (Vec<Hunk>, Option<Fatal>, usize) {
        let n = self.lines.len();
        let mut hunks = Vec::new();
        while j < n && self.lines[j].starts_with(b"@@ ") {
            let header = self.lines[j];
            let Some((old_start, old_len, new_start, new_len, func)) = unified_header(header) else {
                return (hunks, Some(Fatal::Malformed { line: j + 1, text: header.to_vec() }), j);
            };
            j += 1;
            let mut lines: Vec<PLine> = Vec::new();
            let (mut old_left, mut new_left) = (old_len, new_len);
            let mut last_text: Vec<u8> = header.to_vec();
            let mut last_line = j;
            while old_left > 0 || new_left > 0 {
                sysabi::sys::checkpoint();
                let (kind, text, filled): (u8, Vec<u8>, bool) = if j < n {
                    let l = self.lines[j];
                    j += 1;
                    last_line = j;
                    last_text = l.to_vec();
                    match l.first() {
                        Some(b' ') => (b' ', l[1..].to_vec(), false),
                        Some(b'\n') => (b' ', l.to_vec(), false),
                        Some(b'\t') => (b' ', l.to_vec(), false),
                        Some(b'-') => (b'-', l[1..].to_vec(), false),
                        Some(b'+') => (b'+', l[1..].to_vec(), false),
                        Some(b'\\') => {
                            no_newline_at_end(&mut lines);
                            continue;
                        }
                        _ => return (hunks, Some(Fatal::Malformed { line: j, text: l.to_vec() }), j),
                    }
                } else if new_left <= 3 {
                    last_text = b" \n".to_vec();
                    (b' ', b"\n".to_vec(), true)
                } else {
                    return (hunks, Some(Fatal::UnexpectedEof), j);
                };
                let ok = match kind {
                    b' ' => old_left > 0 && new_left > 0,
                    b'-' => old_left > 0,
                    _ => new_left > 0,
                };
                if !ok {
                    let line = if filled { last_line } else { j };
                    return (hunks, Some(Fatal::Malformed { line, text: last_text }), j);
                }
                match kind {
                    b' ' => {
                        old_left -= 1;
                        new_left -= 1;
                    }
                    b'-' => old_left -= 1,
                    _ => new_left -= 1,
                }
                lines.push(PLine { mark: kind, text });
            }
            if j < n && self.lines[j].starts_with(b"\\") {
                no_newline_at_end(&mut lines);
                j += 1;
            }
            if lines.iter().all(|l| l.mark == b' ') {
                return (hunks, Some(Fatal::Malformed { line: last_line.max(1), text: last_text }), j);
            }
            let (old, new) = sections_from_unified(&lines);
            hunks.push(Hunk {
                format: Format::Unified,
                old_first: if old_len == 0 { old_start + 1 } else { old_start },
                old,
                new_first: if new_len == 0 { new_start + 1 } else { new_start },
                new,
                func,
                normal_cmd: 0,
            });
        }
        (hunks, None, j)
    }

    /// Hunks de contexto a partir da linha `j` (o primeiro `***************`).
    fn parse_context(&self, mut j: usize) -> (Vec<Hunk>, Option<Fatal>, usize) {
        let n = self.lines.len();
        let mut hunks = Vec::new();
        while j < n && self.lines[j].starts_with(b"***************") {
            let func: Vec<u8> = super::names::strip_eol(&self.lines[j][15..]).to_vec();
            j += 1;
            if j >= n {
                return (hunks, Some(Fatal::UnexpectedEof), j);
            }
            let Some((oa, ob)) = context_header(self.lines[j], b"*** ", b'*') else {
                return (hunks, Some(Fatal::Malformed { line: j + 1, text: self.lines[j].to_vec() }), j);
            };
            j += 1;
            let mut old: Vec<PLine> = Vec::new();
            while j < n && !is_context_separator(self.lines[j]) {
                let l = self.lines[j];
                if l.starts_with(b"\\") {
                    no_newline_at_end(&mut old);
                } else if is_context_line(l) {
                    old.push(PLine::new(if l == b"\n" { b' ' } else { l[0] }, ctx_text(l)));
                } else {
                    return (hunks, Some(Fatal::Malformed { line: j + 1, text: l.to_vec() }), j);
                }
                j += 1;
            }
            if j >= n {
                return (hunks, Some(Fatal::UnexpectedEof), j);
            }
            let Some((na, nb)) = context_header(self.lines[j], b"--- ", b'-') else {
                return (hunks, Some(Fatal::Malformed { line: j + 1, text: self.lines[j].to_vec() }), j);
            };
            j += 1;
            let mut new: Vec<PLine> = Vec::new();
            while j < n && !self.lines[j].starts_with(b"***************") && is_context_line(self.lines[j]) {
                let l = self.lines[j];
                if l.starts_with(b"\\") {
                    no_newline_at_end(&mut new);
                } else {
                    new.push(PLine::new(if l == b"\n" { b' ' } else { l[0] }, ctx_text(l)));
                }
                j += 1;
            }
            // Seção omitida: o GNU completa com as linhas de contexto da outra.
            if old.is_empty() && new.iter().any(|l| l.mark == b' ') {
                old = new.iter().filter(|l| l.mark == b' ').cloned().collect();
            }
            if new.is_empty() && old.iter().any(|l| l.mark == b' ') {
                new = old.iter().filter(|l| l.mark == b' ').cloned().collect();
            }
            let _ = (ob, nb);
            hunks.push(Hunk {
                format: Format::Context,
                old_first: if old.is_empty() { oa + 1 } else { oa },
                old,
                new_first: if new.is_empty() { na + 1 } else { na },
                new,
                func,
                normal_cmd: 0,
            });
        }
        (hunks, None, j)
    }

    /// Hunks do diff normal a partir da linha `j`.
    fn parse_normal(&self, mut j: usize) -> (Vec<Hunk>, Option<Fatal>, usize) {
        let n = self.lines.len();
        let mut hunks = Vec::new();
        while j < n {
            let Some((l1, l2, cmd, r1, r2)) = normal_command(self.lines[j]) else { break };
            j += 1;
            let mut old: Vec<PLine> = Vec::new();
            let mut new: Vec<PLine> = Vec::new();
            if cmd != b'a' {
                if let Err(fatal) = self.normal_section(&mut j, &mut old, l2.saturating_sub(l1) + 1, b"< ", b'-') {
                    return (hunks, Some(fatal), j);
                }
            }
            if cmd == b'c' {
                if j >= n {
                    return (hunks, Some(Fatal::UnexpectedEof), j);
                }
                if !self.lines[j].starts_with(b"---") {
                    return (hunks, Some(Fatal::Malformed { line: j + 1, text: self.lines[j].to_vec() }), j);
                }
                j += 1;
            }
            if cmd != b'd' {
                if let Err(fatal) = self.normal_section(&mut j, &mut new, r2.saturating_sub(r1) + 1, b"> ", b'+') {
                    return (hunks, Some(fatal), j);
                }
            }
            hunks.push(Hunk {
                format: Format::Normal,
                old_first: if cmd == b'a' { l1 + 1 } else { l1 },
                old,
                new_first: if cmd == b'd' { r1 + 1 } else { r1 },
                new,
                func: Vec::new(),
                normal_cmd: cmd,
            });
        }
        (hunks, None, j)
    }

    /// Uma seção do diff normal a partir da linha `*j`: `count` linhas com `prefix` (`< ` ou `> `),
    /// guardadas com `mark`, e os `\ No newline` no meio ou logo depois.
    fn normal_section(&self, j: &mut usize, out: &mut Vec<PLine>, count: usize, prefix: &[u8], mark: u8) -> Result<(), Fatal> {
        let n = self.lines.len();
        while out.len() < count {
            let Some(&l) = self.lines.get(*j) else { return Err(Fatal::UnexpectedEof) };
            if l.starts_with(b"\\") {
                no_newline_at_end(out);
            } else if let Some(t) = l.strip_prefix(prefix) {
                out.push(PLine::new(mark, t));
            } else {
                return Err(Fatal::Malformed { line: *j + 1, text: l.to_vec() });
            }
            *j += 1;
        }
        if *j < n && self.lines[*j].starts_with(b"\\") {
            no_newline_at_end(out);
            *j += 1;
        }
        Ok(())
    }
}

/// `@@ -a[,b] +c[,d] @@[resto]`.
fn unified_header(line: &[u8]) -> Option<(usize, usize, usize, usize, Vec<u8>)> {
    let l = super::names::strip_eol(line);
    let rest = l.strip_prefix(b"@@ -")?;
    let sp = rest.iter().position(|&c| c == b' ')?;
    let (os, ol) = parse_pair(&rest[..sp])?;
    let rest = rest[sp + 1..].strip_prefix(b"+")?;
    let sp2 = rest.iter().position(|&c| c == b' ')?;
    let (ns, nl) = parse_pair(&rest[..sp2])?;
    let rest = rest[sp2..].strip_prefix(b" @@")?;
    Some((os, ol.unwrap_or(1), ns, nl.unwrap_or(1), rest.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunks(text: &str, target: bool) -> Vec<Chunk> {
        let mut s = Scanner::new(text.as_bytes(), None, target);
        let mut v = Vec::new();
        while let Some(c) = s.next_chunk() {
            v.push(c);
        }
        v
    }

    #[test]
    fn unified_with_names_and_eof_fill() {
        let c = chunks("--- a.txt\n+++ a.txt\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n", false);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].input_line, 3);
        match &c[0].body {
            Body::Hunks { hunks, error, .. } => {
                assert!(error.is_none());
                assert_eq!(hunks[0].old.len(), 3);
                assert_eq!(hunks[0].old[2].text, b"\n");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn malformed_cases_match_gnu() {
        let err = |t: &str| match &chunks(t, false)[0].body {
            Body::Hunks { error: Some(e), .. } => String::from_utf8(e.message()).unwrap(),
            _ => panic!("sem erro"),
        };
        assert_eq!(err("--- m\n+++ m\n@@ -1,3 +1,3 @@\n a\n-b\n"), "malformed patch at line 5:  \n");
        assert_eq!(err("--- m\n+++ m\n@@ -1,3 +1,3 @@\n a\n"), "malformed patch at line 4:  \n");
        assert_eq!(err("--- m\n+++ m\n@@ -1,3 +1,3 @@\n a\nxyz\n-b\n+B\n c\n"), "malformed patch at line 5: xyz\n");
        assert_eq!(err("--- m\n+++ m\n@@ -1,2 +1,2 @@\n a\n b\n"), "malformed patch at line 5:  b\n");
        assert_eq!(err("--- m\n+++ m\n@@ -1,5 +1,5 @@\n-a\n+A\n"), "unexpected end of file in patch");
    }

    #[test]
    fn headless_needs_target() {
        assert!(chunks("@@ -1 +1 @@\n-a\n+b\n", false).is_empty());
        assert_eq!(chunks("@@ -1 +1 @@\n-a\n+b\n", true).len(), 1);
        assert!(chunks("2c2\n< a\n---\n> b\n", false).is_empty());
        assert_eq!(chunks("Index: a\n2c2\n< a\n---\n> b\n", false).len(), 1);
    }

    #[test]
    fn git_headers() {
        let c = chunks("diff --git a/old.txt b/new.txt\nsimilarity index 100%\nrename from old.txt\nrename to new.txt\n", false);
        assert_eq!(c.len(), 1);
        let g = c[0].git.as_ref().unwrap();
        assert_eq!(g.rename_from.as_deref(), Some(&b"old.txt"[..]));
        assert!(matches!(c[0].body, Body::Nothing));
    }
}
