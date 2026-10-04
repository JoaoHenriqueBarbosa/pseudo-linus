//! Mini-shell de TESTE do ul-git: `bash`/`sh` e utilitários mínimos sobre o `sysabi`.
//!
//! Só existe pra rodar, no kernel de teste, os casos `script` do corpus do git enquanto o crate
//! `shell` do projeto não fica pronto. Não é entrega de produto: cobre o que agentes escrevem em
//! volta do git (listas, pipelines, redirecionamentos, heredocs, `$(...)`, aspas, variáveis, `for`,
//! `if`, `while`, glob) e utilitários pequenos (`cat`, `printf`, `rm`, `ls`, `grep`, `sed`...).
//! Tudo passa por `sysabi::sys`; nada toca o host.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::sync::Arc;

use sysabi::{
    AccessMode, AtFlags, Ctx, Errno, Fd, FdAction, FileType, OFlags, ProcAttrs, Program, RenameFlags, SetTime, SpawnSpec, Syscalls,
    WaitOptions, WaitTarget, sys,
};

/// Os programas do mini-shell: `bash`, `sh` e os utilitários.
pub fn programs() -> Vec<Program> {
    vec![
        Program::bin("bash", bash_main),
        Program::bin("sh", bash_main),
        Program::bin("cat", cat_main),
        Program::bin("echo", echo_main),
        Program::bin("printf", printf_main),
        Program::bin("rm", rm_main),
        Program::bin("mkdir", mkdir_main),
        Program::bin("ls", ls_main),
        Program::bin("mv", mv_main),
        Program::bin("cp", cp_main),
        Program::bin("touch", touch_main),
        Program::bin("head", head_main),
        Program::bin("tail", tail_main),
        Program::bin("wc", wc_main),
        Program::bin("sort", sort_main),
        Program::bin("grep", grep_main),
        Program::bin("sed", sed_main),
        Program::bin("true", true_main),
        Program::bin("false", false_main),
        Program::bin("test", test_main),
        Program::bin("[", bracket_main),
        Program::bin("chmod", chmod_main),
        Program::bin("ln", ln_main),
        Program::bin("pwd", pwd_main),
        Program::bin("env", env_main),
        Program::bin("tr", tr_main),
        Program::bin("xargs", xargs_main),
        Program::bin("basename", basename_main),
        Program::bin("dirname", dirname_main),
        Program::bin("seq", seq_main),
        Program::bin("sleep", true_main),
        Program::bin("uniq", uniq_main),
        Program::bin("cut", cut_main),
    ]
}

// ---------------------------------------------------------------------------------------------
// Utilidades de E/S
// ---------------------------------------------------------------------------------------------

fn sysc() -> Arc<dyn Syscalls> {
    sys::current()
}

fn write_fd(fd: Fd, data: &[u8]) {
    let _ = sys::write_all(fd, data);
}

fn out(data: &[u8]) {
    write_fd(Fd::STDOUT, data);
}

fn err(data: &[u8]) {
    write_fd(Fd::STDERR, data);
}

fn errs(s: &str) {
    err(s.as_bytes());
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn to_args(args: &[OsString]) -> Vec<Vec<u8>> {
    args.iter().map(|a| a.as_bytes().to_vec()).collect()
}

fn join_path(a: &[u8], b: &[u8]) -> Vec<u8> {
    if b.starts_with(b"/") || a.is_empty() {
        return b.to_vec();
    }
    let mut p = a.to_vec();
    if !p.ends_with(b"/") {
        p.push(b'/');
    }
    p.extend_from_slice(b);
    p
}

fn base_name(p: &[u8]) -> &[u8] {
    let t = p.strip_suffix(b"/").unwrap_or(p);
    match t.iter().rposition(|c| *c == b'/') {
        Some(i) => &t[i + 1..],
        None => t,
    }
}

fn lstat(p: &[u8]) -> Result<sysabi::Stat, Errno> {
    sysc().fstatat(Fd::CWD, p, AtFlags::SYMLINK_NOFOLLOW)
}

fn stat(p: &[u8]) -> Result<sysabi::Stat, Errno> {
    sysc().fstatat(Fd::CWD, p, AtFlags::empty())
}

fn is_dir(p: &[u8]) -> bool {
    stat(p).map(|s| s.file_type() == FileType::Directory).unwrap_or(false)
}

fn read_all_fd(fd: Fd) -> Vec<u8> {
    sys::read_to_end(fd).unwrap_or_default()
}

/// Lê um arquivo ou o stdin (`-`).
fn read_input(name: &[u8]) -> Result<Vec<u8>, Errno> {
    if name == b"-" {
        return Ok(read_all_fd(Fd::STDIN));
    }
    let s = sysc();
    let fd = s.openat(Fd::CWD, name, OFlags::RDONLY | OFlags::CLOEXEC, 0)?;
    if let Ok(st) = s.fstat(fd)
        && st.file_type() == FileType::Directory
    {
        let _ = s.close(fd);
        return Err(Errno::EISDIR);
    }
    let d = sys::read_to_end(fd);
    let _ = s.close(fd);
    d
}

fn write_file(p: &[u8], data: &[u8], flags: OFlags) -> Result<(), Errno> {
    let s = sysc();
    let fd = s.openat(Fd::CWD, p, OFlags::WRONLY | OFlags::CREAT | OFlags::CLOEXEC | flags, 0o666)?;
    let r = sys::write_all(fd, data);
    let _ = s.close(fd);
    r
}

fn split_lines(d: &[u8]) -> Vec<&[u8]> {
    if d.is_empty() {
        return Vec::new();
    }
    let body = d.strip_suffix(b"\n").unwrap_or(d);
    body.split(|c| *c == b'\n').collect()
}

// ---------------------------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Part {
    /// Texto sem aspas (sujeito a glob).
    Lit(Vec<u8>),
    /// Texto entre aspas simples ou escapado.
    Quoted(Vec<u8>),
    /// Entre aspas duplas.
    Dq(Vec<Part>),
    /// `$x`, `${x}`, `${x:-w}`.
    Var(String, Option<(String, Word)>),
    /// `$(...)` ou crase.
    Subst(List),
    /// `$((...))`.
    Arith(Vec<u8>),
}

type Word = Vec<Part>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ROp {
    In,
    Out,
    Append,
    DupOut,
    DupIn,
    BothOut,
    BothAppend,
    HereDoc,
    HereStr,
    ReadWrite,
}

#[derive(Clone, Debug)]
enum Target {
    Word(Word),
    Heredoc(usize),
    Body(Word),
}

#[derive(Clone, Debug)]
struct Redir {
    fd: i32,
    op: ROp,
    target: Target,
}

#[derive(Clone, Debug, Default)]
struct Simple {
    assigns: Vec<(String, Word)>,
    words: Vec<Word>,
    redirs: Vec<Redir>,
    line: usize,
}

#[derive(Clone, Debug)]
enum Cmd {
    Simple(Simple),
    Group(List, Vec<Redir>),
    Sub(List, Vec<Redir>),
    For(String, Option<Vec<Word>>, List, Vec<Redir>),
    If(Vec<(List, List)>, Option<List>, Vec<Redir>),
    While(List, List, bool, Vec<Redir>),
}

#[derive(Clone, Debug)]
struct Pipeline {
    neg: bool,
    cmds: Vec<Cmd>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Conn {
    And,
    Or,
}

#[derive(Clone, Debug)]
struct AndOr {
    first: Pipeline,
    rest: Vec<(Conn, Pipeline)>,
}

type List = Vec<AndOr>;

// ---------------------------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------------------------

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
    line: usize,
    pending: Vec<(Vec<u8>, bool, bool, usize)>,
    bodies: Vec<Option<Word>>,
}

type PResult<T> = Result<T, String>;

fn is_meta(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b';' | b'&' | b'|' | b'<' | b'>' | b'(' | b')')
}

fn is_name_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

impl<'a> Parser<'a> {
    fn new(s: &'a [u8]) -> Parser<'a> {
        Parser { s, pos: 0, line: 1, pending: Vec::new(), bodies: Vec::new() }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn peek_at(&self, k: usize) -> Option<u8> {
        self.s.get(self.pos + k).copied()
    }

    fn eof(&self) -> bool {
        self.pos >= self.s.len()
    }

    fn starts(&self, t: &[u8]) -> bool {
        self.s[self.pos.min(self.s.len())..].starts_with(t)
    }

    /// Consome um `\n` e lê os heredocs pendentes.
    fn newline(&mut self) -> PResult<()> {
        self.pos += 1;
        self.line += 1;
        self.read_heredocs()
    }

    fn read_heredocs(&mut self) -> PResult<()> {
        let pending = std::mem::take(&mut self.pending);
        for (delim, strip, expand, id) in pending {
            let mut body = Vec::new();
            while self.pos < self.s.len() {
                let end = self.s[self.pos..].iter().position(|c| *c == b'\n').map(|p| p + self.pos).unwrap_or(self.s.len());
                let mut line = &self.s[self.pos..end];
                self.pos = (end + 1).min(self.s.len());
                if end < self.s.len() {
                    self.line += 1;
                }
                if strip {
                    while line.first() == Some(&b'\t') {
                        line = &line[1..];
                    }
                }
                if line == delim.as_slice() {
                    break;
                }
                body.extend_from_slice(line);
                body.push(b'\n');
            }
            let word = if expand {
                let mut sub = Parser::new(&body);
                let parts = sub.parse_dq_inner(true)?;
                let mut w = parts;
                let bodies = sub.bodies.clone();
                fill_word(&mut w, &bodies);
                w
            } else {
                vec![Part::Quoted(body)]
            };
            self.bodies[id] = Some(word);
        }
        Ok(())
    }

    fn skip_blanks(&mut self) {
        loop {
            match self.peek() {
                Some(b' ') | Some(b'\t') => self.pos += 1,
                Some(b'\\') if self.peek_at(1) == Some(b'\n') => {
                    self.pos += 2;
                    self.line += 1;
                }
                Some(b'#') => {
                    while let Some(c) = self.peek() {
                        if c == b'\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
    }

    fn skip_linebreaks(&mut self) -> PResult<()> {
        loop {
            self.skip_blanks();
            if self.peek() == Some(b'\n') {
                self.newline()?;
            } else {
                return Ok(());
            }
        }
    }

    /// Palavra literal na posição (pra reconhecer palavras reservadas).
    fn peek_reserved(&self) -> Option<&'a [u8]> {
        let start = self.pos;
        let mut i = start;
        while i < self.s.len() && !is_meta(self.s[i]) {
            if matches!(self.s[i], b'\'' | b'"' | b'$' | b'\\' | b'`') {
                return None;
            }
            i += 1;
        }
        if i == start { None } else { Some(&self.s[start..i]) }
    }

    fn is_reserved(&self, w: &str) -> bool {
        self.peek_reserved() == Some(w.as_bytes())
    }

    fn expect_reserved(&mut self, w: &str) -> PResult<()> {
        self.skip_linebreaks()?;
        if self.is_reserved(w) {
            self.pos += w.len();
            Ok(())
        } else {
            Err(self.unexpected())
        }
    }

    fn unexpected(&self) -> String {
        match self.peek() {
            None => "syntax error: unexpected end of file".to_string(),
            Some(b'\n') => "syntax error near unexpected token `newline'".to_string(),
            Some(_) => {
                let t = match self.peek_reserved() {
                    Some(w) => lossy(w),
                    None => {
                        let mut e = self.pos + 1;
                        if matches!(self.peek(), Some(b'&') | Some(b'|') | Some(b';')) && self.peek_at(1) == self.peek() {
                            e += 1;
                        }
                        lossy(&self.s[self.pos..e.min(self.s.len())])
                    }
                };
                format!("syntax error near unexpected token `{t}'")
            }
        }
    }

    fn parse_program(&mut self) -> PResult<List> {
        let mut list = self.parse_list(&[], false)?;
        self.skip_linebreaks()?;
        if !self.eof() {
            return Err(self.unexpected());
        }
        if !self.pending.is_empty() {
            self.read_heredocs()?;
        }
        let bodies = self.bodies.clone();
        fill_list(&mut list, &bodies);
        Ok(list)
    }

    fn parse_list(&mut self, stops: &[&str], rparen: bool) -> PResult<List> {
        let mut list = Vec::new();
        loop {
            self.skip_linebreaks()?;
            let Some(c) = self.peek() else { break };
            if c == b')' {
                if rparen {
                    break;
                }
                return Err(self.unexpected());
            }
            if let Some(w) = self.peek_reserved()
                && stops.iter().any(|s| s.as_bytes() == w)
            {
                break;
            }
            if c == b';' {
                return Err(self.unexpected());
            }
            let ao = self.parse_and_or()?;
            list.push(ao);
            self.skip_blanks();
            match self.peek() {
                Some(b';') if self.peek_at(1) != Some(b';') => self.pos += 1,
                Some(b'&') if self.peek_at(1) != Some(b'&') => self.pos += 1,
                Some(b'\n') => self.newline()?,
                None => break,
                Some(b')') if rparen => break,
                Some(_) => {
                    if let Some(w) = self.peek_reserved()
                        && stops.iter().any(|s| s.as_bytes() == w)
                    {
                        break;
                    }
                    return Err(self.unexpected());
                }
            }
        }
        Ok(list)
    }

    fn parse_and_or(&mut self) -> PResult<AndOr> {
        let first = self.parse_pipeline()?;
        let mut rest = Vec::new();
        loop {
            self.skip_blanks();
            if self.starts(b"&&") {
                self.pos += 2;
                self.skip_linebreaks()?;
                rest.push((Conn::And, self.parse_pipeline()?));
            } else if self.starts(b"||") {
                self.pos += 2;
                self.skip_linebreaks()?;
                rest.push((Conn::Or, self.parse_pipeline()?));
            } else {
                break;
            }
        }
        Ok(AndOr { first, rest })
    }

    fn parse_pipeline(&mut self) -> PResult<Pipeline> {
        self.skip_blanks();
        let mut neg = false;
        if self.is_reserved("!") {
            self.pos += 1;
            neg = true;
            self.skip_blanks();
        }
        let mut cmds = vec![self.parse_command()?];
        loop {
            self.skip_blanks();
            if self.peek() == Some(b'|') && self.peek_at(1) != Some(b'|') {
                self.pos += 1;
                if self.peek() == Some(b'&') {
                    // `|&` = `2>&1 |`
                    self.pos += 1;
                    if let Some(Cmd::Simple(sc)) = cmds.last_mut() {
                        sc.redirs.push(Redir { fd: 2, op: ROp::DupOut, target: Target::Word(vec![Part::Lit(b"1".to_vec())]) });
                    }
                }
                self.skip_linebreaks()?;
                cmds.push(self.parse_command()?);
            } else {
                break;
            }
        }
        Ok(Pipeline { neg, cmds })
    }

    fn parse_redirs(&mut self) -> PResult<Vec<Redir>> {
        let mut out = Vec::new();
        loop {
            self.skip_blanks();
            match self.try_redir()? {
                Some(r) => out.push(r),
                None => return Ok(out),
            }
        }
    }

    fn parse_command(&mut self) -> PResult<Cmd> {
        self.skip_blanks();
        match self.peek_reserved() {
            Some(b"{") => {
                self.pos += 1;
                let body = self.parse_list(&["}"], false)?;
                self.expect_reserved("}")?;
                let r = self.parse_redirs()?;
                return Ok(Cmd::Group(body, r));
            }
            Some(b"if") => {
                self.pos += 2;
                let mut branches = Vec::new();
                let cond = self.parse_list(&["then"], false)?;
                self.expect_reserved("then")?;
                let body = self.parse_list(&["elif", "else", "fi"], false)?;
                branches.push((cond, body));
                let mut els = None;
                loop {
                    self.skip_linebreaks()?;
                    if self.is_reserved("elif") {
                        self.pos += 4;
                        let c = self.parse_list(&["then"], false)?;
                        self.expect_reserved("then")?;
                        let b = self.parse_list(&["elif", "else", "fi"], false)?;
                        branches.push((c, b));
                    } else if self.is_reserved("else") {
                        self.pos += 4;
                        els = Some(self.parse_list(&["fi"], false)?);
                    } else if self.is_reserved("fi") {
                        self.pos += 2;
                        break;
                    } else {
                        return Err(self.unexpected());
                    }
                }
                let r = self.parse_redirs()?;
                return Ok(Cmd::If(branches, els, r));
            }
            Some(b"for") => {
                self.pos += 3;
                self.skip_blanks();
                let start = self.pos;
                while self.peek().is_some_and(is_name_char) {
                    self.pos += 1;
                }
                if start == self.pos {
                    return Err(self.unexpected());
                }
                let name = lossy(&self.s[start..self.pos]);
                self.skip_linebreaks()?;
                let mut items = None;
                if self.is_reserved("in") {
                    self.pos += 2;
                    let mut ws = Vec::new();
                    loop {
                        self.skip_blanks();
                        match self.peek() {
                            Some(b';') => {
                                self.pos += 1;
                                break;
                            }
                            Some(b'\n') => {
                                self.newline()?;
                                break;
                            }
                            None => break,
                            _ => ws.push(self.parse_word()?),
                        }
                    }
                    items = Some(ws);
                } else if self.peek() == Some(b';') {
                    self.pos += 1;
                }
                self.expect_reserved("do")?;
                let body = self.parse_list(&["done"], false)?;
                self.expect_reserved("done")?;
                let r = self.parse_redirs()?;
                return Ok(Cmd::For(name, items, body, r));
            }
            Some(b"while") | Some(b"until") => {
                let until = self.is_reserved("until");
                self.pos += 5;
                let cond = self.parse_list(&["do"], false)?;
                self.expect_reserved("do")?;
                let body = self.parse_list(&["done"], false)?;
                self.expect_reserved("done")?;
                let r = self.parse_redirs()?;
                return Ok(Cmd::While(cond, body, until, r));
            }
            _ => {}
        }
        if self.peek() == Some(b'(') {
            self.pos += 1;
            let body = self.parse_list(&[], true)?;
            self.skip_linebreaks()?;
            if self.peek() != Some(b')') {
                return Err(self.unexpected());
            }
            self.pos += 1;
            let r = self.parse_redirs()?;
            return Ok(Cmd::Sub(body, r));
        }
        self.parse_simple().map(Cmd::Simple)
    }

    fn parse_simple(&mut self) -> PResult<Simple> {
        let mut sc = Simple { line: self.line, ..Simple::default() };
        loop {
            self.skip_blanks();
            let Some(c) = self.peek() else { break };
            if c == b'\n' || c == b';' || c == b'|' || c == b')' || (c == b'&' && self.peek_at(1) != Some(b'>')) {
                break;
            }
            if c == b'(' {
                return Err(self.unexpected());
            }
            if let Some(r) = self.try_redir()? {
                sc.redirs.push(r);
                continue;
            }
            let w = self.parse_word()?;
            if sc.words.is_empty()
                && let Some(a) = as_assignment(&w)
            {
                sc.assigns.push(a);
                continue;
            }
            sc.words.push(w);
        }
        if sc.words.is_empty() && sc.assigns.is_empty() && sc.redirs.is_empty() {
            return Err(self.unexpected());
        }
        Ok(sc)
    }

    fn try_redir(&mut self) -> PResult<Option<Redir>> {
        let start = self.pos;
        let mut fd: Option<i32> = None;
        let mut i = self.pos;
        while i < self.s.len() && self.s[i].is_ascii_digit() {
            i += 1;
        }
        if i > self.pos && matches!(self.s.get(i), Some(b'<') | Some(b'>')) {
            fd = std::str::from_utf8(&self.s[self.pos..i]).ok().and_then(|x| x.parse().ok());
            self.pos = i;
        }
        let ops: &[(&[u8], ROp)] = &[
            (b"&>>", ROp::BothAppend),
            (b"&>", ROp::BothOut),
            (b"<<<", ROp::HereStr),
            (b"<<-", ROp::HereDoc),
            (b"<<", ROp::HereDoc),
            (b"<&", ROp::DupIn),
            (b"<>", ROp::ReadWrite),
            (b"<", ROp::In),
            (b">>", ROp::Append),
            (b">&", ROp::DupOut),
            (b">|", ROp::Out),
            (b">", ROp::Out),
        ];
        let mut found = None;
        for (t, op) in ops {
            if self.starts(t) {
                if fd.is_some() && t.starts_with(b"&") {
                    continue;
                }
                found = Some((t.len(), *op, *t == b"<<-"));
                break;
            }
        }
        let Some((len, op, strip)) = found else {
            self.pos = start;
            return Ok(None);
        };
        self.pos += len;
        self.skip_blanks();
        let default_fd = match op {
            ROp::In | ROp::DupIn | ROp::HereDoc | ROp::HereStr | ROp::ReadWrite => 0,
            _ => 1,
        };
        let fd = fd.unwrap_or(default_fd);
        if op == ROp::HereDoc {
            let raw_start = self.pos;
            let w = self.parse_word()?;
            if w.is_empty() {
                return Err(self.unexpected());
            }
            let quoted = self.s[raw_start..self.pos].iter().any(|c| matches!(c, b'\'' | b'"' | b'\\'));
            let delim = literal_of(&w);
            let id = self.bodies.len();
            self.bodies.push(None);
            self.pending.push((delim, strip, !quoted, id));
            return Ok(Some(Redir { fd, op, target: Target::Heredoc(id) }));
        }
        let w = self.parse_word()?;
        if w.is_empty() {
            return Err(self.unexpected());
        }
        Ok(Some(Redir { fd, op, target: Target::Word(w) }))
    }

    fn parse_word(&mut self) -> PResult<Word> {
        let mut parts = Vec::new();
        let mut lit = Vec::new();
        fn flush(lit: &mut Vec<u8>, parts: &mut Word) {
            if !lit.is_empty() {
                parts.push(Part::Lit(std::mem::take(lit)));
            }
        }
        while let Some(c) = self.peek() {
            if is_meta(c) {
                break;
            }
            match c {
                b'\\' => {
                    match self.peek_at(1) {
                        Some(b'\n') => {
                            self.pos += 2;
                            self.line += 1;
                        }
                        Some(n) => {
                            flush(&mut lit, &mut parts);
                            parts.push(Part::Quoted(vec![n]));
                            self.pos += 2;
                        }
                        None => {
                            lit.push(b'\\');
                            self.pos += 1;
                        }
                    }
                }
                b'\'' => {
                    flush(&mut lit, &mut parts);
                    self.pos += 1;
                    let start = self.pos;
                    while self.peek().is_some_and(|c| c != b'\'') {
                        if self.peek() == Some(b'\n') {
                            self.line += 1;
                        }
                        self.pos += 1;
                    }
                    if self.eof() {
                        return Err("unexpected EOF while looking for matching `''".into());
                    }
                    parts.push(Part::Quoted(self.s[start..self.pos].to_vec()));
                    self.pos += 1;
                }
                b'"' => {
                    flush(&mut lit, &mut parts);
                    self.pos += 1;
                    let inner = self.parse_dq_inner(false)?;
                    parts.push(Part::Dq(inner));
                }
                b'$' => {
                    flush(&mut lit, &mut parts);
                    parts.push(self.parse_dollar(false)?);
                }
                b'`' => {
                    flush(&mut lit, &mut parts);
                    parts.push(self.parse_backtick()?);
                }
                _ => {
                    lit.push(c);
                    self.pos += 1;
                }
            }
        }
        flush(&mut lit, &mut parts);
        Ok(parts)
    }

    /// Conteúdo de aspas duplas (`heredoc`: até o fim, sem `"` especial).
    fn parse_dq_inner(&mut self, heredoc: bool) -> PResult<Vec<Part>> {
        let mut parts = Vec::new();
        let mut lit = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                if heredoc {
                    break;
                }
                return Err("unexpected EOF while looking for matching `\"'".into());
            };
            match c {
                b'"' if !heredoc => {
                    self.pos += 1;
                    break;
                }
                b'\\' => {
                    let n = self.peek_at(1);
                    match n {
                        Some(b'$') | Some(b'`') | Some(b'\\') => {
                            lit.push(n.unwrap_or(b'\\'));
                            self.pos += 2;
                        }
                        Some(b'"') if !heredoc => {
                            lit.push(b'"');
                            self.pos += 2;
                        }
                        Some(b'\n') => {
                            self.pos += 2;
                            self.line += 1;
                        }
                        _ => {
                            lit.push(b'\\');
                            self.pos += 1;
                        }
                    }
                }
                b'$' => {
                    if !lit.is_empty() {
                        parts.push(Part::Lit(std::mem::take(&mut lit)));
                    }
                    parts.push(self.parse_dollar(true)?);
                }
                b'`' => {
                    if !lit.is_empty() {
                        parts.push(Part::Lit(std::mem::take(&mut lit)));
                    }
                    parts.push(self.parse_backtick()?);
                }
                b'\n' => {
                    lit.push(c);
                    self.pos += 1;
                    self.line += 1;
                }
                _ => {
                    lit.push(c);
                    self.pos += 1;
                }
            }
        }
        if !lit.is_empty() {
            parts.push(Part::Lit(lit));
        }
        Ok(parts)
    }

    fn parse_dollar(&mut self, in_dq: bool) -> PResult<Part> {
        match self.peek_at(1) {
            Some(b'(') if self.peek_at(2) == Some(b'(') => {
                self.pos += 3;
                let start = self.pos;
                let mut depth = 0i32;
                loop {
                    match self.peek() {
                        None => return Err("unexpected EOF while looking for matching `)'".into()),
                        Some(b'(') => depth += 1,
                        Some(b')') => {
                            if depth == 0 && self.peek_at(1) == Some(b')') {
                                break;
                            }
                            depth -= 1;
                        }
                        _ => {}
                    }
                    self.pos += 1;
                }
                let raw = self.s[start..self.pos].to_vec();
                self.pos += 2;
                Ok(Part::Arith(raw))
            }
            Some(b'(') => {
                self.pos += 2;
                let list = self.parse_list(&[], true)?;
                self.skip_linebreaks()?;
                if self.peek() != Some(b')') {
                    return Err("unexpected EOF while looking for matching `)'".into());
                }
                self.pos += 1;
                Ok(Part::Subst(list))
            }
            Some(b'{') => {
                self.pos += 2;
                let start = self.pos;
                match self.peek() {
                    Some(b'?' | b'#' | b'@' | b'*' | b'!' | b'$') => self.pos += 1,
                    _ => {
                        while self.peek().is_some_and(is_name_char) {
                            self.pos += 1;
                        }
                    }
                }
                let name = lossy(&self.s[start..self.pos]);
                if self.peek() == Some(b'}') {
                    self.pos += 1;
                    return Ok(Part::Var(name, None));
                }
                let mut op = String::new();
                if self.peek() == Some(b':') {
                    op.push(':');
                    self.pos += 1;
                }
                match self.peek() {
                    Some(c @ (b'-' | b'=' | b'+' | b'?')) => {
                        op.push(c as char);
                        self.pos += 1;
                    }
                    _ => return Err("bad substitution".into()),
                }
                // Palavra até o `}` correspondente.
                let wstart = self.pos;
                let mut depth = 0;
                let mut q: Option<u8> = None;
                loop {
                    let Some(c) = self.peek() else { return Err("unexpected EOF while looking for matching `}'".into()) };
                    match (q, c) {
                        (None, b'\'') | (None, b'"') => q = Some(c),
                        (Some(x), c2) if x == c2 => q = None,
                        (None, b'{') => depth += 1,
                        (None, b'}') => {
                            if depth == 0 {
                                break;
                            }
                            depth -= 1;
                        }
                        (_, b'\\') => self.pos += 1,
                        _ => {}
                    }
                    self.pos += 1;
                }
                let raw = self.s[wstart..self.pos].to_vec();
                self.pos += 1;
                let mut sub = Parser::new(&raw);
                let mut w = sub.parse_loose_word()?;
                let bodies = sub.bodies.clone();
                fill_word(&mut w, &bodies);
                Ok(Part::Var(name, Some((op, w))))
            }
            Some(b'\'') if !in_dq => {
                self.pos += 2;
                let mut v = Vec::new();
                loop {
                    let Some(c) = self.peek() else { return Err("unexpected EOF while looking for matching `''".into()) };
                    self.pos += 1;
                    match c {
                        b'\'' => break,
                        b'\\' => {
                            let n = self.peek().unwrap_or(b'\\');
                            self.pos += 1;
                            match n {
                                b'n' => v.push(b'\n'),
                                b't' => v.push(b'\t'),
                                b'r' => v.push(b'\r'),
                                b'e' | b'E' => v.push(0x1b),
                                b'a' => v.push(7),
                                b'0' => v.push(0),
                                b'\\' | b'\'' | b'"' => v.push(n),
                                _ => {
                                    v.push(b'\\');
                                    v.push(n);
                                }
                            }
                        }
                        _ => v.push(c),
                    }
                }
                Ok(Part::Quoted(v))
            }
            Some(c) if is_name_start(c) => {
                self.pos += 1;
                let start = self.pos;
                while self.peek().is_some_and(is_name_char) {
                    self.pos += 1;
                }
                Ok(Part::Var(lossy(&self.s[start..self.pos]), None))
            }
            Some(c) if c.is_ascii_digit() || matches!(c, b'?' | b'$' | b'#' | b'@' | b'*' | b'!' | b'-') => {
                self.pos += 2;
                Ok(Part::Var((c as char).to_string(), None))
            }
            _ => {
                self.pos += 1;
                Ok(Part::Lit(b"$".to_vec()))
            }
        }
    }

    fn parse_backtick(&mut self) -> PResult<Part> {
        self.pos += 1;
        let mut raw = Vec::new();
        loop {
            let Some(c) = self.peek() else { return Err("unexpected EOF while looking for matching ``'".into()) };
            self.pos += 1;
            match c {
                b'`' => break,
                b'\\' => {
                    let n = self.peek().unwrap_or(b'\\');
                    if matches!(n, b'`' | b'\\' | b'$') {
                        raw.push(n);
                        self.pos += 1;
                    } else {
                        raw.push(b'\\');
                    }
                }
                b'\n' => {
                    self.line += 1;
                    raw.push(c);
                }
                _ => raw.push(c),
            }
        }
        let mut sub = Parser::new(&raw);
        let list = sub.parse_program()?;
        Ok(Part::Subst(list))
    }

    /// Palavra sem parar em metacaractere (o lado direito de `${x:-...}`).
    fn parse_loose_word(&mut self) -> PResult<Word> {
        let mut parts = Vec::new();
        let mut lit = Vec::new();
        while let Some(c) = self.peek() {
            match c {
                b'\\' => {
                    if let Some(n) = self.peek_at(1) {
                        if !lit.is_empty() {
                            parts.push(Part::Lit(std::mem::take(&mut lit)));
                        }
                        parts.push(Part::Quoted(vec![n]));
                        self.pos += 2;
                    } else {
                        lit.push(c);
                        self.pos += 1;
                    }
                }
                b'\'' => {
                    if !lit.is_empty() {
                        parts.push(Part::Lit(std::mem::take(&mut lit)));
                    }
                    self.pos += 1;
                    let start = self.pos;
                    while self.peek().is_some_and(|c| c != b'\'') {
                        self.pos += 1;
                    }
                    parts.push(Part::Quoted(self.s[start..self.pos].to_vec()));
                    self.pos += 1;
                }
                b'"' => {
                    if !lit.is_empty() {
                        parts.push(Part::Lit(std::mem::take(&mut lit)));
                    }
                    self.pos += 1;
                    let inner = self.parse_dq_inner(false)?;
                    parts.push(Part::Dq(inner));
                }
                b'$' => {
                    if !lit.is_empty() {
                        parts.push(Part::Lit(std::mem::take(&mut lit)));
                    }
                    parts.push(self.parse_dollar(false)?);
                }
                _ => {
                    lit.push(c);
                    self.pos += 1;
                }
            }
        }
        if !lit.is_empty() {
            parts.push(Part::Lit(lit));
        }
        Ok(parts)
    }
}

/// Texto literal de uma palavra (aspas removidas, sem expansão), pro delimitador de heredoc.
fn literal_of(w: &Word) -> Vec<u8> {
    let mut out = Vec::new();
    for p in w {
        match p {
            Part::Lit(t) | Part::Quoted(t) => out.extend_from_slice(t),
            Part::Dq(inner) => out.extend(literal_of(inner)),
            Part::Var(n, _) => {
                out.push(b'$');
                out.extend_from_slice(n.as_bytes());
            }
            _ => {}
        }
    }
    out
}

fn as_assignment(w: &Word) -> Option<(String, Word)> {
    let Some(Part::Lit(first)) = w.first() else { return None };
    let eq = first.iter().position(|c| *c == b'=')?;
    let name = &first[..eq];
    if name.is_empty() || !is_name_start(name[0]) || !name.iter().all(|c| is_name_char(*c)) {
        return None;
    }
    let mut value: Word = Vec::new();
    if eq + 1 < first.len() {
        value.push(Part::Lit(first[eq + 1..].to_vec()));
    }
    value.extend(w[1..].iter().cloned());
    Some((lossy(name), value))
}

// Preenche os heredocs depois do parse.

fn fill_list(l: &mut List, b: &[Option<Word>]) {
    for ao in l {
        fill_pipe(&mut ao.first, b);
        for (_, p) in &mut ao.rest {
            fill_pipe(p, b);
        }
    }
}

fn fill_pipe(p: &mut Pipeline, b: &[Option<Word>]) {
    for c in &mut p.cmds {
        fill_cmd(c, b);
    }
}

fn fill_cmd(c: &mut Cmd, b: &[Option<Word>]) {
    match c {
        Cmd::Simple(s) => {
            for (_, w) in &mut s.assigns {
                fill_word(w, b);
            }
            for w in &mut s.words {
                fill_word(w, b);
            }
            fill_redirs(&mut s.redirs, b);
        }
        Cmd::Group(l, r) | Cmd::Sub(l, r) => {
            fill_list(l, b);
            fill_redirs(r, b);
        }
        Cmd::For(_, items, body, r) => {
            if let Some(ws) = items {
                for w in ws {
                    fill_word(w, b);
                }
            }
            fill_list(body, b);
            fill_redirs(r, b);
        }
        Cmd::If(branches, els, r) => {
            for (c, d) in branches {
                fill_list(c, b);
                fill_list(d, b);
            }
            if let Some(e) = els {
                fill_list(e, b);
            }
            fill_redirs(r, b);
        }
        Cmd::While(c, d, _, r) => {
            fill_list(c, b);
            fill_list(d, b);
            fill_redirs(r, b);
        }
    }
}

fn fill_redirs(rs: &mut [Redir], b: &[Option<Word>]) {
    for r in rs {
        match &mut r.target {
            Target::Heredoc(id) => {
                let id = *id;
                r.target = Target::Body(b.get(id).cloned().flatten().unwrap_or_default());
            }
            Target::Word(w) => fill_word(w, b),
            Target::Body(_) => {}
        }
    }
}

fn fill_word(w: &mut Word, b: &[Option<Word>]) {
    for p in w {
        match p {
            Part::Dq(ps) => fill_word(ps, b),
            Part::Subst(l) => fill_list(l, b),
            Part::Var(_, Some((_, w2))) => fill_word(w2, b),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Interpretador
// ---------------------------------------------------------------------------------------------

#[derive(Debug)]
enum Flow {
    Exit(i32),
}

type X = Result<i32, Flow>;

#[derive(Clone, Debug)]
struct Shell {
    vars: BTreeMap<String, Vec<u8>>,
    exported: BTreeSet<String>,
    status: i32,
    errexit: bool,
    pipefail: bool,
    in_cond: usize,
    line: usize,
    argv0: Vec<u8>,
    args: Vec<Vec<u8>>,
}

const BUILTINS: &[&str] = &["cd", "pwd", "export", "unset", "exit", "set", "true", "false", ":", "echo", "printf", "test", "[", "read", "command", "wait", "shift", "local"];

impl Shell {
    fn new(argv0: Vec<u8>, args: Vec<Vec<u8>>) -> Shell {
        let s = sysc();
        let mut sh = Shell {
            vars: BTreeMap::new(),
            exported: BTreeSet::new(),
            status: 0,
            errexit: false,
            pipefail: false,
            in_cond: 0,
            line: 1,
            argv0,
            args,
        };
        for kv in s.environ() {
            if let Some(eq) = kv.iter().position(|c| *c == b'=') {
                let k = lossy(&kv[..eq]);
                sh.vars.insert(k.clone(), kv[eq + 1..].to_vec());
                sh.exported.insert(k);
            }
        }
        if let Ok(cwd) = s.getcwd() {
            sh.vars.insert("PWD".into(), cwd);
            sh.exported.insert("PWD".into());
        }
        sh
    }

    fn set_var(&mut self, name: &str, value: Vec<u8>) {
        self.vars.insert(name.to_string(), value);
    }

    fn env_list(&self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for k in &self.exported {
            if let Some(v) = self.vars.get(k) {
                let mut kv = k.as_bytes().to_vec();
                kv.push(b'=');
                kv.extend_from_slice(v);
                out.push(kv);
            }
        }
        out
    }

    fn error(&self, msg: &str) {
        let name = lossy(&self.argv0);
        errs(&format!("{name}: line {}: {msg}\n", self.line));
    }

    fn run_list(&mut self, list: &List) -> X {
        let mut st = 0;
        for ao in list {
            st = self.run_and_or(ao)?;
        }
        Ok(st)
    }

    fn run_and_or(&mut self, ao: &AndOr) -> X {
        let mut st = self.run_pipeline(&ao.first)?;
        let mut last = 0;
        let mut last_neg = ao.first.neg;
        for (i, (conn, p)) in ao.rest.iter().enumerate() {
            let go = match conn {
                Conn::And => st == 0,
                Conn::Or => st != 0,
            };
            if go {
                st = self.run_pipeline(p)?;
                last = i + 1;
                last_neg = p.neg;
            }
        }
        self.status = st;
        if self.errexit && self.in_cond == 0 && st != 0 && last == ao.rest.len() && !last_neg {
            return Err(Flow::Exit(st));
        }
        Ok(st)
    }

    fn run_pipeline(&mut self, p: &Pipeline) -> X {
        let st = if p.cmds.len() == 1 { self.run_cmd(&p.cmds[0])? } else { self.run_multi(&p.cmds)? };
        let st = if p.neg { (st == 0) as i32 } else { st };
        self.status = st;
        Ok(st)
    }

    fn run_multi(&mut self, cmds: &[Cmd]) -> X {
        let s = sysc();
        let n = cmds.len();
        let mut prev: Option<Fd> = None;
        let mut pids = Vec::new();
        for (i, c) in cmds.iter().enumerate() {
            let (r, w) = if i + 1 < n {
                match s.pipe2(OFlags::CLOEXEC) {
                    Ok((r, w)) => (Some(r), Some(w)),
                    Err(e) => {
                        self.error(&format!("pipe error: {}", e.message()));
                        return Ok(1);
                    }
                }
            } else {
                (None, None)
            };
            let mut acts = Vec::new();
            if let Some(p) = prev {
                acts.push(FdAction::Dup2 { from: p, to: Fd::STDIN });
                acts.push(FdAction::Close(p));
            }
            if let Some(w) = w {
                acts.push(FdAction::Dup2 { from: w, to: Fd::STDOUT });
                acts.push(FdAction::Close(w));
            }
            if let Some(r) = r {
                acts.push(FdAction::Close(r));
            }
            let sh = self.clone();
            let cmd = c.clone();
            let attrs = ProcAttrs { fd_actions: acts, ..ProcAttrs::default() };
            let pid = s.spawn_fn(
                attrs,
                b"bash".to_vec(),
                Box::new(move || {
                    let mut sh = sh;
                    match sh.run_cmd(&cmd) {
                        Ok(st) => st,
                        Err(Flow::Exit(c)) => c,
                    }
                }),
            );
            if let Some(p) = prev {
                let _ = s.close(p);
            }
            if let Some(w) = w {
                let _ = s.close(w);
            }
            prev = r;
            pids.push(pid.ok());
        }
        let mut statuses = Vec::new();
        for pid in pids {
            let st = match pid {
                Some(p) => wait_pid(&*s, p),
                None => 1,
            };
            statuses.push(st);
        }
        let last = *statuses.last().unwrap_or(&0);
        if self.pipefail {
            return Ok(statuses.iter().rev().find(|x| **x != 0).copied().unwrap_or(0));
        }
        Ok(last)
    }

    fn run_cmd(&mut self, cmd: &Cmd) -> X {
        match cmd {
            Cmd::Simple(sc) => self.run_simple(sc),
            Cmd::Group(list, r) => {
                let list = list.clone();
                self.with_redirs(r, move |sh| sh.run_list(&list))
            }
            Cmd::Sub(list, r) => {
                let s = sysc();
                let (acts, close) = match self.redir_actions(r)? {
                    Ok(x) => x,
                    Err(st) => return Ok(st),
                };
                let sh = self.clone();
                let list = list.clone();
                let pid = s.spawn_fn(
                    ProcAttrs { fd_actions: acts, ..ProcAttrs::default() },
                    b"bash".to_vec(),
                    Box::new(move || {
                        let mut sh = sh;
                        match sh.run_list(&list) {
                            Ok(st) => st,
                            Err(Flow::Exit(c)) => c,
                        }
                    }),
                );
                for fd in close {
                    let _ = s.close(fd);
                }
                let st = match pid {
                    Ok(p) => wait_pid(&*s, p),
                    Err(_) => 1,
                };
                self.status = st;
                Ok(st)
            }
            Cmd::For(name, items, body, r) => {
                let name = name.clone();
                let items = items.clone();
                let body = body.clone();
                self.with_redirs(r, move |sh| {
                    let values = match &items {
                        Some(ws) => sh.expand_words(ws)?,
                        None => sh.args.clone(),
                    };
                    let mut st = 0;
                    for v in values {
                        sh.set_var(&name, v);
                        st = sh.run_list(&body)?;
                    }
                    Ok(st)
                })
            }
            Cmd::If(branches, els, r) => {
                let branches = branches.clone();
                let els = els.clone();
                self.with_redirs(r, move |sh| {
                    for (c, b) in &branches {
                        sh.in_cond += 1;
                        let res = sh.run_list(c);
                        sh.in_cond -= 1;
                        if res? == 0 {
                            return sh.run_list(b);
                        }
                    }
                    match &els {
                        Some(e) => sh.run_list(e),
                        None => Ok(0),
                    }
                })
            }
            Cmd::While(cond, body, until, r) => {
                let cond = cond.clone();
                let body = body.clone();
                let until = *until;
                self.with_redirs(r, move |sh| {
                    let mut st = 0;
                    loop {
                        sh.in_cond += 1;
                        let res = sh.run_list(&cond);
                        sh.in_cond -= 1;
                        let c = res?;
                        if (c == 0) == until {
                            break;
                        }
                        st = sh.run_list(&body)?;
                    }
                    Ok(st)
                })
            }
        }
    }

    fn run_simple(&mut self, sc: &Simple) -> X {
        self.line = sc.line;
        let before = self.status;
        let words = self.expand_words(&sc.words)?;
        let mut assigns = Vec::new();
        for (n, w) in &sc.assigns {
            let v = self.expand_str(w)?;
            assigns.push((n.clone(), v));
        }
        if words.is_empty() {
            let subst_status = if self.status != before { self.status } else { 0 };
            for (n, v) in assigns {
                self.set_var(&n, v);
            }
            return self.with_redirs(&sc.redirs, move |_| Ok(subst_status));
        }
        let name = lossy(&words[0]);
        if BUILTINS.contains(&name.as_str()) {
            let words2 = words.clone();
            return self.with_redirs(&sc.redirs, move |sh| sh.builtin(&words2));
        }
        let Some(path) = self.find_cmd(&words[0]) else {
            self.error(&format!("{name}: command not found"));
            self.status = 127;
            return Ok(127);
        };
        let (acts, close) = match self.redir_actions(&sc.redirs)? {
            Ok(x) => x,
            Err(st) => return Ok(st),
        };
        let mut env = self.env_list();
        for (n, v) in assigns {
            let prefix = format!("{n}=");
            env.retain(|kv| !kv.starts_with(prefix.as_bytes()));
            let mut kv = prefix.into_bytes();
            kv.extend_from_slice(&v);
            env.push(kv);
        }
        let s = sysc();
        let spec = SpawnSpec { path, argv: words.clone(), attrs: ProcAttrs { env: Some(env), fd_actions: acts, ..ProcAttrs::default() } };
        let r = s.spawn(spec);
        for fd in close {
            let _ = s.close(fd);
        }
        let st = match r {
            Ok(pid) => wait_pid(&*s, pid),
            Err(Errno::ENOENT) => {
                self.error(&format!("{name}: No such file or directory"));
                127
            }
            Err(e) => {
                self.error(&format!("{name}: {}", e.message()));
                126
            }
        };
        self.status = st;
        Ok(st)
    }

    fn find_cmd(&self, name: &[u8]) -> Option<Vec<u8>> {
        if name.contains(&b'/') {
            return Some(name.to_vec());
        }
        if name.is_empty() {
            return None;
        }
        let path = self.vars.get("PATH").cloned().unwrap_or_else(|| b"/usr/bin:/bin".to_vec());
        let s = sysc();
        for dir in path.split(|c| *c == b':') {
            let cand = join_path(if dir.is_empty() { b"." } else { dir }, name);
            if let Ok(st) = s.fstatat(Fd::CWD, &cand, AtFlags::empty())
                && st.file_type() == FileType::Regular
                && s.faccessat(Fd::CWD, &cand, AccessMode::X_OK, AtFlags::empty()).is_ok()
            {
                return Some(cand);
            }
        }
        None
    }

    // ---- redirecionamentos ----

    /// Abre os alvos no processo do shell e devolve as ações de fd pro filho e os fds a fechar
    /// depois do spawn. `Err(status)` se um alvo não abriu (mensagem já impressa).
    #[allow(clippy::type_complexity)]
    fn redir_actions(&mut self, redirs: &[Redir]) -> Result<Result<(Vec<FdAction>, Vec<Fd>), i32>, Flow> {
        let s = sysc();
        let mut acts = Vec::new();
        let mut close = Vec::new();
        for r in redirs {
            match self.open_redir(r)? {
                Opened::Fd(nf) => {
                    acts.push(FdAction::Dup2 { from: nf, to: Fd(r.fd) });
                    if r.op == ROp::BothOut || r.op == ROp::BothAppend {
                        acts.push(FdAction::Dup2 { from: nf, to: Fd::STDERR });
                    }
                    acts.push(FdAction::Close(nf));
                    close.push(nf);
                }
                Opened::Dup(src) => acts.push(FdAction::Dup2 { from: Fd(src), to: Fd(r.fd) }),
                Opened::Close => acts.push(FdAction::Close(Fd(r.fd))),
                Opened::Failed(st) => {
                    for fd in close {
                        let _ = s.close(fd);
                    }
                    return Ok(Err(st));
                }
            }
        }
        Ok(Ok((acts, close)))
    }

    fn open_redir(&mut self, r: &Redir) -> Result<Opened, Flow> {
        let s = sysc();
        match (&r.target, r.op) {
            (Target::Body(w), ROp::HereDoc) => {
                let body = self.expand_str(w)?;
                Ok(self.pipe_with(&body))
            }
            (Target::Word(w), ROp::HereStr) => {
                let mut body = self.expand_str(w)?;
                body.push(b'\n');
                Ok(self.pipe_with(&body))
            }
            (Target::Word(w), ROp::DupOut | ROp::DupIn) => {
                let t = self.expand_str(w)?;
                if t == b"-" {
                    return Ok(Opened::Close);
                }
                match std::str::from_utf8(&t).ok().and_then(|x| x.parse::<i32>().ok()) {
                    Some(n) => Ok(Opened::Dup(n)),
                    None => {
                        if r.op == ROp::DupOut {
                            // `>&arquivo` = `&>arquivo`.
                            let flags = OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC;
                            return Ok(match s.openat(Fd::CWD, &t, flags, 0o666) {
                                Ok(fd) => Opened::Fd(fd),
                                Err(e) => {
                                    self.error(&format!("{}: {}", lossy(&t), e.message()));
                                    Opened::Failed(1)
                                }
                            });
                        }
                        self.error(&format!("{}: ambiguous redirect", lossy(&t)));
                        Ok(Opened::Failed(1))
                    }
                }
            }
            (Target::Word(w), op) => {
                let t = self.expand_str(w)?;
                let flags = match op {
                    ROp::In => OFlags::RDONLY,
                    ROp::ReadWrite => OFlags::RDWR | OFlags::CREAT,
                    ROp::Append | ROp::BothAppend => OFlags::WRONLY | OFlags::CREAT | OFlags::APPEND,
                    _ => OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC,
                };
                match s.openat(Fd::CWD, &t, flags | OFlags::CLOEXEC, 0o666) {
                    Ok(fd) => Ok(Opened::Fd(fd)),
                    Err(e) => {
                        self.error(&format!("{}: {}", lossy(&t), e.message()));
                        Ok(Opened::Failed(1))
                    }
                }
            }
            _ => Ok(Opened::Failed(1)),
        }
    }

    fn pipe_with(&self, body: &[u8]) -> Opened {
        let s = sysc();
        match s.pipe2(OFlags::CLOEXEC) {
            Ok((r, w)) => {
                let _ = sys::write_all(w, body);
                let _ = s.close(w);
                Opened::Fd(r)
            }
            Err(_) => Opened::Failed(1),
        }
    }

    /// Roda `f` com os redirecionamentos aplicados no próprio processo (salvando e restaurando).
    fn with_redirs(&mut self, redirs: &[Redir], f: impl FnOnce(&mut Shell) -> X) -> X {
        if redirs.is_empty() {
            return f(self);
        }
        let s = sysc();
        let mut saved: Vec<(i32, Option<Fd>)> = Vec::new();
        let save = |fd: i32, saved: &mut Vec<(i32, Option<Fd>)>| {
            if saved.iter().all(|(f, _)| *f != fd) {
                saved.push((fd, s.dup_min(Fd(fd), Fd(10), true).ok()));
            }
        };
        let mut failed = None;
        for r in redirs {
            let opened = self.open_redir(r)?;
            match opened {
                Opened::Fd(nf) => {
                    save(r.fd, &mut saved);
                    let _ = s.dup3(nf, Fd(r.fd), false);
                    if r.op == ROp::BothOut || r.op == ROp::BothAppend {
                        save(2, &mut saved);
                        let _ = s.dup3(nf, Fd::STDERR, false);
                    }
                    let _ = s.close(nf);
                }
                Opened::Dup(src) => {
                    save(r.fd, &mut saved);
                    if src != r.fd {
                        let _ = s.dup3(Fd(src), Fd(r.fd), false);
                    }
                }
                Opened::Close => {
                    save(r.fd, &mut saved);
                    let _ = s.close(Fd(r.fd));
                }
                Opened::Failed(st) => {
                    failed = Some(st);
                    break;
                }
            }
        }
        let res = match failed {
            Some(st) => Ok(st),
            None => f(self),
        };
        for (fd, sv) in saved.into_iter().rev() {
            match sv {
                Some(sfd) => {
                    let _ = s.dup3(sfd, Fd(fd), false);
                    let _ = s.close(sfd);
                }
                None => {
                    let _ = s.close(Fd(fd));
                }
            }
        }
        if let Ok(st) = &res {
            self.status = *st;
        }
        res
    }

    // ---- expansão ----

    fn expand_words(&mut self, words: &[Word]) -> Result<Vec<Vec<u8>>, Flow> {
        let mut out = Vec::new();
        for w in words {
            out.extend(self.expand_fields(w)?);
        }
        Ok(out)
    }

    fn tilde(&self, t: &[u8]) -> Vec<u8> {
        if t == b"~" || t.starts_with(b"~/") {
            let mut v = self.vars.get("HOME").cloned().unwrap_or_default();
            v.extend_from_slice(&t[1..]);
            return v;
        }
        t.to_vec()
    }

    fn expand_fields(&mut self, w: &Word) -> Result<Vec<Vec<u8>>, Flow> {
        let mut fields: Vec<Vec<(u8, bool)>> = Vec::new();
        let mut cur: Option<Vec<(u8, bool)>> = None;
        for (i, part) in w.iter().enumerate() {
            match part {
                Part::Lit(t) => {
                    let t = if i == 0 { self.tilde(t) } else { t.clone() };
                    cur.get_or_insert_with(Vec::new).extend(t.iter().map(|&b| (b, true)));
                }
                Part::Quoted(t) => cur.get_or_insert_with(Vec::new).extend(t.iter().map(|&b| (b, false))),
                Part::Dq(parts) => {
                    if parts.len() == 1
                        && let Part::Var(n, None) = &parts[0]
                        && n == "@"
                    {
                        // "$@": um campo por argumento.
                        let args = self.args.clone();
                        for (k, a) in args.iter().enumerate() {
                            let c = cur.get_or_insert_with(Vec::new);
                            c.extend(a.iter().map(|&b| (b, false)));
                            if k + 1 < args.len() {
                                fields.push(cur.take().unwrap_or_default());
                            }
                        }
                        continue;
                    }
                    let v = self.expand_dq(parts)?;
                    cur.get_or_insert_with(Vec::new).extend(v.into_iter().map(|b| (b, false)));
                }
                Part::Var(..) | Part::Subst(..) | Part::Arith(..) => {
                    let v = self.part_value(part)?;
                    for &b in &v {
                        if b == b' ' || b == b'\t' || b == b'\n' {
                            if let Some(c) = cur.take() {
                                fields.push(c);
                            }
                        } else {
                            cur.get_or_insert_with(Vec::new).push((b, true));
                        }
                    }
                }
            }
        }
        if let Some(c) = cur {
            fields.push(c);
        }
        let mut outv = Vec::new();
        for f in fields {
            if f.iter().any(|(b, active)| *active && matches!(b, b'*' | b'?' | b'[')) {
                let m = glob(&f);
                if !m.is_empty() {
                    outv.extend(m);
                    continue;
                }
            }
            outv.push(f.into_iter().map(|(b, _)| b).collect());
        }
        Ok(outv)
    }

    fn expand_dq(&mut self, parts: &[Part]) -> Result<Vec<u8>, Flow> {
        let mut out = Vec::new();
        for p in parts {
            match p {
                Part::Lit(t) | Part::Quoted(t) => out.extend_from_slice(t),
                Part::Dq(inner) => out.extend(self.expand_dq(inner)?),
                _ => out.extend(self.part_value(p)?),
            }
        }
        Ok(out)
    }

    /// Expansão sem divisão nem glob (atribuições, alvos de redirecionamento, heredocs).
    fn expand_str(&mut self, w: &Word) -> Result<Vec<u8>, Flow> {
        let mut out = Vec::new();
        for (i, p) in w.iter().enumerate() {
            match p {
                Part::Lit(t) => out.extend(if i == 0 { self.tilde(t) } else { t.clone() }),
                Part::Quoted(t) => out.extend_from_slice(t),
                Part::Dq(inner) => out.extend(self.expand_dq(inner)?),
                _ => out.extend(self.part_value(p)?),
            }
        }
        Ok(out)
    }

    fn part_value(&mut self, p: &Part) -> Result<Vec<u8>, Flow> {
        match p {
            Part::Var(name, op) => self.var_value(name, op.as_ref()),
            Part::Subst(list) => self.run_subst(list),
            Part::Arith(raw) => {
                let text = {
                    let mut sub = Parser::new(raw);
                    let w = sub.parse_loose_word().unwrap_or_default();
                    self.expand_str(&w)?
                };
                let v = arith(&lossy(&text), &self.vars);
                Ok(v.to_string().into_bytes())
            }
            _ => Ok(Vec::new()),
        }
    }

    fn var_value(&mut self, name: &str, op: Option<&(String, Word)>) -> Result<Vec<u8>, Flow> {
        let val: Option<Vec<u8>> = match name {
            "?" => Some(self.status.to_string().into_bytes()),
            "$" => Some(sysc().getpid().to_string().into_bytes()),
            "#" => Some(self.args.len().to_string().into_bytes()),
            "0" => Some(self.argv0.clone()),
            "@" | "*" => Some(self.args.join(&b' ')),
            "!" => Some(Vec::new()),
            "-" => Some(if self.errexit { b"e".to_vec() } else { Vec::new() }),
            n if n.bytes().all(|c| c.is_ascii_digit()) => {
                let k: usize = n.parse().unwrap_or(0);
                self.args.get(k.wrapping_sub(1)).cloned()
            }
            n => self.vars.get(n).cloned(),
        };
        let Some((op, w)) = op else { return Ok(val.unwrap_or_default()) };
        let colon = op.starts_with(':');
        let unset = val.is_none() || (colon && val.as_ref().is_some_and(|v| v.is_empty()));
        match op.trim_start_matches(':') {
            "-" => {
                if unset {
                    self.expand_str(w)
                } else {
                    Ok(val.unwrap_or_default())
                }
            }
            "=" => {
                if unset {
                    let v = self.expand_str(w)?;
                    self.set_var(name, v.clone());
                    Ok(v)
                } else {
                    Ok(val.unwrap_or_default())
                }
            }
            "+" => {
                if unset {
                    Ok(Vec::new())
                } else {
                    self.expand_str(w)
                }
            }
            "?" => {
                if unset {
                    let m = self.expand_str(w)?;
                    let m = if m.is_empty() { b"parameter null or not set".to_vec() } else { m };
                    self.error(&format!("{name}: {}", lossy(&m)));
                    return Err(Flow::Exit(1));
                }
                Ok(val.unwrap_or_default())
            }
            _ => Ok(val.unwrap_or_default()),
        }
    }

    fn run_subst(&mut self, list: &List) -> Result<Vec<u8>, Flow> {
        let s = sysc();
        let Ok((r, w)) = s.pipe2(OFlags::CLOEXEC) else { return Ok(Vec::new()) };
        let sh = self.clone();
        let list = list.clone();
        let attrs = ProcAttrs { fd_actions: vec![FdAction::Dup2 { from: w, to: Fd::STDOUT }, FdAction::Close(w), FdAction::Close(r)], ..ProcAttrs::default() };
        let pid = s.spawn_fn(
            attrs,
            b"bash".to_vec(),
            Box::new(move || {
                let mut sh = sh;
                match sh.run_list(&list) {
                    Ok(st) => st,
                    Err(Flow::Exit(c)) => c,
                }
            }),
        );
        let _ = s.close(w);
        let mut data = read_all_fd(r);
        let _ = s.close(r);
        let st = match pid {
            Ok(p) => wait_pid(&*s, p),
            Err(_) => 1,
        };
        self.status = st;
        while data.last() == Some(&b'\n') {
            data.pop();
        }
        Ok(data)
    }

    // ---- builtins ----

    fn builtin(&mut self, words: &[Vec<u8>]) -> X {
        let name = lossy(&words[0]);
        let args = &words[1..];
        let st = match name.as_str() {
            ":" | "true" | "wait" | "local" => 0,
            "false" => 1,
            "echo" => {
                out(&echo_impl(args));
                0
            }
            "printf" => printf_impl(args, "printf"),
            "test" => test_impl(args, false, "test"),
            "[" => test_impl(args, true, "["),
            "pwd" => {
                let cwd = sysc().getcwd().unwrap_or_default();
                out(&cwd);
                out(b"\n");
                0
            }
            "cd" => {
                let target = match args.first() {
                    None => self.vars.get("HOME").cloned().unwrap_or_else(|| b"/".to_vec()),
                    Some(a) if a == b"-" => {
                        let o = self.vars.get("OLDPWD").cloned().unwrap_or_default();
                        out(&o);
                        out(b"\n");
                        o
                    }
                    Some(a) => a.clone(),
                };
                let s = sysc();
                match s.chdir(&target) {
                    Ok(()) => {
                        let old = self.vars.get("PWD").cloned().unwrap_or_default();
                        self.set_var("OLDPWD", old);
                        let cwd = s.getcwd().unwrap_or_default();
                        self.set_var("PWD", cwd);
                        0
                    }
                    Err(e) => {
                        self.error(&format!("cd: {}: {}", lossy(&target), e.message()));
                        1
                    }
                }
            }
            "export" => {
                for a in args {
                    if a.starts_with(b"-") {
                        continue;
                    }
                    match a.iter().position(|c| *c == b'=') {
                        Some(eq) => {
                            let k = lossy(&a[..eq]);
                            self.set_var(&k, a[eq + 1..].to_vec());
                            self.exported.insert(k);
                        }
                        None => {
                            self.exported.insert(lossy(a));
                        }
                    }
                }
                0
            }
            "unset" => {
                for a in args {
                    if a.starts_with(b"-") {
                        continue;
                    }
                    let k = lossy(a);
                    self.vars.remove(&k);
                    self.exported.remove(&k);
                }
                0
            }
            "exit" => {
                let code = match args.first() {
                    Some(a) => lossy(a).parse::<i32>().map(|c| c & 0xff).unwrap_or(2),
                    None => self.status,
                };
                return Err(Flow::Exit(code));
            }
            "set" => {
                let mut i = 0;
                while i < args.len() {
                    let a = lossy(&args[i]);
                    if a == "-o" || a == "+o" {
                        if args.get(i + 1).map(|x| x.as_slice()) == Some(b"pipefail") {
                            self.pipefail = a == "-o";
                        }
                        if args.get(i + 1).map(|x| x.as_slice()) == Some(b"errexit") {
                            self.errexit = a == "-o";
                        }
                        i += 2;
                        continue;
                    }
                    if let Some(f) = a.strip_prefix('-') {
                        if f.contains('e') {
                            self.errexit = true;
                        }
                        if f.contains('o') && args.get(i + 1).map(|x| x.as_slice()) == Some(b"pipefail") {
                            self.pipefail = true;
                            i += 1;
                        }
                    } else if let Some(f) = a.strip_prefix('+')
                        && f.contains('e')
                    {
                        self.errexit = false;
                    }
                    i += 1;
                }
                0
            }
            "shift" => {
                let n: usize = args.first().and_then(|a| lossy(a).parse().ok()).unwrap_or(1);
                if n <= self.args.len() {
                    self.args.drain(..n);
                    0
                } else {
                    1
                }
            }
            "read" => {
                let mut names: Vec<String> = args.iter().filter(|a| !a.starts_with(b"-")).map(|a| lossy(a)).collect();
                if names.is_empty() {
                    names.push("REPLY".into());
                }
                let s = sysc();
                let mut line = Vec::new();
                let mut got_any = false;
                let mut nl = false;
                let mut b = [0u8; 1];
                while let Ok(1) = s.read(Fd::STDIN, &mut b) {
                    got_any = true;
                    if b[0] == b'\n' {
                        nl = true;
                        break;
                    }
                    line.push(b[0]);
                }
                let text = lossy(&line);
                let fields: Vec<&str> = text.split_whitespace().collect();
                for (k, n) in names.iter().enumerate() {
                    let v = if k + 1 == names.len() {
                        fields.get(k..).map(|r| r.join(" ")).unwrap_or_default()
                    } else {
                        fields.get(k).copied().unwrap_or("").to_string()
                    };
                    self.set_var(n, v.into_bytes());
                }
                let _ = got_any;
                if nl { 0 } else { 1 }
            }
            "command" => {
                if args.first().map(|a| a.as_slice()) == Some(b"-v") {
                    let mut st = 1;
                    for a in &args[1..] {
                        if BUILTINS.contains(&lossy(a).as_str()) {
                            out(a);
                            out(b"\n");
                            st = 0;
                        } else if let Some(p) = self.find_cmd(a) {
                            out(&p);
                            out(b"\n");
                            st = 0;
                        }
                    }
                    st
                } else if !args.is_empty() {
                    let sc = Simple { words: args.iter().map(|a| vec![Part::Quoted(a.clone())]).collect(), line: self.line, ..Simple::default() };
                    return self.run_simple(&sc);
                } else {
                    0
                }
            }
            _ => 0,
        };
        self.status = st;
        Ok(st)
    }
}

enum Opened {
    Fd(Fd),
    Dup(i32),
    Close,
    Failed(i32),
}

fn wait_pid(s: &dyn Syscalls, pid: i32) -> i32 {
    loop {
        match s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()) {
            Ok(Some((_, st))) => return st.shell_status(),
            Ok(None) => continue,
            Err(Errno::EINTR) => continue,
            Err(_) => return 1,
        }
    }
}

/// Aritmética inteira simples: `+ - * / %`, parênteses, comparações e nomes de variável.
fn arith(expr: &str, vars: &BTreeMap<String, Vec<u8>>) -> i64 {
    struct P<'a> {
        s: &'a [u8],
        i: usize,
        vars: &'a BTreeMap<String, Vec<u8>>,
    }
    impl P<'_> {
        fn ws(&mut self) {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
        }
        fn cmp(&mut self) -> i64 {
            let a = self.add();
            self.ws();
            for (op, f) in [("==", 0), ("!=", 1), ("<=", 2), (">=", 3), ("<", 4), (">", 5)] {
                if self.s[self.i..].starts_with(op.as_bytes()) {
                    self.i += op.len();
                    let b = self.add();
                    return match f {
                        0 => (a == b) as i64,
                        1 => (a != b) as i64,
                        2 => (a <= b) as i64,
                        3 => (a >= b) as i64,
                        4 => (a < b) as i64,
                        _ => (a > b) as i64,
                    };
                }
            }
            a
        }
        fn add(&mut self) -> i64 {
            let mut v = self.mul();
            loop {
                self.ws();
                match self.s.get(self.i) {
                    Some(b'+') => {
                        self.i += 1;
                        v = v.wrapping_add(self.mul());
                    }
                    Some(b'-') => {
                        self.i += 1;
                        v = v.wrapping_sub(self.mul());
                    }
                    _ => return v,
                }
            }
        }
        fn mul(&mut self) -> i64 {
            let mut v = self.atom();
            loop {
                self.ws();
                match self.s.get(self.i) {
                    Some(b'*') => {
                        self.i += 1;
                        v = v.wrapping_mul(self.atom());
                    }
                    Some(b'/') => {
                        self.i += 1;
                        let d = self.atom();
                        v = if d == 0 { 0 } else { v / d };
                    }
                    Some(b'%') => {
                        self.i += 1;
                        let d = self.atom();
                        v = if d == 0 { 0 } else { v % d };
                    }
                    _ => return v,
                }
            }
        }
        fn atom(&mut self) -> i64 {
            self.ws();
            match self.s.get(self.i) {
                Some(b'(') => {
                    self.i += 1;
                    let v = self.cmp();
                    self.ws();
                    if self.s.get(self.i) == Some(&b')') {
                        self.i += 1;
                    }
                    v
                }
                Some(b'-') => {
                    self.i += 1;
                    -self.atom()
                }
                Some(b'+') => {
                    self.i += 1;
                    self.atom()
                }
                Some(c) if c.is_ascii_digit() => {
                    let st = self.i;
                    while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                        self.i += 1;
                    }
                    lossy(&self.s[st..self.i]).parse().unwrap_or(0)
                }
                Some(c) if is_name_start(*c) => {
                    let st = self.i;
                    while self.i < self.s.len() && is_name_char(self.s[self.i]) {
                        self.i += 1;
                    }
                    let name = lossy(&self.s[st..self.i]);
                    self.vars.get(&name).and_then(|v| lossy(v).trim().parse().ok()).unwrap_or(0)
                }
                _ => 0,
            }
        }
    }
    let mut p = P { s: expr.as_bytes(), i: 0, vars };
    p.cmp()
}

// ---- glob ----

/// `fnmatch` com o padrão marcado (bytes ativos podem ser curinga; os citados são literais).
fn fnmatch(p: &[(u8, bool)], s: &[u8]) -> bool {
    fn go(p: &[(u8, bool)], s: &[u8]) -> bool {
        let Some(&(c, active)) = p.first() else { return s.is_empty() };
        if active {
            match c {
                b'*' => {
                    for k in 0..=s.len() {
                        if go(&p[1..], &s[k..]) {
                            return true;
                        }
                    }
                    return false;
                }
                b'?' => return !s.is_empty() && go(&p[1..], &s[1..]),
                b'[' => {
                    let Some(&ch) = s.first() else { return false };
                    let mut i = 1;
                    let mut neg = false;
                    if i < p.len() && matches!(p[i].0, b'!' | b'^') {
                        neg = true;
                        i += 1;
                    }
                    let mut matched = false;
                    let mut first = true;
                    while i < p.len() && (first || p[i].0 != b']') {
                        first = false;
                        let lo = p[i].0;
                        if i + 2 < p.len() && p[i + 1].0 == b'-' && p[i + 2].0 != b']' {
                            if ch >= lo && ch <= p[i + 2].0 {
                                matched = true;
                            }
                            i += 3;
                        } else {
                            if ch == lo {
                                matched = true;
                            }
                            i += 1;
                        }
                    }
                    if i >= p.len() {
                        // Sem `]`: `[` literal.
                        return ch == b'[' && go(&p[1..], &s[1..]);
                    }
                    return matched != neg && go(&p[i + 1..], &s[1..]);
                }
                _ => {}
            }
        }
        !s.is_empty() && s[0] == c && go(&p[1..], &s[1..])
    }
    go(p, s)
}

fn glob(pat: &[(u8, bool)]) -> Vec<Vec<u8>> {
    let absolute = pat.first().map(|x| x.0) == Some(b'/');
    let comps: Vec<&[(u8, bool)]> = pat.split(|x| x.0 == b'/').filter(|c| !c.is_empty()).collect();
    let mut cur: Vec<Vec<u8>> = vec![if absolute { b"/".to_vec() } else { Vec::new() }];
    for comp in comps {
        let has_glob = comp.iter().any(|(b, a)| *a && matches!(b, b'*' | b'?' | b'['));
        let mut next = Vec::new();
        for base in &cur {
            if !has_glob {
                let lit: Vec<u8> = comp.iter().map(|x| x.0).collect();
                next.push(join_rel(base, &lit));
                continue;
            }
            let dir = if base.is_empty() { b".".to_vec() } else { base.clone() };
            let Ok(entries) = sys::read_dir(&dir) else { continue };
            let mut names: Vec<Vec<u8>> = entries.into_iter().map(|e| e.name).collect();
            names.sort();
            for n in names {
                if n.starts_with(b".") && comp.first().map(|x| x.0) != Some(b'.') {
                    continue;
                }
                if fnmatch(comp, &n) {
                    next.push(join_rel(base, &n));
                }
            }
        }
        cur = next;
    }
    cur.retain(|p| lstat(p).is_ok());
    cur
}

fn join_rel(base: &[u8], name: &[u8]) -> Vec<u8> {
    if base.is_empty() {
        return name.to_vec();
    }
    let mut p = base.to_vec();
    if !p.ends_with(b"/") {
        p.push(b'/');
    }
    p.extend_from_slice(name);
    p
}

// ---------------------------------------------------------------------------------------------
// bash
// ---------------------------------------------------------------------------------------------

fn bash_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv = to_args(args);
    let mut i = 1;
    let mut script: Option<Vec<u8>> = None;
    let mut errexit = false;
    let mut name = b"bash".to_vec();
    let mut positional = Vec::new();
    let mut from_c = false;
    while i < argv.len() {
        let a = &argv[i];
        if a == b"-c" {
            from_c = true;
            script = argv.get(i + 1).cloned();
            i += 2;
            if let Some(n) = argv.get(i) {
                name = n.clone();
                positional = argv[i + 1..].to_vec();
            }
            break;
        } else if a == b"-e" {
            errexit = true;
        } else if a == b"-o" || a == b"+o" {
            i += 1;
        } else if a.starts_with(b"-") && a.len() > 1 {
            if a.contains(&b'e') {
                errexit = true;
            }
        } else {
            match read_input(a) {
                Ok(d) => script = Some(d),
                Err(e) => {
                    errs(&format!("bash: {}: {}\n", lossy(a), e.message()));
                    return 127;
                }
            }
            name = a.clone();
            positional = argv[i + 1..].to_vec();
            break;
        }
        i += 1;
    }
    let script = match script {
        Some(s) => s,
        None => {
            if from_c {
                errs("bash: -c: option requires an argument\n");
                return 2;
            }
            read_all_fd(Fd::STDIN)
        }
    };
    let mut p = Parser::new(&script);
    let list = match p.parse_program() {
        Ok(l) => l,
        Err(e) => {
            let line_no = p.line;
            let src_line = script.split(|c| *c == b'\n').nth(line_no.saturating_sub(1)).unwrap_or(&[]);
            if from_c {
                errs(&format!("bash: -c: line {line_no}: {e}\n"));
                if e.starts_with("syntax error near") {
                    errs(&format!("bash: -c: line {line_no}: `{}'\n", lossy(src_line)));
                }
            } else {
                errs(&format!("{}: line {line_no}: {e}\n", lossy(&name)));
            }
            return 2;
        }
    };
    let mut sh = Shell::new(name, positional);
    sh.errexit = errexit;
    match sh.run_list(&list) {
        Ok(st) => st,
        Err(Flow::Exit(c)) => c,
    }
}

// ---------------------------------------------------------------------------------------------
// echo, printf, test
// ---------------------------------------------------------------------------------------------

fn unescape(s: &[u8], stop_at_c: &mut bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c != b'\\' || i + 1 >= s.len() {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        let e = s[i];
        i += 1;
        match e {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'v' => out.push(11),
            b'e' | b'E' => out.push(0x1b),
            b'\\' => out.push(b'\\'),
            b'c' => {
                *stop_at_c = true;
                return out;
            }
            b'0' => {
                let mut v: u32 = 0;
                let mut k = 0;
                while k < 3 && i < s.len() && (b'0'..=b'7').contains(&s[i]) {
                    v = v * 8 + (s[i] - b'0') as u32;
                    i += 1;
                    k += 1;
                }
                out.push(v as u8);
            }
            b'x' => {
                let mut v: u32 = 0;
                let mut k = 0;
                while k < 2 && i < s.len() && s[i].is_ascii_hexdigit() {
                    v = v * 16 + (s[i] as char).to_digit(16).unwrap_or(0);
                    i += 1;
                    k += 1;
                }
                if k == 0 {
                    out.extend_from_slice(b"\\x");
                } else {
                    out.push(v as u8);
                }
            }
            _ => {
                out.push(b'\\');
                out.push(e);
            }
        }
    }
    out
}

fn echo_impl(args: &[Vec<u8>]) -> Vec<u8> {
    let mut newline = true;
    let mut escapes = false;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a.len() > 1 && a[0] == b'-' && a[1..].iter().all(|c| matches!(c, b'n' | b'e' | b'E')) {
            for c in &a[1..] {
                match c {
                    b'n' => newline = false,
                    b'e' => escapes = true,
                    _ => escapes = false,
                }
            }
            i += 1;
        } else {
            break;
        }
    }
    let mut out_v = Vec::new();
    for (k, a) in args[i..].iter().enumerate() {
        if k > 0 {
            out_v.push(b' ');
        }
        if escapes {
            let mut stop = false;
            out_v.extend(unescape(a, &mut stop));
            if stop {
                return out_v;
            }
        } else {
            out_v.extend_from_slice(a);
        }
    }
    if newline {
        out_v.push(b'\n');
    }
    out_v
}

fn echo_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    out(&echo_impl(&a[1..]));
    0
}

fn parse_int_arg(a: &[u8]) -> Option<i64> {
    if a.first() == Some(&b'\'') || a.first() == Some(&b'"') {
        return a.get(1).map(|c| *c as i64).or(Some(0));
    }
    let s = lossy(a);
    let t = s.trim();
    if t.is_empty() {
        return Some(0);
    }
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i64::from_str_radix(h, 16).ok();
    }
    t.parse().ok()
}

fn printf_impl(args: &[Vec<u8>], prog: &str) -> i32 {
    let Some(fmt) = args.first() else {
        errs(&format!("{prog}: usage: printf [-v var] format [arguments]\n"));
        return 2;
    };
    let rest = &args[1..];
    let mut ai = 0;
    let mut status = 0;
    let mut buf = Vec::new();
    loop {
        let mut consumed = false;
        let mut i = 0;
        while i < fmt.len() {
            let c = fmt[i];
            if c == b'\\' {
                // Um escape por vez: `\0NNN`, `\NNN`, `\xHH` ou `\c`.
                let mut j = i + 1;
                let octal = |b: u8| (b'0'..=b'7').contains(&b);
                match fmt.get(j) {
                    Some(b'0') => {
                        j += 1;
                        let mut k = 0;
                        while k < 3 && j < fmt.len() && octal(fmt[j]) {
                            j += 1;
                            k += 1;
                        }
                    }
                    Some(b'1'..=b'7') => {
                        let mut k = 0;
                        while k < 3 && j < fmt.len() && octal(fmt[j]) {
                            j += 1;
                            k += 1;
                        }
                        let v = fmt[i + 1..j].iter().fold(0u32, |a, d| a * 8 + (d - b'0') as u32);
                        buf.push(v as u8);
                        i = j;
                        continue;
                    }
                    Some(b'x') => {
                        j += 1;
                        let mut k = 0;
                        while k < 2 && j < fmt.len() && fmt[j].is_ascii_hexdigit() {
                            j += 1;
                            k += 1;
                        }
                    }
                    Some(_) => j += 1,
                    None => {}
                }
                let mut stop = false;
                buf.extend(unescape(&fmt[i..j], &mut stop));
                i = j;
                continue;
            }
            if c != b'%' {
                buf.push(c);
                i += 1;
                continue;
            }
            i += 1;
            if i < fmt.len() && fmt[i] == b'%' {
                buf.push(b'%');
                i += 1;
                continue;
            }
            let mut flags = Vec::new();
            while i < fmt.len() && matches!(fmt[i], b'-' | b'+' | b' ' | b'#' | b'0') {
                flags.push(fmt[i]);
                i += 1;
            }
            let mut width: Option<usize> = None;
            if i < fmt.len() && fmt[i] == b'*' {
                let a = rest.get(ai).cloned().unwrap_or_default();
                ai += 1;
                consumed = true;
                width = parse_int_arg(&a).map(|v| v.max(0) as usize);
                i += 1;
            } else {
                let st = i;
                while i < fmt.len() && fmt[i].is_ascii_digit() {
                    i += 1;
                }
                if i > st {
                    width = lossy(&fmt[st..i]).parse().ok();
                }
            }
            let mut prec: Option<usize> = None;
            if i < fmt.len() && fmt[i] == b'.' {
                i += 1;
                let st = i;
                while i < fmt.len() && fmt[i].is_ascii_digit() {
                    i += 1;
                }
                prec = Some(lossy(&fmt[st..i]).parse().unwrap_or(0));
            }
            while i < fmt.len() && matches!(fmt[i], b'l' | b'h' | b'j' | b'z' | b'q') {
                i += 1;
            }
            let Some(&conv) = fmt.get(i) else {
                buf.push(b'%');
                break;
            };
            i += 1;
            let arg = rest.get(ai).cloned();
            if arg.is_some() {
                consumed = true;
            }
            ai += 1;
            let arg = arg.unwrap_or_default();
            let left = flags.contains(&b'-');
            let zero = flags.contains(&b'0') && !left;
            let pad = |s: Vec<u8>, numeric: bool| -> Vec<u8> {
                let w = width.unwrap_or(0);
                if s.len() >= w {
                    return s;
                }
                let n = w - s.len();
                if left {
                    let mut v = s;
                    v.extend(std::iter::repeat_n(b' ', n));
                    v
                } else if zero && numeric {
                    let (sign, digits) = if s.first() == Some(&b'-') { (vec![b'-'], s[1..].to_vec()) } else { (Vec::new(), s.clone()) };
                    let mut v = sign;
                    v.extend(std::iter::repeat_n(b'0', n));
                    v.extend(digits);
                    v
                } else {
                    let mut v: Vec<u8> = std::iter::repeat_n(b' ', n).collect();
                    v.extend(s);
                    v
                }
            };
            match conv {
                b's' => {
                    let mut s = arg.clone();
                    if let Some(p) = prec {
                        s.truncate(p);
                    }
                    buf.extend(pad(s, false));
                }
                b'b' => {
                    let mut stop = false;
                    let s = unescape(&arg, &mut stop);
                    buf.extend(pad(s, false));
                    if stop {
                        out(&buf);
                        return status;
                    }
                }
                b'c' => {
                    let s: Vec<u8> = arg.first().map(|c| vec![*c]).unwrap_or_default();
                    buf.extend(pad(s, false));
                }
                b'd' | b'i' | b'u' | b'x' | b'X' | b'o' => {
                    let v = match parse_int_arg(&arg) {
                        Some(v) => v,
                        None => {
                            errs(&format!("{prog}: {}: invalid number\n", lossy(&arg)));
                            status = 1;
                            0
                        }
                    };
                    let mut s = match conv {
                        b'x' => format!("{v:x}"),
                        b'X' => format!("{v:X}"),
                        b'o' => format!("{v:o}"),
                        _ => {
                            if flags.contains(&b'+') && v >= 0 {
                                format!("+{v}")
                            } else {
                                v.to_string()
                            }
                        }
                    }
                    .into_bytes();
                    if let Some(p) = prec
                        && s.len() < p
                    {
                        let mut z: Vec<u8> = std::iter::repeat_n(b'0', p - s.len()).collect();
                        z.extend(s);
                        s = z;
                    }
                    buf.extend(pad(s, true));
                }
                b'f' | b'g' | b'e' => {
                    let v: f64 = lossy(&arg).trim().parse().unwrap_or(0.0);
                    let s = format!("{:.*}", prec.unwrap_or(6), v).into_bytes();
                    buf.extend(pad(s, true));
                }
                b'q' => buf.extend(arg),
                other => {
                    buf.push(b'%');
                    buf.push(other);
                }
            }
        }
        if !consumed || ai >= rest.len() {
            break;
        }
    }
    out(&buf);
    status
}

fn printf_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    printf_impl(&a[1..], "printf")
}

fn test_impl(args: &[Vec<u8>], bracket: bool, prog: &str) -> i32 {
    let mut a: Vec<Vec<u8>> = args.to_vec();
    if bracket {
        if a.last().map(|x| x.as_slice()) != Some(b"]") {
            errs("bash: [: missing `]'\n");
            return 2;
        }
        a.pop();
    }
    let _ = prog;
    match test_eval(&a) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(e) => {
            errs(&format!("bash: {}: {e}\n", if bracket { "[" } else { "test" }));
            2
        }
    }
}

fn test_unary(op: &[u8], arg: &[u8]) -> Option<bool> {
    let st = || stat(arg).ok();
    Some(match op {
        b"-e" | b"-a" => st().is_some(),
        b"-f" => st().is_some_and(|s| s.file_type() == FileType::Regular),
        b"-d" => st().is_some_and(|s| s.file_type() == FileType::Directory),
        b"-s" => st().is_some_and(|s| s.size > 0),
        b"-x" => sysc().faccessat(Fd::CWD, arg, AccessMode::X_OK, AtFlags::empty()).is_ok(),
        b"-r" | b"-w" => st().is_some(),
        b"-L" | b"-h" => lstat(arg).is_ok_and(|s| s.file_type() == FileType::Symlink),
        b"-z" => arg.is_empty(),
        b"-n" => !arg.is_empty(),
        _ => return None,
    })
}

fn test_binary(a: &[u8], op: &[u8], b: &[u8]) -> Result<Option<bool>, String> {
    let num = |x: &[u8]| -> Result<i64, String> { lossy(x).trim().parse::<i64>().map_err(|_| format!("{}: integer expression expected", lossy(x))) };
    Ok(Some(match op {
        b"=" | b"==" => a == b,
        b"!=" => a != b,
        b"<" => a < b,
        b">" => a > b,
        b"-eq" => num(a)? == num(b)?,
        b"-ne" => num(a)? != num(b)?,
        b"-lt" => num(a)? < num(b)?,
        b"-le" => num(a)? <= num(b)?,
        b"-gt" => num(a)? > num(b)?,
        b"-ge" => num(a)? >= num(b)?,
        _ => return Ok(None),
    }))
}

fn test_eval(a: &[Vec<u8>]) -> Result<bool, String> {
    match a.len() {
        0 => return Ok(false),
        1 => return Ok(!a[0].is_empty()),
        2 => {
            if a[0] == b"!" {
                return Ok(a[1].is_empty());
            }
            return test_unary(&a[0], &a[1]).ok_or_else(|| format!("{}: unary operator expected", lossy(&a[0])));
        }
        3 => {
            if let Some(r) = test_binary(&a[0], &a[1], &a[2])? {
                return Ok(r);
            }
            if a[0] == b"!" {
                return test_eval(&a[1..]).map(|x| !x);
            }
            if a[0] == b"(" && a[2] == b")" {
                return Ok(!a[1].is_empty());
            }
        }
        _ => {}
    }
    // -o tem a menor precedência, depois -a.
    if let Some(i) = a.iter().position(|x| x == b"-o") {
        return Ok(test_eval(&a[..i])? || test_eval(&a[i + 1..])?);
    }
    if let Some(i) = a.iter().position(|x| x == b"-a") {
        return Ok(test_eval(&a[..i])? && test_eval(&a[i + 1..])?);
    }
    if a[0] == b"!" {
        return test_eval(&a[1..]).map(|x| !x);
    }
    if a[0] == b"(" && a.last().map(|x| x.as_slice()) == Some(b")") {
        return test_eval(&a[1..a.len() - 1]);
    }
    Err("too many arguments".into())
}

fn test_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    test_impl(&a[1..], false, "test")
}

fn bracket_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    test_impl(&a[1..], true, "[")
}

fn true_main(_ctx: &mut Ctx, _args: &[OsString]) -> i32 {
    0
}

fn false_main(_ctx: &mut Ctx, _args: &[OsString]) -> i32 {
    1
}

// ---------------------------------------------------------------------------------------------
// Utilitários de arquivo
// ---------------------------------------------------------------------------------------------

/// Separa flags curtas (`-rf`) dos operandos; `--` encerra as flags.
fn split_flags(args: &[Vec<u8>]) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut flags = Vec::new();
    let mut ops = Vec::new();
    let mut done = false;
    for a in args {
        if !done && a == b"--" {
            done = true;
            continue;
        }
        if !done && a.len() > 1 && a[0] == b'-' && a[1] != b'-' {
            flags.extend_from_slice(&a[1..]);
        } else if !done && a.starts_with(b"--") {
            // Opções longas: as que importam viram curtas.
            match a.as_slice() {
                b"--recursive" => flags.push(b'r'),
                b"--force" => flags.push(b'f'),
                b"--parents" => flags.push(b'p'),
                _ => {}
            }
        } else {
            ops.push(a.clone());
        }
    }
    (flags, ops)
}

fn cat_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let (_, mut files) = split_flags(&a[1..]);
    if files.is_empty() {
        files.push(b"-".to_vec());
    }
    let mut st = 0;
    for f in files {
        match read_input(&f) {
            Ok(d) => out(&d),
            Err(e) => {
                errs(&format!("cat: {}: {}\n", lossy(&f), e.message()));
                st = 1;
            }
        }
    }
    st
}

fn remove_tree(p: &[u8]) -> Result<(), Errno> {
    let s = sysc();
    let st = lstat(p)?;
    if st.file_type() == FileType::Directory {
        for e in sys::read_dir(p)? {
            remove_tree(&join_path(p, &e.name))?;
        }
        s.unlinkat(Fd::CWD, p, AtFlags::REMOVEDIR)
    } else {
        s.unlinkat(Fd::CWD, p, AtFlags::empty())
    }
}

fn rm_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let (flags, ops) = split_flags(&a[1..]);
    let force = flags.contains(&b'f');
    let recursive = flags.contains(&b'r') || flags.contains(&b'R');
    let verbose = flags.contains(&b'v');
    let mut st = 0;
    if ops.is_empty() && !force {
        errs("rm: missing operand\nTry 'rm --help' for more information.\n");
        return 1;
    }
    for p in ops {
        match lstat(&p) {
            Err(e) => {
                if !force {
                    errs(&format!("rm: cannot remove '{}': {}\n", lossy(&p), e.message()));
                    st = 1;
                }
            }
            Ok(s) => {
                if s.file_type() == FileType::Directory && !recursive {
                    errs(&format!("rm: cannot remove '{}': Is a directory\n", lossy(&p)));
                    st = 1;
                    continue;
                }
                if let Err(e) = remove_tree(&p) {
                    errs(&format!("rm: cannot remove '{}': {}\n", lossy(&p), e.message()));
                    st = 1;
                } else if verbose {
                    out(format!("removed '{}'\n", lossy(&p)).as_bytes());
                }
            }
        }
    }
    st
}

fn mkdir_p(p: &[u8]) -> Result<(), Errno> {
    let s = sysc();
    match s.mkdirat(Fd::CWD, p, 0o777) {
        Ok(()) => Ok(()),
        Err(Errno::EEXIST) => {
            if is_dir(p) {
                Ok(())
            } else {
                Err(Errno::EEXIST)
            }
        }
        Err(Errno::ENOENT) => {
            let t = p.strip_suffix(b"/").unwrap_or(p);
            if let Some(i) = t.iter().rposition(|c| *c == b'/')
                && i > 0
            {
                mkdir_p(&t[..i])?;
            }
            match s.mkdirat(Fd::CWD, p, 0o777) {
                Ok(()) | Err(Errno::EEXIST) => Ok(()),
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(e),
    }
}

fn mkdir_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let (flags, ops) = split_flags(&a[1..]);
    let parents = flags.contains(&b'p');
    let mut st = 0;
    for p in ops {
        let r = if parents { mkdir_p(&p) } else { sysc().mkdirat(Fd::CWD, &p, 0o777) };
        if let Err(e) = r {
            errs(&format!("mkdir: cannot create directory \u{2018}{}\u{2019}: {}\n", lossy(&p), e.message()));
            st = 1;
        }
    }
    st
}

fn ls_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let (flags, mut ops) = split_flags(&a[1..]);
    let all = flags.contains(&b'a');
    let almost = flags.contains(&b'A');
    let dir_only = flags.contains(&b'd');
    if ops.is_empty() {
        ops.push(b".".to_vec());
    }
    let mut st = 0;
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for p in &ops {
        match stat(p).or_else(|_| lstat(p)) {
            Err(e) => {
                errs(&format!("ls: cannot access '{}': {}\n", lossy(p), e.message()));
                st = 2;
            }
            Ok(s) => {
                if s.file_type() == FileType::Directory && !dir_only {
                    dirs.push(p.clone());
                } else {
                    files.push(p.clone());
                }
            }
        }
    }
    files.sort();
    dirs.sort();
    for f in &files {
        out(f);
        out(b"\n");
    }
    let multi = ops.len() > 1;
    for (k, d) in dirs.iter().enumerate() {
        if multi {
            if k > 0 || !files.is_empty() {
                out(b"\n");
            }
            out(d);
            out(b":\n");
        }
        let mut names: Vec<Vec<u8>> = match sys::read_dir(d) {
            Ok(es) => es.into_iter().map(|e| e.name).collect(),
            Err(e) => {
                errs(&format!("ls: cannot open directory '{}': {}\n", lossy(d), e.message()));
                st = 2;
                continue;
            }
        };
        if all {
            names.push(b".".to_vec());
            names.push(b"..".to_vec());
        }
        names.retain(|n| all || almost || !n.starts_with(b"."));
        names.sort();
        for n in names {
            out(&n);
            out(b"\n");
        }
    }
    st
}

fn copy_tree(src: &[u8], dst: &[u8], recursive: bool) -> Result<(), String> {
    let s = sysc();
    let st = lstat(src).map_err(|e| format!("cannot stat '{}': {}", lossy(src), e.message()))?;
    match st.file_type() {
        FileType::Directory => {
            if !recursive {
                return Err(format!("-r not specified; omitting directory '{}'", lossy(src)));
            }
            let _ = s.mkdirat(Fd::CWD, dst, 0o777);
            for e in sys::read_dir(src).map_err(|e| e.message())? {
                copy_tree(&join_path(src, &e.name), &join_path(dst, &e.name), true)?;
            }
            Ok(())
        }
        FileType::Symlink if recursive => {
            let t = s.readlinkat(Fd::CWD, src).map_err(|e| e.message())?;
            s.symlinkat(&t, Fd::CWD, dst).map_err(|e| e.message())
        }
        _ => {
            let data = read_input(src).map_err(|e| format!("cannot open '{}' for reading: {}", lossy(src), e.message()))?;
            write_file(dst, &data, OFlags::TRUNC).map_err(|e| format!("cannot create regular file '{}': {}", lossy(dst), e.message()))?;
            let _ = s.fchmodat(Fd::CWD, dst, st.mode & 0o777 & !current_umask(), AtFlags::empty());
            Ok(())
        }
    }
}

fn current_umask() -> u32 {
    let s = sysc();
    let m = s.umask(0o022);
    s.umask(m);
    m
}

fn cp_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let (flags, ops) = split_flags(&a[1..]);
    let recursive = flags.iter().any(|c| matches!(c, b'r' | b'R' | b'a'));
    if ops.len() < 2 {
        errs("cp: missing file operand\n");
        return 1;
    }
    let (srcs, dst) = ops.split_at(ops.len() - 1);
    let dst = &dst[0];
    let into_dir = is_dir(dst);
    let mut st = 0;
    for src in srcs {
        let target = if into_dir { join_path(dst, base_name(src)) } else { dst.clone() };
        if let Err(e) = copy_tree(src, &target, recursive) {
            errs(&format!("cp: {e}\n"));
            st = 1;
        }
    }
    st
}

fn mv_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let (_, ops) = split_flags(&a[1..]);
    if ops.len() < 2 {
        errs("mv: missing file operand\n");
        return 1;
    }
    let (srcs, dst) = ops.split_at(ops.len() - 1);
    let dst = &dst[0];
    let into_dir = is_dir(dst);
    let mut st = 0;
    for src in srcs {
        let target = if into_dir { join_path(dst, base_name(src)) } else { dst.clone() };
        if lstat(src).is_err() {
            errs(&format!("mv: cannot stat '{}': No such file or directory\n", lossy(src)));
            st = 1;
            continue;
        }
        if let Err(e) = sysc().renameat2(Fd::CWD, src, Fd::CWD, &target, RenameFlags::empty()) {
            errs(&format!("mv: cannot move '{}' to '{}': {}\n", lossy(src), lossy(&target), e.message()));
            st = 1;
        }
    }
    st
}

fn touch_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let (_, ops) = split_flags(&a[1..]);
    let s = sysc();
    let mut st = 0;
    for p in ops {
        if lstat(&p).is_err() {
            if let Err(e) = write_file(&p, b"", OFlags::empty()) {
                errs(&format!("touch: cannot touch '{}': {}\n", lossy(&p), e.message()));
                st = 1;
            }
        } else {
            let _ = s.utimensat(Fd::CWD, &p, SetTime::Now, SetTime::Now, AtFlags::empty());
        }
    }
    st
}

/// `-n N`, `-N`, `-nN`, `--lines=N`: (contagem, `+` na frente).
fn count_opt(args: &[Vec<u8>]) -> (Option<(i64, bool)>, Vec<Vec<u8>>, bool) {
    let mut n = None;
    let mut files = Vec::new();
    let mut bytes = false;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let parse = |v: &[u8]| -> Option<(i64, bool)> {
            let plus = v.first() == Some(&b'+');
            let t = lossy(v.strip_prefix(b"+").unwrap_or(v));
            t.parse::<i64>().ok().map(|x| (x, plus))
        };
        if a == b"-n" || a == b"-c" {
            bytes = a == b"-c";
            n = args.get(i + 1).and_then(|v| parse(v));
            i += 2;
            continue;
        }
        if let Some(v) = a.strip_prefix(b"-n").or_else(|| a.strip_prefix(b"--lines=")) {
            n = parse(v);
        } else if let Some(v) = a.strip_prefix(b"-c") {
            bytes = true;
            n = parse(v);
        } else if a.len() > 1 && a[0] == b'-' && a[1..].iter().all(|c| c.is_ascii_digit()) {
            n = parse(&a[1..]);
        } else if a == b"-q" || a == b"-v" {
        } else {
            files.push(a.clone());
        }
        i += 1;
    }
    (n, files, bytes)
}

fn head_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let (n, mut files, bytes) = count_opt(&a[1..]);
    let (n, _) = n.unwrap_or((10, false));
    if files.is_empty() {
        files.push(b"-".to_vec());
    }
    let multi = files.len() > 1;
    let mut st = 0;
    for (k, f) in files.iter().enumerate() {
        let d = match read_input(f) {
            Ok(d) => d,
            Err(e) => {
                errs(&format!("head: cannot open '{}' for reading: {}\n", lossy(f), e.message()));
                st = 1;
                continue;
            }
        };
        if multi {
            if k > 0 {
                out(b"\n");
            }
            out(format!("==> {} <==\n", lossy(f)).as_bytes());
        }
        if bytes {
            let end = if n >= 0 { (n as usize).min(d.len()) } else { d.len().saturating_sub((-n) as usize) };
            out(&d[..end]);
            continue;
        }
        let lines: Vec<&[u8]> = d.split_inclusive(|c| *c == b'\n').collect();
        let take = if n >= 0 { (n as usize).min(lines.len()) } else { lines.len().saturating_sub((-n) as usize) };
        for l in &lines[..take] {
            out(l);
        }
    }
    st
}

fn tail_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let (n, mut files, bytes) = count_opt(&a[1..]);
    let (n, plus) = n.unwrap_or((10, false));
    if files.is_empty() {
        files.push(b"-".to_vec());
    }
    let mut st = 0;
    for f in &files {
        let d = match read_input(f) {
            Ok(d) => d,
            Err(e) => {
                errs(&format!("tail: cannot open '{}' for reading: {}\n", lossy(f), e.message()));
                st = 1;
                continue;
            }
        };
        if bytes {
            let start = if plus { ((n.max(1) - 1) as usize).min(d.len()) } else { d.len().saturating_sub(n as usize) };
            out(&d[start..]);
            continue;
        }
        let lines: Vec<&[u8]> = d.split_inclusive(|c| *c == b'\n').collect();
        let start = if plus { ((n.max(1) - 1) as usize).min(lines.len()) } else { lines.len().saturating_sub(n as usize) };
        for l in &lines[start..] {
            out(l);
        }
    }
    st
}

fn wc_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let (flags, files) = split_flags(&a[1..]);
    let mut want_l = flags.contains(&b'l');
    let mut want_w = flags.contains(&b'w');
    let mut want_c = flags.contains(&b'c') || flags.contains(&b'm');
    if !want_l && !want_w && !want_c {
        want_l = true;
        want_w = true;
        want_c = true;
    }
    let inputs: Vec<Vec<u8>> = if files.is_empty() { vec![b"-".to_vec()] } else { files.clone() };
    let mut rows: Vec<(Vec<usize>, Option<Vec<u8>>)> = Vec::new();
    let mut totals = [0usize; 3];
    let mut st = 0;
    for f in &inputs {
        let d = match read_input(f) {
            Ok(d) => d,
            Err(e) => {
                errs(&format!("wc: {}: {}\n", lossy(f), e.message()));
                st = 1;
                continue;
            }
        };
        let l = d.iter().filter(|c| **c == b'\n').count();
        let w = d.split(|c| c.is_ascii_whitespace()).filter(|x| !x.is_empty()).count();
        let c = d.len();
        totals[0] += l;
        totals[1] += w;
        totals[2] += c;
        let mut v = Vec::new();
        if want_l {
            v.push(l);
        }
        if want_w {
            v.push(w);
        }
        if want_c {
            v.push(c);
        }
        rows.push((v, if files.is_empty() { None } else { Some(f.clone()) }));
    }
    if inputs.len() > 1 {
        let mut v = Vec::new();
        if want_l {
            v.push(totals[0]);
        }
        if want_w {
            v.push(totals[1]);
        }
        if want_c {
            v.push(totals[2]);
        }
        rows.push((v, Some(b"total".to_vec())));
    }
    let ncounts = [want_l, want_w, want_c].iter().filter(|x| **x).count();
    let width = if ncounts == 1 && rows.len() == 1 {
        1
    } else if files.is_empty() {
        7
    } else {
        totals[2].to_string().len().max(1)
    };
    for (v, name) in rows {
        let cells: Vec<String> = v.iter().map(|x| format!("{x:>width$}")).collect();
        let mut line = cells.join(" ");
        if let Some(n) = name {
            line.push(' ');
            line.push_str(&lossy(&n));
        }
        line.push('\n');
        out(line.as_bytes());
    }
    st
}

fn sort_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let (flags, mut files) = split_flags(&a[1..]);
    if files.is_empty() {
        files.push(b"-".to_vec());
    }
    let mut lines: Vec<Vec<u8>> = Vec::new();
    for f in &files {
        match read_input(f) {
            Ok(d) => lines.extend(split_lines(&d).into_iter().map(|l| l.to_vec())),
            Err(e) => {
                errs(&format!("sort: cannot read: {}: {}\n", lossy(f), e.message()));
                return 2;
            }
        }
    }
    if flags.contains(&b'n') {
        lines.sort_by(|x, y| {
            let nx: f64 = lossy(x).trim().parse().unwrap_or(0.0);
            let ny: f64 = lossy(y).trim().parse().unwrap_or(0.0);
            nx.partial_cmp(&ny).unwrap_or(std::cmp::Ordering::Equal).then(x.cmp(y))
        });
    } else {
        lines.sort();
    }
    if flags.contains(&b'r') {
        lines.reverse();
    }
    if flags.contains(&b'u') {
        lines.dedup();
    }
    for l in lines {
        out(&l);
        out(b"\n");
    }
    0
}

fn uniq_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let (flags, files) = split_flags(&a[1..]);
    let d = read_input(files.first().map(|v| v.as_slice()).unwrap_or(b"-")).unwrap_or_default();
    let lines = split_lines(&d);
    let mut groups: Vec<(&[u8], usize)> = Vec::new();
    for l in lines {
        match groups.last_mut() {
            Some((g, n)) if *g == l => *n += 1,
            _ => groups.push((l, 1)),
        }
    }
    for (l, n) in groups {
        if flags.contains(&b'c') {
            out(format!("{n:>7} ").as_bytes());
        }
        out(l);
        out(b"\n");
    }
    0
}

fn cut_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let mut delim = b'\t';
    let mut fields: Vec<(usize, usize)> = Vec::new();
    let mut chars = false;
    let mut files = Vec::new();
    let mut i = 1;
    let parse_list = |s: &[u8]| -> Vec<(usize, usize)> {
        lossy(s)
            .split(',')
            .filter_map(|r| {
                let (x, y) = match r.split_once('-') {
                    Some((x, y)) => (x.parse().unwrap_or(1), if y.is_empty() { usize::MAX } else { y.parse().unwrap_or(usize::MAX) }),
                    None => {
                        let v: usize = r.parse().ok()?;
                        (v, v)
                    }
                };
                Some((x, y))
            })
            .collect()
    };
    while i < a.len() {
        let x = &a[i];
        if x == b"-d" {
            delim = a.get(i + 1).and_then(|v| v.first().copied()).unwrap_or(b'\t');
            i += 2;
            continue;
        } else if let Some(d) = x.strip_prefix(b"-d") {
            delim = d.first().copied().unwrap_or(b'\t');
        } else if x == b"-f" || x == b"-c" {
            chars = x == b"-c";
            fields = parse_list(a.get(i + 1).map(|v| v.as_slice()).unwrap_or(b""));
            i += 2;
            continue;
        } else if let Some(f) = x.strip_prefix(b"-f") {
            fields = parse_list(f);
        } else if let Some(f) = x.strip_prefix(b"-c") {
            chars = true;
            fields = parse_list(f);
        } else {
            files.push(x.clone());
        }
        i += 1;
    }
    let d = read_input(files.first().map(|v| v.as_slice()).unwrap_or(b"-")).unwrap_or_default();
    let sel = |k: usize| fields.iter().any(|(x, y)| k >= *x && k <= *y);
    for l in split_lines(&d) {
        if chars {
            let v: Vec<u8> = l.iter().enumerate().filter(|(k, _)| sel(k + 1)).map(|(_, c)| *c).collect();
            out(&v);
        } else if !l.contains(&delim) {
            out(l);
        } else {
            let parts: Vec<&[u8]> = l.split(|c| *c == delim).enumerate().filter(|(k, _)| sel(k + 1)).map(|(_, p)| p).collect();
            out(&parts.join(&delim));
        }
        out(b"\n");
    }
    0
}

// ---- regex mínima (grep e sed) ----

#[derive(Clone, Debug)]
enum Atom {
    Ch(u8),
    Any,
    Class(Vec<(u8, u8)>, bool),
    Start,
    End,
}

#[derive(Clone, Debug)]
struct Piece {
    atom: Atom,
    min: usize,
    max: usize,
}

fn compile_re(pat: &[u8], ere: bool, fixed: bool) -> Vec<Vec<Piece>> {
    if fixed {
        return vec![pat.iter().map(|&c| Piece { atom: Atom::Ch(c), min: 1, max: 1 }).collect()];
    }
    // Alternativas no nível de cima.
    let mut alts: Vec<Vec<u8>> = vec![Vec::new()];
    let mut i = 0;
    while i < pat.len() {
        if ere && pat[i] == b'|' {
            alts.push(Vec::new());
            i += 1;
            continue;
        }
        if !ere && pat[i] == b'\\' && pat.get(i + 1) == Some(&b'|') {
            alts.push(Vec::new());
            i += 2;
            continue;
        }
        if pat[i] == b'\\' && i + 1 < pat.len() {
            alts.last_mut().map(|a| a.extend_from_slice(&pat[i..i + 2]));
            i += 2;
            continue;
        }
        if let Some(a) = alts.last_mut() {
            a.push(pat[i]);
        }
        i += 1;
    }
    alts.iter().map(|a| compile_alt(a, ere)).collect()
}

fn compile_alt(p: &[u8], ere: bool) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let mut i = 0;
    while i < p.len() {
        let c = p[i];
        let atom = match c {
            b'^' if i == 0 => {
                i += 1;
                out.push(Piece { atom: Atom::Start, min: 1, max: 1 });
                continue;
            }
            b'$' if i + 1 == p.len() => {
                i += 1;
                out.push(Piece { atom: Atom::End, min: 1, max: 1 });
                continue;
            }
            b'.' => {
                i += 1;
                Atom::Any
            }
            b'[' => {
                let mut j = i + 1;
                let mut neg = false;
                if p.get(j) == Some(&b'^') {
                    neg = true;
                    j += 1;
                }
                let mut set = Vec::new();
                let mut first = true;
                while j < p.len() && (first || p[j] != b']') {
                    first = false;
                    if p[j] == b'[' && p.get(j + 1) == Some(&b':') {
                        let end = p[j..].windows(2).position(|w| w == b":]").map(|e| e + j);
                        if let Some(e) = end {
                            let name = &p[j + 2..e];
                            match name {
                                b"digit" => set.push((b'0', b'9')),
                                b"alpha" => {
                                    set.push((b'a', b'z'));
                                    set.push((b'A', b'Z'));
                                }
                                b"alnum" => {
                                    set.push((b'a', b'z'));
                                    set.push((b'A', b'Z'));
                                    set.push((b'0', b'9'));
                                }
                                b"upper" => set.push((b'A', b'Z')),
                                b"lower" => set.push((b'a', b'z')),
                                b"space" => {
                                    for c in [b' ', b'\t', b'\n', b'\r', 11, 12] {
                                        set.push((c, c));
                                    }
                                }
                                _ => {}
                            }
                            j = e + 2;
                            continue;
                        }
                    }
                    if j + 2 < p.len() && p[j + 1] == b'-' && p[j + 2] != b']' {
                        set.push((p[j], p[j + 2]));
                        j += 3;
                    } else {
                        set.push((p[j], p[j]));
                        j += 1;
                    }
                }
                i = j + 1;
                Atom::Class(set, neg)
            }
            b'\\' if i + 1 < p.len() => {
                let n = p[i + 1];
                i += 2;
                match n {
                    b'+' | b'?' if !ere => {
                        if let Some(last) = out.last_mut() {
                            if n == b'+' {
                                last.min = 1;
                                last.max = usize::MAX;
                            } else {
                                last.min = 0;
                                last.max = 1;
                            }
                        }
                        continue;
                    }
                    b't' => Atom::Ch(b'\t'),
                    b'n' => Atom::Ch(b'\n'),
                    b'd' => Atom::Class(vec![(b'0', b'9')], false),
                    b's' => Atom::Class(vec![(b' ', b' '), (b'\t', b'\t')], false),
                    b'w' => Atom::Class(vec![(b'a', b'z'), (b'A', b'Z'), (b'0', b'9'), (b'_', b'_')], false),
                    _ => Atom::Ch(n),
                }
            }
            b'*' if !out.is_empty() => {
                i += 1;
                if let Some(last) = out.last_mut() {
                    last.min = 0;
                    last.max = usize::MAX;
                }
                continue;
            }
            b'+' | b'?' if ere && !out.is_empty() => {
                i += 1;
                if let Some(last) = out.last_mut() {
                    if c == b'+' {
                        last.min = 1;
                        last.max = usize::MAX;
                    } else {
                        last.min = 0;
                        last.max = 1;
                    }
                }
                continue;
            }
            b'(' | b')' if ere => {
                i += 1;
                continue;
            }
            _ => {
                i += 1;
                Atom::Ch(c)
            }
        };
        out.push(Piece { atom, min: 1, max: 1 });
    }
    out
}

fn atom_matches(a: &Atom, c: u8, icase: bool) -> bool {
    let fold = |x: u8| if icase { x.to_ascii_lowercase() } else { x };
    match a {
        Atom::Ch(x) => fold(*x) == fold(c),
        Atom::Any => c != b'\n',
        Atom::Class(set, neg) => {
            let hit = set.iter().any(|(lo, hi)| {
                (c >= *lo && c <= *hi) || (icase && ((fold(c) >= fold(*lo) && fold(c) <= fold(*hi)) || (c.to_ascii_uppercase() >= *lo && c.to_ascii_uppercase() <= *hi)))
            });
            hit != *neg
        }
        Atom::Start | Atom::End => false,
    }
}

fn match_here(p: &[Piece], s: &[u8], i: usize, icase: bool) -> Option<usize> {
    let Some(first) = p.first() else { return Some(i) };
    match first.atom {
        Atom::Start => return if i == 0 { match_here(&p[1..], s, i, icase) } else { None },
        Atom::End => return if i == s.len() { match_here(&p[1..], s, i, icase) } else { None },
        _ => {}
    }
    let mut n = 0;
    while n < first.max && i + n < s.len() && atom_matches(&first.atom, s[i + n], icase) {
        n += 1;
    }
    if n < first.min {
        return None;
    }
    let mut k = n;
    loop {
        if let Some(e) = match_here(&p[1..], s, i + k, icase) {
            return Some(e);
        }
        if k == first.min {
            return None;
        }
        k -= 1;
    }
}

fn re_find(alts: &[Vec<Piece>], s: &[u8], from: usize, icase: bool) -> Option<(usize, usize)> {
    for start in from..=s.len() {
        let mut best: Option<usize> = None;
        for a in alts {
            if let Some(e) = match_here(a, s, start, icase) {
                best = Some(best.map_or(e, |b: usize| b.max(e)));
            }
        }
        if let Some(e) = best {
            return Some((start, e));
        }
    }
    None
}

fn grep_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let mut pats: Vec<Vec<u8>> = Vec::new();
    let mut files = Vec::new();
    let (mut count, mut invert, mut quiet, mut number, mut icase, mut list, mut ere, mut fixed, mut whole_line, mut only) =
        (false, false, false, false, false, false, false, false, false, false);
    let mut no_name = false;
    let mut i = 1;
    let mut opts_done = false;
    while i < a.len() {
        let x = &a[i];
        if !opts_done && x == b"--" {
            opts_done = true;
        } else if !opts_done && x == b"-e" {
            if let Some(p) = a.get(i + 1) {
                pats.push(p.clone());
            }
            i += 1;
        } else if !opts_done && x.len() > 1 && x[0] == b'-' && x[1] != b'-' {
            for &c in &x[1..] {
                match c {
                    b'c' => count = true,
                    b'v' => invert = true,
                    b'q' => quiet = true,
                    b'n' => number = true,
                    b'i' => icase = true,
                    b'l' => list = true,
                    b'E' => ere = true,
                    b'F' => fixed = true,
                    b'x' => whole_line = true,
                    b'o' => only = true,
                    b'h' => no_name = true,
                    _ => {}
                }
            }
        } else if !opts_done && x.starts_with(b"--") {
            match x.as_slice() {
                b"--count" => count = true,
                b"--invert-match" => invert = true,
                b"--quiet" => quiet = true,
                _ => {}
            }
        } else if pats.is_empty() {
            pats.push(x.clone());
        } else {
            files.push(x.clone());
        }
        i += 1;
    }
    let mut alts = Vec::new();
    for p in &pats {
        alts.extend(compile_re(p, ere, fixed));
    }
    if whole_line {
        for alt in &mut alts {
            alt.insert(0, Piece { atom: Atom::Start, min: 1, max: 1 });
            alt.push(Piece { atom: Atom::End, min: 1, max: 1 });
        }
    }
    if files.is_empty() {
        files.push(b"-".to_vec());
    }
    let show_name = files.len() > 1 && !no_name;
    let mut any = false;
    let mut err_st = false;
    for f in &files {
        let d = match read_input(f) {
            Ok(d) => d,
            Err(e) => {
                errs(&format!("grep: {}: {}\n", lossy(f), e.message()));
                err_st = true;
                continue;
            }
        };
        let label = if f == b"-" { b"(standard input)".to_vec() } else { f.clone() };
        let mut n = 0;
        for (k, line) in split_lines(&d).into_iter().enumerate() {
            let m = re_find(&alts, line, 0, icase);
            if m.is_some() != invert {
                n += 1;
                any = true;
                if quiet {
                    return 0;
                }
                if count || list {
                    continue;
                }
                let mut prefix = Vec::new();
                if show_name {
                    prefix.extend_from_slice(&label);
                    prefix.push(b':');
                }
                if number {
                    prefix.extend_from_slice(format!("{}:", k + 1).as_bytes());
                }
                if only && !invert {
                    let mut from = 0;
                    while let Some((s, e)) = re_find(&alts, line, from, icase) {
                        if e > s {
                            out(&prefix);
                            out(&line[s..e]);
                            out(b"\n");
                        }
                        from = if e > s { e } else { s + 1 };
                        if from > line.len() {
                            break;
                        }
                    }
                } else {
                    out(&prefix);
                    out(line);
                    out(b"\n");
                }
            }
        }
        if count {
            if show_name {
                out(&label);
                out(b":");
            }
            out(format!("{n}\n").as_bytes());
        }
        if list && n > 0 {
            out(&label);
            out(b"\n");
        }
    }
    if err_st {
        2
    } else if any {
        0
    } else {
        1
    }
}

// ---- sed mínimo: s///, p, d, q com endereços N, $, /re/ e faixas ----

#[derive(Clone, Debug)]
enum Addr {
    Line(usize),
    Last,
    Re(Vec<Vec<Piece>>),
}

#[derive(Clone, Debug)]
struct SedCmd {
    a1: Option<Addr>,
    a2: Option<Addr>,
    cmd: u8,
    re: Vec<Vec<Piece>>,
    rep: Vec<u8>,
    global: bool,
    print: bool,
    in_range: bool,
}

fn sed_parse(script: &[u8], ere: bool) -> Result<Vec<SedCmd>, String> {
    let mut cmds = Vec::new();
    let mut i = 0;
    let parse_addr = |i: &mut usize| -> Option<Addr> {
        let s = script;
        if *i < s.len() && s[*i].is_ascii_digit() {
            let st = *i;
            while *i < s.len() && s[*i].is_ascii_digit() {
                *i += 1;
            }
            return lossy(&s[st..*i]).parse().ok().map(Addr::Line);
        }
        if *i < s.len() && s[*i] == b'$' {
            *i += 1;
            return Some(Addr::Last);
        }
        if *i < s.len() && s[*i] == b'/' {
            *i += 1;
            let st = *i;
            while *i < s.len() && s[*i] != b'/' {
                if s[*i] == b'\\' {
                    *i += 1;
                }
                *i += 1;
            }
            let re = compile_re(&s[st..(*i).min(s.len())], ere, false);
            *i += 1;
            return Some(Addr::Re(re));
        }
        None
    };
    while i < script.len() {
        while i < script.len() && matches!(script[i], b' ' | b'\t' | b'\n' | b';') {
            i += 1;
        }
        if i >= script.len() {
            break;
        }
        let a1 = parse_addr(&mut i);
        let mut a2 = None;
        if a1.is_some() && script.get(i) == Some(&b',') {
            i += 1;
            a2 = parse_addr(&mut i);
        }
        while i < script.len() && script[i] == b' ' {
            i += 1;
        }
        let Some(&c) = script.get(i) else { return Err("missing command".into()) };
        i += 1;
        let mut cmd = SedCmd { a1, a2, cmd: c, re: Vec::new(), rep: Vec::new(), global: false, print: false, in_range: false };
        if c == b's' {
            let delim = *script.get(i).ok_or("unterminated `s' command")?;
            i += 1;
            let mut parts: Vec<Vec<u8>> = Vec::new();
            for _ in 0..2 {
                let mut v = Vec::new();
                while i < script.len() && script[i] != delim {
                    if script[i] == b'\\' && script.get(i + 1) == Some(&delim) {
                        v.push(delim);
                        i += 2;
                        continue;
                    }
                    if script[i] == b'\\' && i + 1 < script.len() {
                        v.push(script[i]);
                        v.push(script[i + 1]);
                        i += 2;
                        continue;
                    }
                    v.push(script[i]);
                    i += 1;
                }
                i += 1;
                parts.push(v);
            }
            cmd.re = compile_re(&parts[0], ere, false);
            cmd.rep = parts[1].clone();
            while i < script.len() && !matches!(script[i], b';' | b'\n' | b'}') {
                match script[i] {
                    b'g' => cmd.global = true,
                    b'p' => cmd.print = true,
                    _ => {}
                }
                i += 1;
            }
        }
        cmds.push(cmd);
    }
    Ok(cmds)
}

fn sed_addr_match(a: &Addr, line: &[u8], n: usize, last: bool) -> bool {
    match a {
        Addr::Line(k) => *k == n,
        Addr::Last => last,
        Addr::Re(re) => re_find(re, line, 0, false).is_some(),
    }
}

fn sed_run(cmds: &mut [SedCmd], data: &[u8], quiet: bool) -> Vec<u8> {
    let lines = split_lines(data);
    let total = lines.len();
    let mut outv = Vec::new();
    'lines: for (k, l) in lines.iter().enumerate() {
        let n = k + 1;
        let last = n == total;
        let mut line = l.to_vec();
        let mut deleted = false;
        for c in cmds.iter_mut() {
            let selected = match (&c.a1, &c.a2) {
                (None, _) => true,
                (Some(a), None) => sed_addr_match(a, &line, n, last),
                (Some(a), Some(b)) => {
                    if c.in_range {
                        if sed_addr_match(b, &line, n, last) || matches!(b, Addr::Line(e) if *e <= n) {
                            c.in_range = false;
                        }
                        true
                    } else if sed_addr_match(a, &line, n, last) {
                        c.in_range = !matches!(b, Addr::Line(e) if *e <= n);
                        true
                    } else {
                        false
                    }
                }
            };
            if !selected {
                continue;
            }
            match c.cmd {
                b'p' => {
                    outv.extend_from_slice(&line);
                    outv.push(b'\n');
                }
                b'd' => {
                    deleted = true;
                    break;
                }
                b'q' => {
                    if !quiet {
                        outv.extend_from_slice(&line);
                        outv.push(b'\n');
                    }
                    break 'lines;
                }
                b's' => {
                    let mut res = Vec::new();
                    let mut from = 0;
                    let mut changed = false;
                    while from <= line.len() {
                        let Some((s, e)) = re_find(&c.re, &line, from, false) else { break };
                        res.extend_from_slice(&line[from..s]);
                        let mut j = 0;
                        while j < c.rep.len() {
                            match c.rep[j] {
                                b'&' => res.extend_from_slice(&line[s..e]),
                                b'\\' if j + 1 < c.rep.len() => {
                                    j += 1;
                                    match c.rep[j] {
                                        b'n' => res.push(b'\n'),
                                        b't' => res.push(b'\t'),
                                        x => res.push(x),
                                    }
                                }
                                x => res.push(x),
                            }
                            j += 1;
                        }
                        changed = true;
                        if e == s {
                            if s < line.len() {
                                res.push(line[s]);
                            }
                            from = s + 1;
                        } else {
                            from = e;
                        }
                        if !c.global {
                            break;
                        }
                    }
                    if from <= line.len() {
                        res.extend_from_slice(&line[from.min(line.len())..]);
                    }
                    if changed {
                        line = res;
                        if c.print {
                            outv.extend_from_slice(&line);
                            outv.push(b'\n');
                        }
                    }
                }
                _ => {}
            }
        }
        if !deleted && !quiet {
            outv.extend_from_slice(&line);
            outv.push(b'\n');
        }
    }
    outv
}

fn sed_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let mut quiet = false;
    let mut inplace = false;
    let mut ere = false;
    let mut scripts: Vec<Vec<u8>> = Vec::new();
    let mut files = Vec::new();
    let mut i = 1;
    while i < a.len() {
        let x = &a[i];
        if x == b"-e" {
            if let Some(s) = a.get(i + 1) {
                scripts.push(s.clone());
            }
            i += 2;
            continue;
        }
        if x.len() > 1 && x[0] == b'-' && x[1] != b'-' {
            for &c in &x[1..] {
                match c {
                    b'n' => quiet = true,
                    b'i' => inplace = true,
                    b'E' | b'r' => ere = true,
                    _ => {}
                }
            }
        } else if x == b"--quiet" {
            quiet = true;
        } else if scripts.is_empty() {
            scripts.push(x.clone());
        } else {
            files.push(x.clone());
        }
        i += 1;
    }
    let script = scripts.join(&b'\n');
    let mut cmds = match sed_parse(&script, ere) {
        Ok(c) => c,
        Err(e) => {
            errs(&format!("sed: -e expression #1, char 1: {e}\n"));
            return 1;
        }
    };
    if files.is_empty() {
        files.push(b"-".to_vec());
    }
    let mut st = 0;
    if inplace {
        for f in &files {
            match read_input(f) {
                Ok(d) => {
                    for c in cmds.iter_mut() {
                        c.in_range = false;
                    }
                    let r = sed_run(&mut cmds, &d, quiet);
                    if let Err(e) = write_file(f, &r, OFlags::TRUNC) {
                        errs(&format!("sed: couldn't open file {}: {}\n", lossy(f), e.message()));
                        st = 4;
                    }
                }
                Err(e) => {
                    errs(&format!("sed: can't read {}: {}\n", lossy(f), e.message()));
                    st = 2;
                }
            }
        }
        return st;
    }
    let mut data = Vec::new();
    for f in &files {
        match read_input(f) {
            Ok(d) => data.extend(d),
            Err(e) => {
                errs(&format!("sed: can't read {}: {}\n", lossy(f), e.message()));
                st = 2;
            }
        }
    }
    out(&sed_run(&mut cmds, &data, quiet));
    st
}

// ---- outros ----

fn chmod_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let mut ops: Vec<Vec<u8>> = a[1..].iter().filter(|x| x.as_slice() != b"-R").cloned().collect();
    if ops.len() < 2 {
        errs("chmod: missing operand\n");
        return 1;
    }
    let mode = ops.remove(0);
    let s = sysc();
    let mut st = 0;
    for p in ops {
        let cur = match stat(&p) {
            Ok(x) => x.mode & 0o7777,
            Err(e) => {
                errs(&format!("chmod: cannot access '{}': {}\n", lossy(&p), e.message()));
                st = 1;
                continue;
            }
        };
        let new = if mode.iter().all(|c| (b'0'..=b'7').contains(c)) {
            u32::from_str_radix(&lossy(&mode), 8).unwrap_or(cur)
        } else {
            let mut m = cur;
            for clause in lossy(&mode).split(',') {
                let who_end = clause.find(|c: char| matches!(c, '+' | '-' | '=')).unwrap_or(clause.len());
                let who = &clause[..who_end];
                let Some(op) = clause[who_end..].chars().next() else { continue };
                let perms = &clause[who_end + 1..];
                let mut bits = 0u32;
                for c in perms.chars() {
                    bits |= match c {
                        'r' => 0o444,
                        'w' => 0o222,
                        'x' => 0o111,
                        _ => 0,
                    };
                }
                let mask = if who.is_empty() || who.contains('a') {
                    if who.is_empty() && op != '-' { 0o777 & !current_umask() } else { 0o777 }
                } else {
                    let mut k = 0;
                    if who.contains('u') {
                        k |= 0o700;
                    }
                    if who.contains('g') {
                        k |= 0o070;
                    }
                    if who.contains('o') {
                        k |= 0o007;
                    }
                    k
                };
                match op {
                    '+' => m |= bits & mask,
                    '-' => m &= !(bits & mask),
                    _ => m = (m & !mask) | (bits & mask),
                }
            }
            m
        };
        if let Err(e) = s.fchmodat(Fd::CWD, &p, new, AtFlags::empty()) {
            errs(&format!("chmod: changing permissions of '{}': {}\n", lossy(&p), e.message()));
            st = 1;
        }
    }
    st
}

fn ln_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let (flags, ops) = split_flags(&a[1..]);
    if ops.len() != 2 {
        errs("ln: missing file operand\n");
        return 1;
    }
    let s = sysc();
    let (target, mut link) = (ops[0].clone(), ops[1].clone());
    if is_dir(&link) {
        link = join_path(&link, base_name(&target));
    }
    if flags.contains(&b'f') {
        let _ = s.unlinkat(Fd::CWD, &link, AtFlags::empty());
    }
    let r = if flags.contains(&b's') { s.symlinkat(&target, Fd::CWD, &link) } else { s.linkat(Fd::CWD, &target, Fd::CWD, &link, AtFlags::empty()) };
    match r {
        Ok(()) => 0,
        Err(e) => {
            errs(&format!("ln: failed to create link '{}': {}\n", lossy(&link), e.message()));
            1
        }
    }
}

fn pwd_main(_ctx: &mut Ctx, _args: &[OsString]) -> i32 {
    let cwd = sysc().getcwd().unwrap_or_default();
    out(&cwd);
    out(b"\n");
    0
}

fn env_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let s = sysc();
    let mut env = s.environ();
    let mut i = 1;
    while i < a.len() {
        let x = &a[i];
        if x == b"-i" {
            env.clear();
        } else if x == b"-u" {
            if let Some(n) = a.get(i + 1) {
                let mut p = n.clone();
                p.push(b'=');
                env.retain(|kv| !kv.starts_with(&p));
            }
            i += 1;
        } else if let Some(eq) = x.iter().position(|c| *c == b'=') {
            let p = x[..=eq].to_vec();
            env.retain(|kv| !kv.starts_with(&p));
            env.push(x.clone());
        } else {
            break;
        }
        i += 1;
    }
    if i >= a.len() {
        for kv in env {
            out(&kv);
            out(b"\n");
        }
        return 0;
    }
    let cmd = &a[i..];
    let path_var = env.iter().find_map(|kv| kv.strip_prefix(b"PATH=").map(|v| v.to_vec())).unwrap_or_else(|| b"/usr/bin:/bin".to_vec());
    let mut path = None;
    if cmd[0].contains(&b'/') {
        path = Some(cmd[0].clone());
    } else {
        for d in path_var.split(|c| *c == b':') {
            let c = join_path(d, &cmd[0]);
            if s.faccessat(Fd::CWD, &c, AccessMode::X_OK, AtFlags::empty()).is_ok() && !is_dir(&c) {
                path = Some(c);
                break;
            }
        }
    }
    let Some(path) = path else {
        errs(&format!("env: \u{2018}{}\u{2019}: No such file or directory\n", lossy(&cmd[0])));
        return 127;
    };
    match s.spawn(SpawnSpec { path, argv: cmd.to_vec(), attrs: ProcAttrs { env: Some(env), ..ProcAttrs::default() } }) {
        Ok(pid) => wait_pid(&*s, pid),
        Err(e) => {
            errs(&format!("env: \u{2018}{}\u{2019}: {}\n", lossy(&cmd[0]), e.message()));
            126
        }
    }
}

fn tr_set(s: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    let mut i = 0;
    let mut raw = Vec::new();
    while i < s.len() {
        if s[i] == b'\\' && i + 1 < s.len() {
            raw.push(match s[i + 1] {
                b'n' => b'\n',
                b't' => b'\t',
                b'\\' => b'\\',
                c => c,
            });
            i += 2;
        } else if s[i..].starts_with(b"[:upper:]") {
            raw.extend(b'A'..=b'Z');
            i += 9;
        } else if s[i..].starts_with(b"[:lower:]") {
            raw.extend(b'a'..=b'z');
            i += 9;
        } else if s[i..].starts_with(b"[:space:]") {
            raw.extend_from_slice(b" \t\n\r\x0b\x0c");
            i += 9;
        } else if s[i..].starts_with(b"[:digit:]") {
            raw.extend(b'0'..=b'9');
            i += 9;
        } else {
            raw.push(s[i]);
            i += 1;
        }
    }
    let mut k = 0;
    while k < raw.len() {
        if k + 2 < raw.len() && raw[k + 1] == b'-' {
            v.extend(raw[k]..=raw[k + 2]);
            k += 3;
        } else {
            v.push(raw[k]);
            k += 1;
        }
    }
    v
}

fn tr_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let (flags, ops) = split_flags(&a[1..]);
    let data = read_all_fd(Fd::STDIN);
    let s1 = tr_set(ops.first().map(|v| v.as_slice()).unwrap_or(b""));
    if flags.contains(&b'd') {
        let v: Vec<u8> = data.into_iter().filter(|c| !s1.contains(c)).collect();
        out(&v);
        return 0;
    }
    let s2 = tr_set(ops.get(1).map(|v| v.as_slice()).unwrap_or(b""));
    let v: Vec<u8> = data
        .into_iter()
        .map(|c| match s1.iter().rposition(|x| *x == c) {
            Some(k) => s2.get(k).or(s2.last()).copied().unwrap_or(c),
            None => c,
        })
        .collect();
    out(&v);
    0
}

fn xargs_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let mut i = 1;
    while i < a.len() && a[i].starts_with(b"-") {
        i += 1;
    }
    let mut cmd: Vec<Vec<u8>> = a[i..].to_vec();
    if cmd.is_empty() {
        cmd.push(b"echo".to_vec());
    }
    let data = read_all_fd(Fd::STDIN);
    let items: Vec<Vec<u8>> = data.split(|c| c.is_ascii_whitespace()).filter(|x| !x.is_empty()).map(|x| x.to_vec()).collect();
    if items.is_empty() {
        return 0;
    }
    cmd.extend(items);
    let s = sysc();
    let path_var = s.getenv(b"PATH").unwrap_or_else(|| b"/usr/bin:/bin".to_vec());
    let mut path = cmd[0].clone();
    if !path.contains(&b'/') {
        for d in path_var.split(|c| *c == b':') {
            let c = join_path(d, &cmd[0]);
            if s.faccessat(Fd::CWD, &c, AccessMode::X_OK, AtFlags::empty()).is_ok() {
                path = c;
                break;
            }
        }
    }
    match s.spawn(SpawnSpec { path, argv: cmd.clone(), attrs: ProcAttrs::default() }) {
        Ok(pid) => {
            let st = wait_pid(&*s, pid);
            if st != 0 { 123 } else { 0 }
        }
        Err(_) => {
            errs(&format!("xargs: {}: No such file or directory\n", lossy(&cmd[0])));
            127
        }
    }
}

fn basename_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let Some(p) = a.get(1) else { return 1 };
    let mut b = base_name(p).to_vec();
    if let Some(suf) = a.get(2)
        && b.ends_with(suf)
        && b.len() > suf.len()
    {
        b.truncate(b.len() - suf.len());
    }
    out(&b);
    out(b"\n");
    0
}

fn dirname_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let Some(p) = a.get(1) else { return 1 };
    let t = p.strip_suffix(b"/").unwrap_or(p);
    let d: &[u8] = match t.iter().rposition(|c| *c == b'/') {
        Some(0) => b"/",
        Some(i) => &t[..i],
        None => b".",
    };
    out(d);
    out(b"\n");
    0
}

fn seq_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let a = to_args(args);
    let nums: Vec<i64> = a[1..].iter().filter_map(|x| lossy(x).parse().ok()).collect();
    let (first, step, last) = match nums.as_slice() {
        [l] => (1, 1, *l),
        [f, l] => (*f, 1, *l),
        [f, s, l] => (*f, *s, *l),
        _ => return 1,
    };
    let mut x = first;
    while (step > 0 && x <= last) || (step < 0 && x >= last) {
        out(format!("{x}\n").as_bytes());
        x += step;
        if step == 0 {
            break;
        }
    }
    0
}
