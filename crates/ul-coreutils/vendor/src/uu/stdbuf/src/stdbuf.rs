// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (ToDO) tempdir dyld dylib optgrps libstdbuf

// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
use clap::{Arg, ArgAction, ArgMatches, Command};
use std::ffi::OsString;
use sysio::process;
use thiserror::Error;
use uucore::diagnostics::OptionValue;
use uucore::error::{UResult, USimpleError, UUsageError, strip_errno};
use uucore::format_usage;
use uucore::parser::parse_size::{ParseSizeError, parse_size_u64};
use uucore::translate;

mod options {
    pub const INPUT: &str = "input";
    pub const INPUT_SHORT: char = 'i';
    pub const OUTPUT: &str = "output";
    pub const OUTPUT_SHORT: char = 'o';
    pub const ERROR: &str = "error";
    pub const ERROR_SHORT: char = 'e';
    pub const COMMAND: &str = "command";
}

// Porte pseudo-linus: sem a libstdbuf embutida (ver `uumain`).
enum BufferType {
    Default,
    Line,
    Size(usize),
}

struct ProgramOptions {
    stdin: BufferType,
    stdout: BufferType,
    stderr: BufferType,
}

impl TryFrom<&ArgMatches> for ProgramOptions {
    type Error = ProgramOptionsError;

    fn try_from(matches: &ArgMatches) -> Result<Self, Self::Error> {
        Ok(Self {
            stdin: check_option(matches, options::INPUT, options::INPUT_SHORT)?,
            stdout: check_option(matches, options::OUTPUT, options::OUTPUT_SHORT)?,
            stderr: check_option(matches, options::ERROR, options::ERROR_SHORT)?,
        })
    }
}

/// A buffering mode that did not parse as a size, and where it came from.
///
/// The message is built where it always was; the rest is what a caret needs:
/// the mode as typed with the option it was given to, and what the size parser
/// made of it.
#[derive(Debug)]
struct ModeError {
    option: OptionValue,
    error: ParseSizeError,
}

#[derive(Debug, Error)]
enum ProgramOptionsError {
    #[error("{}", translate!("stdbuf-error-line-buffering-stdin-meaningless"))]
    LineBufferingStdinMeaningless,
    #[error("{}", translate!("stdbuf-error-invalid-mode", "error" => _0.error.to_string()))]
    InvalidMode(Box<ModeError>),
    #[error("{}", translate!("stdbuf-error-value-too-large", "value" => _0))]
    ValueTooLarge(String),
}

fn check_option(
    matches: &ArgMatches,
    name: &'static str,
    short: char,
) -> Result<BufferType, ProgramOptionsError> {
    match matches.get_one::<String>(name) {
        Some(value) => match value.as_str() {
            "L" => {
                if name == options::INPUT {
                    Err(ProgramOptionsError::LineBufferingStdinMeaningless)
                } else {
                    Ok(BufferType::Line)
                }
            }
            x => parse_size_u64(x).map_or_else(
                |error| {
                    Err(ProgramOptionsError::InvalidMode(Box::new(ModeError {
                        option: OptionValue::new(x, short, name),
                        error,
                    })))
                },
                |m| {
                    Ok(BufferType::Size(m.try_into().map_err(|_| {
                        ProgramOptionsError::ValueTooLarge(x.to_string())
                    })?))
                },
            ),
        },
        None => Ok(BufferType::Default),
    }
}

fn set_command_env(command: &mut process::Command, buffer_name: &str, buffer_type: &BufferType) {
    match buffer_type {
        BufferType::Size(m) => {
            command.env(buffer_name, m.to_string());
        }
        BufferType::Line => {
            command.env(buffer_name, "L");
        }
        BufferType::Default => {}
    }
}

#[uucore::main]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let raw_args: Vec<OsString> = args.collect();
    // Kept for the caret in mode diagnostics, which needs the mode as typed.
    let diag_args = uucore::diagnostics::capture(&raw_args);
    let matches =
        uucore::clap_localization::handle_clap_result_with_exit_code(uu_app(), raw_args, 125)?;

    let options = ProgramOptions::try_from(&matches).map_err(|e| {
        let message = e.to_string();
        uucore::diagnostics::error_after_report(
            diag_args.as_deref(),
            UUsageError::new(125, message.clone()),
            |args, _| match &e {
                ProgramOptionsError::InvalidMode(mode) => {
                    mode.error
                        .render_size_value(args, &mode.option, 0, &message)
                }
                // The rest is not about a mode that failed to parse, so there
                // is nothing to point a caret at.
                _ => false,
            },
        )
    })?;

    let mut command_values = matches
        .get_many::<OsString>(options::COMMAND)
        .ok_or_else(|| UUsageError::new(125, "no command specified"))?;
    let Some(first_command) = command_values.next() else {
        return Err(UUsageError::new(125, "no command specified"));
    };
    let mut command = process::Command::new(first_command);
    let command_params: Vec<&OsString> = command_values.collect();

    // Porte pseudo-linus: sem a libstdbuf injetada por LD_PRELOAD (não há carregador dinâmico).
    // O `run` do sysio de cada programa lê `_STDBUF_I/O/E` ao iniciar e ajusta os buffers, que é o
    // que o construtor da libstdbuf faz; aqui basta passar as variáveis, como o GNU, e fazer
    // `execvp`.
    set_command_env(&mut command, "_STDBUF_I", &options.stdin);
    set_command_env(&mut command, "_STDBUF_O", &options.stdout);
    set_command_env(&mut command, "_STDBUF_E", &options.stderr);
    command.args(command_params);

    let e = sysio::os::unix::process::CommandExt::exec(&mut command);
    let exit_code = match e.kind() {
        sysio::io::ErrorKind::NotFound => 127,
        _ => 126,
    };
    Err(USimpleError::new(
        exit_code,
        format!(
            "failed to run command {}: {}",
            uucore::display::locale_quote(first_command),
            strip_errno(&e)
        ),
    ))
}

pub fn uu_app() -> Command {
    #[cfg(unix)]
    let about = translate!("stdbuf-about");
    #[cfg(windows)]
    let about = translate!("stdbuf-about-windows");
    Command::new("stdbuf")
        .version(uucore::crate_version!())
        .help_template(uucore::localized_help_template("stdbuf"))
        .about(about)
        .after_help(translate!("stdbuf-after-help"))
        .override_usage(format_usage(&translate!("stdbuf-usage")))
        .trailing_var_arg(true)
        .infer_long_args(true)
        .arg(
            Arg::new(options::INPUT)
                .long(options::INPUT)
                .short(options::INPUT_SHORT)
                .help(translate!("stdbuf-help-input"))
                .value_name("MODE")
                .required_unless_present_any([options::OUTPUT, options::ERROR]),
        )
        .arg(
            Arg::new(options::OUTPUT)
                .long(options::OUTPUT)
                .short(options::OUTPUT_SHORT)
                .help(translate!("stdbuf-help-output"))
                .value_name("MODE")
                .required_unless_present_any([options::INPUT, options::ERROR]),
        )
        .arg(
            Arg::new(options::ERROR)
                .long(options::ERROR)
                .short(options::ERROR_SHORT)
                .help(translate!("stdbuf-help-error"))
                .value_name("MODE")
                .required_unless_present_any([options::INPUT, options::OUTPUT]),
        )
        .arg(
            Arg::new(options::COMMAND)
                .action(ArgAction::Append)
                .hide(true)
                .required(true)
                .value_hint(clap::ValueHint::CommandName)
                .value_parser(clap::value_parser!(OsString)),
        )
}
