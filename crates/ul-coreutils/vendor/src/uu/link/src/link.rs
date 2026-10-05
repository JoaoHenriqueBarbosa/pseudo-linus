// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
use clap::builder::ValueParser;
use clap::{Arg, Command};
use std::ffi::OsString;
use sysio::fs::hard_link;
use std::path::Path;
use uucore::display::Quotable;
use uucore::error::{FromIo, UResult, UUsageError};
use uucore::format_usage;
use uucore::translate;

pub mod options {
    pub static FILES: &str = "FILES";
}

#[uucore::main(no_signals)]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let matches = uucore::clap_localization::handle_clap_result(uu_app(), args)?;
    let files: Vec<_> = matches
        .get_many::<OsString>(options::FILES)
        .unwrap_or_default()
        .collect();

    // Porte pseudo-linus: a contagem de operandos é checada à mão, com as mensagens do GNU
    // (o clap diria outra coisa pra operando a menos).
    match files.len() {
        0 => {
            return Err(UUsageError::new(1, translate!("link-error-missing-operand")));
        }
        1 => {
            return Err(UUsageError::new(
                1,
                translate!("link-error-missing-operand-after", "operand" => uucore::display::locale_quote(files[0])),
            ));
        }
        2 => {}
        _ => {
            return Err(UUsageError::new(
                1,
                translate!("link-error-extra-operand", "operand" => uucore::display::locale_quote(files[2])),
            ));
        }
    }

    let old = Path::new(files[0]);
    let new = Path::new(files[1]);

    hard_link(old, new).map_err_context(
        || translate!("link-error-cannot-create-link", "new" => new.quote(), "old" => old.quote()),
    )
}

pub fn uu_app() -> Command {
    Command::new("link")
        .version(uucore::crate_version!())
        .help_template(uucore::localized_help_template("link"))
        .about(translate!("link-about"))
        .override_usage(format_usage(&translate!("link-usage")))
        .infer_long_args(true)
        .arg(
            Arg::new(options::FILES)
                .hide(true)
                .num_args(0..)
                .value_hint(clap::ValueHint::AnyPath)
                .value_parser(ValueParser::os_string()),
        )
}
