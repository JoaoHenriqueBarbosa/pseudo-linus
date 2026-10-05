//! `ldattach` do util-linux 2.41: associa uma disciplina de linha a uma linha serial.
//!
//! O sandbox não tem tty: valida a disciplina e a velocidade como o original e, quando o
//! dispositivo abre, falha ao ler os atributos do terminal com `Inappropriate ioctl for device`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("debug", HasArg::No, b'd' as i32),
    LongOpt::new("speed", HasArg::Required, b's' as i32),
    LongOpt::new("intro-command", HasArg::Required, b'c' as i32),
    LongOpt::new("pause", HasArg::Required, b'p' as i32),
    LongOpt::new("sevenbits", HasArg::No, b'7' as i32),
    LongOpt::new("eightbits", HasArg::No, b'8' as i32),
    LongOpt::new("noparity", HasArg::No, b'n' as i32),
    LongOpt::new("evenparity", HasArg::No, b'e' as i32),
    LongOpt::new("oddparity", HasArg::No, b'o' as i32),
    LongOpt::new("onestopbit", HasArg::No, b'1' as i32),
    LongOpt::new("twostopbits", HasArg::No, b'2' as i32),
    LongOpt::new("iflag", HasArg::Required, b'i' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

const USAGE: &str = "
Usage:
 ldattach [options] <ldisc> <device>

Attach a line discipline to a serial line.

Options:
 -d, --debug             print verbose messages to stderr
 -s, --speed <value>     set serial line speed
 -c, --intro-command <string> intro sent before ldattach
 -p, --pause <seconds>   pause between intro and ldattach
 -7, --sevenbits         set character size to 7 bits
 -8, --eightbits         set character size to 8 bits
 -n, --noparity          set parity to none
 -e, --evenparity        set parity to even
 -o, --oddparity         set parity to odd
 -1, --onestopbit        set stop bits to one
 -2, --twostopbits       set stop bits to two
 -i, --iflag [-]<iflag>  set input mode flag

 -h, --help              display this help
 -V, --version           display version

Known <ldisc> names:
  TTY           SLIP          MOUSE         PPP           STRIP
  AX25          X25           6PACK         R3964         IRDA
  HDLC          SYNC_PPP      SYNCPPP       HCI           GIGASET_M101
  M101          GIGASET       PPS           GSM0710

Known <iflag> names:
  IGNBRK        BRKINT        IGNPAR        PARMRK        INPCK
  ISTRIP        INLCR         IGNCR         ICRNL         IUCLC
  IXON          IXANY         IXOFF         IMAXBEL       IUTF8

For more details see ldattach(8).
";

const LDISCS: &[&str] = &[
    "TTY", "SLIP", "MOUSE", "PPP", "STRIP", "AX25", "X25", "6PACK", "R3964", "IRDA", "HDLC",
    "SYNC_PPP", "SYNCPPP", "HCI", "GIGASET_M101", "M101", "GIGASET", "PPS", "GSM0710",
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut g = Getopt::from_env(&argv[1..], "ds:c:p:78neo12i:Vh", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        match o.short() {
            Some('d') | Some('7') | Some('8') | Some('n') | Some('e') | Some('o') | Some('1')
            | Some('2') | Some('i') | Some('c') | Some('p') => {}
            Some('s') => {
                let a = o.arg.as_deref().unwrap_or(b"");
                if let Err(m) = ul::strtou32_or_err(a, "invalid speed argument") {
                    ul::warnx(&short, m);
                    return 1;
                }
            }
            Some('h') => {
                let _ = io::stdout().write_all(USAGE.as_bytes());
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
    let ops = g.operands();
    if ops.len() != 2 {
        ul::warnx(&short, "not enough arguments");
        ul::errtryhelp(&short);
        return 1;
    }
    let name = io::lossy(&ops[0]);
    let valid = LDISCS.contains(&name.as_str()) || name.parse::<i64>().is_ok();
    if !valid {
        ul::warnx(&short, format!("invalid line discipline argument: '{name}'"));
        return 1;
    }
    let dev = &ops[1];
    let dev_s = io::lossy(dev);
    if let Err(e) = io::File::open(dev) {
        ul::warn(&short, format!("cannot open {dev_s}"), e);
        return 1;
    }
    ul::warn(
        &short,
        format!("cannot get terminal attributes for {dev_s}"),
        Errno::ENOTTY,
    );
    1
}
