// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

use clap::{Arg, ArgAction, Command};
use std::time::Duration;
// Porte pseudo-linus: o sono é o `nanosleep` do pseudo-kernel.
use sysio::thread;
use uucore::display::locale_quote;
use uucore::translate;
use uucore::{
    error::{UResult, UUsageError},
    format_usage,
    parser::parse_time,
    show_error,
};

mod options {
    pub const NUMBER: &str = "NUMBER";
}

#[uucore::main]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let matches = uucore::clap_localization::handle_clap_result(uu_app(), args)?;

    // Porte pseudo-linus: sem operando o GNU diz "missing operand" e sugere o --help (o clap
    // escreveria o próprio texto de argumento obrigatório).
    let Some(numbers) = matches.get_many::<String>(options::NUMBER) else {
        return Err(UUsageError::new(1, "missing operand"));
    };
    let numbers = numbers.map(String::as_str).collect::<Vec<_>>();

    sleep(&numbers)
}

pub fn uu_app() -> Command {
    Command::new("sleep")
        .version(uucore::crate_version!())
        .help_template(uucore::localized_help_template("sleep"))
        .about(translate!("sleep-about"))
        .after_help(translate!("sleep-after-help"))
        .override_usage(format_usage(&translate!("sleep-usage")))
        .infer_long_args(true)
        .arg(
            Arg::new(options::NUMBER)
                .help(translate!("sleep-help-number"))
                .value_name(options::NUMBER)
                .action(ArgAction::Append),
        )
}

fn sleep(args: &[&str]) -> UResult<()> {
    let mut arg_error = false;

    let sleep_dur = args
        .iter()
        .filter_map(|input| if let Ok(duration) = parse_time::from_str(input, true) { Some(duration) } else {
            arg_error = true;
            // Porte pseudo-linus: o GNU cita o operando com aspas curvas (`quote` do gnulib).
            show_error!("invalid time interval {}", locale_quote(*input));
            None
        })
        .fold(Duration::ZERO, Duration::saturating_add);

    if arg_error {
        return Err(UUsageError::new(1, ""));
    }
    thread::sleep(sleep_dur);
    Ok(())
}
