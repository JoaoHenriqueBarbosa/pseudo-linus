//! `rtcwake` do util-linux 2.41: entra em um estado de suspensão até uma hora de despertar.
//!
//! Porte do `sys-utils/rtcwake.c`, no recorte do sandbox: não há relógio RTC (`/dev/rtc0`) nem
//! `/sys/power/state` acessível, então o programa valida os argumentos e termina com o erro de
//! dispositivo ausente, como num contêiner sem privilégio.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("auto", HasArg::No, b'a' as i32),
    LongOpt::new("device", HasArg::Required, b'd' as i32),
    LongOpt::new("dry-run", HasArg::No, b'n' as i32),
    LongOpt::new("local", HasArg::No, b'l' as i32),
    LongOpt::new("list-modes", HasArg::No, 0x100),
    LongOpt::new("mode", HasArg::Required, b'm' as i32),
    LongOpt::new("seconds", HasArg::Required, b's' as i32),
    LongOpt::new("time", HasArg::Required, b't' as i32),
    LongOpt::new("utc", HasArg::No, b'u' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const KNOWN_MODES: &[&str] = &[
    "standby", "mem", "freeze", "disk", "no", "off", "on", "disable", "show",
];

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [options] [-d <device> | --device <device>] [-m standby | --mode standby] [-s <seconds> | --seconds <seconds> | -t <time_t> | --time <time_t>]

Enter a system sleep state until specified wakeup time.

Options:
 -a, --auto               reads the clock mode from adjust file (default)
 -d, --device <device>    select rtc device (rtc0|rtc1|...)
 -n, --dry-run            does everything, but suspend
 -l, --local              RTC uses local timezone
     --list-modes         list available modes
 -m, --mode <mode>        standby|mem|... sleep mode
 -s, --seconds <seconds>  seconds to sleep
 -t, --time <time_t>      time to wake
 -u, --utc                RTC uses UTC
 -v, --verbose            verbose messages

 -h, --help               display this help
 -V, --version            display version

For more details see {short}(8).
"
    )
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut device = String::from("/dev/rtc0");
    let mut mode = String::from("standby");
    let mut seconds: Option<u32> = None;
    let mut time: Option<u64> = None;
    let mut list_modes = false;
    let mut clock: Option<char> = None;

    let mut g = Getopt::from_env(&argv[1..], "ahd:lm:ns:t:uvV", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        if o.id == 0x100 {
            list_modes = true;
            continue;
        }
        match o.short() {
            Some('a') | Some('l') | Some('u') => {
                let c = o.short().unwrap_or('a');
                if clock.is_some_and(|p| p != c) {
                    ul::warnx(
                        &short,
                        "--auto, --local and --utc are mutually exclusive",
                    );
                    return 1;
                }
                clock = Some(c);
            }
            Some('d') => {
                let a = o.arg_str();
                device = match a.strip_prefix("/dev/") {
                    Some(n) => format!("/dev/{n}"),
                    None => format!("/dev/{a}"),
                };
            }
            Some('m') => mode = o.arg_str(),
            Some('n') | Some('v') => {}
            Some('s') => {
                match ul::strtou32_or_err(&o.arg.clone().unwrap_or_default(), "invalid seconds argument") {
                    Ok(v) => seconds = Some(v),
                    Err(m) => {
                        ul::warnx(&short, m);
                        return 1;
                    }
                }
            }
            Some('t') => {
                match ul::strtou64_or_err(&o.arg.clone().unwrap_or_default(), "invalid time argument") {
                    Ok(v) => time = Some(v),
                    Err(m) => {
                        ul::warnx(&short, m);
                        return 1;
                    }
                }
            }
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
                return 0;
            }
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }

    if list_modes {
        return match sys::read_file(b"/sys/power/state") {
            Ok(d) => {
                let text = String::from_utf8_lossy(&d);
                let mut out = io::stdout();
                let _ = out.write_all(format!("{}\n", text.trim()).as_bytes());
                0
            }
            Err(e) => {
                ul::warn(&short, "could not read: /sys/power/state", e);
                1
            }
        };
    }

    if !g.operands().is_empty() {
        ul::errtryhelp(&short);
        return 1;
    }
    if seconds.is_some() && time.is_some() {
        ul::warnx(&short, "--time and --seconds are mutually exclusive");
        return 1;
    }
    let passive = mode == "disable" || mode == "show";
    if seconds.is_none() && time.is_none() && !passive {
        ul::warnx(&short, "must provide wake time (see --seconds or --time)");
        return 1;
    }
    if !KNOWN_MODES.contains(&mode.as_str()) {
        ul::warnx(&short, format!("unrecognized suspend state '{mode}'"));
        return 1;
    }

    match io::File::open(device.as_bytes()) {
        Ok(_) => {
            ul::warnx(&short, format!("{device}: unable to read the RTC: Permission denied"));
            1
        }
        Err(e) => {
            ul::warn(&short, format!("{device}: unable to find device"), e);
            1
        }
    }
}
