// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (ToDO) dircolors eightbit fnmatch setenv colorterm disp cshell

// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
use sysio::path::PathExt; // Porte pseudo-linus: métodos de Path sobre o FS do pseudo-processo.
use std::borrow::Borrow;
use sysio::env;
use std::ffi::OsString;
use sysio::fs::File;
use sysio::io::{BufRead, BufReader, Write, stdout};
use std::path::Path;

use clap::{Arg, ArgAction, Command};
use uucore::colors::FILE_ATTRIBUTE_CODES;
use uucore::display::Quotable;
use uucore::error::{UResult, USimpleError, UUsageError, set_exit_code, strip_errno};
use uucore::translate;

use uucore::{format_usage, parser::parse_glob, show_error};

/// Porte pseudo-linus: o banco de dados embutido do GNU 9.7 (o que `dircolors -p` imprime). Sem
/// arquivo, o GNU passa este texto pelo mesmo analisador que usa nos arquivos do usuário, então os
/// filtros `TERM` e `COLORTERM` valem também pra ele.
const DATABASE: &str = include_str!("database.txt");

mod options {
    pub const BOURNE_SHELL: &str = "bourne-shell";
    pub const C_SHELL: &str = "c-shell";
    pub const PRINT_DATABASE: &str = "print-database";
    pub const PRINT_LS_COLORS: &str = "print-ls-colors";
    pub const FILE: &str = "FILE";
}

#[derive(PartialEq, Eq, Debug)]
enum OutputFmt {
    Shell,
    CShell,
    Display,
}

fn guess_syntax<T: AsRef<Path>>(path: T) -> Option<OutputFmt> {
    let shell_path = path.as_ref();

    if shell_path.as_os_str().is_empty() {
        return None;
    }

    let is_cshell = |name| name == "csh" || name == "tcsh";

    if shell_path.file_name().is_some_and(is_cshell) {
        Some(OutputFmt::CShell)
    } else {
        Some(OutputFmt::Shell)
    }
}

fn get_colors_format_strings(fmt: &OutputFmt) -> (String, String) {
    let prefix = match fmt {
        OutputFmt::Shell => "LS_COLORS='".to_string(),
        OutputFmt::CShell => "setenv LS_COLORS '".to_string(),
        OutputFmt::Display => String::new(),
    };

    let suffix = match fmt {
        OutputFmt::Shell => "';\nexport LS_COLORS".to_string(),
        OutputFmt::CShell => "'".to_string(),
        OutputFmt::Display => String::new(),
    };

    (prefix, suffix)
}

#[uucore::main(no_signals)]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let matches = uucore::clap_localization::handle_clap_result(uu_app(), args)?;

    let files = matches
        .get_many::<OsString>(options::FILE)
        .map_or(vec![], Iterator::collect);

    // clap provides .conflicts_with / .conflicts_with_all, but we want to
    // manually handle conflicts so we can match the output of GNU coreutils
    if (matches.get_flag(options::C_SHELL) || matches.get_flag(options::BOURNE_SHELL))
        && (matches.get_flag(options::PRINT_DATABASE) || matches.get_flag(options::PRINT_LS_COLORS))
    {
        return Err(UUsageError::new(
            1,
            translate!("dircolors-error-shell-and-output-exclusive"),
        ));
    }

    if matches.get_flag(options::PRINT_DATABASE) && matches.get_flag(options::PRINT_LS_COLORS) {
        return Err(UUsageError::new(
            1,
            translate!("dircolors-error-print-database-and-ls-colors-exclusive"),
        ));
    }

    if matches.get_flag(options::PRINT_DATABASE) {
        if !files.is_empty() {
            return Err(UUsageError::new(
                1,
                translate!("dircolors-error-extra-operand-print-database", "operand" => uucore::display::locale_quote(files[0])),
            ));
        }

        // Porte pseudo-linus: o banco embutido já termina em quebra de linha.
        write!(stdout(), "{DATABASE}")?;
        return Ok(());
    }

    let out_format = if matches.get_flag(options::C_SHELL) {
        OutputFmt::CShell
    } else if matches.get_flag(options::BOURNE_SHELL) {
        OutputFmt::Shell
    } else if matches.get_flag(options::PRINT_LS_COLORS) {
        OutputFmt::Display
    } else {
        env::var_os("SHELL")
            .and_then(|path| guess_syntax(&path))
            .ok_or_else(|| {
                USimpleError::new(1, translate!("dircolors-error-no-shell-environment"))
            })?
    };

    let result = match files.as_slice() {
        // Porte pseudo-linus: sem arquivo o GNU analisa o banco embutido, com os mesmos filtros de
        // `TERM` e `COLORTERM` de um arquivo do usuário.
        [] => parse(DATABASE.lines(), &out_format, "<internal>"),
        [_file_arg, extra, ..] => {
            return Err(UUsageError::new(
                1,
                translate!("dircolors-error-extra-operand", "operand" => uucore::display::locale_quote(*extra)),
            ));
        }
        [file_arg] => {
            if *file_arg == "-" {
                let fin = BufReader::new(sysio::io::stdin());
                // For example, for echo "owt 40;33"|dircolors -b -
                parse(fin.lines().map_while(Result::ok), &out_format, "-")
            } else {
                let path = Path::new(&file_arg);
                if path.sys_is_dir() {
                    return Err(USimpleError::new(
                        2,
                        translate!("dircolors-error-expected-file-got-directory", "path" => path.quote()),
                    ));
                }
                let file = File::open(path).map_err(|e| {
                    USimpleError::new(1, format!("{}: {}", path.maybe_quote(), strip_errno(&e)))
                })?;
                let fin = BufReader::new(file);
                parse(
                    fin.lines().map_while(Result::ok),
                    &out_format,
                    &path.to_string_lossy(),
                )
            }
        }
    };

    // Porte pseudo-linus: com erro de sintaxe o GNU já avisou cada linha ruim, não escreve nada e
    // sai com 1.
    match result {
        Some(string) if out_format == OutputFmt::Display => write!(stdout(), "{string}")?,
        Some(string) => writeln!(stdout(), "{string}")?,
        None => set_exit_code(1),
    }
    Ok(())
}

pub fn uu_app() -> Command {
    Command::new("dircolors")
        .version(uucore::crate_version!())
        .help_template(uucore::localized_help_template("dircolors"))
        .about(translate!("dircolors-about"))
        .after_help(translate!("dircolors-after-help"))
        .override_usage(format_usage(&translate!("dircolors-usage")))
        .args_override_self(true)
        .infer_long_args(true)
        .arg(
            Arg::new(options::BOURNE_SHELL)
                .long("sh")
                .short('b')
                .visible_alias("bourne-shell")
                .overrides_with(options::C_SHELL)
                .help(translate!("dircolors-help-bourne-shell"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::C_SHELL)
                .long("csh")
                .short('c')
                .visible_alias("c-shell")
                .overrides_with(options::BOURNE_SHELL)
                .help(translate!("dircolors-help-c-shell"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::PRINT_DATABASE)
                .long("print-database")
                .short('p')
                .help(translate!("dircolors-help-print-database"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::PRINT_LS_COLORS)
                .long("print-ls-colors")
                .help(translate!("dircolors-help-print-ls-colors"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::FILE)
                .hide(true)
                .value_hint(clap::ValueHint::FilePath)
                .value_parser(clap::value_parser!(OsString))
                .action(ArgAction::Append),
        )
}

trait StrUtils {
    /// Remove comments and trim whitespace
    fn purify(&self) -> &Self;
    /// Like `split_whitespace()` but only produce 2 parts
    fn split_two(&self) -> (&str, &str);
    fn fnmatch(&self, pattern: &str) -> bool;
}

impl StrUtils for str {
    fn purify(&self) -> &Self {
        let mut line = self;
        for (n, _) in self
            .as_bytes()
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == b'#')
        {
            // Ignore the content after '#'
            // only if it is preceded by at least one whitespace
            match self[..n].chars().last() {
                Some(c) if c.is_whitespace() => {
                    line = &self[..n - c.len_utf8()];
                    break;
                }
                None => {
                    // n == 0
                    line = &self[..0];
                    break;
                }
                _ => (),
            }
        }
        line.trim()
    }

    fn split_two(&self) -> (&str, &str) {
        if let Some(b) = self.find(char::is_whitespace) {
            let key = &self[..b];
            if let Some(e) = self[b..].find(|c: char| !c.is_whitespace()) {
                (key, &self[b + e..])
            } else {
                (key, "")
            }
        } else {
            ("", "")
        }
    }

    fn fnmatch(&self, pat: &str) -> bool {
        // An invalid glob never matches (GNU ignores it); don't unwrap the Err.
        parse_glob::from_str(pat).is_ok_and(|glob| glob.matches(self))
    }
}

/// Estado do filtro de terminal (`TERM` e `COLORTERM`), como o GNU o mede.
#[derive(PartialEq, Clone, Copy)]
enum ParseState {
    /// Antes de qualquer filtro: as entradas valem pra todo terminal.
    Global,
    /// O último filtro casou e ainda não houve entrada depois dele (um filtro seguinte soma a ele).
    Matched,
    /// O filtro casou e já houve entrada: um filtro seguinte abre uma condição nova.
    Continue,
    /// O filtro não casou: as entradas são ignoradas até um filtro que case.
    Pass,
}

/// Analisa o banco de cores (o embutido ou o arquivo do usuário) e devolve a saída pronta.
///
/// Porte pseudo-linus: como o GNU, avisa de cada linha ruim (`arquivo:linha: ...`) e segue em frente;
/// com algum aviso devolve `None`, e quem chamou não escreve nada. Palavra-chave desconhecida só
/// conta como erro depois do primeiro filtro de terminal.
fn parse<T>(user_input: T, fmt: &OutputFmt, fp: &str) -> Option<String>
where
    T: IntoIterator,
    T::Item: Borrow<str>,
{
    let mut result = String::with_capacity(1790);
    let (prefix, suffix) = get_colors_format_strings(fmt);

    result.push_str(&prefix);

    // Get environment variables once at the start
    let term = env::var("TERM").unwrap_or_else(|_| "none".to_owned());
    let colorterm = env::var("COLORTERM").unwrap_or_default();

    let mut state = ParseState::Global;
    let mut ok = true;

    for (num, line) in (1..).zip(user_input) {
        let line = line.borrow().purify();
        if line.is_empty() {
            continue;
        }

        let line = escape(line);

        let (key, val) = line.split_two();
        if val.is_empty() {
            show_error!(
                "{}",
                translate!("dircolors-error-invalid-line-missing-token", "file" => fp.maybe_quote(), "line" => num)
            );
            ok = false;
            continue;
        }

        let lower = key.to_lowercase();
        match lower.as_str() {
            "term" | "colorterm" => {
                let matched = if lower == "term" {
                    term.fnmatch(val)
                } else if val == "?*" {
                    // For COLORTERM ?*, only match if COLORTERM is non-empty
                    !colorterm.is_empty()
                } else {
                    colorterm.fnmatch(val)
                };
                state = if matched {
                    ParseState::Matched
                } else if state == ParseState::Global || state == ParseState::Continue {
                    ParseState::Pass
                } else {
                    state
                };
            }
            _ => {
                if state == ParseState::Matched {
                    state = ParseState::Continue;
                }
                if state != ParseState::Pass
                    && !append_entry(&mut result, fmt, key, &lower, val)
                    && state != ParseState::Global
                {
                    show_error!(
                        "{}",
                        translate!("dircolors-error-unrecognized-keyword", "file" => fp.maybe_quote(), "line" => num, "keyword" => key)
                    );
                    ok = false;
                }
            }
        }
    }

    result.push_str(&suffix);

    ok.then_some(result)
}

/// Acrescenta uma entrada à saída; `false` quando a palavra-chave não existe.
fn append_entry(result: &mut String, fmt: &OutputFmt, key: &str, lower: &str, val: &str) -> bool {
    if key.starts_with(['.', '*']) {
        let entry = if key.starts_with('.') {
            format!("*{key}")
        } else {
            key.to_string()
        };
        let disp = if *fmt == OutputFmt::Display {
            format!("\x1b[{val}m{entry}\t{val}\x1b[0m\n")
        } else {
            format!("{entry}={val}:")
        };
        result.push_str(&disp);
        return true;
    }

    match lower {
        "options" | "color" | "eightbit" => true, // Slackware only, ignore
        _ => {
            if let Some((_, s)) = FILE_ATTRIBUTE_CODES.iter().find(|&&(key, _)| key == lower) {
                let disp = if *fmt == OutputFmt::Display {
                    format!("\x1b[{val}m{s}\t{val}\x1b[0m\n")
                } else {
                    format!("{s}={val}:")
                };
                result.push_str(&disp);
                true
            } else {
                false
            }
        }
    }
}

/// Escape single quotes because they are not allowed between single quotes in shell code, and code
/// enclosed by single quotes is what is returned by `parse()`.
///
/// We also escape ":" to make the "quote" test pass in the GNU test suite:
/// <https://github.com/coreutils/coreutils/blob/master/tests/misc/dircolors.pl>
fn escape(s: &str) -> String {
    let mut result = String::new();
    let mut previous = ' ';

    for c in s.chars() {
        match c {
            '\'' => result.push_str("'\\''"),
            ':' if previous != '\\' => result.push_str("\\:"),
            _ => result.push(c),
        }
        previous = c;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape() {
        assert_eq!("", escape(""));
        assert_eq!("'\\''", escape("'"));
        assert_eq!("\\:", escape(":"));
        assert_eq!("\\:", escape("\\:"));
    }

    #[test]
    fn test_guess_syntax() {
        assert_eq!(Some(OutputFmt::CShell), guess_syntax("/path/csh"));
        assert_eq!(Some(OutputFmt::CShell), guess_syntax("csh"));
        assert_eq!(Some(OutputFmt::Shell), guess_syntax("/path/bash"));
        assert_eq!(Some(OutputFmt::Shell), guess_syntax("bash"));
        assert_eq!(Some(OutputFmt::Shell), guess_syntax("/asd/bar"));
        assert_eq!(Some(OutputFmt::Shell), guess_syntax("foo"));
        assert_eq!(None, guess_syntax(""));
    }

    #[test]
    fn test_purify() {
        let s = "  asd#zcv #hk\t\n  ";
        assert_eq!("asd#zcv", s.purify());
    }

    #[test]
    fn test_fnmatch() {
        let s = "con256asd";
        assert!(s.fnmatch("*[2][3-6][5-9]?sd")); // spell-checker:disable-line
    }

    #[test]
    fn test_split_two() {
        let s = "zxc \t\nqwe jlk    hjl"; // spell-checker:disable-line
        let (k, v) = s.split_two();
        assert_eq!("zxc", k);
        assert_eq!("qwe jlk    hjl", v);
    }
}
