//! `tset` e `reset` do ncurses 6.5.20250216 (`tset.c`). Os modos do terminal (termios) não passam
//! pelo `sysabi`: sem terminal o programa morre em `terminal attributes` como o original; com um
//! terminal, as cadeias de inicialização e as mensagens saem, mas os modos não são alterados e as
//! mudanças de caracteres de controle não são relatadas.

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use sysabi::{Ctx, Fd, sys};

use super::reset::Reset;
use super::terminfo::{SetupOpts, Term, isatty, setupterm};
use super::{VERSION, rootname, save_tty_settings};
use crate::util::io;
use crate::util::{Getopt, GetoptError};

const GT: u32 = 0x01;
const EQ: u32 = 0x02;
const LT: u32 = 0x04;
const NOT: u32 = 0x08;

/// `MAP`: `[porttype][teste de velocidade]:tipo`.
#[derive(Clone, Debug)]
struct Map {
    porttype: Option<Vec<u8>>,
    ty: Vec<u8>,
    conditional: u32,
    speed: i32,
}

/// `exit_error()`: uma linha em branco no erro e a saída 1.
fn exit_error() -> ! {
    io::eprint("\n");
    sys::exit(1)
}

/// `err()`: `tset: <mensagem>` mais `exit_error()`.
fn err(progname: &str, msg: &str) -> ! {
    io::eprint(format!("{progname}: {msg}"));
    exit_error()
}

/// `askuser(dflt)`: pergunta o tipo do terminal no erro padrão e lê do stdin.
struct Asker {
    input: Vec<u8>,
    pos: usize,
    at_eof_seen: bool,
}

impl Asker {
    fn new() -> Asker {
        Asker { input: Vec::new(), pos: 0, at_eof_seen: false }
    }

    fn fgets(&mut self) -> Option<Vec<u8>> {
        if !self.at_eof_seen {
            // Lê o stdin todo na primeira pergunta (o original lê por linha, o efeito é o mesmo).
            self.input = io::read_stdin().unwrap_or_default();
            self.at_eof_seen = true;
        }
        if self.pos >= self.input.len() {
            return None;
        }
        let mut end = self.pos;
        while end < self.input.len() && end - self.pos < 255 {
            end += 1;
            if self.input[end - 1] == b'\n' {
                break;
            }
        }
        let line = self.input[self.pos..end].to_vec();
        self.pos = end;
        Some(line)
    }

    fn ask(&mut self, dflt: Option<&[u8]>) -> Vec<u8> {
        loop {
            match dflt {
                Some(d) => io::eprint(format!("Terminal type? [{}] ", io::lossy(d))),
                None => io::eprint("Terminal type? "),
            }
            let Some(mut answer) = self.fgets() else {
                match dflt {
                    None => exit_error(),
                    Some(d) => return d.to_vec(),
                }
            };
            if let Some(p) = answer.iter().position(|b| *b == b'\n') {
                answer.truncate(p);
            }
            if !answer.is_empty() {
                return answer;
            }
            if let Some(d) = dflt {
                return d.to_vec();
            }
        }
    }
}

fn caseless_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
}

/// `tbaudrate`: a tabela do original só alcança até `134`, porque `134.5` repete o valor de `134` e
/// o teste de "velocidades crescentes" para ali.
fn tbaudrate(progname: &str, rate: &[u8]) -> i32 {
    const SPEEDS: &[(&str, i32)] = &[("0", 0), ("50", 1), ("75", 2), ("110", 3), ("134", 4), ("134.5", 4)];
    let rate = rate.strip_prefix(b"B").unwrap_or(rate);
    for (n, (name, speed)) in SPEEDS.iter().enumerate() {
        if n > 0 && *speed <= SPEEDS[n - 1].1 {
            break;
        }
        if caseless_eq(rate, name.as_bytes()) {
            return *speed;
        }
    }
    err(progname, &format!("unknown baud rate {}", io::lossy(rate)))
}

/// `badmopt:`.
fn bad_mapping(progname: &str, arg: &[u8]) -> ! {
    err(progname, &format!("illegal -m option format: {}", io::lossy(arg)))
}

/// O fim de `add_mapping`: com `-a`/`-d`/`-p` o tipo de porta vem da opção.
fn finish_mapping(progname: &str, arg: &[u8], port: Option<&str>, mut mapp: Map, maps: &mut Vec<Map>) {
    if let Some(port) = port {
        if mapp.porttype.is_some() {
            bad_mapping(progname, arg);
        }
        mapp.porttype = Some(port.as_bytes().to_vec());
    }
    maps.push(mapp);
}

/// `add_mapping`.
fn add_mapping(progname: &str, maps: &mut Vec<Map>, port: Option<&str>, arg: &[u8]) {
    let mut mapp = Map { porttype: Some(arg.to_vec()), ty: Vec::new(), conditional: 0, speed: 0 };
    let Some(first) = arg.iter().position(|b| b"><@=!:".contains(b)) else {
        mapp.ty = arg.to_vec();
        mapp.porttype = None;
        finish_mapping(progname, arg, port, mapp, maps);
        return;
    };
    let base: Option<usize> = if first == 0 {
        mapp.porttype = None;
        None
    } else {
        Some(first)
    };
    let mut i = first;
    loop {
        match arg.get(i).copied().unwrap_or(0) {
            b'<' => {
                if mapp.conditional & GT != 0 {
                    bad_mapping(progname, arg);
                }
                mapp.conditional |= LT;
            }
            b'>' => {
                if mapp.conditional & LT != 0 {
                    bad_mapping(progname, arg);
                }
                mapp.conditional |= GT;
            }
            b'@' | b'=' => mapp.conditional |= EQ,
            b'!' => mapp.conditional |= NOT,
            _ => break,
        }
        i += 1;
    }
    if arg.get(i) == Some(&b':') {
        if mapp.conditional != 0 {
            bad_mapping(progname, arg);
        }
        i += 1;
    } else {
        // baud rate opcional, até o ':'
        let Some(colon) = arg[i.min(arg.len())..].iter().position(|b| *b == b':') else {
            bad_mapping(progname, arg)
        };
        let colon = i + colon;
        mapp.speed = tbaudrate(progname, &arg[i..colon]);
        i = colon + 1;
    }
    mapp.ty = arg[i.min(arg.len())..].to_vec();
    if let Some(b) = base {
        mapp.porttype = Some(arg[..b].to_vec());
    }
    if mapp.conditional & NOT != 0 {
        mapp.conditional = !mapp.conditional & (EQ | GT | LT);
    }
    finish_mapping(progname, arg, port, mapp, maps);
}

/// `mapped(type)`: o tipo do primeiro mapeamento que vale (a velocidade da linha é 0 aqui).
fn mapped(maps: &[Map], ty: &[u8]) -> Vec<u8> {
    let ospeed = 0i32;
    for m in maps {
        if m.porttype.as_deref().is_none_or(|p| p == ty) {
            let matched = match m.conditional {
                0 => true,
                EQ => ospeed == m.speed,
                c if c == (GT | EQ) => ospeed >= m.speed,
                GT => ospeed > m.speed,
                c if c == (LT | EQ) => ospeed <= m.speed,
                LT => ospeed < m.speed,
                _ => false,
            };
            if matched {
                return m.ty.clone();
            }
        }
    }
    ty.to_vec()
}

/// `get_termcap_entry`: o tipo vem do argumento, do `TERM` ou é `unknown`, e é pedido ao usuário
/// quando falta ou não existe.
fn get_termcap_entry(progname: &str, fd: Fd, userarg: Option<&[u8]>, maps: &[Map], asker: &mut Asker) -> (Term, Vec<u8>) {
    let mut ttype: Vec<u8> = match userarg {
        Some(u) => u.to_vec(),
        None => match sys::getenv("TERM") {
            Some(t) => mapped(maps, &t),
            None => mapped(maps, b"unknown"),
        },
    };
    // Um TERMCAP que não é um caminho ficaria desatualizado: sai do ambiente.
    if let Some(tc) = sys::getenv("TERMCAP") {
        if tc.first() != Some(&b'/') {
            if let Some(s) = sys::try_current() {
                let _ = s.unsetenv(b"TERMCAP");
            }
        }
    }
    if ttype.first() == Some(&b'?') {
        ttype = if ttype.len() > 1 { asker.ask(Some(&ttype[1..])) } else { asker.ask(None) };
    }
    loop {
        match setupterm(Some(&ttype), fd, SetupOpts::default()) {
            Ok(t) => return (t, ttype),
            Err(f) => {
                if f.code == 0 {
                    io::eprint(format!("{progname}: unknown terminal type {}\n", io::lossy(&ttype)));
                } else {
                    io::eprint(format!("{progname}: can't initialize terminal type {} (error {})\n", io::lossy(&ttype), f.code));
                }
                ttype = asker.ask(None);
            }
        }
    }
}

/// `obsolete()`: `-` vira `-q`, e `-e`, `-i`, `-k` sem argumento ganham o padrão.
fn obsolete(argv: &mut [Vec<u8>]) {
    for i in 0..argv.len() {
        let parm = argv[i].clone();
        if parm == b"-" {
            argv[i] = b"-q".to_vec();
            continue;
        }
        let next_is_arg = argv.get(i + 1).is_some_and(|n| n.first() != Some(&b'-'));
        if parm.first() != Some(&b'-') || next_is_arg || !matches!(parm.get(1), Some(b'e' | b'i' | b'k')) || parm.len() != 2 {
            continue;
        }
        argv[i] = match parm[1] {
            b'e' => b"-e^H".to_vec(),
            b'i' => b"-i^C".to_vec(),
            _ => b"-k^U".to_vec(),
        };
    }
}

fn print_shell_commands(ttype: &[u8]) {
    let mut out = io::stdout();
    let csh = sys::getenv("SHELL").is_some_and(|v| {
        let leaf = rootname(&v);
        leaf.len() >= 3 && leaf.ends_with(b"csh")
    });
    let t = io::lossy(ttype);
    if csh {
        let _ = write!(out, "set noglob;\nsetenv TERM {t};\nunset noglob;\n");
    } else {
        let _ = write!(out, "TERM={t};\n");
    }
}

fn usage(progname: &str) -> ! {
    let msg = "\nOptions:\n  -c          set control characters\n  -e ch       erase character\n  -I          no initialization strings\n  -i ch       interrupt character\n  -k ch       kill character\n  -m mapping  map identifier to type\n  -Q          do not output control key settings\n  -q          display term only, do no changes\n  -r          display term on stderr\n  -s          output TERM set command\n  -V          print curses-version\n  -w          set window-size\n\nIf neither -c/-w are given, both are assumed.\n";
    io::eprint(format!("Usage: {progname} [options] [terminal]\n{msg}"));
    sys::exit(1)
}

fn arg_char(optarg: &[u8]) -> i32 {
    match optarg {
        [b'^', second, ..] => {
            if *second == b'?' {
                0o177
            } else {
                i32::from(second & 0x1f)
            }
        }
        [first, ..] => i32::from(*first),
        [] => 0,
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let mut argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let progname = io::lossy(rootname(&argv[0]));
    let is_reset = rootname(&argv[0]) == b"reset";
    obsolete(&mut argv);
    let (mut noinit, mut noset, mut quiet, mut s_flag_big, mut s_flag, mut showterm) = (false, false, false, false, false, false);
    let (mut opt_c, mut opt_w) = (false, false);
    let mut maps: Vec<Map> = Vec::new();
    let (mut terasechar, mut intrchar, mut tkillchar) = (-1, -1, -1);
    let _ = (&mut terasechar, &mut intrchar, &mut tkillchar);
    let mut g = Getopt::from_env(&argv[1..], "a:cd:e:Ii:k:m:p:qQrSsVw", &[]);
    while let Some(r) = g.next_opt() {
        match r {
            Ok(o) => {
                let arg = o.arg.clone().unwrap_or_default();
                match o.short().unwrap_or('?') {
                    'c' => opt_c = true,
                    'a' => add_mapping(&progname, &mut maps, Some("arpanet"), &arg),
                    'd' => add_mapping(&progname, &mut maps, Some("dialup"), &arg),
                    'e' => terasechar = arg_char(&arg),
                    'I' => noinit = true,
                    'i' => intrchar = arg_char(&arg),
                    'k' => tkillchar = arg_char(&arg),
                    'm' => add_mapping(&progname, &mut maps, None, &arg),
                    'p' => add_mapping(&progname, &mut maps, Some("plugboard"), &arg),
                    'Q' => quiet = true,
                    'q' => noset = true,
                    'r' => showterm = true,
                    'S' => s_flag_big = true,
                    's' => s_flag = true,
                    'V' => {
                        let mut out = io::stdout();
                        let _ = writeln!(out, "{VERSION}");
                        return 0;
                    }
                    'w' => opt_w = true,
                    _ => usage(&progname),
                }
            }
            Err(e) => bad_option(&e, &argv0, &progname),
        }
    }
    let operands = g.operands();
    if operands.len() > 1 {
        usage(&progname);
    }
    if !opt_c && !opt_w {
        opt_c = true;
        opt_w = true;
    }
    let _ = quiet;
    let fd = save_tty_settings(&progname, true);
    let mut asker = Asker::new();
    let (term, ttype) = get_termcap_entry(&progname, fd, operands.first().map(Vec::as_slice), &maps, &mut asker);
    let mut r = Reset::new(&term, Fd::STDERR, is_reset, !is_reset, &progname);
    if !noset {
        if opt_w && isatty(fd) {
            if let Some(s) = sys::try_current() {
                if let Ok(ws) = s.tcgetwinsize(fd) {
                    if ws.rows > 0 && ws.cols > 0 {
                        r.columns = i32::from(ws.cols);
                    }
                }
            }
        }
        if opt_c && !noinit && r.send_init_strings() {
            r.out.write_all(b"\r");
            r.flush();
            if isatty(fd) {
                if let Some(s) = sys::try_current() {
                    let _ = s.nanosleep(Duration::from_millis(1000));
                }
            }
        }
    }
    if noset {
        let mut out = io::stdout();
        let _ = writeln!(out, "{}", io::lossy(&ttype));
    } else if showterm {
        r.flush();
        io::eprint(format!("Terminal type is {}.\n", io::lossy(&ttype)));
    }
    r.flush();
    if s_flag_big {
        err(&progname, "The -S option is not supported under terminfo.");
    }
    if s_flag {
        print_shell_commands(&ttype);
    }
    0
}

fn bad_option(e: &GetoptError, argv0: &str, progname: &str) -> ! {
    io::eprint(format!("{}\n", e.message(argv0)));
    usage(progname)
}
