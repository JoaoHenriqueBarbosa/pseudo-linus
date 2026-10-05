//! `tput` do ncurses 6.5.20250216 (`tput.c`).

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Fd, sys};

use super::clear::clear_cmd;
use super::reset::Reset;
use super::terminfo::{SetupOpts, Term, TiStr, isatty, setupterm};
use super::tparm::{Arg, ParmState, analyze, tiparm, tparm, tputs};
use super::{StdoutSink, VERSION, err_system, find_type_entry, rootname, save_tty_settings, strtol, Kind};
use crate::util::io;
use crate::util::{Getopt, GetoptError};

const ERR_USAGE: i32 = 2;
const ERR_TERMTYPE: i32 = 3;
const ERR_CAPNAME: i32 = 4;

/// `TParams` do `tparm_type.h`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum TParams {
    Numbers,
    Str,
    StrStr,
    NumStr,
    NumStrStr,
    Other,
}

/// `tparm_type(name)`: o que a capacidade espera, pelos nomes (longo, terminfo, termcap).
fn tparm_type(name: &[u8]) -> TParams {
    const TABLE: &[(TParams, [&str; 3])] = &[
        (TParams::NumStr, ["pkey_key", "pfkey", "pk"]),
        (TParams::NumStr, ["pkey_local", "pfloc", "pl"]),
        (TParams::NumStr, ["pkey_xmit", "pfx", "px"]),
        (TParams::NumStr, ["plab_norm", "pln", "pn"]),
        (TParams::NumStrStr, ["pkey_plab", "pfxl", "xl"]),
        (TParams::Str, ["Cs", "Cs", "Cs"]),
        (TParams::StrStr, ["Ms", "Ms", "Ms"]),
    ];
    for (code, names) in TABLE {
        if names.iter().any(|n| n.as_bytes() == name) {
            return *code;
        }
    }
    TParams::Numbers
}

struct Tput {
    progname: String,
    term: Term,
    state: ParmState,
    opt_v: bool,
    opt_x: bool,
    is_init: bool,
    is_reset: bool,
    is_clear: bool,
}

fn quit(progname: &str, status: i32, msg: &str) -> ! {
    io::eprint(format!("{progname}: {msg}\n"));
    sys::exit(status)
}

/// `usage()`: com `optstring`, só as opções listadas (e o corte de linha do original, que come o
/// primeiro espaço da linha seguinte à que foi pulada).
fn usage(progname: &str, optstring: Option<&str>) -> ! {
    let msg = "\nOptions:\n  -S <<       read commands from standard input\n  -T TERM     use this instead of $TERM\n  -V          print curses-version\n  -v          verbose, show warnings\n  -x          do not try to clear scrollback\n\nCommands:\n  clear       clear the screen\n  init        initialize the terminal\n  reset       reinitialize the terminal\n  capname     unlike clear/init/reset, print value for capability \"capname\"\n";
    let mut text = format!("Usage: {progname} [options] [command]\n").into_bytes();
    match optstring {
        Some(opts) => {
            let m = msg.as_bytes();
            let mut s = 0usize;
            while s < m.len() {
                text.push(m[s]);
                if m[s..].starts_with(b"  -") {
                    if !opts.as_bytes().contains(&m[s + 3]) {
                        s = s + m[s..].iter().position(|b| *b == b'\n').unwrap_or(0) + 1;
                    }
                } else if m[s..].starts_with(b"\n\nC") {
                    break;
                }
                s += 1;
            }
        }
        None => text.extend_from_slice(msg.as_bytes()),
    }
    io::eprint(text);
    sys::exit(ERR_USAGE)
}

impl Tput {
    fn check_aliases(&mut self, name: &[u8], program: bool) -> Vec<u8> {
        let _ = program;
        self.is_init = name == b"init";
        self.is_reset = name == b"reset";
        self.is_clear = name == b"clear";
        if self.is_clear {
            return b"clear".to_vec();
        }
        if self.is_reset {
            return b"reset".to_vec();
        }
        if self.is_init {
            return b"init".to_vec();
        }
        name.to_vec()
    }

    /// `putp(s)`: `tputs(s, 1, putchar)`.
    fn putp(&self, s: &[u8], always_delay: bool) {
        tputs(Some(&self.term.tt), s, 1, always_delay, &mut StdoutSink);
    }

    /// `tput_cmd`: devolve o código e quantos argumentos consumiu.
    fn tput_cmd(&mut self, fd: Fd, argv: &[Vec<u8>]) -> (i32, usize) {
        let argc = argv.len();
        let name = self.check_aliases(&argv[0], false);
        let mut used = 1usize;
        if self.is_reset || self.is_init {
            let is_reset = self.is_reset;
            let progname = self.progname.clone();
            let mut r = Reset::new(&self.term, Fd::STDOUT, is_reset, !is_reset, &progname);
            // `set_window_size`: com o tamanho do terminal, ele vale; os modos (termios) não passam
            // pelo `sysabi`.
            if isatty(fd)
                && let Some(s) = sys::try_current()
                && let Ok(ws) = s.tcgetwinsize(fd)
                && ws.rows > 0
                && ws.cols > 0
            {
                r.columns = i32::from(ws.cols);
            }
            if r.send_init_strings() {
                r.flush();
            }
            r.flush();
            return (0, used);
        }
        if name == b"longname" {
            let mut out = io::stdout();
            let _ = out.write_all(&self.term.longname());
            return (0, used);
        }
        if name == b"clear" {
            return (if clear_cmd(&self.term, self.opt_x) { 0 } else { ERR_USAGE }, used);
        }
        let status = self.term.tigetflag(&name);
        if status != -1 {
            return (i32::from(status == 0), used);
        }
        let status = self.term.tigetnum(&name);
        if status != -2 {
            let mut out = io::stdout();
            let _ = writeln!(out, "{status}");
            return (0, used);
        }
        let s: Vec<u8> = match self.term.tigetstr(&name) {
            TiStr::NotCap => quit(&self.progname, ERR_CAPNAME, &format!("unknown terminfo capability '{}'", io::lossy(&name))),
            TiStr::Absent => return (1, used),
            TiStr::Val(v) => v.to_vec(),
        };
        let mut param_type = tparm_type(&name);
        // Uma capacidade estendida (fora da tabela predefinida) é analisada pela cadeia.
        if param_type == TParams::Numbers && find_type_entry(&name, Kind::Str).is_none() {
            param_type = TParams::Other;
        }
        // `strtol` dos argumentos: o que não é número vira 0.
        let mut numbers = [0i64; 10];
        let mut strings: [Option<Vec<u8>>; 10] = Default::default();
        let mut k = 1;
        while k < argc && k <= 9 {
            strings[k] = Some(argv[k].clone());
            let (v, end) = strtol(&argv[k]);
            numbers[k] = if end != argv[k].len() { 0 } else { v };
            k += 1;
        }
        let mut popcount: i32 = 0;
        let mut analyzed: i32 = match param_type {
            TParams::Str => 1,
            TParams::StrStr | TParams::NumStr => 2,
            TParams::NumStrStr => 3,
            TParams::Numbers | TParams::Other => {
                let a = analyze(&s);
                popcount = a.popcount;
                a.number
            }
        };
        if analyzed < popcount {
            analyzed = popcount;
        }
        let mut out_str: Option<Vec<u8>> = Some(s.clone());
        let mut always_delay = false;
        if argc > 1 {
            self.state.reset();
            // Quantos argumentos numéricos (não negativos) seguem o nome.
            let mut provided = 0i32;
            for (narg, arg) in argv.iter().enumerate().skip(1) {
                let (check, end) = strtol(arg);
                if check < 0 || end == 0 || end != arg.len() {
                    break;
                }
                provided = narg as i32;
            }
            let sarg = |i: usize| Arg::Str(strings[i].clone());
            let narg = |i: usize| Arg::Num(numbers[i]);
            match param_type {
                TParams::Str => {
                    out_str = tparm(&self.term.tt, &mut self.state, &s, &[sarg(1)]);
                    if provided == 0 {
                        provided += 1;
                    }
                }
                TParams::StrStr => {
                    out_str = tparm(&self.term.tt, &mut self.state, &s, &[sarg(1), sarg(2)]);
                    if provided == 0 {
                        provided += 1;
                    }
                    if provided == 1 && argc >= 2 {
                        provided += 1;
                    }
                }
                TParams::NumStr => {
                    out_str = tparm(&self.term.tt, &mut self.state, &s, &[narg(1), sarg(2)]);
                    if provided == 1 && argc >= 2 {
                        provided += 1;
                    }
                }
                TParams::NumStrStr => {
                    out_str = tparm(&self.term.tt, &mut self.state, &s, &[narg(1), sarg(2), sarg(3)]);
                    if provided == 1 && argc >= 2 {
                        provided += 1;
                    }
                    if provided == 2 && argc >= 3 {
                        provided += 1;
                    }
                }
                TParams::Numbers | TParams::Other => {
                    let args: Vec<i64> = (1..=9).map(|i| numbers[i]).collect();
                    out_str = tiparm(&self.term.tt, &mut self.state, analyzed, &s, &args);
                }
            }
            if self.opt_v && analyzed != provided {
                io::eprint(format!(
                    "{}: {} parameters for \"{}\"\n",
                    self.progname,
                    if analyzed < provided { "extra" } else { "missing" },
                    io::lossy(&argv[0])
                ));
            }
            used += provided.max(0) as usize;
        } else {
            if self.opt_v {
                io::eprint(format!("{}: missing parameters for \"{}\"\n", self.progname, io::lossy(&argv[0])));
            }
            // A própria cadeia da capacidade: `bell` e `flash` esperam mesmo sem `padding_baud_rate`.
            always_delay = name == b"bel" || name == b"flash";
        }
        if let Some(o) = out_str {
            self.putp(&o, always_delay);
        }
        (0, used)
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let root = rootname(&argv[0]).to_vec();
    let mut tput = Tput {
        progname: io::lossy(&root),
        term: Term { tt: Default::default(), fd: Fd::STDOUT, termname: Vec::new() },
        state: ParmState::default(),
        opt_v: false,
        opt_x: false,
        is_init: false,
        is_reset: false,
        is_clear: false,
    };
    let aliased = tput.check_aliases(&root, true);
    tput.progname = io::lossy(&aliased);
    let is_alias = tput.is_clear || tput.is_reset || tput.is_init;
    let progname = tput.progname.clone();

    let mut term: Option<Vec<u8>> = sys::getenv("TERM");
    let mut opts = SetupOpts::default();
    let mut cmdline = true;
    let spec = if is_alias { "T:Vvx" } else { "ST:Vvx" };
    let mut g = Getopt::from_env(&argv[1..], spec, &[]);
    while let Some(r) = g.next_opt() {
        match r {
            Ok(o) => match o.short() {
                Some('S') => cmdline = false,
                Some('T') => {
                    opts = SetupOpts { use_env: false, use_tioctl: true };
                    term = o.arg.clone();
                }
                Some('V') => {
                    let mut out = io::stdout();
                    let _ = writeln!(out, "{VERSION}");
                    return 0;
                }
                Some('v') => tput.opt_v = true,
                Some('x') => tput.opt_x = true,
                _ => usage(&progname, if is_alias { Some("TVx") } else { None }),
            },
            Err(e) => bad_option(&e, &argv0, &progname, is_alias),
        }
    }
    let operands = g.operands();
    let need_tty = tput.is_reset
        || tput.is_init
        || operands.first().is_some_and(|a| a == b"reset" || a == b"init");

    // `argv` sem as opções processadas; no alias o primeiro item é o nome do programa.
    let mut cmds: Vec<Vec<u8>> = Vec::new();
    if is_alias {
        cmds.push(progname.clone().into_bytes());
    }
    cmds.extend(operands);

    let term_bytes = match &term {
        Some(t) if !t.is_empty() => t.clone(),
        _ => quit(&progname, ERR_USAGE, "No value for $TERM and no -T specified"),
    };
    let fd = save_tty_settings(&progname, need_tty);
    tput.term = match setupterm(Some(&term_bytes), fd, opts) {
        Ok(t) => t,
        Err(f) => match (f.code, f.term) {
            (code, Some(t)) if code > 0 => t,
            _ => quit(&progname, ERR_TERMTYPE, &format!("unknown terminal \"{}\"", io::lossy(&term_bytes))),
        },
    };

    if cmdline {
        let mut code = 0;
        if cmds.is_empty() && !is_alias {
            usage(&progname, None);
        }
        let mut rest: &[Vec<u8>] = &cmds;
        while !rest.is_empty() {
            let (c, used) = tput.tput_cmd(fd, rest);
            code = c;
            if code != 0 {
                break;
            }
            rest = &rest[used.min(rest.len())..];
        }
        return code;
    }

    // -S: um comando por linha, em tokens separados por espaço.
    let input = io::read_stdin().unwrap_or_default();
    let mut result = 0;
    let mut pos = 0usize;
    while pos < input.len() {
        // `fgets(buf, BUFSIZ)`: uma linha ou 8191 bytes.
        let mut end = pos;
        while end < input.len() && end - pos < 8191 {
            end += 1;
            if input[end - 1] == b'\n' {
                break;
            }
        }
        let chunk = &input[pos..end];
        pos = end;
        let chunk = match chunk.iter().position(|b| *b == 0) {
            Some(p) => &chunk[..p],
            None => chunk,
        };
        let tokens: Vec<Vec<u8>> = chunk
            .split(|b| super::c_isspace(*b))
            .filter(|t| !t.is_empty())
            .map(<[u8]>::to_vec)
            .collect();
        let mut rest: &[Vec<u8>] = &tokens;
        while !rest.is_empty() {
            let (code, used) = tput.tput_cmd(fd, rest);
            if code != 0 {
                if result == 0 {
                    result = err_system(0);
                }
                result += 1;
            }
            rest = &rest[used.min(rest.len())..];
        }
    }
    result
}

fn bad_option(e: &GetoptError, argv0: &str, progname: &str, is_alias: bool) -> ! {
    io::eprint(format!("{}\n", e.message(argv0)));
    usage(progname, if is_alias { Some("TVx") } else { None })
}
