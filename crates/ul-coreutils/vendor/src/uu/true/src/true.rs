// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
use clap::{Arg, ArgAction, Command};
use sysio::io::{self, Write as _};
use uucore::error::strip_errno;
use uucore::{crate_version, show_error, translate};

/// Porte pseudo-linus: o texto de `--help` do GNU coreutils 9.7 (o `usage()` de true.c e false.c),
/// escrito como o Debian imprime em C.UTF-8. O uutils usa o `print_help` do clap, que sai com o
/// formato do clap e no stdout do host, não no do pseudo-processo.
fn help_text(name: &str, result: &str) -> String {
    format!(
        "Usage: {name} [ignored command line arguments]\n\
         \x20 or:  {name} OPTION\n\
         Exit with a status code indicating {result}.\n\
         \n\
         \x20     --help        display this help and exit\n\
         \x20     --version     output version information and exit\n\
         \n\
         Your shell may have its own version of {name}, which usually supersedes\n\
         the version described here.  Please refer to your shell's documentation\n\
         for details about the options it supports.\n\
         \n\
         GNU coreutils online help: <https://www.gnu.org/software/coreutils/>\n\
         Report any translation bugs to <https://translationproject.org/team/>\n\
         Full documentation <https://www.gnu.org/software/coreutils/{name}>\n\
         or available locally via: info '(coreutils) {name} invocation'\n"
    )
}

/// Porte pseudo-linus: o texto de `--version` do true do Debian 13.
fn version_text(name: &str) -> String {
    format!(
        "{name} (GNU coreutils) 9.7\n\
         Packaged by Debian (9.7-3)\n\
         Copyright (C) 2025 Free Software Foundation, Inc.\n\
         License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>.\n\
         This is free software: you are free to change and redistribute it.\n\
         There is NO WARRANTY, to the extent permitted by law.\n\
         \n\
         Written by Jim Meyering.\n"
    )
}

// uucore::main does not support no-result
pub fn uumain(mut args: impl uucore::Args) -> i32 {
    // skip binary name
    let (Some(flag), None) = (args.nth(1), args.next()) else {
        return 0;
    };

    let text = if flag == "--help" {
        help_text("true", "success")
    } else if flag == "--version" {
        version_text("true")
    } else {
        return 0;
    };

    let mut out = io::stdout();
    let res = out.write_all(text.as_bytes()).and_then(|()| out.flush());

    if let Err(e) = res
        && e.kind() != io::ErrorKind::BrokenPipe
    {
        // Try to display this error.
        show_error!("write error: {}", strip_errno(&e));
        // Mirror GNU options. When failing to print warnings or version flags, then we exit
        // with FAIL. This avoids allocation some error information which may result in yet
        // other types of failure.
        return 1;
    }
    0
}

pub fn uu_app() -> Command {
    Command::new("true")
        .version(crate_version!())
        .help_template(uucore::localized_help_template("true"))
        .about(translate!("true-about"))
        // We provide our own help and version options, to ensure maximum compatibility with GNU.
        .disable_help_flag(true)
        .disable_version_flag(true)
        .arg(
            Arg::new("help")
                .long("help")
                .help(translate!("true-help-text"))
                .action(ArgAction::Help),
        )
        .arg(
            Arg::new("version")
                .long("version")
                .help(translate!("true-version-text"))
                .action(ArgAction::Version),
        )
}
