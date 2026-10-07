//! Opções de linha de comando do `patch`, com a tabela de opções longas na ordem do GNU patch 2.8
//! (a ordem aparece nas mensagens de abreviação ambígua) e as mensagens de erro de cada opção.

use ul_common::getopt::{Getopt, HasArg, Item, LongOpt};

/// Formato forçado com -c, -e, -n, -u.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForcedFormat {
    Context,
    Ed,
    Normal,
    Unified,
}

/// Estilo de nome de backup (-V e `VERSION_CONTROL`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionControl {
    Simple,
    Numbered,
    Existing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectFormat {
    /// O formato do próprio hunk (unificado vira unificado, contexto e normal viram contexto).
    Auto,
    Context,
    Unified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOnly {
    Ignore,
    Warn,
    Fail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeStyle {
    Merge,
    Diff3,
}

/// Estilo de citação de nomes (`--quoting-style` e `QUOTING_STYLE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quoting {
    Literal,
    Shell,
    ShellAlways,
    C,
    Escape,
}

#[derive(Clone, Debug)]
pub struct Opts {
    pub strip: Option<usize>,
    pub fuzz: usize,
    pub ignore_whitespace: bool,
    pub forced: Option<ForcedFormat>,
    pub forward: bool,
    pub reverse: bool,
    pub input: Option<Vec<u8>>,
    pub output: Option<Vec<u8>>,
    pub reject_file: Option<Vec<u8>>,
    pub ifdef: Option<Vec<u8>>,
    pub merge: Option<MergeStyle>,
    pub remove_empty: bool,
    pub set_time: bool,
    pub set_utc: bool,
    pub quoting: Quoting,
    pub backup: bool,
    /// `None`: o padrão (fazer backup quando não casa exato, fora do modo POSIX).
    pub backup_if_mismatch: Option<bool>,
    pub version_control: Option<VersionControl>,
    pub prefix: Option<Vec<u8>>,
    pub basename_prefix: Option<Vec<u8>>,
    pub suffix: Option<Vec<u8>>,
    pub batch: bool,
    pub force: bool,
    pub silent: bool,
    pub verbose: bool,
    pub dry_run: bool,
    pub posix: bool,
    pub directory: Option<Vec<u8>>,
    pub reject_format: RejectFormat,
    pub binary: bool,
    pub read_only: ReadOnly,
    pub follow_symlinks: bool,
    pub positional: Vec<Vec<u8>>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            strip: None,
            fuzz: 2,
            ignore_whitespace: false,
            forced: None,
            forward: false,
            reverse: false,
            input: None,
            output: None,
            reject_file: None,
            ifdef: None,
            merge: None,
            remove_empty: false,
            set_time: false,
            set_utc: false,
            quoting: Quoting::Shell,
            backup: false,
            backup_if_mismatch: None,
            version_control: None,
            prefix: None,
            basename_prefix: None,
            suffix: None,
            batch: false,
            force: false,
            silent: false,
            verbose: false,
            dry_run: false,
            posix: false,
            directory: None,
            reject_format: RejectFormat::Auto,
            binary: false,
            read_only: ReadOnly::Warn,
            follow_symlinks: false,
            positional: Vec::new(),
        }
    }
}

const BACKUP_IF_MISMATCH: i32 = 300;
const NO_BACKUP_IF_MISMATCH: i32 = 301;
const POSIX: i32 = 302;
const QUOTING_STYLE: i32 = 303;
const REJECT_FORMAT: i32 = 304;
const READ_ONLY: i32 = 305;
const FOLLOW_SYMLINKS: i32 = 306;
const DRY_RUN: i32 = 307;
const VERBOSE: i32 = 308;
const BINARY: i32 = 309;
const MERGE: i32 = 310;
const HELP: i32 = 311;
const DEBUG: i32 = 312;

/// Ordem do GNU patch 2.8, deduzida das mensagens de ambiguidade do oráculo.
const LONGS: &[LongOpt] = &[
    LongOpt::new("backup", HasArg::No, b'b' as i32),
    LongOpt::new("prefix", HasArg::Required, b'B' as i32),
    LongOpt::new("context", HasArg::No, b'c' as i32),
    LongOpt::new("directory", HasArg::Required, b'd' as i32),
    LongOpt::new("ifdef", HasArg::Required, b'D' as i32),
    LongOpt::new("ed", HasArg::No, b'e' as i32),
    LongOpt::new("remove-empty-files", HasArg::No, b'E' as i32),
    LongOpt::new("force", HasArg::No, b'f' as i32),
    LongOpt::new("fuzz", HasArg::Required, b'F' as i32),
    LongOpt::new("get", HasArg::Required, b'g' as i32),
    LongOpt::new("input", HasArg::Required, b'i' as i32),
    LongOpt::new("ignore-whitespace", HasArg::No, b'l' as i32),
    LongOpt::new("normal", HasArg::No, b'n' as i32),
    LongOpt::new("forward", HasArg::No, b'N' as i32),
    LongOpt::new("output", HasArg::Required, b'o' as i32),
    LongOpt::new("strip", HasArg::Required, b'p' as i32),
    LongOpt::new("reject-file", HasArg::Required, b'r' as i32),
    LongOpt::new("reverse", HasArg::No, b'R' as i32),
    LongOpt::new("quiet", HasArg::No, b's' as i32),
    LongOpt::new("silent", HasArg::No, b's' as i32),
    LongOpt::new("batch", HasArg::No, b't' as i32),
    LongOpt::new("set-time", HasArg::No, b'T' as i32),
    LongOpt::new("unified", HasArg::No, b'u' as i32),
    LongOpt::new("version", HasArg::No, b'v' as i32),
    LongOpt::new("version-control", HasArg::Required, b'V' as i32),
    LongOpt::new("debug", HasArg::Required, DEBUG),
    LongOpt::new("basename-prefix", HasArg::Required, b'Y' as i32),
    LongOpt::new("suffix", HasArg::Required, b'z' as i32),
    LongOpt::new("set-utc", HasArg::No, b'Z' as i32),
    LongOpt::new("dry-run", HasArg::No, DRY_RUN),
    LongOpt::new("verbose", HasArg::No, VERBOSE),
    LongOpt::new("binary", HasArg::No, BINARY),
    LongOpt::new("help", HasArg::No, HELP),
    LongOpt::new("backup-if-mismatch", HasArg::No, BACKUP_IF_MISMATCH),
    LongOpt::new("no-backup-if-mismatch", HasArg::No, NO_BACKUP_IF_MISMATCH),
    LongOpt::new("posix", HasArg::No, POSIX),
    LongOpt::new("quoting-style", HasArg::Required, QUOTING_STYLE),
    LongOpt::new("reject-format", HasArg::Required, REJECT_FORMAT),
    LongOpt::new("read-only", HasArg::Required, READ_ONLY),
    LongOpt::new("follow-symlinks", HasArg::No, FOLLOW_SYMLINKS),
    LongOpt::new("merge", HasArg::Optional, MERGE),
];

const SHORTS: &str = "bB:cd:D:eEfF:g:i:lnNo:p:r:RstTuvV:Y:z:Z";

/// O que fazer depois de analisar a linha de comando.
pub enum Parsed {
    Run(Box<Opts>),
    /// Sai com o código dado depois de escrever `stdout` e `stderr`.
    Exit { code: i32, stdout: Vec<u8>, stderr: Vec<u8> },
}

fn try_help(argv0: &str) -> String {
    format!("{argv0}: Try '{argv0} --help' for more information.\n")
}

fn fatal(argv0: &str, msg: &str) -> Parsed {
    Parsed::Exit { code: 2, stdout: Vec::new(), stderr: format!("{argv0}: **** {msg}\n").into_bytes() }
}

fn usage_error(argv0: &str, msg: &str) -> Parsed {
    Parsed::Exit { code: 2, stdout: Vec::new(), stderr: format!("{msg}{}", try_help(argv0)).into_bytes() }
}

/// Número de opção como o GNU patch lê: só dígitos; negativo e lixo têm mensagem própria. Valores
/// enormes saturam.
fn number(arg: &[u8], what: &str) -> Result<usize, String> {
    let s = String::from_utf8_lossy(arg);
    if let Some(rest) = s.strip_prefix('-')
        && !rest.is_empty()
        && rest.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(format!("{what} {s} is negative"));
    }
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("{what} {s} is not a number"));
    }
    let mut n: usize = 0;
    for b in s.bytes() {
        n = n.saturating_mul(10).saturating_add((b - b'0') as usize);
    }
    Ok(n)
}

/// `-V` e `VERSION_CONTROL`, com abreviação como o `argmatch` do gnulib.
pub fn parse_version_control(v: &[u8]) -> Option<VersionControl> {
    const TABLE: &[(&str, VersionControl)] = &[
        ("none", VersionControl::Numbered),
        ("off", VersionControl::Numbered),
        ("simple", VersionControl::Simple),
        ("never", VersionControl::Simple),
        ("existing", VersionControl::Existing),
        ("nil", VersionControl::Existing),
        ("numbered", VersionControl::Numbered),
        ("t", VersionControl::Numbered),
    ];
    if v.is_empty() {
        return None;
    }
    if let Some((_, vc)) = TABLE.iter().find(|(n, _)| n.as_bytes() == v) {
        return Some(*vc);
    }
    let hits: Vec<VersionControl> =
        TABLE.iter().filter(|(n, _)| n.as_bytes().starts_with(v)).map(|(_, vc)| *vc).collect();
    match hits.first() {
        Some(first) if hits.iter().all(|h| h == first) => Some(*first),
        _ => None,
    }
}

fn version_control_error(argv0: &str, v: &[u8], ambiguous: bool) -> Parsed {
    let kind = if ambiguous { "ambiguous" } else { "invalid" };
    let msg = format!(
        "{argv0}: {kind} argument '{}' for '--version-control or -V option'\nValid arguments are:\n  - 'none', 'off'\n  - 'simple', 'never'\n  - 'existing', 'nil'\n  - 'numbered', 't'\n",
        String::from_utf8_lossy(v)
    );
    Parsed::Exit { code: 2, stdout: Vec::new(), stderr: msg.into_bytes() }
}

fn is_ambiguous_vc(v: &[u8]) -> bool {
    !v.is_empty()
        && ["none", "off", "simple", "never", "existing", "nil", "numbered", "t"]
            .iter()
            .filter(|n| n.as_bytes().starts_with(v))
            .count()
            > 1
}

fn parse_quoting(v: &[u8]) -> Option<Quoting> {
    match v {
        b"literal" => Some(Quoting::Literal),
        b"shell" => Some(Quoting::Shell),
        b"shell-always" => Some(Quoting::ShellAlways),
        b"c" => Some(Quoting::C),
        b"escape" => Some(Quoting::Escape),
        _ => None,
    }
}

pub const VERSION_TEXT: &str = "GNU patch 2.8
Copyright 1989-2025 Free Software Foundation, Inc.
Copyright 1984-1988 Larry Wall

License GPLv3+: GNU GPL version 3 or later <http://gnu.org/licenses/gpl.html>.
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.

Written by Larry Wall and Paul Eggert
";

pub const HELP_TEXT: &str = "Usage: patch [OPTION]... [ORIGFILE [PATCHFILE]]

Input options:

  -p NUM  --strip=NUM  Strip NUM leading components from file names.
  -F LINES  --fuzz LINES  Set the fuzz factor to LINES for inexact matching.
  -l  --ignore-whitespace  Ignore white space changes between patch and input.

  -c  --context  Interpret the patch as a context difference.
  -e  --ed  Interpret the patch as an ed script.
  -n  --normal  Interpret the patch as a normal difference.
  -u  --unified  Interpret the patch as a unified difference.

  -N  --forward  Ignore patches that appear to be reversed or already applied.
  -R  --reverse  Assume patches were created with old and new files swapped.

  -i PATCHFILE  --input=PATCHFILE  Read patch from PATCHFILE instead of stdin.

Output options:

  -o FILE  --output=FILE  Output patched files to FILE.
  -r FILE  --reject-file=FILE  Output rejects to FILE.

  -D NAME  --ifdef=NAME  Make merged if-then-else output using NAME.
  --merge  Merge using conflict markers instead of creating reject files.
  -E  --remove-empty-files  Remove output files that are empty after patching.

  -Z  --set-utc  Set times of patched files, assuming diff uses UTC (GMT).
  -T  --set-time  Likewise, assuming local time.

  --quoting-style=WORD   output file names using quoting style WORD.
    Valid WORDs are: literal, shell, shell-always, c, escape.
    Default is taken from QUOTING_STYLE env variable, or 'shell' if unset.

Backup and version control options:

  -b  --backup  Back up the original contents of each file.
  --backup-if-mismatch  Back up if the patch does not match exactly.
  --no-backup-if-mismatch  Back up mismatches only if otherwise requested.

  -V STYLE  --version-control=STYLE  Use STYLE version control.
\tSTYLE is either 'simple', 'numbered', or 'existing'.
  -B PREFIX  --prefix=PREFIX  Prepend PREFIX to backup file names.
  -Y PREFIX  --basename-prefix=PREFIX  Prepend PREFIX to backup file basenames.
  -z SUFFIX  --suffix=SUFFIX  Append SUFFIX to backup file names.

  -g NUM  --get=NUM  Get files from RCS etc. if positive; ask if negative.

Miscellaneous options:

  -t  --batch  Ask no questions; skip bad-Prereq patches; assume reversed.
  -f  --force  Like -t, but ignore bad-Prereq patches, and assume unreversed.
  -s  --quiet  --silent  Work silently unless an error occurs.
  --verbose  Output extra information about the work being done.
  --dry-run  Do not actually change any files; just print what would happen.
  --posix  Conform to the POSIX standard.

  -d DIR  --directory=DIR  Change the working directory to DIR first.
  --reject-format=FORMAT  Create 'context' or 'unified' rejects.
  --binary  Read and write data in binary mode.
  --read-only=BEHAVIOR  How to handle read-only input files: 'ignore' that they
                        are read-only, 'warn' (default), or 'fail'.

  -v  --version  Output version info.
  --help  Output this help.

Report bugs to <bug-patch@gnu.org>.
";

/// Analisa argv (argv[0] incluído). `env` dá as variáveis que mudam o padrão das opções.
pub fn parse(argv: &[Vec<u8>], env: &dyn Fn(&str) -> Option<Vec<u8>>) -> Parsed {
    let argv0 = crate::sysutil::argv0(argv);
    let mut o = Opts::default();
    if env("POSIXLY_CORRECT").is_some() {
        o.posix = true;
    }
    if let Some(q) = env("QUOTING_STYLE")
        && let Some(style) = parse_quoting(&q)
    {
        o.quoting = style;
    }
    let mut vc_arg: Option<Vec<u8>> = None;
    let getopt = Getopt::new(argv, SHORTS, LONGS, o.posix).after_argv0();
    for item in getopt {
        let item = match item {
            Ok(i) => i,
            Err(e) => return Parsed::Exit { code: 2, stdout: Vec::new(), stderr: [e.message_line(&argv0), try_help(&argv0).into_bytes()].concat() },
        };
        let opt = match item {
            Item::Operand(a) => {
                o.positional.push(a);
                continue;
            }
            Item::Opt(opt) => opt,
        };
        let arg = opt.arg.clone().unwrap_or_default();
        match opt.id {
            x if x == b'b' as i32 => o.backup = true,
            x if x == b'B' as i32 => {
                if arg.is_empty() {
                    return fatal(&argv0, "backup prefix is empty");
                }
                o.prefix = Some(arg);
            }
            x if x == b'c' as i32 => o.forced = Some(ForcedFormat::Context),
            x if x == b'd' as i32 => o.directory = Some(arg),
            x if x == b'D' as i32 => o.ifdef = Some(arg),
            x if x == b'e' as i32 => o.forced = Some(ForcedFormat::Ed),
            x if x == b'E' as i32 => o.remove_empty = true,
            x if x == b'f' as i32 => o.force = true,
            x if x == b'F' as i32 => match number(&arg, "fuzz factor") {
                Ok(n) => o.fuzz = n,
                Err(m) => return fatal(&argv0, &m),
            },
            x if x == b'g' as i32 => {
                if let Err(m) = number(&arg, "get option value").or_else(|_| {
                    let s = String::from_utf8_lossy(&arg);
                    s.parse::<i64>().map(|_| 0).map_err(|_| format!("get option value {s} is not a number"))
                }) {
                    return fatal(&argv0, &m);
                }
            }
            x if x == b'i' as i32 => o.input = Some(arg),
            x if x == b'l' as i32 => o.ignore_whitespace = true,
            x if x == b'n' as i32 => o.forced = Some(ForcedFormat::Normal),
            x if x == b'N' as i32 => o.forward = true,
            x if x == b'o' as i32 => o.output = Some(arg),
            x if x == b'p' as i32 => match number(&arg, "strip count") {
                Ok(n) => o.strip = Some(n),
                Err(m) => return fatal(&argv0, &m),
            },
            x if x == b'r' as i32 => o.reject_file = Some(arg),
            x if x == b'R' as i32 => o.reverse = true,
            x if x == b's' as i32 => o.silent = true,
            x if x == b't' as i32 => o.batch = true,
            x if x == b'T' as i32 => o.set_time = true,
            x if x == b'u' as i32 => o.forced = Some(ForcedFormat::Unified),
            x if x == b'v' as i32 => {
                return Parsed::Exit { code: 0, stdout: VERSION_TEXT.as_bytes().to_vec(), stderr: Vec::new() };
            }
            x if x == b'V' as i32 => vc_arg = Some(arg),
            x if x == b'Y' as i32 => {
                if arg.is_empty() {
                    return fatal(&argv0, "backup basename prefix is empty");
                }
                o.basename_prefix = Some(arg);
            }
            x if x == b'z' as i32 => {
                if arg.is_empty() {
                    return fatal(&argv0, "backup suffix is empty");
                }
                o.suffix = Some(arg);
            }
            x if x == b'Z' as i32 => o.set_utc = true,
            DRY_RUN => o.dry_run = true,
            VERBOSE => o.verbose = true,
            BINARY => o.binary = true,
            HELP => return Parsed::Exit { code: 0, stdout: HELP_TEXT.as_bytes().to_vec(), stderr: Vec::new() },
            BACKUP_IF_MISMATCH => o.backup_if_mismatch = Some(true),
            NO_BACKUP_IF_MISMATCH => o.backup_if_mismatch = Some(false),
            POSIX => o.posix = true,
            QUOTING_STYLE => match parse_quoting(&arg) {
                Some(q) => o.quoting = q,
                None => {
                    return usage_error(
                        &argv0,
                        &format!("{argv0}: invalid argument '{}' for 'quoting style'\n", String::from_utf8_lossy(&arg)),
                    );
                }
            },
            REJECT_FORMAT => match arg.as_slice() {
                b"context" => o.reject_format = RejectFormat::Context,
                b"unified" => o.reject_format = RejectFormat::Unified,
                _ => return usage_error(&argv0, ""),
            },
            READ_ONLY => match arg.as_slice() {
                b"ignore" => o.read_only = ReadOnly::Ignore,
                b"warn" => o.read_only = ReadOnly::Warn,
                b"fail" => o.read_only = ReadOnly::Fail,
                _ => return usage_error(&argv0, ""),
            },
            FOLLOW_SYMLINKS => o.follow_symlinks = true,
            MERGE => match opt.arg.as_deref() {
                None | Some(b"merge") => o.merge = Some(MergeStyle::Merge),
                Some(b"diff3") => o.merge = Some(MergeStyle::Diff3),
                Some(_) => return usage_error(&argv0, ""),
            },
            DEBUG => {}
            _ => return usage_error(&argv0, ""),
        }
    }
    if o.positional.len() > 2 {
        let extra = String::from_utf8_lossy(&o.positional[2]).into_owned();
        return usage_error(&argv0, &format!("{argv0}: {extra}: extra operand\n"));
    }
    let vc = vc_arg.clone().or_else(|| env("PATCH_VERSION_CONTROL")).or_else(|| env("VERSION_CONTROL"));
    if let Some(v) = vc {
        match parse_version_control(&v) {
            Some(style) => o.version_control = Some(style),
            None if vc_arg.is_some() => return version_control_error(&argv0, &v, is_ambiguous_vc(&v)),
            None => {}
        }
    }
    if o.suffix.is_none()
        && let Some(s) = env("SIMPLE_BACKUP_SUFFIX")
        && !s.is_empty()
    {
        o.suffix = Some(s);
    }
    Parsed::Run(Box::new(o))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(v: &[&str]) -> Vec<Vec<u8>> {
        v.iter().map(|s| s.as_bytes().to_vec()).collect()
    }

    fn no_env(_: &str) -> Option<Vec<u8>> {
        None
    }

    fn err(v: &[&str]) -> String {
        match parse(&argv(v), &no_env) {
            Parsed::Exit { stderr, .. } => String::from_utf8(stderr).unwrap(),
            Parsed::Run(_) => panic!("esperava erro"),
        }
    }

    #[test]
    fn ambiguity_lists_follow_gnu_order() {
        assert_eq!(
            err(&["patch", "--b"]),
            "patch: option '--b' is ambiguous; possibilities: '--backup' '--batch' '--basename-prefix' '--binary' '--backup-if-mismatch'\npatch: Try 'patch --help' for more information.\n"
        );
        assert!(err(&["patch", "--r"]).contains("'--remove-empty-files' '--reject-file' '--reverse' '--reject-format' '--read-only'\n"));
        assert!(err(&["patch", "--s"]).contains("'--strip' '--silent' '--set-time' '--suffix' '--set-utc'\n"));
        assert!(err(&["patch", "--f"]).contains("'--force' '--fuzz' '--forward' '--follow-symlinks'\n"));
        assert!(err(&["patch", "--v"]).contains("'--version' '--version-control' '--verbose'\n"));
    }

    #[test]
    fn numbers_and_extra_operands() {
        assert_eq!(err(&["patch", "-px"]), "patch: **** strip count x is not a number\n");
        assert_eq!(err(&["patch", "-F", "-1"]), "patch: **** fuzz factor -1 is negative\n");
        assert_eq!(err(&["patch", "a", "b", "c"]), "patch: c: extra operand\npatch: Try 'patch --help' for more information.\n");
        assert!(err(&["patch", "-V", "bogus"]).starts_with("patch: invalid argument 'bogus' for '--version-control or -V option'\n"));
    }

    #[test]
    fn version_control_names() {
        assert_eq!(parse_version_control(b"none"), Some(VersionControl::Numbered));
        assert_eq!(parse_version_control(b"simple"), Some(VersionControl::Simple));
        assert_eq!(parse_version_control(b"exi"), Some(VersionControl::Existing));
        assert_eq!(parse_version_control(b"n"), None);
    }
}
