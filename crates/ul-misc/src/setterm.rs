//! `setterm` do util-linux 2.41: ajusta atributos do terminal.
//!
//! Cobre `--help`, `--version`, o erro de opção desconhecida e as opções liga/desliga e de cor que
//! só escrevem sequências de escape no stdout (`--cursor`, `--linewrap`, `--bold`, `--foreground`...).
//! As que dependem de ioctl de console (`--blank`, `--powersave`, `--dump`, `--tabs`...) são aceitas
//! mas só validam o argumento, porque o sandbox não tem console de verdade.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Ctx;

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, GetoptError, HasArg, LongOpt};

const COLORS: &[&str] = &[
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

const LONGS: &[LongOpt] = &[
    LongOpt::new("term", HasArg::Required, 256),
    LongOpt::new("reset", HasArg::No, 257),
    LongOpt::new("resize", HasArg::No, 258),
    LongOpt::new("initialize", HasArg::No, 259),
    LongOpt::new("default", HasArg::No, 260),
    LongOpt::new("store", HasArg::No, 261),
    LongOpt::new("cursor", HasArg::Required, 262),
    LongOpt::new("repeat", HasArg::Required, 263),
    LongOpt::new("appcursorkeys", HasArg::Required, 264),
    LongOpt::new("linewrap", HasArg::Required, 265),
    LongOpt::new("inversescreen", HasArg::Required, 266),
    LongOpt::new("foreground", HasArg::Required, 267),
    LongOpt::new("background", HasArg::Required, 268),
    LongOpt::new("bold", HasArg::Required, 269),
    LongOpt::new("half-bright", HasArg::Required, 270),
    LongOpt::new("blink", HasArg::Required, 271),
    LongOpt::new("reverse", HasArg::Required, 272),
    LongOpt::new("underline", HasArg::Required, 273),
    LongOpt::new("clear", HasArg::Optional, 274),
    LongOpt::new("msg", HasArg::Required, 275),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 setterm [options]

Set the attributes of a terminal.

Options:
 --term <terminal_name>        override TERM environment variable
 --reset                       reset terminal to power-on state
 --resize                      reset terminal rows and columns
 --initialize                  display init string, and use default settings
 --default                     use default terminal settings
 --store                       save current terminal settings as default

 --cursor on|off               display cursor
 --repeat on|off               keyboard repeat
 --appcursorkeys on|off        cursor key application mode
 --linewrap on|off             continue on a new line when a line is full
 --inversescreen on|off        swap colors for the whole screen

 --msg on|off                  send kernel messages to console
 --msglevel <0-8>              kernel console log level

 --foreground default|<color>  set foreground color
 --background default|<color>  set background color
 --ulcolor [bright] <color>    set underlined text color
 --hbcolor [bright] <color>    set half-bright text color
        <color>: black blue cyan green grey magenta red white yellow

 --bold on|off                 bold
 --half-bright on|off          dim
 --blink on|off                blink
 --underline on|off            underline
 --reverse  on|off             swap foreground and background colors

 --clear[=<all|rest>]          clear screen and set cursor position
 --tabs[=<number>...]          set these tab stop positions, or show them
 --clrtabs[=<number>...]       clear these tab stop positions, or all
 --regtabs[=1-160]             set a regular tab stop interval
 --blank[=0-60|force|poke]     set time of inactivity before screen blanks

 --dump[=<number>]             write vcsa<number> console dump to file
 --append <number>             append vcsa<number> console dump to file
 --file <filename>             name of the dump file

 --powersave on|vsync|hsync|powerdown|off
                               set vesa powersaving features
 --powerdown[=<0-60>]          set vesa powerdown interval in minutes

 --blength[=<0-2000>]          duration of the bell in milliseconds
 --bfreq[=<number>]            bell frequency in Hertz

 --help                        display this help
 --version                     display version

For more details see setterm(1).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn onoff(short: &str, name: &str, arg: &[u8]) -> Result<bool, ()> {
    match arg {
        b"on" => Ok(true),
        b"off" => Ok(false),
        _ => {
            ul::warnx(
                short,
                format!("argument error: {} {}", name, io::lossy(arg)),
            );
            Err(())
        }
    }
}

fn color(short: &str, name: &str, arg: &[u8]) -> Result<u8, ()> {
    if arg == b"default" {
        return Ok(9);
    }
    match COLORS.iter().position(|c| c.as_bytes() == arg) {
        Some(i) => Ok(i as u8),
        None => {
            ul::warnx(
                short,
                format!("argument error: {} {}", name, io::lossy(arg)),
            );
            Err(())
        }
    }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut esc: Vec<u8> = Vec::new();
    // O original usa `getopt_long_only` sem opções curtas: `-x` é tratado como longa `--x`, e a
    // falha mostra o argumento com um traço só.
    let converted: Vec<Vec<u8>> = argv[1..]
        .iter()
        .map(|a| {
            if a.len() >= 2 && a[0] == b'-' && a[1] != b'-' {
                let mut c = b"-".to_vec();
                c.extend_from_slice(a);
                c
            } else {
                a.clone()
            }
        })
        .collect();
    let mut g = Getopt::from_env(&converted, "", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                let e = match e {
                    GetoptError::Unrecognized(s)
                        if argv[1..].iter().any(|a| a.as_slice() == s[1..].as_bytes()) =>
                    {
                        GetoptError::Unrecognized(s[1..].to_string())
                    }
                    other => other,
                };
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        let arg = o.arg.clone().unwrap_or_default();
        let name = o.spelled.clone();
        let seq: String = match o.id {
            262 | 263 | 264 | 265 | 266 => {
                let Ok(on) = onoff(&short, &name, &arg) else {
                    return 1;
                };
                let code = match o.id {
                    262 => 25,
                    263 => 8,
                    264 => 1,
                    265 => 7,
                    _ => 5,
                };
                format!("\x1b[?{code}{}", if on { 'h' } else { 'l' })
            }
            267 | 268 => {
                let Ok(c) = color(&short, &name, &arg) else {
                    return 1;
                };
                let base = if o.id == 267 { 30 } else { 40 };
                format!("\x1b[{}m", base + c as u32)
            }
            269..=273 => {
                let Ok(on) = onoff(&short, &name, &arg) else {
                    return 1;
                };
                let (a, b) = match o.id {
                    269 => (1, 22),
                    270 => (2, 22),
                    271 => (5, 25),
                    272 => (7, 27),
                    _ => (4, 24),
                };
                format!("\x1b[{}m", if on { a } else { b })
            }
            274 => match arg.as_slice() {
                b"" | b"all" => "\x1b[H\x1b[J".to_string(),
                b"rest" => "\x1b[J".to_string(),
                _ => {
                    ul::warnx(&short, format!("argument error: {} {}", name, io::lossy(&arg)));
                    return 1;
                }
            },
            275 => {
                if onoff(&short, &name, &arg).is_err() {
                    return 1;
                }
                String::new()
            }
            _ => match o.short() {
                Some('h') => {
                    let mut out = io::stdout();
                    let _ = out.write_all(USAGE.as_bytes());
                    return 0;
                }
                _ => String::new(),
            },
        };
        esc.extend_from_slice(seq.as_bytes());
    }
    if !esc.is_empty() {
        let mut out = io::stdout();
        let _ = out.write_all(&esc);
    }
    0
}
