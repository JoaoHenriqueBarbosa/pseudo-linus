//! `xxd` do vim 9.1.1230 (pacote `xxd` do Debian 13), fiel byte a byte.
//!
//! O comportamento foi levantado em caixa preta contra o Debian 13 (o código do xxd não foi
//! consultado nem traduzido). Pontos em que o original tem jeito próprio e que este módulo
//! reproduz:
//!
//! - **Argumentos**: não é `getopt`. Cada argumento que começa com `-` é uma opção até o primeiro
//!   operando (ou `--`); só a primeira letra conta (`-ab` é `-a`, `-pu` é `-p`), `--x` vale como
//!   `-x`. Opção com valor aceita o valor colado (`-c8`) ou no argumento seguinte (`-c 8`); a forma
//!   longa (`-cols`, `-group`, `-len`, `-offset`, `-seek`, `-name`) sempre pega o próximo argumento,
//!   e `-capitalize` é o `-C`. Números como o `strtol` com base 0 (`0x10`, `010`), sem reclamar de
//!   lixo no fim. Erro de uso imprime o resumo no stderr e sai com 1.
//! - **Layout por posição**: cada linha é um buffer de espaços; o byte `p` da linha vai pra coluna
//!   `addrlen + 1 + (grplen * x) / g` (com `x = p ^ (g - 1)` no `-e`) e o caractere dele pra coluna
//!   de texto, e a linha termina logo depois do último caractere. Com `-e` e grupo que não é
//!   potência de 2 depois do ajuste ao número de colunas, as posições se atropelam exatamente como
//!   no original.
//! - **Cores** (`-R always`, ou `auto` em terminal): o par hexa e o caractere de cada byte saem na cor
//!   da classe dele (NUL branco, `0xff` azul, tab/LF/CR amarelo, imprimível verde, resto vermelho);
//!   numa linha incompleta, cada byte que falta vira um espaço vermelho numa posição fixa.
//! - **`-a`**: a primeira linha nula de uma sequência sai, a segunda fica guardada, da terceira em
//!   diante vira um `*`; a última linha do arquivo sai sempre.
//! - **`-r`**: máquina de estados por caractere. No começo da linha os dígitos hexa formam o
//!   endereço (o primeiro não hexa encerra); depois, pares de dígitos viram bytes, um caractere não
//!   hexa entre dois dígitos descarta o meio byte, dois não hexa seguidos depois de um byte (ou três
//!   em qualquer lugar) mandam pular o resto da linha, e a linha também acaba ao completar as
//!   colunas. Com `-p` não há endereço nem colunas e espaço, tab e LF são ignorados; com `-b` só `0`
//!   e `1` contam. Antes de cada caractere de dado a saída é posicionada no endereço pedido: em
//!   arquivo com `lseek`, em pipe completando com zeros (pra trás é erro 5).

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, Fd, Mode, OFlags, Whence, sys};

use crate::util::io::{self, File};

const VERSION: &str = "xxd 2024-12-07 by Juergen Weigert et al.";

/// Tabela EBCDIC do `-E`: o caractere mostrado pra cada byte (16 linhas de 16). `.` é "não
/// imprimível", exceto no `0x4b`, que é o ponto de verdade.
const EBCDIC_ROWS: [&[u8; 16]; 16] = [
    b"................",
    b"................",
    b"................",
    b"................",
    b" ...........<(+|",
    b"&.........!$*);~",
    b"-/.........,%_>?",
    b".........`:#@'=\"",
    b".abcdefghi......",
    b".jklmnopqr^.....",
    b"..stuvwxyz...[..",
    b".............]..",
    b"{ABCDEFGHI......",
    b"}JKLMNOPQR......",
    b"\\.STUVWXYZ......",
    b"0123456789......",
];

const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";
const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

/// Cores ANSI das classes de byte (o dígito depois do `3` em `ESC[1;3Xm`).
const COLOR_RED: u8 = b'1';
const COLOR_GREEN: u8 = b'2';
const COLOR_YELLOW: u8 = b'3';
const COLOR_BLUE: u8 = b'4';
const COLOR_WHITE: u8 = b'7';

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColorMode {
    /// Sem `-R`: cor se o stdout é terminal e `NO_COLOR` não está definido (ou está vazio).
    Default,
    /// `-R auto`: cor se o stdout é terminal.
    Auto,
    Always,
    Never,
}

/// `-s [+][-]N`: `+` é relativo à posição corrente, `-` conta do fim (ou pra trás, com `+`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Seek {
    relative: bool,
    negative: bool,
    value: i64,
}

#[derive(Clone, Debug)]
struct Opts {
    autoskip: bool,
    bits: bool,
    little: bool,
    plain: bool,
    include: bool,
    upper: bool,
    decimal: bool,
    revert: bool,
    ebcdic: bool,
    capitalize: bool,
    cols: Option<i32>,
    group: i32,
    length: Option<i64>,
    offset: u64,
    seek: Option<Seek>,
    name: Option<Vec<u8>>,
    color: ColorMode,
    infile: Option<Vec<u8>>,
    outfile: Option<Vec<u8>>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            autoskip: false,
            bits: false,
            little: false,
            plain: false,
            include: false,
            upper: false,
            decimal: false,
            revert: false,
            ebcdic: false,
            capitalize: false,
            cols: None,
            group: -1,
            length: None,
            offset: 0,
            seek: None,
            name: None,
            color: ColorMode::Default,
            infile: None,
            outfile: None,
        }
    }
}

/// Resultado da leitura dos argumentos: seguir, ou sair já com esse código (uso, versão).
enum Parsed {
    Run(Box<Opts>),
    Exit(i32),
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let pname = prog_name(argv.first().map(Vec::as_slice).unwrap_or(b"xxd"));
    let o = match parse(&argv, &pname) {
        Parsed::Run(o) => *o,
        Parsed::Exit(code) => return code,
    };
    match execute(&o, &pname) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

/// O nome do programa nas mensagens: o basename do `argv[0]`.
fn prog_name(argv0: &[u8]) -> String {
    let base = argv0.rsplit(|b| *b == b'/').next().unwrap_or(argv0);
    io::lossy(base)
}

fn usage(pname: &str) -> i32 {
    // Uma linha por item: a continuação `\` de string do Rust come o recuo da linha seguinte, e
    // as opções do original têm quatro espaços na frente.
    let options = [
        "-a          toggle autoskip: A single '*' replaces nul-lines. Default off.",
        "-b          binary digit dump (incompatible with -ps). Default hex.",
        "-C          capitalize variable names in C include file style (-i).",
        "-c cols     format <cols> octets per line. Default 16 (-i: 12, -ps: 30).",
        "-E          show characters in EBCDIC. Default ASCII.",
        "-e          little-endian dump (incompatible with -ps,-i,-r).",
        "-g bytes    number of octets per group in normal output. Default 2 (-e: 4).",
        "-h          print this summary.",
        "-i          output in C include file style.",
        "-l len      stop after <len> octets.",
        "-n name     set the variable name used in C include output (-i).",
        "-o off      add <off> to the displayed file position.",
        "-ps         output in postscript plain hexdump style.",
        "-r          reverse operation: convert (or patch) hexdump into binary.",
        "-r -s off   revert with <off> added to file positions found in hexdump.",
        "-d          show offset in decimal instead of hex.",
        "-s [+][-]seek  start at <seek> bytes abs. (or +: rel.) infile offset.",
        "-u          use upper case hex letters.",
        "-R when     colorize the output; <when> can be 'always', 'auto' or 'never'. Default: 'auto'.",
    ];
    let mut text = format!(
        "Usage:\n       {pname} [options] [infile [outfile]]\n    or\n       {pname} -r [-s [-]offset] [-c cols] [-ps] [infile [outfile]]\nOptions:\n"
    );
    for line in options {
        text.push_str("    ");
        text.push_str(line);
        text.push('\n');
    }
    text.push_str(&format!("    -v          show version: \"{VERSION}\".\n"));
    io::eprint(text);
    1
}

/// Valor de uma opção: colado (`-c8`) ou, na forma curta sozinha ou na longa (`-cols`, que pode
/// vir com lixo depois, como `-cols3`), o próximo argumento. `None` quando falta o argumento.
fn opt_value<'a>(
    pp: &'a [u8],
    long: Option<&[u8]>,
    argv: &'a [Vec<u8>],
    i: &mut usize,
) -> Option<&'a [u8]> {
    if pp.len() == 2 || long.is_some_and(|l| pp.starts_with(l)) {
        *i += 1;
        argv.get(*i).map(Vec::as_slice)
    } else {
        Some(&pp[2..])
    }
}

fn parse(argv: &[Vec<u8>], pname: &str) -> Parsed {
    let mut o = Opts::default();
    let mut i = 1;
    while i < argv.len() {
        let raw = argv[i].as_slice();
        if raw.len() < 2 || raw[0] != b'-' {
            break;
        }
        if raw == b"--" {
            i += 1;
            break;
        }
        let pp = if raw.starts_with(b"--") {
            &raw[1..]
        } else {
            raw
        };
        macro_rules! value {
            ($long:expr) => {
                match opt_value(pp, $long, argv, &mut i) {
                    Some(v) => v,
                    None => return Parsed::Exit(usage(pname)),
                }
            };
        }
        match pp[1] {
            b'a' => o.autoskip = !o.autoskip,
            b'b' => o.bits = true,
            b'e' => o.little = true,
            b'u' => o.upper = true,
            b'p' => o.plain = true,
            b'i' => o.include = true,
            b'd' => o.decimal = true,
            b'r' => o.revert = true,
            b'E' => o.ebcdic = true,
            b'C' => o.capitalize = true,
            b'v' => {
                io::eprint(format!("{VERSION}\n"));
                return Parsed::Exit(0);
            }
            b'h' => return Parsed::Exit(usage(pname)),
            b'c' => {
                if pp.starts_with(b"-capitalize") {
                    o.capitalize = true;
                } else {
                    let v = value!(Some(b"-cols".as_slice()));
                    o.cols = Some(strtol(v) as i32);
                }
            }
            b'g' => {
                let v = value!(Some(b"-group".as_slice()));
                o.group = strtol(v) as i32;
            }
            b'l' => {
                let v = value!(Some(b"-len".as_slice()));
                o.length = Some(strtol(v));
            }
            b'o' => {
                let v = value!(Some(b"-offset".as_slice()));
                o.offset = strtoul(v);
            }
            b's' => {
                let v = value!(Some(b"-seek".as_slice()));
                o.seek = Some(parse_seek(v));
            }
            b'n' => {
                let v = value!(Some(b"-name".as_slice()));
                o.name = Some(v.to_vec());
            }
            b'R' => {
                let v = value!(None);
                o.color = match v {
                    b"always" => ColorMode::Always,
                    b"auto" => ColorMode::Auto,
                    b"never" => ColorMode::Never,
                    _ => return Parsed::Exit(usage(pname)),
                };
            }
            _ => return Parsed::Exit(usage(pname)),
        }
        i += 1;
    }
    let rest = &argv[i.min(argv.len())..];
    if rest.len() > 2 {
        return Parsed::Exit(usage(pname));
    }
    o.infile = rest.first().cloned();
    o.outfile = rest.get(1).cloned();
    Parsed::Run(Box::new(o))
}

fn parse_seek(v: &[u8]) -> Seek {
    let mut s = v;
    let mut relative = false;
    let mut negative = false;
    if let Some(r) = s.strip_prefix(b"+") {
        relative = true;
        s = r;
    }
    if let Some(r) = s.strip_prefix(b"-") {
        negative = true;
        s = r;
    }
    Seek {
        relative,
        negative,
        value: strtol(s),
    }
}

fn c_isspace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Prefixo numérico como o `strtoul`/`strtol` da glibc com base 0: espaços, sinal, `0x` (só se
/// vier dígito hexa depois), `0` inicial octal. Devolve (magnitude saturada, negativo, estourou).
fn strto_parts(s: &[u8]) -> (u64, bool, bool) {
    let mut i = 0;
    while i < s.len() && c_isspace(s[i]) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let base: u64 = if s.get(i) == Some(&b'0') {
        if matches!(s.get(i + 1), Some(b'x' | b'X'))
            && s.get(i + 2).is_some_and(u8::is_ascii_hexdigit)
        {
            i += 2;
            16
        } else {
            8
        }
    } else {
        10
    };
    let mut val: u64 = 0;
    let mut overflow = false;
    while let Some(&b) = s.get(i) {
        let d = match b {
            b'0'..=b'9' => u64::from(b - b'0'),
            b'a'..=b'f' => u64::from(b - b'a' + 10),
            b'A'..=b'F' => u64::from(b - b'A' + 10),
            _ => break,
        };
        if d >= base {
            break;
        }
        match val.checked_mul(base).and_then(|v| v.checked_add(d)) {
            Some(v) => val = v,
            None => overflow = true,
        }
        i += 1;
    }
    (val, neg, overflow)
}

/// `strtol(s, NULL, 0)`: satura em `LONG_MAX`/`LONG_MIN`.
fn strtol(s: &[u8]) -> i64 {
    let (val, neg, overflow) = strto_parts(s);
    if neg {
        if overflow || val > (i64::MAX as u64) + 1 {
            i64::MIN
        } else {
            (val as i64).wrapping_neg()
        }
    } else if overflow || val > i64::MAX as u64 {
        i64::MAX
    } else {
        val as i64
    }
}

/// `strtoul(s, NULL, 0)`: negativo dá a volta, estouro satura em `ULONG_MAX`.
fn strtoul(s: &[u8]) -> u64 {
    let (val, neg, overflow) = strto_parts(s);
    if overflow {
        u64::MAX
    } else if neg {
        val.wrapping_neg()
    } else {
        val
    }
}

/// Mensagem `xxd: ...` no stderr.
fn complain(pname: &str, msg: impl AsRef<[u8]>) {
    let mut line = format!("{pname}: ").into_bytes();
    line.extend_from_slice(msg.as_ref());
    line.push(b'\n');
    io::eprint(line);
}

// ---------------------------------------------------------------------------------------------
// Entrada e saída

/// Entrada com buffer, lida byte a byte como o `getc`.
struct Input {
    fd: Fd,
    _file: Option<File>,
    buf: Vec<u8>,
    pos: usize,
    len: usize,
}

impl Input {
    fn new(file: Option<File>) -> Input {
        let fd = file.as_ref().map_or(Fd::STDIN, File::fd);
        Input {
            fd,
            _file: file,
            buf: vec![0; 4096],
            pos: 0,
            len: 0,
        }
    }

    fn getc(&mut self) -> Result<Option<u8>, Errno> {
        if self.pos == self.len {
            loop {
                match sys::read(self.fd, &mut self.buf) {
                    Ok(0) => return Ok(None),
                    Ok(n) => {
                        self.len = n;
                        self.pos = 0;
                        break;
                    }
                    Err(Errno::EINTR) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        let b = self.buf[self.pos];
        self.pos += 1;
        Ok(Some(b))
    }
}

/// Saída: o stdout do processo (com o buffer do stdio da glibc, via `sysio`) ou o arquivo de
/// saída com buffer de bloco próprio.
enum Output {
    Stdout(sysio::io::Stdout),
    File { file: File, buf: Vec<u8> },
}

impl Output {
    fn put(&mut self, data: &[u8]) -> Result<(), Errno> {
        match self {
            Output::Stdout(s) => s.write_all(data).map_err(|e| io::io_errno(&e)),
            Output::File { file, buf } => {
                if buf.len() + data.len() > 4096 {
                    Self::drain(file, buf)?;
                }
                if data.len() >= 4096 {
                    write_fd(file.fd(), data)
                } else {
                    buf.extend_from_slice(data);
                    Ok(())
                }
            }
        }
    }

    fn drain(file: &File, buf: &mut Vec<u8>) -> Result<(), Errno> {
        let r = write_fd(file.fd(), buf);
        buf.clear();
        r
    }

    fn flush(&mut self) -> Result<(), Errno> {
        match self {
            Output::Stdout(_) => io::flush_stdout().map_err(|e| io::io_errno(&e)),
            Output::File { file, buf } => Self::drain(file, buf),
        }
    }

    fn fd(&self) -> Fd {
        match self {
            Output::Stdout(_) => Fd::STDOUT,
            Output::File { file, .. } => file.fd(),
        }
    }
}

fn write_fd(fd: Fd, data: &[u8]) -> Result<(), Errno> {
    sys::write_all(fd, data)
}

/// Erro de escrita: `xxd: <strerror>` e código 3.
fn write_failed(pname: &str, e: Errno) -> i32 {
    complain(pname, e.message());
    3
}

// ---------------------------------------------------------------------------------------------
// Execução

fn execute(o: &Opts, pname: &str) -> Result<(), i32> {
    if (o.plain && (o.bits || o.little || o.include)) || (o.little && (o.bits || o.include)) {
        complain(pname, "only one of -b, -e, -u, -p, -i can be used");
        return Err(1);
    }
    if let Some(c) = o.cols
        && (c < 0 || (c > 256 && !o.plain && !o.include))
    {
        complain(pname, "invalid number of columns (max. 256).");
        return Err(1);
    }
    let mut group = o.group;
    if group < 0 {
        group = if o.bits {
            1
        } else if o.little {
            4
        } else {
            2
        };
    }
    if o.little && group & (group - 1) != 0 {
        complain(
            pname,
            "number of octets per group must be a power of 2 with -e.",
        );
        return Err(1);
    }

    let infile = o.infile.as_deref().filter(|f| *f != b"-");
    let input = match infile {
        Some(name) => match File::open(name) {
            Ok(f) => Some(f),
            Err(e) => {
                complain(pname, [name, b": ", e.message().as_bytes()].concat());
                return Err(2);
            }
        },
        None => None,
    };
    let outfile = o.outfile.as_deref().filter(|f| *f != b"-");
    let output = match outfile {
        Some(name) => {
            let mut flags = OFlags::WRONLY | OFlags::CREAT;
            if !o.revert {
                flags |= OFlags::TRUNC;
            }
            match File::open_with(name, flags, 0o666 as Mode) {
                Ok(file) => Output::File {
                    file,
                    buf: Vec::new(),
                },
                Err(e) => {
                    complain(pname, [name, b": ", e.message().as_bytes()].concat());
                    return Err(3);
                }
            }
        }
        None => Output::Stdout(io::stdout()),
    };
    let mut input = Input::new(input);
    let mut out = output;

    let result = if o.revert {
        if o.little || o.include {
            complain(pname, "Sorry, cannot revert this type of hexdump");
            return Err(255);
        }
        revert(o, pname, &mut input, &mut out)
    } else {
        dump(o, pname, group, infile, &mut input, &mut out)
    };
    match result {
        Ok(()) => out.flush().map_err(|e| write_failed(pname, e)),
        Err(code) => {
            let _ = out.flush();
            Err(code)
        }
    }
}

/// Aplica o `-s` na entrada. Devolve a posição de onde o dump começa (o `seekoff`).
fn do_seek(s: Seek, pname: &str, input: &mut Input) -> Result<u64, i32> {
    let (whence, off) = match (s.negative, s.relative) {
        (true, true) => (Whence::Cur, s.value.wrapping_neg()),
        (true, false) => (Whence::End, s.value.wrapping_neg()),
        (false, true) => (Whence::Cur, s.value),
        (false, false) => (Whence::Set, s.value),
    };
    match sys::current().lseek(input.fd, off, whence) {
        Ok(pos) => Ok(pos),
        Err(Errno::ESPIPE) if !s.negative && s.value >= 0 => {
            // Entrada que não posiciona (pipe): pula lendo, como o original.
            let mut left = s.value as u64;
            while left > 0 {
                match input.getc() {
                    Ok(Some(_)) => left -= 1,
                    _ => {
                        complain(pname, "Sorry, cannot seek.");
                        return Err(4);
                    }
                }
            }
            Ok(s.value as u64)
        }
        Err(_) => {
            complain(pname, "Sorry, cannot seek.");
            Err(4)
        }
    }
}

/// Se a saída deve sair colorida.
fn want_color(mode: ColorMode, out: &Output) -> bool {
    let tty = || sys::try_current().is_some_and(|s| s.isatty(out.fd()));
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => tty(),
        ColorMode::Default => tty() && sys::getenv("NO_COLOR").is_none_or(|v| v.is_empty()),
    }
}

/// Nome de variável C do `-i`: não alfanumérico vira `_`, dígito no começo ganha `__` na frente;
/// `-C` passa pra maiúsculas.
fn c_name(name: &[u8], capitalize: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len() + 2);
    if name.first().is_some_and(u8::is_ascii_digit) {
        out.extend_from_slice(b"__");
    }
    for &b in name {
        if b.is_ascii_alphanumeric() {
            out.push(if capitalize {
                b.to_ascii_uppercase()
            } else {
                b
            });
        } else {
            out.push(b'_');
        }
    }
    out
}

fn dump(
    o: &Opts,
    pname: &str,
    group: i32,
    infile: Option<&[u8]>,
    input: &mut Input,
    out: &mut Output,
) -> Result<(), i32> {
    let seekoff = match o.seek {
        Some(s) => do_seek(s, pname, input)?,
        None => 0,
    };
    let length = o.length.filter(|l| *l >= 0).map(|l| l as u64);
    let wf = |e: Errno| write_failed(pname, e);
    let read_failed = |e: Errno| {
        complain(pname, e.message());
        2
    };

    if o.plain {
        let cols = match o.cols {
            None => 30,
            Some(c) => c as usize,
        };
        let digits = if o.upper { HEX_UPPER } else { HEX_LOWER };
        let mut n: u64 = 0;
        let mut p = 0usize;
        while length.is_none_or(|l| n < l) {
            let Some(e) = input.getc().map_err(read_failed)? else {
                break;
            };
            out.put(&[digits[usize::from(e >> 4)], digits[usize::from(e & 15)]])
                .map_err(wf)?;
            n += 1;
            p += 1;
            if cols > 0 && p == cols {
                out.put(b"\n").map_err(wf)?;
                p = 0;
            }
        }
        if p > 0 {
            out.put(b"\n").map_err(wf)?;
        }
        return Ok(());
    }

    if o.include {
        let cols = match o.cols {
            Some(c) if c > 0 => c as usize,
            _ if o.bits => 6,
            _ => 12,
        };
        let name = match (&o.name, infile) {
            (Some(n), _) => Some(c_name(n, o.capitalize)),
            (None, Some(f)) => Some(c_name(f, o.capitalize)),
            (None, None) => None,
        };
        if let Some(name) = &name {
            out.put(&[b"unsigned char ".as_slice(), name, b"[] = {\n"].concat())
                .map_err(wf)?;
        }
        let digits = if o.upper { HEX_UPPER } else { HEX_LOWER };
        let mut n: u64 = 0;
        while length.is_none_or(|l| n < l) {
            let Some(e) = input.getc().map_err(read_failed)? else {
                break;
            };
            let mut item: Vec<u8> = Vec::with_capacity(16);
            if n == 0 {
                item.extend_from_slice(b"  ");
            } else if n.is_multiple_of(cols as u64) {
                item.extend_from_slice(b",\n  ");
            } else {
                item.extend_from_slice(b", ");
            }
            if o.bits {
                item.extend_from_slice(b"0b");
                for k in 0..8 {
                    item.push(if e & (0x80 >> k) != 0 { b'1' } else { b'0' });
                }
            } else {
                item.extend_from_slice(if o.upper { b"0X" } else { b"0x" });
                item.push(digits[usize::from(e >> 4)]);
                item.push(digits[usize::from(e & 15)]);
            }
            out.put(&item).map_err(wf)?;
            n += 1;
        }
        if n > 0 {
            out.put(b"\n").map_err(wf)?;
        }
        if let Some(name) = &name {
            let len_suffix: &[u8] = if o.capitalize { b"_LEN" } else { b"_len" };
            let tail = [
                b"};\nunsigned int ".as_slice(),
                name,
                len_suffix,
                format!(" = {n};\n").as_bytes(),
            ]
            .concat();
            out.put(&tail).map_err(wf)?;
        }
        return Ok(());
    }

    let cols = match o.cols {
        Some(c) if c > 0 => c as usize,
        _ if o.bits => 6,
        _ => 16,
    };
    let mut g = group as usize;
    if g == 0 || g > cols {
        g = cols;
    }
    let color = want_color(o.color, out);
    let layout = Layout {
        cols,
        group: g,
        bits: o.bits,
        little: o.little,
    };
    let mut line = LineBuf::new(layout.capacity());
    let mut skip = AutoSkip::default();
    let digits = if o.upper { HEX_UPPER } else { HEX_LOWER };
    let mut n: u64 = 0;
    let mut p = 0usize;
    let mut nonzero = 0usize;
    let mut addrlen = 0usize;
    let mut text: Vec<u8> = Vec::new();
    while length.is_none_or(|l| n < l) {
        let Some(e) = input.getc().map_err(read_failed)? else {
            break;
        };
        if p == 0 {
            let addr = n.wrapping_add(seekoff).wrapping_add(o.offset);
            let s = if o.decimal {
                format!("{:08}:", addr as i64)
            } else {
                format!("{addr:08x}:")
            };
            addrlen = s.len();
            line.reset(addrlen + layout.capacity());
            line.put_str(0, s.as_bytes());
        }
        let class = byte_color(e, o.ebcdic);
        let x = if o.little { p ^ (g - 1) } else { p };
        let c = addrlen + 1 + (layout.grplen() * x) / g;
        if o.bits {
            for k in 0..8 {
                line.put(
                    c + k,
                    if e & (0x80 >> k) != 0 { b'1' } else { b'0' },
                    0,
                    false,
                );
            }
        } else {
            let col = if color { class } else { 0 };
            line.put(c, digits[usize::from(e >> 4)], col, true);
            line.put(c + 1, digits[usize::from(e & 15)], col, false);
        }
        if e != 0 {
            nonzero += 1;
        }
        let a = layout.ascii_start(addrlen) + p;
        line.put(
            a,
            display_char(e, o.ebcdic),
            if color { class } else { 0 },
            true,
        );
        n += 1;
        p += 1;
        if p == cols {
            text.clear();
            line.render(a + 1, &mut text);
            text.push(b'\n');
            skip.line(out, &text, o.autoskip && nonzero == 0)
                .map_err(wf)?;
            nonzero = 0;
            p = 0;
        }
    }
    if p > 0 {
        if color {
            fill_missing(&mut line, &layout, addrlen, p);
        }
        text.clear();
        line.render(layout.ascii_start(addrlen) + p, &mut text);
        text.push(b'\n');
        skip.line(out, &text, false).map_err(wf)?;
    } else {
        skip.finish(out).map_err(wf)?;
    }
    Ok(())
}

/// Geometria de uma linha do dump normal.
struct Layout {
    cols: usize,
    group: usize,
    bits: bool,
    little: bool,
}

impl Layout {
    /// Largura de um grupo com o espaço que o separa do próximo.
    fn grplen(&self) -> usize {
        if self.bits {
            8 * self.group + 1
        } else {
            2 * self.group + 1
        }
    }

    /// Coluna onde começa o texto (ASCII ou EBCDIC) da linha.
    fn ascii_start(&self, addrlen: usize) -> usize {
        if self.little {
            let groups = self.cols.div_ceil(self.group);
            addrlen + 2 + self.grplen() * groups
        } else {
            addrlen + 3 + (self.grplen() * self.cols - 1) / self.group
        }
    }

    /// Células necessárias depois do endereço (com folga pros atropelos do `-e` sem potência de 2).
    fn capacity(&self) -> usize {
        let span = 2 * self.cols.max(self.group) + 2;
        self.ascii_start(0) + self.cols + (self.grplen() * span) / self.group + 16
    }
}

/// Os bytes que faltam numa linha incompleta, com cor: um espaço vermelho por byte, nas posições
/// do original (primeiro o resto do grupo corrente no `-e`, depois o bloco no fim da área hexa).
fn fill_missing(line: &mut LineBuf, layout: &Layout, addrlen: usize, p: usize) {
    let g = layout.group;
    let grplen = layout.grplen();
    let mut p = p;
    let mut x = p;
    if layout.little {
        let fill = (g - p % g) % g;
        if fill > 0 {
            let start = addrlen + 1 + (grplen * (x - (g - fill))) / g;
            for c in (start..).take(fill) {
                line.put(c, b' ', COLOR_RED, true);
                x += 1;
                p += 1;
            }
        }
    }
    if !layout.bits {
        let rem = layout.cols.saturating_sub(p);
        let start = addrlen + 1 + (grplen * x) / g + rem + rem / g;
        for c in (start..).take(rem) {
            line.put(c, b' ', COLOR_RED, true);
        }
    }
}

/// Caractere da coluna de texto.
fn display_char(e: u8, ebcdic: bool) -> u8 {
    if ebcdic {
        EBCDIC_ROWS[usize::from(e >> 4)][usize::from(e & 15)]
    } else if (32..127).contains(&e) {
        e
    } else {
        b'.'
    }
}

/// Classe de cor de um byte.
fn byte_color(e: u8, ebcdic: bool) -> u8 {
    if e == 0 {
        return COLOR_WHITE;
    }
    if e == 0xff {
        return COLOR_BLUE;
    }
    if ebcdic {
        if matches!(e, 0x05 | 0x25 | 0x0d) {
            COLOR_YELLOW
        } else if EBCDIC_ROWS[usize::from(e >> 4)][usize::from(e & 15)] != b'.' || e == 0x4b {
            COLOR_GREEN
        } else {
            COLOR_RED
        }
    } else if matches!(e, b'\t' | b'\n' | b'\r') {
        COLOR_YELLOW
    } else if (32..127).contains(&e) {
        COLOR_GREEN
    } else {
        COLOR_RED
    }
}

/// Uma linha em montagem: células de um caractere, cada uma com cor opcional; `start` marca o
/// começo de uma unidade colorida (o par hexa de um byte, um caractere, um espaço de preenchimento).
struct LineBuf {
    chars: Vec<u8>,
    colors: Vec<u8>,
    starts: Vec<bool>,
}

impl LineBuf {
    fn new(cap: usize) -> LineBuf {
        LineBuf {
            chars: vec![b' '; cap],
            colors: vec![0; cap],
            starts: vec![false; cap],
        }
    }

    fn reset(&mut self, cap: usize) {
        if self.chars.len() < cap {
            self.chars.resize(cap, b' ');
            self.colors.resize(cap, 0);
            self.starts.resize(cap, false);
        }
        self.chars.fill(b' ');
        self.colors.fill(0);
        self.starts.fill(false);
    }

    fn put(&mut self, at: usize, ch: u8, color: u8, start: bool) {
        if at >= self.chars.len() {
            let cap = at + 64;
            self.chars.resize(cap, b' ');
            self.colors.resize(cap, 0);
            self.starts.resize(cap, false);
        }
        self.chars[at] = ch;
        self.colors[at] = color;
        self.starts[at] = start;
    }

    fn put_str(&mut self, at: usize, s: &[u8]) {
        for (k, &b) in s.iter().enumerate() {
            self.put(at + k, b, 0, false);
        }
    }

    /// As células `[0, end)` como bytes, com as sequências de cor.
    fn render(&self, end: usize, out: &mut Vec<u8>) {
        let end = end.min(self.chars.len());
        let mut i = 0;
        while i < end {
            let color = self.colors[i];
            if color != 0 && self.starts[i] {
                let mut j = i + 1;
                while j < end && !self.starts[j] && self.colors[j] == color {
                    j += 1;
                }
                out.extend_from_slice(b"\x1b[1;3");
                out.push(color);
                out.push(b'm');
                out.extend_from_slice(&self.chars[i..j]);
                out.extend_from_slice(b"\x1b[0m");
                i = j;
            } else {
                out.push(self.chars[i]);
                i += 1;
            }
        }
    }
}

/// O `-a`: sequências de linhas nulas.
#[derive(Default)]
struct AutoSkip {
    /// Linhas nulas seguidas até agora.
    run: u32,
    /// A segunda linha nula da sequência, que sai se a sequência tiver só duas.
    pending: Vec<u8>,
    /// A linha nula mais recente (a última do arquivo sai sempre).
    last: Vec<u8>,
}

impl AutoSkip {
    fn line(&mut self, out: &mut Output, text: &[u8], nul: bool) -> Result<(), Errno> {
        if nul {
            self.run += 1;
            match self.run {
                1 => out.put(text)?,
                2 => self.pending = text.to_vec(),
                _ => {}
            }
            self.last.clear();
            self.last.extend_from_slice(text);
            return Ok(());
        }
        self.close_run(out, self.run)?;
        self.run = 0;
        out.put(text)
    }

    /// Fecha uma sequência de `run` linhas nulas antes de uma linha que sai.
    fn close_run(&self, out: &mut Output, run: u32) -> Result<(), Errno> {
        if run == 2 {
            out.put(&self.pending)?;
        } else if run > 2 {
            out.put(b"*\n")?;
        }
        Ok(())
    }

    /// Fim do arquivo depois de uma linha completa: se ela era nula e ficou retida, sai agora.
    fn finish(&mut self, out: &mut Output) -> Result<(), Errno> {
        if self.run >= 2 {
            self.close_run(out, self.run - 1)?;
            let last = std::mem::take(&mut self.last);
            out.put(&last)?;
        }
        self.run = 0;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// -r

fn hex_value(c: u8) -> i32 {
    match c {
        b'0'..=b'9' => i32::from(c - b'0'),
        b'a'..=b'f' => i32::from(c - b'a' + 10),
        b'A'..=b'F' => i32::from(c - b'A' + 10),
        _ => -1,
    }
}

/// Erro do `-r` ao posicionar a saída.
enum RevertError {
    Backwards,
    Write(Errno),
}

/// Posiciona a saída em `target` (a posição atual é `have`): `lseek` quando dá; em pipe, completa
/// com zeros pra frente. Pra trás num pipe, ou posição negativa, é erro.
fn position_output(out: &mut Output, target: i64, have: i64) -> Result<(), RevertError> {
    if target < 0 {
        return Err(RevertError::Backwards);
    }
    out.flush().map_err(RevertError::Write)?;
    match sys::current().lseek(out.fd(), target, Whence::Set) {
        Ok(_) => Ok(()),
        Err(Errno::ESPIPE) if target > have => {
            let zeros = [0u8; 4096];
            let mut left = (target - have) as u64;
            while left > 0 {
                let k = left.min(zeros.len() as u64) as usize;
                out.put(&zeros[..k]).map_err(RevertError::Write)?;
                left -= k as u64;
            }
            Ok(())
        }
        Err(_) => Err(RevertError::Backwards),
    }
}

/// Lê até o fim da linha (ou do arquivo); devolve o caractere que parou (`\n`) ou `None` no fim.
fn skip_to_eol(input: &mut Input) -> Option<u8> {
    loop {
        match input.getc() {
            Ok(Some(b'\n')) => return Some(b'\n'),
            Ok(Some(_)) => {}
            _ => return None,
        }
    }
}

fn revert(o: &Opts, pname: &str, input: &mut Input, out: &mut Output) -> Result<(), i32> {
    let plain = o.plain;
    let bits = o.bits;
    let cols = match o.cols {
        Some(c) if c > 0 => c as usize,
        _ if bits => 6,
        _ => 16,
    };
    let base: i64 = match o.seek {
        Some(s) if s.negative => s.value.wrapping_neg(),
        Some(s) => s.value,
        None => 0,
    };
    let (mut n1, mut n2) = (-1i32, -1i32);
    let mut n3: i32;
    let mut p = cols;
    let mut ignore = true;
    let mut want: i64 = 0;
    let mut have: i64 = 0;
    let mut acc: u32 = 0;
    let mut nbits = 0u32;
    let fail = |e: RevertError| match e {
        RevertError::Backwards => {
            complain(pname, "Sorry, cannot seek backwards.");
            5
        }
        RevertError::Write(e) => write_failed(pname, e),
    };
    let mut count = 0u32;
    loop {
        count = count.wrapping_add(1);
        if count.is_multiple_of(4096) {
            sys::checkpoint();
        }
        // Erro de leitura no -r é fim de arquivo, como no original.
        let Ok(Some(byte)) = input.getc() else { break };
        let mut c = Some(byte);
        if byte == b'\r' {
            continue;
        }
        if plain && matches!(byte, b' ' | b'\t' | b'\n') {
            continue;
        }
        n3 = n2;
        n2 = n1;
        n1 = hex_value(byte);
        if n1 < 0 && ignore {
            continue;
        }
        ignore = false;
        if !plain && p >= cols {
            if n1 < 0 {
                p = 0;
                continue;
            }
            want = ((want as u64) << 4 | n1 as u64) as i64;
            continue;
        }
        let target = base.wrapping_add(want);
        if target != have {
            position_output(out, target, have).map_err(fail)?;
            have = target;
        }
        if bits {
            if byte == b'0' || byte == b'1' {
                acc = (acc << 1) | u32::from(byte - b'0');
                nbits += 1;
                if nbits == 8 {
                    out.put(&[acc as u8]).map_err(|e| write_failed(pname, e))?;
                    have += 1;
                    want += 1;
                    acc = 0;
                    nbits = 0;
                    p += 1;
                    if p >= cols {
                        c = skip_to_eol(input);
                    }
                }
            }
        } else if n2 >= 0 && n1 >= 0 {
            out.put(&[((n2 << 4) | n1) as u8])
                .map_err(|e| write_failed(pname, e))?;
            have += 1;
            want += 1;
            n1 = -1;
            if !plain {
                p += 1;
                if p >= cols {
                    c = skip_to_eol(input);
                }
            }
        } else if n1 < 0 && n2 < 0 && n3 < 0 {
            c = skip_to_eol(input);
        }
        if c == Some(b'\n') {
            if !plain {
                want = 0;
                p = cols;
            }
            ignore = true;
            acc = 0;
            nbits = 0;
        }
        if c.is_none() {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sysabi::Program;
    use sysabi::testkit::{RunResult, TestKit};

    // Saídas esperadas capturadas do xxd 9.1.1230 do Debian 13 (imagem
    // pseudo-linus-oracle:719900900623), em /work, com LC_ALL=C.UTF-8.

    fn kit(files: &[(&str, &[u8])]) -> TestKit {
        let kit = TestKit::new()
            .programs([Program::bin("xxd", main)])
            .dir("/work", 0o755)
            .cwd("/work");
        for (name, data) in files {
            kit.put_file(format!("/work/{name}").as_bytes(), data, 0o644);
        }
        kit
    }

    fn xxd(args: &[&str], stdin: &[u8], files: &[(&str, &[u8])]) -> RunResult {
        let mut argv = vec!["xxd"];
        argv.extend_from_slice(args);
        kit(files).run(&argv, stdin)
    }

    const TXT: &[u8] = b"The quick brown fox jumps over the lazy dog.\n0123456789";

    fn out(args: &[&str]) -> String {
        xxd(args, b"", &[("txt", TXT)]).stdout_str()
    }

    #[test]
    fn strtol_like_glibc() {
        assert_eq!(strtol(b"0x10"), 16);
        assert_eq!(strtol(b"010"), 8);
        assert_eq!(strtol(b"08"), 0);
        assert_eq!(strtol(b"1e"), 1);
        assert_eq!(strtol(b" -5"), -5);
        assert_eq!(strtol(b"0x"), 0);
        assert_eq!(strtol(b"999999999999999999999"), i64::MAX);
        assert_eq!(strtoul(b"-1"), u64::MAX);
        assert_eq!(strtoul(b"999999999999999999999"), u64::MAX);
        assert_eq!(strtol(b"2147483648") as i32, i32::MIN);
    }

    #[test]
    fn seek_spec() {
        assert_eq!(
            parse_seek(b"+-2"),
            Seek {
                relative: true,
                negative: true,
                value: 2
            }
        );
        assert_eq!(
            parse_seek(b"-+2"),
            Seek {
                relative: false,
                negative: true,
                value: 2
            }
        );
        assert_eq!(
            parse_seek(b"++2"),
            Seek {
                relative: true,
                negative: false,
                value: 2
            }
        );
        assert_eq!(
            parse_seek(b"-"),
            Seek {
                relative: false,
                negative: true,
                value: 0
            }
        );
    }

    #[test]
    fn include_names() {
        assert_eq!(c_name(b"sub/my-file.v2.bin", false), b"sub_my_file_v2_bin");
        assert_eq!(c_name(b"9lives", false), b"__9lives");
        assert_eq!(c_name("ção".as_bytes(), false), b"____o");
        assert_eq!(c_name(b"Ab.c", true), b"AB_C");
        assert_eq!(c_name(b"", false), b"");
    }

    #[test]
    fn default_dump() {
        assert_eq!(
            out(&["txt"]),
            "00000000: 5468 6520 7175 6963 6b20 6272 6f77 6e20  The quick brown \n\
             00000010: 666f 7820 6a75 6d70 7320 6f76 6572 2074  fox jumps over t\n\
             00000020: 6865 206c 617a 7920 646f 672e 0a30 3132  he lazy dog..012\n\
             00000030: 3334 3536 3738 39                        3456789\n"
        );
    }

    #[test]
    fn cols_and_groups() {
        assert_eq!(
            out(&["-c", "5", "-l", "10", "txt"]),
            "00000000: 5468 6520 71  The q\n00000005: 7569 636b 20  uick \n"
        );
        assert_eq!(
            out(&["-c", "7", "-g", "3", "-l", "14", "txt"]),
            "00000000: 546865 207175 69  The qui\n00000007: 636b20 62726f 77  ck brow\n"
        );
        assert_eq!(
            out(&["-g", "4", "-l", "16", "txt"]),
            "00000000: 54686520 71756963 6b206272 6f776e20  The quick brown \n"
        );
        assert_eq!(
            out(&["-g", "5", "-c", "12", "-s", "48", "txt"]),
            "00000030: 3334353637 3839             3456789\n"
        );
        assert_eq!(
            out(&["-c", "33", "-g", "0", "-s", "33", "txt"]),
            "00000021: 65206c617a7920646f672e0a30313233343536373839                        e lazy dog..0123456789\n"
        );
    }

    #[test]
    fn little_endian() {
        assert_eq!(
            out(&["-e", "-s", "32", "txt"]),
            "00000020: 6c206568 20797a61 2e676f64 3231300a  he lazy dog..012\n00000030: 36353433   393837                    3456789\n"
        );
        assert_eq!(
            out(&["-e", "-c", "10", "-l", "10", "txt"]),
            "00000000: 20656854 63697571     206b  The quick \n"
        );
        assert_eq!(
            out(&["-e", "-g", "8", "-s", "48", "txt"]),
            "00000030:   39383736353433                   3456789\n"
        );
    }

    #[test]
    fn little_endian_odd_group_overlaps() {
        let r = xxd(
            &["-e", "-c", "6", "-g", "8", "-l", "6", "s"],
            b"",
            &[("s", b"abcdefghijklmnopqrstuvwxyz")],
        );
        assert_eq!(r.stdout_str(), "00000000: 6665    6261 646cdef\n");
        let r = xxd(
            &["-e", "-c", "3", "-l", "3", "s"],
            b"",
            &[("s", b"abcdefghijklmnopqrstuvwxyz")],
        );
        assert_eq!(r.stdout_str(), "00000000: 63  61 62bc\n");
    }

    #[test]
    fn little_endian_group_must_be_power_of_two() {
        let r = xxd(&["-e", "-g", "3", "txt"], b"", &[("txt", TXT)]);
        assert_eq!(
            r.stderr_str(),
            "xxd: number of octets per group must be a power of 2 with -e.\n"
        );
        assert_eq!(r.code(), 1);
    }

    #[test]
    fn bits() {
        // No `-b` o texto começa na coluna `addrlen + 3 + (grplen * cols - 1) / g`, com
        // `grplen = 8 * g + 1` e 6 colunas: 65 com `-g 1` e 62 com `-g 2` (o endereço tem 9).
        assert_eq!(
            out(&["-b", "-l", "8", "txt"]),
            format!(
                "00000000: 01010100 01101000 01100101 00100000 01110001 01110101  The qu\n\
                 00000006: 01101001 01100011{}ic\n",
                " ".repeat(65 - 27)
            )
        );
        assert_eq!(
            out(&["-b", "-g", "2", "-l", "3", "txt"]),
            format!(
                "00000000: 0101010001101000 01100101{}The\n",
                " ".repeat(62 - 35)
            )
        );
    }

    #[test]
    fn upper_decimal_offset() {
        assert_eq!(
            out(&["-u", "-l", "16", "txt"]),
            "00000000: 5468 6520 7175 6963 6B20 6272 6F77 6E20  The quick brown \n"
        );
        assert_eq!(
            out(&["-d", "-s", "3", "-l", "20", "txt"]),
            "00000003: 2071 7569 636b 2062 726f 776e 2066 6f78   quick brown fox\n00000019: 206a 756d                                 jum\n"
        );
        assert_eq!(
            out(&["-d", "-o", "-5", "-l", "2", "txt"]),
            "-0000005: 5468                                     Th\n"
        );
        assert_eq!(
            out(&["-o", "-1", "-l", "2", "txt"]),
            "ffffffffffffffff: 5468                                     Th\n"
        );
        assert_eq!(
            out(&["-u", "-o", "0xab", "-l", "2", "txt"]),
            "000000ab: 5468                                     Th\n"
        );
    }

    #[test]
    fn ebcdic() {
        assert_eq!(
            out(&["-E", "-l", "16", "txt"]),
            "00000000: 5468 6520 7175 6963 6b20 6272 6f77 6e20  ........,...?.>.\n"
        );
        let all: Vec<u8> = (0..=255u8).collect();
        let r = xxd(&["-E", "-s", "64", "-l", "16", "a"], b"", &[("a", &all)]);
        assert_eq!(
            r.stdout_str(),
            "00000040: 4041 4243 4445 4647 4849 4a4b 4c4d 4e4f   ...........<(+|\n"
        );
    }

    #[test]
    fn autoskip_runs() {
        let mut zz = vec![b'A'; 16];
        zz.extend([0u8; 64]);
        zz.extend([b'B'; 5]);
        zz.extend([0u8; 32]);
        zz.extend([b'C'; 3]);
        zz.extend([0u8; 48]);
        let r = xxd(&["-a", "zz"], b"", &[("zz", &zz)]);
        assert_eq!(
            r.stdout_str(),
            "00000000: 4141 4141 4141 4141 4141 4141 4141 4141  AAAAAAAAAAAAAAAA\n\
             00000010: 0000 0000 0000 0000 0000 0000 0000 0000  ................\n\
             *\n\
             00000050: 4242 4242 4200 0000 0000 0000 0000 0000  BBBBB...........\n\
             00000060: 0000 0000 0000 0000 0000 0000 0000 0000  ................\n\
             00000070: 0000 0000 0043 4343 0000 0000 0000 0000  .....CCC........\n\
             00000080: 0000 0000 0000 0000 0000 0000 0000 0000  ................\n\
             00000090: 0000 0000 0000 0000 0000 0000 0000 0000  ................\n\
             000000a0: 0000 0000 0000 0000                      ........\n"
        );
        let r = xxd(&["-a", "-c", "8", "zz"], b"", &[("zz", &zz)]);
        assert_eq!(
            r.stdout_str(),
            "00000000: 4141 4141 4141 4141  AAAAAAAA\n00000008: 4141 4141 4141 4141  AAAAAAAA\n\
             00000010: 0000 0000 0000 0000  ........\n*\n00000050: 4242 4242 4200 0000  BBBBB...\n\
             00000058: 0000 0000 0000 0000  ........\n*\n00000070: 0000 0000 0043 4343  .....CCC\n\
             00000078: 0000 0000 0000 0000  ........\n*\n000000a0: 0000 0000 0000 0000  ........\n"
        );
    }

    #[test]
    fn autoskip_tail_cases() {
        let mut z3 = vec![0u8; 48];
        z3.push(b'A');
        z3.extend([0u8; 15 + 48]);
        let r = xxd(&["-a", "z3"], b"", &[("z3", &z3)]);
        assert_eq!(
            r.stdout_str(),
            "00000000: 0000 0000 0000 0000 0000 0000 0000 0000  ................\n*\n\
             00000030: 4100 0000 0000 0000 0000 0000 0000 0000  A...............\n\
             00000040: 0000 0000 0000 0000 0000 0000 0000 0000  ................\n\
             00000050: 0000 0000 0000 0000 0000 0000 0000 0000  ................\n\
             00000060: 0000 0000 0000 0000 0000 0000 0000 0000  ................\n"
        );
        let r = xxd(&["-a", "z5"], b"", &[("z5", &[0u8; 67])]);
        assert_eq!(
            r.stdout_str(),
            "00000000: 0000 0000 0000 0000 0000 0000 0000 0000  ................\n*\n00000040: 0000 00                                  ...\n"
        );
        let r = xxd(&["-a", "-a", "z5"], b"", &[("z5", &[0u8; 32])]);
        assert_eq!(r.stdout_str().lines().count(), 2);
    }

    #[test]
    fn plain() {
        assert_eq!(
            out(&["-p", "txt"]),
            "54686520717569636b2062726f776e20666f78206a756d7073206f766572\n20746865206c617a7920646f672e0a30313233343536373839\n"
        );
        assert_eq!(
            out(&["-p", "-c", "0", "-l", "20", "txt"]),
            "54686520717569636b2062726f776e20666f7820\n"
        );
        assert_eq!(
            out(&["-ps", "-u", "-c", "10", "-l", "15", "txt"]),
            "54686520717569636B20\n62726F776E\n"
        );
        assert_eq!(
            out(&["-p", "-s", "5", "-l", "7", "txt"]),
            "7569636b206272\n"
        );
    }

    #[test]
    fn include() {
        let r = xxd(&["-i", "small"], b"", &[("small", b"abc")]);
        assert_eq!(
            r.stdout_str(),
            "unsigned char small[] = {\n  0x61, 0x62, 0x63\n};\nunsigned int small_len = 3;\n"
        );
        let r = xxd(
            &["-i", "-C", "-c", "2", "-u", "my-file"],
            b"",
            &[("my-file", b"abc")],
        );
        assert_eq!(
            r.stdout_str(),
            "unsigned char MY_FILE[] = {\n  0X61, 0X62,\n  0X63\n};\nunsigned int MY_FILE_LEN = 3;\n"
        );
        let r = xxd(&["-i", "-b", "-n", "9x", "small"], b"", &[("small", b"ab")]);
        assert_eq!(
            r.stdout_str(),
            "unsigned char __9x[] = {\n  0b01100001, 0b01100010\n};\nunsigned int __9x_len = 2;\n"
        );
        let r = xxd(&["-i", "e"], b"", &[("e", b"")]);
        assert_eq!(
            r.stdout_str(),
            "unsigned char e[] = {\n};\nunsigned int e_len = 0;\n"
        );
    }

    #[test]
    fn include_stdin() {
        assert_eq!(
            xxd(&["-i"], b"abc", &[]).stdout_str(),
            "  0x61, 0x62, 0x63\n"
        );
        assert_eq!(
            xxd(&["-i", "-"], b"abc", &[]).stdout_str(),
            "  0x61, 0x62, 0x63\n"
        );
        assert_eq!(
            xxd(&["-i", "-n", "zz", "-C"], b"abc", &[]).stdout_str(),
            "unsigned char ZZ[] = {\n  0x61, 0x62, 0x63\n};\nunsigned int ZZ_LEN = 3;\n"
        );
        assert_eq!(xxd(&["-i"], b"", &[]).stdout_str(), "");
    }

    #[test]
    fn lazy_option_parser() {
        let f: &[(&str, &[u8])] = &[("small", b"abc")];
        assert_eq!(
            xxd(&["-ab", "small"], b"", f).stdout_str(),
            "00000000: 6162 63                                  abc\n"
        );
        assert_eq!(xxd(&["-pu", "small"], b"", f).stdout_str(), "616263\n");
        assert_eq!(
            xxd(&["-up", "small"], b"", f).stdout_str(),
            "00000000: 6162 63                                  abc\n"
        );
        assert_eq!(xxd(&["--p", "small"], b"", f).stdout_str(), "616263\n");
        assert_eq!(
            xxd(&["-cols3", "2", "small"], b"", f).stdout_str(),
            "00000000: 6162  ab\n00000002: 63    c\n"
        );
        assert_eq!(
            xxd(&["-capitalizex", "-i", "small"], b"", f).stdout_str(),
            "unsigned char SMALL[] = {\n  0x61, 0x62, 0x63\n};\nunsigned int SMALL_LEN = 3;\n"
        );
        let r = xxd(&["-col", "3", "small"], b"", f);
        assert_eq!(r.stderr_str(), "xxd: 3: No such file or directory\n");
        assert_eq!(r.code(), 2);
    }

    #[test]
    fn usage_and_version() {
        let r = xxd(&["-h"], b"", &[]);
        assert_eq!(r.code(), 1);
        assert!(
            r.stderr_str()
                .starts_with("Usage:\n       xxd [options] [infile [outfile]]\n    or\n")
        );
        assert!(r.stderr_str().ends_with(
            "    -v          show version: \"xxd 2024-12-07 by Juergen Weigert et al.\".\n"
        ));
        assert_eq!(r.stdout, b"");
        let r = xxd(&["--version"], b"", &[]);
        assert_eq!(
            (r.code(), r.stderr_str().as_str()),
            (0, "xxd 2024-12-07 by Juergen Weigert et al.\n")
        );
        for bad in [
            &["-x"][..],
            &["-R", "alw"],
            &["-c"],
            &["a", "b", "c"],
            &["---p"],
        ] {
            let r = xxd(bad, b"", &[]);
            assert_eq!(r.code(), 1, "{bad:?}");
            assert!(r.stderr_str().starts_with("Usage:"), "{bad:?}");
        }
    }

    #[test]
    fn mode_conflicts_and_columns() {
        let f: &[(&str, &[u8])] = &[("small", b"abc")];
        for args in [
            &["-p", "-b"][..],
            &["-e", "-i"],
            &["-b", "-e"],
            &["-i", "-p"],
        ] {
            let r = xxd(args, b"", f);
            assert_eq!(
                r.stderr_str(),
                "xxd: only one of -b, -e, -u, -p, -i can be used\n",
                "{args:?}"
            );
            assert_eq!(r.code(), 1);
        }
        assert_eq!(xxd(&["-i", "-b", "small"], b"", f).code(), 0);
        let r = xxd(&["-c", "257", "small"], b"", f);
        assert_eq!(
            (r.code(), r.stderr_str().as_str()),
            (1, "xxd: invalid number of columns (max. 256).\n")
        );
        assert_eq!(xxd(&["-p", "-c", "300", "small"], b"", f).code(), 0);
        assert_eq!(
            xxd(&["-c", "0", "small"], b"", f).stdout_str(),
            "00000000: 6162 63                                  abc\n"
        );
    }

    #[test]
    fn seeks() {
        assert_eq!(
            out(&["-s", "-2", "txt"]),
            "00000035: 3839                                     89\n"
        );
        assert_eq!(out(&["-s", "100", "txt"]), "");
        let r = xxd(&["-s", "-100", "txt"], b"", &[("txt", TXT)]);
        assert_eq!(
            (r.code(), r.stderr_str().as_str()),
            (4, "xxd: Sorry, cannot seek.\n")
        );
        assert_eq!(
            xxd(&["-s", "+3"], b"hello world\n", &[]).stdout_str(),
            "00000003: 6c6f 2077 6f72 6c64 0a                   lo world.\n"
        );
        assert_eq!(
            xxd(&["-s", "+3", "-o", "100"], b"hello world\n", &[]).stdout_str(),
            "00000067: 6c6f 2077 6f72 6c64 0a                   lo world.\n"
        );
        let r = xxd(&["-s", "13"], b"hello world\n", &[]);
        assert_eq!(
            (r.code(), r.stderr_str().as_str()),
            (4, "xxd: Sorry, cannot seek.\n")
        );
        let r = xxd(&["-s", "-1"], b"hello world\n", &[]);
        assert_eq!(r.code(), 4);
        assert_eq!(xxd(&["-s", "12"], b"hello world\n", &[]).code(), 0);
    }

    #[test]
    fn file_errors() {
        let r = xxd(&["nofile"], b"", &[]);
        assert_eq!(
            (r.code(), r.stderr_str().as_str()),
            (2, "xxd: nofile: No such file or directory\n")
        );
        let k = kit(&[("in1", b"6162")]);
        k.put_dir(b"/work/dd", 0o755);
        let r = k.run(&["xxd", "in1", "dd"], b"");
        assert_eq!(
            (r.code(), r.stderr_str().as_str()),
            (3, "xxd: dd: Is a directory\n")
        );
        let r = k.run(&["xxd", "dd"], b"");
        assert_eq!(
            (r.code(), r.stderr_str().as_str()),
            (2, "xxd: Is a directory\n")
        );
        let r = k.run(&["xxd", "-r", "dd"], b"");
        assert_eq!((r.code(), r.stderr_str().as_str()), (0, ""));
        let r = k.run(&["xxd", "-i", "dd"], b"");
        assert_eq!(r.stdout_str(), "unsigned char dd[] = {\n");
        assert_eq!(r.code(), 2);
    }

    #[test]
    fn outfile_truncates_in_dump_mode() {
        let k = kit(&[
            ("in1", b"ab"),
            (
                "f3",
                b"XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX",
            ),
        ]);
        let r = k.run(&["xxd", "in1", "f3"], b"");
        assert_eq!(r.code(), 0);
        assert_eq!(
            k.read_file("/work/f3").unwrap(),
            b"00000000: 6162                                     ab\n"
        );
    }

    #[test]
    fn revert_normal() {
        let r = xxd(&["-r"], b"00000000: 6162 6364  abcd\n00000004: 6566\n", &[]);
        assert_eq!(r.stdout, b"abcdef");
        let r = xxd(&["-r"], b"0: 41 42\n5: 43\n", &[]);
        assert_eq!(r.stdout, b"AB\0\0\0C");
        let r = xxd(&["-r"], b"00000000: 616 263\n", &[]);
        assert_eq!(r.stdout, b"a&");
        let r = xxd(&["-r"], b"0: 41.42 . 43\n", &[]);
        assert_eq!(r.stdout, b"AB");
        let r = xxd(&["-r"], b"hello\n", &[]);
        assert_eq!(r.stdout, vec![0u8; 14]);
        let r = xxd(&["-r"], b"12\n34\n", &[]);
        let mut want = vec![0u8; 18];
        want.push(0x34);
        assert_eq!(r.stdout, want);
    }

    #[test]
    fn revert_columns_and_offsets() {
        let line = b"0: 4142434445464748494a4b4c4d4e4f505152\n";
        assert_eq!(xxd(&["-r"], line, &[]).stdout, b"ABCDEFGHIJKLMNOP");
        assert_eq!(xxd(&["-r", "-c", "4"], line, &[]).stdout, b"ABCD");
        assert_eq!(
            xxd(&["-r", "-c", "20"], line, &[]).stdout,
            b"ABCDEFGHIJKLMNOPQR"
        );
        assert_eq!(
            xxd(&["-r", "-s", "4"], b"00000000: 4142\n", &[]).stdout,
            b"\0\0\0\0AB"
        );
        assert_eq!(
            xxd(&["-r", "-s", "-4"], b"00000010: 4142\n", &[]).stdout,
            b"\0\0\0\0\0\0\0\0\0\0\0\0AB"
        );
        assert_eq!(xxd(&["-r"], b"10000000000000000: 41\n", &[]).stdout, b"A");
    }

    #[test]
    fn revert_backwards_on_pipe() {
        let r = xxd(&["-r"], b"3: 41\n1: 42\n", &[]);
        assert_eq!(r.stdout, b"\0\0\0A");
        assert_eq!(
            (r.code(), r.stderr_str().as_str()),
            (5, "xxd: Sorry, cannot seek backwards.\n")
        );
        let r = xxd(&["-r", "-s", "-4"], b"0: 4142\n", &[]);
        assert_eq!(r.code(), 5);
    }

    #[test]
    fn revert_patches_file_without_truncating() {
        let k = kit(&[("f1", b"XXXXXXXXXX")]);
        let r = k.run(&["xxd", "-r", "-", "f1"], b"3: 41\n1: 42\n");
        assert_eq!(r.code(), 0);
        assert_eq!(k.read_file("/work/f1").unwrap(), b"XBXAXXXXXX");
        let k = kit(&[("f4", b"XX")]);
        k.run(&["xxd", "-r", "-", "f4"], b"5: 41\n");
        assert_eq!(k.read_file("/work/f4").unwrap(), b"XX\0\0\0A");
        let k = kit(&[("f8", b"XX")]);
        k.run(&["xxd", "-r", "-", "f8"], b"hello\n");
        assert_eq!(k.read_file("/work/f8").unwrap(), b"XX");
    }

    #[test]
    fn revert_plain() {
        assert_eq!(
            xxd(&["-r", "-p"], b"61 62\n63\t64 65\n", &[]).stdout,
            b"abcde"
        );
        assert_eq!(xxd(&["-r", "-p"], b"616\n2\n", &[]).stdout, b"ab");
        assert_eq!(xxd(&["-r", "-p"], b"61zz62\n", &[]).stdout, b"a");
        assert_eq!(xxd(&["-r", "-p"], b"61zz\nz62\n", &[]).stdout, b"ab");
        assert_eq!(xxd(&["-r", "-p"], b"zz61\n", &[]).stdout, b"a");
        assert_eq!(xxd(&["-r", "-p"], b"6\x0b2\n", &[]).stdout, b"");
        assert_eq!(
            xxd(&["-r", "-p", "-s", "3"], b"6162\n", &[]).stdout,
            b"\0\0\0ab"
        );
    }

    #[test]
    fn revert_bits() {
        assert_eq!(
            xxd(&["-r", "-b"], b"00000000: 01100001 01100010  ab\n", &[]).stdout,
            b"ab"
        );
        assert_eq!(
            xxd(&["-r", "-b"], b"0: 0110x0001 01 10 00 10\n", &[]).stdout,
            b"ab"
        );
        assert_eq!(xxd(&["-r", "-b"], b"0: 41 01100001\n", &[]).stdout, [0xb0]);
        assert_eq!(xxd(&["-r", "-b"], b"0: 0100000\n0:1\n", &[]).stdout, b"");
        let long = b"0: 0110000101100010011000110110010001100101011001100110011101101000\n";
        assert_eq!(xxd(&["-r", "-b"], long, &[]).stdout, b"abcdef");
        assert_eq!(xxd(&["-r", "-b", "-c", "3"], long, &[]).stdout, b"abc");
    }

    #[test]
    fn revert_rejects_little_endian_and_include() {
        let r = xxd(&["-r", "-e", "small"], b"", &[("small", b"abc")]);
        assert_eq!(
            (r.code(), r.stderr_str().as_str()),
            (255, "xxd: Sorry, cannot revert this type of hexdump\n")
        );
        let r = xxd(&["-r", "-i", "nofile"], b"", &[]);
        assert_eq!(r.code(), 2);
    }

    #[test]
    fn color_always() {
        let r = xxd(&["-R", "always"], b"hello\n", &[]);
        let want = "00000000: \x1b[1;32m68\x1b[0m\x1b[1;32m65\x1b[0m \x1b[1;32m6c\x1b[0m\x1b[1;32m6c\x1b[0m \
                    \x1b[1;32m6f\x1b[0m\x1b[1;33m0a\x1b[0m                \x1b[1;31m \x1b[0m\x1b[1;31m \x1b[0m\
                    \x1b[1;31m \x1b[0m\x1b[1;31m \x1b[0m\x1b[1;31m \x1b[0m\x1b[1;31m \x1b[0m\x1b[1;31m \x1b[0m\
                    \x1b[1;31m \x1b[0m\x1b[1;31m \x1b[0m\x1b[1;31m \x1b[0m \x1b[1;32mh\x1b[0m\x1b[1;32me\x1b[0m\
                    \x1b[1;32ml\x1b[0m\x1b[1;32ml\x1b[0m\x1b[1;32mo\x1b[0m\x1b[1;33m.\x1b[0m\n";
        assert_eq!(r.stdout_str(), want);
    }

    /// Troca `ESC[1;3Xm...ESC[0m` por `<X...>` (R, G, Y, B, W) pra comparar com o tokenizado.
    fn tok(s: &str) -> String {
        let mut out = String::new();
        let mut rest = s;
        while let Some(i) = rest.find("\x1b[1;3") {
            out.push_str(&rest[..i].replace(' ', "_"));
            let code = &rest[i + 5..i + 6];
            let body_start = i + 7;
            let end = rest[body_start..].find("\x1b[0m").unwrap() + body_start;
            let name = match code {
                "1" => 'R',
                "2" => 'G',
                "3" => 'Y',
                "4" => 'B',
                _ => 'W',
            };
            out.push('<');
            out.push(name);
            out.push_str(&rest[body_start..end].replace(' ', "_"));
            out.push('>');
            rest = &rest[end + 4..];
        }
        out.push_str(&rest.replace(' ', "_"));
        out
    }

    #[test]
    fn color_partial_lines() {
        let c = |args: &[&str], input: &[u8]| {
            let mut a = vec!["-R", "always"];
            a.extend_from_slice(args);
            tok(&xxd(&a, input, &[]).stdout_str())
        };
        assert_eq!(
            c(&["-c", "8"], b"ABC"),
            "00000000:_<G41><G42>_<G43>_______<R_><R_><R_><R_><R_>__<GA><GB><GC>\n"
        );
        assert_eq!(
            c(&["-c", "8", "-g", "3"], b"ABCD"),
            "00000000:_<G41><G42><G43>_<G44>_____<R_><R_><R_><R_>__<GA><GB><GC><GD>\n"
        );
        assert_eq!(
            c(&["-c", "8", "-e"], b"A"),
            "00000000:_<R_><R_><R_>___<G41>______<R_><R_><R_><R_>_<GA>\n"
        );
        assert_eq!(
            c(&["-c", "8", "-e"], b"ABCDE"),
            "00000000:_<G44><G43><G42><G41>_<R_><R_><R_>___<G45>__<GA><GB><GC><GD><GE>\n"
        );
        assert_eq!(
            c(&["-c", "4", "-e", "-g", "2"], b"ABC"),
            "00000000:_<G42><G41>_<R_>_<G43>__<GA><GB><GC>\n"
        );
        assert_eq!(
            c(&["-b", "-c", "4"], b"AB"),
            "00000000:_01000001_01000010____________________<GA><GB>\n"
        );
        assert_eq!(
            c(&["-c", "6"], b"\t\n\r\0\xff "),
            "00000000:_<Y09><Y0a>_<Y0d><W00>_<Bff><G20>__<Y.><Y.><Y.><W.><B.><G_>\n"
        );
        assert_eq!(
            c(&["-E"], b"ABC"),
            "00000000:_<R41><R42>_<R43>___________________<R_><R_><R_><R_><R_><R_><R_><R_><R_><R_><R_><R_><R_>__<R.><R.><R.>\n"
        );
        assert_eq!(c(&["-p"], b"ABC"), "414243\n");
        assert_eq!(
            c(&["-d", "-o", "5"], b"ABC"),
            "00000005:_<G41><G42>_<G43>___________________<R_><R_><R_><R_><R_><R_><R_><R_><R_><R_><R_><R_><R_>__<GA><GB><GC>\n"
        );
    }

    #[test]
    fn color_never_and_default() {
        assert_eq!(
            xxd(&["-R", "never"], b"AB", &[]).stdout_str(),
            "00000000: 4142                                     AB\n"
        );
        assert_eq!(
            xxd(&["-R", "auto"], b"AB", &[]).stdout_str(),
            "00000000: 4142                                     AB\n"
        );
        assert_eq!(
            xxd(&[], b"AB", &[]).stdout_str(),
            "00000000: 4142                                     AB\n"
        );
    }

    #[test]
    fn repeated_options_last_wins() {
        let f: &[(&str, &[u8])] = &[("small", b"abc")];
        assert_eq!(
            xxd(&["-c", "3", "-c", "4", "small"], b"", f).stdout_str(),
            "00000000: 6162 63    abc\n"
        );
        assert_eq!(
            xxd(&["-n", "a", "-n", "b", "-i", "small"], b"", f).stdout_str(),
            "unsigned char b[] = {\n  0x61, 0x62, 0x63\n};\nunsigned int b_len = 3;\n"
        );
        assert_eq!(
            xxd(&["-s", "1", "-l", "1", "-o", "1", "-d", "small"], b"", f).stdout_str(),
            "00000002: 62                                       b\n"
        );
    }

    #[test]
    fn empty_input() {
        assert_eq!(xxd(&[], b"", &[]).stdout, b"");
        assert_eq!(xxd(&["-p"], b"", &[]).stdout, b"");
        assert_eq!(xxd(&["-l", "0"], b"abc", &[]).stdout, b"");
    }

    #[test]
    fn stdin_and_dash() {
        assert_eq!(
            xxd(&["-"], b"hi", &[]).stdout_str(),
            "00000000: 6869                                     hi\n"
        );
        assert_eq!(
            xxd(&["--", "-"], b"hi", &[]).stdout_str(),
            "00000000: 6869                                     hi\n"
        );
    }
}
