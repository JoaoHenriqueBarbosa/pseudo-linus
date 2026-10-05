// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (ToDO) parsemode makedev sysmacros perror IFBLK IFCHR IFIFO sflag

// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
use clap::{Arg, ArgAction, Command, value_parser};
// Porte pseudo-linus: o `nix` (mknod(2) e umask(2) do host) vira este shim sobre o sysio, com a
// mesma interface.
#[allow(non_camel_case_types)]
type mode_t = u32;
#[allow(non_camel_case_types)]
type dev_t = u64;
const S_IRUSR: mode_t = 0o400;
const S_IWUSR: mode_t = 0o200;
const S_IRGRP: mode_t = 0o040;
const S_IWGRP: mode_t = 0o020;
const S_IROTH: mode_t = 0o004;
const S_IWOTH: mode_t = 0o002;

#[derive(Clone, Copy)]
struct Mode(mode_t);

impl Mode {
    fn empty() -> Self {
        Self(0)
    }
    fn from_bits_truncate(bits: mode_t) -> Self {
        Self(bits & 0o7777)
    }
    fn bits(self) -> mode_t {
        self.0
    }
}

struct SFlag(mode_t);

impl SFlag {
    const S_IFBLK: Self = Self(sysio::errno::S_IFBLK);
    const S_IFCHR: Self = Self(sysio::errno::S_IFCHR);
    const S_IFIFO: Self = Self(sysio::errno::S_IFIFO);
    #[allow(dead_code)]
    fn bits(&self) -> mode_t {
        self.0
    }
}

fn nix_mknod(path: &str, kind: SFlag, perm: Mode, dev: dev_t) -> io::Result<()> {
    sysio::fs::mknod(path, kind.0 | perm.0, dev)
}

fn nix_umask(mask: Mode) -> Mode {
    Mode(sysio::process::set_umask(mask.0))
}
use std::ffi::OsString;
use sysio::io::{self, Write as _};

use uucore::error::{ExitCode, UResult, USimpleError, UUsageError, set_exit_code};
use uucore::format_usage;
use uucore::fs::makedev;
use uucore::translate;

#[allow(clippy::unnecessary_cast)]
const MODE_RW_UGO: u32 = (S_IRUSR | S_IWUSR | S_IRGRP | S_IWGRP | S_IROTH | S_IWOTH) as u32;

mod options {
    pub const MODE: &str = "mode";
    pub const TYPE: &str = "type";
    pub const MAJOR: &str = "major";
    pub const MINOR: &str = "minor";
    pub const EXTRA: &str = "extra";
    pub const SECURITY_CONTEXT: &str = "z";
    pub const CONTEXT: &str = "context";
}

#[derive(Clone, PartialEq)]
enum FileType {
    Block,
    Character,
    Fifo,
}

impl FileType {
    fn as_sflag(&self) -> SFlag {
        match self {
            Self::Block => SFlag::S_IFBLK,
            Self::Character => SFlag::S_IFCHR,
            Self::Fifo => SFlag::S_IFIFO,
        }
    }
}

/// Configuration for special inode creation.
struct Config {
    /// Permission bits for the inode
    mode: Mode,

    file_type: FileType,

    /// when false, the exact mode bits will be set
    use_umask: bool,

    dev: dev_t,

    /// Set security context (SELinux/SMACK).
    #[cfg(any(
        all(feature = "selinux", any(target_os = "android", target_os = "linux")),
        all(feature = "smack", target_os = "linux"),
    ))]
    set_security_context: bool,

    /// Specific security context (SELinux/SMACK).
    #[cfg(any(
        all(feature = "selinux", any(target_os = "android", target_os = "linux")),
        all(feature = "smack", target_os = "linux"),
    ))]
    context: Option<String>,
}

fn mknod(file_name: &str, config: Config) -> i32 {
    // Label the node at creation, as GNU does; relabelling after leaves a window.
    #[cfg(all(feature = "selinux", any(target_os = "android", target_os = "linux")))]
    let _selinux_guard = if config.set_security_context {
        let mode = config.file_type.as_sflag().bits() | config.mode.bits();
        match uucore::selinux::FsCreateContext::new(
            std::path::Path::new(file_name),
            Some(mode),
            config.context.as_ref(),
        ) {
            Ok(guard) => Some(guard),
            Err(e) => {
                let _ = writeln!(io::stderr(), "mknod: {e}");
                return 1;
            }
        }
    } else {
        None
    };

    // set umask to 0 and store previous umask
    let have_prev_umask = if config.use_umask {
        None
    } else {
        Some(nix_umask(Mode::empty()))
    };

    let mknod_err = nix_mknod(
        file_name,
        config.file_type.as_sflag(),
        config.mode,
        config.dev,
    )
    .err();
    // Porte pseudo-linus: o GNU sai com 1 (o -1 do uutils virava 255).
    let errno = i32::from(mknod_err.is_some());

    // set umask back to original value
    if let Some(prev_umask) = have_prev_umask {
        nix_umask(prev_umask);
    }

    if let Some(err) = mknod_err {
        let _ = writeln!(
            io::stderr(),
            "{}: {}: {}",
            uucore::execution_phrase(),
            file_name,
            // Porte pseudo-linus: mensagem da glibc, sem o " (os error N)" do Display do std.
            uucore::error::strip_errno(&err)
        );
    }

    // Apply SMACK context if requested
    #[cfg(all(feature = "smack", target_os = "linux"))]
    if config.set_security_context
        && let Err(e) =
            uucore::smack::set_smack_label_and_cleanup(file_name, config.context.as_ref(), |p| {
                sysio::fs::remove_file(p)
            })
    {
        let _ = writeln!(io::stderr(), "mknod: {e}");
        return 1;
    }

    errno
}

#[uucore::main]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let args: Vec<OsString> = args.collect();
    // Kept for the caret in mode diagnostics, which needs the mode as typed.
    let diag_args = uucore::diagnostics::operands(&args);
    let matches = uucore::clap_localization::handle_clap_result(uu_app(), args)?;

    let mut use_umask = true;
    let mode_permissions = match matches.get_one::<String>("mode") {
        None => MODE_RW_UGO,
        Some(str_mode) => {
            use_umask = false;
            let mode =
                uucore::mode::parse_chmod(MODE_RW_UGO, str_mode, true, uucore::mode::get_umask())
                    .map_err(|err| {
                    // Porte pseudo-linus: o GNU diz só `invalid mode`.
                    let message = translate!("mknod-error-invalid-mode");
                    if let Some(args) = &diag_args
                        && err.render_mode_value(args, str_mode, 0, &message)
                    {
                        // The diagnostic is already on stderr; exit quietly.
                        return ExitCode::new(1);
                    }
                    USimpleError::new(1, message)
                })?;
            if mode > 0o777 {
                return Err(USimpleError::new(
                    1,
                    translate!("mknod-error-mode-permission-bits-only"),
                ));
            }
            mode
        }
    };
    let mode = Mode::from_bits_truncate(mode_permissions as mode_t);

    let file_name = matches
        .get_one::<String>("name")
        .expect("Missing argument 'NAME'");
    let type_name = matches
        .get_one::<String>(options::TYPE)
        .expect("Missing argument 'TYPE'");

    // Porte pseudo-linus: os operandos depois de NOME e TIPO são contados à mão, como o GNU:
    // falta de MAJOR e MINOR, operando a mais e tipo inválido, cada um com a sua mensagem.
    let major = matches.get_one::<String>(options::MAJOR);
    let minor = matches.get_one::<String>(options::MINOR);
    let mut operands: Vec<&String> = vec![file_name, type_name];
    operands.extend(major);
    operands.extend(minor);
    if let Some(extra) = matches.get_many::<String>(options::EXTRA) {
        operands.extend(extra);
    }
    let nargs = operands.len();
    let first_char = type_name.chars().next();
    if nargs == 2 && first_char != Some('p') {
        return Err(UUsageError::new(
            1,
            format!(
                "{}\n{}",
                translate!("mknod-error-missing-operand-after", "operand" => uucore::display::locale_quote(operands[1])),
                translate!("mknod-error-special-require-major-minor")
            ),
        ));
    }
    let file_type = parse_type(type_name).map_err(|message| UUsageError::new(1, message))?;
    match file_type {
        FileType::Fifo if nargs != 2 => {
            let mut message = translate!("mknod-error-extra-operand", "operand" => uucore::display::locale_quote(operands[2]));
            if nargs == 4 {
                message.push('\n');
                message.push_str(&translate!("mknod-error-fifo-no-major-minor"));
            }
            return Err(UUsageError::new(1, message));
        }
        FileType::Block | FileType::Character if nargs < 4 => {
            return Err(UUsageError::new(
                1,
                translate!("mknod-error-missing-operand-after", "operand" => uucore::display::locale_quote(operands[nargs - 1])),
            ));
        }
        FileType::Block | FileType::Character if nargs > 4 => {
            return Err(UUsageError::new(
                1,
                translate!("mknod-error-extra-operand", "operand" => uucore::display::locale_quote(operands[4])),
            ));
        }
        _ => {}
    }

    // Extract the security context related flags and options
    #[cfg(any(
        all(feature = "selinux", any(target_os = "android", target_os = "linux")),
        all(feature = "smack", target_os = "linux"),
    ))]
    let set_security_context = matches.get_flag(options::SECURITY_CONTEXT);
    #[cfg(any(
        all(feature = "selinux", any(target_os = "android", target_os = "linux")),
        all(feature = "smack", target_os = "linux"),
    ))]
    let context = matches.get_one::<String>(options::CONTEXT).cloned();

    let dev = match (major, minor) {
        (Some(major), Some(minor)) => {
            let major = parse_device_number(major).ok_or_else(|| {
                USimpleError::new(
                    1,
                    translate!("mknod-error-invalid-major", "number" => uucore::display::locale_quote(major)),
                )
            })?;
            let minor = parse_device_number(minor).ok_or_else(|| {
                USimpleError::new(
                    1,
                    translate!("mknod-error-invalid-minor", "number" => uucore::display::locale_quote(minor)),
                )
            })?;
            makedev(major, minor)
        }
        _ => 0,
    };

    let config = Config {
        mode,
        file_type,
        use_umask,
        dev,
        #[cfg(any(
            all(feature = "selinux", any(target_os = "android", target_os = "linux")),
            all(feature = "smack", target_os = "linux"),
        ))]
        set_security_context: set_security_context || context.is_some(),
        #[cfg(any(
            all(feature = "selinux", any(target_os = "android", target_os = "linux")),
            all(feature = "smack", target_os = "linux"),
        ))]
        context,
    };

    let exit_code = mknod(file_name, config);
    set_exit_code(exit_code);
    Ok(())
}

pub fn uu_app() -> Command {
    Command::new("mknod")
        .version(uucore::crate_version!())
        .help_template(uucore::localized_help_template("mknod"))
        .override_usage(format_usage(&translate!("mknod-usage")))
        .after_help(translate!("mknod-after-help"))
        .about(translate!("mknod-about"))
        .infer_long_args(true)
        .arg(
            Arg::new(options::MODE)
                .short('m')
                .long("mode")
                .value_name("MODE")
                .help(translate!("mknod-help-mode")),
        )
        .arg(
            Arg::new("name")
                .value_name("NAME")
                .help(translate!("mknod-help-name"))
                .required(true)
                .value_hint(clap::ValueHint::AnyPath),
        )
        .arg(
            Arg::new(options::TYPE)
                .value_name("TYPE")
                .help(translate!("mknod-help-type"))
                .required(true)
                .value_parser(value_parser!(String)),
        )
        .arg(
            Arg::new(options::MAJOR)
                .value_name(options::MAJOR)
                .help(translate!("mknod-help-major"))
                .value_parser(value_parser!(String)),
        )
        .arg(
            Arg::new(options::MINOR)
                .value_name(options::MINOR)
                .help(translate!("mknod-help-minor"))
                .value_parser(value_parser!(String)),
        )
        // Porte pseudo-linus: recolhe os operandos a mais pra dizer `extra operand` como o GNU.
        .arg(
            Arg::new(options::EXTRA)
                .hide(true)
                .num_args(0..)
                .action(ArgAction::Append)
                .value_parser(value_parser!(String)),
        )
        .arg(
            Arg::new(options::SECURITY_CONTEXT)
                .short('Z')
                .help(translate!("mknod-help-selinux"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::CONTEXT)
                .long(options::CONTEXT)
                .value_name("CTX")
                .value_parser(value_parser!(String))
                .num_args(0..=1)
                .require_equals(true)
                .help(translate!("mknod-help-context")),
        )
}

fn parse_type(tpe: &str) -> Result<FileType, String> {
    // Dispatch on the leading character alone, so a spelled-out type works
    // wherever its initial does: `character` is read like `c` in
    // `mknod /dev/ttyS0 character 4 64`.
    tpe.chars()
        .next()
        .ok_or_else(|| translate!("mknod-error-invalid-device-type", "type" => uucore::display::locale_quote(tpe)))
        .and_then(|first_char| match first_char {
            'b' => Ok(FileType::Block),
            'c' | 'u' => Ok(FileType::Character),
            'p' => Ok(FileType::Fifo),
            _ => Err(translate!("mknod-error-invalid-device-type", "type" => uucore::display::locale_quote(tpe))),
        })
}

/// Porte pseudo-linus: o número de dispositivo como o `xstrtoumax` (base 0) do GNU: espaço no
/// começo, `+` opcional, `0x` hexadecimal, `0` octal, senão decimal, e a cadeia toda consumida.
/// `None` quando é inválido ou não cabe em 32 bits.
fn parse_device_number(text: &str) -> Option<u32> {
    let text = text.trim_start();
    let text = text.strip_prefix('+').unwrap_or(text);
    let (digits, radix) = if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        (hex, 16)
    } else if text.len() > 1 && text.starts_with('0') {
        (&text[1..], 8)
    } else {
        (text, 10)
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    u64::from_str_radix(digits, radix)
        .ok()
        .and_then(|value| u32::try_from(value).ok())
}
