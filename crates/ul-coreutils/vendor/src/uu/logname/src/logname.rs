// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (ToDO) getlogin userlogin

// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
use clap::Command;
use sysio::io::{Write, stdout};
use uucore::translate;
use uucore::{error::UResult, show_error};

// Porte pseudo-linus: o `getlogin(3)` da glibc procura o terminal de controle no utmp. O
// pseudo-linus não tem sessão de login nem utmp (como um container), e ali a glibc devolve NULL.
fn get_userlogin() -> Option<String> {
    None
}

#[uucore::main(no_signals)]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let _ = uucore::clap_localization::handle_clap_result(uu_app(), args)?;

    if let Some(userlogin) = get_userlogin() {
        writeln!(stdout(), "{userlogin}")?;
        Ok(())
    } else {
        show_error!("{}", translate!("logname-error-no-login-name"));
        Err(1.into())
    }
}

pub fn uu_app() -> Command {
    Command::new("logname")
        .version(uucore::crate_version!())
        .help_template(uucore::localized_help_template("logname"))
        .override_usage(translate!("logname-usage"))
        .about(translate!("logname-about"))
        .infer_long_args(true)
}
