// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (words) gecos utmp

//! Porte pseudo-linus: `pinky` escrito sobre o uucore portado e o sysio, seguindo o pinky.c do GNU
//! coreutils 9.7. O utmp é lido como o `who` lê (`uucore::utmpx`); sem utmp sai só o cabeçalho.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::path::PathBuf;

use clap::{Arg, ArgAction, Command};
use sysio::io::{Write as _, stdout};
use sysio::os::unix::fs::MetadataExt as _;
use uucore::entries::{Locate, Passwd};
use uucore::error::{UResult, UUsageError};
use uucore::utmpx::{self, Utmpx, UtmpxRecord};
use uucore::{format_usage, translate};

mod options {
    pub const LONG: &str = "long";
    pub const OMIT_HOME_SHELL: &str = "omit-home-shell";
    pub const OMIT_PROJECT: &str = "omit-project";
    pub const OMIT_PLAN: &str = "omit-plan";
    pub const SHORT: &str = "short";
    pub const OMIT_HEADING: &str = "omit-heading";
    pub const OMIT_NAME: &str = "omit-name";
    pub const OMIT_NAME_HOST: &str = "omit-name-host";
    pub const OMIT_NAME_HOST_TIME: &str = "omit-name-host-time";
    pub const LOOKUP: &str = "lookup";
    pub const HELP: &str = "help";
    pub const USER: &str = "user";
}

/// O bit de escrita do grupo (`S_IWGRP`), sem a libc.
const S_IWGRP: u32 = 0o020;

struct Pinky {
    heading: bool,
    fullname: bool,
    where_: bool,
    idle: bool,
    home_and_shell: bool,
    project: bool,
    plan: bool,
    lookup: bool,
    names: Vec<String>,
}

#[uucore::main]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let matches = uucore::clap_localization::handle_clap_result(uu_app(), args)?;

    let flag = |name: &str| matches.get_flag(name);
    let omit_name_host_time = flag(options::OMIT_NAME_HOST_TIME);
    let omit_name_host = flag(options::OMIT_NAME_HOST) || omit_name_host_time;

    // `-s` e `-l` valem na ordem em que aparecem: o último ganha.
    let last = |name: &str| matches.indices_of(name).and_then(Iterator::last);
    let short = match (last(options::SHORT), last(options::LONG)) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some(s), Some(l)) => s > l,
    };

    let pinky = Pinky {
        heading: !flag(options::OMIT_HEADING),
        fullname: !(flag(options::OMIT_NAME) || omit_name_host),
        where_: !omit_name_host,
        idle: !omit_name_host_time,
        home_and_shell: !flag(options::OMIT_HOME_SHELL),
        project: !flag(options::OMIT_PROJECT),
        plan: !flag(options::OMIT_PLAN),
        lookup: flag(options::LOOKUP),
        names: matches
            .get_many::<String>(options::USER)
            .map(|v| v.cloned().collect())
            .unwrap_or_default(),
    };

    let mut out: Vec<u8> = Vec::new();
    if short {
        pinky.short(&mut out);
    } else {
        if pinky.names.is_empty() {
            return Err(UUsageError::new(
                1,
                "no username specified; at least one must be specified when using -l",
            ));
        }
        for name in &pinky.names {
            pinky.long_entry(name, &mut out);
        }
    }
    stdout().write_all(&out)?;
    Ok(())
}

/// O formato da hora: `%b %e %H:%M` só no locale `C` (o `hard_locale` do GNU é falso), senão o ISO.
/// Mesma regra do `who` do pseudo-linus.
fn time_pattern() -> &'static str {
    if ["LC_ALL", "LC_TIME", "LANG"]
        .into_iter()
        .find_map(sysio::env::var_os)
        .as_deref()
        == Some(OsStr::new("C"))
    {
        "%b %e %H:%M"
    } else {
        "%Y-%m-%d %H:%M"
    }
}

fn time_width() -> usize {
    if time_pattern() == "%b %e %H:%M" { 12 } else { 16 }
}

/// `create_fullname` do GNU: o gecos com cada `&` trocado pelo nome de usuário com a inicial
/// maiúscula.
fn create_fullname(gecos: &str, user: &str) -> String {
    let mut result = String::with_capacity(gecos.len());
    for c in gecos.chars() {
        if c == '&' {
            let mut chars = user.chars();
            if let Some(first) = chars.next() {
                if first.is_ascii_lowercase() {
                    result.push(first.to_ascii_uppercase());
                } else {
                    result.push(first);
                }
                result.push_str(chars.as_str());
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// O usuário do `getpwnam` (só pelo nome, nunca pelo número).
fn getpwnam(name: &str) -> Option<Passwd> {
    Passwd::locate(name).ok().filter(|pw| pw.name == name)
}

/// `idle_string` do pinky: espaços abaixo de um minuto, `HH:MM` abaixo de um dia, `Nd` acima.
fn idle_string(when: i64) -> String {
    let now = sysio::time::now()
        .duration_since(sysio::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let idle = now - when;
    if idle < 60 {
        "     ".to_string()
    } else if idle < 24 * 60 * 60 {
        format!("{:02}:{:02}", idle / 3600, (idle % 3600) / 60)
    } else {
        format!("{}d", idle / (24 * 60 * 60))
    }
}

/// `%-W.Ps` do printf: corta em `P` bytes e completa com espaços até `W`.
fn push_padded_truncated(out: &mut Vec<u8>, text: &str, width: usize) {
    let bytes = &text.as_bytes()[..text.len().min(width)];
    out.extend_from_slice(bytes);
    out.extend(std::iter::repeat_n(b' ', width - bytes.len()));
}

impl Pinky {
    fn short(&self, out: &mut Vec<u8>) {
        if self.heading {
            self.heading(out);
        }
        for ut in Utmpx::iter_all_records_from(utmpx::DEFAULT_FILE) {
            if !ut.is_user_process() {
                continue;
            }
            if !self.names.is_empty() && !self.names.iter().any(|n| *n == ut.user()) {
                continue;
            }
            self.entry(&ut, out);
        }
    }

    fn heading(&self, out: &mut Vec<u8>) {
        let mut line = format!("{:<8}", "Login");
        if self.fullname {
            write!(line, " {:<19}", " Name").unwrap();
        }
        write!(line, " {:<9}", " TTY").unwrap();
        if self.idle {
            write!(line, " {:<6}", "Idle").unwrap();
        }
        write!(line, " {:<width$}", "When", width = time_width()).unwrap();
        if self.where_ {
            line.push_str(" Where");
        }
        line.push('\n');
        out.extend_from_slice(line.as_bytes());
    }

    fn entry(&self, ut: &UtmpxRecord, out: &mut Vec<u8>) {
        let tty = ut.tty_device();
        let dev = if tty.starts_with('/') {
            PathBuf::from(&tty)
        } else {
            PathBuf::from("/dev").join(&tty)
        };
        let (mesg, last_change) = match sysio::path::PathExt::sys_metadata(&dev) {
            Ok(meta) => (if meta.mode() & S_IWGRP == 0 { '*' } else { ' ' }, meta.atime()),
            Err(_) => ('?', 0),
        };

        let user = ut.user();
        let mut line = format!("{user:<8}");
        if self.fullname {
            match getpwnam(&user) {
                None => write!(line, " {:>19}", "        ???").unwrap(),
                Some(pw) => {
                    let gecos = pw.user_info.unwrap_or_default();
                    let gecos = gecos.split(',').next().unwrap_or_default();
                    line.push(' ');
                    let mut bytes = std::mem::take(&mut line).into_bytes();
                    push_padded_truncated(&mut bytes, &create_fullname(gecos, &pw.name), 19);
                    out.extend_from_slice(&bytes);
                }
            }
        }
        write!(line, " {mesg}{tty:<8}").unwrap();
        if self.idle {
            if last_change != 0 {
                write!(line, " {:<6}", idle_string(last_change)).unwrap();
            } else {
                write!(line, " {:<6}", "?????").unwrap();
            }
        }
        write!(line, " {}", ut.login_time().strftime(time_pattern())).unwrap();

        let host = ut.host();
        if self.where_ && !host.is_empty() {
            let (name, display) = match host.split_once(':') {
                Some((h, d)) => (h.to_string(), Some(d.to_string())),
                None => (host.clone(), None),
            };
            let resolved = if self.lookup && !name.is_empty() {
                ut.canon_host()
                    .ok()
                    .map(|c| c.split(':').next().unwrap_or_default().to_string())
                    .unwrap_or(name)
            } else {
                name
            };
            match display {
                Some(d) => write!(line, " {resolved}:{d}").unwrap(),
                None => write!(line, " {resolved}").unwrap(),
            }
        }
        line.push('\n');
        out.extend_from_slice(line.as_bytes());
    }

    fn long_entry(&self, name: &str, out: &mut Vec<u8>) {
        let mut text = format!("Login name: {name:<28}In real life: ");
        let Some(pw) = getpwnam(name) else {
            text.push_str(" ???\n");
            out.extend_from_slice(text.as_bytes());
            return;
        };
        let gecos = pw.user_info.clone().unwrap_or_default();
        write!(text, " {}", create_fullname(&gecos, &pw.name)).unwrap();
        text.push('\n');
        let dir = pw.user_dir.clone().unwrap_or_default();
        if self.home_and_shell {
            write!(text, "Directory: {dir:<29}Shell: ").unwrap();
            write!(text, " {}", pw.user_shell.clone().unwrap_or_default()).unwrap();
            text.push('\n');
        }
        out.extend_from_slice(text.as_bytes());

        if self.project
            && let Ok(content) = sysio::fs::read(format!("{dir}/.project"))
        {
            out.extend_from_slice(b"Project: ");
            out.extend_from_slice(&content);
        }
        if self.plan
            && let Ok(content) = sysio::fs::read(format!("{dir}/.plan"))
        {
            out.extend_from_slice(b"Plan:\n");
            out.extend_from_slice(&content);
        }
        out.push(b'\n');
    }
}

pub fn uu_app() -> Command {
    Command::new("pinky")
        .version(uucore::crate_version!())
        .help_template(uucore::localized_help_template("pinky"))
        .about(translate!("pinky-about", "default_file" => utmpx::DEFAULT_FILE))
        .override_usage(format_usage(&translate!("pinky-usage")))
        .infer_long_args(true)
        .disable_help_flag(true)
        .arg(Arg::new(options::LONG).short('l').help(translate!("pinky-help-long")).action(ArgAction::Count))
        .arg(
            Arg::new(options::OMIT_HOME_SHELL)
                .short('b')
                .help(translate!("pinky-help-omit-home-shell"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::OMIT_PROJECT)
                .short('h')
                .help(translate!("pinky-help-omit-project"))
                .action(ArgAction::SetTrue),
        )
        .arg(Arg::new(options::OMIT_PLAN).short('p').help(translate!("pinky-help-omit-plan")).action(ArgAction::SetTrue))
        .arg(Arg::new(options::SHORT).short('s').help(translate!("pinky-help-short")).action(ArgAction::Count))
        .arg(
            Arg::new(options::OMIT_HEADING)
                .short('f')
                .help(translate!("pinky-help-omit-heading"))
                .action(ArgAction::SetTrue),
        )
        .arg(Arg::new(options::OMIT_NAME).short('w').help(translate!("pinky-help-omit-name")).action(ArgAction::SetTrue))
        .arg(
            Arg::new(options::OMIT_NAME_HOST)
                .short('i')
                .help(translate!("pinky-help-omit-name-host"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::OMIT_NAME_HOST_TIME)
                .short('q')
                .help(translate!("pinky-help-omit-name-host-time"))
                .action(ArgAction::SetTrue),
        )
        .arg(Arg::new(options::LOOKUP).long(options::LOOKUP).help(translate!("pinky-help-lookup")).action(ArgAction::SetTrue))
        .arg(Arg::new(options::HELP).long(options::HELP).action(ArgAction::Help))
        .arg(Arg::new(options::USER).action(ArgAction::Append).num_args(1..))
}
