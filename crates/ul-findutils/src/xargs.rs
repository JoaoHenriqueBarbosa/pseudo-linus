//! `xargs` (GNU findutils 4.10): monta linhas de comando com os itens lidos da entrada e as executa.
//!
//! Escrito a partir do manual do findutils e do comportamento medido no oráculo (Debian 13), sem
//! abrir o código do GNU. Pontos que moldam o código:
//!
//! - Leitura padrão: brancos e newlines separam itens; aspas simples e duplas e a barra invertida
//!   protegem; aspa sem par é erro. Uma linha que termina em branco continua na seguinte (conta pro
//!   `-L`). `-0` e `-d` desligam aspas, barra e a string de fim.
//! - Tamanho da linha de comando: cada argumento (o comando inclusive) custa o tamanho mais um. O
//!   padrão do `-s` é 131072, limitado ao que sobra do ARG_MAX do Linux depois do ambiente.
//! - O comando lê de /dev/null (a entrada é do xargs), a menos que os itens venham de `-a`.
//! - Saída: 123 se alguma execução saiu com 1 a 125; 124 se uma saiu com 255 (aborta); 125 se uma
//!   morreu por sinal; 126 se o comando não pôde ser executado; 127 se não existe.

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use sysabi::{Ctx, Errno, Fd, OFlags, WaitStatus, sys};
use sysio::process::{Child, Command, Stdio};

use crate::getopt::{Getopt, HasArg, LongOpt, long};

const USAGE: &str = include_str!("xargs_usage.txt");

const VERSION: &str = "xargs (GNU findutils) 4.10.0
Copyright (C) 2024 Free Software Foundation, Inc.
License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>.
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.

Written by Eric B. Decker, James Youngman, and Kevin Dalley.
";

/// `ARG_MAX` do Linux com a pilha padrão de 8 MiB (um quarto dela).
const ARG_MAX: usize = 2 * 1024 * 1024;
/// O que o POSIX manda reservar além dos argumentos.
const ARG_HEADROOM: usize = 2048;
/// Tamanho padrão do buffer de comando do GNU.
const DEFAULT_MAX_CHARS: usize = 128 * 1024;
/// Menor limite que o POSIX permite.
const POSIX_MIN_ARG_MAX: usize = 4096;

const PROCESS_SLOT_VAR: i32 = 256;
const SHOW_LIMITS: i32 = 257;
const HELP: i32 = 258;
const VERSION_OPT: i32 = 259;

const fn c(ch: u8) -> i32 {
    ch as i32
}

const LONG_OPTIONS: &[LongOpt] = &[
    long("null", HasArg::No, c(b'0')),
    long("arg-file", HasArg::Required, c(b'a')),
    long("delimiter", HasArg::Required, c(b'd')),
    long("eof", HasArg::Optional, c(b'e')),
    long("replace", HasArg::Optional, c(b'i')),
    long("max-lines", HasArg::Optional, c(b'l')),
    long("max-args", HasArg::Required, c(b'n')),
    long("open-tty", HasArg::No, c(b'o')),
    long("interactive", HasArg::No, c(b'p')),
    long("no-run-if-empty", HasArg::No, c(b'r')),
    long("max-chars", HasArg::Required, c(b's')),
    long("verbose", HasArg::No, c(b't')),
    long("show-limits", HasArg::No, SHOW_LIMITS),
    long("exit", HasArg::No, c(b'x')),
    long("max-procs", HasArg::Required, c(b'P')),
    long("process-slot-var", HasArg::Required, PROCESS_SLOT_VAR),
    long("version", HasArg::No, VERSION_OPT),
    long("help", HasArg::No, HELP),
];

const SHORT_OPTIONS: &str = "+0a:E:e::i::I:l::L:n:prs:txP:d:o";

/// Saída antecipada com o código.
struct Exit(i32);

fn err(msg: &str) {
    err_bytes(&[msg.as_bytes()]);
}

fn err_bytes(parts: &[&[u8]]) {
    let mut line = b"xargs: ".to_vec();
    for p in parts {
        line.extend_from_slice(p);
    }
    line.push(b'\n');
    let _ = sys::write_all(Fd::STDERR, &line);
}

fn usage_fail(msg: &str) -> Exit {
    err(msg);
    let _ = sys::write_all(Fd::STDERR, b"Try 'xargs --help' for more information.\n");
    Exit(1)
}

/// Opções já resolvidas.
struct Options {
    /// Separador de itens (`-0` ou `-d`); `None` é a leitura com aspas.
    delim: Option<u8>,
    arg_file: Option<Vec<u8>>,
    eof: Option<Vec<u8>>,
    replace: Option<Vec<u8>>,
    max_lines: Option<usize>,
    max_args: Option<usize>,
    /// 0 = sem limite.
    max_procs: usize,
    interactive: bool,
    open_tty: bool,
    slot_var: Option<Vec<u8>>,
    no_run_if_empty: bool,
    /// O `-s` pedido, ainda sem limitar.
    max_chars: Option<i64>,
    show_limits: bool,
    verbose: bool,
    exit_on_size: bool,
}

/// `strtol` estrito do xargs: o número inteiro, com sinal opcional.
fn parse_num(arg: &[u8], opt: char) -> Result<i64, Exit> {
    let text = String::from_utf8_lossy(arg);
    let t = text.trim_start();
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(usage_fail(&format!("invalid number \"{text}\" for -{opt} option")));
    }
    Ok(t.parse::<i64>().unwrap_or(if t.starts_with('-') { i64::MIN } else { i64::MAX }))
}

fn at_least(arg: &[u8], opt: char, min: i64) -> Result<usize, Exit> {
    let v = parse_num(arg, opt)?;
    if v < min {
        return Err(usage_fail(&format!("value {v} for -{opt} option should be >= {min}")));
    }
    Ok(usize::try_from(v).unwrap_or(usize::MAX))
}

/// `-d`: um caractere só, ou uma sequência de escape.
fn parse_delim(arg: &[u8]) -> Result<u8, Exit> {
    let bad = || {
        err_bytes(&[
            b"Invalid input delimiter specification ",
            arg,
            b": the delimiter must be either a single character or an escape sequence starting with \\.",
        ]);
        Exit(1)
    };
    let radix = |digits: &[u8], base: u32| {
        std::str::from_utf8(digits).ok().and_then(|s| u32::from_str_radix(s, base).ok()).and_then(|v| u8::try_from(v).ok())
    };
    match arg {
        [b] => Ok(*b),
        [b'\\', rest @ ..] => match rest {
            [b'a'] => Ok(7),
            [b'b'] => Ok(8),
            [b'f'] => Ok(12),
            [b'n'] => Ok(b'\n'),
            [b'r'] => Ok(b'\r'),
            [b't'] => Ok(b'\t'),
            [b'v'] => Ok(11),
            [b'\\'] => Ok(b'\\'),
            [b'x', hex @ ..] if (1..=2).contains(&hex.len()) => radix(hex, 16).ok_or_else(bad),
            oct if (1..=3).contains(&oct.len()) && oct.iter().all(|b| (b'0'..=b'7').contains(b)) => {
                radix(oct, 8).ok_or_else(bad)
            }
            _ => Err(bad()),
        },
        _ => Err(bad()),
    }
}

pub(crate) fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv: Vec<Vec<u8>> = args.iter().skip(1).map(|a| a.as_bytes().to_vec()).collect();
    match run(&argv) {
        Ok(code) | Err(Exit(code)) => code,
    }
}

/// Opções e operandos; `Err(código)` quando a opção já resolveu tudo (`--help`, `--version`).
fn parse_options(argv: &[Vec<u8>]) -> Result<Result<(Options, Vec<Vec<u8>>), i32>, Exit> {
    let mut o = Options {
        delim: None,
        arg_file: None,
        eof: None,
        replace: None,
        max_lines: None,
        max_args: None,
        max_procs: 1,
        interactive: false,
        open_tty: false,
        slot_var: None,
        no_run_if_empty: false,
        max_chars: None,
        show_limits: false,
        verbose: false,
        exit_on_size: false,
    };
    let mut eof_given = false;
    let mut g = Getopt::new(argv, SHORT_OPTIONS, LONG_OPTIONS);
    while let Some(opt) = g.next() {
        let opt = match opt {
            Ok(o) => o,
            Err(msg) => return Err(usage_fail(&msg)),
        };
        let arg = opt.arg;
        match opt.val {
            x if x == c(b'0') => o.delim = Some(0),
            x if x == c(b'a') => o.arg_file = arg,
            x if x == c(b'd') => o.delim = Some(parse_delim(&arg.unwrap_or_default())?),
            x if x == c(b'E') => {
                let v = arg.unwrap_or_default();
                o.eof = (!v.is_empty()).then_some(v);
                eof_given = true;
            }
            x if x == c(b'e') => {
                o.eof = arg.filter(|v| !v.is_empty());
                eof_given = o.eof.is_some();
            }
            x if x == c(b'I') || x == c(b'i') => {
                if o.max_args.take().is_some() {
                    err("warning: options --max-args and --replace/-I/-i are mutually exclusive, ignoring previous --max-args value");
                }
                if o.max_lines.take().is_some() {
                    err("warning: options --max-lines and --replace/-I/-i are mutually exclusive, ignoring previous --max-lines value");
                }
                let default = if x == c(b'I') { Vec::new() } else { b"{}".to_vec() };
                o.replace = Some(arg.unwrap_or(default));
            }
            x if x == c(b'L') || x == c(b'l') => {
                let n = match arg {
                    Some(a) => at_least(&a, if x == c(b'L') { 'L' } else { 'l' }, 1)?,
                    None => 1,
                };
                if o.max_args.take().is_some() {
                    err("warning: options --max-args and -L are mutually exclusive, ignoring previous --max-args value");
                }
                if o.replace.take().is_some() {
                    err("warning: options --replace and -L are mutually exclusive, ignoring previous --replace value");
                }
                o.max_lines = Some(n);
            }
            x if x == c(b'n') => {
                let n = at_least(&arg.unwrap_or_default(), 'n', 1)?;
                if o.max_lines.take().is_some() {
                    err("warning: options --max-lines and --max-args/-n are mutually exclusive, ignoring previous --max-lines value");
                }
                if o.replace.take().is_some() {
                    err("warning: options --replace and --max-args/-n are mutually exclusive, ignoring previous --replace value");
                }
                o.max_args = Some(n);
            }
            x if x == c(b'o') => o.open_tty = true,
            x if x == c(b'p') => {
                o.interactive = true;
                o.verbose = true;
            }
            x if x == c(b'r') => o.no_run_if_empty = true,
            x if x == c(b's') => o.max_chars = Some(parse_num(&arg.unwrap_or_default(), 's')?),
            x if x == c(b't') => o.verbose = true,
            x if x == c(b'x') => o.exit_on_size = true,
            x if x == c(b'P') => o.max_procs = at_least(&arg.unwrap_or_default(), 'P', 0)?,
            PROCESS_SLOT_VAR => o.slot_var = arg,
            SHOW_LIMITS => o.show_limits = true,
            HELP => {
                let _ = sys::write_all(Fd::STDOUT, USAGE.as_bytes());
                return Ok(Err(0));
            }
            VERSION_OPT => {
                let _ = sys::write_all(Fd::STDOUT, VERSION.as_bytes());
                return Ok(Err(0));
            }
            _ => return Err(Exit(1)),
        }
    }
    if eof_given && o.delim.is_some() {
        err("warning: the -E option has no effect if -0 or -d is used.\n");
        o.eof = None;
    }
    if o.replace.is_some() {
        // `-I` implica `-x`.
        o.exit_on_size = true;
    }
    Ok(Ok((o, g.operands)))
}

/// Tamanho do ambiente como o xargs conta (cada `NOME=valor` mais o terminador).
fn env_size() -> usize {
    sys::current().environ().iter().map(|kv| kv.len() + 1).sum()
}

fn run(argv: &[Vec<u8>]) -> Result<i32, Exit> {
    let (opts, mut command) = match parse_options(argv)? {
        Ok(v) => v,
        Err(code) => return Ok(code),
    };
    if command.is_empty() {
        command.push(b"echo".to_vec());
    }

    // Limites do tamanho da linha de comando.
    let env = env_size();
    let posix_upper = ARG_MAX - ARG_HEADROOM - env;
    let usable = posix_upper - env;
    let mut max_chars = DEFAULT_MAX_CHARS.min(usable);
    if let Some(v) = opts.max_chars {
        if v < 1 {
            err(&format!("value {v} for -s option should be >= 1"));
            max_chars = 1;
        } else if v as u64 > posix_upper as u64 {
            err(&format!("value {v} for -s option should be <= {posix_upper}"));
            max_chars = posix_upper;
        } else {
            max_chars = v as usize;
        }
    }
    if opts.show_limits {
        let text = format!(
            "Your environment variables take up {env} bytes\n\
             POSIX upper limit on argument length (this system): {posix_upper}\n\
             POSIX smallest allowable upper limit on argument length (all systems): {POSIX_MIN_ARG_MAX}\n\
             Maximum length of command we could actually use: {usable}\n\
             Size of command buffer we are actually using: {max_chars}\n\
             Maximum parallelism (--max-procs must be no greater): {}\n",
            i32::MAX
        );
        let _ = sys::write_all(Fd::STDERR, text.as_bytes());
    }
    let base_size: usize = command.iter().map(|a| a.len() + 1).sum();
    if base_size > max_chars {
        err("cannot fit single argument within argument list size limit");
        return Err(Exit(1));
    }

    // Fonte dos itens.
    let (input_fd, from_file) = match &opts.arg_file {
        Some(path) if path.as_slice() != b"-" => match sys::open(path, OFlags::RDONLY | OFlags::CLOEXEC, 0) {
            Ok(fd) => (fd, true),
            Err(e) => {
                err_bytes(&[b"Cannot open input file \xe2\x80\x98", path, b"\xe2\x80\x99: ", e.message().as_bytes()]);
                return Err(Exit(1));
            }
        },
        Some(_) => (Fd::STDIN, true),
        None => (Fd::STDIN, false),
    };
    let mut reader = Reader::new(input_fd, &opts);
    let mut exec = Executor::new(&opts, from_file);

    let result = drive(&opts, &command, base_size, max_chars, &mut reader, &mut exec);
    let waited = exec.wait_all();
    match result.and(waited) {
        Ok(()) => Ok(if exec.any_failed { 123 } else { 0 }),
        Err(e) => Err(e),
    }
}

/// Lê os itens e dispara os comandos.
fn drive(
    opts: &Options,
    command: &[Vec<u8>],
    base_size: usize,
    max_chars: usize,
    reader: &mut Reader,
    exec: &mut Executor,
) -> Result<(), Exit> {
    if let Some(repl) = &opts.replace {
        // Um comando por linha de entrada, com o item no lugar de cada ocorrência.
        while let Some(item) = reader.next_line_item()? {
            let built: Vec<Vec<u8>> = command.iter().map(|a| replace_all(a, repl, &item)).collect();
            let size: usize = built.iter().map(|a| a.len() + 1).sum();
            if size > max_chars {
                err("argument line too long");
                return Err(Exit(1));
            }
            exec.run(built)?;
        }
        return Ok(());
    }

    let mut batch: Vec<Vec<u8>> = Vec::new();
    let mut size = base_size;
    let mut lines = 0usize;
    let mut ran = false;
    let flush = |batch: &mut Vec<Vec<u8>>, size: &mut usize, lines: &mut usize, exec: &mut Executor| -> Result<(), Exit> {
        let mut full = command.to_vec();
        full.append(batch);
        *size = base_size;
        *lines = 0;
        exec.run(full)
    };
    while let Some((item, eol)) = reader.next_item()? {
        let cost = item.len() + 1;
        if size + cost > max_chars {
            if batch.is_empty() {
                err("argument line too long");
                return Err(Exit(1));
            }
            if opts.exit_on_size && (opts.max_args.is_some() || opts.max_lines.is_some()) {
                err("argument line too long");
                return Err(Exit(1));
            }
            flush(&mut batch, &mut size, &mut lines, exec)?;
            ran = true;
            if size + cost > max_chars {
                err("argument line too long");
                return Err(Exit(1));
            }
        }
        batch.push(item);
        size += cost;
        if eol {
            lines += 1;
        }
        let args_full = opts.max_args.is_some_and(|n| batch.len() >= n);
        let lines_full = opts.max_lines.is_some_and(|n| lines >= n);
        if args_full || lines_full {
            flush(&mut batch, &mut size, &mut lines, exec)?;
            ran = true;
        }
    }
    if !batch.is_empty() || (!ran && !opts.no_run_if_empty) {
        flush(&mut batch, &mut size, &mut lines, exec)?;
    }
    Ok(())
}

/// Troca todas as ocorrências de `pat` em `s` por `with`.
fn replace_all(s: &[u8], pat: &[u8], with: &[u8]) -> Vec<u8> {
    if pat.is_empty() {
        return s.to_vec();
    }
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with(pat) {
            out.extend_from_slice(with);
            i += pat.len();
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

/// Leitor da entrada, em blocos (a entrada pode não ter fim: `yes | xargs -n 1`).
struct Reader {
    fd: Fd,
    buf: Vec<u8>,
    pos: usize,
    eof: bool,
    delim: Option<u8>,
    eof_str: Option<Vec<u8>>,
    /// A string de fim apareceu: o resto da entrada é ignorado.
    stopped: bool,
}

const BLANK: [u8; 2] = [b' ', b'\t'];

impl Reader {
    fn new(fd: Fd, opts: &Options) -> Reader {
        Reader { fd, buf: Vec::new(), pos: 0, eof: false, delim: opts.delim, eof_str: opts.eof.clone(), stopped: false }
    }

    fn byte(&mut self) -> Result<Option<u8>, Exit> {
        if self.pos >= self.buf.len() {
            if self.eof {
                return Ok(None);
            }
            self.buf.resize(64 * 1024, 0);
            self.pos = 0;
            loop {
                match sys::read(self.fd, &mut self.buf) {
                    Ok(0) => {
                        self.buf.clear();
                        self.eof = true;
                        return Ok(None);
                    }
                    Ok(n) => {
                        self.buf.truncate(n);
                        break;
                    }
                    Err(Errno::EINTR) => {}
                    Err(e) => {
                        err(&format!("read error: {}", e.message()));
                        return Err(Exit(1));
                    }
                }
            }
            sys::checkpoint();
        }
        let b = self.buf[self.pos];
        self.pos += 1;
        Ok(Some(b))
    }

    fn unmatched(quote: u8) -> Exit {
        let which = if quote == b'\'' { "single" } else { "double" };
        err(&format!(
            "unmatched {which} quote; by default quotes are special to xargs unless you use the -0 option"
        ));
        Exit(1)
    }

    /// Próximo item e se ele fechou uma linha lógica (pro `-L`).
    fn next_item(&mut self) -> Result<Option<(Vec<u8>, bool)>, Exit> {
        if self.stopped {
            return Ok(None);
        }
        if let Some(d) = self.delim {
            let mut item = Vec::new();
            loop {
                match self.byte()? {
                    None if item.is_empty() => return Ok(None),
                    None => return Ok(Some((item, true))),
                    Some(b) if b == d => return Ok(Some((item, true))),
                    Some(b) => item.push(b),
                }
            }
        }
        // Pula brancos e newlines antes do item.
        let mut b = loop {
            match self.byte()? {
                None => return Ok(None),
                Some(b) if BLANK.contains(&b) || b == b'\n' => {}
                Some(b) => break b,
            }
        };
        let mut item = Vec::new();
        let mut quoted = false;
        let eol;
        loop {
            match b {
                b'\'' | b'"' => {
                    quoted = true;
                    let q = b;
                    loop {
                        match self.byte()? {
                            None | Some(b'\n') => return Err(Self::unmatched(q)),
                            Some(x) if x == q => break,
                            Some(x) => item.push(x),
                        }
                    }
                }
                b'\\' => {
                    quoted = true;
                    if let Some(x) = self.byte()? {
                        item.push(x);
                    }
                }
                _ => item.push(b),
            }
            match self.byte()? {
                None => {
                    eol = true;
                    break;
                }
                Some(b'\n') => {
                    eol = true;
                    break;
                }
                Some(x) if BLANK.contains(&x) => {
                    eol = false;
                    break;
                }
                Some(x) => b = x,
            }
        }
        if !quoted && self.eof_str.as_deref() == Some(item.as_slice()) {
            self.stopped = true;
            return Ok(None);
        }
        Ok(Some((item, eol)))
    }

    /// Modo `-I`: cada linha é um item (brancos iniciais saem; aspas e barra continuam valendo).
    fn next_line_item(&mut self) -> Result<Option<Vec<u8>>, Exit> {
        if self.stopped {
            return Ok(None);
        }
        if let Some(d) = self.delim {
            let mut item = Vec::new();
            loop {
                match self.byte()? {
                    None if item.is_empty() => return Ok(None),
                    None => return Ok(Some(item)),
                    Some(b) if b == d => return Ok(Some(item)),
                    Some(b) => item.push(b),
                }
            }
        }
        loop {
            let mut item = Vec::new();
            let mut quoted = false;
            let mut started = false;
            loop {
                let Some(b) = self.byte()? else {
                    if !started {
                        return Ok(None);
                    }
                    break;
                };
                match b {
                    b'\n' => break,
                    x if !started && BLANK.contains(&x) => {}
                    b'\'' | b'"' => {
                        started = true;
                        quoted = true;
                        loop {
                            match self.byte()? {
                                None | Some(b'\n') => return Err(Self::unmatched(b)),
                                Some(x) if x == b => break,
                                Some(x) => item.push(x),
                            }
                        }
                    }
                    b'\\' => {
                        started = true;
                        quoted = true;
                        if let Some(x) = self.byte()? {
                            item.push(x);
                        }
                    }
                    x => {
                        started = true;
                        item.push(x);
                    }
                }
            }
            if !started {
                // Linha em branco.
                continue;
            }
            if !quoted && self.eof_str.as_deref() == Some(item.as_slice()) {
                self.stopped = true;
                return Ok(None);
            }
            return Ok(Some(item));
        }
    }
}

/// Citação do `-t`/`-p`: como o shell, só quando precisa.
fn shell_quote(arg: &[u8]) -> Vec<u8> {
    let safe = |b: u8| b.is_ascii_alphanumeric() || b"_%+,-./:=@^".contains(&b);
    if !arg.is_empty() && arg.iter().all(|&b| safe(b) || b >= 0x80) {
        return arg.to_vec();
    }
    let mut out = vec![b'\''];
    for &b in arg {
        if b == b'\'' {
            out.extend_from_slice(b"'\\''");
        } else {
            out.push(b);
        }
    }
    out.push(b'\'');
    out
}

/// Um filho em execução.
struct Running {
    child: Child,
    name: Vec<u8>,
    slot: usize,
}

/// Dispara os comandos (até `max_procs` de uma vez) e acumula o código de saída.
struct Executor<'o> {
    opts: &'o Options,
    /// O filho herda o stdin (itens de `-a`); senão, /dev/null.
    inherit_stdin: bool,
    running: Vec<Running>,
    any_failed: bool,
}

impl<'o> Executor<'o> {
    fn new(opts: &'o Options, inherit_stdin: bool) -> Executor<'o> {
        Executor { opts, inherit_stdin, running: Vec::new(), any_failed: false }
    }

    fn free_slot(&self) -> usize {
        (0..).find(|s| !self.running.iter().any(|r| r.slot == *s)).unwrap_or(0)
    }

    fn run(&mut self, args: Vec<Vec<u8>>) -> Result<(), Exit> {
        if self.opts.verbose {
            let line: Vec<Vec<u8>> = args.iter().map(|a| shell_quote(a)).collect();
            let mut text = line.join(&b' ');
            if !self.opts.interactive {
                text.push(b'\n');
                let _ = sys::write_all(Fd::STDERR, &text);
            } else {
                let _ = sys::write_all(Fd::STDERR, &text);
                if !self.confirm()? {
                    return Ok(());
                }
            }
        }
        let limit = if self.opts.max_procs == 0 { usize::MAX } else { self.opts.max_procs };
        while self.running.len() >= limit {
            self.wait_one()?;
        }
        let slot = self.free_slot();
        let name = args[0].clone();
        let mut cmd = Command::new(OsString::from_vec(name.clone()));
        cmd.args(args[1..].iter().map(|a| OsString::from_vec(a.clone())));
        if let Some(var) = &self.opts.slot_var {
            cmd.env(OsString::from_vec(var.clone()), slot.to_string());
        }
        if self.opts.open_tty {
            match sysio::fs::File::open("/dev/tty") {
                Ok(f) => {
                    cmd.stdin(f);
                }
                Err(e) => {
                    err(&format!("failed to open /dev/tty for reading: {}", sysio::errno::strerror(&e)));
                    return Err(Exit(1));
                }
            }
        } else if !self.inherit_stdin {
            cmd.stdin(Stdio::null());
        }
        match cmd.spawn() {
            Ok(child) => {
                self.running.push(Running { child, name, slot });
                if limit == 1 {
                    self.wait_one()?;
                }
                Ok(())
            }
            Err(e) => {
                let code = if e.kind() == sysio::io::ErrorKind::NotFound { 127 } else { 126 };
                err_bytes(&[&name, b": ", sysio::errno::strerror(&e).as_bytes()]);
                Err(Exit(code))
            }
        }
    }

    /// `-p`: pergunta no terminal.
    fn confirm(&mut self) -> Result<bool, Exit> {
        let tty = match sys::open(b"/dev/tty", OFlags::RDONLY | OFlags::CLOEXEC, 0) {
            Ok(fd) => fd,
            Err(e) => {
                err(&format!("failed to open /dev/tty for reading: {}", e.message()));
                return Err(Exit(1));
            }
        };
        let _ = sys::write_all(Fd::STDERR, b" ?...");
        let mut answer = Vec::new();
        let mut byte = [0u8; 1];
        while let Ok(1) = sys::read(tty, &mut byte) {
            if byte[0] == b'\n' {
                break;
            }
            answer.push(byte[0]);
        }
        let _ = sys::close(tty);
        Ok(matches!(answer.first(), Some(b'y' | b'Y')))
    }

    /// Espera um filho qualquer terminar e trata o status.
    fn wait_one(&mut self) -> Result<(), Exit> {
        if self.running.is_empty() {
            return Ok(());
        }
        let idx = if self.running.len() == 1 {
            0
        } else {
            match sys::current().wait4(sysabi::WaitTarget::Any, sysabi::WaitOptions::empty()) {
                Ok(Some((pid, st))) => {
                    match self.running.iter().position(|r| r.child.id() as i32 == pid) {
                        Some(i) => {
                            let r = self.running.remove(i);
                            return self.check(&r.name, st);
                        }
                        // Filho que não é nosso (não deveria acontecer).
                        None => return Ok(()),
                    }
                }
                Ok(None) | Err(_) => 0,
            }
        };
        let mut r = self.running.remove(idx);
        match r.child.wait() {
            Ok(st) => self.check(&r.name, st.wait_status()),
            Err(e) => {
                err(&format!("waiting for child process: {}", sysio::errno::strerror(&e)));
                Err(Exit(1))
            }
        }
    }

    fn check(&mut self, name: &[u8], st: WaitStatus) -> Result<(), Exit> {
        match st {
            WaitStatus::Exited(0) => Ok(()),
            WaitStatus::Exited(255) => {
                err_bytes(&[name, b": exited with status 255; aborting"]);
                Err(Exit(124))
            }
            WaitStatus::Exited(_) => {
                self.any_failed = true;
                Ok(())
            }
            WaitStatus::Signaled { signal, .. } => {
                err_bytes(&[name, format!(": terminated by signal {}", signal.0).as_bytes()]);
                Err(Exit(125))
            }
            WaitStatus::Stopped(_) | WaitStatus::Continued => Ok(()),
        }
    }

    /// Espera todos; o primeiro erro fatal vale.
    fn wait_all(&mut self) -> Result<(), Exit> {
        let mut result = Ok(());
        while !self.running.is_empty() {
            if let Err(e) = self.wait_one()
                && result.is_ok()
            {
                result = Err(e);
            }
        }
        result
    }
}
