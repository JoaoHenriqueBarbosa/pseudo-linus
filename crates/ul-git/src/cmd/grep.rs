//! `git grep`: procura nos arquivos rastreados da árvore de trabalho, no índice (`--cached`), em trees
//! (`git grep x HEAD`), nos não rastreados (`--untracked`) ou num diretório qualquer (`--no-index`). As
//! expressões combinam `-e` com `--and`, `--or`, `--not` e parênteses, como o `grep.c` do git 2.47.3.

use regex::bytes::Regex;
use sysabi::FileType;

use super::Git;
use crate::cmd::ls::quote_fully;
use crate::error::{Fail, R};
use crate::ignore::Ignores;
use crate::index::Index;
use crate::object;
use crate::os;
use crate::pathspec::Pathspec;
use crate::quote;
use crate::re::{self, Flavor};
use crate::repo::relative_to;
use crate::rev::Want;
use crate::worktree::{self, UntrackedMode};

/// Uma peça da expressão, na ordem em que apareceu na linha de comando.
enum Tok {
    Pat(Vec<u8>),
    And,
    Or,
    Not,
    Open,
    Close,
}

enum Expr {
    Pat(Regex),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum List {
    No,
    With,
    Without,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Source {
    WorkTree,
    Cached,
    NoIndex,
}

struct Opts {
    flavor: Flavor,
    icase: bool,
    word: bool,
    invert: bool,
    text: bool,
    skip_binary: bool,
    lineno: bool,
    column: bool,
    name: bool,
    full_name: bool,
    list: List,
    null: bool,
    only: bool,
    count: bool,
    heading: bool,
    brk: bool,
    before: usize,
    after: usize,
    show_func: bool,
    func_ctx: bool,
    quiet: bool,
    max_count: Option<usize>,
    all_match: bool,
    source: Source,
    untracked: bool,
    exclude_standard: Option<bool>,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            flavor: Flavor::Basic,
            icase: false,
            word: false,
            invert: false,
            text: false,
            skip_binary: false,
            lineno: false,
            column: false,
            name: true,
            full_name: false,
            list: List::No,
            null: false,
            only: false,
            count: false,
            heading: false,
            brk: false,
            before: 0,
            after: 0,
            show_func: false,
            func_ctx: false,
            quiet: false,
            max_count: None,
            all_match: false,
            source: Source::WorkTree,
            untracked: false,
            exclude_standard: None,
        }
    }
}

/// O que cada opção longa recebe.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Takes {
    Nothing,
    Value,
    /// Valor só colado (`--color=always`).
    Optional,
}

/// As opções longas do `git grep` (nome, o que recebem, se aceitam `--no-`).
const LONG: &[(&str, Takes, bool)] = &[
    ("cached", Takes::Nothing, true),
    ("no-index", Takes::Nothing, false),
    ("index", Takes::Nothing, false),
    ("untracked", Takes::Nothing, true),
    ("exclude-standard", Takes::Nothing, true),
    ("recurse-submodules", Takes::Nothing, true),
    ("invert-match", Takes::Nothing, true),
    ("ignore-case", Takes::Nothing, true),
    ("word-regexp", Takes::Nothing, true),
    ("text", Takes::Nothing, true),
    ("textconv", Takes::Nothing, true),
    ("recursive", Takes::Nothing, true),
    ("max-depth", Takes::Value, false),
    ("extended-regexp", Takes::Nothing, true),
    ("basic-regexp", Takes::Nothing, true),
    ("fixed-strings", Takes::Nothing, true),
    ("perl-regexp", Takes::Nothing, true),
    ("line-number", Takes::Nothing, true),
    ("column", Takes::Nothing, true),
    ("full-name", Takes::Nothing, true),
    ("files-with-matches", Takes::Nothing, true),
    ("name-only", Takes::Nothing, true),
    ("files-without-match", Takes::Nothing, true),
    ("null", Takes::Nothing, true),
    ("only-matching", Takes::Nothing, true),
    ("count", Takes::Nothing, true),
    ("color", Takes::Optional, true),
    ("break", Takes::Nothing, true),
    ("heading", Takes::Nothing, true),
    ("context", Takes::Value, true),
    ("before-context", Takes::Value, true),
    ("after-context", Takes::Value, true),
    ("threads", Takes::Value, true),
    ("show-function", Takes::Nothing, true),
    ("function-context", Takes::Nothing, true),
    ("and", Takes::Nothing, false),
    ("or", Takes::Nothing, false),
    ("not", Takes::Nothing, false),
    ("quiet", Takes::Nothing, true),
    ("all-match", Takes::Nothing, true),
    ("open-files-in-pager", Takes::Optional, true),
    ("ext-grep", Takes::Nothing, true),
    ("max-count", Takes::Value, true),
];

/// Opções curtas: as que levam valor e as letras de flag (com a opção longa equivalente).
const SHORT_VALUE: &[u8] = b"CBAfem";
const SHORT_FLAG: &[(u8, &str)] = &[
    (b'v', "invert-match"),
    (b'i', "ignore-case"),
    (b'y', "ignore-case"),
    (b'w', "word-regexp"),
    (b'a', "text"),
    (b'I', "I"),
    (b'r', "recursive"),
    (b'E', "extended-regexp"),
    (b'G', "basic-regexp"),
    (b'F', "fixed-strings"),
    (b'P', "perl-regexp"),
    (b'n', "line-number"),
    (b'h', "h"),
    (b'H', "H"),
    (b'l', "files-with-matches"),
    (b'L', "files-without-match"),
    (b'z', "null"),
    (b'o', "only-matching"),
    (b'c', "count"),
    (b'p', "show-function"),
    (b'W', "function-context"),
    (b'q', "quiet"),
    (b'O', "open-files-in-pager"),
];

fn usage_error(git: &Git, msg: &str) -> Fail {
    crate::opts::usage_error(git.usage(), msg)
}

fn number(git: &Git, name: &str, v: &[u8]) -> R<usize> {
    os::lossy(v).parse::<usize>().map_err(|_| usage_error(git, &format!("option `{name}' expects a numerical value")))
}

/// Aplica uma opção (pelo nome longo, ou o nome interno das curtas sem longa).
fn apply(git: &Git, o: &mut Opts, toks: &mut Vec<Tok>, name: &str, neg: bool, value: Option<Vec<u8>>) -> R<()> {
    let on = !neg;
    match name {
        "cached" => o.source = if on { Source::Cached } else { Source::WorkTree },
        "no-index" => o.source = Source::NoIndex,
        "index" => o.source = Source::WorkTree,
        "untracked" => o.untracked = on,
        "exclude-standard" => o.exclude_standard = Some(on),
        "invert-match" => o.invert = on,
        "ignore-case" => o.icase = on,
        "word-regexp" => o.word = on,
        "text" => o.text = on,
        "I" => o.skip_binary = true,
        "extended-regexp" => o.flavor = if on { Flavor::Extended } else { Flavor::Basic },
        "basic-regexp" => o.flavor = Flavor::Basic,
        "fixed-strings" => o.flavor = if on { Flavor::Fixed } else { Flavor::Basic },
        "perl-regexp" => o.flavor = if on { Flavor::Perl } else { Flavor::Basic },
        "line-number" => o.lineno = on,
        "column" => o.column = on,
        "h" => o.name = false,
        "H" => o.name = true,
        "full-name" => o.full_name = on,
        "files-with-matches" | "name-only" => o.list = if on { List::With } else { List::No },
        "files-without-match" => o.list = if on { List::Without } else { List::No },
        "null" => o.null = on,
        "only-matching" => o.only = on,
        "count" => o.count = on,
        "break" => o.brk = on,
        "heading" => o.heading = on,
        "context" => {
            let n = if on { number(git, "context", &value.unwrap_or_default())? } else { 0 };
            o.before = n;
            o.after = n;
        }
        "before-context" => o.before = if on { number(git, "before-context", &value.unwrap_or_default())? } else { 0 },
        "after-context" => o.after = if on { number(git, "after-context", &value.unwrap_or_default())? } else { 0 },
        "show-function" => o.show_func = on,
        "function-context" => o.func_ctx = on,
        "quiet" => o.quiet = on,
        "all-match" => o.all_match = on,
        "max-count" => o.max_count = if on { Some(number(git, "max-count", &value.unwrap_or_default())?) } else { None },
        "and" => toks.push(Tok::And),
        "or" => toks.push(Tok::Or),
        "not" => toks.push(Tok::Not),
        "e" => toks.push(Tok::Pat(value.unwrap_or_default())),
        "f" => {
            let path = value.unwrap_or_default();
            let data = if path == b"-" {
                os::stdin_all()
            } else {
                match os::read_opt(&path) {
                    Ok(Some(d)) => d,
                    _ => return Err(Fail::Fatal(format!("cannot open '{}': No such file or directory", os::lossy(&path)))),
                }
            };
            for line in data.split(|c| *c == b'\n') {
                if !line.is_empty() {
                    toks.push(Tok::Pat(line.to_vec()));
                }
            }
        }
        "C" => {
            let n = number(git, "context", &value.unwrap_or_default())?;
            o.before = n;
            o.after = n;
        }
        "B" => o.before = number(git, "before-context", &value.unwrap_or_default())?,
        "A" => o.after = number(git, "after-context", &value.unwrap_or_default())?,
        "m" => o.max_count = Some(number(git, "max-count", &value.unwrap_or_default())?),
        _ => {}
    }
    Ok(())
}

/// Varre os argumentos como o `parse_options` do grep: opções em qualquer posição até o `--`; devolve os
/// argumentos que sobram (padrão, revisões, caminhos) e os que vieram depois do `--`.
fn parse(git: &Git, args: &[Vec<u8>], o: &mut Opts, toks: &mut Vec<Tok>) -> R<(Vec<Vec<u8>>, Option<Vec<Vec<u8>>>)> {
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if a == b"--" {
            return Ok((rest, Some(args[i..].to_vec())));
        }
        if a == b"(" {
            toks.push(Tok::Open);
            continue;
        }
        if a == b")" {
            toks.push(Tok::Close);
            continue;
        }
        if a.len() < 2 || a[0] != b'-' {
            rest.push(a.clone());
            continue;
        }
        if let Some(long) = a.strip_prefix(b"--") {
            let s = os::lossy(long);
            let (key, val) = match s.split_once('=') {
                Some((k, v)) => (k.to_string(), Some(v.as_bytes().to_vec())),
                None => (s.clone(), None),
            };
            if key == "no-index" && !rest.is_empty() {
                return Err(Fail::Fatal("option '--no-index' must come before non-option arguments".into()));
            }
            let (neg, base) = match key.strip_prefix("no-") {
                Some(b) if LONG.iter().any(|(n, _, ng)| *n == b && *ng) => (true, b.to_string()),
                _ => (false, key.clone()),
            };
            // Nome exato, ou abreviação que só serve a uma opção.
            let found = LONG.iter().find(|(n, _, _)| *n == base).or_else(|| {
                let cands: Vec<_> = LONG.iter().filter(|(n, _, _)| n.starts_with(base.as_str())).collect();
                if cands.len() == 1 { Some(cands[0]) } else { None }
            });
            let Some(&(name, takes, _)) = found else {
                return Err(usage_error(git, &format!("unknown option `{key}'")));
            };
            let value = match (takes, neg) {
                (Takes::Value, false) => match val {
                    Some(v) => Some(v),
                    None => {
                        let Some(v) = args.get(i) else {
                            return Err(crate::opts::error_only(&format!("option `{name}' requires a value")));
                        };
                        i += 1;
                        Some(v.clone())
                    }
                },
                _ => val,
            };
            apply(git, o, toks, name, neg, value)?;
            continue;
        }
        // `-NUM` é `-C NUM`.
        if a[1..].iter().all(u8::is_ascii_digit) {
            apply(git, o, toks, "C", false, Some(a[1..].to_vec()))?;
            continue;
        }
        let mut j = 1;
        while j < a.len() {
            let c = a[j];
            j += 1;
            if SHORT_VALUE.contains(&c) {
                let value = if j < a.len() {
                    let v = a[j..].to_vec();
                    j = a.len();
                    v
                } else {
                    let Some(v) = args.get(i) else {
                        return Err(crate::opts::error_only(&format!("switch `{}' requires a value", c as char)));
                    };
                    i += 1;
                    v.clone()
                };
                apply(git, o, toks, &(c as char).to_string(), false, Some(value))?;
            } else if let Some((_, name)) = SHORT_FLAG.iter().find(|(s, _)| *s == c) {
                apply(git, o, toks, name, false, None)?;
            } else if c.is_ascii_digit() {
                // `-n3` e afins: os dígitos até o fim são o contexto.
                let k = a[j - 1..].iter().take_while(|d| d.is_ascii_digit()).count();
                apply(git, o, toks, "C", false, Some(a[j - 1..j - 1 + k].to_vec()))?;
                j = j - 1 + k;
            } else {
                return Err(usage_error(git, &format!("unknown switch `{}'", c as char)));
            }
        }
    }
    Ok((rest, None))
}

/// O parser de expressão do `grep.c`: `--or` (ou só justaposição) liga mais fraco que `--and`, que liga
/// mais fraco que `--not`.
struct ExprParser<'a> {
    toks: &'a [Tok],
    pos: usize,
    opts: &'a Opts,
}

impl ExprParser<'_> {
    fn compile(&self, p: &[u8]) -> R<Regex> {
        re::compile(p, self.opts.flavor, self.opts.icase)
            .map_err(|e| Fail::Fatal(format!("command line, '{}': {e}", os::lossy(p))))
    }

    fn or(&mut self) -> R<Expr> {
        let mut x = self.and()?;
        while self.pos < self.toks.len() && !matches!(self.toks[self.pos], Tok::Close) {
            if matches!(self.toks[self.pos], Tok::Or) {
                self.pos += 1;
            }
            let y = self.and()?;
            x = Expr::Or(Box::new(x), Box::new(y));
        }
        Ok(x)
    }

    fn and(&mut self) -> R<Expr> {
        let mut x = self.not()?;
        while matches!(self.toks.get(self.pos), Some(Tok::And)) {
            self.pos += 1;
            let y = self.not()?;
            x = Expr::And(Box::new(x), Box::new(y));
        }
        Ok(x)
    }

    fn not(&mut self) -> R<Expr> {
        if matches!(self.toks.get(self.pos), Some(Tok::Not)) {
            self.pos += 1;
            if self.pos >= self.toks.len() {
                return Err(Fail::Fatal("--not not followed by pattern expression".into()));
            }
            return Ok(Expr::Not(Box::new(self.not()?)));
        }
        self.atom()
    }

    fn atom(&mut self) -> R<Expr> {
        match self.toks.get(self.pos) {
            Some(Tok::Pat(p)) => {
                self.pos += 1;
                Ok(Expr::Pat(self.compile(p)?))
            }
            Some(Tok::Open) => {
                self.pos += 1;
                let x = self.or()?;
                if !matches!(self.toks.get(self.pos), Some(Tok::Close)) {
                    return Err(Fail::Fatal("unmatched ( for expression group".into()));
                }
                self.pos += 1;
                Ok(x)
            }
            Some(Tok::Close) => Err(Fail::Fatal("unmatched ) for expression group".into())),
            _ => Err(Fail::Fatal("incomplete pattern expression group".into())),
        }
    }
}

fn word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// As ocorrências de `re` em `line`; com `-w`, só as que têm borda de palavra dos dois lados (se uma
/// falha, procura de novo um byte adiante, como o `match_one_pattern`).
fn occurrences(re: &Regex, line: &[u8], word: bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at <= line.len() {
        let Some(m) = re.find_at(line, at) else { break };
        let (s, e) = (m.start(), m.end());
        let ok = !word || ((s == 0 || !word_byte(line[s - 1])) && (e == line.len() || !word_byte(line[e])) && e > s);
        if ok {
            out.push((s, e));
            at = if e > s { e } else { e + 1 };
        } else {
            at = s + 1;
        }
    }
    out
}

impl Expr {
    fn matches(&self, line: &[u8], word: bool) -> bool {
        match self {
            Expr::Pat(re) => {
                if word {
                    !occurrences(re, line, true).is_empty()
                } else {
                    re.is_match(line)
                }
            }
            Expr::Not(x) => !x.matches(line, word),
            Expr::And(a, b) => a.matches(line, word) && b.matches(line, word),
            Expr::Or(a, b) => a.matches(line, word) || b.matches(line, word),
        }
    }

    /// As ocorrências dos padrões positivos (fora de `--not`), em ordem.
    fn spans(&self, line: &[u8], word: bool, out: &mut Vec<(usize, usize)>) {
        match self {
            Expr::Pat(re) => out.extend(occurrences(re, line, word)),
            Expr::Not(_) => {}
            Expr::And(a, b) | Expr::Or(a, b) => {
                a.spans(line, word, out);
                b.spans(line, word, out);
            }
        }
    }
}

/// O que `--all-match` exige: cada padrão de topo (os termos do `--or`) casou alguma linha do arquivo.
fn or_terms(e: &Expr) -> Vec<&Expr> {
    match e {
        Expr::Or(a, b) => {
            let mut v = or_terms(a);
            v.extend(or_terms(b));
            v
        }
        x => vec![x],
    }
}

/// O `funcname` padrão do git: linha que começa com letra, `_` ou `$`.
fn is_funcname(line: &[u8]) -> bool {
    line.first().is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_' || *c == b'$')
}

/// Um arquivo a examinar: o nome como aparece na saída e o conteúdo.
struct Item {
    name: Vec<u8>,
    data: Vec<u8>,
}

struct Grep<'a> {
    o: &'a Opts,
    expr: &'a Expr,
    out: Vec<u8>,
    /// Algum arquivo já produziu saída (para `--break` e o `--` entre arquivos).
    any_shown: bool,
    hit: bool,
}

impl Grep<'_> {
    fn prefix(&mut self, name: &[u8], lno: usize, col: Option<usize>, sep: u8) {
        if self.o.name && !self.o.heading {
            self.out.extend_from_slice(name);
            self.out.push(if self.o.null { 0 } else { sep });
        }
        if self.o.lineno {
            self.out.extend_from_slice(lno.to_string().as_bytes());
            self.out.push(sep);
        }
        if let Some(c) = col {
            self.out.extend_from_slice(c.to_string().as_bytes());
            self.out.push(sep);
        }
    }

    fn emit_line(&mut self, name: &[u8], lines: &[&[u8]], i: usize, sep: u8) {
        let line = lines[i];
        if sep == b':' && self.o.only {
            let mut spans = Vec::new();
            self.expr.spans(line, self.o.word, &mut spans);
            spans.sort_unstable();
            for (s, e) in spans {
                self.prefix(name, i + 1, self.o.column.then_some(s + 1), sep);
                self.out.extend_from_slice(&line[s..e]);
                self.out.push(b'\n');
            }
            return;
        }
        let col = if self.o.column && sep == b':' && !self.o.invert {
            let mut spans = Vec::new();
            self.expr.spans(line, self.o.word, &mut spans);
            spans.iter().map(|s| s.0 + 1).min()
        } else {
            None
        };
        self.prefix(name, i + 1, col, sep);
        self.out.extend_from_slice(line);
        self.out.push(b'\n');
    }

    /// Examina um arquivo; devolve se ele casou.
    fn file(&mut self, item: &Item) -> bool {
        let o = self.o;
        let data = &item.data;
        let binary = !o.text && data[..data.len().min(8000)].contains(&0);
        if binary && o.skip_binary {
            return false;
        }
        let mut lines: Vec<&[u8]> = data.split(|c| *c == b'\n').collect();
        if data.ends_with(b"\n") || data.is_empty() {
            lines.pop();
        }
        let matched: Vec<bool> = lines.iter().map(|l| self.expr.matches(l, o.word) != o.invert).collect();
        if o.all_match && !o.invert {
            let terms = or_terms(self.expr);
            if !terms.iter().all(|t| lines.iter().any(|l| t.matches(l, o.word))) {
                return self.finish_unmatched(item);
            }
        }
        let limit = o.max_count.unwrap_or(usize::MAX);
        let total = matched.iter().filter(|m| **m).count().min(limit);
        if total == 0 {
            return self.finish_unmatched(item);
        }
        if o.quiet {
            return true;
        }
        match o.list {
            List::With => {
                self.out.extend_from_slice(&item.name);
                self.out.push(if o.null { 0 } else { b'\n' });
                return true;
            }
            List::Without => return true,
            List::No => {}
        }
        if o.count {
            self.out.extend_from_slice(&item.name);
            self.out.push(if o.null { 0 } else { b':' });
            self.out.extend_from_slice(format!("{total}\n").as_bytes());
            return true;
        }
        if binary {
            if !o.heading {
                self.out.extend_from_slice(b"Binary file ");
                self.out.extend_from_slice(&item.name);
                self.out.extend_from_slice(b" matches\n");
            }
            self.any_shown = true;
            return true;
        }
        let context = o.before > 0 || o.after > 0 || o.func_ctx;
        if o.brk && self.any_shown {
            self.out.push(b'\n');
        }
        if o.heading && o.name {
            self.out.extend_from_slice(&item.name);
            self.out.push(b'\n');
        }
        let mut last: Option<usize> = None;
        let mut shown_here = false;
        let mut until = 0usize;
        let mut count = 0usize;
        let name = item.name.clone();
        let mut i = 0;
        while i < lines.len() {
            if matched[i] && count < limit {
                count += 1;
                // Começo do trecho: o contexto de antes, ou a linha de função com -W.
                let mut start = i.saturating_sub(o.before);
                let mut func_line = None;
                if o.func_ctx {
                    if let Some(f) = (0..=i).rev().find(|&k| is_funcname(lines[k])) {
                        start = start.min(f);
                        func_line = Some(f);
                    }
                }
                if let Some(l) = last {
                    start = start.max(l + 1);
                }
                if context && (last.is_some_and(|l| start > l + 1) || (last.is_none() && self.any_shown)) {
                    self.out.extend_from_slice(b"--\n");
                }
                if o.show_func && !o.func_ctx {
                    let floor = last.map_or(0, |l| l + 1);
                    if let Some(f) = (floor..start).rev().find(|&k| is_funcname(lines[k])) {
                        self.emit_line(&name, &lines, f, b'=');
                    }
                }
                for k in start..i {
                    let sep = if Some(k) == func_line { b'=' } else { b'-' };
                    self.emit_line(&name, &lines, k, sep);
                }
                self.emit_line(&name, &lines, i, b':');
                last = Some(i);
                shown_here = true;
                until = i + o.after;
                if o.func_ctx {
                    let end = (i + 1..lines.len()).find(|&k| is_funcname(lines[k])).unwrap_or(lines.len());
                    until = until.max(end.saturating_sub(1));
                }
            } else if last.is_some_and(|l| i > l) && i <= until && (count < limit || !matched[i]) {
                self.emit_line(&name, &lines, i, b'-');
                last = Some(i);
            } else if count >= limit && i > until {
                break;
            }
            i += 1;
        }
        if shown_here {
            self.any_shown = true;
        }
        true
    }

    fn finish_unmatched(&mut self, item: &Item) -> bool {
        if self.o.list == List::Without && !self.o.quiet {
            self.out.extend_from_slice(&item.name);
            self.out.push(if self.o.null { 0 } else { b'\n' });
            self.hit = true;
        }
        false
    }
}

/// Os arquivos de `dir` (relativo ao diretório atual), recursivo, sem `.git`, em ordem de nome.
fn walk_dir(dir: &[u8], out: &mut Vec<Vec<u8>>) {
    for (n, kind) in worktree::sorted_dir(dir) {
        if n == b".git" {
            continue;
        }
        let path = worktree::join_rel(dir, &n);
        match kind {
            FileType::Directory => walk_dir(&path, out),
            _ => out.push(path),
        }
    }
}

/// O conteúdo de um arquivo da árvore de trabalho como o git o lê: link simbólico vira o destino.
fn read_worktree(path: &[u8]) -> Option<Vec<u8>> {
    let st = os::lstat(path).ok()?;
    match st.file_type() {
        FileType::Symlink => os::readlink(path).ok(),
        FileType::Regular => os::read_opt(path).ok().flatten(),
        _ => None,
    }
}

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let mut o = Opts::default();
    let mut toks = Vec::new();
    let (mut rest, after_dashdash) = parse(git, args, &mut o, &mut toks)?;
    if toks.iter().all(|t| !matches!(t, Tok::Pat(_))) {
        if rest.is_empty() {
            return Err(Fail::Fatal("no pattern given".into()));
        }
        toks.push(Tok::Pat(rest.remove(0)));
    }
    let expr = {
        let mut p = ExprParser { toks: &toks, pos: 0, opts: &o };
        let e = p.or()?;
        if p.pos < toks.len() {
            return Err(Fail::Fatal("unmatched ) for expression group".into()));
        }
        e
    };

    if o.source == Source::NoIndex {
        let mut paths = rest;
        paths.extend(after_dashdash.unwrap_or_default());
        // Achar o repositório leva o processo para o topo; os caminhos continuam vindo do diretório atual.
        let prefix = git.repo.as_ref().map(|r| r.prefix.clone()).unwrap_or_default();
        return no_index(&o, &expr, &paths, &prefix);
    }
    if git.repo.is_none() {
        return Err(crate::repo::not_a_repository());
    }

    // Revisões até o primeiro argumento que não é uma; dali em diante são caminhos.
    let mut revs: Vec<(Vec<u8>, crate::hash::Oid)> = Vec::new();
    let mut paths: Vec<Vec<u8>> = Vec::new();
    {
        let repo = git.repo()?;
        let mut k = 0;
        while k < rest.len() {
            match repo.rev_parse_want(&rest[k], Want::Treeish)? {
                Some(id) => {
                    let Some(tree) = repo.peel_to_tree(&id)? else {
                        return Err(Fail::Fatal(format!("unable to read tree ({})", os::lossy(&rest[k]))));
                    };
                    revs.push((rest[k].clone(), tree));
                    k += 1;
                }
                None => break,
            }
        }
        for p in &rest[k..] {
            if after_dashdash.is_none() && !os::exists(p) {
                return Err(Fail::Fatal(format!(
                    "ambiguous argument '{}': unknown revision or path not in the working tree.\nUse '--' to separate paths from revisions, like this:\n'git <command> [<revision>...] -- [<file>...]'",
                    os::lossy(p)
                )));
            }
            paths.push(p.clone());
        }
        paths.extend(after_dashdash.unwrap_or_default());
    }
    if !revs.is_empty() && (o.source == Source::Cached || o.untracked) {
        return Err(Fail::Fatal(if o.untracked { "--untracked cannot be used with revisions" } else { "--cached cannot be used with revisions" }.into()));
    }
    let repo = git.repo()?;
    let ps: Pathspec = if paths.is_empty() && !repo.prefix.is_empty() { git.pathspec(&[b".".to_vec()])? } else { git.pathspec(&paths)? };
    let fully = quote_fully(repo);
    let display = |path: &[u8]| -> Vec<u8> {
        let shown = if o.full_name { path.to_vec() } else { relative_to(path, &repo.prefix) };
        if o.null { shown } else { quote::quote_c(&shown, fully) }
    };

    let mut items: Vec<(Vec<u8>, Box<dyn Fn() -> R<Option<Vec<u8>>> + '_>)> = Vec::new();
    if revs.is_empty() {
        let idx = Index::load(&repo.index_path())?;
        let top = repo.work_tree.clone().unwrap_or_default();
        let mut seen: Option<Vec<u8>> = None;
        for e in &idx.entries {
            if seen.as_deref() == Some(e.path.as_slice()) || object::is_gitlink(e.mode) || ps.matches(&e.path, false, None).is_none() {
                continue;
            }
            seen = Some(e.path.clone());
            let name = display(&e.path);
            if o.source == Source::Cached {
                if e.stage != 0 {
                    continue;
                }
                let oid = e.oid;
                items.push((name, Box::new(move || Ok(Some(repo.read_object(&oid)?.1.to_vec())))));
            } else {
                if e.skip_worktree() {
                    continue;
                }
                let full = os::join(&top, &e.path);
                items.push((name, Box::new(move || Ok(read_worktree(&full)))));
            }
        }
        if o.untracked && o.source == Source::WorkTree {
            let mode = UntrackedMode::All;
            let found = if o.exclude_standard == Some(false) {
                worktree::untracked_using(Ignores::none(), &idx, &ps, mode)
            } else {
                worktree::untracked(repo, &idx, &ps, mode)
            };
            for p in found {
                let full = os::join(&top, &p);
                items.push((display(&p), Box::new(move || Ok(read_worktree(&full)))));
            }
            items.sort_by(|a, b| a.0.cmp(&b.0));
        }
    } else {
        for (label, tree) in &revs {
            for (path, (mode, oid)) in repo.flatten_tree(tree)? {
                if object::is_gitlink(mode) || ps.matches(&path, false, None).is_none() {
                    continue;
                }
                let mut name = label.clone();
                name.push(b':');
                name.extend_from_slice(&display(&path));
                items.push((name, Box::new(move || Ok(Some(repo.read_object(&oid)?.1.to_vec())))));
            }
        }
    }

    let mut g = Grep { o: &o, expr: &expr, out: Vec::new(), any_shown: false, hit: false };
    for (name, read) in &items {
        let Some(data) = read()? else { continue };
        if g.file(&Item { name: name.clone(), data }) {
            if o.list != List::Without {
                g.hit = true;
            }
            if o.quiet {
                break;
            }
        }
        if g.out.len() > 1 << 16 {
            os::out(&std::mem::take(&mut g.out));
        }
    }
    os::out(&g.out);
    Ok(if g.hit { 0 } else { 1 })
}

/// `--no-index`: os arquivos do diretório atual (ou dos caminhos dados), rastreados ou não.
fn no_index(o: &Opts, expr: &Expr, paths: &[Vec<u8>], prefix: &[u8]) -> R<i32> {
    let base = prefix.strip_suffix(b"/").unwrap_or(prefix);
    let mut files = Vec::new();
    if paths.is_empty() {
        walk_dir(base, &mut files);
    } else {
        for p in paths {
            let p = p.strip_suffix(b"/").unwrap_or(p);
            let p = if p == b"." { base.to_vec() } else { worktree::join_rel(base, p) };
            if p.is_empty() || os::is_dir(&p) {
                walk_dir(&p, &mut files);
            } else {
                files.push(p);
            }
        }
    }
    let mut g = Grep { o, expr, out: Vec::new(), any_shown: false, hit: false };
    for f in files {
        let Some(data) = read_worktree(&f) else { continue };
        if g.file(&Item { name: relative_to(&f, prefix), data }) {
            if o.list != List::Without {
                g.hit = true;
            }
            if o.quiet {
                break;
            }
        }
    }
    os::out(&g.out);
    Ok(if g.hit { 0 } else { 1 })
}
