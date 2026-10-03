//! Front-end de flags do GNU grep 3.11: `getopt_long` com permutação, opções curtas agrupadas,
//! `-NUM`, prefixo único de opção longa, `--`, e as mensagens de erro do getopt.

use crate::fsview::FsView;

pub const USAGE: &str = "Usage: grep [OPTION]... PATTERNS [FILE]...\nTry 'grep --help' for more information.\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Basic,
    Extended,
    Fixed,
    Perl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListMode {
    WithMatch,
    WithoutMatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryMode {
    Binary,
    Text,
    WithoutMatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirMode {
    Read,
    Skip,
    Recurse,
}

#[derive(Clone, Debug)]
pub struct GrepOpts {
    pub mode: Mode,
    pub patterns: Vec<String>,
    pub icase: bool,
    pub invert: bool,
    pub word: bool,
    pub line: bool,
    pub count: bool,
    pub list: Option<ListMode>,
    pub only: bool,
    pub quiet: bool,
    pub no_messages: bool,
    pub line_number: bool,
    pub byte_offset: bool,
    pub with_filename: Option<bool>,
    pub label: Option<String>,
    pub max_count: Option<u64>,
    pub after: usize,
    pub before: usize,
    /// `Some(true)` = `-R` (segue symlinks), `Some(false)` = `-r`.
    pub recursive: Option<bool>,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub exclude_dirs: Vec<String>,
    pub binary: BinaryMode,
    pub directories: DirMode,
    pub null: bool,
    pub initial_tab: bool,
    pub null_data: bool,
    /// Separador de grupos de contexto (`--group-separator`, `None` = `--no-group-separator`).
    pub group_separator: Option<String>,
    pub files: Vec<String>,
}

/// Saída antecipada (erro de uso, `--help`, etc.).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EarlyExit {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

fn usage_error(msg: String) -> EarlyExit {
    EarlyExit { stdout: String::new(), stderr: format!("{msg}{USAGE}"), code: 2 }
}

fn fatal(msg: String) -> EarlyExit {
    EarlyExit { stdout: String::new(), stderr: format!("grep: {msg}\n"), code: 2 }
}

const SHORT_WITH_ARG: &str = "ABCDXdefm";
const SHORT_FLAGS: &str = "EFGHILPRTUVZabchiLlnoqrsuvwxyz";

/// (nome, recebe argumento: 0 não, 1 obrigatório, 2 opcional)
const LONG: &[(&str, u8)] = &[
    ("after-context", 1),
    ("basic-regexp", 0),
    ("before-context", 1),
    ("binary-files", 1),
    ("byte-offset", 0),
    ("binary", 0),
    ("color", 2),
    ("colour", 2),
    ("context", 1),
    ("count", 0),
    ("dereference-recursive", 0),
    ("devices", 1),
    ("directories", 1),
    ("exclude", 1),
    ("exclude-dir", 1),
    ("exclude-from", 1),
    ("extended-regexp", 0),
    ("file", 1),
    ("files-with-matches", 0),
    ("files-without-match", 0),
    ("fixed-strings", 0),
    ("group-separator", 1),
    ("help", 0),
    ("ignore-case", 0),
    ("include", 1),
    ("initial-tab", 0),
    ("invert-match", 0),
    ("label", 1),
    ("line-buffered", 0),
    ("line-number", 0),
    ("line-regexp", 0),
    ("max-count", 1),
    ("no-filename", 0),
    ("no-group-separator", 0),
    ("no-ignore-case", 0),
    ("no-messages", 0),
    ("null", 0),
    ("null-data", 0),
    ("only-matching", 0),
    ("perl-regexp", 0),
    ("quiet", 0),
    ("recursive", 0),
    ("regexp", 1),
    ("silent", 0),
    ("text", 0),
    ("version", 0),
    ("with-filename", 0),
    ("word-regexp", 0),
];

fn context_arg(v: &str) -> Result<usize, EarlyExit> {
    v.parse::<usize>().map_err(|_| fatal(format!("{v}: invalid context length argument")))
}

/// Analisa o argv (sem o `argv[0]`). `-f` lê os padrões da árvore do caso.
pub fn parse(args: &[String], fs: &FsView<'_>, stdin: &[u8]) -> Result<GrepOpts, EarlyExit> {
    let mut o = GrepOpts {
        mode: Mode::Basic,
        patterns: Vec::new(),
        icase: false,
        invert: false,
        word: false,
        line: false,
        count: false,
        list: None,
        only: false,
        quiet: false,
        no_messages: false,
        line_number: false,
        byte_offset: false,
        with_filename: None,
        label: None,
        max_count: None,
        after: 0,
        before: 0,
        recursive: None,
        includes: Vec::new(),
        excludes: Vec::new(),
        exclude_dirs: Vec::new(),
        binary: BinaryMode::Binary,
        directories: DirMode::Read,
        null: false,
        initial_tab: false,
        null_data: false,
        group_separator: Some("--".into()),
        files: Vec::new(),
    };
    let mut pattern_given = false;
    let mut operands: Vec<String> = Vec::new();
    let mut default_context: Option<usize> = None;
    let mut after: Option<usize> = None;
    let mut before: Option<usize> = None;
    let mut i = 0;
    let add_patterns = |o: &mut GrepOpts, text: &str| {
        o.patterns.extend(text.split('\n').map(str::to_string));
    };
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if a == "--" {
            operands.extend(args[i..].iter().cloned());
            break;
        }
        if let Some(long) = a.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let exact = LONG.iter().find(|(n, _)| *n == name);
            let candidates: Vec<&(&str, u8)> = match exact {
                Some(e) => vec![e],
                None => LONG.iter().filter(|(n, _)| n.starts_with(name)).collect(),
            };
            let (full, argk) = match candidates.as_slice() {
                [one] => (one.0, one.1),
                [] => return Err(usage_error(format!("grep: unrecognized option '{a}'\n"))),
                many => {
                    let list: Vec<String> = many.iter().map(|(n, _)| format!("'--{n}'")).collect();
                    return Err(usage_error(format!("grep: option '--{name}' is ambiguous; possibilities: {}\n", list.join(" "))));
                }
            };
            let value = match (argk, value) {
                (0, Some(_)) => return Err(usage_error(format!("grep: option '--{full}' doesn't allow an argument\n"))),
                (1, None) => {
                    if i < args.len() {
                        i += 1;
                        Some(args[i - 1].clone())
                    } else {
                        return Err(usage_error(format!("grep: option '--{full}' requires an argument\n")));
                    }
                }
                (_, v) => v,
            };
            let v = value.clone().unwrap_or_default();
            match full {
                "after-context" => after = Some(context_arg(&v)?),
                "before-context" => before = Some(context_arg(&v)?),
                "context" => default_context = Some(context_arg(&v)?),
                "basic-regexp" => o.mode = Mode::Basic,
                "extended-regexp" => o.mode = Mode::Extended,
                "fixed-strings" => o.mode = Mode::Fixed,
                "perl-regexp" => o.mode = Mode::Perl,
                "binary-files" => {
                    o.binary = match v.as_str() {
                        "binary" => BinaryMode::Binary,
                        "text" => BinaryMode::Text,
                        "without-match" => BinaryMode::WithoutMatch,
                        _ => return Err(fatal("unknown binary-files type".into())),
                    }
                }
                "byte-offset" => o.byte_offset = true,
                "count" => o.count = true,
                "dereference-recursive" => o.recursive = Some(true),
                "recursive" => o.recursive = Some(false),
                "directories" => {
                    o.directories = match v.as_str() {
                        "read" => DirMode::Read,
                        "skip" => DirMode::Skip,
                        "recurse" => DirMode::Recurse,
                        _ => return Err(fatal("invalid argument for --directories".into())),
                    }
                }
                "group-separator" => o.group_separator = Some(v),
                "no-group-separator" => o.group_separator = None,
                "exclude" => o.excludes.push(v),
                "include" => o.includes.push(v),
                "exclude-dir" => o.exclude_dirs.push(v),
                "file" => {
                    pattern_given = true;
                    read_pattern_file(&mut o, &v, fs, stdin)?;
                }
                "regexp" => {
                    pattern_given = true;
                    add_patterns(&mut o, &v);
                }
                "files-with-matches" => o.list = Some(ListMode::WithMatch),
                "files-without-match" => o.list = Some(ListMode::WithoutMatch),
                "ignore-case" => o.icase = true,
                "no-ignore-case" => o.icase = false,
                "initial-tab" => o.initial_tab = true,
                "invert-match" => o.invert = true,
                "label" => o.label = Some(v),
                "line-number" => o.line_number = true,
                "line-regexp" => o.line = true,
                "word-regexp" => o.word = true,
                "max-count" => o.max_count = Some(v.parse().map_err(|_| fatal("invalid max count".into()))?),
                "no-filename" => o.with_filename = Some(false),
                "with-filename" => o.with_filename = Some(true),
                "no-messages" => o.no_messages = true,
                "null" => o.null = true,
                "null-data" => o.null_data = true,
                "only-matching" => o.only = true,
                "quiet" | "silent" => o.quiet = true,
                "text" => o.binary = BinaryMode::Text,
                "help" => {
                    return Err(EarlyExit { stdout: USAGE.to_string(), stderr: String::new(), code: 0 });
                }
                "version" => {
                    return Err(EarlyExit { stdout: "grep (GNU grep) 3.11\n".into(), stderr: String::new(), code: 0 });
                }
                _ => {}
            }
            continue;
        }
        if a.len() > 1 && a.starts_with('-') {
            let chars: Vec<char> = a[1..].chars().collect();
            let mut j = 0;
            let mut digits = String::new();
            while j < chars.len() {
                let c = chars[j];
                j += 1;
                if c.is_ascii_digit() {
                    digits.push(c);
                    default_context = Some(digits.parse().unwrap_or(usize::MAX));
                    continue;
                }
                digits.clear();
                if SHORT_WITH_ARG.contains(c) {
                    let rest: String = chars[j..].iter().collect();
                    let v = if !rest.is_empty() {
                        rest
                    } else if i < args.len() {
                        i += 1;
                        args[i - 1].clone()
                    } else {
                        return Err(usage_error(format!("grep: option requires an argument -- '{c}'\n")));
                    };
                    match c {
                        'A' => after = Some(context_arg(&v)?),
                        'B' => before = Some(context_arg(&v)?),
                        'C' => default_context = Some(context_arg(&v)?),
                        'e' => {
                            pattern_given = true;
                            add_patterns(&mut o, &v);
                        }
                        'f' => {
                            pattern_given = true;
                            read_pattern_file(&mut o, &v, fs, stdin)?;
                        }
                        'm' => o.max_count = Some(v.parse().map_err(|_| fatal("invalid max count".into()))?),
                        'd' => {
                            o.directories = match v.as_str() {
                                "read" => DirMode::Read,
                                "skip" => DirMode::Skip,
                                "recurse" => DirMode::Recurse,
                                _ => return Err(fatal("invalid argument for --directories".into())),
                            }
                        }
                        _ => {}
                    }
                    break;
                }
                if !SHORT_FLAGS.contains(c) {
                    return Err(usage_error(format!("grep: invalid option -- '{c}'\n")));
                }
                match c {
                    'E' => o.mode = Mode::Extended,
                    'F' => o.mode = Mode::Fixed,
                    'G' => o.mode = Mode::Basic,
                    'P' => o.mode = Mode::Perl,
                    'H' => o.with_filename = Some(true),
                    'h' => o.with_filename = Some(false),
                    'I' => o.binary = BinaryMode::WithoutMatch,
                    'L' => o.list = Some(ListMode::WithoutMatch),
                    'l' => o.list = Some(ListMode::WithMatch),
                    'R' => o.recursive = Some(true),
                    'r' => o.recursive = Some(false),
                    'T' => o.initial_tab = true,
                    'Z' => o.null = true,
                    'a' => o.binary = BinaryMode::Text,
                    'b' => o.byte_offset = true,
                    'c' => o.count = true,
                    'i' | 'y' => o.icase = true,
                    'n' => o.line_number = true,
                    'o' => o.only = true,
                    'q' => o.quiet = true,
                    's' => o.no_messages = true,
                    'v' => o.invert = true,
                    'w' => o.word = true,
                    'x' => o.line = true,
                    'z' => o.null_data = true,
                    'V' => {
                        return Err(EarlyExit { stdout: "grep (GNU grep) 3.11\n".into(), stderr: String::new(), code: 0 });
                    }
                    _ => {}
                }
            }
            continue;
        }
        operands.push(a.clone());
    }
    if !pattern_given {
        if operands.is_empty() {
            return Err(EarlyExit { stdout: String::new(), stderr: USAGE.to_string(), code: 2 });
        }
        let p = operands.remove(0);
        add_patterns(&mut o, &p);
    }
    if o.directories == DirMode::Recurse && o.recursive.is_none() {
        o.recursive = Some(false);
    }
    if o.recursive.is_some() {
        o.directories = DirMode::Recurse;
    }
    o.after = after.or(default_context).unwrap_or(0);
    o.before = before.or(default_context).unwrap_or(0);
    o.files = operands;
    Ok(o)
}

fn read_pattern_file(o: &mut GrepOpts, path: &str, fs: &FsView<'_>, stdin: &[u8]) -> Result<(), EarlyExit> {
    let data = if path == "-" {
        stdin.to_vec()
    } else {
        fs.read(path).map_err(|e| fatal(format!("{path}: {}", e.message())))?.to_vec()
    };
    let text = String::from_utf8_lossy(&data);
    let body = text.strip_suffix('\n').unwrap_or(&text);
    if !data.is_empty() {
        o.patterns.extend(body.split('\n').map(str::to_string));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness::MemTree;

    fn p(args: &[&str]) -> Result<GrepOpts, EarlyExit> {
        let t = MemTree::new();
        let v: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse(&v, &FsView::new(&t), b"")
    }

    #[test]
    fn clusters_and_permutation() {
        let o = p(&["-rnE", "a|b", "src", "-i"]).unwrap();
        assert_eq!(o.mode, Mode::Extended);
        assert!(o.icase && o.line_number);
        assert_eq!(o.recursive, Some(false));
        assert_eq!(o.patterns, vec!["a|b"]);
        assert_eq!(o.files, vec!["src"]);
        let o = p(&["-m1", "-A", "2", "-C3", "x"]).unwrap();
        assert_eq!((o.max_count, o.after, o.before), (Some(1), 2, 3));
        let o = p(&["-2", "x"]).unwrap();
        assert_eq!((o.after, o.before), (2, 2));
        let o = p(&["--coun", "--regexp=x", "f"]).unwrap();
        assert!(o.count);
        assert_eq!(o.patterns, vec!["x"]);
    }

    #[test]
    fn errors_match_getopt() {
        assert_eq!(p(&["-k", "x"]).unwrap_err().stderr, format!("grep: invalid option -- 'k'\n{USAGE}"));
        assert_eq!(p(&["--frob", "x"]).unwrap_err().stderr, format!("grep: unrecognized option '--frob'\n{USAGE}"));
        assert_eq!(p(&[]).unwrap_err().code, 2);
        assert_eq!(p(&["-e"]).unwrap_err().stderr, format!("grep: option requires an argument -- 'e'\n{USAGE}"));
    }
}
