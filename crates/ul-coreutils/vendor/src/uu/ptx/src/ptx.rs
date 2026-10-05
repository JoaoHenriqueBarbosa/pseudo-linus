// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (ToDOs) corasick memchr Roff trunc oset iset CHARCLASS

// Porte pseudo-linus: o miolo do `ptx` foi reescrito pra seguir o GNU coreutils 9.7 (medido no
// oráculo): contextos por sentença sobre o arquivo inteiro, tudo em bytes, palavra padrão de letras
// ASCII, geometria das linhas de terminal e dimensionamento dos campos (ver `input`, `layout` e
// `window`). Aqui ficam as opções, a configuração e o fluxo principal.
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use sysio::fs::File;
use sysio::io::{BufWriter, Write, stdout};
use std::path::Path;

use clap::{Arg, ArgAction, Command};
use regex::bytes::Regex;
use rustc_hash::FxHashSet;
use uucore::display::Quotable;
use uucore::error::{FromIo, UResult, USimpleError, UUsageError};
use uucore::format_usage;
use uucore::translate;

mod input;
mod layout;
mod window;

use input::{
    ContextSplit, FileContent, Occurrence, WordFilter, WordMatcher, find_occurrences, read_all,
    read_contexts,
};
use layout::{Dims, Geometry, RefPlace, compute_fields, format_dumb, format_roff, format_tex};

/// GNU's regex engine treats a trailing lone backslash as a literal backslash,
/// while the `regex` crate rejects it as an incomplete escape sequence. Double
/// it so that such patterns keep working instead of erroring out.
fn escape_trailing_backslash(pattern: &str) -> String {
    let trailing = pattern.chars().rev().take_while(|&c| c == '\\').count();
    if trailing % 2 == 1 {
        format!("{pattern}\\")
    } else {
        pattern.to_owned()
    }
}

/// Porte pseudo-linus: o `ptx` do GNU converte as sequências de escape do C (`\n`, `\t`, `\xHH`,
/// `\ooo`...) em `-F`; as desconhecidas ficam como estão.
fn unescape(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' || i + 1 >= bytes.len() {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        i += 1;
        let simple = match bytes[i] {
            b'a' => Some(0x07),
            b'b' => Some(0x08),
            b'f' => Some(0x0c),
            b'n' => Some(b'\n'),
            b'r' => Some(b'\r'),
            b't' => Some(b'\t'),
            b'v' => Some(0x0b),
            b'\\' => Some(b'\\'),
            b'"' => Some(b'"'),
            b'\'' => Some(b'\''),
            b'?' => Some(b'?'),
            _ => None,
        };
        if let Some(byte) = simple {
            out.push(byte);
            i += 1;
        } else if bytes[i] == b'x' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_hexdigit() {
            let mut value = 0u32;
            let mut digits = 0;
            i += 1;
            while i < bytes.len() && digits < 2 && bytes[i].is_ascii_hexdigit() {
                value = value * 16 + char::from(bytes[i]).to_digit(16).unwrap_or(0);
                i += 1;
                digits += 1;
            }
            out.push((value & 0xff) as u8);
        } else if (b'0'..=b'7').contains(&bytes[i]) {
            let mut value = 0u32;
            let mut digits = 0;
            while i < bytes.len() && digits < 3 && (b'0'..=b'7').contains(&bytes[i]) {
                value = value * 8 + u32::from(bytes[i] - b'0');
                i += 1;
                digits += 1;
            }
            out.push((value & 0xff) as u8);
        } else {
            out.push(b'\\');
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

#[derive(Debug, PartialEq)]
enum OutFormat {
    Dumb,
    Roff,
    Tex,
}

#[derive(Debug)]
struct Config {
    format: OutFormat,
    gnu_ext: bool,
    auto_ref: bool,
    input_ref: bool,
    right_ref: bool,
    ignore_case: bool,
    macro_name: String,
    trunc_str: Vec<u8>,
    line_width: usize,
    gap_size: usize,
    sentence_regex: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            format: OutFormat::Dumb,
            gnu_ext: true,
            auto_ref: false,
            input_ref: false,
            right_ref: false,
            ignore_case: false,
            macro_name: "xx".to_owned(),
            trunc_str: b"/".to_vec(),
            line_width: 72,
            gap_size: 3,
            sentence_regex: None,
        }
    }
}

/// Lê um arquivo de palavras (um por linha) de `-i` ou `-o`.
fn read_word_set(matches: &clap::ArgMatches, option: &str) -> UResult<FxHashSet<Vec<u8>>> {
    let filename = matches
        .get_one::<OsString>(option)
        .expect("parsing options failed!");
    let data = read_all(filename)?;
    let mut words: FxHashSet<Vec<u8>> = FxHashSet::default();
    let mut lines: Vec<&[u8]> = data.split(|&b| b == b'\n').collect();
    // O que vem depois da última quebra de linha não é uma linha.
    if lines.last().is_some_and(|last| last.is_empty()) {
        lines.pop();
    }
    for line in lines {
        words.insert(line.to_vec());
    }
    Ok(words)
}

fn compile_regex(pattern: &str) -> UResult<Regex> {
    Regex::new(pattern).map_err(|error| {
        USimpleError::new(
            1,
            translate!("ptx-error-invalid-regexp", "error" => error.to_string()),
        )
    })
}

fn build_filter(matches: &clap::ArgMatches) -> UResult<WordFilter> {
    let only = if matches.contains_id(options::ONLY_FILE) {
        Some(read_word_set(matches, options::ONLY_FILE)?)
    } else {
        None
    };
    let ignore = if matches.contains_id(options::IGNORE_FILE) {
        Some(read_word_set(matches, options::IGNORE_FILE)?)
    } else {
        None
    };
    Ok(WordFilter { only, ignore })
}

/// O reconhecedor de palavras: `-W` ganha de `-b`, que ganha do padrão do modo escolhido.
fn build_matcher(matches: &clap::ArgMatches, config: &Config) -> UResult<WordMatcher> {
    let word_regexp = matches
        .get_one::<String>(options::WORD_REGEXP)
        .filter(|v| !v.is_empty());
    if let Some(word_regexp) = word_regexp {
        return Ok(WordMatcher::Pattern(compile_regex(
            &escape_trailing_backslash(word_regexp),
        )?));
    }
    if matches.contains_id(options::BREAK_FILE) && !matches.contains_id(options::WORD_REGEXP) {
        let filename = matches
            .get_one::<OsString>(options::BREAK_FILE)
            .expect("parsing options failed!");
        let mut breaks: Vec<u8> = read_all(filename)?;
        if !config.gnu_ext {
            // GNU off means at least these are considered
            breaks.extend_from_slice(b" \t\n");
        }
        breaks.sort_unstable();
        breaks.dedup();
        let pattern = if breaks.is_empty() {
            "(?s-u:.)+".to_owned()
        } else {
            let mut pattern = String::from("(?-u)[^");
            for byte in &breaks {
                let _ = write!(pattern, "\\x{byte:02x}");
            }
            pattern.push_str("]+");
            pattern
        };
        return Ok(WordMatcher::Pattern(compile_regex(&pattern)?));
    }
    Ok(if config.gnu_ext {
        WordMatcher::Letters
    } else {
        WordMatcher::NonSpace
    })
}

/// A divisão em contextos: sentenças por padrão, linhas com `-r` ou `-G`.
fn build_split(config: &Config) -> UResult<ContextSplit> {
    match &config.sentence_regex {
        Some(regex) if regex.is_empty() => Ok(ContextSplit::Whole),
        Some(regex) => Ok(ContextSplit::Custom(compile_regex(regex)?)),
        None if config.input_ref || !config.gnu_ext => Ok(ContextSplit::Lines),
        None => Ok(ContextSplit::Sentences),
    }
}

fn positive_number(
    matches: &clap::ArgMatches,
    option: &str,
    message: impl Fn(String) -> String,
) -> UResult<Option<usize>> {
    let Some(text) = matches.get_one::<String>(option) else {
        return Ok(None);
    };
    match text.parse::<usize>() {
        Ok(n) if n > 0 => Ok(Some(n)),
        _ => Err(USimpleError::new(
            1,
            message(uucore::display::locale_quote(text.as_str())),
        )),
    }
}

fn get_config(matches: &mut clap::ArgMatches) -> UResult<Config> {
    let mut config = Config::default();
    let err_msg = "parsing options failed";
    if matches.get_flag(options::TRADITIONAL) {
        config.gnu_ext = false;
        config.format = OutFormat::Roff;
        // Porte pseudo-linus: sem as extensões do GNU a referência vai sempre pra direita.
        config.right_ref = true;
    }
    if let Some(regex) = matches
        .remove_one::<String>(options::SENTENCE_REGEXP)
        .map(|r| escape_trailing_backslash(&r))
    {
        // Verify regex is valid and doesn't match empty string (an empty one turns the sentence
        // detection off: the whole file is a single context).
        if !regex.is_empty() {
            let re = compile_regex(&regex)?;
            if re.is_match(b"") {
                return Err(USimpleError::new(1, translate!("ptx-error-empty-regexp")));
            }
        }
        config.sentence_regex = Some(regex);
    }
    config.auto_ref = matches.get_flag(options::AUTO_REFERENCE);
    config.input_ref = matches.get_flag(options::REFERENCES);
    config.right_ref |= matches.get_flag(options::RIGHT_SIDE_REFS);
    config.ignore_case = matches.get_flag(options::IGNORE_CASE);
    if matches.contains_id(options::MACRO_NAME) {
        matches
            .get_one::<String>(options::MACRO_NAME)
            .expect(err_msg)
            .clone_into(&mut config.macro_name);
    }
    if matches.contains_id(options::FLAG_TRUNCATION) {
        config.trunc_str = unescape(
            matches
                .get_one::<String>(options::FLAG_TRUNCATION)
                .expect(err_msg),
        );
    }
    if let Some(width) = positive_number(matches, options::WIDTH, |value| {
        translate!("ptx-error-invalid-line-width", "width" => value)
    })? {
        config.line_width = width;
    } else if matches.get_flag(options::TYPESET_MODE) {
        config.line_width = 100;
    }
    if let Some(gap) = positive_number(matches, options::GAP_SIZE, |value| {
        translate!("ptx-error-invalid-gap-width", "gap" => value)
    })? {
        config.gap_size = gap;
    }
    if let Some(format) = matches.get_one::<String>(options::FORMAT) {
        config.format = match format.as_str() {
            "roff" => OutFormat::Roff,
            "tex" => OutFormat::Tex,
            _ => unreachable!("should be caught by clap"),
        };
    }
    if matches.get_flag(options::format::ROFF) {
        config.format = OutFormat::Roff;
    }
    if matches.get_flag(options::format::TEX) {
        config.format = OutFormat::Tex;
    }
    Ok(config)
}

/// A referência de uma ocorrência: `arquivo:linha` (vazio o arquivo na entrada padrão) com `-A`, ou
/// a primeira palavra da linha com `-r`.
fn reference_of(config: &Config, files: &[FileContent], occurrence: &Occurrence) -> Vec<u8> {
    let file = &files[occurrence.file];
    let context = &file.contexts[occurrence.context];
    if config.auto_ref {
        let mut reference = if file.name.as_os_str() == OsStr::new("-") {
            Vec::new()
        } else {
            file.name.as_encoded_bytes().to_vec()
        };
        reference.push(b':');
        reference
            .extend_from_slice(file.line_of(context, occurrence.position).to_string().as_bytes());
        reference
    } else if config.input_ref {
        context.text[..context.ref_end].to_vec()
    } else {
        Vec::new()
    }
}

/// Escreve o índice permutado em `output_filename`.
fn write_output(
    config: &Config,
    files: &[FileContent],
    occurrences: &[Occurrence],
    max_word: usize,
    output_filename: &OsStr,
) -> UResult<()> {
    let mut writer: BufWriter<Box<dyn Write>> =
        BufWriter::new(if output_filename == OsStr::new("-") {
            Box::new(stdout())
        } else {
            let file = File::create(Path::new(output_filename))
                .map_err_context(|| output_filename.maybe_quote().to_string())?;
            Box::new(file)
        });

    let has_refs = config.auto_ref || config.input_ref;
    let left_ref = has_refs && !config.right_ref;
    let references: Vec<Vec<u8>> = occurrences
        .iter()
        .map(|occurrence| {
            if has_refs {
                reference_of(config, files, occurrence)
            } else {
                Vec::new()
            }
        })
        .collect();

    // A referência à esquerda e o espaço depois dela saem da largura da linha; à direita ela não
    // conta. Sem referência a linha leva só o espaço de margem.
    let ref_width = if left_ref {
        references.iter().map(Vec::len).max().unwrap_or(0)
    } else {
        0
    };
    let inner_width = if left_ref {
        config.line_width.saturating_sub(ref_width + config.gap_size)
    } else {
        config.line_width
    };
    let half = inner_width / 2;
    let geometry = Geometry {
        margin: if config.right_ref {
            0
        } else {
            ref_width + config.gap_size
        },
        inner_width,
        half,
        gap: config.gap_size,
        width: config.line_width,
    };
    let dims = Dims {
        half,
        gap: config.gap_size,
        trunc: &config.trunc_str,
        flags: config.format != OutFormat::Tex,
        max_word,
    };

    for (occurrence, reference) in occurrences.iter().zip(&references) {
        let context = &files[occurrence.file].contexts[occurrence.context];
        let end = occurrence.position + occurrence.len;
        let fields = compute_fields(
            &dims,
            &context.text[context.text_start..occurrence.position],
            &context.text[occurrence.position..end],
            &context.text[end..],
        );
        let line = match config.format {
            OutFormat::Dumb => {
                let place = if !has_refs {
                    RefPlace::None
                } else if config.right_ref {
                    RefPlace::Right { text: reference }
                } else {
                    RefPlace::Left {
                        text: reference,
                        colon: config.auto_ref,
                    }
                };
                format_dumb(&geometry, &fields, &place)
            }
            OutFormat::Roff => format_roff(&config.macro_name, &fields, has_refs.then_some(reference.as_slice())),
            OutFormat::Tex => format_tex(&config.macro_name, &fields, has_refs.then_some(reference.as_slice())),
        };
        writer
            .write_all(&line)
            .and_then(|()| writer.write_all(b"\n"))
            .map_err_context(|| translate!("ptx-error-write-failed"))?;
    }

    writer
        .flush()
        .map_err_context(|| translate!("ptx-error-write-failed"))?;

    Ok(())
}

mod options {
    pub mod format {
        pub static ROFF: &str = "roff";
        pub static TEX: &str = "tex";
    }

    pub static FILE: &str = "file";
    pub static AUTO_REFERENCE: &str = "auto-reference";
    pub static TRADITIONAL: &str = "traditional";
    pub static FLAG_TRUNCATION: &str = "flag-truncation";
    pub static MACRO_NAME: &str = "macro-name";
    pub static FORMAT: &str = "format";
    pub static RIGHT_SIDE_REFS: &str = "right-side-refs";
    pub static SENTENCE_REGEXP: &str = "sentence-regexp";
    pub static WORD_REGEXP: &str = "word-regexp";
    pub static BREAK_FILE: &str = "break-file";
    pub static IGNORE_CASE: &str = "ignore-case";
    pub static GAP_SIZE: &str = "gap-size";
    pub static IGNORE_FILE: &str = "ignore-file";
    pub static ONLY_FILE: &str = "only-file";
    pub static REFERENCES: &str = "references";
    pub static TYPESET_MODE: &str = "typeset-mode";
    pub static WIDTH: &str = "width";
}

#[uucore::main]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let mut matches = uucore::clap_localization::handle_clap_result(uu_app(), args)?;
    let config = get_config(&mut matches)?;

    let input_files;
    let output_file: OsString;

    let mut files = matches
        .get_many::<OsString>(options::FILE)
        .into_iter()
        .flatten()
        .cloned();

    if config.gnu_ext {
        input_files = {
            let mut files = files.collect::<Vec<_>>();
            if files.is_empty() {
                files.push(OsString::from("-"));
            }
            files
        };
        output_file = OsString::from("-");
    } else {
        input_files = vec![files.next().unwrap_or(OsString::from("-"))];
        output_file = files.next().unwrap_or(OsString::from("-"));
        if let Some(file) = files.next() {
            return Err(UUsageError::new(
                1,
                translate!("ptx-error-extra-operand", "operand" => uucore::display::locale_quote(&file)),
            ));
        }
    }

    let filter = build_filter(&matches)?;
    let matcher = build_matcher(&matches, &config)?;
    let split = build_split(&config)?;

    let mut contents = Vec::with_capacity(input_files.len());
    for name in &input_files {
        let data = read_all(name)?;
        contents.push(read_contexts(name.clone(), &data, &split, config.input_ref));
    }

    let (occurrences, max_word) =
        find_occurrences(&contents, &matcher, &filter, config.ignore_case);
    write_output(&config, &contents, &occurrences, max_word, &output_file)
}

pub fn uu_app() -> Command {
    Command::new("ptx")
        .about(translate!("ptx-about"))
        .version(uucore::crate_version!())
        .help_template(uucore::localized_help_template("ptx"))
        .override_usage(format_usage(&translate!("ptx-usage")))
        .infer_long_args(true)
        .arg(
            Arg::new(options::FILE)
                .hide(true)
                .action(ArgAction::Append)
                .value_hint(clap::ValueHint::FilePath)
                .value_parser(clap::value_parser!(OsString)),
        )
        .arg(
            Arg::new(options::AUTO_REFERENCE)
                .short('A')
                .long(options::AUTO_REFERENCE)
                .help(translate!("ptx-help-auto-reference"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::TRADITIONAL)
                .short('G')
                .long(options::TRADITIONAL)
                .help(translate!("ptx-help-traditional"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::FLAG_TRUNCATION)
                .short('F')
                .long(options::FLAG_TRUNCATION)
                .help(translate!("ptx-help-flag-truncation"))
                .value_name("STRING"),
        )
        .arg(
            Arg::new(options::MACRO_NAME)
                .short('M')
                .long(options::MACRO_NAME)
                .help(translate!("ptx-help-macro-name"))
                .value_name("STRING"),
        )
        .arg(
            Arg::new(options::FORMAT)
                .long(options::FORMAT)
                .hide(true)
                .value_parser(["roff", "tex"])
                .overrides_with_all([options::FORMAT, options::format::ROFF, options::format::TEX]),
        )
        .arg(
            Arg::new(options::format::ROFF)
                .short('O')
                .help(translate!("ptx-help-roff"))
                .overrides_with_all([options::FORMAT, options::format::ROFF, options::format::TEX])
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::format::TEX)
                .short('T')
                .help(translate!("ptx-help-tex"))
                .overrides_with_all([options::FORMAT, options::format::ROFF, options::format::TEX])
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::RIGHT_SIDE_REFS)
                .short('R')
                .long(options::RIGHT_SIDE_REFS)
                .help(translate!("ptx-help-right-side-refs"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::SENTENCE_REGEXP)
                .short('S')
                .long(options::SENTENCE_REGEXP)
                .help(translate!("ptx-help-sentence-regexp"))
                .value_name("REGEXP"),
        )
        .arg(
            Arg::new(options::WORD_REGEXP)
                .short('W')
                .long(options::WORD_REGEXP)
                .help(translate!("ptx-help-word-regexp"))
                .value_name("REGEXP"),
        )
        .arg(
            Arg::new(options::BREAK_FILE)
                .short('b')
                .long(options::BREAK_FILE)
                .help(translate!("ptx-help-break-file"))
                .value_name("FILE")
                .value_hint(clap::ValueHint::FilePath)
                .value_parser(clap::value_parser!(OsString)),
        )
        .arg(
            Arg::new(options::IGNORE_CASE)
                .short('f')
                .long(options::IGNORE_CASE)
                .help(translate!("ptx-help-ignore-case"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::GAP_SIZE)
                .short('g')
                .long(options::GAP_SIZE)
                .allow_hyphen_values(true)
                .help(translate!("ptx-help-gap-size"))
                .value_name("NUMBER"),
        )
        .arg(
            Arg::new(options::IGNORE_FILE)
                .short('i')
                .long(options::IGNORE_FILE)
                .help(translate!("ptx-help-ignore-file"))
                .value_name("FILE")
                .value_hint(clap::ValueHint::FilePath)
                .value_parser(clap::value_parser!(OsString)),
        )
        .arg(
            Arg::new(options::ONLY_FILE)
                .short('o')
                .long(options::ONLY_FILE)
                .help(translate!("ptx-help-only-file"))
                .value_name("FILE")
                .value_hint(clap::ValueHint::FilePath)
                .value_parser(clap::value_parser!(OsString)),
        )
        .arg(
            Arg::new(options::REFERENCES)
                .short('r')
                .long(options::REFERENCES)
                .help(translate!("ptx-help-references"))
                .value_name("FILE")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::TYPESET_MODE)
                .short('t')
                .long(options::TYPESET_MODE)
                .help(translate!("ptx-help-typeset-mode"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::WIDTH)
                .short('w')
                .long(options::WIDTH)
                .allow_hyphen_values(true)
                .help(translate!("ptx-help-width"))
                .value_name("NUMBER"),
        )
}
