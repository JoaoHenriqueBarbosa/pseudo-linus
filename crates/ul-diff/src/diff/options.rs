//! Opções do `diff` (GNU diffutils 3.10): tabela de longas na ordem do GNU (é a ordem das
//! possibilidades nas mensagens de abreviação ambígua), conflitos de estilo, validação de números e as
//! mensagens de erro com "Try 'diff --help' for more information.".

use crate::getopt::{Getopt, HasArg, Item, LongOpt};
use crate::sysutil;

use super::format::Palette;
use super::ifdef::Formats;
use super::text::Normalize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Normal,
    Context,
    Unified,
    Ed,
    ForwardEd,
    Rcs,
    SideBySide,
    Ifdef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorWhen {
    Never,
    Always,
    Auto,
}

/// Opções já interpretadas.
#[derive(Clone, Debug)]
pub struct Opts {
    pub style: Style,
    pub context: usize,
    pub brief: bool,
    pub report_identical: bool,
    pub recursive: bool,
    pub new_file: bool,
    pub unidirectional_new_file: bool,
    pub text: bool,
    pub strip_trailing_cr: bool,
    pub norm: Normalize,
    pub ignore_blank_lines: bool,
    pub ignore_regex: Vec<Vec<u8>>,
    pub function_regex: Vec<Vec<u8>>,
    pub labels: Vec<Vec<u8>>,
    pub width: usize,
    pub left_column: bool,
    pub suppress_common: bool,
    pub expand_tabs: bool,
    pub initial_tab: bool,
    pub tabsize: usize,
    pub suppress_blank_empty: bool,
    pub paginate: bool,
    pub color: ColorWhen,
    pub palette: Palette,
    pub formats: Formats,
    pub excludes: Vec<Vec<u8>>,
    pub exclude_from: Vec<Vec<u8>>,
    pub starting_file: Option<Vec<u8>>,
    pub from_file: Option<Vec<u8>>,
    pub to_file: Option<Vec<u8>>,
    pub no_dereference: bool,
    pub ignore_file_name_case: bool,
    pub minimal: bool,
    pub speed_large_files: bool,
    pub horizon: usize,
    /// Opções como foram digitadas, já com as aspas do shell, pra linha "diff -r ..." dos diretórios.
    pub switches: Vec<u8>,
    pub operands: Vec<Vec<u8>>,
}

const BINARY: u32 = 1000;
const CHANGED_GROUP_FORMAT: u32 = 1001;
const COLOR: u32 = 1002;
const FROM_FILE: u32 = 1003;
const HELP: u32 = 1004;
const HORIZON_LINES: u32 = 1005;
const IGNORE_FILE_NAME_CASE: u32 = 1006;
const INHIBIT_HUNK_MERGE: u32 = 1007;
const LEFT_COLUMN: u32 = 1008;
const LINE_FORMAT: u32 = 1009;
const NEW_GROUP_FORMAT: u32 = 1010;
const NEW_LINE_FORMAT: u32 = 1011;
const NO_DEREFERENCE: u32 = 1012;
const NO_IGNORE_FILE_NAME_CASE: u32 = 1013;
const NORMAL: u32 = 1014;
const OLD_GROUP_FORMAT: u32 = 1015;
const OLD_LINE_FORMAT: u32 = 1016;
const PALETTE: u32 = 1017;
const SDIFF_MERGE_ASSIST: u32 = 1018;
const STRIP_TRAILING_CR: u32 = 1019;
const SUPPRESS_BLANK_EMPTY: u32 = 1020;
const SUPPRESS_COMMON_LINES: u32 = 1021;
const TABSIZE: u32 = 1022;
const TO_FILE: u32 = 1023;
const UNCHANGED_GROUP_FORMAT: u32 = 1024;
const UNCHANGED_LINE_FORMAT: u32 = 1025;

const fn c(ch: u8) -> u32 {
    ch as u32
}

/// Tabela de opções longas, em ordem alfabética como a do GNU.
pub const LONGS: &[LongOpt] = &[
    LongOpt::new("binary", HasArg::No, BINARY),
    LongOpt::new("brief", HasArg::No, c(b'q')),
    LongOpt::new("changed-group-format", HasArg::Required, CHANGED_GROUP_FORMAT),
    LongOpt::new("color", HasArg::Optional, COLOR),
    LongOpt::new("context", HasArg::Optional, c(b'C')),
    LongOpt::new("ed", HasArg::No, c(b'e')),
    LongOpt::new("exclude", HasArg::Required, c(b'x')),
    LongOpt::new("exclude-from", HasArg::Required, c(b'X')),
    LongOpt::new("expand-tabs", HasArg::No, c(b't')),
    LongOpt::new("forward-ed", HasArg::No, c(b'f')),
    LongOpt::new("from-file", HasArg::Required, FROM_FILE),
    LongOpt::new("help", HasArg::No, HELP),
    LongOpt::new("horizon-lines", HasArg::Required, HORIZON_LINES),
    LongOpt::new("ifdef", HasArg::Required, c(b'D')),
    LongOpt::new("ignore-all-space", HasArg::No, c(b'w')),
    LongOpt::new("ignore-blank-lines", HasArg::No, c(b'B')),
    LongOpt::new("ignore-case", HasArg::No, c(b'i')),
    LongOpt::new("ignore-file-name-case", HasArg::No, IGNORE_FILE_NAME_CASE),
    LongOpt::new("ignore-matching-lines", HasArg::Required, c(b'I')),
    LongOpt::new("ignore-space-change", HasArg::No, c(b'b')),
    LongOpt::new("ignore-tab-expansion", HasArg::No, c(b'E')),
    LongOpt::new("ignore-trailing-space", HasArg::No, c(b'Z')),
    LongOpt::new("inhibit-hunk-merge", HasArg::No, INHIBIT_HUNK_MERGE),
    LongOpt::new("initial-tab", HasArg::No, c(b'T')),
    LongOpt::new("label", HasArg::Required, c(b'L')),
    LongOpt::new("left-column", HasArg::No, LEFT_COLUMN),
    LongOpt::new("line-format", HasArg::Required, LINE_FORMAT),
    LongOpt::new("minimal", HasArg::No, c(b'd')),
    LongOpt::new("new-file", HasArg::No, c(b'N')),
    LongOpt::new("new-group-format", HasArg::Required, NEW_GROUP_FORMAT),
    LongOpt::new("new-line-format", HasArg::Required, NEW_LINE_FORMAT),
    LongOpt::new("no-dereference", HasArg::No, NO_DEREFERENCE),
    LongOpt::new("no-ignore-file-name-case", HasArg::No, NO_IGNORE_FILE_NAME_CASE),
    LongOpt::new("normal", HasArg::No, NORMAL),
    LongOpt::new("old-group-format", HasArg::Required, OLD_GROUP_FORMAT),
    LongOpt::new("old-line-format", HasArg::Required, OLD_LINE_FORMAT),
    LongOpt::new("paginate", HasArg::No, c(b'l')),
    LongOpt::new("palette", HasArg::Required, PALETTE),
    LongOpt::new("rcs", HasArg::No, c(b'n')),
    LongOpt::new("recursive", HasArg::No, c(b'r')),
    LongOpt::new("report-identical-files", HasArg::No, c(b's')),
    LongOpt::new("sdiff-merge-assist", HasArg::No, SDIFF_MERGE_ASSIST),
    LongOpt::new("show-c-function", HasArg::No, c(b'p')),
    LongOpt::new("show-function-line", HasArg::Required, c(b'F')),
    LongOpt::new("side-by-side", HasArg::No, c(b'y')),
    LongOpt::new("speed-large-files", HasArg::No, c(b'H')),
    LongOpt::new("starting-file", HasArg::Required, c(b'S')),
    LongOpt::new("strip-trailing-cr", HasArg::No, STRIP_TRAILING_CR),
    LongOpt::new("suppress-blank-empty", HasArg::No, SUPPRESS_BLANK_EMPTY),
    LongOpt::new("suppress-common-lines", HasArg::No, SUPPRESS_COMMON_LINES),
    LongOpt::new("tabsize", HasArg::Required, TABSIZE),
    LongOpt::new("text", HasArg::No, c(b'a')),
    LongOpt::new("to-file", HasArg::Required, TO_FILE),
    LongOpt::new("unchanged-group-format", HasArg::Required, UNCHANGED_GROUP_FORMAT),
    LongOpt::new("unchanged-line-format", HasArg::Required, UNCHANGED_LINE_FORMAT),
    LongOpt::new("unidirectional-new-file", HasArg::No, c(b'P')),
    LongOpt::new("unified", HasArg::Optional, c(b'U')),
    LongOpt::new("version", HasArg::No, c(b'v')),
    LongOpt::new("width", HasArg::Required, c(b'W')),
];

const SHORTS: &str = "0123456789abBcC:dD:eEfF:hHiI:lL:nNpPqrsS:tTuU:vwW:x:X:yZ";

/// Como o `main` deve terminar depois de interpretar as opções.
pub enum Parsed {
    Run(Box<Opts>),
    /// Já imprimiu o que tinha que imprimir (ajuda, versão ou erro); sai com o código.
    Exit(i32),
}

/// Aspas do shell como o gnulib (`shell_quoting_style`): sem aspas quando todos os bytes são seguros;
/// senão entre aspas simples, com `'` virando `'\''`.
pub fn shell_quote(arg: &[u8]) -> Vec<u8> {
    if arg.is_empty() {
        return b"''".to_vec();
    }
    let safe = |(i, &b): (usize, &u8)| -> bool {
        b.is_ascii_alphanumeric()
            || matches!(b, b'%' | b'+' | b',' | b'-' | b'.' | b'/' | b':' | b'@' | b']' | b'_')
            || (i > 0 && matches!(b, b'#' | b'~'))
            || b >= 0x80
    };
    if arg.iter().enumerate().all(safe) {
        return arg.to_vec();
    }
    let mut out = vec![b'\''];
    for &b in arg {
        if b == b'\'' {
            out.extend_from_slice(b"'\\''");
        } else {
            out.push(b);
        }
    }
    out.push(b'\'');
    out
}

fn try_help(argv0: &str, msg: &str) -> Parsed {
    sysutil::eprint(format!("{argv0}: {msg}\n{argv0}: Try '{argv0} --help' for more information.\n"));
    Parsed::Exit(2)
}

fn parse_size(v: &[u8]) -> Option<usize> {
    if v.is_empty() || !v.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut n: usize = 0;
    for &b in v {
        n = n.saturating_mul(10).saturating_add((b - b'0') as usize);
    }
    Some(n)
}

/// Lê as opções. Imprime ajuda, versão e erros por conta própria.
pub fn parse(argv: &[Vec<u8>]) -> Parsed {
    let argv0 = sysutil::argv0(argv);
    let mut o = Opts {
        style: Style::Normal,
        context: 0,
        brief: false,
        report_identical: false,
        recursive: false,
        new_file: false,
        unidirectional_new_file: false,
        text: false,
        strip_trailing_cr: false,
        norm: Normalize::default(),
        ignore_blank_lines: false,
        ignore_regex: Vec::new(),
        function_regex: Vec::new(),
        labels: Vec::new(),
        width: 130,
        left_column: false,
        suppress_common: false,
        expand_tabs: false,
        initial_tab: false,
        tabsize: 8,
        suppress_blank_empty: false,
        paginate: false,
        color: ColorWhen::Never,
        palette: Palette::default(),
        formats: Formats::default(),
        excludes: Vec::new(),
        exclude_from: Vec::new(),
        starting_file: None,
        from_file: None,
        to_file: None,
        no_dereference: false,
        ignore_file_name_case: false,
        minimal: false,
        speed_large_files: false,
        horizon: 0,
        switches: Vec::new(),
        operands: Vec::new(),
    };
    let mut style: Option<Style> = None;
    let mut context: isize = -1;
    let mut explicit_context = false;
    let mut ocontext: isize = -1;
    let mut prev_digit_index: Option<(usize, bool)> = None;
    let mut show_c_function = false;
    let mut horizon: Option<usize> = None;
    let mut used: Vec<usize> = Vec::new();

    let set_style = |style: &mut Option<Style>, s: Style| -> bool {
        match style {
            Some(cur) if *cur != s => false,
            _ => {
                *style = Some(s);
                true
            }
        }
    };

    let mut g = Getopt::from_env(argv, SHORTS, LONGS);
    while let Some(item) = g.next() {
        let opt = match item {
            Ok(Item::Operand(v)) => {
                o.operands.push(v);
                continue;
            }
            Ok(Item::Opt(opt)) => opt,
            Err(e) => {
                sysutil::eprint(e.message_bytes(&argv0));
                sysutil::eprint(format!("{argv0}: Try '{argv0} --help' for more information.\n"));
                return Parsed::Exit(2);
            }
        };
        // O elemento da opção e, quando o valor veio separado, o seguinte.
        used.extend(opt.index..g.optind().max(opt.index + 1));
        let arg = opt.arg.clone().unwrap_or_default();
        let id = opt.id;
        match id {
            x if (b'0' as u32..=b'9' as u32).contains(&x) => {
                let d = (x - b'0' as u32) as isize;
                let continues = matches!(prev_digit_index, Some((idx, true)) if idx == opt.index);
                if continues {
                    ocontext = ocontext.saturating_mul(10).saturating_add(d);
                } else {
                    ocontext = d;
                }
                prev_digit_index = Some((opt.index, true));
                continue;
            }
            _ => {}
        }
        prev_digit_index = Some((opt.index, false));
        match id {
            x if x == c(b'a') => o.text = true,
            x if x == c(b'b') => o.norm.ignore_space_change = true,
            x if x == c(b'B') => o.ignore_blank_lines = true,
            x if x == c(b'c') || x == c(b'u') || x == c(b'C') || x == c(b'U') => {
                let s = if x == c(b'c') || x == c(b'C') { Style::Context } else { Style::Unified };
                if opt.arg.is_some() {
                    let Some(n) = parse_size(&arg) else {
                        return try_help(&argv0, &format!("invalid context length '{}'", String::from_utf8_lossy(&arg)));
                    };
                    let n = n.min(isize::MAX as usize) as isize;
                    if context < n {
                        context = n;
                    }
                    explicit_context = true;
                } else if context < 3 {
                    context = 3;
                }
                if !set_style(&mut style, s) {
                    return try_help(&argv0, "conflicting output style options");
                }
            }
            x if x == c(b'd') => o.minimal = true,
            x if x == c(b'D') => {
                if !set_style(&mut style, Style::Ifdef) {
                    return try_help(&argv0, "conflicting output style options");
                }
                o.formats.ifdef = Some(arg);
            }
            x if x == c(b'e') => {
                if !set_style(&mut style, Style::Ed) {
                    return try_help(&argv0, "conflicting output style options");
                }
            }
            x if x == c(b'E') => o.norm.ignore_tab_expansion = true,
            x if x == c(b'f') => {
                if !set_style(&mut style, Style::ForwardEd) {
                    return try_help(&argv0, "conflicting output style options");
                }
            }
            x if x == c(b'F') => o.function_regex.push(arg),
            x if x == c(b'h') => {}
            x if x == c(b'H') => o.speed_large_files = true,
            x if x == c(b'i') => o.norm.ignore_case = true,
            x if x == c(b'I') => o.ignore_regex.push(arg),
            x if x == c(b'l') => o.paginate = true,
            x if x == c(b'L') => {
                if o.labels.len() >= 2 {
                    sysutil::eprint(format!("{argv0}: too many file label options\n"));
                    return Parsed::Exit(2);
                }
                o.labels.push(arg);
            }
            x if x == c(b'n') => {
                if !set_style(&mut style, Style::Rcs) {
                    return try_help(&argv0, "conflicting output style options");
                }
            }
            x if x == c(b'N') => o.new_file = true,
            x if x == c(b'p') => {
                show_c_function = true;
                o.function_regex.push(b"^[[:alpha:]$_]".to_vec());
            }
            x if x == c(b'P') => o.unidirectional_new_file = true,
            x if x == c(b'q') => o.brief = true,
            x if x == c(b'r') => o.recursive = true,
            x if x == c(b's') => o.report_identical = true,
            x if x == c(b'S') => o.starting_file = Some(arg),
            x if x == c(b't') => o.expand_tabs = true,
            x if x == c(b'T') => o.initial_tab = true,
            x if x == c(b'v') => {
                let mut out = sysutil::Output::stdout();
                out.write_str(super::help::VERSION);
                return Parsed::Exit(if out.finish().is_ok() { 0 } else { 2 });
            }
            x if x == c(b'w') => o.norm.ignore_all_space = true,
            x if x == c(b'W') => match parse_size(&arg) {
                Some(n) if n > 0 => o.width = n,
                _ => return try_help(&argv0, &format!("invalid width '{}'", String::from_utf8_lossy(&arg))),
            },
            x if x == c(b'x') => o.excludes.push(arg),
            x if x == c(b'X') => o.exclude_from.push(arg),
            x if x == c(b'y') => {
                if !set_style(&mut style, Style::SideBySide) {
                    return try_help(&argv0, "conflicting output style options");
                }
            }
            x if x == c(b'Z') => o.norm.ignore_trailing_space = true,
            BINARY | INHIBIT_HUNK_MERGE | SDIFF_MERGE_ASSIST => {}
            CHANGED_GROUP_FORMAT | NEW_GROUP_FORMAT | OLD_GROUP_FORMAT | UNCHANGED_GROUP_FORMAT | LINE_FORMAT
            | NEW_LINE_FORMAT | OLD_LINE_FORMAT | UNCHANGED_LINE_FORMAT => {
                if !set_style(&mut style, Style::Ifdef) {
                    return try_help(&argv0, "conflicting output style options");
                }
                let f = &mut o.formats;
                match id {
                    CHANGED_GROUP_FORMAT => f.changed_group = Some(arg),
                    NEW_GROUP_FORMAT => f.new_group = Some(arg),
                    OLD_GROUP_FORMAT => f.old_group = Some(arg),
                    UNCHANGED_GROUP_FORMAT => f.unchanged_group = Some(arg),
                    NEW_LINE_FORMAT => f.new_line = Some(arg),
                    OLD_LINE_FORMAT => f.old_line = Some(arg),
                    UNCHANGED_LINE_FORMAT => f.unchanged_line = Some(arg),
                    _ => {
                        f.new_line = Some(arg.clone());
                        f.old_line = Some(arg.clone());
                        f.unchanged_line = Some(arg);
                    }
                }
            }
            COLOR => {
                o.color = match opt.arg.as_deref() {
                    None | Some(b"auto") | Some(b"tty") | Some(b"if-tty") => ColorWhen::Auto,
                    Some(b"always") | Some(b"yes") | Some(b"force") => ColorWhen::Always,
                    Some(b"never") | Some(b"no") | Some(b"none") => ColorWhen::Never,
                    Some(v) => {
                        return try_help(&argv0, &format!("invalid color '{}'", String::from_utf8_lossy(v)));
                    }
                };
            }
            FROM_FILE => o.from_file = Some(arg),
            TO_FILE => o.to_file = Some(arg),
            HELP => {
                let mut out = sysutil::Output::stdout();
                out.write_str(super::help::HELP);
                return Parsed::Exit(if out.finish().is_ok() { 0 } else { 2 });
            }
            HORIZON_LINES => match parse_size(&arg) {
                Some(n) => horizon = Some(n),
                None => {
                    return try_help(&argv0, &format!("invalid horizon length '{}'", String::from_utf8_lossy(&arg)));
                }
            },
            IGNORE_FILE_NAME_CASE => o.ignore_file_name_case = true,
            NO_IGNORE_FILE_NAME_CASE => o.ignore_file_name_case = false,
            LEFT_COLUMN => o.left_column = true,
            NO_DEREFERENCE => o.no_dereference = true,
            NORMAL => {
                if !set_style(&mut style, Style::Normal) {
                    return try_help(&argv0, "conflicting output style options");
                }
            }
            PALETTE => o.palette.apply(&String::from_utf8_lossy(&arg)),
            STRIP_TRAILING_CR => o.strip_trailing_cr = true,
            SUPPRESS_BLANK_EMPTY => o.suppress_blank_empty = true,
            SUPPRESS_COMMON_LINES => o.suppress_common = true,
            TABSIZE => match parse_size(&arg) {
                Some(n) if n > 0 => o.tabsize = n,
                _ => return try_help(&argv0, &format!("invalid tabsize '{}'", String::from_utf8_lossy(&arg))),
            },
            _ => {}
        }
    }

    // Opções como digitadas (na ordem), com o "--" quando houve, pro cabeçalho dos diretórios.
    let dashdash = argv.iter().skip(1).position(|a| a == b"--").map(|p| p + 1);
    let mut idx: Vec<usize> = used;
    idx.sort_unstable();
    idx.dedup();
    let mut sw = Vec::new();
    for i in idx.iter().copied().filter(|i| Some(*i) != dashdash && dashdash.is_none_or(|d| *i < d)) {
        if let Some(a) = argv.get(i) {
            sw.push(b' ');
            sw.extend_from_slice(&shell_quote(a));
        }
    }
    if dashdash.is_some() {
        sw.extend_from_slice(b" --");
    }
    o.switches = sw;

    if show_c_function && style.is_none() {
        style = Some(Style::Context);
        if context < 3 {
            context = 3;
        }
    }
    o.style = style.unwrap_or(Style::Normal);
    if !explicit_context && ocontext >= 0 && matches!(o.style, Style::Context | Style::Unified) {
        context = ocontext;
    }
    o.context = context.max(0) as usize;
    if matches!(o.style, Style::Context | Style::Unified) && context < 0 {
        o.context = 3;
    }
    let base_horizon = if matches!(o.style, Style::Context | Style::Unified) { o.context } else { 0 };
    o.horizon = horizon.unwrap_or(0).max(base_horizon);
    o.norm.tabsize = o.tabsize;
    Parsed::Run(Box::new(o))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_like_gnulib() {
        assert_eq!(shell_quote(b"-r"), b"-r");
        assert_eq!(shell_quote(b"--unified=1"), b"'--unified=1'");
        assert_eq!(shell_quote(b"a b"), b"'a b'");
        assert_eq!(shell_quote(b"it's"), b"'it'\\''s'");
        assert_eq!(shell_quote(b""), b"''");
    }
}
