//! grep (GNU grep 3.11), egrep e fgrep (os scripts do Debian que chamam `grep -E`/`grep -F`).
//!
//! - [`matcher`]: casamento de linhas sobre o regex-posix (`EGexecute`).
//! - [`search`]: varredura de um arquivo, contexto e saída (`grep()` do `grep.c`).
//! - aqui: opções (`main` do `grep.c`), padrões, travessia de diretórios (`fts`) e códigos de saída.

mod glob;
pub mod matcher;
pub mod search;

use std::collections::HashSet;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;

use regex_posix::{RegexBuilder, Syntax};
use sysabi::{AtFlags, Ctx, Errno, Fd, FileType, OFlags, Stat, sys};
use ul_common::getopt::{Getopt, HasArg, LongOpt};

use crate::io::{Out, errno_msg, error, read_all};
use glob::Excludes;
use matcher::{LineMatcher, MatcherSpec, Mode};
use search::{BinaryFiles, Searcher};

const USAGE_SHORT: &str = "Usage: grep [OPTION]... PATTERNS [FILE]...\nTry 'grep --help' for more information.\n";

const VERSION: &str = "grep (GNU grep) 3.11
Copyright (C) 2023 Free Software Foundation, Inc.
License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>.
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.

Written by Mike Haertel and others; see
<https://git.savannah.gnu.org/cgit/grep.git/tree/AUTHORS>.

grep -P uses PCRE2 10.46 2025-08-27
";

const HELP: &str = include_str!("help.txt");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListFiles {
    None,
    Matching,
    NonMatching,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Directories {
    Read,
    Recurse,
    Skip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Devices {
    ReadCommandLine,
    Read,
    Skip,
}

/// Opções já resolvidas (as variáveis globais do `grep.c`).
pub struct Opts {
    pub eol: u8,
    pub invert: bool,
    pub only_matching: bool,
    pub line_number: bool,
    pub byte_offset: bool,
    pub initial_tab: bool,
    pub null_after_name: bool,
    pub line_buffered: bool,
    pub count_matches: bool,
    pub list_files: ListFiles,
    pub suppress_errors: bool,
    pub binary_files: BinaryFiles,
    pub max_count: i64,
    pub before: i64,
    pub after: i64,
    pub group_separator: Option<Vec<u8>>,
    pub out_quiet: bool,
    pub done_on_match: bool,
    pub exit_on_match: bool,
    pub directories: Directories,
    pub devices: Devices,
    pub logical: bool,
    pub label: Option<Vec<u8>>,
}

/// Um padrão e de onde ele veio (pras mensagens `arquivo:linha:`).
struct Pattern {
    text: Vec<u8>,
    /// `None` = linha de comando; `Some(("-", n))` = stdin.
    origin: Option<(Vec<u8>, usize)>,
}

pub fn main(ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
    main_bytes(ctx, &argv, None)
}

/// `egrep`: o script do Debian (`exec grep -E "$@"`); as mensagens saem como `grep`.
pub fn egrep_main(ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
    main_bytes(ctx, &argv, Some(b"-E"))
}

/// `fgrep`: `exec grep -F "$@"`.
pub fn fgrep_main(ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
    main_bytes(ctx, &argv, Some(b"-F"))
}

/// `rgrep`: `exec grep -r "$@"`.
pub fn rgrep_main(ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
    main_bytes(ctx, &argv, Some(b"-r"))
}

// Valores das opções longas sem equivalente curto.
const BINARY_FILES_OPTION: i32 = 256;
const COLOR_OPTION: i32 = 257;
const EXCLUDE_DIRECTORY_OPTION: i32 = 258;
const EXCLUDE_OPTION: i32 = 259;
const EXCLUDE_FROM_OPTION: i32 = 260;
const GROUP_SEPARATOR_OPTION: i32 = 261;
const INCLUDE_OPTION: i32 = 262;
const LINE_BUFFERED_OPTION: i32 = 263;
const LABEL_OPTION: i32 = 264;
const NO_IGNORE_CASE_OPTION: i32 = 265;
const NO_GROUP_SEPARATOR_OPTION: i32 = 266;
const HELP_OPTION: i32 = 267;

const fn c(ch: u8) -> i32 {
    ch as i32
}

const LONG_OPTIONS: &[LongOpt] = &[
    LongOpt::new("basic-regexp", HasArg::No, c(b'G')),
    LongOpt::new("extended-regexp", HasArg::No, c(b'E')),
    LongOpt::new("fixed-regexp", HasArg::No, c(b'F')),
    LongOpt::new("fixed-strings", HasArg::No, c(b'F')),
    LongOpt::new("perl-regexp", HasArg::No, c(b'P')),
    LongOpt::new("after-context", HasArg::Required, c(b'A')),
    LongOpt::new("before-context", HasArg::Required, c(b'B')),
    LongOpt::new("binary-files", HasArg::Required, BINARY_FILES_OPTION),
    LongOpt::new("byte-offset", HasArg::No, c(b'b')),
    LongOpt::new("context", HasArg::Required, c(b'C')),
    LongOpt::new("color", HasArg::Optional, COLOR_OPTION),
    LongOpt::new("colour", HasArg::Optional, COLOR_OPTION),
    LongOpt::new("count", HasArg::No, c(b'c')),
    LongOpt::new("devices", HasArg::Required, c(b'D')),
    LongOpt::new("directories", HasArg::Required, c(b'd')),
    LongOpt::new("exclude", HasArg::Required, EXCLUDE_OPTION),
    LongOpt::new("exclude-from", HasArg::Required, EXCLUDE_FROM_OPTION),
    LongOpt::new("exclude-dir", HasArg::Required, EXCLUDE_DIRECTORY_OPTION),
    LongOpt::new("file", HasArg::Required, c(b'f')),
    LongOpt::new("files-with-matches", HasArg::No, c(b'l')),
    LongOpt::new("files-without-match", HasArg::No, c(b'L')),
    LongOpt::new("group-separator", HasArg::Required, GROUP_SEPARATOR_OPTION),
    LongOpt::new("help", HasArg::No, HELP_OPTION),
    LongOpt::new("include", HasArg::Required, INCLUDE_OPTION),
    LongOpt::new("ignore-case", HasArg::No, c(b'i')),
    LongOpt::new("no-ignore-case", HasArg::No, NO_IGNORE_CASE_OPTION),
    LongOpt::new("initial-tab", HasArg::No, c(b'T')),
    LongOpt::new("label", HasArg::Required, LABEL_OPTION),
    LongOpt::new("line-buffered", HasArg::No, LINE_BUFFERED_OPTION),
    LongOpt::new("line-number", HasArg::No, c(b'n')),
    LongOpt::new("line-regexp", HasArg::No, c(b'x')),
    LongOpt::new("max-count", HasArg::Required, c(b'm')),
    LongOpt::new("no-filename", HasArg::No, c(b'h')),
    LongOpt::new("no-group-separator", HasArg::No, NO_GROUP_SEPARATOR_OPTION),
    LongOpt::new("no-messages", HasArg::No, c(b's')),
    LongOpt::new("null", HasArg::No, c(b'Z')),
    LongOpt::new("null-data", HasArg::No, c(b'z')),
    LongOpt::new("only-matching", HasArg::No, c(b'o')),
    LongOpt::new("quiet", HasArg::No, c(b'q')),
    LongOpt::new("recursive", HasArg::No, c(b'r')),
    LongOpt::new("dereference-recursive", HasArg::No, c(b'R')),
    LongOpt::new("regexp", HasArg::Required, c(b'e')),
    LongOpt::new("invert-match", HasArg::No, c(b'v')),
    LongOpt::new("silent", HasArg::No, c(b'q')),
    LongOpt::new("text", HasArg::No, c(b'a')),
    LongOpt::new("binary", HasArg::No, c(b'U')),
    LongOpt::new("unix-byte-offsets", HasArg::No, c(b'u')),
    LongOpt::new("version", HasArg::No, c(b'V')),
    LongOpt::new("with-filename", HasArg::No, c(b'H')),
    LongOpt::new("word-regexp", HasArg::No, c(b'w')),
];

const SHORT_OPTIONS: &str = "0123456789A:B:C:D:EFGHIPTUVX:abcd:e:f:hiLlm:noqRrsuvwxyZz";

/// Saída antecipada: mensagem já impressa, código de saída.
struct Die(i32);

fn die(prog: &[u8], msg: impl AsRef<[u8]>) -> Die {
    error(prog, msg.as_ref());
    Die(2)
}

fn usage_error(prog: &[u8]) -> Die {
    let _ = prog;
    crate::io::stderr(USAGE_SHORT.as_bytes());
    Die(2)
}

/// `context_length_arg`: inteiro não negativo (sobra vira o máximo).
fn context_length(prog: &[u8], s: &[u8]) -> Result<i64, Die> {
    let text = String::from_utf8_lossy(s);
    let t = text.trim_start();
    let ok_digits = !t.is_empty() && t.trim_start_matches('+').chars().all(|c| c.is_ascii_digit()) && !t.starts_with("++");
    if !ok_digits || t == "+" {
        return Err(die(prog, format!("{text}: invalid context length argument")));
    }
    Ok(t.trim_start_matches('+').parse::<i64>().unwrap_or(i64::MAX))
}

/// `xstrtoimax` do `-m`: aceita sinal; estouro satura.
fn max_count_arg(prog: &[u8], s: &[u8]) -> Result<i64, Die> {
    let text = String::from_utf8_lossy(s);
    let t = text.trim_start();
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(die(prog, "invalid max count"));
    }
    let v = digits.parse::<i64>().unwrap_or(i64::MAX);
    Ok(if neg { -v } else { v })
}

fn main_bytes(_ctx: &mut Ctx, argv: &[Vec<u8>], prepend: Option<&[u8]>) -> i32 {
    let prog: Vec<u8> = b"grep".to_vec();
    let mut args: Vec<Vec<u8>> = Vec::with_capacity(argv.len() + 1);
    if let Some(p) = prepend {
        args.push(p.to_vec());
    }
    args.extend(argv.iter().skip(1).cloned());
    let mut out = Out::stdout();
    match run(&prog, &args, &mut out) {
        Ok(code) => {
            out.flush();
            if let Some(e) = out.error {
                error(&prog, &errno_msg(b"write error", e));
                return 2;
            }
            code
        }
        Err(Die(code)) => {
            out.flush();
            code
        }
    }
}

fn run(prog: &[u8], args: &[Vec<u8>], out: &mut Out) -> Result<i32, Die> {
    let posixly = sys::getenv("POSIXLY_CORRECT").is_some();
    let mut g = Getopt::new(args, SHORT_OPTIONS, LONG_OPTIONS, posixly);

    let mut mode: Option<Mode> = None;
    let mut filename_option = 0i32;
    let mut eol = b'\n';
    let mut null_after_name = false;
    let mut max_count = i64::MAX;
    let mut out_after: i64 = -1;
    let mut out_before: i64 = -1;
    let mut default_context: i64 = -1;
    let mut only_matching = false;
    let mut patterns: Vec<Pattern> = Vec::new();
    let mut keys_given = false;
    let mut icase = false;
    let mut list_files = ListFiles::None;
    let mut line_number = false;
    let mut byte_offset = false;
    let mut initial_tab = false;
    let mut exit_on_match = false;
    let mut directories = Directories::Read;
    let mut devices = Devices::ReadCommandLine;
    let mut logical = false;
    let mut last_recursive = 0usize;
    let mut suppress_errors = false;
    let mut invert = false;
    let mut words = false;
    let mut lines = false;
    let mut binary_files = BinaryFiles::Binary;
    let mut count_matches = false;
    let mut group_separator: Option<Vec<u8>> = Some(b"--".to_vec());
    let mut line_buffered = false;
    let mut label: Option<Vec<u8>> = None;
    let mut show_version = false;
    let mut show_help = false;
    let mut excludes = Excludes::default();
    let mut dir_excludes = Excludes::default();

    // Dígitos (`-NUM`): o último grupo de dígitos seguidos no mesmo elemento vale.
    let mut digits = String::new();
    let mut digit_index: Option<usize> = None;
    let mut was_digit = false;

    let set_mode = |cur: &mut Option<Mode>, m: Mode| -> Result<(), Die> {
        if let Some(old) = *cur
            && old != m
        {
            return Err(die(prog, "conflicting matchers specified"));
        }
        *cur = Some(m);
        Ok(())
    };

    while let Some(item) = g.next_opt() {
        let opt = match item {
            Ok(o) => o,
            Err(e) => {
                error(prog, &e.detail());
                return Err(usage_error(prog));
            }
        };
        let arg = opt.arg.clone().unwrap_or_default();
        let ch = opt.id;
        if (c(b'0')..=c(b'9')).contains(&ch) {
            // Novo grupo de dígitos, ou zero à esquerda trivial (`p -= buf[0] == '0'`).
            if !(was_digit && digit_index == Some(opt.index)) || digits == "0" {
                digits.clear();
            }
            digits.push(ch as u8 as char);
            digit_index = Some(opt.index);
            was_digit = true;
            continue;
        }
        if was_digit {
            default_context = context_length(prog, digits.as_bytes())?;
            was_digit = false;
        }
        match ch {
            x if x == c(b'A') => out_after = context_length(prog, &arg)?,
            x if x == c(b'B') => out_before = context_length(prog, &arg)?,
            x if x == c(b'C') => default_context = context_length(prog, &arg)?,
            x if x == c(b'D') => {
                devices = match arg.as_slice() {
                    b"read" => Devices::Read,
                    b"skip" => Devices::Skip,
                    _ => return Err(die(prog, "unknown devices method")),
                }
            }
            x if x == c(b'E') => set_mode(&mut mode, Mode::Extended)?,
            x if x == c(b'F') => set_mode(&mut mode, Mode::Fixed)?,
            x if x == c(b'P') => set_mode(&mut mode, Mode::Perl)?,
            x if x == c(b'G') => set_mode(&mut mode, Mode::Basic)?,
            x if x == c(b'X') => {
                let m = match arg.as_slice() {
                    b"grep" => Mode::Basic,
                    b"egrep" => Mode::Extended,
                    b"fgrep" => Mode::Fixed,
                    b"perl" => Mode::Perl,
                    _ => {
                        let mut msg = b"invalid matcher ".to_vec();
                        msg.extend_from_slice(&arg);
                        return Err(die(prog, msg));
                    }
                };
                set_mode(&mut mode, m)?;
            }
            x if x == c(b'H') => filename_option = 1,
            x if x == c(b'I') => binary_files = BinaryFiles::WithoutMatch,
            x if x == c(b'T') => initial_tab = true,
            x if x == c(b'U') => {}
            x if x == c(b'u') => error(prog, b"warning: --unix-byte-offsets (-u) is obsolete"),
            x if x == c(b'V') => show_version = true,
            x if x == c(b'a') => binary_files = BinaryFiles::Text,
            x if x == c(b'b') => byte_offset = true,
            x if x == c(b'c') => count_matches = true,
            x if x == c(b'd') => {
                directories = match arg.as_slice() {
                    b"read" => Directories::Read,
                    b"recurse" => Directories::Recurse,
                    b"skip" => Directories::Skip,
                    _ => {
                        let s = String::from_utf8_lossy(&arg);
                        let msg = format!(
                            "invalid argument \u{2018}{s}\u{2019} for \u{2018}--directories\u{2019}\nValid arguments are:\n  - \u{2018}read\u{2019}\n  - \u{2018}recurse\u{2019}\n  - \u{2018}skip\u{2019}"
                        );
                        error(prog, msg.as_bytes());
                        crate::io::stderr(USAGE_SHORT.as_bytes());
                        return Err(Die(1));
                    }
                };
                if directories == Directories::Recurse {
                    last_recursive = opt.index + 1;
                }
            }
            x if x == c(b'e') => {
                keys_given = true;
                for p in arg.split(|&b| b == b'\n') {
                    patterns.push(Pattern { text: p.to_vec(), origin: None });
                }
            }
            x if x == c(b'f') => {
                keys_given = true;
                let data = if arg == b"-" {
                    read_all(Fd::STDIN).map_err(|e| die(prog, errno_msg(&arg, e)))?
                } else {
                    sys::read_file(&arg).map_err(|e| die(prog, errno_msg(&arg, e)))?
                };
                if !data.is_empty() {
                    let body = data.strip_suffix(b"\n").unwrap_or(&data);
                    for (k, p) in body.split(|&b| b == b'\n').enumerate() {
                        patterns.push(Pattern { text: p.to_vec(), origin: Some((arg.clone(), k + 1)) });
                    }
                }
            }
            x if x == c(b'h') => filename_option = -1,
            x if x == c(b'i') || x == c(b'y') => icase = true,
            NO_IGNORE_CASE_OPTION => icase = false,
            x if x == c(b'L') => list_files = ListFiles::NonMatching,
            x if x == c(b'l') => list_files = ListFiles::Matching,
            x if x == c(b'm') => max_count = max_count_arg(prog, &arg)?,
            x if x == c(b'n') => line_number = true,
            x if x == c(b'o') => only_matching = true,
            x if x == c(b'q') => exit_on_match = true,
            x if x == c(b'R') || x == c(b'r') => {
                if x == c(b'R') {
                    logical = true;
                }
                directories = Directories::Recurse;
                last_recursive = opt.index + 1;
            }
            x if x == c(b's') => suppress_errors = true,
            x if x == c(b'v') => invert = true,
            x if x == c(b'w') => words = true,
            x if x == c(b'x') => lines = true,
            x if x == c(b'Z') => null_after_name = true,
            x if x == c(b'z') => eol = 0,
            BINARY_FILES_OPTION => {
                binary_files = match arg.as_slice() {
                    b"binary" => BinaryFiles::Binary,
                    b"text" => BinaryFiles::Text,
                    b"without-match" => BinaryFiles::WithoutMatch,
                    _ => return Err(die(prog, "unknown binary-files type")),
                }
            }
            COLOR_OPTION => {
                if let Some(v) = &opt.arg {
                    let low = String::from_utf8_lossy(v).to_ascii_lowercase();
                    if !matches!(low.as_str(), "always" | "yes" | "force" | "never" | "no" | "none" | "auto" | "tty" | "if-tty") {
                        show_help = true;
                    }
                }
            }
            EXCLUDE_OPTION | INCLUDE_OPTION => excludes.add(&arg, ch == INCLUDE_OPTION),
            EXCLUDE_FROM_OPTION => {
                let data = sys::read_file(&arg).map_err(|e| die(prog, errno_msg(&arg, e)))?;
                for p in data.split(|&b| b == b'\n').filter(|p| !p.is_empty()) {
                    excludes.add(p, false);
                }
            }
            EXCLUDE_DIRECTORY_OPTION => {
                let mut a = arg.clone();
                while a.len() > 1 && a.ends_with(b"/") {
                    a.pop();
                }
                dir_excludes.add(&a, false);
            }
            GROUP_SEPARATOR_OPTION => group_separator = Some(arg),
            NO_GROUP_SEPARATOR_OPTION => group_separator = None,
            LINE_BUFFERED_OPTION => line_buffered = true,
            LABEL_OPTION => label = Some(arg),
            HELP_OPTION => show_help = true,
            _ => return Err(usage_error(prog)),
        }
    }
    if was_digit {
        default_context = context_length(prog, digits.as_bytes())?;
    }
    let mut operands = g.operands();

    if show_version {
        out.write(VERSION.as_bytes());
        return Ok(0);
    }
    if show_help {
        out.write(HELP.as_bytes());
        return Ok(0);
    }

    let mode = mode.unwrap_or(Mode::Basic);
    if !keys_given {
        if operands.is_empty() {
            return Err(usage_error(prog));
        }
        let mut pat = operands.remove(0);
        // `grep '\-x'`: o `\` antes do `-` cai (evita o aviso de barra solta).
        if mode != Mode::Fixed && pat.starts_with(b"\\-") {
            pat.remove(0);
        }
        for p in pat.split(|&b| b == b'\n') {
            patterns.push(Pattern { text: p.to_vec(), origin: None });
        }
    }

    // Padrões repetidos saem (como o `update_patterns`); o primeiro fica, com sua origem.
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    patterns.retain(|p| seen.insert(p.text.clone()));
    if keys_given && patterns.is_empty() {
        // `-f /dev/null`: nenhum padrão, nada casa.
        invert = !invert;
        lines = false;
        words = false;
        patterns.push(Pattern { text: Vec::new(), origin: None });
    }
    let keycc_zero = patterns.len() == 1 && patterns[0].text.is_empty();

    let stdout_stat = sys::current().fstat(Fd::STDOUT).ok();
    let dev_null_output = !exit_on_match
        && stdout_stat.as_ref().is_some_and(|st| {
            st.file_type() == FileType::CharDevice
                && sys::stat(b"/dev/null").is_ok_and(|n| n.dev == st.dev && n.ino == st.ino)
        });
    if exit_on_match || dev_null_output {
        list_files = ListFiles::None;
    }
    let mut done_on_match = false;
    if exit_on_match || dev_null_output || list_files != ListFiles::None {
        count_matches = false;
        done_on_match = true;
    }
    let out_quiet = count_matches || done_on_match;
    if out_after < 0 {
        out_after = default_context;
    }
    if out_before < 0 {
        out_before = default_context;
    }
    if (max_count == 0 || (keycc_zero && invert && !lines && !words)) && list_files != ListFiles::NonMatching {
        return Ok(1);
    }

    // Erros de sintaxe: cada padrão sozinho, todos relatados (`regex_compile` do GEAcompile).
    if mode != Mode::Fixed && mode != Mode::Perl {
        let syntax = if mode == Mode::Extended { Syntax::EGREP } else { Syntax::GREP };
        let b = RegexBuilder::new(syntax).icase(icase);
        let mut failed = false;
        for p in &patterns {
            if let Err(regex_posix::Error::Syntax(code)) = b.check(&p.text) {
                let msg = match &p.origin {
                    Some((file, line)) => {
                        let mut m = file.clone();
                        m.extend_from_slice(format!(":{line}: {}", code.message()).as_bytes());
                        m
                    }
                    None => code.message().as_bytes().to_vec(),
                };
                error(prog, &msg);
                failed = true;
            }
        }
        if failed {
            return Err(Die(2));
        }
        // Diagnósticos do dfa.c, na ordem dos padrões.
        for p in &patterns {
            let d = b.check(&p.text).map_err(|_| Die(2))?;
            if d.confusing_brackets {
                return Err(die(prog, regex_posix::CONFUSING_BRACKETS));
            }
            for w in d.warnings {
                let mut m = b"warning: ".to_vec();
                m.extend_from_slice(w.message().as_bytes());
                error(prog, &m);
            }
        }
    }

    let texts: Vec<Vec<u8>> = patterns.iter().map(|p| p.text.clone()).collect();
    let spec = MatcherSpec { mode, patterns: &texts, icase, words, lines, eol };
    let matcher = if mode == Mode::Perl {
        if texts.len() > 1 {
            return Err(die(prog, "the -P option only supports a single pattern"));
        }
        LineMatcher::build_perl(&spec).map_err(|e| die(prog, e.0))?
    } else {
        LineMatcher::build(&spec).map_err(|e| die(prog, e.message()))?
    };

    let num_operands = operands.len();
    let out_file_init: i32 = if filename_option == 0 && num_operands <= 1 {
        -((directories == Directories::Recurse) as i32)
    } else {
        (0 <= filename_option) as i32
    };
    if logical && devices == Devices::ReadCommandLine {
        devices = Devices::Read;
    }
    let mut omit_dot_slash = false;
    if operands.is_empty() {
        if directories == Directories::Recurse && last_recursive > 0 {
            operands.push(b".".to_vec());
            omit_dot_slash = true;
        } else {
            operands.push(b"-".to_vec());
        }
    }

    let opts = Opts {
        eol,
        invert,
        only_matching,
        line_number,
        byte_offset,
        initial_tab,
        null_after_name,
        line_buffered,
        count_matches,
        list_files,
        suppress_errors,
        binary_files,
        max_count,
        before: out_before,
        after: out_after,
        group_separator,
        out_quiet,
        done_on_match,
        exit_on_match,
        directories,
        devices,
        logical,
        label,
    };
    if line_buffered {
        out.set_line_buffered(true);
    }
    let out_stat = stdout_stat.filter(|st| !exit_on_match && st.file_type() == FileType::Regular);
    let mut w = Walker {
        s: Searcher::new(&opts, &matcher, out, prog),
        excludes,
        dir_excludes,
        omit_dot_slash,
        out_file: out_file_init,
        out_stat,
        ancestors: Vec::new(),
    };
    let mut status = true;
    for arg in &operands {
        status &= w.command_line_arg(arg);
    }
    Ok(if w.s.errseen { 2 } else if status { 1 } else { 0 })
}

/// Travessia dos arquivos (`grepfile`, `grepdesc`, `grepdirent` com a semântica do `fts`).
struct Walker<'a> {
    s: Searcher<'a>,
    excludes: Excludes,
    dir_excludes: Excludes,
    omit_dot_slash: bool,
    /// -1: decidir pelo primeiro operando (`grep -r PADRÃO OPERANDO`).
    out_file: i32,
    out_stat: Option<Stat>,
    /// (dev, ino) dos diretórios abertos na recursão, pra detectar ciclo com `-R`.
    ancestors: Vec<(u64, u64)>,
}

fn is_device(st: &Stat) -> bool {
    matches!(st.file_type(), FileType::CharDevice | FileType::BlockDevice | FileType::Socket | FileType::Fifo)
}

impl Walker<'_> {
    fn skip_devices(&self, command_line: bool) -> bool {
        let o = self.s.o;
        o.devices == Devices::Skip || (o.devices == Devices::ReadCommandLine && !command_line)
    }

    /// `skipped_file`.
    fn skipped_file(&self, name: &[u8], command_line: bool, is_dir: bool) -> bool {
        if !is_dir {
            self.excludes.excluded(name, !command_line)
        } else if self.s.o.directories == Directories::Skip {
            true
        } else if command_line && self.omit_dot_slash {
            false
        } else {
            self.dir_excludes.excluded(name, !command_line)
        }
    }

    fn command_line_arg(&mut self, arg: &[u8]) -> bool {
        if arg == b"-" {
            self.s.filename = self.s.o.label.clone().unwrap_or_else(|| b"(standard input)".to_vec());
            self.grepdesc(Fd::STDIN, true)
        } else {
            self.s.filename = arg.to_vec();
            self.grepfile(arg, true, true)
        }
    }

    fn grepfile(&mut self, path: &[u8], follow: bool, command_line: bool) -> bool {
        let mut flags = OFlags::RDONLY | OFlags::NOCTTY;
        if !follow {
            flags |= OFlags::NOFOLLOW;
        }
        if self.skip_devices(command_line) {
            flags |= OFlags::NONBLOCK;
        }
        match sys::current().openat(Fd::CWD, path, flags, 0) {
            Ok(fd) => self.grepdesc(fd, command_line),
            Err(e) => {
                if follow || e != Errno::ELOOP {
                    self.s.suppressible_error(e);
                }
                true
            }
        }
    }

    fn close(&mut self, fd: Fd) {
        if fd != Fd::STDIN
            && let Err(e) = sys::close(fd)
        {
            self.s.suppressible_error(e);
        }
    }

    fn grepdesc(&mut self, fd: Fd, command_line: bool) -> bool {
        let sys = sys::current();
        let st = match sys.fstat(fd) {
            Ok(st) => st,
            Err(e) => {
                self.s.suppressible_error(e);
                self.close(fd);
                return true;
            }
        };
        let is_dir = st.file_type() == FileType::Directory;
        if fd != Fd::STDIN && self.skip_devices(command_line) && is_device(&st) {
            self.close(fd);
            return true;
        }
        if fd != Fd::STDIN && command_line && self.skipped_file(&self.s.filename.clone(), true, is_dir) {
            self.close(fd);
            return true;
        }
        if self.out_file < 0 {
            self.out_file = is_dir as i32;
        }
        self.s.out_file = self.out_file > 0;
        let o = self.s.o;
        if fd != Fd::STDIN && o.directories == Directories::Recurse && is_dir {
            self.close(fd);
            let root = self.s.filename.clone();
            return self.traverse(&root, command_line);
        }
        if fd != Fd::STDIN
            && ((o.directories == Directories::Skip && is_dir)
                || ((o.devices == Devices::Skip || (o.devices == Devices::ReadCommandLine && !command_line)) && is_device(&st)))
        {
            self.close(fd);
            return true;
        }
        if !self.s.out_quiet
            && o.list_files == ListFiles::None
            && 1 < o.max_count
            && st.file_type() == FileType::Regular
            && self.out_stat.as_ref().is_some_and(|os| os.dev == st.dev && os.ino == st.ino)
        {
            if !o.suppress_errors {
                let mut m = self.s.filename.clone();
                m.extend_from_slice(b": input file is also the output");
                error(self.s.prog, &m);
            }
            self.s.errseen = true;
            self.close(fd);
            return true;
        }
        let scan = self.s.grep(fd, &st);
        if o.count_matches {
            if self.s.out_file {
                let name = self.s.filename.clone();
                self.s.out.write(&name);
                self.s.out.byte(if o.null_after_name { 0 } else { b':' });
            }
            self.s.out.write(format!("{}\n", scan.nlines).as_bytes());
            if o.line_buffered {
                self.s.out.flush();
            }
        }
        let status = scan.nlines == 0;
        if o.list_files == ListFiles::None {
            self.s.finalize_input(fd, &st, scan.ineof);
        } else if o.list_files == (if status { ListFiles::NonMatching } else { ListFiles::Matching }) {
            let name = self.s.filename.clone();
            self.s.out.write(&name);
            self.s.out.byte(if o.null_after_name { 0 } else { b'\n' });
            if o.line_buffered {
                self.s.out.flush();
            }
        }
        self.close(fd);
        status
    }

    /// Nome exibido de um caminho da travessia (`omit_dot_slash`).
    fn display(&self, path: &[u8]) -> Vec<u8> {
        if self.omit_dot_slash && path.len() > 1 && path.starts_with(b"./") {
            path[2..].to_vec()
        } else {
            path.to_vec()
        }
    }

    /// `fts` sobre `root` (já aberto como diretório na linha de comando).
    fn traverse(&mut self, root: &[u8], _command_line: bool) -> bool {
        let sys = sys::current();
        let st = match sys.fstatat(Fd::CWD, root, AtFlags::empty()) {
            Ok(st) => st,
            Err(e) => {
                self.s.filename = self.display(root);
                self.s.suppressible_error(e);
                return true;
            }
        };
        self.ancestors.push((st.dev, st.ino));
        let status = self.walk_dir(root);
        self.ancestors.pop();
        status
    }

    fn walk_dir(&mut self, dir: &[u8]) -> bool {
        let entries = match sys::read_dir(dir) {
            Ok(v) => v,
            Err(e) => {
                self.s.filename = self.display(dir);
                self.s.suppressible_error(e);
                return true;
            }
        };
        let mut status = true;
        for ent in entries {
            sys::checkpoint();
            let mut path = dir.to_vec();
            if !path.ends_with(b"/") {
                path.push(b'/');
            }
            path.extend_from_slice(&ent.name);
            status &= self.dirent(&path, &ent.name, ent.kind);
        }
        status
    }

    /// `grepdirent` pra uma entrada abaixo da raiz.
    fn dirent(&mut self, path: &[u8], name: &[u8], kind: FileType) -> bool {
        let sys = sys::current();
        let logical = self.s.o.logical;
        // Com -R o fts segue symlinks; com -r eles chegam como tipo symlink.
        let (kind, target_stat) = if kind == FileType::Symlink && logical {
            match sys.fstatat(Fd::CWD, path, AtFlags::empty()) {
                Ok(st) => (st.file_type(), Some(st)),
                Err(_) => (FileType::Symlink, None),
            }
        } else {
            (kind, None)
        };
        let is_dir = kind == FileType::Directory;
        if self.skipped_file(name, false, is_dir) {
            return true;
        }
        self.s.filename = self.display(path);
        if is_dir {
            if self.s.o.directories != Directories::Recurse {
                return true;
            }
            let st = match target_stat {
                Some(st) => st,
                None => match sys.fstatat(Fd::CWD, path, AtFlags::SYMLINK_NOFOLLOW) {
                    Ok(st) => st,
                    Err(e) => {
                        self.s.suppressible_error(e);
                        return true;
                    }
                },
            };
            if self.ancestors.contains(&(st.dev, st.ino)) {
                if !self.s.o.suppress_errors {
                    let mut m = self.s.filename.clone();
                    m.extend_from_slice(b": warning: recursive directory loop");
                    error(self.s.prog, &m);
                }
                return true;
            }
            self.ancestors.push((st.dev, st.ino));
            let r = self.walk_dir(path);
            self.ancestors.pop();
            return r;
        }
        match kind {
            FileType::Symlink if !logical => {
                // FTS_NSOK com S_IFLNK: abre com O_NOFOLLOW e o ELOOP é silencioso.
                self.grepfile(path, false, false)
            }
            FileType::CharDevice | FileType::BlockDevice | FileType::Fifo | FileType::Socket if self.skip_devices(false) => true,
            _ => self.grepfile(path, logical, false),
        }
    }
}
