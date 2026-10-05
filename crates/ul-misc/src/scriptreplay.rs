//! `scriptreplay` do util-linux 2.41: reproduz uma sessão gravada pelo `script`, respeitando os
//! tempos do arquivo de timing.
//!
//! Formato clássico: linhas `<atraso> <bytes>`; o primeiro registro do typescript é o cabeçalho
//! `Script started ...`, descartado até a primeira quebra de linha. Formato avançado: linhas
//! `<tipo> <atraso> <bytes>`, onde só o tipo `O` (saída) é reproduzido e `H` é cabeçalho.

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use sysabi::{Ctx, OFlags, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("timing", HasArg::Required, b't' as i32),
    LongOpt::new("log-timing", HasArg::Required, b'T' as i32),
    LongOpt::new("typescript", HasArg::Required, b's' as i32),
    LongOpt::new("log-out", HasArg::Required, b'O' as i32),
    LongOpt::new("log-io", HasArg::Required, b'B' as i32),
    LongOpt::new("log-in", HasArg::Required, b'I' as i32),
    LongOpt::new("maxdelay", HasArg::Required, b'm' as i32),
    LongOpt::new("divisor", HasArg::Required, b'd' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 scriptreplay [options] [-t] timingfile [typescript [divisor]]

Play back typescripts, using timing information.

Options:
 -t, --timing <file>     script timing output file
 -T, --log-timing <file> alias to -t
 -s, --typescript <file> script terminal session output file
 -O, --log-out <file>    script terminal session output file
 -B, --log-io <file>     script terminal session log file with both input and output
 -I, --log-in <file>     script terminal session input file
 -m, --maxdelay <num>    wait at most this many seconds between updates
 -d, --divisor <num>     speed up or slow down execution with time divisor
 -h, --help              display this help
 -V, --version           display version

For more details see scriptreplay(1).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn parse_f64(text: &[u8]) -> Option<f64> {
    std::str::from_utf8(text).ok()?.trim().parse::<f64>().ok()
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut timing: Option<Vec<u8>> = None;
    let mut typescript: Option<Vec<u8>> = None;
    let mut divisor = 1.0f64;
    let mut maxdelay: Option<f64> = None;

    let mut g = Getopt::from_env(&argv[1..], "t:T:s:O:B:I:m:d:Vh", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        let arg = o.arg.clone().unwrap_or_default();
        match o.short() {
            Some('t') | Some('T') => timing = Some(arg),
            Some('s') | Some('O') | Some('B') | Some('I') => typescript = Some(arg),
            Some('d') => match parse_f64(&arg) {
                Some(v) => divisor = v,
                None => {
                    ul::warnx(&short, format!("unsupported divisor: '{}'", io::lossy(&arg)));
                    return 1;
                }
            },
            Some('m') => match parse_f64(&arg) {
                Some(v) => maxdelay = Some(v),
                None => {
                    ul::warnx(&short, format!("unsupported maxdelay: '{}'", io::lossy(&arg)));
                    return 1;
                }
            },
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(&USAGE.as_bytes()[1..]);
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }

    // Operandos posicionais: [timing] [typescript] [divisor].
    let mut ops = g.operands().into_iter();
    if timing.is_none() {
        timing = ops.next();
    }
    if typescript.is_none() {
        typescript = ops.next();
    }
    if let Some(d) = ops.next() {
        match parse_f64(&d) {
            Some(v) => divisor = v,
            None => {
                ul::warnx(&short, format!("unsupported divisor: '{}'", io::lossy(&d)));
                return 1;
            }
        }
    }
    let Some(timing) = timing else {
        ul::warnx(&short, "no timing file specified");
        ul::errtryhelp(&short);
        return 1;
    };
    let typescript = typescript.unwrap_or_else(|| b"typescript".to_vec());
    if divisor <= 0.0 {
        ul::warnx(&short, "unsupported divisor: must be greater than zero");
        return 1;
    }

    let tdata = match io::read_path(&timing) {
        Ok(d) => d,
        Err(e) => {
            ul::warn(&short, format!("cannot open {}", io::lossy(&timing)), e);
            return 1;
        }
    };
    let sdata = match io::read_path(&typescript) {
        Ok(d) => d,
        Err(e) => {
            ul::warn(&short, format!("cannot open {}", io::lossy(&typescript)), e);
            return 1;
        }
    };
    let _ = OFlags::RDONLY;

    // Pula o cabeçalho do typescript (primeira linha).
    let mut pos = sdata
        .iter()
        .position(|b| *b == b'\n')
        .map(|p| p + 1)
        .unwrap_or(sdata.len());

    let mut out = io::raw_stdout();
    let s = sys::current();
    for line in tdata.split(|b| *b == b'\n') {
        let text = String::from_utf8_lossy(line);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let mut parts = text.split_whitespace();
        let first = parts.next().unwrap_or("");
        let (kind, delay_s) = if first.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            (first.chars().next().unwrap(), parts.next().unwrap_or(""))
        } else {
            ('O', first)
        };
        let size_s = parts.next().unwrap_or("");
        let (Ok(delay), Ok(size)) = (delay_s.parse::<f64>(), size_s.parse::<usize>()) else {
            if kind == 'H' {
                continue;
            }
            ul::warnx(
                &short,
                format!("{}: line '{}': unexpected format", io::lossy(&timing), text),
            );
            return 1;
        };
        if kind != 'O' {
            continue;
        }
        let mut secs = delay;
        if let Some(m) = maxdelay {
            if secs > m {
                secs = m;
            }
        }
        secs /= divisor;
        if secs > 0.0 {
            let _ = out.flush();
            let _ = s.nanosleep(Duration::from_secs_f64(secs));
        }
        let end = (pos + size).min(sdata.len());
        let _ = out.write_all(&sdata[pos..end]);
        let _ = out.flush();
        pos = end;
    }
    let _ = out.flush();
    0
}
