// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (ToDO) lstat

// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
use clap::{Arg, ArgAction, Command};
use std::ffi::{OsStr, OsString};
use sysio::fs;
use sysio::io::ErrorKind;
use uucore::display::Quotable;
use uucore::error::strip_errno;
use uucore::error::{UResult, set_exit_code};
use uucore::format_usage;
use uucore::show_error;
use uucore::translate;

// operating mode
enum Mode {
    Default, // use filesystem to determine information and limits
    Basic,   // check basic compatibility with POSIX
    Extra,   // check for leading dashes and empty names
    Both,    // a combination of `Basic` and `Extra`
}

mod options {
    pub const POSIX: &str = "posix";
    pub const POSIX_SPECIAL: &str = "posix-special";
    pub const PORTABILITY: &str = "portability";
    pub const PATH: &str = "path";
}

// a few global constants as used in the GNU implementation
// Porte pseudo-linus: o GNU compara com `_POSIX_PATH_MAX - 1` (o limite conta o NUL final), e a
// mensagem diz 255.
const POSIX_PATH_MAX: usize = 255;
const POSIX_NAME_MAX: usize = 14;

// Porte pseudo-linus: os valores do Linux (PATH_MAX e FILENAME_MAX da glibc) sem a libc do host.
#[cfg(all(unix, not(target_os = "redox")))]
const PATH_MAX: usize = 4096;
#[cfg(all(unix, not(target_os = "redox")))]
const FILENAME_MAX: usize = 4096;
#[cfg(target_os = "redox")]
const PATH_MAX: usize = 4096;
#[cfg(target_os = "redox")]
const FILENAME_MAX: usize = 255;
// for Windows. But don't deny wasm
#[cfg(not(unix))]
const PATH_MAX: usize = 260;
#[cfg(not(unix))]
const FILENAME_MAX: usize = 255;

#[uucore::main(no_signals)]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let matches = uucore::clap_localization::handle_clap_result(uu_app(), args)?;

    // set working mode
    let is_posix = matches.get_flag(options::POSIX);
    let is_posix_special = matches.get_flag(options::POSIX_SPECIAL);
    let is_portability = matches.get_flag(options::PORTABILITY);

    let mode = if (is_posix && is_posix_special) || is_portability {
        Mode::Both
    } else if is_posix {
        Mode::Basic
    } else if is_posix_special {
        Mode::Extra
    } else {
        Mode::Default
    };

    // take necessary actions
    let paths = matches.get_many::<OsString>(options::PATH);

    // free strings are path operands
    // FIXME: TCS, seems inefficient and overly verbose (?)
    let mut res = true;
    for p in paths.unwrap() {
        let path_str = p.to_string_lossy();
        let mut path = Vec::new();
        for path_segment in path_str.split('/') {
            path.push(path_segment.to_string());
        }
        res &= check_path(&mode, p, &path);
    }

    // determine error code
    if !res {
        set_exit_code(1);
    }
    Ok(())
}

pub fn uu_app() -> Command {
    Command::new("pathchk")
        .version(uucore::crate_version!())
        .help_template(uucore::localized_help_template("pathchk"))
        .about(translate!("pathchk-about"))
        .override_usage(format_usage(&translate!("pathchk-usage")))
        .infer_long_args(true)
        .arg(
            Arg::new(options::POSIX)
                .short('p')
                .help(translate!("pathchk-help-posix"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::POSIX_SPECIAL)
                .short('P')
                .help(translate!("pathchk-help-posix-special"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::PORTABILITY)
                .long(options::PORTABILITY)
                .help(translate!("pathchk-help-portability"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::PATH)
                .hide(true)
                .action(ArgAction::Append)
                .value_hint(clap::ValueHint::AnyPath)
                .required(true)
                .value_parser(clap::value_parser!(OsString)),
        )
}

/// check a path, given as the operand and as a slice of it's components and an operating mode
///
/// Porte pseudo-linus: a ordem das verificações é a do GNU, medida no oráculo: nome vazio, `-`
/// inicial (`-P`), caracteres portáveis do nome inteiro, comprimento do caminho e só então o de cada
/// componente (`-p`).
fn check_path(mode: &Mode, name: &OsStr, path: &[String]) -> bool {
    // GNU rejects an empty file name in any portability mode before touching the filesystem.
    if !matches!(mode, Mode::Default) && name.is_empty() {
        show_error!("{}", translate!("pathchk-error-empty-file-name"));
        return false;
    }

    match *mode {
        Mode::Basic => check_basic(name),
        Mode::Extra => check_extra(name) && check_default(path),
        Mode::Both => check_extra(name) && check_basic(name),
        Mode::Default => check_default(path),
    }
}

/// check a path in basic compatibility mode
fn check_basic(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    // characters: only the portable filename character set (and the separator)
    if !check_portable_chars(name) {
        return false;
    }
    // path length
    if bytes.len() > POSIX_PATH_MAX {
        show_error!(
            "{}",
            translate!("pathchk-error-posix-path-length-exceeded", "limit" => POSIX_PATH_MAX, "length" => bytes.len(), "path" => name.quote())
        );
        return false;
    }

    // components: length (every byte is ASCII here, the character check above already passed)
    for component in bytes.split(|b| *b == b'/') {
        if component.len() > POSIX_NAME_MAX {
            let component = String::from_utf8_lossy(component);
            show_error!(
                "{}",
                translate!("pathchk-error-posix-name-length-exceeded", "limit" => POSIX_NAME_MAX, "length" => component.len(), "component" => uucore::display::locale_quote(&*component))
            );
            return false;
        }
    }
    // permission checks
    check_searchable(&name.to_string_lossy())
}

/// check a path in extra compatibility mode
fn check_extra(name: &OsStr) -> bool {
    // components: leading hyphens
    if name
        .as_encoded_bytes()
        .split(|b| *b == b'/')
        .any(|component| component.first() == Some(&b'-'))
    {
        show_error!(
            "{}",
            translate!("pathchk-error-leading-hyphen", "path" => name.quote())
        );
        return false;
    }
    true
}

/// check a path in default mode (using the file system)
fn check_default(path: &[String]) -> bool {
    let joined_path = path.join("/");
    let total_len = joined_path.len();
    // path length
    if total_len > PATH_MAX {
        show_error!(
            "{}",
            translate!("pathchk-error-path-length-exceeded", "limit" => PATH_MAX, "length" => total_len, "path" => joined_path.quote())
        );
        return false;
    }
    if total_len == 0 {
        // POSIX has no empty file name, yet some systems accept one as a way
        // of writing the current directory. Rather than decide that here, ask
        // the platform: keep the operand when `symlink_metadata` (`lstat`)
        // resolves it, reject it when that fails.
        if fs::symlink_metadata(&joined_path).is_err() {
            show_error!("{}", translate!("pathchk-error-empty-path-not-found"));
            return false;
        }
    }

    // components: length
    for p in path {
        let component_len = p.len();
        if component_len > FILENAME_MAX {
            show_error!(
                "{}",
                translate!("pathchk-error-name-length-exceeded", "limit" => FILENAME_MAX, "length" => component_len, "component" => uucore::display::locale_quote(p))
            );
            return false;
        }
    }
    // permission checks
    check_searchable(&joined_path)
}

/// check whether a path is or if other problems arise
fn check_searchable(path: &str) -> bool {
    // we use lstat, just like the original implementation
    match fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(e) if e.kind() == ErrorKind::NotFound => true,
        Err(e) => {
            show_error!("{}: {}", path, strip_errno(&e));
            false
        }
    }
}

/// check whether a file name contains only portable characters (and the separator)
///
/// Porte pseudo-linus: o GNU olha o nome inteiro, mostra o primeiro caractere fora do conjunto
/// portável entre aspas do `quote()` (`‘ç’`, `‘\t’`, `‘\377’` pra byte inválido) e o nome com as
/// aspas de shell do `quoteaf`.
fn check_portable_chars(name: &OsStr) -> bool {
    const VALID_CHARS: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-/";
    let bytes = name.as_encoded_bytes();
    for (i, ch) in bytes.iter().enumerate() {
        if !VALID_CHARS.contains(ch) {
            let rest = &bytes[i..];
            let chunk = rest.utf8_chunks().next().unwrap();
            let shown = match chunk.valid().chars().next() {
                Some(c) => {
                    let mut buf = [0u8; 4];
                    let s: &str = c.encode_utf8(&mut buf);
                    uucore::display::locale_quote(s)
                }
                None => format!("\u{2018}\\{:03o}\u{2019}", chunk.invalid()[0]),
            };
            show_error!(
                "{}",
                translate!("pathchk-error-nonportable-character", "character" => shown, "path" => name.quote())
            );
            return false;
        }
    }
    true
}
