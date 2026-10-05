//! `killall` do psmisc 23.7.
//!
//! Porte de `killall.c`: casamento de nome por `comm` (com o desvio para o `cmdline` quando o nome
//! enche os 15 caracteres do kernel), `-e`, `-I`, `-g`, `-y`, `-o`, `-i`, `-l`, `-q`, `-r`, `-s`, `-u`,
//! `-v`, `-V`, `-w`, `-n` e `-Z`. A leitura de opções é o `getopt_long_only` da glibc, reproduzido
//! passo a passo (permutação dos operandos, prefixos de opções longas, ambiguidade silenciosa com
//! `opterr = 0`), porque o psmisc depende dele: `-9`, `-TERM` e `-INT` chegam como opção inválida e
//! viram sinal, e `-ve` cai no atalho de `-v -e`.
//!
//! Diferenças: o sinal sai por `kill` (o original usa `pidfd_send_signal`); `-version` sem operando
//! derruba o programa original com SIGSEGV, aqui sai com 139 sem mensagem.

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use sysabi::{Ctx, Errno, Fd, FileType, KillTarget, Signal, sys};
use ul_misc::util::io;

use crate::common::{self, out};
use crate::matcher::{self, Matcher};
use crate::procfs;

const USAGE: &str = "Usage: killall [OPTION]... [--] NAME...\n       killall -l, --list\n       killall -V, --version\n\n  -e,--exact          require exact match for very long names\n  -I,--ignore-case    case insensitive process name match\n  -g,--process-group  kill process group instead of process\n  -y,--younger-than   kill processes younger than TIME\n  -o,--older-than     kill processes older than TIME\n  -i,--interactive    ask for confirmation before killing\n  -l,--list           list all known signal names\n  -q,--quiet          don't print complaints\n  -r,--regexp         interpret NAME as an extended regular expression\n  -s,--signal SIGNAL  send this signal instead of SIGTERM\n  -u,--user USER      kill only process(es) running as USER\n  -v,--verbose        report if the signal was successfully sent\n  -V,--version        display version information\n  -w,--wait           wait for processes to die\n  -n,--ns PID         match processes that belong to the same namespaces\n                      as PID\n  -Z,--context REGEXP kill only process(es) having context\n                      (must precede other arguments)\n\n";

const VERSION: &str = "killall (PSmisc) 23.7\nCopyright (C) 1993-2024 Werner Almesberger and Craig Small\n\nPSmisc comes with ABSOLUTELY NO WARRANTY.\nThis is free software, and you are welcome to redistribute it under\nthe terms of the GNU General Public License.\nFor more information about these matters, see the files named COPYING.\n";

/// Nomes de sinal da tabela do psmisc (a mesma do `killall -l`).
const SIGNAMES: [(&str, i32); 31] = [
    ("HUP", 1),
    ("INT", 2),
    ("QUIT", 3),
    ("ILL", 4),
    ("TRAP", 5),
    ("ABRT", 6),
    ("BUS", 7),
    ("FPE", 8),
    ("KILL", 9),
    ("USR1", 10),
    ("SEGV", 11),
    ("USR2", 12),
    ("PIPE", 13),
    ("ALRM", 14),
    ("TERM", 15),
    ("STKFLT", 16),
    ("CHLD", 17),
    ("CONT", 18),
    ("STOP", 19),
    ("TSTP", 20),
    ("TTIN", 21),
    ("TTOU", 22),
    ("URG", 23),
    ("XCPU", 24),
    ("XFSZ", 25),
    ("VTALRM", 26),
    ("PROF", 27),
    ("WINCH", 28),
    ("POLL", 29),
    ("PWR", 30),
    ("SYS", 31),
];

const COMM_LEN: usize = 64;
const OLD_COMM_LEN: usize = 16;
const MAX_NAMES: usize = 64;

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| match run(args) {
        Ok(code) | Err(code) => code,
    })
}

/// `usage(msg)`: mensagem opcional e o texto de uso no stderr, saída 1.
fn usage(msg: Option<&str>) -> i32 {
    let mut s = String::new();
    if let Some(m) = msg {
        s.push_str(m);
        s.push('\n');
    }
    s.push_str(USAGE);
    io::eprint(s);
    1
}

fn list_signals() {
    let mut s = String::new();
    let mut col = 0usize;
    for (name, _) in SIGNAMES.iter() {
        if col + name.len() + 1 > 80 {
            s.push('\n');
            col = 0;
        }
        if col != 0 {
            s.push(' ');
        }
        s.push_str(name);
        col += name.len() + 1;
    }
    s.push('\n');
    out(s);
}

/// `atoi`: dígitos iniciais (com espaço e sinal), saturando.
fn atoi(s: &[u8]) -> i32 {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = (v * 10 + i64::from(s[i] - b'0')).min(i64::from(i32::MAX) + 1);
        i += 1;
    }
    let v = if neg { -v } else { v };
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// `get_signal`: número ou nome (com ou sem `SIG`); nome desconhecido sai com 1.
fn get_signal(name: &[u8]) -> Result<i32, i32> {
    if name.first().is_some_and(u8::is_ascii_digit) {
        return Ok(atoi(name));
    }
    let bare = name.strip_prefix(b"SIG").unwrap_or(name);
    if let Some((_, n)) = SIGNAMES.iter().find(|(s, _)| s.as_bytes() == bare) {
        return Ok(*n);
    }
    let mut m = bare.to_vec();
    m.extend_from_slice(b": unknown signal; killall -l lists signals.\n");
    io::eprint(m);
    Err(1)
}

/// `parse_time_units`: número seguido de `s m h d w M y`; negativo é erro.
fn parse_time_units(age: &[u8]) -> i64 {
    let mut i = 0;
    while i < age.len() && matches!(age[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let start_num = i;
    if i < age.len() && (age[i] == b'-' || age[i] == b'+') {
        i += 1;
    }
    let digits_start = i;
    while i < age.len() && age[i].is_ascii_digit() {
        i += 1;
    }
    if i == digits_start {
        return -1;
    }
    let num: i64 = String::from_utf8_lossy(&age[start_num..i]).parse().unwrap_or(i64::MAX);
    if i >= age.len() {
        return -1;
    }
    match age[i] {
        b's' => num,
        b'm' => num.wrapping_mul(60),
        b'h' => num.wrapping_mul(60 * 60),
        b'd' => num.wrapping_mul(60 * 60 * 24),
        b'w' => num.wrapping_mul(60 * 60 * 24 * 7),
        b'M' => num.wrapping_mul(60 * 60 * 24 * 7 * 4),
        b'y' => num.wrapping_mul(60 * 60 * 24 * 7 * 4 * 12),
        _ => -1,
    }
}

// ---------------------------------------------------------------------------------------------
// getopt_long_only da glibc.

/// Opções longas do killall: nome, se leva argumento, código.
const LONGS: [(&str, bool, u8); 16] = [
    ("exact", false, b'e'),
    ("ignore-case", false, b'I'),
    ("process-group", false, b'g'),
    ("younger-than", true, b'y'),
    ("older-than", true, b'o'),
    ("interactive", false, b'i'),
    ("list-signals", false, b'l'),
    ("quiet", false, b'q'),
    ("regexp", false, b'r'),
    ("signal", true, b's'),
    ("user", true, b'u'),
    ("verbose", false, b'v'),
    ("wait", false, b'w'),
    ("ns", true, b'n'),
    ("context", true, b'Z'),
    ("version", false, b'V'),
];

const OPTSTRING: &[u8] = b"egy:o:ilqrs:u:vwZ:VIn:";

struct Getopt {
    argv: Vec<Vec<u8>>,
    optind: usize,
    optarg: Option<Vec<u8>>,
    nextchar: Vec<u8>,
    first_nonopt: usize,
    last_nonopt: usize,
}

impl Getopt {
    fn new(argv: Vec<Vec<u8>>) -> Getopt {
        Getopt { argv, optind: 1, optarg: None, nextchar: Vec::new(), first_nonopt: 1, last_nonopt: 1 }
    }

    fn nonoption(&self, i: usize) -> bool {
        let a = &self.argv[i];
        a.first() != Some(&b'-') || a.len() == 1
    }

    /// `exchange`: troca o bloco de não-opções `[first, last)` com as opções `[last, optind)`.
    fn exchange(&mut self) {
        let bottom = self.first_nonopt;
        let middle = self.last_nonopt;
        let top = self.optind;
        let block: Vec<Vec<u8>> = self.argv[bottom..top].to_vec();
        let nonopts = &block[..middle - bottom];
        let opts = &block[middle - bottom..];
        let mut merged: Vec<Vec<u8>> = Vec::new();
        merged.extend_from_slice(opts);
        merged.extend_from_slice(nonopts);
        for (k, v) in merged.into_iter().enumerate() {
            self.argv[bottom + k] = v;
        }
        self.first_nonopt += self.optind - self.last_nonopt;
        self.last_nonopt = self.optind;
    }

    fn strchr_opt(c: u8) -> Option<usize> {
        OPTSTRING.iter().position(|b| *b == c)
    }

    /// `process_long_option` com `opterr = 0`. `Some(c)` é o código devolvido; `None` é o `-1`
    /// ("tente como opção curta").
    fn process_long_option(&mut self, prefix_is_dash: bool) -> Option<u8> {
        let next = self.nextchar.clone();
        let nameend = next.iter().position(|b| *b == b'=').unwrap_or(next.len());
        let name = &next[..nameend];
        let mut found: Option<usize> = LONGS.iter().position(|l| l.0.as_bytes() == name);
        if found.is_none() {
            let mut ambig = false;
            let mut first: Option<usize> = None;
            for (i, l) in LONGS.iter().enumerate() {
                if l.0.as_bytes().starts_with(name) {
                    if first.is_none() {
                        first = Some(i);
                    } else {
                        // long_only: qualquer segundo casamento por prefixo é ambíguo.
                        ambig = true;
                    }
                }
            }
            if ambig {
                self.nextchar.clear();
                self.optind += 1;
                return Some(b'?');
            }
            found = first;
        }
        let Some(idx) = found else {
            if !prefix_is_dash || Self::strchr_opt(next.first().copied().unwrap_or(0)).is_none() {
                self.nextchar.clear();
                self.optind += 1;
                return Some(b'?');
            }
            return None;
        };
        let (_, has_arg, val) = LONGS[idx];
        self.optind += 1;
        self.nextchar.clear();
        if nameend < next.len() {
            if has_arg {
                self.optarg = Some(next[nameend + 1..].to_vec());
            } else {
                return Some(b'?');
            }
        } else if has_arg {
            if self.optind < self.argv.len() {
                self.optarg = Some(self.argv[self.optind].clone());
                self.optind += 1;
            } else {
                return Some(b'?');
            }
        }
        Some(val)
    }

    /// Próxima opção: `Some(código)` ou `None` no fim.
    fn next(&mut self) -> Option<u8> {
        self.optarg = None;
        let argc = self.argv.len();
        if self.nextchar.is_empty() {
            if self.last_nonopt > self.optind {
                self.last_nonopt = self.optind;
            }
            if self.first_nonopt > self.optind {
                self.first_nonopt = self.optind;
            }
            if self.first_nonopt != self.last_nonopt && self.last_nonopt != self.optind {
                self.exchange();
            } else if self.last_nonopt != self.optind {
                self.first_nonopt = self.optind;
            }
            while self.optind < argc && self.nonoption(self.optind) {
                self.optind += 1;
            }
            self.last_nonopt = self.optind;
            if self.optind != argc && self.argv[self.optind] == b"--" {
                self.optind += 1;
                if self.first_nonopt != self.last_nonopt && self.last_nonopt != self.optind {
                    self.exchange();
                } else if self.first_nonopt == self.last_nonopt {
                    self.first_nonopt = self.optind;
                }
                self.last_nonopt = argc;
                self.optind = argc;
            }
            if self.optind == argc {
                if self.first_nonopt != self.last_nonopt {
                    self.optind = self.first_nonopt;
                }
                return None;
            }
            let cur = self.argv[self.optind].clone();
            if cur.get(1) == Some(&b'-') {
                self.nextchar = cur[2..].to_vec();
                // O `--` sozinho já foi tratado; aqui o nome nunca é vazio, mas `--=x` é possível.
                return self.process_long_option(false).or(Some(b'?'));
            }
            if cur.get(2).is_some() || cur.get(1).is_none_or(|c| Self::strchr_opt(*c).is_none()) {
                self.nextchar = cur[1..].to_vec();
                if let Some(code) = self.process_long_option(true) {
                    return Some(code);
                }
            }
            self.nextchar = cur[1..].to_vec();
        }
        let c = self.nextchar.remove(0);
        let temp = Self::strchr_opt(c);
        if self.nextchar.is_empty() {
            self.optind += 1;
        }
        let Some(t) = temp else { return Some(b'?') };
        if c == b':' || c == b';' {
            return Some(b'?');
        }
        if OPTSTRING.get(t + 1) == Some(&b':') {
            if OPTSTRING.get(t + 2) == Some(&b':') {
                if !self.nextchar.is_empty() {
                    self.optarg = Some(std::mem::take(&mut self.nextchar));
                    self.optind += 1;
                }
                self.nextchar.clear();
            } else {
                if !self.nextchar.is_empty() {
                    self.optarg = Some(std::mem::take(&mut self.nextchar));
                    self.optind += 1;
                } else if self.optind == argc {
                    self.nextchar.clear();
                    return Some(b'?');
                } else {
                    self.optarg = Some(self.argv[self.optind].clone());
                    self.optind += 1;
                }
                self.nextchar.clear();
            }
        }
        Some(c)
    }
}

// ---------------------------------------------------------------------------------------------
// Estado e execução.

#[derive(Default)]
struct Opts {
    verbose: bool,
    exact: bool,
    interactive: bool,
    reg: bool,
    quiet: bool,
    wait_until_dead: bool,
    process_group: bool,
    ignore_case: bool,
    younger_than: i64,
    older_than: i64,
    opt_ns_pid: i32,
}

fn run(args: &[OsString]) -> Result<i32, i32> {
    let mut argv = io::args_bytes(args);
    if argv.len() < 2 {
        return Ok(usage(None));
    }
    let _ = argv.first();
    let mut o = Opts::default();
    let mut sig_num = 15i32;
    let mut pwent_uid: Option<u32> = None;
    let mut scontext: Option<Matcher> = None;
    let mut have_scontext = false;
    let mut skip_error = 0usize;
    let mut names = common::Names::new();
    let mut g = Getopt::new(argv.clone());
    while let Some(c) = g.next() {
        let optarg = g.optarg.clone().unwrap_or_default();
        let prev = g.argv.get(g.optind.wrapping_sub(1)).cloned().unwrap_or_default();
        match c {
            b'e' => o.exact = true,
            b'g' => o.process_group = true,
            b'y' => {
                let mut yt = optarg.clone();
                yt.truncate(COMM_LEN - 1);
                o.younger_than = parse_time_units(&yt);
                if o.younger_than <= 0 {
                    return Ok(usage(Some("Invalid time format")));
                }
            }
            b'o' => {
                let mut ot = optarg.clone();
                ot.truncate(COMM_LEN - 1);
                o.older_than = parse_time_units(&ot);
                if o.older_than <= 0 {
                    return Ok(usage(Some("Invalid time format")));
                }
            }
            b'i' => o.interactive = true,
            b'l' => {
                list_signals();
                return Ok(0);
            }
            b'q' => o.quiet = true,
            b'r' => o.reg = true,
            b's' => sig_num = get_signal(&optarg)?,
            b'u' => {
                let n = String::from_utf8_lossy(&optarg).into_owned();
                match names.uid_of(&n) {
                    Some(u) => pwent_uid = Some(u),
                    None => {
                        io::eprint(format!("Cannot find user {n}\n"));
                        return Err(1);
                    }
                }
            }
            b'v' => o.verbose = true,
            b'w' => o.wait_until_dead = true,
            b'I' => {
                if prev == b"-I" || prev.starts_with(b"--") {
                    o.ignore_case = true;
                } else {
                    let next = g.argv.get(g.optind).cloned();
                    let Some(n) = next else { return Ok(139) };
                    sig_num = get_signal(&n[1.min(n.len())..])?;
                    skip_error = g.optind;
                }
            }
            b'V' => {
                if prev == b"-V" || prev.starts_with(b"--") {
                    io::eprint(VERSION);
                    return Ok(0);
                }
                let next = g.argv.get(g.optind).cloned();
                let Some(n) = next else { return Ok(139) };
                sig_num = get_signal(&n[1.min(n.len())..])?;
                skip_error = g.optind;
            }
            b'n' => {
                let (num, ok) = strtol_checked(&optarg);
                if !ok {
                    return Ok(usage(Some("Invalid namespace PID")));
                }
                o.opt_ns_pid = num as i32;
            }
            b'Z' => {
                have_scontext = true;
                match matcher::compile(&optarg, false) {
                    Ok(m) => scontext = Some(m),
                    Err(_) => {
                        io::eprint(format!("Bad regular expression: {}\n", String::from_utf8_lossy(&optarg)));
                        return Err(1);
                    }
                }
            }
            _ => {
                // case '?'
                if skip_error == g.optind {
                    continue;
                }
                if prev.starts_with(b"-ve") {
                    o.verbose = true;
                    o.exact = true;
                    continue;
                }
                let c1 = prev.get(1).copied().unwrap_or(0);
                if c1.is_ascii_uppercase() {
                    sig_num = get_signal(&prev[1..])?;
                } else if c1.is_ascii_digit() {
                    sig_num = atoi(&prev[1..]);
                } else {
                    return Ok(usage(None));
                }
            }
        }
    }
    let myoptind = g.optind;
    argv = g.argv;
    let operands: Vec<Vec<u8>> = argv[myoptind.min(argv.len())..].to_vec();
    if operands.is_empty() && pwent_uid.is_none() && !have_scontext {
        return Ok(usage(None));
    }
    if operands.len() > MAX_NAMES {
        io::eprint(format!("killall: Maximum number of names is {MAX_NAMES}\n"));
        return Err(1);
    }
    let me = sys::current().getpid();
    if sys::stat(format!("/proc/{me}/stat").as_bytes()).is_err() {
        io::eprint("killall: /proc lacks process entries (not mounted ?)\n");
        return Err(1);
    }
    kill_all(&o, sig_num, &operands, pwent_uid, scontext.as_ref())
}

/// `strtol(arg, &end, 10)` com a checagem do `-n`: erro se não leu dígito algum.
fn strtol_checked(s: &[u8]) -> (i64, bool) {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut j = i;
    if j < s.len() && (s[j] == b'-' || s[j] == b'+') {
        j += 1;
    }
    let d = j;
    while j < s.len() && s[j].is_ascii_digit() {
        j += 1;
    }
    if j == d {
        return (0, false);
    }
    match String::from_utf8_lossy(&s[i..j]).parse::<i64>() {
        Ok(v) => (v, true),
        Err(_) => (0, false),
    }
}

/// Um nome da linha de comando: com `/` (compara o executável) ou simples.
struct NameInfo {
    name: Vec<u8>,
    name_length: usize,
    /// `Some((dev, ino))` quando o nome tem `/` e o `stat` funcionou.
    st: Option<(u64, u64)>,
}

fn build_nameinfo(names: &[Vec<u8>]) -> Result<Vec<NameInfo>, i32> {
    let mut v = Vec::new();
    for n in names {
        if !n.contains(&b'/') {
            v.push(NameInfo { name: n.clone(), name_length: n.len(), st: None });
        } else {
            match sys::stat(n) {
                Ok(st) => v.push(NameInfo { name: n.clone(), name_length: 0, st: Some((st.dev, st.ino)) }),
                Err(e) => {
                    let mut m = n.clone();
                    m.extend_from_slice(format!(": {}\n", e.message()).as_bytes());
                    io::eprint(m);
                    return Err(1);
                }
            }
        }
    }
    Ok(v)
}

/// `uptime()` do killall: primeiro campo de /proc/uptime.
fn uptime() -> Result<f64, i32> {
    match procfs::read("/proc/uptime") {
        Some(d) => {
            let t = String::from_utf8_lossy(&d).into_owned();
            Ok(t.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0))
        }
        None => {
            io::eprint("killall: error opening uptime file\n");
            Err(1)
        }
    }
}

fn process_age(jf: u64) -> Result<f64, i32> {
    let age = uptime()? - jf as f64 / 100.0;
    Ok(if age < 0.0 { 0.0 } else { age })
}

/// `load_process_name_and_age`: `comm` (até 63 bytes) e, se pedido, a idade em segundos.
fn load_process_name_and_age(pid: i32, load_age: bool) -> Result<Option<(Vec<u8>, f64)>, i32> {
    let Some(data) = procfs::read(&format!("/proc/{pid}/stat")) else { return Ok(None) };
    let line_end = data.iter().position(|b| *b == b'\n').map_or(data.len(), |p| p + 1).min(1023);
    let buf = &data[..line_end.min(data.len())];
    if buf.is_empty() {
        return Ok(None);
    }
    let Some(open) = buf.iter().position(|b| *b == b'(') else { return Ok(None) };
    let rest = &buf[open + 1..];
    let Some(close) = rest.iter().rposition(|b| *b == b')') else { return Ok(None) };
    let lencomm = close.min(COMM_LEN - 1);
    let comm = rest[..lencomm].to_vec();
    let mut age = 0.0;
    if load_age {
        let tail = if close + 2 <= rest.len() { &rest[close + 2..] } else { &rest[rest.len()..] };
        let text = String::from_utf8_lossy(tail).into_owned();
        // Pula 19 campos e lê o início do processo.
        let jf = text.split_whitespace().nth(19).and_then(|v| v.parse::<u64>().ok());
        match jf {
            Some(j) => age = process_age(j)?,
            None => return Ok(None),
        }
    }
    Ok(Some((comm, age)))
}

/// `load_proc_cmdline`: procura entre os argumentos o primeiro cujo nome-base começa com `comm`.
/// `Ok(Some(cmd))` achou, `Ok(None)` não achou (okay = 0), `Err(())` não deu para abrir.
fn load_proc_cmdline(pid: i32, comm: &[u8], check_len: usize) -> Result<Option<Vec<u8>>, ()> {
    let data = procfs::read(&format!("/proc/{pid}/cmdline")).ok_or(())?;
    let mut pos = 0usize;
    loop {
        let end = data[pos.min(data.len())..].iter().position(|b| *b == 0).map(|p| pos + p);
        let arg: &[u8] = match end {
            Some(e) => &data[pos..e],
            None => &data[pos.min(data.len())..],
        };
        if arg.is_empty() {
            return Ok(None);
        }
        let base = match arg.iter().rposition(|b| *b == b'/') {
            Some(i) => &arg[i + 1..],
            None => arg,
        };
        // strncmp(p, comm, check_comm_length): para no primeiro NUL de qualquer lado.
        let a: Vec<u8> = base.iter().copied().take(check_len).collect();
        let b: Vec<u8> = comm.iter().copied().take(check_len).collect();
        if a == b && base.len() >= check_len.min(comm.len()) {
            return Ok(Some(base.to_vec()));
        }
        match end {
            Some(e) => pos = e + 1,
            None => return Ok(None),
        }
    }
}

fn match_process_uid(pid: i32, uid: u32) -> Result<bool, i32> {
    let Some(data) = procfs::read(&format!("/proc/{pid}/status")) else { return Ok(false) };
    for line in data.split(|b| *b == b'\n') {
        if let Some(rest) = line.strip_prefix(b"Uid:\t") {
            let digits: Vec<u8> = rest.iter().copied().take_while(|b| b.is_ascii_digit() || *b == b'-').collect();
            let puid: i64 = String::from_utf8_lossy(&digits).parse().unwrap_or(0);
            return Ok(puid as u32 == uid);
        }
    }
    io::eprint("killall: Cannot get UID from process status\n");
    Err(1)
}

/// `match_process_context`: `true` casa (também quando o arquivo não existe, como o original).
fn match_process_context(pid: i32, ctx: &Matcher) -> bool {
    if let Some(d) = procfs::read(&format!("/proc/{pid}/attr/current")) {
        let line_end = d.iter().position(|b| *b == b'\n').map_or(d.len(), |p| p + 1);
        if line_end > 0 {
            return ctx.is_match(&d[..line_end]);
        }
    }
    true
}

fn get_ns_pid(pid: i32) -> i64 {
    match sys::stat(format!("/proc/{pid}/ns/pid").as_bytes()) {
        Ok(st) => i64::from(st.ino as i32),
        Err(_) => 0,
    }
}

fn eq_cmp(a: &[u8], b: &[u8], icase: bool) -> bool {
    if icase { a.eq_ignore_ascii_case(b) } else { a == b }
}

fn ncmp(a: &[u8], b: &[u8], n: usize, icase: bool) -> bool {
    let ta: Vec<u8> = a.iter().copied().take(n).collect();
    let tb: Vec<u8> = b.iter().copied().take(n).collect();
    eq_cmp(&ta, &tb, icase)
}

fn match_process_name(o: &Opts, comm: &[u8], comm_len: usize, cmdline: &[u8], name: &[u8], match_len: usize, got_long: bool) -> bool {
    if comm_len == OLD_COMM_LEN - 1 && match_len >= OLD_COMM_LEN - 1 {
        return if got_long { eq_cmp(name, cmdline, o.ignore_case) } else { ncmp(name, comm, OLD_COMM_LEN - 1, o.ignore_case) };
    }
    if comm_len == COMM_LEN - 1 && match_len >= COMM_LEN - 1 {
        return if got_long { eq_cmp(name, cmdline, o.ignore_case) } else { ncmp(name, comm, COMM_LEN - 1, o.ignore_case) };
    }
    if got_long {
        return eq_cmp(name, cmdline, o.ignore_case);
    }
    eq_cmp(name, comm, o.ignore_case)
}

/// Leitor de linhas do stdin com o buffer do `getline`.
struct Stdin {
    buf: Vec<u8>,
    eof: bool,
}

impl Stdin {
    fn line(&mut self) -> Option<Vec<u8>> {
        loop {
            if let Some(p) = self.buf.iter().position(|b| *b == b'\n') {
                let rest = self.buf.split_off(p + 1);
                return Some(std::mem::replace(&mut self.buf, rest));
            }
            if self.eof {
                if self.buf.is_empty() {
                    return None;
                }
                return Some(std::mem::take(&mut self.buf));
            }
            let mut chunk = [0u8; 4096];
            match sys::read(Fd::STDIN, &mut chunk) {
                Ok(0) | Err(_) => self.eof = true,
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
            }
        }
    }
}

/// `ask`: pergunta `Kill nome(pid) ? (y/N) `; só `y`/`Y` confirma.
fn ask(o: &Opts, stdin: &mut Stdin, name: &[u8], pid: i32, signal: i32) -> bool {
    loop {
        let verb = if signal == 15 { "Kill" } else { "Signal" };
        let mut s = format!("{verb} ").into_bytes();
        s.extend_from_slice(name);
        s.extend_from_slice(format!("({}{}) ? (y/N) ", if o.process_group { "pgid " } else { "" }, pid).as_bytes());
        out(&s);
        let _ = io::stdout().flush();
        let Some(line) = stdin.line() else { return false };
        if line.first() == Some(&b'\n') {
            return false;
        }
        match line.first() {
            Some(b'y') | Some(b'Y') => return true,
            Some(b'n') | Some(b'N') => return false,
            _ => {}
        }
    }
}

fn kill_all(o: &Opts, signal: i32, names: &[Vec<u8>], pwent_uid: Option<u32>, scontext: Option<&Matcher>) -> Result<i32, i32> {
    let name_count = names.len();
    let ns_ino = if o.opt_ns_pid != 0 { get_ns_pid(o.opt_ns_pid) } else { 0 };
    let mut reglist: Vec<Matcher> = Vec::new();
    let mut name_info: Vec<NameInfo> = Vec::new();
    if name_count > 0 && o.reg {
        for n in names {
            match matcher::compile(n, o.ignore_case) {
                Ok(m) => reglist.push(m),
                Err(_) => {
                    io::eprint(format!("killall: Bad regular expression: {}\n", String::from_utf8_lossy(n)));
                    return Err(1);
                }
            }
        }
    } else {
        name_info = build_nameinfo(names)?;
    }
    // create_pid_table
    let me = sys::current().getpid();
    let entries = match sys::read_dir(b"/proc") {
        Ok(e) => e,
        Err(e) => {
            io::eprint(format!("/proc: {}\n", e.message()));
            return Err(1);
        }
    };
    let mut pid_table: Vec<i32> = Vec::new();
    for e in &entries {
        let pid = atoi(&e.name);
        if pid == 0 || pid == me {
            continue;
        }
        pid_table.push(pid);
    }
    let sysc = sys::current();
    let mut found: u64 = 0;
    let mut pid_killed: Vec<i32> = Vec::new();
    let mut pgids: Vec<i32> = vec![0; pid_table.len()];
    let mut stdin = Stdin { buf: Vec::new(), eof: false };
    for (i, &pid) in pid_table.iter().enumerate() {
        if i % 64 == 0 {
            sys::checkpoint();
        }
        let mut found_name: i32 = -1;
        match sys::stat(format!("/proc/{pid}").as_bytes()) {
            Ok(st) if st.file_type() == FileType::Directory => {}
            _ => continue,
        }
        if let Some(uid) = pwent_uid
            && !match_process_uid(pid, uid)? {
                continue;
            }
        if o.opt_ns_pid != 0 && ns_ino != 0 && ns_ino != get_ns_pid(pid) {
            continue;
        }
        if let Some(ctx) = scontext
            && !match_process_context(pid, ctx) {
                continue;
            }
        let load_age = o.younger_than != 0 || o.older_than != 0;
        let Some((comm, age)) = load_process_name_and_age(pid, load_age)? else { continue };
        let length = comm.len();
        if o.younger_than != 0 && age > o.younger_than as f64 {
            continue;
        }
        if o.older_than != 0 && age < o.older_than as f64 {
            continue;
        }
        let mut command: Option<Vec<u8>> = None;
        let mut got_long = false;
        if length == COMM_LEN - 1 || length == OLD_COMM_LEN - 1 {
            match load_proc_cmdline(pid, &comm, length) {
                Err(()) => continue,
                Ok(found_cmd) => {
                    got_long = found_cmd.is_some();
                    if o.exact && !got_long {
                        if o.verbose {
                            let mut m = b"killall: skipping partial match ".to_vec();
                            m.extend_from_slice(&comm);
                            m.extend_from_slice(format!("({pid})\n").as_bytes());
                            io::eprint(m);
                        }
                        continue;
                    }
                    command = found_cmd;
                }
            }
        }
        let cmdline: &[u8] = command.as_deref().unwrap_or(&[]);
        for j in 0..name_count {
            if o.reg {
                let target: &[u8] = if got_long { cmdline } else { &comm };
                if !reglist[j].is_match(target) {
                    continue;
                }
            } else {
                let ni = &name_info[j];
                match ni.st {
                    None => {
                        if !match_process_name(o, &comm, length, cmdline, &ni.name, ni.name_length, got_long) {
                            continue;
                        }
                    }
                    Some((dev, ino)) => {
                        let mut ok = true;
                        match sys::stat(format!("/proc/{pid}/exe").as_bytes()) {
                            Err(_) => ok = false,
                            Ok(st) => {
                                if dev != st.dev || ino != st.ino {
                                    let len = ni.name.len();
                                    match sysc.readlinkat(Fd::CWD, format!("/proc/{pid}/exe").as_bytes()) {
                                        Ok(t) if t.len() == len && t == ni.name => {}
                                        _ => ok = false,
                                    }
                                }
                            }
                        }
                        if !ok {
                            continue;
                        }
                    }
                }
            }
            found_name = j as i32;
            break;
        }
        if name_count > 0 && found_name == -1 {
            continue;
        }
        let id: i32;
        if !o.process_group {
            id = pid;
        } else {
            id = match sysc.getpgid(pid) {
                Ok(g) => g,
                Err(e) => {
                    io::eprint(format!("killall: getpgid({pid}): {}\n", e.message()));
                    -1
                }
            };
            pgids[i] = id;
            if pgids[..i].contains(&id) {
                continue;
            }
        }
        if o.interactive && !ask(o, &mut stdin, &comm, id, signal) {
            continue;
        }
        // `my_send_signal(pidfd, -id, ...)`: com `-g` e um pgid válido vai para o grupo; sem `-g`
        // (ou com o getpgid que falhou, que dá -(-1) = 1 > 0) o sinal vai ao processo examinado.
        let target = if o.process_group && id > 0 { KillTarget::Group(id) } else { KillTarget::Pid(pid) };
        let shown: &[u8] = if got_long { cmdline } else { &comm };
        match sysc.kill(target, Signal(signal)) {
            Ok(()) => {
                if o.verbose {
                    let mut m = b"Killed ".to_vec();
                    m.extend_from_slice(shown);
                    m.extend_from_slice(format!("({}{id}) with signal {signal}\n", if o.process_group { "pgid " } else { "" }).as_bytes());
                    io::eprint(m);
                }
                if found_name >= 0 {
                    found |= 1u64 << found_name;
                }
                pid_killed.push(id);
            }
            Err(e) => {
                if e != Errno::ESRCH || o.interactive {
                    let mut m = shown.to_vec();
                    m.extend_from_slice(format!("({id}): {}\n", e.message()).as_bytes());
                    io::eprint(m);
                }
            }
        }
    }
    if !o.quiet {
        for (i, n) in names.iter().enumerate() {
            if found & (1u64 << i) == 0 {
                let mut m = n.clone();
                m.extend_from_slice(b": no process found\n");
                io::eprint(m);
            }
        }
    }
    let error = if name_count > 0 {
        let all = if name_count >= 64 { u64::MAX } else { (1u64 << name_count) - 1 };
        i32::from(found != all)
    } else {
        i32::from(pid_killed.is_empty())
    };
    while !pid_killed.is_empty() && o.wait_until_dead {
        let mut i = 0;
        while i < pid_killed.len() {
            let target = if o.process_group { KillTarget::Group(pid_killed[i]) } else { KillTarget::Pid(pid_killed[i]) };
            if sysc.kill(target, Signal(0)) == Err(Errno::ESRCH) {
                pid_killed.swap_remove(i);
                continue;
            }
            i += 1;
        }
        if sysc.nanosleep(Duration::from_secs(1)).is_err() {
            sys::checkpoint();
        }
    }
    Ok(error)
}
