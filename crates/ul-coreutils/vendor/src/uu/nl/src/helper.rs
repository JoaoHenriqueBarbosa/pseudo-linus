// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (ToDO) conv

use std::ffi::OsString;

use uucore::display::locale_quote;
use uucore::error::{UResult, USimpleError, UUsageError};

use crate::options;

// Porte pseudo-linus: o `nl` do GNU valida cada opção com a sua própria mensagem
// (`invalid line number increment: ‘x’`), e o `-n` e os estilos de numeração entram como erro de
// uso (com a dica `Try 'nl --help'`). O uutils deixava isso pro clap, que fala outra língua.
const NUMBER_FORMATS: [&str; 3] = ["ln", "rn", "rz"];

/// Converte `text` pra um inteiro dentro de `min..=max`, com as mensagens do GNU: a conversão falha
/// com a frase pura, a faixa vira `Numerical result out of range` (abaixo) ou
/// `Value too large for defined data type` (acima).
fn parse_in_range(text: &str, what: &str, min: i128, max: i128) -> UResult<i128> {
    let invalid = |suffix: &str| {
        USimpleError::new(
            1,
            format!("invalid {what}: {}{suffix}", locale_quote(text)),
        )
    };
    let value: i128 = text.parse().map_err(|error: std::num::ParseIntError| {
        match error.kind() {
            std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow => {
                invalid(": Value too large for defined data type")
            }
            _ => invalid(""),
        }
    })?;
    if value < min {
        Err(invalid(": Numerical result out of range"))
    } else if value > max {
        Err(invalid(": Value too large for defined data type"))
    } else {
        Ok(value)
    }
}

/// Valida as opções antes de aplicá-las, na ordem do GNU, e devolve os números já convertidos.
pub struct Numbers {
    pub width: Option<usize>,
    pub join_blank_lines: Option<u64>,
    pub line_increment: Option<i64>,
    pub starting_line_number: Option<i64>,
}

pub fn validate_options(opts: &clap::ArgMatches) -> UResult<Numbers> {
    for (option, name) in [
        (options::HEADER_NUMBERING, "header"),
        (options::BODY_NUMBERING, "body"),
        (options::FOOTER_NUMBERING, "footer"),
    ] {
        if let Some(style) = opts.get_one::<String>(option) {
            let known = matches!(style.as_str(), "a" | "t" | "n") || style.starts_with('p');
            if !known {
                return Err(UUsageError::new(
                    1,
                    format!("invalid {name} numbering style: {}", locale_quote(style.as_str())),
                ));
            }
        }
    }
    if let Some(format) = opts.get_one::<String>(options::NUMBER_FORMAT) {
        if !NUMBER_FORMATS.contains(&format.as_str()) {
            return Err(UUsageError::new(
                1,
                format!("invalid line numbering format: {}", locale_quote(format.as_str())),
            ));
        }
    }

    let width = opts
        .get_one::<String>(options::NUMBER_WIDTH)
        .map(|text| parse_in_range(text, "line number field width", 1, i128::from(i32::MAX)))
        .transpose()?
        .map(|value| value as usize);
    let join_blank_lines = opts
        .get_one::<String>(options::JOIN_BLANK_LINES)
        .map(|text| {
            parse_in_range(text, "line number of blank lines", 0, i128::from(i64::MAX))
        })
        .transpose()?
        .map(|value| value as u64);
    let line_increment = opts
        .get_one::<String>(options::LINE_INCREMENT)
        .map(|text| {
            parse_in_range(text, "line number increment", i128::from(i64::MIN), i128::from(i64::MAX))
        })
        .transpose()?
        .map(|value| value as i64);
    let starting_line_number = opts
        .get_one::<String>(options::STARTING_LINE_NUMBER)
        .map(|text| {
            parse_in_range(text, "starting line number", i128::from(i64::MIN), i128::from(i64::MAX))
        })
        .transpose()?
        .map(|value| value as i64);

    Ok(Numbers {
        width,
        join_blank_lines,
        line_increment,
        starting_line_number,
    })
}

// parse_options loads the options into the settings, returning an array of
// error messages.
#[allow(clippy::cognitive_complexity)]
pub fn parse_options(
    settings: &mut crate::Settings,
    opts: &clap::ArgMatches,
    numbers: &Numbers,
) -> Vec<String> {
    // This vector holds error messages encountered.
    let mut errs: Vec<String> = vec![];
    settings.renumber = opts.get_flag(options::NO_RENUMBER);

    if let Some(mut delimiter) = opts
        .get_one::<OsString>(options::SECTION_DELIMITER)
        .cloned()
    {
        let is_single_char = delimiter
            .to_str()
            .map_or_else(|| delimiter.len() == 1, |s| s.chars().count() == 1);

        // A "single character" implies the second character of the delimiter is ':'.
        if is_single_char {
            delimiter.push(":");
        }

        settings.section_delimiter = delimiter;
    }

    if let Some(val) = opts.get_one::<OsString>(options::NUMBER_SEPARATOR) {
        settings.number_separator.clone_from(val);
    }
    settings.number_format = opts
        .get_one::<String>(options::NUMBER_FORMAT)
        .map(Into::into)
        .unwrap_or_default();
    match opts
        .get_one::<String>(options::HEADER_NUMBERING)
        .map(String::as_str)
        .map(TryInto::try_into)
    {
        None => {}
        Some(Ok(style)) => settings.header_numbering = style,
        Some(Err(message)) => errs.push(message),
    }
    match opts
        .get_one::<String>(options::BODY_NUMBERING)
        .map(String::as_str)
        .map(TryInto::try_into)
    {
        None => {}
        Some(Ok(style)) => settings.body_numbering = style,
        Some(Err(message)) => errs.push(message),
    }
    match opts
        .get_one::<String>(options::FOOTER_NUMBERING)
        .map(String::as_str)
        .map(TryInto::try_into)
    {
        None => {}
        Some(Ok(style)) => settings.footer_numbering = style,
        Some(Err(message)) => errs.push(message),
    }
    if let Some(num) = numbers.width {
        settings.number_width = num;
    }
    if let Some(num) = numbers.join_blank_lines {
        settings.join_blank_lines = num;
    }
    if let Some(num) = numbers.line_increment {
        settings.line_increment = num;
    }
    if let Some(num) = numbers.starting_line_number {
        settings.starting_line_number = num;
    }
    errs
}
