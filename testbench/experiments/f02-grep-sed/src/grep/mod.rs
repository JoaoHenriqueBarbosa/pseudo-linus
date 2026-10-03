//! grep montado: front-end de flags nosso ([`args`]), `grep-searcher` pra varrer as linhas, casador
//! ([`matcher`]: motor do F01 ou `grep-regex`) e um de dois printers: o nosso no formato do GNU, ou o
//! `grep-printer` do ripgrep configurado o mais perto possível do GNU.
//!
//! Arquivos vêm da árvore do caso em memória ([`crate::fsview`]); nada toca o disco do host.

pub mod args;
pub mod matcher;

use std::io::Write;

use grep_printer::{StandardBuilder, SummaryBuilder, SummaryKind};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkContext, SinkMatch};
use harness::{Invocation, Outcome};

use crate::fsview::{Errno, FsView, Kind};
use args::{BinaryMode, DirMode, GrepOpts, ListMode};
use matcher::{BuildError, LineMatcher, MatcherKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrinterKind {
    /// Printer nosso, no formato do GNU grep.
    Gnu,
    /// `grep-printer` (Standard e Summary) do ripgrep.
    Ripgrep,
}

pub struct GrepImpl {
    pub name: String,
    pub matcher: MatcherKind,
    pub printer: PrinterKind,
}

struct Out {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    any_selected: bool,
    error: bool,
    /// (arquivo, última linha coberta) pra decidir o separador `--` do contexto.
    last: Option<(usize, u64)>,
    pending_gap: bool,
    printed_any: bool,
    file_idx: usize,
    quit: bool,
}

impl Out {
    fn err(&mut self, opts: &GrepOpts, msg: &str) {
        self.error = true;
        if !opts.no_messages {
            self.stderr.extend_from_slice(format!("grep: {msg}\n").as_bytes());
        }
    }
}

fn glob_match(pat: &[char], text: &[char]) -> bool {
    match (pat.first(), text.first()) {
        (None, None) => true,
        (Some('*'), _) => glob_match(&pat[1..], text) || (!text.is_empty() && glob_match(pat, &text[1..])),
        (Some('?'), Some(_)) => glob_match(&pat[1..], &text[1..]),
        (Some('['), Some(&c)) => {
            let Some(close) = pat.iter().skip(2).position(|&x| x == ']').map(|p| p + 2) else {
                return c == '[' && glob_match(&pat[1..], &text[1..]);
            };
            let body = &pat[1..close];
            let (neg, body) = if matches!(body.first(), Some('!') | Some('^')) { (true, &body[1..]) } else { (false, body) };
            let mut hit = false;
            let mut i = 0;
            while i < body.len() {
                if i + 2 < body.len() && body[i + 1] == '-' {
                    hit |= body[i] <= c && c <= body[i + 2];
                    i += 3;
                } else {
                    hit |= body[i] == c;
                    i += 1;
                }
            }
            hit != neg && glob_match(&pat[close + 1..], &text[1..])
        }
        (Some('\\'), _) if pat.len() > 1 => text.first() == Some(&pat[1]) && glob_match(&pat[2..], &text[1..]),
        (Some(p), Some(t)) => p == t && glob_match(&pat[1..], &text[1..]),
        _ => false,
    }
}

fn glob(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = name.chars().collect();
    glob_match(&p, &t)
}

fn basename(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path)
}

/// `--include`/`--exclude`: o último que casar decide; sem `--include` casando, inclui se não houver
/// nenhum `--include`.
fn file_selected(opts: &GrepOpts, path: &str) -> bool {
    let name = basename(path);
    if opts.excludes.iter().any(|g| glob(g, name)) {
        return false;
    }
    opts.includes.is_empty() || opts.includes.iter().any(|g| glob(g, name))
}

fn is_binary(data: &[u8]) -> bool {
    data.contains(&0) || std::str::from_utf8(data).is_err()
}

fn strip_eol(b: &[u8]) -> &[u8] {
    b.strip_suffix(b"\n").unwrap_or(b)
}

/// Sink no formato do GNU. Faz o `-m` ele mesmo: depois da m-ésima linha selecionada, as próximas
/// `-A` linhas saem como contexto, casando ou não (é o que o GNU faz; o `max_matches` do
/// grep-searcher continua reportando casadas).
struct GnuSink<'a> {
    out: &'a mut Out,
    opts: &'a GrepOpts,
    matcher: &'a LineMatcher,
    name: Option<&'a str>,
    count: u64,
    /// Última linha que ainda sai depois de atingir o `-m`.
    limit_line: Option<u64>,
}

impl GnuSink<'_> {
    fn context_enabled(&self) -> bool {
        self.opts.after > 0 || self.opts.before > 0
    }

    /// Marca a linha como coberta (impressa ou não); um buraco desde a última vira separador pendente.
    fn cover(&mut self, line: u64) {
        if let Some((f, l)) = self.out.last
            && (f != self.out.file_idx || line != l + 1)
        {
            self.out.pending_gap = true;
        }
        self.out.last = Some((self.out.file_idx, line));
    }

    /// Antes de imprimir uma linha: o separador de grupo, se houve buraco desde a última impressa.
    fn begin_output(&mut self) {
        if self.context_enabled()
            && self.out.pending_gap
            && self.out.printed_any
            && let Some(sep) = &self.opts.group_separator
        {
            self.out.stdout.extend_from_slice(sep.as_bytes());
            self.out.stdout.push(b'\n');
        }
        self.out.pending_gap = false;
        self.out.printed_any = true;
    }

    /// `selected`: linha selecionada (casa, ou não casa com `-v`); senão é contexto.
    fn line(&mut self, ln: u64, off: u64, bytes: &[u8], mut selected: bool) -> Result<bool, std::io::Error> {
        // O searcher pode reentregar como contexto uma linha que já saiu depois do `-m`.
        if self.out.last.is_some_and(|(f, l)| f == self.out.file_idx && ln <= l) {
            return Ok(self.limit_line.is_none_or(|limit| ln < limit));
        }
        if let Some(limit) = self.limit_line {
            if ln > limit {
                return Ok(false);
            }
            selected = false;
        }
        if selected {
            self.count += 1;
            self.out.any_selected = true;
            if self.opts.max_count == Some(self.count) {
                self.limit_line = Some(ln + self.opts.after as u64);
            }
        }
        self.cover(ln);
        let line = strip_eol(bytes);
        let sep = if selected { b':' } else { b'-' };
        if self.opts.only {
            // `-o` imprime as casadas: das linhas selecionadas sem `-v`, e (quirk do GNU) das linhas
            // de contexto com `-v`, que são justamente as que casam.
            if selected != self.opts.invert {
                let spans = self.matcher.only_matching(line).map_err(|e| std::io::Error::other(e.0))?;
                for (s, e) in spans {
                    self.begin_output();
                    self.prefix(ln, off + s as u64, sep);
                    self.out.stdout.extend_from_slice(&line[s..e]);
                    self.out.stdout.push(b'\n');
                }
            }
        } else {
            self.begin_output();
            self.prefix(ln, off, sep);
            self.out.stdout.extend_from_slice(line);
            self.out.stdout.push(b'\n');
        }
        let done = self.limit_line.is_some_and(|l| ln >= l);
        Ok(!done)
    }

    fn prefix(&mut self, line: u64, offset: u64, sep: u8) {
        let o = &mut self.out.stdout;
        let mut any = false;
        if let Some(n) = self.name {
            o.extend_from_slice(n.as_bytes());
            o.push(if self.opts.null { 0 } else { sep });
            any = true;
        }
        if self.opts.line_number {
            o.extend_from_slice(line.to_string().as_bytes());
            o.push(sep);
            any = true;
        }
        if self.opts.byte_offset {
            o.extend_from_slice(offset.to_string().as_bytes());
            o.push(sep);
            any = true;
        }
        if self.opts.initial_tab && any {
            o.push(b'\t');
        }
    }
}

impl Sink for GnuSink<'_> {
    type Error = std::io::Error;

    fn matched(&mut self, _s: &Searcher, m: &SinkMatch<'_>) -> Result<bool, std::io::Error> {
        self.line(m.line_number().unwrap_or(0), m.absolute_byte_offset(), m.bytes(), true)
    }

    fn context(&mut self, _s: &Searcher, c: &SinkContext<'_>) -> Result<bool, std::io::Error> {
        self.line(c.line_number().unwrap_or(0), c.absolute_byte_offset(), c.bytes(), false)
    }
}

/// O que a varredura de arquivos precisa carregar.
struct Walk<'a> {
    opts: &'a GrepOpts,
    m: &'a LineMatcher,
    fs: &'a FsView<'a>,
    with_name: bool,
}

/// Sink que só conta linhas selecionadas.
struct CountSink(u64);

impl Sink for CountSink {
    type Error = std::io::Error;
    fn matched(&mut self, _s: &Searcher, _m: &SinkMatch<'_>) -> Result<bool, std::io::Error> {
        self.0 += 1;
        Ok(true)
    }
}

impl GrepImpl {
    fn searcher(&self, opts: &GrepOpts, context: bool) -> Searcher {
        self.searcher_with(opts, context, true)
    }

    fn searcher_with(&self, opts: &GrepOpts, context: bool, max: bool) -> Searcher {
        SearcherBuilder::new()
            .line_number(self.printer == PrinterKind::Gnu || opts.line_number)
            .invert_match(opts.invert)
            .after_context(if context { opts.after } else { 0 })
            .before_context(if context { opts.before } else { 0 })
            .max_matches(if max { opts.max_count } else { None })
            .binary_detection(BinaryDetection::none())
            .build()
    }

    /// `name` é o prefixo impresso (quando há); `label` é o nome do arquivo pras mensagens e pro `-l`.
    fn search(&self, out: &mut Out, opts: &GrepOpts, m: &LineMatcher, name: Option<&str>, label: &str, data: &[u8]) -> Result<(), String> {
        let binary = opts.binary != BinaryMode::Text && is_binary(data);
        if binary && opts.binary == BinaryMode::WithoutMatch {
            if opts.list == Some(ListMode::WithoutMatch) {
                self.print_name(out, opts, label);
            }
            return Ok(());
        }
        let summary = opts.count || opts.list.is_some() || opts.quiet;
        // Binário em modo normal: o GNU não imprime as linhas, só avisa no stderr.
        if summary || binary || m.never() {
            let mut sink = CountSink(0);
            if !m.never() {
                self.searcher(opts, false).search_slice(m, data, &mut sink).map_err(|e| e.to_string())?;
            }
            let n = sink.0;
            if n > 0 {
                out.any_selected = true;
            }
            if opts.quiet {
                out.quit |= n > 0;
                return Ok(());
            }
            if self.printer == PrinterKind::Ripgrep && summary {
                return self.ripgrep_summary(out, opts, m, name, label, data);
            }
            match opts.list {
                Some(ListMode::WithMatch) if n > 0 => self.print_name(out, opts, label),
                Some(ListMode::WithoutMatch) if n == 0 => self.print_name(out, opts, label),
                Some(_) => {}
                None if opts.count => {
                    if let Some(n) = name {
                        out.stdout.extend_from_slice(n.as_bytes());
                        out.stdout.push(if opts.null { 0 } else { b':' });
                    }
                    out.stdout.extend_from_slice(format!("{n}\n").as_bytes());
                }
                None => {
                    if n > 0 {
                        out.stderr.extend_from_slice(format!("grep: {label}: binary file matches\n").as_bytes());
                    }
                }
            }
            return Ok(());
        }
        match self.printer {
            PrinterKind::Gnu => {
                let mut sink = GnuSink { out, opts, matcher: m, name, count: 0, limit_line: None };
                let mut searcher = self.searcher_with(opts, true, false);
                searcher.search_slice(m, data, &mut sink).map_err(|e| e.to_string())?;
            }
            PrinterKind::Ripgrep => {
                let mut counter = CountSink(0);
                self.searcher(opts, false).search_slice(m, data, &mut counter).map_err(|e| e.to_string())?;
                out.any_selected |= counter.0 > 0;
                let mut printer = StandardBuilder::new()
                    .only_matching(opts.only)
                    .byte_offset(opts.byte_offset)
                    .path_terminator(if opts.null { Some(0) } else { None })
                    .separator_context(opts.group_separator.as_ref().map(|s| s.as_bytes().to_vec()))
                    .build_no_color(Vec::new());
                let mut searcher = self.searcher(opts, true);
                let res = match name {
                    Some(n) => searcher.search_slice(m, data, printer.sink_with_path(m, n)),
                    None => searcher.search_slice(m, data, printer.sink(m)),
                };
                res.map_err(|e| e.to_string())?;
                out.stdout.extend_from_slice(&printer.into_inner().into_inner());
            }
        }
        Ok(())
    }

    fn ripgrep_summary(
        &self,
        out: &mut Out,
        opts: &GrepOpts,
        m: &LineMatcher,
        name: Option<&str>,
        label: &str,
        data: &[u8],
    ) -> Result<(), String> {
        let kind = match opts.list {
            Some(ListMode::WithMatch) => SummaryKind::PathWithMatch,
            Some(ListMode::WithoutMatch) => SummaryKind::PathWithoutMatch,
            None => SummaryKind::Count,
        };
        let mut printer = SummaryBuilder::new()
            .kind(kind)
            .exclude_zero(false)
            .path_terminator(if opts.null { Some(0) } else { None })
            .build_no_color(Vec::new());
        let mut searcher = self.searcher(opts, false);
        let res = if name.is_some() || opts.list.is_some() {
            searcher.search_slice(m, data, printer.sink_with_path(m, label))
        } else {
            searcher.search_slice(m, data, printer.sink(m))
        };
        res.map_err(|e| e.to_string())?;
        out.stdout.extend_from_slice(&printer.into_inner().into_inner());
        Ok(())
    }

    fn print_name(&self, out: &mut Out, opts: &GrepOpts, name: &str) {
        out.stdout.extend_from_slice(name.as_bytes());
        out.stdout.push(if opts.null { 0 } else { b'\n' });
    }

    fn visit(&self, w: &Walk<'_>, out: &mut Out, display: &str, key: &str) {
        let (opts, fs) = (w.opts, w.fs);
        for (child, is_link) in fs.list(key) {
            if out.quit {
                return;
            }
            let child_display = if display.is_empty() {
                child.clone()
            } else if display.ends_with('/') {
                format!("{display}{child}")
            } else {
                format!("{display}/{child}")
            };
            let child_key = if key.is_empty() { child.clone() } else { format!("{key}/{child}") };
            if is_link && opts.recursive != Some(true) {
                continue;
            }
            match fs.resolve(&child_key, true) {
                Ok((resolved, _, Kind::Dir)) => {
                    if opts.exclude_dirs.iter().any(|g| glob(g, &child)) {
                        continue;
                    }
                    self.visit(w, out, &child_display, &resolved);
                }
                Ok((_, _, Kind::File)) => {
                    if !file_selected(opts, &child) {
                        continue;
                    }
                    self.search_file(w, out, &child_display, &child_key);
                }
                Err(e) => {
                    let msg = format!("{child_display}: {}", e.message());
                    out.err(opts, &msg);
                }
            }
        }
    }

    fn search_file(&self, w: &Walk<'_>, out: &mut Out, display: &str, key: &str) {
        match w.fs.read(key) {
            Ok(data) => {
                out.file_idx += 1;
                let name = w.with_name.then_some(display);
                if let Err(e) = self.search(out, w.opts, w.m, name, display, data) {
                    out.err(w.opts, &format!("{display}: {e}"));
                }
            }
            Err(e) => out.err(w.opts, &format!("{display}: {}", e.message())),
        }
    }

    pub fn run(&self, inv: &Invocation) -> Outcome {
        let files = inv.files.clone();
        let fs = FsView::new(&inv.files);
        if inv.script.is_some() {
            return Outcome::unsupported("caso com script de shell");
        }
        let opts = match args::parse(inv.args(), &fs, &inv.stdin) {
            Ok(o) => o,
            Err(e) => return Outcome::exited(e.stdout, e.stderr, e.code, files),
        };
        if opts.null_data {
            return Outcome::unsupported("-z (linhas terminadas em NUL) fora do escopo");
        }
        let m = match LineMatcher::build(&opts, &self.matcher) {
            Ok(m) => m,
            Err(BuildError::Syntax(msg)) => return Outcome::exited("", format!("grep: {msg}\n"), 2, files),
            Err(BuildError::Unsupported(why)) => return Outcome::unsupported(why),
        };
        if opts.max_count == Some(0) {
            return Outcome::exited("", "", 1, files);
        }
        let mut out = Out {
            stdout: Vec::new(),
            stderr: Vec::new(),
            any_selected: false,
            error: false,
            last: None,
            pending_gap: false,
            printed_any: false,
            file_idx: 0,
            quit: false,
        };
        let recursive = opts.directories == DirMode::Recurse;
        let mut targets = opts.files.clone();
        let implicit_dot = targets.is_empty() && recursive;
        if targets.is_empty() {
            targets.push(if recursive { ".".into() } else { "-".into() });
        }
        let with_name = opts.with_filename.unwrap_or(targets.len() > 1 || recursive);
        let walk = Walk { opts: &opts, m: &m, fs: &fs, with_name };
        for t in &targets {
            if out.quit {
                break;
            }
            if t == "-" {
                out.file_idx += 1;
                let label = opts.label.clone().unwrap_or_else(|| "(standard input)".into());
                let name = with_name.then_some(label.as_str());
                if let Err(e) = self.search(&mut out, &opts, &m, name, &label, &inv.stdin) {
                    out.err(&opts, &format!("(standard input): {e}"));
                }
                continue;
            }
            match fs.resolve(t, true) {
                Err(Errno::NotFound) | Err(Errno::Loop) | Err(Errno::IsDir) => {
                    let e = fs.resolve(t, true).err().unwrap_or(Errno::NotFound);
                    out.err(&opts, &format!("{t}: {}", e.message()));
                }
                Ok((key, _, Kind::Dir)) => match opts.directories {
                    DirMode::Recurse => {
                        if opts.exclude_dirs.iter().any(|g| glob(g, basename(t))) && t != "." {
                            continue;
                        }
                        let display = if implicit_dot { String::new() } else { t.clone() };
                        self.visit(&walk, &mut out, &display, &key);
                    }
                    DirMode::Skip => {}
                    DirMode::Read => out.err(&opts, &format!("{t}: Is a directory")),
                },
                Ok((_, _, Kind::File)) => {
                    if !file_selected(&opts, t) {
                        continue;
                    }
                    self.search_file(&walk, &mut out, t, t);
                }
            }
        }
        let _ = out.stdout.flush();
        let code = if opts.quiet && out.any_selected {
            0
        } else if out.error {
            2
        } else if out.any_selected {
            0
        } else {
            1
        };
        Outcome::exited(out.stdout, out.stderr, code, files)
    }
}

impl harness::Candidate for GrepImpl {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        GrepImpl::run(self, inv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grep(args: &[&str], files: &[(&str, &str)], stdin: &str) -> Outcome {
        let mut tree = harness::MemTree::new();
        for (p, c) in files {
            tree.insert(p, harness::Entry::file(c.as_bytes().to_vec(), 0o644));
        }
        let inv = Invocation {
            case_id: "t".into(),
            argv: std::iter::once("grep").chain(args.iter().copied()).map(str::to_string).collect(),
            script: None,
            stdin: stdin.as_bytes().to_vec(),
            files: tree,
            env: Default::default(),
            faketime: None,
        };
        let g = GrepImpl {
            name: "t".into(),
            matcher: MatcherKind::Gnu(Box::new(f01_regex::engines::AutomataFerroni)),
            printer: PrinterKind::Gnu,
        };
        g.run(&inv)
    }

    fn text(o: &Outcome) -> String {
        String::from_utf8(o.stdout.0.clone()).unwrap()
    }

    #[test]
    fn gnu_semantics_end_to_end() {
        // -w do GNU: casada mais curta no mesmo início, depois o próximo início (saída do GNU grep 3.11).
        let o = grep(&["-ow", "ab*", "in"], &[("in", "abbbx ab a abb\n")], "");
        assert_eq!(text(&o), "ab\na\nabb\n");
        // leftmost-longest no -o.
        assert_eq!(text(&grep(&["-oE", "ab|abcd", "in"], &[("in", "abcd ab\n")], "")), "abcd\nab\n");
        // -m com contexto: depois do limite, as linhas seguintes saem como contexto.
        let o = grep(&["-m1", "-A2", "x", "in"], &[("in", "x1\n2\nx3\n4\n")], "");
        assert_eq!(text(&o), "x1\n2\nx3\n");
        // vários arquivos, erro e exit 2.
        let o = grep(&["E", "nope", "a"], &[("a", "E\n")], "");
        assert_eq!((text(&o), o.exit), ("a:E\n".to_string(), Some(2)));
        assert_eq!(String::from_utf8(o.stderr.0).unwrap(), "grep: nope: No such file or directory\n");
        // stdin e -c.
        assert_eq!(text(&grep(&["-c", "b"], &[], "a\nb\nab\n")), "2\n");
    }

    #[test]
    fn globs() {
        assert!(glob("*.rs", "main.rs"));
        assert!(!glob("*.rs", "main.py"));
        assert!(glob("[ab]?.txt", "ax.txt"));
        assert!(glob("target", "target"));
    }
}
