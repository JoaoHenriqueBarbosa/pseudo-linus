//! Utilitários externos mínimos, fiéis ao GNU coreutils 9.7 (Debian 13, `LC_ALL=C.UTF-8`), para os
//! testes de conformidade do shell no kernel de teste do `sysabi` (feature `testkit`).
//!
//! Os programas de verdade são feitos por outros crates; estes existem só para o shell ter com quem
//! conversar enquanto isso. Cada um cobre as opções que o corpus `testbench/corpus/cases/shell` usa e
//! as vizinhas óbvias, com saída, mensagens de erro e status iguais aos do GNU byte a byte. Tudo passa
//! pelas syscalls do `sysabi` (nada de `std::fs`, `std::process`, `std::env` ou `print!`), e todo
//! texto é tratado como bytes.
//!
//! Programas (todos em `/usr/bin`): basename, cat, chmod, cut, dirname, env, false, head, ls, mkdir,
//! od, paste, readlink, rm, seq, sleep, sort, tail, tee, timeout, touch, tr, true, uniq, wc, yes.
//!
//! Limites conhecidos (opções fora daqui respondem com `unsupported option '...' (shell test
//! miniutils)` em vez de fingir que funcionam):
//!
//! - `--help` e `--version` não imprimem os textos do GNU.
//! - `timeout DURAÇÃO comando`: o `TestKit` é síncrono, o `spawn` só volta depois que o filho
//!   termina, então nada consegue interromper o filho no meio. O caso `timeout 0.1 sleep 5` (que
//!   espera status 124) **só passa no kernel real**; no testkit o `sleep` dorme os 5 s do relógio
//!   simulado e o `timeout` devolve o status dele (0). A implementação já é a do kernel real: depois
//!   do `spawn`, laço de `wait4(NOHANG)` com `nanosleep` curto, conferindo o prazo com
//!   `clock_gettime(Monotonic)`; vencido o prazo, manda o sinal (SIGTERM por padrão) ao filho e ao
//!   grupo, como o GNU, e devolve 124.
//! - `readlink -f /dev/stdin` depende do procfs (`/proc/self/fd/0`), que o testkit não tem; lá ele
//!   falha com ENOENT, no kernel real resolve como o GNU.
//! - `ls` sempre lista um nome por linha (o formato de colunas do terminal e o `-l` ficaram de fora).
//! - `touch -t` e `touch -d` só sabem converter datas em UTC (a bancada roda com `TZ=UTC`).

#![allow(dead_code)]

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::time::Duration;

use sysabi::sys::{self, SysResult};
use sysabi::{
    AccessMode, AtFlags, Clock, Ctx, Errno, Fd, FileType, KillTarget, Mode, OFlags, ProcAttrs, Program, SetTime,
    SigDisposition, Signal, SpawnSpec, Stat, TimeSpec, WaitOptions, WaitStatus, WaitTarget, Whence, mode,
};

/// Todos os programas, para registrar no `TestKit` (ou no kernel).
pub fn programs() -> Vec<Program> {
    vec![
        Program::bin("basename", |c: &mut Ctx, a: &[OsString]| entry(c, a, basename)),
        Program::bin("cat", |c: &mut Ctx, a: &[OsString]| entry(c, a, cat)),
        Program::bin("chmod", |c: &mut Ctx, a: &[OsString]| entry(c, a, chmod)),
        Program::bin("cut", |c: &mut Ctx, a: &[OsString]| entry(c, a, cut)),
        Program::bin("dirname", |c: &mut Ctx, a: &[OsString]| entry(c, a, dirname)),
        Program::bin("env", |c: &mut Ctx, a: &[OsString]| entry(c, a, env)),
        Program::bin("false", |c: &mut Ctx, a: &[OsString]| entry(c, a, false_)),
        Program::bin("head", |c: &mut Ctx, a: &[OsString]| entry(c, a, head)),
        Program::bin("ls", |c: &mut Ctx, a: &[OsString]| entry(c, a, ls)),
        Program::bin("mkdir", |c: &mut Ctx, a: &[OsString]| entry(c, a, mkdir)),
        Program::bin("od", |c: &mut Ctx, a: &[OsString]| entry(c, a, od)),
        Program::bin("paste", |c: &mut Ctx, a: &[OsString]| entry(c, a, paste)),
        Program::bin("readlink", |c: &mut Ctx, a: &[OsString]| entry(c, a, readlink)),
        Program::bin("rm", |c: &mut Ctx, a: &[OsString]| entry(c, a, rm)),
        Program::bin("seq", |c: &mut Ctx, a: &[OsString]| entry(c, a, seq)),
        Program::bin("sleep", |c: &mut Ctx, a: &[OsString]| entry(c, a, sleep)),
        Program::bin("sort", |c: &mut Ctx, a: &[OsString]| entry(c, a, sort)),
        Program::bin("tail", |c: &mut Ctx, a: &[OsString]| entry(c, a, tail)),
        Program::bin("tee", |c: &mut Ctx, a: &[OsString]| entry(c, a, tee)),
        Program::bin("timeout", |c: &mut Ctx, a: &[OsString]| entry(c, a, timeout)),
        Program::bin("touch", |c: &mut Ctx, a: &[OsString]| entry(c, a, touch)),
        Program::bin("tr", |c: &mut Ctx, a: &[OsString]| entry(c, a, tr)),
        Program::bin("true", |c: &mut Ctx, a: &[OsString]| entry(c, a, true_)),
        Program::bin("uniq", |c: &mut Ctx, a: &[OsString]| entry(c, a, uniq)),
        Program::bin("wc", |c: &mut Ctx, a: &[OsString]| entry(c, a, wc)),
        Program::bin("yes", |c: &mut Ctx, a: &[OsString]| entry(c, a, yes)),
    ]
}

// =====================================================================================================
// Infraestrutura comum
// =====================================================================================================

/// Argumentos depois do argv[0], em bytes.
type Argv = [Vec<u8>];

fn entry(ctx: &mut Ctx, args: &[OsString], run: fn(&Tool, &Argv) -> i32) -> i32 {
    let tool = Tool::new(ctx, args);
    let rest: Vec<Vec<u8>> = args.iter().skip(1).map(|a| a.as_bytes().to_vec()).collect();
    run(&tool, &rest)
}

/// Junta pedaços de bytes.
fn bv(parts: &[&[u8]]) -> Vec<u8> {
    let mut v = Vec::with_capacity(parts.iter().map(|p| p.len()).sum());
    for p in parts {
        v.extend_from_slice(p);
    }
    v
}

fn num(n: impl std::fmt::Display) -> Vec<u8> {
    n.to_string().into_bytes()
}

fn stderr(b: &[u8]) {
    let _ = sys::write_all(Fd::STDERR, b);
}

/// Identidade do programa para as mensagens.
struct Tool {
    /// Basename do argv[0]: é o que o `error()` das coreutils imprime.
    name: Vec<u8>,
    /// argv[0] inteiro: é o que o getopt da glibc e o "Try '... --help'" imprimem.
    argv0: Vec<u8>,
}

impl Tool {
    fn new(ctx: &Ctx, args: &[OsString]) -> Tool {
        let argv0 = args.first().map(|a| a.as_bytes().to_vec()).unwrap_or_else(|| ctx.prog.clone().into_bytes());
        Tool { name: ctx.prog.clone().into_bytes(), argv0 }
    }

    /// `prog: msg`, como o `error(0, 0, ...)`.
    fn error(&self, msg: &[u8]) {
        stderr(&bv(&[&self.name, b": ", msg, b"\n"]));
    }

    /// `prog: msg: strerror`, como o `error(0, errno, ...)`.
    fn error_errno(&self, msg: &[u8], e: Errno) {
        self.error(&bv(&[msg, b": ", e.message().as_bytes()]));
    }

    fn try_help(&self) {
        stderr(&bv(&[b"Try '", &self.argv0, b" --help' for more information.\n"]));
    }

    /// Erro de uso: mensagem, "Try ...", e o status pedido.
    fn usage_error(&self, msg: &[u8], code: i32) -> i32 {
        self.error(msg);
        self.try_help();
        code
    }

    /// Opção válida no GNU que estes utilitários não implementam: avisa em vez de fingir.
    fn unsupported(&self, what: &[u8], code: i32) -> i32 {
        self.error(&bv(&[b"unsupported option '", what, b"' (shell test miniutils)"]));
        code
    }
}

// ---- quoting (gnulib quotearg) ----------------------------------------------------------------------

/// Resultado de decodificar UTF-8 numa posição.
enum Utf8 {
    Char(u32, usize),
    Invalid,
    /// Prefixo válido cortado pelo fim do buffer.
    Incomplete,
}

fn utf8_decode(s: &[u8], i: usize) -> Utf8 {
    let b0 = s[i];
    if b0 < 0x80 {
        return Utf8::Char(b0 as u32, 1);
    }
    let (len, min, init) = match b0 {
        0xC2..=0xDF => (2, 0x80, (b0 & 0x1F) as u32),
        0xE0..=0xEF => (3, 0x800, (b0 & 0x0F) as u32),
        0xF0..=0xF4 => (4, 0x10000, (b0 & 0x07) as u32),
        _ => return Utf8::Invalid,
    };
    let mut cp = init;
    for k in 1..len {
        let Some(&b) = s.get(i + k) else { return Utf8::Incomplete };
        if b & 0xC0 != 0x80 {
            return Utf8::Invalid;
        }
        cp = (cp << 6) | (b & 0x3F) as u32;
        // Rejeita cedo os prefixos que nunca formam um caractere válido.
        if k == 1 {
            let bad = match b0 {
                0xE0 => b < 0xA0,
                0xED => b > 0x9F,
                0xF0 => b < 0x90,
                0xF4 => b > 0x8F,
                _ => false,
            };
            if bad {
                return Utf8::Invalid;
            }
        }
    }
    if cp < min || cp > 0x10FFFF || (0xD800..=0xDFFF).contains(&cp) {
        return Utf8::Invalid;
    }
    Utf8::Char(cp, len)
}

/// `iswprint` aproximado do C.UTF-8 da glibc.
fn is_print(cp: u32) -> bool {
    !(cp < 0x20
        || (0x7F..=0x9F).contains(&cp)
        || cp == 0x2028
        || cp == 0x2029
        || (0xFDD0..=0xFDEF).contains(&cp)
        || cp & 0xFFFE == 0xFFFE
        || (0xE000..=0xF8FF).contains(&cp)
        || cp >= 0xF0000)
}

fn octal_escape(b: u8, out: &mut Vec<u8>) {
    out.push(b'\\');
    out.push(b'0' + (b >> 6));
    out.push(b'0' + ((b >> 3) & 7));
    out.push(b'0' + (b & 7));
}

/// Escape à moda do C para um byte de controle.
fn c_escape(b: u8, out: &mut Vec<u8>) {
    let named = match b {
        7 => Some(b'a'),
        8 => Some(b'b'),
        12 => Some(b'f'),
        10 => Some(b'n'),
        13 => Some(b'r'),
        9 => Some(b't'),
        11 => Some(b'v'),
        _ => None,
    };
    match named {
        Some(c) => out.extend_from_slice(&[b'\\', c]),
        None => octal_escape(b, out),
    }
}

/// `quote()` das coreutils no C.UTF-8: aspas tipográficas e escapes do C.
fn quote(s: &[u8]) -> Vec<u8> {
    let mut out = "\u{2018}".as_bytes().to_vec();
    let mut i = 0;
    while i < s.len() {
        match utf8_decode(s, i) {
            Utf8::Char(cp, n) if is_print(cp) => {
                if cp == u32::from(b'\\') {
                    out.extend_from_slice(b"\\\\");
                } else {
                    out.extend_from_slice(&s[i..i + n]);
                }
                i += n;
            }
            Utf8::Char(cp, 1) => {
                c_escape(cp as u8, &mut out);
                i += 1;
            }
            Utf8::Char(_, n) => {
                for &b in &s[i..i + n] {
                    octal_escape(b, &mut out);
                }
                i += n;
            }
            Utf8::Invalid | Utf8::Incomplete => {
                octal_escape(s[i], &mut out);
                i += 1;
            }
        }
    }
    out.extend_from_slice("\u{2019}".as_bytes());
    out
}

/// Estilos `shell-escape` da gnulib: `always` sempre põe aspas (o `quoteaf`); `colon` também protege
/// `:` (o `quotef`).
fn quote_shell(s: &[u8], always: bool, colon: bool) -> Vec<u8> {
    if s.is_empty() {
        return b"''".to_vec();
    }
    let mut needs = always;
    let mut single_quote = false;
    // Todos os caracteres que pediram aspas também cabem entre aspas duplas?
    let mut dq_compatible = true;
    let mut i = 0;
    while i < s.len() {
        match utf8_decode(s, i) {
            Utf8::Char(cp, n) if n > 1 => {
                if !is_print(cp) {
                    needs = true;
                    dq_compatible = false;
                }
                i += n;
                continue;
            }
            Utf8::Invalid | Utf8::Incomplete => {
                needs = true;
                dq_compatible = false;
                i += 1;
                continue;
            }
            Utf8::Char(..) => {}
        }
        match s[i] {
            b'\'' => {
                needs = true;
                single_quote = true;
            }
            b' ' => needs = true,
            b'#' | b'~' if i == 0 => needs = true,
            b'{' | b'}' if s.len() == 1 => needs = true,
            b'!' | b'"' | b'$' | b'&' | b'(' | b')' | b'*' | b';' | b'<' | b'=' | b'>' | b'[' | b'^' | b'`'
            | b'|' | b'?' | b'\\' => {
                needs = true;
                dq_compatible = false;
            }
            b':' if colon => needs = true,
            0..=0x1F | 0x7F => {
                needs = true;
                dq_compatible = false;
            }
            _ => {}
        }
        i += 1;
    }
    if !needs {
        return s.to_vec();
    }
    if single_quote && dq_compatible {
        return bv(&[b"\"", s, b"\""]);
    }
    let mut out = vec![b'\''];
    let mut in_escape = false;
    let mut i = 0;
    while i < s.len() {
        let (printable, n) = match utf8_decode(s, i) {
            Utf8::Char(cp, n) => (is_print(cp), n),
            _ => (false, 1),
        };
        if printable {
            if in_escape {
                out.extend_from_slice(b"''");
                in_escape = false;
            }
            if s[i] == b'\'' {
                out.extend_from_slice(b"'\\''");
            } else {
                out.extend_from_slice(&s[i..i + n]);
            }
        } else {
            if !in_escape {
                out.extend_from_slice(b"'$'");
                in_escape = true;
            }
            for &b in &s[i..i + n] {
                c_escape(b, &mut out);
            }
        }
        i += n;
    }
    out.push(b'\'');
    out
}

/// `quotef`: aspas só quando precisa (nomes de arquivo em mensagens no formato "prog: nome: erro").
fn quotef(s: &[u8]) -> Vec<u8> {
    quote_shell(s, false, true)
}

/// `quoteaf`: sempre entre aspas.
fn quoteaf(s: &[u8]) -> Vec<u8> {
    quote_shell(s, true, false)
}

// ---- saída com buffer (stdio) -----------------------------------------------------------------------

/// Saída padrão com buffer como o stdio da glibc quando o stdout não é terminal (4096 bytes, o
/// `st_blksize` de pipes e do tmpfs). Erro de escrita: mensagem e `exit(1)`; num pipe fechado o
/// kernel entrega SIGPIPE antes disso.
struct Out {
    fd: Fd,
    buf: Vec<u8>,
    cap: usize,
    prog: Vec<u8>,
    what: &'static [u8],
}

impl Out {
    fn new(t: &Tool) -> Out {
        Out { fd: Fd::STDOUT, buf: Vec::with_capacity(4096), cap: 4096, prog: t.name.clone(), what: b"write error" }
    }

    fn put(&mut self, data: &[u8]) {
        if self.buf.len() + data.len() < self.cap {
            self.buf.extend_from_slice(data);
            return;
        }
        let room = self.cap - self.buf.len();
        self.buf.extend_from_slice(&data[..room]);
        self.flush();
        let mut rest = &data[room..];
        let direct = rest.len() - rest.len() % self.cap;
        if direct > 0 {
            self.raw(&rest[..direct]);
            rest = &rest[direct..];
        }
        self.buf.extend_from_slice(rest);
    }

    fn byte(&mut self, b: u8) {
        self.buf.push(b);
        if self.buf.len() >= self.cap {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if !self.buf.is_empty() {
            let data = std::mem::take(&mut self.buf);
            self.raw(&data);
            self.buf = data;
            self.buf.clear();
        }
    }

    fn raw(&self, data: &[u8]) {
        if let Err(e) = sys::write_all(self.fd, data) {
            stderr(&bv(&[&self.prog, b": ", self.what, b": ", e.message().as_bytes(), b"\n"]));
            sys::exit(1);
        }
    }

    /// Esvazia o buffer e devolve o status (o `close_stdout` do fim do `main`).
    fn finish(&mut self, code: i32) -> i32 {
        self.flush();
        code
    }
}

// ---- entrada com buffer -----------------------------------------------------------------------------

struct Input {
    fd: Fd,
    owned: bool,
    buf: Vec<u8>,
    pos: usize,
    end: usize,
    eof: bool,
}

const IN_BUF: usize = 64 * 1024;

impl Input {
    fn from_fd(fd: Fd, owned: bool) -> Input {
        Input { fd, owned, buf: vec![0; IN_BUF], pos: 0, end: 0, eof: false }
    }

    fn stdin() -> Input {
        Input::from_fd(Fd::STDIN, false)
    }

    /// Abre para leitura; "-" é a entrada padrão.
    fn open(path: &[u8]) -> SysResult<Input> {
        if path == b"-" {
            return Ok(Input::stdin());
        }
        let fd = sys::open(path, OFlags::RDONLY | OFlags::CLOEXEC, 0)?;
        Ok(Input::from_fd(fd, true))
    }

    /// Garante dados no buffer; `false` no fim.
    fn fill(&mut self) -> SysResult<bool> {
        if self.pos < self.end {
            return Ok(true);
        }
        if self.eof {
            return Ok(false);
        }
        loop {
            match sys::read(self.fd, &mut self.buf) {
                Ok(0) => {
                    self.eof = true;
                    return Ok(false);
                }
                Ok(n) => {
                    self.pos = 0;
                    self.end = n;
                    return Ok(true);
                }
                Err(Errno::EINTR) => {}
                Err(e) => return Err(e),
            }
        }
    }

    /// O próximo pedaço disponível (vazio no fim).
    fn chunk(&mut self) -> SysResult<&[u8]> {
        if !self.fill()? {
            return Ok(&[]);
        }
        let (p, e) = (self.pos, self.end);
        self.pos = e;
        Ok(&self.buf[p..e])
    }

    fn getc(&mut self) -> SysResult<Option<u8>> {
        if !self.fill()? {
            return Ok(None);
        }
        let b = self.buf[self.pos];
        self.pos += 1;
        Ok(Some(b))
    }

    /// Acrescenta a `line` um registro terminado por `delim` (com o terminador, se houver).
    /// `false` quando não havia nada.
    fn read_record(&mut self, delim: u8, line: &mut Vec<u8>) -> SysResult<bool> {
        let mut got = false;
        while self.fill()? {
            let avail = &self.buf[self.pos..self.end];
            got = true;
            if let Some(k) = avail.iter().position(|&b| b == delim) {
                line.extend_from_slice(&avail[..=k]);
                self.pos += k + 1;
                return Ok(true);
            }
            line.extend_from_slice(avail);
            self.pos = self.end;
        }
        Ok(got)
    }

    fn read_all(&mut self) -> SysResult<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            let c = self.chunk()?;
            if c.is_empty() {
                return Ok(out);
            }
            out.extend_from_slice(c);
        }
    }

    /// Bytes lidos do fd e ainda não consumidos (para o `head` devolver o ponteiro do arquivo).
    fn unconsumed(&self) -> usize {
        self.end - self.pos
    }

    fn close(self) {
        if self.owned {
            let _ = sys::close(self.fd);
        }
    }
}

// ---- getopt_long da glibc ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HasArg {
    No,
    Req,
    Opt,
}

#[derive(Clone, Copy, Debug)]
struct LongOpt {
    name: &'static str,
    has_arg: HasArg,
    key: u32,
}

const fn long(name: &'static str, has_arg: HasArg, key: u32) -> LongOpt {
    LongOpt { name, has_arg, key }
}

const KEY_HELP: u32 = 0x1_0000;
const KEY_VERSION: u32 = 0x1_0001;

enum Got {
    Opt(u32, Option<Vec<u8>>),
    End,
    /// O getopt já imprimiu a mensagem; falta o "Try ..." e o status de cada programa.
    Fail,
}

/// `getopt_long` com permutação (a menos que a especificação comece com `+` ou exista
/// `POSIXLY_CORRECT`), abreviação de opções longas e as mensagens exatas da glibc.
struct Getopt<'a> {
    tool: &'a Tool,
    args: &'a [Vec<u8>],
    shorts: &'static [u8],
    longs: Vec<LongOpt>,
    permute: bool,
    /// Para o laço de opções antes de um argumento (o `seq` faz isso com números negativos).
    stop_at: Option<fn(&[u8]) -> bool>,
    idx: usize,
    sub: usize,
    operands: Vec<Vec<u8>>,
    finished: bool,
    /// Índice do argumento que produziu a última opção (o `argv[optind - 1]`).
    last_arg: usize,
}

impl<'a> Getopt<'a> {
    fn new(tool: &'a Tool, args: &'a [Vec<u8>], shorts: &'static [u8], longs: &[LongOpt]) -> Getopt<'a> {
        let permute = !shorts.starts_with(b"+") && sys::getenv("POSIXLY_CORRECT").is_none();
        let shorts = shorts.strip_prefix(b"+").unwrap_or(shorts);
        let mut all = longs.to_vec();
        all.push(long("help", HasArg::No, KEY_HELP));
        all.push(long("version", HasArg::No, KEY_VERSION));
        Getopt { tool, args, shorts, longs: all, permute, stop_at: None, idx: 0, sub: 0, operands: Vec::new(), finished: false, last_arg: 0 }
    }

    fn fail(&self, msg: &[u8]) -> Got {
        stderr(&bv(&[&self.tool.argv0, b": ", msg, b"\n"]));
        Got::Fail
    }

    fn finish_rest(&mut self) {
        self.operands.extend(self.args[self.idx..].iter().cloned());
        self.idx = self.args.len();
        self.finished = true;
    }

    fn next(&mut self) -> Got {
        if self.finished {
            return Got::End;
        }
        if self.sub > 0 {
            return self.short();
        }
        loop {
            let args = self.args;
            let Some(arg) = args.get(self.idx) else {
                self.finished = true;
                return Got::End;
            };
            if let Some(stop) = self.stop_at
                && stop(arg)
            {
                self.finish_rest();
                return Got::End;
            }
            if arg == b"--" {
                self.idx += 1;
                self.finish_rest();
                return Got::End;
            }
            if arg.len() > 2 && arg.starts_with(b"--") {
                return self.long();
            }
            if arg.len() > 1 && arg[0] == b'-' {
                self.sub = 1;
                return self.short();
            }
            if self.permute {
                self.operands.push(arg.clone());
                self.idx += 1;
                continue;
            }
            self.finish_rest();
            return Got::End;
        }
    }

    fn short(&mut self) -> Got {
        let args = self.args;
        let arg = &args[self.idx];
        let c = arg[self.sub];
        self.sub += 1;
        self.last_arg = self.idx;
        let at_end = self.sub >= arg.len();
        let pos = if c == b':' { None } else { self.shorts.iter().position(|&s| s == c) };
        let Some(pos) = pos else {
            if at_end {
                self.sub = 0;
                self.idx += 1;
            }
            return self.fail(&bv(&[b"invalid option -- '", &[c], b"'"]));
        };
        let kind = match (self.shorts.get(pos + 1), self.shorts.get(pos + 2)) {
            (Some(b':'), Some(b':')) => HasArg::Opt,
            (Some(b':'), _) => HasArg::Req,
            _ => HasArg::No,
        };
        match kind {
            HasArg::No => {
                if at_end {
                    self.sub = 0;
                    self.idx += 1;
                }
                Got::Opt(u32::from(c), None)
            }
            HasArg::Opt => {
                let v = if at_end { None } else { Some(arg[self.sub..].to_vec()) };
                self.sub = 0;
                self.idx += 1;
                Got::Opt(u32::from(c), v)
            }
            HasArg::Req => {
                if !at_end {
                    let v = arg[self.sub..].to_vec();
                    self.sub = 0;
                    self.idx += 1;
                    return Got::Opt(u32::from(c), Some(v));
                }
                self.sub = 0;
                self.idx += 1;
                match args.get(self.idx) {
                    Some(v) => {
                        self.idx += 1;
                        Got::Opt(u32::from(c), Some(v.clone()))
                    }
                    None => self.fail(&bv(&[b"option requires an argument -- '", &[c], b"'"])),
                }
            }
        }
    }

    fn long(&mut self) -> Got {
        let args = self.args;
        let arg = &args[self.idx];
        self.last_arg = self.idx;
        self.idx += 1;
        let body = &arg[2..];
        let (name, value) = match body.iter().position(|&b| b == b'=') {
            Some(p) => (&body[..p], Some(body[p + 1..].to_vec())),
            None => (body, None),
        };
        let found = match self.longs.iter().find(|o| o.name.as_bytes() == name) {
            Some(o) => *o,
            None => {
                let cands: Vec<LongOpt> = self.longs.iter().filter(|o| o.name.as_bytes().starts_with(name)).copied().collect();
                if cands.is_empty() {
                    return self.fail(&bv(&[b"unrecognized option '--", body, b"'"]));
                }
                if cands.iter().any(|o| o.key != cands[0].key || o.has_arg != cands[0].has_arg) {
                    let mut msg = bv(&[b"option '--", body, b"' is ambiguous; possibilities:"]);
                    for o in &cands {
                        msg.extend_from_slice(b" '--");
                        msg.extend_from_slice(o.name.as_bytes());
                        msg.push(b'\'');
                    }
                    return self.fail(&msg);
                }
                cands[0]
            }
        };
        match found.has_arg {
            HasArg::No if value.is_some() => {
                self.fail(&bv(&[b"option '--", found.name.as_bytes(), b"' doesn't allow an argument"]))
            }
            HasArg::No | HasArg::Opt => Got::Opt(found.key, value),
            HasArg::Req => {
                if value.is_some() {
                    return Got::Opt(found.key, value);
                }
                match args.get(self.idx) {
                    Some(v) => {
                        self.idx += 1;
                        Got::Opt(found.key, Some(v.clone()))
                    }
                    None => self.fail(&bv(&[b"option '--", found.name.as_bytes(), b"' requires an argument"])),
                }
            }
        }
    }

    /// Os operandos (na ordem original), depois que `next` devolveu `End`.
    fn operands(mut self) -> Vec<Vec<u8>> {
        if !self.finished {
            self.finish_rest();
        }
        self.operands
    }
}

/// Resposta comum a `--help` e `--version`.
fn help_or_version(t: &Tool, key: u32, code: i32) -> i32 {
    t.unsupported(if key == KEY_HELP { b"--help" } else { b"--version" }, code)
}

// ---- números ----------------------------------------------------------------------------------------

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

fn is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

#[derive(Debug, PartialEq, Eq)]
enum NumErr {
    Invalid,
    Overflow,
}

/// `xstrtoumax` da gnulib: base 10 (ou 0, automática como no `strtoul`), sinal `+` opcional, e os
/// sufixos multiplicadores listados em `valid` (o `0` em `valid` liga os segundos sufixos `B`/`iB`).
fn xstrtoumax(s: &[u8], base: u32, valid: &[u8]) -> Result<u64, NumErr> {
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    if s.get(i) == Some(&b'-') {
        return Err(NumErr::Invalid);
    }
    if s.get(i) == Some(&b'+') {
        i += 1;
    }
    let mut base = base;
    if base == 0 {
        if s.get(i) == Some(&b'0') && matches!(s.get(i + 1), Some(b'x' | b'X')) && s.get(i + 2).is_some_and(|b| b.is_ascii_hexdigit()) {
            base = 16;
            i += 2;
        } else if s.get(i) == Some(&b'0') {
            base = 8;
        } else {
            base = 10;
        }
    }
    let start = i;
    let mut value: u64 = 0;
    let mut overflow = false;
    while let Some(d) = s.get(i).and_then(|&b| (b as char).to_digit(base)) {
        match value.checked_mul(u64::from(base)).and_then(|v| v.checked_add(u64::from(d))) {
            Some(v) => value = v,
            None => overflow = true,
        }
        i += 1;
    }
    if i == start {
        if s.get(i).is_some_and(|b| valid.contains(b)) {
            value = 1;
        } else {
            return Err(NumErr::Invalid);
        }
    }
    if i < s.len() {
        let c = s[i];
        if !valid.contains(&c) {
            return Err(NumErr::Invalid);
        }
        let mut power_base: u64 = 1024;
        let mut used = 1;
        if valid.contains(&b'0') && matches!(c, b'E' | b'G' | b'g' | b'k' | b'K' | b'M' | b'm' | b'P' | b'Q' | b'R' | b'T' | b't' | b'Y' | b'Z') {
            match (s.get(i + 1), s.get(i + 2)) {
                (Some(b'i'), Some(b'B')) => used += 2,
                (Some(b'B' | b'D'), _) => {
                    power_base = 1000;
                    used += 1;
                }
                _ => {}
            }
        }
        let mult: Option<u64> = match c {
            b'b' => Some(512),
            b'B' => Some(1024),
            b'c' => Some(1),
            b'w' => Some(2),
            b'k' | b'K' => power_base.checked_pow(1),
            b'M' | b'm' => power_base.checked_pow(2),
            b'G' | b'g' => power_base.checked_pow(3),
            b'T' | b't' => power_base.checked_pow(4),
            b'P' => power_base.checked_pow(5),
            b'E' => power_base.checked_pow(6),
            b'Z' => power_base.checked_pow(7),
            b'Y' => power_base.checked_pow(8),
            b'R' => power_base.checked_pow(9),
            b'Q' => power_base.checked_pow(10),
            _ => return Err(NumErr::Invalid),
        };
        match mult.and_then(|m| value.checked_mul(m)) {
            Some(v) => value = v,
            None => overflow = true,
        }
        if i + used < s.len() {
            return Err(NumErr::Invalid);
        }
    }
    if overflow { Err(NumErr::Overflow) } else { Ok(value) }
}

/// Prefixo numérico à moda do `strtod` no locale C: devolve o valor e quantos bytes foram consumidos,
/// ou `None` se não há número.
fn strtod_prefix(s: &[u8]) -> Option<(f64, usize)> {
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let mut neg = false;
    if let Some(&c @ (b'+' | b'-')) = s.get(i) {
        neg = c == b'-';
        i += 1;
    }
    let rest = &s[i..];
    let lower = |n: usize| -> Vec<u8> { rest.iter().take(n).map(|b| b.to_ascii_lowercase()).collect() };
    let sign = |v: f64| if neg { -v } else { v };
    if lower(8) == b"infinity" {
        return Some((sign(f64::INFINITY), i + 8));
    }
    if lower(3) == b"inf" {
        return Some((sign(f64::INFINITY), i + 3));
    }
    if lower(3) == b"nan" {
        let mut j = i + 3;
        if s.get(j) == Some(&b'(') {
            let mut k = j + 1;
            while s.get(k).is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_') {
                k += 1;
            }
            if s.get(k) == Some(&b')') {
                j = k + 1;
            }
        }
        return Some((f64::NAN, j));
    }
    let hex = rest.len() > 2
        && rest[0] == b'0'
        && matches!(rest[1], b'x' | b'X')
        && (rest[2].is_ascii_hexdigit() || (rest[2] == b'.' && rest.get(3).is_some_and(|b| b.is_ascii_hexdigit())));
    if hex {
        let mut j = i + 2;
        let mut mant = 0f64;
        let mut exp: i64 = 0;
        while let Some(d) = s.get(j).and_then(|&b| (b as char).to_digit(16)) {
            mant = mant * 16.0 + f64::from(d);
            j += 1;
        }
        if s.get(j) == Some(&b'.') {
            j += 1;
            while let Some(d) = s.get(j).and_then(|&b| (b as char).to_digit(16)) {
                mant = mant * 16.0 + f64::from(d);
                exp -= 4;
                j += 1;
            }
        }
        if matches!(s.get(j), Some(b'p' | b'P')) {
            let mut k = j + 1;
            let mut eneg = false;
            if let Some(&c @ (b'+' | b'-')) = s.get(k) {
                eneg = c == b'-';
                k += 1;
            }
            if s.get(k).is_some_and(u8::is_ascii_digit) {
                let mut e: i64 = 0;
                while let Some(&d) = s.get(k).filter(|b| b.is_ascii_digit()) {
                    e = (e * 10 + i64::from(d - b'0')).min(100_000);
                    k += 1;
                }
                exp += if eneg { -e } else { e };
                j = k;
            }
        }
        let v = mant * 2f64.powi(exp.clamp(-5000, 5000) as i32);
        return Some((sign(v), j));
    }
    let mut j = i;
    let mut text = String::new();
    while s.get(j).is_some_and(u8::is_ascii_digit) {
        text.push(s[j] as char);
        j += 1;
    }
    let int_digits = text.len();
    let mut frac_digits = 0;
    if s.get(j) == Some(&b'.') {
        let mut k = j + 1;
        let mut frac = String::new();
        while s.get(k).is_some_and(u8::is_ascii_digit) {
            frac.push(s[k] as char);
            k += 1;
        }
        frac_digits = frac.len();
        if int_digits > 0 || frac_digits > 0 {
            text.push('.');
            text.push_str(&frac);
            j = k;
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return None;
    }
    if matches!(s.get(j), Some(b'e' | b'E')) {
        let mut k = j + 1;
        let mut etext = String::from("e");
        if let Some(&c @ (b'+' | b'-')) = s.get(k) {
            etext.push(c as char);
            k += 1;
        }
        if s.get(k).is_some_and(u8::is_ascii_digit) {
            while s.get(k).is_some_and(u8::is_ascii_digit) {
                etext.push(s[k] as char);
                k += 1;
            }
            text.push_str(&etext);
            j = k;
        }
    }
    if text.starts_with('.') {
        text.insert(0, '0');
    }
    let v: f64 = text.parse().ok()?;
    Some((sign(v), j))
}

// ---- caminhos (gnulib basename-lgpl / dirname-lgpl) -------------------------------------------------

/// Início do último componente (`last_component`).
fn last_component(s: &[u8]) -> usize {
    let mut base = 0;
    while base < s.len() && s[base] == b'/' {
        base += 1;
    }
    let mut last_was_slash = false;
    for (p, &c) in s.iter().enumerate().skip(base) {
        if c == b'/' {
            last_was_slash = true;
        } else if last_was_slash {
            base = p;
            last_was_slash = false;
        }
    }
    base
}

/// Tamanho sem as barras finais, mantendo ao menos um byte (`base_len`).
fn base_len(s: &[u8]) -> usize {
    let mut len = s.len();
    while len > 1 && s[len - 1] == b'/' {
        len -= 1;
    }
    len
}

/// Tamanho da parte de diretório (`dir_len`).
fn dir_len(s: &[u8]) -> usize {
    let prefix = usize::from(s.first() == Some(&b'/'));
    let mut length = last_component(s);
    while prefix < length {
        if s[length - 1] != b'/' {
            break;
        }
        length -= 1;
    }
    length
}

/// `file_name_concat` para listagens: `dir/nome` sem barra dobrada.
fn path_join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = dir.to_vec();
    if !p.ends_with(b"/") {
        p.push(b'/');
    }
    p.extend_from_slice(name);
    p
}

// ---- processos e tempo ------------------------------------------------------------------------------

fn monotonic() -> Duration {
    match sys::current().clock_gettime(Clock::Monotonic) {
        Ok(ts) => Duration::new(ts.sec.max(0) as u64, ts.nsec),
        Err(_) => Duration::ZERO,
    }
}

fn current_umask() -> Mode {
    let s = sys::current();
    let m = s.umask(0);
    s.umask(m);
    m
}

/// Procura `file` como o `execvp` da glibc (PATH padrão `/bin:/usr/bin`, elemento vazio = diretório
/// corrente, EACCES lembrado, ENOEXEC tentado de novo com `/bin/sh`), chamando `run` para cada
/// candidato. `run` devolve `Err(errno)` quando o candidato falha.
fn exec_search<T>(file: &[u8], path_var: Option<&[u8]>, argv: &[Vec<u8>], mut run: impl FnMut(&[u8], &[Vec<u8>]) -> SysResult<T>) -> SysResult<T> {
    if file.is_empty() {
        return Err(Errno::ENOENT);
    }
    let mut attempt = |cand: &[u8]| -> SysResult<T> {
        match run(cand, argv) {
            Err(Errno::ENOEXEC) => {
                let mut script = vec![b"/bin/sh".to_vec(), cand.to_vec()];
                script.extend(argv.iter().skip(1).cloned());
                run(b"/bin/sh", &script)
            }
            r => r,
        }
    };
    if file.contains(&b'/') {
        return attempt(file);
    }
    let path = path_var.unwrap_or(b"/bin:/usr/bin");
    let mut eacces = false;
    let mut last = Errno::ENOENT;
    for dir in path.split(|&b| b == b':') {
        let cand = if dir.is_empty() { file.to_vec() } else { bv(&[dir, b"/", file]) };
        match attempt(&cand) {
            Ok(v) => return Ok(v),
            Err(e) => {
                if e == Errno::EACCES {
                    eacces = true;
                } else if ![Errno::ENOENT, Errno::ESTALE, Errno::ENOTDIR, Errno::ENODEV, Errno::ETIMEDOUT].contains(&e) {
                    return Err(e);
                }
                last = e;
            }
        }
    }
    Err(if eacces { Errno::EACCES } else { last })
}

/// Valor de uma variável numa lista `NOME=valor`.
fn env_lookup<'e>(env: &'e [Vec<u8>], name: &[u8]) -> Option<&'e [u8]> {
    env.iter().find_map(|kv| kv.strip_prefix(name).and_then(|r| r.strip_prefix(b"=")))
}

fn is_dir(st: &Stat) -> bool {
    st.file_type() == FileType::Directory
}

/// `argmatch`: valor exato ou abreviação sem ambiguidade. `Err(true)` = ambíguo.
fn argmatch(value: &[u8], valid: &[&str]) -> Result<usize, bool> {
    if let Some(i) = valid.iter().position(|v| v.as_bytes() == value) {
        return Ok(i);
    }
    let cands: Vec<usize> = (0..valid.len()).filter(|&i| valid[i].as_bytes().starts_with(value)).collect();
    match cands.len() {
        0 => Err(false),
        1 => Ok(cands[0]),
        _ => Err(true),
    }
}

/// Mensagem do `argmatch_die`: argumento inválido ou ambíguo, a lista dos válidos e o "Try".
fn argmatch_fail(t: &Tool, option: &[u8], value: &[u8], ambiguous: bool, valid: &[&str], code: i32) -> i32 {
    let kind: &[u8] = if ambiguous { b"ambiguous argument " } else { b"invalid argument " };
    t.error(&bv(&[kind, &quote(value), b" for ", &quote(option)]));
    let mut msg = b"Valid arguments are:".to_vec();
    for v in valid {
        msg.extend_from_slice(b"\n  - ");
        msg.extend_from_slice(&quote(v.as_bytes()));
    }
    msg.push(b'\n');
    stderr(&msg);
    t.try_help();
    code
}

// =====================================================================================================
// true, false
// =====================================================================================================

fn true_(_t: &Tool, _args: &Argv) -> i32 {
    0
}

fn false_(_t: &Tool, _args: &Argv) -> i32 {
    1
}

// =====================================================================================================
// basename, dirname
// =====================================================================================================

fn basename(t: &Tool, args: &Argv) -> i32 {
    const LONGS: &[LongOpt] =
        &[long("multiple", HasArg::No, b'a' as u32), long("suffix", HasArg::Req, b's' as u32), long("zero", HasArg::No, b'z' as u32)];
    let mut g = Getopt::new(t, args, b"+as:z", LONGS);
    let (mut multiple, mut suffix, mut zero) = (false, None::<Vec<u8>>, false);
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, v) => match k {
                k if k == u32::from(b'a') => multiple = true,
                k if k == u32::from(b's') => {
                    suffix = v;
                    multiple = true;
                }
                k if k == u32::from(b'z') => zero = true,
                k => return help_or_version(t, k, 1),
            },
        }
    }
    let ops = g.operands();
    if ops.is_empty() {
        return t.usage_error(b"missing operand", 1);
    }
    if !multiple && ops.len() > 2 {
        return t.usage_error(&bv(&[b"extra operand ", &quote(&ops[2])]), 1);
    }
    let mut out = Out::new(t);
    let term = if zero { 0 } else { b'\n' };
    if multiple {
        for op in &ops {
            out.put(&base_name(op, suffix.as_deref()));
            out.byte(term);
        }
    } else {
        out.put(&base_name(&ops[0], ops.get(1).map(Vec::as_slice)));
        out.byte(term);
    }
    out.finish(0)
}

/// `perform_basename`: último componente sem as barras finais e sem o sufixo (se sobrar algo).
fn base_name(s: &[u8], suffix: Option<&[u8]>) -> Vec<u8> {
    let base = last_component(s);
    let mut name = if base == s.len() {
        s[..base_len(s)].to_vec()
    } else {
        let rest = &s[base..];
        let mut len = base_len(rest);
        if rest.get(len) == Some(&b'/') {
            len += 1;
        }
        rest[..len].to_vec()
    };
    // strip_trailing_slashes
    let lc = last_component(&name);
    let b = if lc == name.len() { 0 } else { lc };
    let keep = b + base_len(&name[b..]);
    name.truncate(keep);
    if let Some(suf) = suffix
        && !name.starts_with(b"/")
        && name.len() > suf.len()
        && name.ends_with(suf)
    {
        name.truncate(name.len() - suf.len());
    }
    name
}

fn dirname(t: &Tool, args: &Argv) -> i32 {
    const LONGS: &[LongOpt] = &[long("zero", HasArg::No, b'z' as u32)];
    let mut g = Getopt::new(t, args, b"z", LONGS);
    let mut zero = false;
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, _) if k == u32::from(b'z') => zero = true,
            Got::Opt(k, _) => return help_or_version(t, k, 1),
        }
    }
    let ops = g.operands();
    if ops.is_empty() {
        return t.usage_error(b"missing operand", 1);
    }
    let mut out = Out::new(t);
    for op in &ops {
        let len = dir_len(op);
        if len == 0 {
            out.byte(b'.');
        } else {
            out.put(&op[..len]);
        }
        out.byte(if zero { 0 } else { b'\n' });
    }
    out.finish(0)
}

// =====================================================================================================
// yes, sleep
// =====================================================================================================

/// `parse_gnu_standard_options_only`: só `--help`, `--version` e `--`; o resto é operando.
fn standard_options_only(t: &Tool, args: &Argv, permute: bool) -> Result<Vec<Vec<u8>>, i32> {
    let mut g = Getopt::new(t, args, if permute { b"" } else { b"+" }, &[]);
    loop {
        match g.next() {
            Got::End => return Ok(g.operands()),
            Got::Fail => {
                t.try_help();
                return Err(1);
            }
            Got::Opt(k, _) => return Err(help_or_version(t, k, 1)),
        }
    }
}

fn yes(t: &Tool, args: &Argv) -> i32 {
    let ops = match standard_options_only(t, args, true) {
        Ok(o) => o,
        Err(c) => return c,
    };
    let mut line = if ops.is_empty() { b"y".to_vec() } else { ops.join(&b' ') };
    line.push(b'\n');
    let mut buf = line.clone();
    while buf.len() + line.len() <= 8192 {
        buf.extend_from_slice(&line);
    }
    loop {
        if let Err(e) = sys::write_all(Fd::STDOUT, &buf) {
            t.error_errno(b"standard output", e);
            return 1;
        }
    }
}

fn sleep(t: &Tool, args: &Argv) -> i32 {
    let ops = match standard_options_only(t, args, true) {
        Ok(o) => o,
        Err(c) => return c,
    };
    // Como o GNU: só reclama de operando faltando quando não há argumento nenhum (`sleep --` dorme 0).
    if args.is_empty() {
        return t.usage_error(b"missing operand", 1);
    }
    let mut total = 0f64;
    let mut ok = true;
    for op in &ops {
        match parse_interval(op) {
            Some(s) => total += s,
            None => {
                t.error(&bv(&[b"invalid time interval ", &quote(op)]));
                ok = false;
            }
        }
    }
    if !ok {
        t.try_help();
        return 1;
    }
    sleep_seconds(total);
    0
}

/// NÚMERO[SUFIXO] do `sleep`/`timeout`: s, m, h ou d; negativo e NaN são inválidos.
fn parse_interval(s: &[u8]) -> Option<f64> {
    let (v, n) = strtod_prefix(s)?;
    if v.is_nan() || v < 0.0 {
        return None;
    }
    let mult = match &s[n..] {
        [] | [b's'] => 1.0,
        [b'm'] => 60.0,
        [b'h'] => 3600.0,
        [b'd'] => 86400.0,
        _ => return None,
    };
    Some(v * mult)
}

/// `dtotimespec`: nanossegundos arredondados para cima; `None` = para sempre.
fn seconds_to_duration(secs: f64) -> Option<Duration> {
    if !(secs < 9.0e18) {
        return None;
    }
    let s = secs.trunc();
    let frac = (secs - s) * 1e9;
    let mut ns = frac as u64;
    if (ns as f64) < frac {
        ns += 1;
    }
    Some(Duration::from_secs(s as u64) + Duration::from_nanos(ns))
}

/// `xnanosleep`: dorme o total, retomando depois de EINTR.
fn sleep_seconds(secs: f64) {
    let s = sys::current();
    let chunk = Duration::from_secs(86_400);
    let Some(mut rem) = seconds_to_duration(secs) else {
        loop {
            let _ = s.nanosleep(chunk);
        }
    };
    while !rem.is_zero() {
        let step = rem.min(chunk);
        let before = monotonic();
        match s.nanosleep(step) {
            Ok(()) => rem -= step,
            Err(Errno::EINTR) => {
                let elapsed = monotonic().saturating_sub(before).min(step);
                rem = rem.saturating_sub(elapsed);
            }
            Err(_) => break,
        }
    }
}

// =====================================================================================================
// cat
// =====================================================================================================

#[derive(Default)]
struct CatOpts {
    number: bool,
    nonblank: bool,
    squeeze: bool,
    ends: bool,
    tabs: bool,
    nonprinting: bool,
}

/// Estado que atravessa os arquivos: linhas em branco seguidas e o número da linha.
struct CatState {
    newlines: i32,
    line_num: u64,
}

fn cat(t: &Tool, args: &Argv) -> i32 {
    const LONGS: &[LongOpt] = &[
        long("show-all", HasArg::No, b'A' as u32),
        long("number-nonblank", HasArg::No, b'b' as u32),
        long("show-ends", HasArg::No, b'E' as u32),
        long("number", HasArg::No, b'n' as u32),
        long("squeeze-blank", HasArg::No, b's' as u32),
        long("show-tabs", HasArg::No, b'T' as u32),
        long("show-nonprinting", HasArg::No, b'v' as u32),
    ];
    let mut g = Getopt::new(t, args, b"benstuvAET", LONGS);
    let mut o = CatOpts::default();
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, _) => match u8::try_from(k).unwrap_or(0) {
                b'b' => {
                    o.number = true;
                    o.nonblank = true;
                }
                b'e' => {
                    o.ends = true;
                    o.nonprinting = true;
                }
                b'n' => o.number = true,
                b's' => o.squeeze = true,
                b't' => {
                    o.tabs = true;
                    o.nonprinting = true;
                }
                b'u' => {}
                b'v' => o.nonprinting = true,
                b'A' => {
                    o.nonprinting = true;
                    o.ends = true;
                    o.tabs = true;
                }
                b'E' => o.ends = true,
                b'T' => o.tabs = true,
                _ => return help_or_version(t, k, 1),
            },
        }
    }
    let mut files = g.operands();
    if files.is_empty() {
        files.push(b"-".to_vec());
    }
    let s = sys::current();
    let out_stat = match s.fstat(Fd::STDOUT) {
        Ok(st) => st,
        Err(e) => {
            t.error_errno(b"standard output", e);
            return 1;
        }
    };
    let simple = !(o.number || o.squeeze || o.ends || o.tabs || o.nonprinting);
    let mut out = Out::new(t);
    let mut state = CatState { newlines: 0, line_num: 0 };
    let mut ok = true;
    for f in &files {
        let mut input = match Input::open(f) {
            Ok(i) => i,
            Err(e) => {
                t.error_errno(&quotef(f), e);
                ok = false;
                continue;
            }
        };
        match s.fstat(input.fd) {
            Ok(st) => {
                let kind = st.file_type();
                if kind != FileType::Fifo
                    && kind != FileType::Socket
                    && out_stat.file_type() == FileType::Regular
                    && st.dev == out_stat.dev
                    && st.ino == out_stat.ino
                    && s.lseek(input.fd, 0, Whence::Cur).is_ok_and(|off| off < st.size)
                {
                    t.error(&bv(&[&quotef(f), b": input file is output file"]));
                    ok = false;
                    input.close();
                    continue;
                }
            }
            Err(e) => {
                t.error_errno(&quotef(f), e);
                ok = false;
                input.close();
                continue;
            }
        }
        loop {
            let chunk = match input.chunk() {
                Ok(c) => c,
                Err(e) => {
                    out.flush();
                    t.error_errno(&quotef(f), e);
                    ok = false;
                    break;
                }
            };
            if chunk.is_empty() {
                break;
            }
            if simple {
                out.put(chunk);
            } else {
                cat_format(chunk, &o, &mut state, &mut out);
            }
            // O cat escreve cada bloco lido antes de ler o próximo.
            out.flush();
        }
        input.close();
    }
    out.finish(if ok { 0 } else { 1 })
}

fn cat_format(chunk: &[u8], o: &CatOpts, st: &mut CatState, out: &mut Out) {
    let mut number_line = |st: &mut CatState, out: &mut Out| {
        st.line_num += 1;
        out.put(format!("{:>6}\t", st.line_num).as_bytes());
    };
    for &c in chunk {
        if c == b'\n' {
            st.newlines += 1;
            if st.newlines > 0 {
                if st.newlines >= 2 {
                    st.newlines = 2;
                    if o.squeeze {
                        continue;
                    }
                }
                if o.number && !o.nonblank {
                    number_line(st, out);
                }
            }
            if o.ends {
                out.byte(b'$');
            }
            out.byte(b'\n');
            continue;
        }
        if st.newlines >= 0 && o.number {
            number_line(st, out);
        }
        st.newlines = -1;
        if o.nonprinting {
            if c >= 32 {
                if c < 127 {
                    out.byte(c);
                } else if c == 127 {
                    out.put(b"^?");
                } else {
                    out.put(b"M-");
                    let m = c - 128;
                    if m >= 32 {
                        if m < 127 {
                            out.byte(m);
                        } else {
                            out.put(b"^?");
                        }
                    } else {
                        out.byte(b'^');
                        out.byte(m + 64);
                    }
                }
            } else if c == b'\t' && !o.tabs {
                out.byte(b'\t');
            } else {
                out.byte(b'^');
                out.byte(c + 64);
            }
        } else if c == b'\t' && o.tabs {
            out.put(b"^I");
        } else {
            out.byte(c);
        }
    }
}

// =====================================================================================================
// tee
// =====================================================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputError {
    Sigpipe,
    Warn,
    WarnNopipe,
    Exit,
    ExitNopipe,
}

fn tee(t: &Tool, args: &Argv) -> i32 {
    const KEY_OUTPUT_ERROR: u32 = b'p' as u32;
    const LONGS: &[LongOpt] = &[
        long("append", HasArg::No, b'a' as u32),
        long("ignore-interrupts", HasArg::No, b'i' as u32),
        long("output-error", HasArg::Opt, KEY_OUTPUT_ERROR),
    ];
    const MODES: &[&str] = &["warn", "warn-nopipe", "exit", "exit-nopipe"];
    let mut g = Getopt::new(t, args, b"aip", LONGS);
    let (mut append, mut ignore_int, mut mode) = (false, false, OutputError::Sigpipe);
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, v) => match u8::try_from(k).unwrap_or(0) {
                b'a' => append = true,
                b'i' => ignore_int = true,
                b'p' => {
                    mode = match v {
                        None => OutputError::WarnNopipe,
                        Some(v) => match argmatch(&v, MODES) {
                            Ok(0) => OutputError::Warn,
                            Ok(1) => OutputError::WarnNopipe,
                            Ok(2) => OutputError::Exit,
                            Ok(_) => OutputError::ExitNopipe,
                            Err(amb) => return argmatch_fail(t, b"--output-error", &v, amb, MODES, 1),
                        },
                    }
                }
                _ => return help_or_version(t, k, 1),
            },
        }
    }
    let files = g.operands();
    let s = sys::current();
    if ignore_int {
        let _ = s.sigaction(Signal::SIGINT, SigDisposition::Ignore);
    }
    if mode != OutputError::Sigpipe {
        let _ = s.sigaction(Signal::SIGPIPE, SigDisposition::Ignore);
    }
    let mut ok = true;
    // (nome para mensagens, fd, ativo)
    let mut outputs: Vec<(Vec<u8>, Fd, bool)> = vec![(b"standard output".to_vec(), Fd::STDOUT, true)];
    let flags = OFlags::WRONLY | OFlags::CREAT | OFlags::CLOEXEC | if append { OFlags::APPEND } else { OFlags::TRUNC };
    for f in &files {
        match sys::open(f, flags, 0o666) {
            Ok(fd) => outputs.push((f.clone(), fd, true)),
            Err(e) => {
                t.error_errno(&quotef(f), e);
                ok = false;
            }
        }
    }
    let mut active = outputs.len();
    let mut buf = vec![0u8; 8192];
    while active > 0 {
        let n = match s.read(Fd::STDIN, &mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(Errno::EINTR) => continue,
            Err(e) => {
                t.error_errno(b"read error", e);
                ok = false;
                break;
            }
        };
        for o in outputs.iter_mut().filter(|o| o.2) {
            if let Err(e) = sys::write_all(o.1, &buf[..n]) {
                let fail = e != Errno::EPIPE || matches!(mode, OutputError::Exit | OutputError::Warn);
                if fail {
                    t.error_errno(&quotef(&o.0), e);
                    if matches!(mode, OutputError::Exit | OutputError::ExitNopipe) {
                        sys::exit(1);
                    }
                    ok = false;
                }
                o.2 = false;
                active -= 1;
            }
        }
    }
    for o in outputs.iter().skip(1) {
        let _ = s.close(o.1);
    }
    if ok { 0 } else { 1 }
}

// =====================================================================================================
// head, tail
// =====================================================================================================

/// Cabeçalhos `==> nome <==`: `None` = só com mais de um arquivo.
type HeaderMode = Option<bool>;

/// Número de linhas ou bytes do head/tail, com as mensagens do GNU. `quiet_overflow` satura no
/// máximo em vez de reclamar (o tail faz assim).
fn parse_count(t: &Tool, s: &[u8], lines: bool, quiet_overflow: bool) -> Result<u64, i32> {
    let what: &[u8] = if lines { b"invalid number of lines" } else { b"invalid number of bytes" };
    match xstrtoumax(s, 10, b"bkKmMGTPEZYRQ0") {
        Ok(v) => Ok(v),
        Err(NumErr::Overflow) if quiet_overflow => Ok(u64::MAX),
        Err(NumErr::Overflow) => {
            t.error_errno(&bv(&[what, b": ", &quote(s)]), Errno::EOVERFLOW);
            Err(1)
        }
        Err(NumErr::Invalid) => {
            t.error(&bv(&[what, b": ", &quote(s)]));
            Err(1)
        }
    }
}

/// Escreve o cabeçalho de arquivo do head/tail (linha em branco antes, exceto no primeiro).
fn write_header(out: &mut Out, first: &mut bool, name: &[u8]) {
    out.put(&bv(&[if *first { b"" } else { b"\n" }, b"==> ", name, b" <==\n"]));
    *first = false;
}

fn display_name(name: &[u8]) -> &[u8] {
    if name == b"-" { b"standard input" } else { name }
}

fn head(t: &Tool, args: &Argv) -> i32 {
    const KEY_PRESUME_PIPE: u32 = 0x100;
    const LONGS: &[LongOpt] = &[
        long("bytes", HasArg::Req, b'c' as u32),
        long("lines", HasArg::Req, b'n' as u32),
        long("presume-input-pipe", HasArg::No, KEY_PRESUME_PIPE),
        long("quiet", HasArg::No, b'q' as u32),
        long("silent", HasArg::No, b'q' as u32),
        long("verbose", HasArg::No, b'v' as u32),
        long("zero-terminated", HasArg::No, b'z' as u32),
    ];
    let mut count_lines = true;
    let mut n_units: u64 = 10;
    let mut elide = false;
    let mut headers: HeaderMode = None;
    let mut delim = b'\n';
    let mut args: Vec<Vec<u8>> = args.to_vec();
    // Sintaxe antiga: -NÚMERO[bkm][clqvz] como primeiro argumento.
    if let Some(first) = args.first().cloned()
        && first.len() > 1
        && first[0] == b'-'
        && first[1].is_ascii_digit()
    {
        let a = &first[1..];
        let end = a.iter().position(|b| !b.is_ascii_digit()).unwrap_or(a.len());
        let mut n_string = a[..end].to_vec();
        let mut mult = None;
        for &c in &a[end..] {
            match c {
                b'c' => {
                    count_lines = false;
                    mult = None;
                }
                b'b' | b'k' | b'm' => {
                    count_lines = false;
                    mult = Some(c);
                }
                b'l' => count_lines = true,
                b'q' => headers = Some(false),
                b'v' => headers = Some(true),
                b'z' => delim = 0,
                _ => return t.usage_error(&bv(&[b"invalid trailing option -- ", &[c]]), 1),
            }
        }
        if let Some(m) = mult {
            n_string.push(m);
        }
        n_units = match parse_count(t, &n_string, count_lines, false) {
            Ok(n) => n,
            Err(c) => return c,
        };
        args.remove(0);
    }
    let mut g = Getopt::new(t, &args, b"c:n:qvz0123456789", LONGS);
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, v) => match u8::try_from(k).unwrap_or(0) {
                c @ (b'c' | b'n') => {
                    count_lines = c == b'n';
                    let v = v.unwrap_or_default();
                    elide = v.first() == Some(&b'-');
                    let s = if elide { &v[1..] } else { &v[..] };
                    n_units = match parse_count(t, s, count_lines, false) {
                        Ok(n) => n,
                        Err(c) => return c,
                    };
                }
                b'q' => headers = Some(false),
                b'v' => headers = Some(true),
                b'z' => delim = 0,
                c @ b'0'..=b'9' => return t.usage_error(&bv(&[b"invalid trailing option -- ", &[c]]), 1),
                _ if k == KEY_PRESUME_PIPE => {}
                _ => return help_or_version(t, k, 1),
            },
        }
    }
    let mut files = g.operands();
    if files.is_empty() {
        files.push(b"-".to_vec());
    }
    let print_headers = headers.unwrap_or(files.len() > 1);
    let mut out = Out::new(t);
    let mut first = true;
    let mut ok = true;
    for f in &files {
        let name = display_name(f);
        let input = match Input::open(f) {
            Ok(i) => i,
            Err(e) => {
                t.error_errno(&bv(&[b"cannot open ", &quoteaf(f), b" for reading"]), e);
                ok = false;
                continue;
            }
        };
        if print_headers {
            write_header(&mut out, &mut first, name);
        }
        let r = if elide {
            head_elide(input, n_units, count_lines, delim, &mut out)
        } else if count_lines {
            head_lines(input, n_units, delim, &mut out)
        } else {
            head_bytes(input, n_units, &mut out)
        };
        if let Err(e) = r {
            t.error_errno(&bv(&[b"error reading ", &quoteaf(name)]), e);
            ok = false;
        }
    }
    out.finish(if ok { 0 } else { 1 })
}

/// Primeiros `n` bytes, sem ler além deles (o resto fica para quem vier depois no mesmo fd).
fn head_bytes(input: Input, mut n: u64, out: &mut Out) -> SysResult<()> {
    let mut buf = vec![0u8; 8192];
    let r = loop {
        if n == 0 {
            break Ok(());
        }
        let want = buf.len().min(usize::try_from(n).unwrap_or(usize::MAX));
        match sys::read(input.fd, &mut buf[..want]) {
            Ok(0) => break Ok(()),
            Ok(k) => {
                out.put(&buf[..k]);
                n -= k as u64;
            }
            Err(Errno::EINTR) => {}
            Err(e) => break Err(e),
        }
    };
    input.close();
    r
}

/// Primeiras `n` linhas; num arquivo comum devolve o ponteiro para logo depois da última linha.
fn head_lines(mut input: Input, n: u64, delim: u8, out: &mut Out) -> SysResult<()> {
    input.buf.truncate(8192);
    let mut done = 0u64;
    let mut line = Vec::new();
    let r = loop {
        if done == n {
            break Ok(());
        }
        line.clear();
        match input.read_record(delim, &mut line) {
            Ok(false) => break Ok(()),
            Ok(true) => {
                out.put(&line);
                done += 1;
            }
            Err(e) => break Err(e),
        }
    };
    let extra = input.unconsumed();
    if extra > 0 {
        let _ = sys::current().lseek(input.fd, -(extra as i64), Whence::Cur);
    }
    input.close();
    r
}

/// Tudo menos as últimas `n` linhas (ou bytes).
fn head_elide(mut input: Input, n: u64, lines: bool, delim: u8, out: &mut Out) -> SysResult<()> {
    let data = input.read_all();
    input.close();
    let data = data?;
    let keep = if lines {
        let mut total = data.iter().filter(|&&b| b == delim).count() as u64;
        if !data.is_empty() && *data.last().unwrap_or(&delim) != delim {
            total += 1;
        }
        let keep_lines = total.saturating_sub(n);
        let mut seen = 0u64;
        let mut end = 0;
        if keep_lines > 0 {
            for (i, &b) in data.iter().enumerate() {
                if b == delim {
                    seen += 1;
                    if seen == keep_lines {
                        end = i + 1;
                        break;
                    }
                }
            }
            if seen < keep_lines {
                end = data.len();
            }
        }
        end
    } else {
        data.len().saturating_sub(usize::try_from(n).unwrap_or(usize::MAX))
    };
    out.put(&data[..keep]);
    Ok(())
}

/// Forma antiga do tail (`tail -5`, `tail +3`, `tail -5c arquivo`): (n, a partir do início, linhas).
fn tail_obsolete(t: &Tool, args: &Argv) -> Option<Result<(u64, bool, bool), i32>> {
    let ok_shape = args.len() == 1
        || (args.len() == 2 && !(args[1].len() > 1 && args[1][0] == b'-'))
        || ((2..=3).contains(&args.len()) && args[1] == b"--");
    if !ok_shape {
        return None;
    }
    let a = &args[0];
    let from_start = match a.first() {
        Some(b'+') => true,
        Some(b'-') => {
            // "-" é a entrada padrão e "-c" é opção.
            let p = &a[1..];
            let probe = if p.first() == Some(&b'c') { p.get(1) } else { p.first() };
            if probe.is_none() {
                return None;
            }
            false
        }
        _ => return None,
    };
    let p = &a[1..];
    let end = p.iter().position(|b| !b.is_ascii_digit()).unwrap_or(p.len());
    let mut default_count: u64 = 10;
    let mut lines = true;
    let mut i = end;
    match p.get(i) {
        Some(b'b') => {
            default_count *= 512;
            lines = false;
            i += 1;
        }
        Some(b'c') => {
            lines = false;
            i += 1;
        }
        Some(b'l') => i += 1,
        _ => {}
    }
    if p.get(i) == Some(&b'f') {
        return Some(Err(t.unsupported(b"-f", 1)));
    }
    if i != p.len() {
        return None;
    }
    let n = if end == 0 {
        default_count
    } else {
        // O "b" (blocos de 512) faz parte do número; "c" e "l" não.
        let num_end = if p.get(end) == Some(&b'b') { end + 1 } else { end };
        match xstrtoumax(&p[..num_end], 10, b"b") {
            Ok(v) => v,
            Err(_) => {
                t.error(&bv(&[b"invalid number: ", &quote(a)]));
                return Some(Err(1));
            }
        }
    };
    Some(Ok((n, from_start, lines)))
}

fn tail(t: &Tool, args: &Argv) -> i32 {
    const KEY_RETRY: u32 = 0x100;
    const KEY_MAX_UNCHANGED: u32 = 0x101;
    const KEY_PID: u32 = 0x102;
    const KEY_FOLLOW: u32 = 0x103;
    const KEY_DEBUG: u32 = 0x104;
    const LONGS: &[LongOpt] = &[
        long("bytes", HasArg::Req, b'c' as u32),
        long("debug", HasArg::No, KEY_DEBUG),
        long("follow", HasArg::Opt, KEY_FOLLOW),
        long("lines", HasArg::Req, b'n' as u32),
        long("max-unchanged-stats", HasArg::Req, KEY_MAX_UNCHANGED),
        long("pid", HasArg::Req, KEY_PID),
        long("quiet", HasArg::No, b'q' as u32),
        long("retry", HasArg::No, KEY_RETRY),
        long("silent", HasArg::No, b'q' as u32),
        long("sleep-interval", HasArg::Req, b's' as u32),
        long("verbose", HasArg::No, b'v' as u32),
        long("zero-terminated", HasArg::No, b'z' as u32),
    ];
    let mut count_lines = true;
    let mut from_start = false;
    let mut n_units: u64 = 10;
    let mut headers: HeaderMode = None;
    let mut delim = b'\n';
    let mut args: Vec<Vec<u8>> = args.to_vec();
    match tail_obsolete(t, &args) {
        Some(Ok((n, fs, lines))) => {
            n_units = n;
            from_start = fs;
            count_lines = lines;
            args.remove(0);
        }
        Some(Err(c)) => return c,
        None => {}
    }
    let mut g = Getopt::new(t, &args, b"c:n:fFqs:vz0123456789", LONGS);
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, v) => match u8::try_from(k).unwrap_or(0) {
                c @ (b'c' | b'n') => {
                    count_lines = c == b'n';
                    let v = v.unwrap_or_default();
                    let s: &[u8] = match v.first() {
                        Some(b'+') => {
                            from_start = true;
                            &v
                        }
                        Some(b'-') => {
                            from_start = false;
                            &v[1..]
                        }
                        _ => {
                            from_start = false;
                            &v
                        }
                    };
                    n_units = match parse_count(t, s, count_lines, true) {
                        Ok(n) => n,
                        Err(c) => return c,
                    };
                }
                b'q' => headers = Some(false),
                b'v' => headers = Some(true),
                b'z' => delim = 0,
                c @ b'0'..=b'9' => {
                    t.error(&bv(&[b"option used in invalid context -- ", &[c]]));
                    return 1;
                }
                b'f' => return t.unsupported(b"-f", 1),
                b'F' => return t.unsupported(b"-F", 1),
                b's' => return t.unsupported(b"--sleep-interval", 1),
                _ if k == KEY_DEBUG => {}
                _ if k == KEY_FOLLOW => return t.unsupported(b"--follow", 1),
                _ if k == KEY_RETRY => return t.unsupported(b"--retry", 1),
                _ if k == KEY_PID => return t.unsupported(b"--pid", 1),
                _ if k == KEY_MAX_UNCHANGED => return t.unsupported(b"--max-unchanged-stats", 1),
                _ => return help_or_version(t, k, 1),
            },
        }
    }
    if from_start && n_units > 0 {
        n_units -= 1;
    }
    let mut files = g.operands();
    if files.is_empty() {
        files.push(b"-".to_vec());
    }
    let print_headers = headers.unwrap_or(files.len() > 1);
    let mut out = Out::new(t);
    let mut first = true;
    let mut ok = true;
    for f in &files {
        let name = display_name(f);
        let mut input = match Input::open(f) {
            Ok(i) => i,
            Err(e) => {
                t.error_errno(&bv(&[b"cannot open ", &quoteaf(f), b" for reading"]), e);
                ok = false;
                continue;
            }
        };
        if print_headers {
            write_header(&mut out, &mut first, name);
        }
        let r = if from_start {
            tail_from_start(&mut input, n_units, count_lines, delim, &mut out)
        } else {
            input.read_all().map(|data| out.put(tail_slice(&data, n_units, count_lines, delim)))
        };
        input.close();
        if let Err(e) = r {
            t.error_errno(&bv(&[b"error reading ", &quoteaf(name)]), e);
            ok = false;
        }
    }
    out.finish(if ok { 0 } else { 1 })
}

/// As últimas `n` linhas (ou bytes) de `data`; uma última linha sem terminador conta.
fn tail_slice(data: &[u8], n: u64, lines: bool, delim: u8) -> &[u8] {
    if !lines {
        let keep = usize::try_from(n).unwrap_or(usize::MAX).min(data.len());
        return &data[data.len() - keep..];
    }
    if n == 0 {
        return &[];
    }
    let search_end = if data.last() == Some(&delim) { data.len() - 1 } else { data.len() };
    let mut count = 0u64;
    for i in (0..search_end).rev() {
        if data[i] == delim {
            count += 1;
            if count == n {
                return &data[i + 1..];
            }
        }
    }
    data
}

/// Pula `skip` linhas (ou bytes) e copia o resto.
fn tail_from_start(input: &mut Input, mut skip: u64, lines: bool, delim: u8, out: &mut Out) -> SysResult<()> {
    loop {
        let chunk = input.chunk()?;
        if chunk.is_empty() {
            return Ok(());
        }
        let mut start = 0;
        while skip > 0 && start < chunk.len() {
            if lines {
                match chunk[start..].iter().position(|&b| b == delim) {
                    Some(k) => {
                        start += k + 1;
                        skip -= 1;
                    }
                    None => start = chunk.len(),
                }
            } else {
                let take = usize::try_from(skip).unwrap_or(usize::MAX).min(chunk.len() - start);
                start += take;
                skip -= take as u64;
            }
        }
        out.put(&chunk[start..]);
    }
}

// =====================================================================================================
// wc
// =====================================================================================================

#[derive(Default, Clone, Copy)]
struct WcCounts {
    lines: u64,
    words: u64,
    chars: u64,
    bytes: u64,
    max_len: u64,
}

#[derive(Default)]
struct WcSel {
    lines: bool,
    words: bool,
    chars: bool,
    bytes: bool,
    max_len: bool,
}

fn wc(t: &Tool, args: &Argv) -> i32 {
    const KEY_FILES0: u32 = 0x100;
    const KEY_TOTAL: u32 = 0x101;
    const KEY_DEBUG: u32 = 0x102;
    const LONGS: &[LongOpt] = &[
        long("bytes", HasArg::No, b'c' as u32),
        long("chars", HasArg::No, b'm' as u32),
        long("debug", HasArg::No, KEY_DEBUG),
        long("files0-from", HasArg::Req, KEY_FILES0),
        long("lines", HasArg::No, b'l' as u32),
        long("max-line-length", HasArg::No, b'L' as u32),
        long("total", HasArg::Req, KEY_TOTAL),
        long("words", HasArg::No, b'w' as u32),
    ];
    const TOTALS: &[&str] = &["auto", "always", "only", "never"];
    let mut g = Getopt::new(t, args, b"clLmw", LONGS);
    let mut sel = WcSel::default();
    let mut total_mode = 0;
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, v) => match u8::try_from(k).unwrap_or(0) {
                b'c' => sel.bytes = true,
                b'm' => sel.chars = true,
                b'l' => sel.lines = true,
                b'w' => sel.words = true,
                b'L' => sel.max_len = true,
                _ if k == KEY_DEBUG => {}
                _ if k == KEY_FILES0 => return t.unsupported(b"--files0-from", 1),
                _ if k == KEY_TOTAL => {
                    let v = v.unwrap_or_default();
                    match argmatch(&v, TOTALS) {
                        Ok(i) => total_mode = i,
                        Err(amb) => return argmatch_fail(t, b"--total", &v, amb, TOTALS, 1),
                    }
                }
                _ => return help_or_version(t, k, 1),
            },
        }
    }
    if !(sel.lines || sel.words || sel.chars || sel.bytes || sel.max_len) {
        sel.lines = true;
        sel.words = true;
        sel.bytes = true;
    }
    let ops = g.operands();
    let stdin_only = ops.is_empty();
    let names: Vec<Vec<u8>> = if stdin_only { vec![b"-".to_vec()] } else { ops };
    let n_selected = [sel.lines, sel.words, sel.chars, sel.bytes, sel.max_len].iter().filter(|&&b| b).count();
    let s = sys::current();
    // Largura das colunas: dígitos do tamanho somado dos arquivos comuns; 7 se houver algo que não é
    // arquivo comum; 1 com um arquivo e uma contagem só, ou com --total=only.
    let width = if total_mode == 2 || (names.len() == 1 && n_selected == 1) {
        1
    } else {
        let mut minimum = 1;
        let mut regular_total: u64 = 0;
        for n in &names {
            let st = if n == b"-" { s.fstat(Fd::STDIN) } else { sys::stat(n) };
            if let Ok(st) = st {
                if st.file_type() == FileType::Regular {
                    regular_total = regular_total.saturating_add(st.size);
                } else {
                    minimum = 7;
                }
            }
        }
        regular_total.to_string().len().max(minimum)
    };
    let mut out = Out::new(t);
    let mut total = WcCounts::default();
    let mut ok = true;
    for n in &names {
        let mut input = match Input::open(n) {
            Ok(i) => i,
            Err(e) => {
                t.error_errno(&quotef(n), e);
                ok = false;
                continue;
            }
        };
        let mut c = WcCounts::default();
        if let Err(e) = wc_count(&mut input, &sel, &mut c) {
            t.error_errno(&quotef(n), e);
            ok = false;
        }
        input.close();
        total.lines += c.lines;
        total.words += c.words;
        total.chars += c.chars;
        total.bytes += c.bytes;
        total.max_len = total.max_len.max(c.max_len);
        if total_mode != 2 {
            wc_line(&mut out, &sel, &c, width, if stdin_only { None } else { Some(n) });
        }
    }
    if total_mode == 1 || total_mode == 2 || (total_mode == 0 && names.len() > 1) {
        wc_line(&mut out, &sel, &total, width, if total_mode == 2 { None } else { Some(b"total") });
    }
    out.finish(if ok { 0 } else { 1 })
}

fn wc_line(out: &mut Out, sel: &WcSel, c: &WcCounts, width: usize, name: Option<&[u8]>) {
    let mut line = Vec::new();
    for (on, v) in [(sel.lines, c.lines), (sel.words, c.words), (sel.chars, c.chars), (sel.bytes, c.bytes), (sel.max_len, c.max_len)] {
        if on {
            if !line.is_empty() {
                line.push(b' ');
            }
            line.extend_from_slice(format!("{v:>width$}").as_bytes());
        }
    }
    if let Some(n) = name {
        line.push(b' ');
        if n.contains(&b'\n') {
            line.extend_from_slice(&quotef(n));
        } else {
            line.extend_from_slice(n);
        }
    }
    line.push(b'\n');
    out.put(&line);
}

/// Espaços de `iswspace` no C.UTF-8, mais os não separáveis que o wc 9.7 também trata como espaço.
fn wc_is_space(cp: u32) -> bool {
    matches!(cp, 0x09..=0x0D | 0x20 | 0x1680 | 0x2000..=0x2006 | 0x2008..=0x200A | 0x2028 | 0x2029 | 0x205F | 0x3000)
        || matches!(cp, 0xA0 | 0x2007 | 0x202F | 0x2060)
}

/// Largura de coluna (`wcwidth`) aproximada: 0 para marcas combinantes e formatação, 2 para os
/// blocos largos do leste asiático e emoji, 1 para o resto imprimível, -1 para controle.
fn wcwidth(cp: u32) -> i32 {
    if cp == 0 {
        return 0;
    }
    if cp < 0x20 || (0x7F..0xA0).contains(&cp) {
        return -1;
    }
    let zero = matches!(cp,
        0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5
        | 0x05C7 | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC | 0x06DF..=0x06E4
        | 0x06E7..=0x06E8 | 0x06EA..=0x06ED | 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E
        | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064
        | 0x20D0..=0x20FF | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F | 0xFEFF | 0xE0100..=0xE01EF);
    if zero {
        return 0;
    }
    let wide = matches!(cp,
        0x1100..=0x115F | 0x231A..=0x231B | 0x2329..=0x232A | 0x23E9..=0x23EC | 0x23F0 | 0x23F3
        | 0x25FD..=0x25FE | 0x2614..=0x2615 | 0x2648..=0x2653 | 0x267F | 0x2693 | 0x26A1 | 0x26AA..=0x26AB
        | 0x26BD..=0x26BE | 0x26C4..=0x26C5 | 0x26CE | 0x26D4 | 0x26EA | 0x26F2..=0x26F3 | 0x26F5 | 0x26FA
        | 0x26FD | 0x2705 | 0x270A..=0x270B | 0x2728 | 0x274C | 0x274E | 0x2753..=0x2755 | 0x2757
        | 0x2795..=0x2797 | 0x27B0 | 0x27BF | 0x2B1B..=0x2B1C | 0x2B50 | 0x2B55 | 0x2E80..=0x303E
        | 0x3041..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xA000..=0xA4CF | 0xA960..=0xA97F
        | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE10..=0xFE19 | 0xFE30..=0xFE6F | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6 | 0x16FE0..=0x16FE4 | 0x17000..=0x18CFF | 0x1B000..=0x1B2FF | 0x1F004 | 0x1F0CF
        | 0x1F18E | 0x1F191..=0x1F19A | 0x1F200..=0x1F251 | 0x1F300..=0x1F64F | 0x1F680..=0x1F6FF
        | 0x1F7E0..=0x1F7EB | 0x1F90C..=0x1F9FF | 0x1FA70..=0x1FAFF | 0x20000..=0x2FFFD | 0x30000..=0x3FFFD);
    if wide { 2 } else { 1 }
}

fn wc_count(input: &mut Input, sel: &WcSel, c: &mut WcCounts) -> SysResult<()> {
    let need_chars = sel.words || sel.chars || sel.max_len;
    let mut carry: Vec<u8> = Vec::new();
    let mut in_word = false;
    let mut linepos: u64 = 0;
    loop {
        let chunk = input.chunk()?;
        let at_eof = chunk.is_empty();
        c.bytes += chunk.len() as u64;
        c.lines += chunk.iter().filter(|&&b| b == b'\n').count() as u64;
        if !need_chars {
            if at_eof {
                return Ok(());
            }
            continue;
        }
        let mut data = std::mem::take(&mut carry);
        data.extend_from_slice(chunk);
        let mut i = 0;
        while i < data.len() {
            let (cp, n) = match utf8_decode(&data, i) {
                Utf8::Char(cp, n) => (Some(cp), n),
                Utf8::Incomplete if !at_eof => {
                    carry = data[i..].to_vec();
                    break;
                }
                _ => (None, 1),
            };
            i += n;
            let Some(cp) = cp else {
                // Byte inválido: não é caractere, mas faz parte de uma palavra.
                if !in_word {
                    c.words += 1;
                    in_word = true;
                }
                continue;
            };
            c.chars += 1;
            match cp {
                0x0A | 0x0D | 0x0C => {
                    linepos_end(&mut linepos, c);
                    in_word = false;
                }
                0x09 => {
                    linepos += 8 - linepos % 8;
                    in_word = false;
                }
                _ => {
                    let w = wcwidth(cp);
                    if w > 0 {
                        linepos += w as u64;
                    }
                    if wc_is_space(cp) {
                        in_word = false;
                    } else if !in_word {
                        c.words += 1;
                        in_word = true;
                    }
                }
            }
        }
        if at_eof {
            linepos_end(&mut linepos, c);
            return Ok(());
        }
    }
}

fn linepos_end(linepos: &mut u64, c: &mut WcCounts) {
    if *linepos > c.max_len {
        c.max_len = *linepos;
    }
    *linepos = 0;
}

// =====================================================================================================
// paste
// =====================================================================================================

/// Lista de delimitadores do `paste -d`; `None` é o delimitador vazio (`\0`).
fn paste_delims(list: &[u8]) -> Option<Vec<Option<u8>>> {
    if list.is_empty() {
        return Some(vec![None]);
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < list.len() {
        if list[i] == b'\\' {
            let c = *list.get(i + 1)?;
            out.push(match c {
                b'0' => None,
                b'b' => Some(8),
                b'f' => Some(12),
                b'n' => Some(b'\n'),
                b'r' => Some(b'\r'),
                b't' => Some(b'\t'),
                b'v' => Some(11),
                other => Some(other),
            });
            i += 2;
        } else {
            out.push(Some(list[i]));
            i += 1;
        }
    }
    Some(out)
}

fn paste(t: &Tool, args: &Argv) -> i32 {
    const LONGS: &[LongOpt] = &[
        long("delimiters", HasArg::Req, b'd' as u32),
        long("serial", HasArg::No, b's' as u32),
        long("zero-terminated", HasArg::No, b'z' as u32),
    ];
    let mut g = Getopt::new(t, args, b"d:sz", LONGS);
    let (mut list, mut serial, mut term) = (b"\t".to_vec(), false, b'\n');
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, v) => match u8::try_from(k).unwrap_or(0) {
                b'd' => list = v.unwrap_or_default(),
                b's' => serial = true,
                b'z' => term = 0,
                _ => return help_or_version(t, k, 1),
            },
        }
    }
    let Some(delims) = paste_delims(&list) else {
        t.error(&bv(&[b"delimiter list ends with an unescaped backslash: ", &list]));
        return 1;
    };
    let mut files = g.operands();
    if files.is_empty() {
        files.push(b"-".to_vec());
    }
    let mut out = Out::new(t);
    let code = if serial { paste_serial(t, &files, &delims, term, &mut out) } else { paste_parallel(t, &files, &delims, term, &mut out) };
    out.finish(code)
}

fn paste_serial(t: &Tool, files: &[Vec<u8>], delims: &[Option<u8>], term: u8, out: &mut Out) -> i32 {
    let mut ok = true;
    for f in files {
        let mut input = match Input::open(f) {
            Ok(i) => i,
            Err(e) => {
                t.error_errno(&quotef(f), e);
                ok = false;
                continue;
            }
        };
        let data = input.read_all();
        input.close();
        let data = match data {
            Ok(d) => d,
            Err(e) => {
                t.error_errno(&quotef(f), e);
                ok = false;
                continue;
            }
        };
        let body = data.strip_suffix(&[term]).unwrap_or(&data);
        let mut d = 0;
        if !data.is_empty() {
            for (i, piece) in body.split(|&b| b == term).enumerate() {
                if i > 0 {
                    if let Some(c) = delims[d] {
                        out.byte(c);
                    }
                    d = (d + 1) % delims.len();
                }
                out.put(piece);
            }
        }
        out.byte(term);
    }
    if ok { 0 } else { 1 }
}

fn paste_parallel(t: &Tool, files: &[Vec<u8>], delims: &[Option<u8>], term: u8, out: &mut Out) -> i32 {
    // Todos os "-" leem do mesmo fluxo.
    let mut streams: Vec<Input> = Vec::new();
    let mut slot_stream: Vec<usize> = Vec::new();
    let mut stdin_idx = None;
    for f in files {
        if f == b"-" {
            let idx = *stdin_idx.get_or_insert_with(|| {
                streams.push(Input::stdin());
                streams.len() - 1
            });
            slot_stream.push(idx);
            continue;
        }
        match Input::open(f) {
            Ok(i) => {
                streams.push(i);
                slot_stream.push(streams.len() - 1);
            }
            Err(e) => {
                t.error_errno(&quotef(f), e);
                return 1;
            }
        }
    }
    let n = files.len();
    let mut open = vec![true; n];
    let mut files_open = n;
    let mut ok = true;
    let mut line = Vec::new();
    while files_open > 0 {
        let mut some_done = false;
        let mut saved: Vec<u8> = Vec::new();
        let mut d = 0;
        let mut i = 0;
        while i < n && files_open > 0 {
            let mut got = false;
            if open[i] {
                line.clear();
                match streams[slot_stream[i]].read_record(term, &mut line) {
                    Ok(true) => got = true,
                    Ok(false) => {}
                    Err(e) => {
                        t.error_errno(&quotef(&files[i]), e);
                        ok = false;
                    }
                }
                if !got {
                    open[i] = false;
                    files_open -= 1;
                }
            }
            if got {
                out.put(&saved);
                saved.clear();
                some_done = true;
                let content = line.strip_suffix(&[term]).unwrap_or(&line);
                out.put(content);
                if i + 1 < n {
                    if let Some(c) = delims[d] {
                        out.byte(c);
                    }
                    d = (d + 1) % delims.len();
                } else {
                    out.byte(term);
                }
            } else if i + 1 == n {
                if some_done {
                    out.put(&saved);
                    out.byte(term);
                }
            } else {
                if let Some(c) = delims[d] {
                    saved.push(c);
                }
                d = (d + 1) % delims.len();
            }
            i += 1;
        }
    }
    for s in streams {
        s.close();
    }
    if ok { 0 } else { 1 }
}

// =====================================================================================================
// cut
// =====================================================================================================

/// Faixas 1-based inclusivas, ordenadas e fundidas quando se sobrepõem (adjacentes continuam
/// separadas, o que importa para o `--output-delimiter`).
fn cut_ranges(t: &Tool, spec: &[u8], positions: bool, complement: bool) -> Result<Vec<(u64, u64)>, i32> {
    let fail = |msg: &[u8]| -> Result<Vec<(u64, u64)>, i32> { Err(t.usage_error(msg, 1)) };
    let zero_msg: &[u8] = if positions { b"byte/character positions are numbered from 1" } else { b"fields are numbered from 1" };
    let mut ranges: Vec<(u64, u64)> = Vec::new();
    for part in spec.split(|&b| b == b',' || is_blank(b)) {
        if part.is_empty() {
            // Vírgula sobrando: o GNU trata como número ausente.
            return fail(zero_msg);
        }
        let parse = |s: &[u8]| -> Result<Option<u64>, Vec<u8>> {
            if s.is_empty() {
                return Ok(None);
            }
            if let Some(k) = s.iter().position(|b| !b.is_ascii_digit()) {
                return Err(s[k..].to_vec());
            }
            Ok(Some(std::str::from_utf8(s).ok().and_then(|x| x.parse::<u64>().ok()).unwrap_or(u64::MAX)))
        };
        let bad_value = |rest: &[u8]| -> Vec<u8> {
            let what: &[u8] = if positions { b"invalid byte/character position " } else { b"invalid field value " };
            bv(&[what, &quote(rest)])
        };
        match part.iter().position(|&b| b == b'-') {
            None => match parse(part) {
                Ok(Some(0)) => return fail(zero_msg),
                Ok(Some(v)) => ranges.push((v, v)),
                Ok(None) => return fail(zero_msg),
                Err(rest) => return fail(&bad_value(&rest)),
            },
            Some(dash) => {
                let (lo_s, hi_s) = (&part[..dash], &part[dash + 1..]);
                if hi_s.contains(&b'-') {
                    return fail(if positions { b"invalid byte or character range" } else { b"invalid field range" });
                }
                let lo = match parse(lo_s) {
                    Ok(v) => v,
                    Err(rest) => return fail(&bad_value(&bv(&[&rest, b"-", hi_s]))),
                };
                let hi = match parse(hi_s) {
                    Ok(v) => v,
                    Err(rest) => return fail(&bad_value(&rest)),
                };
                if lo == Some(0) {
                    return fail(zero_msg);
                }
                match (lo, hi) {
                    (None, None) => return fail(b"invalid range with no endpoint: -"),
                    (lo, None) => ranges.push((lo.unwrap_or(1), u64::MAX)),
                    (lo, Some(hi)) => {
                        let lo = lo.unwrap_or(1);
                        if hi < lo {
                            return fail(b"invalid decreasing range");
                        }
                        ranges.push((lo, hi));
                    }
                }
            }
        }
    }
    if ranges.is_empty() {
        return fail(if positions { b"missing list of byte/character positions" } else { b"missing list of fields" });
    }
    ranges.sort();
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for r in ranges {
        match merged.last_mut() {
            Some(last) if r.0 <= last.1 => last.1 = last.1.max(r.1),
            _ => merged.push(r),
        }
    }
    if complement {
        let mut inv = Vec::new();
        let mut next = 1u64;
        for &(lo, hi) in &merged {
            if lo > next {
                inv.push((next, lo - 1));
            }
            next = hi.saturating_add(1);
            if hi == u64::MAX {
                next = u64::MAX;
                break;
            }
        }
        if merged.last().is_some_and(|r| r.1 < u64::MAX) {
            inv.push((next, u64::MAX));
        }
        merged = inv;
    }
    Ok(merged)
}

fn cut(t: &Tool, args: &Argv) -> i32 {
    const KEY_COMPLEMENT: u32 = 0x100;
    const KEY_OUT_DELIM: u32 = 0x101;
    const LONGS: &[LongOpt] = &[
        long("bytes", HasArg::Req, b'b' as u32),
        long("characters", HasArg::Req, b'c' as u32),
        long("complement", HasArg::No, KEY_COMPLEMENT),
        long("delimiter", HasArg::Req, b'd' as u32),
        long("fields", HasArg::Req, b'f' as u32),
        long("only-delimited", HasArg::No, b's' as u32),
        long("output-delimiter", HasArg::Req, KEY_OUT_DELIM),
        long("zero-terminated", HasArg::No, b'z' as u32),
    ];
    let mut g = Getopt::new(t, args, b"b:c:d:f:nsz", LONGS);
    let mut mode: Option<u8> = None;
    let mut list = Vec::new();
    let mut delim: Option<u8> = None;
    let mut suppress = false;
    let mut term = b'\n';
    let mut complement = false;
    let mut out_delim: Option<Vec<u8>> = None;
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, v) => match u8::try_from(k).unwrap_or(0) {
                m @ (b'b' | b'c' | b'f') => {
                    if mode.is_some() {
                        return t.usage_error(b"only one list may be specified", 1);
                    }
                    mode = Some(if m == b'f' { b'f' } else { b'b' });
                    list = v.unwrap_or_default();
                }
                b'd' => {
                    let v = v.unwrap_or_default();
                    if v.len() > 1 {
                        return t.usage_error(b"the delimiter must be a single character", 1);
                    }
                    delim = Some(v.first().copied().unwrap_or(0));
                }
                b'n' => {}
                b's' => suppress = true,
                b'z' => term = 0,
                _ if k == KEY_COMPLEMENT => complement = true,
                _ if k == KEY_OUT_DELIM => {
                    let v = v.unwrap_or_default();
                    // Vazio vira um byte NUL, como no GNU.
                    out_delim = Some(if v.is_empty() { vec![0] } else { v });
                }
                _ => return help_or_version(t, k, 1),
            },
        }
    }
    let Some(mode) = mode else {
        return t.usage_error(b"you must specify a list of bytes, characters, or fields", 1);
    };
    if delim.is_some() && mode != b'f' {
        return t.usage_error(b"an input delimiter may be specified only when operating on fields", 1);
    }
    if suppress && mode != b'f' {
        return t.usage_error(b"suppressing non-delimited lines makes sense\n\tonly when operating on fields", 1);
    }
    let ranges = match cut_ranges(t, &list, mode != b'f', complement) {
        Ok(r) => r,
        Err(c) => return c,
    };
    let delim = delim.unwrap_or(b'\t');
    let mut files = g.operands();
    if files.is_empty() {
        files.push(b"-".to_vec());
    }
    let selected = |i: u64| ranges.iter().any(|&(lo, hi)| lo <= i && i <= hi);
    let range_start = |i: u64| ranges.iter().any(|&(lo, _)| lo == i);
    let mut out = Out::new(t);
    let mut ok = true;
    let mut line = Vec::new();
    for f in &files {
        let mut input = match Input::open(f) {
            Ok(i) => i,
            Err(e) => {
                t.error_errno(&quotef(f), e);
                ok = false;
                continue;
            }
        };
        loop {
            line.clear();
            match input.read_record(term, &mut line) {
                Ok(true) => {}
                Ok(false) => break,
                Err(e) => {
                    t.error_errno(&quotef(f), e);
                    ok = false;
                    break;
                }
            }
            let content = line.strip_suffix(&[term]).unwrap_or(&line);
            if mode == b'b' {
                let mut printed = false;
                for (k, &b) in content.iter().enumerate() {
                    let pos = k as u64 + 1;
                    if selected(pos) {
                        if printed
                            && range_start(pos)
                            && let Some(od) = &out_delim
                        {
                            out.put(od);
                        }
                        out.byte(b);
                        printed = true;
                    }
                }
                out.byte(term);
                continue;
            }
            if !content.contains(&delim) {
                if !suppress {
                    out.put(content);
                    out.byte(term);
                }
                continue;
            }
            let mut printed = false;
            for (k, field) in content.split(|&b| b == delim).enumerate() {
                if selected(k as u64 + 1) {
                    if printed {
                        match &out_delim {
                            Some(od) => out.put(od),
                            None => out.byte(delim),
                        }
                    }
                    out.put(field);
                    printed = true;
                }
            }
            out.byte(term);
        }
        input.close();
    }
    out.finish(if ok { 0 } else { 1 })
}

// =====================================================================================================
// uniq
// =====================================================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum Separate {
    None,
    Prepend,
    Separate,
    Append,
    Both,
}

fn uniq(t: &Tool, args: &Argv) -> i32 {
    const KEY_GROUP: u32 = 0x100;
    const LONGS: &[LongOpt] = &[
        long("all-repeated", HasArg::Opt, b'D' as u32),
        long("count", HasArg::No, b'c' as u32),
        long("group", HasArg::Opt, KEY_GROUP),
        long("ignore-case", HasArg::No, b'i' as u32),
        long("repeated", HasArg::No, b'd' as u32),
        long("skip-chars", HasArg::Req, b's' as u32),
        long("skip-fields", HasArg::Req, b'f' as u32),
        long("unique", HasArg::No, b'u' as u32),
        long("check-chars", HasArg::Req, b'w' as u32),
        long("zero-terminated", HasArg::No, b'z' as u32),
    ];
    const DELIMIT: &[&str] = &["none", "prepend", "separate"];
    const GROUP: &[&str] = &["prepend", "append", "separate", "both"];
    let mut g = Getopt::new(t, args, b"0123456789Dcdf:is:uw:z", LONGS);
    let (mut count, mut repeated, mut unique, mut all_repeated) = (false, false, false, None::<Separate>);
    let mut group: Option<Separate> = None;
    let (mut skip_fields, mut skip_chars, mut check_chars) = (0u64, 0u64, u64::MAX);
    let (mut ignore_case, mut term) = (false, b'\n');
    let mut prev_digit = false;
    let size_opt = |v: &[u8], msg: &[u8]| -> Result<u64, i32> {
        match xstrtoumax(v, 10, b"") {
            Ok(n) => Ok(n),
            Err(NumErr::Overflow) => Ok(u64::MAX),
            Err(NumErr::Invalid) => {
                t.error(&bv(&[v, b": ", msg]));
                Err(1)
            }
        }
    };
    loop {
        let got = g.next();
        let is_digit = matches!(&got, Got::Opt(k, _) if (u32::from(b'0')..=u32::from(b'9')).contains(k));
        match got {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, v) => {
                let absent = v.is_none();
                let v = v.unwrap_or_default();
                match u8::try_from(k).unwrap_or(0) {
                    d @ b'0'..=b'9' => {
                        if !prev_digit {
                            skip_fields = 0;
                        }
                        skip_fields = skip_fields.saturating_mul(10).saturating_add(u64::from(d - b'0'));
                    }
                    b'c' => count = true,
                    b'd' => repeated = true,
                    b'D' => {
                        all_repeated = Some(if absent {
                            Separate::None
                        } else {
                            match argmatch(&v, DELIMIT) {
                                Ok(0) => Separate::None,
                                Ok(1) => Separate::Prepend,
                                Ok(_) => Separate::Separate,
                                Err(amb) => return argmatch_fail(t, b"--all-repeated", &v, amb, DELIMIT, 1),
                            }
                        })
                    }
                    b'f' => match size_opt(&v, b"invalid number of fields to skip") {
                        Ok(n) => skip_fields = n,
                        Err(c) => return c,
                    },
                    b'i' => ignore_case = true,
                    b's' => match size_opt(&v, b"invalid number of bytes to skip") {
                        Ok(n) => skip_chars = n,
                        Err(c) => return c,
                    },
                    b'u' => unique = true,
                    b'w' => match size_opt(&v, b"invalid number of bytes to compare") {
                        Ok(n) => check_chars = n,
                        Err(c) => return c,
                    },
                    b'z' => term = 0,
                    _ if k == KEY_GROUP => {
                        group = Some(if absent {
                            Separate::Separate
                        } else {
                            match argmatch(&v, GROUP) {
                                Ok(0) => Separate::Prepend,
                                Ok(1) => Separate::Append,
                                Ok(2) => Separate::Separate,
                                Ok(_) => Separate::Both,
                                Err(amb) => return argmatch_fail(t, b"--group", &v, amb, GROUP, 1),
                            }
                        })
                    }
                    _ => return help_or_version(t, k, 1),
                }
            }
        }
        prev_digit = is_digit;
    }
    let ops = g.operands();
    if ops.len() > 2 {
        return t.usage_error(&bv(&[b"extra operand ", &quote(&ops[2])]), 1);
    }
    if group.is_some() && (count || repeated || unique || all_repeated.is_some()) {
        return t.usage_error(b"--group is mutually exclusive with -c/-d/-D/-u", 1);
    }
    if all_repeated.is_some() && count {
        return t.usage_error(b"printing all duplicated lines and repeat counts is meaningless", 1);
    }
    let in_name = ops.first().cloned().unwrap_or_else(|| b"-".to_vec());
    let mut input = match Input::open(&in_name) {
        Ok(i) => i,
        Err(e) => {
            t.error_errno(&quotef(&in_name), e);
            return 1;
        }
    };
    let mut out = Out::new(t);
    if let Some(name) = ops.get(1)
        && name != b"-"
    {
        match sys::open(name, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC, 0o666) {
            Ok(fd) => out.fd = fd,
            Err(e) => {
                t.error_errno(&quotef(name), e);
                return 1;
            }
        }
    }
    let key = |line: &[u8]| -> Vec<u8> {
        let mut i = 0;
        let mut f = 0;
        while f < skip_fields && i < line.len() {
            while i < line.len() && is_blank(line[i]) {
                i += 1;
            }
            while i < line.len() && !is_blank(line[i]) {
                i += 1;
            }
            f += 1;
        }
        i += usize::try_from(skip_chars).unwrap_or(usize::MAX).min(line.len() - i);
        let rest = &line[i..];
        let k = &rest[..rest.len().min(usize::try_from(check_chars).unwrap_or(usize::MAX))];
        if ignore_case { k.to_ascii_uppercase() } else { k.to_vec() }
    };
    let mut groups_seen = 0u64;
    let mut ok = true;
    let mut group_lines: Vec<Vec<u8>> = Vec::new();
    let mut group_key: Vec<u8> = Vec::new();
    // Um grupo é uma sequência de linhas vizinhas com a mesma chave.
    let flush = |lines: &mut Vec<Vec<u8>>, out: &mut Out, groups_seen: &mut u64| {
        if lines.is_empty() {
            return;
        }
        let n = lines.len() as u64;
        let print_all = |out: &mut Out, lines: &[Vec<u8>]| {
            for l in lines {
                out.put(l);
                out.byte(term);
            }
        };
        if let Some(sep) = group {
            let before = matches!(sep, Separate::Prepend | Separate::Both)
                || (matches!(sep, Separate::Separate | Separate::Append) && *groups_seen > 0);
            if before {
                out.byte(term);
            }
            print_all(out, lines);
            *groups_seen += 1;
        } else if let Some(sep) = all_repeated {
            if n > 1 {
                if sep == Separate::Prepend || (sep == Separate::Separate && *groups_seen > 0) {
                    out.byte(term);
                }
                print_all(out, lines);
                *groups_seen += 1;
            }
        } else if (n == 1 && !repeated) || (n > 1 && !unique) {
            if count {
                out.put(format!("{n:>7} ").as_bytes());
            }
            out.put(&lines[0]);
            out.byte(term);
        }
        lines.clear();
    };
    let mut line = Vec::new();
    loop {
        line.clear();
        match input.read_record(term, &mut line) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => {
                t.error_errno(&quotef(&in_name), e);
                ok = false;
                break;
            }
        }
        let content = line.strip_suffix(&[term]).unwrap_or(&line).to_vec();
        let k = key(&content);
        if !group_lines.is_empty() && k != group_key {
            flush(&mut group_lines, &mut out, &mut groups_seen);
        }
        if group_lines.is_empty() {
            group_key = k;
        }
        group_lines.push(content);
    }
    flush(&mut group_lines, &mut out, &mut groups_seen);
    if group == Some(Separate::Append) || group == Some(Separate::Both) {
        if groups_seen > 0 {
            out.byte(term);
        }
    }
    input.close();
    let code = if ok { 0 } else { 1 };
    let fd = out.fd;
    let code = out.finish(code);
    if fd != Fd::STDOUT {
        let _ = sys::close(fd);
    }
    code
}

// =====================================================================================================
// Modos (chmod, mkdir -m)
// =====================================================================================================

const S_ISUID: Mode = 0o4000;
const S_ISGID: Mode = 0o2000;
const S_ISVTX: Mode = 0o1000;
const ALL_BITS: Mode = 0o7777;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PermSrc {
    /// Bits fixos (`rwxst` ou octal).
    Bits,
    /// Copia as permissões atuais de uma classe (`u`, `g` ou `o`), dada pela máscara rwx dela.
    Copy(Mode),
    /// `X`: execução só se for diretório ou já houver algum bit de execução.
    XIfAnyX,
}

#[derive(Clone, Copy, Debug)]
struct ModeOp {
    op: u8,
    /// Classes afetadas; 0 = nenhuma dada (aí vale a umask).
    who: Mode,
    perm: Mode,
    src: PermSrc,
    /// Bits citados explicitamente; em diretório, setuid e setgid não citados são preservados.
    mentioned: Mode,
}

/// Modo do chmod: octal (`755`, `0644`) ou simbólico (`u+x,go-w`, `a=r`, `g=u`, `+X`...).
fn parse_mode(s: &[u8]) -> Option<Vec<ModeOp>> {
    if s.first().is_some_and(|c| (b'0'..=b'7').contains(c)) {
        let mut v: Mode = 0;
        for &c in s {
            if !(b'0'..=b'7').contains(&c) {
                return None;
            }
            v = v * 8 + Mode::from(c - b'0');
            if v > ALL_BITS {
                return None;
            }
        }
        // Com menos de 5 dígitos, setuid e setgid de diretório só mudam se aparecerem no número.
        let mentioned = if s.len() < 5 { (v & (S_ISUID | S_ISGID)) | S_ISVTX | 0o777 } else { ALL_BITS };
        return Some(vec![ModeOp { op: b'=', who: ALL_BITS, perm: v, src: PermSrc::Bits, mentioned }]);
    }
    let mut ops = Vec::new();
    let mut p = 0;
    loop {
        let mut who: Mode = 0;
        loop {
            match s.get(p) {
                Some(b'u') => who |= S_ISUID | 0o700,
                Some(b'g') => who |= S_ISGID | 0o070,
                Some(b'o') => who |= S_ISVTX | 0o007,
                Some(b'a') => who |= ALL_BITS,
                Some(b'=' | b'+' | b'-') => break,
                _ => return None,
            }
            p += 1;
        }
        loop {
            let op = s[p];
            p += 1;
            let mut mentioned: Mode = 0;
            let (perm, src) = match s.get(p) {
                Some(c @ b'0'..=b'7') => {
                    let _ = c;
                    let mut v: Mode = 0;
                    while let Some(&d @ b'0'..=b'7') = s.get(p) {
                        v = v * 8 + Mode::from(d - b'0');
                        if v > ALL_BITS {
                            return None;
                        }
                        p += 1;
                    }
                    if who != 0 || s.get(p).is_some_and(|&c| c != b',') {
                        return None;
                    }
                    who = ALL_BITS;
                    mentioned = ALL_BITS;
                    (v, PermSrc::Bits)
                }
                Some(b'u') => {
                    p += 1;
                    (0o700, PermSrc::Copy(0o700))
                }
                Some(b'g') => {
                    p += 1;
                    (0o070, PermSrc::Copy(0o070))
                }
                Some(b'o') => {
                    p += 1;
                    (0o007, PermSrc::Copy(0o007))
                }
                _ => {
                    let mut v: Mode = 0;
                    let mut src = PermSrc::Bits;
                    loop {
                        match s.get(p) {
                            Some(b'r') => v |= 0o444,
                            Some(b'w') => v |= 0o222,
                            Some(b'x') => v |= 0o111,
                            Some(b'X') => src = PermSrc::XIfAnyX,
                            Some(b's') => v |= S_ISUID | S_ISGID,
                            Some(b't') => v |= S_ISVTX,
                            _ => break,
                        }
                        p += 1;
                    }
                    (v, src)
                }
            };
            if mentioned == 0 {
                mentioned = if who != 0 { who & perm } else { perm };
            }
            ops.push(ModeOp { op, who, perm, src, mentioned });
            if !matches!(s.get(p), Some(b'=' | b'+' | b'-')) {
                break;
            }
        }
        if s.get(p) != Some(&b',') {
            break;
        }
        p += 1;
    }
    if p == s.len() { Some(ops) } else { None }
}

/// Aplica as operações a um modo. Devolve o modo novo (só os 12 bits de permissão) e os bits que as
/// operações determinaram.
fn apply_mode(old: Mode, dir: bool, umask: Mode, ops: &[ModeOp]) -> (Mode, Mode) {
    let mut new = old & ALL_BITS;
    let mut touched: Mode = 0;
    for o in ops {
        let omit = if dir { (S_ISUID | S_ISGID) & !o.mentioned } else { 0 };
        let mut value = match o.src {
            PermSrc::Bits => o.perm,
            PermSrc::Copy(mask) => {
                let v = new & mask;
                (if v & 0o444 != 0 { 0o444 } else { 0 }) | (if v & 0o222 != 0 { 0o222 } else { 0 }) | (if v & 0o111 != 0 { 0o111 } else { 0 })
            }
            PermSrc::XIfAnyX => o.perm | if new & 0o111 != 0 || dir { 0o111 } else { 0 },
        };
        value &= (if o.who != 0 { o.who } else { !umask }) & !omit;
        match o.op {
            b'=' => {
                let keep = (if o.who != 0 { !o.who } else { 0 }) | omit;
                touched |= ALL_BITS & !keep;
                new = (new & keep) | value;
            }
            b'+' => {
                touched |= value;
                new |= value;
            }
            _ => {
                touched |= value;
                new &= !value;
            }
        }
    }
    (new & ALL_BITS, touched)
}

/// `rwxr-xr-x` com s/S/t/T (sem o caractere de tipo).
fn perm_string(m: Mode) -> Vec<u8> {
    let mut s = Vec::with_capacity(9);
    for (shift, special, lower, upper) in [(6, S_ISUID, b's', b'S'), (3, S_ISGID, b's', b'S'), (0, S_ISVTX, b't', b'T')] {
        let bits = (m >> shift) & 7;
        s.push(if bits & 4 != 0 { b'r' } else { b'-' });
        s.push(if bits & 2 != 0 { b'w' } else { b'-' });
        let x = bits & 1 != 0;
        s.push(match (m & special != 0, x) {
            (true, true) => lower,
            (true, false) => upper,
            (false, true) => b'x',
            (false, false) => b'-',
        });
    }
    s
}

/// O argumento resolve para `/`? (para o `--preserve-root`).
fn is_root_dir(path: &[u8]) -> bool {
    match (sys::stat(path), sys::stat(b"/")) {
        (Ok(a), Ok(b)) => a.dev == b.dev && a.ino == b.ino,
        _ => false,
    }
}

fn warn_root(t: &Tool, path: &[u8]) {
    if path == b"/" {
        t.error(&bv(&[b"it is dangerous to operate recursively on ", &quoteaf(path)]));
    } else {
        t.error(&bv(&[b"it is dangerous to operate recursively on ", &quoteaf(path), b" (same as ", &quoteaf(b"/"), b")"]));
    }
    t.error(b"use --no-preserve-root to override this failsafe");
}

// =====================================================================================================
// chmod
// =====================================================================================================

struct ChmodOpts {
    ops: Vec<ModeOp>,
    umask: Mode,
    recursive: bool,
    force: bool,
    /// 0 = calado, 1 = só mudanças (-c), 2 = tudo (-v).
    verbosity: u8,
    surprises: bool,
    preserve_root: bool,
}

fn chmod(t: &Tool, args: &Argv) -> i32 {
    const KEY_REFERENCE: u32 = 0x100;
    const KEY_PRESERVE_ROOT: u32 = 0x101;
    const KEY_NO_PRESERVE_ROOT: u32 = 0x102;
    const LONGS: &[LongOpt] = &[
        long("changes", HasArg::No, b'c' as u32),
        long("recursive", HasArg::No, b'R' as u32),
        long("no-preserve-root", HasArg::No, KEY_NO_PRESERVE_ROOT),
        long("preserve-root", HasArg::No, KEY_PRESERVE_ROOT),
        long("quiet", HasArg::No, b'f' as u32),
        long("reference", HasArg::Req, KEY_REFERENCE),
        long("silent", HasArg::No, b'f' as u32),
        long("verbose", HasArg::No, b'v' as u32),
    ];
    let mut g = Getopt::new(t, args, b"Rcfvr::w::x::X::s::t::u::g::o::a::,::+::=::0::1::2::3::4::5::6::7::", LONGS);
    let mut mode: Option<Vec<u8>> = None;
    let mut reference: Option<Vec<u8>> = None;
    let (mut recursive, mut force, mut verbosity, mut surprises, mut preserve_root) = (false, false, 0u8, false, false);
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, v) => match u8::try_from(k).unwrap_or(0) {
                b'R' => recursive = true,
                b'c' => verbosity = 1,
                b'f' => force = true,
                b'v' => verbosity = 2,
                b'r' | b'w' | b'x' | b'X' | b's' | b't' | b'u' | b'g' | b'o' | b'a' | b',' | b'+' | b'=' | b'0'..=b'7' => {
                    // "chmod -w arquivo": o argumento inteiro é (parte d)o modo.
                    let tok = &args[g.last_arg];
                    match &mut mode {
                        Some(m) => {
                            m.push(b',');
                            m.extend_from_slice(tok);
                        }
                        None => mode = Some(tok.clone()),
                    }
                    surprises = true;
                }
                _ if k == KEY_REFERENCE => reference = v,
                _ if k == KEY_PRESERVE_ROOT => preserve_root = true,
                _ if k == KEY_NO_PRESERVE_ROOT => preserve_root = false,
                _ => return help_or_version(t, k, 1),
            },
        }
    }
    let mut ops = g.operands();
    let mut mode_from_operand = false;
    if reference.is_some() {
        if mode.is_some() {
            return t.usage_error(b"cannot combine mode and --reference options", 1);
        }
    } else if mode.is_none() && !ops.is_empty() {
        mode = Some(ops.remove(0));
        mode_from_operand = true;
    }
    if ops.is_empty() {
        if let (Some(m), true) = (&mode, mode_from_operand) {
            return t.usage_error(&bv(&[b"missing operand after ", &quote(m)]), 1);
        }
        return t.usage_error(b"missing operand", 1);
    }
    let parsed = match (&reference, &mode) {
        (Some(r), _) => match sys::stat(r) {
            Ok(st) => vec![ModeOp { op: b'=', who: ALL_BITS, perm: st.perm(), src: PermSrc::Bits, mentioned: ALL_BITS }],
            Err(e) => {
                t.error_errno(&bv(&[b"failed to get attributes of ", &quoteaf(r)]), e);
                return 1;
            }
        },
        (None, Some(m)) => match parse_mode(m) {
            Some(o) => o,
            None => return t.usage_error(&bv(&[b"invalid mode: ", &quote(m)]), 1),
        },
        (None, None) => return t.usage_error(b"missing operand", 1),
    };
    let umask = sys::current().umask(0);
    let o = ChmodOpts { ops: parsed, umask, recursive, force, verbosity, surprises, preserve_root };
    let mut out = Out::new(t);
    let mut ok = true;
    for f in &ops {
        if o.recursive && o.preserve_root && is_root_dir(f) {
            warn_root(t, f);
            ok = false;
            continue;
        }
        // Pilha explícita: (caminho, é argumento da linha de comando).
        let mut stack: Vec<(Vec<u8>, bool)> = vec![(f.clone(), true)];
        while let Some((path, top)) = stack.pop() {
            if let Some(children) = chmod_one(t, &path, top, &o, &mut out, &mut ok) {
                for c in children.into_iter().rev() {
                    stack.push((c, false));
                }
            }
        }
    }
    out.finish(if ok { 0 } else { 1 })
}

/// Muda um arquivo; num diretório com -R devolve os filhos (na ordem do diretório).
fn chmod_one(t: &Tool, path: &[u8], top: bool, o: &ChmodOpts, out: &mut Out, ok: &mut bool) -> Option<Vec<Vec<u8>>> {
    let st = match if top { sys::stat(path) } else { sys::lstat(path) } {
        Ok(st) => st,
        Err(e) => {
            if !o.force {
                t.error_errno(&bv(&[b"cannot access ", &quoteaf(path)]), e);
            }
            *ok = false;
            return None;
        }
    };
    if st.file_type() == FileType::Symlink {
        if o.verbosity == 2 {
            out.put(&bv(&[b"neither symbolic link ", &quoteaf(path), b" nor referent has been changed\n"]));
        }
        return None;
    }
    let dir = is_dir(&st);
    let old = st.perm();
    let (new, _) = apply_mode(st.mode, dir, o.umask, &o.ops);
    let result = sys::current().fchmodat(Fd::CWD, path, new, AtFlags::empty());
    match &result {
        Ok(()) => {
            let changed = old != new;
            if o.verbosity == 2 || (o.verbosity == 1 && changed) {
                let msg = if changed {
                    bv(&[b"mode of ", &quoteaf(path), b" changed from ", format!("{old:04o}").as_bytes(), b" (", &perm_string(old), b") to ", format!("{new:04o}").as_bytes(), b" (", &perm_string(new), b")\n"])
                } else {
                    bv(&[b"mode of ", &quoteaf(path), b" retained as ", format!("{new:04o}").as_bytes(), b" (", &perm_string(new), b")\n"])
                };
                out.put(&msg);
            }
        }
        Err(e) => {
            if !o.force {
                t.error_errno(&bv(&[b"changing permissions of ", &quoteaf(path)]), *e);
            }
            if o.verbosity == 2 {
                out.put(&bv(&[b"failed to change mode of ", &quoteaf(path), b" from ", format!("{old:04o}").as_bytes(), b" (", &perm_string(old), b") to ", format!("{new:04o}").as_bytes(), b" (", &perm_string(new), b")\n"]));
            }
            *ok = false;
        }
    }
    if result.is_ok() && o.surprises {
        // "chmod -w": avisa quando a umask fez o resultado diferir do que se esperaria sem ela.
        let (naive, _) = apply_mode(st.mode, dir, 0, &o.ops);
        if new & !naive != 0 {
            out.flush();
            t.error(&bv(&[&quotef(path), b": new permissions are ", &perm_string(new), b", not ", &perm_string(naive)]));
            *ok = false;
        }
    }
    if !(o.recursive && dir) {
        return None;
    }
    match sys::read_dir(path) {
        Ok(entries) => Some(entries.into_iter().map(|e| path_join(path, &e.name)).collect()),
        Err(e) => {
            if !o.force {
                out.flush();
                t.error_errno(&bv(&[b"cannot read directory ", &quoteaf(path)]), e);
            }
            *ok = false;
            None
        }
    }
}

// =====================================================================================================
// mkdir
// =====================================================================================================

fn mkdir(t: &Tool, args: &Argv) -> i32 {
    const KEY_CONTEXT: u32 = b'Z' as u32;
    const LONGS: &[LongOpt] = &[
        long("context", HasArg::Opt, KEY_CONTEXT),
        long("mode", HasArg::Req, b'm' as u32),
        long("parents", HasArg::No, b'p' as u32),
        long("verbose", HasArg::No, b'v' as u32),
    ];
    let mut g = Getopt::new(t, args, b"pm:vZ", LONGS);
    let (mut parents, mut verbose, mut mode_arg) = (false, false, None::<Vec<u8>>);
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, v) => match u8::try_from(k).unwrap_or(0) {
                b'p' => parents = true,
                b'v' => verbose = true,
                b'm' => mode_arg = v,
                b'Z' => return t.unsupported(b"-Z", 1),
                _ => return help_or_version(t, k, 1),
            },
        }
    }
    let ops = g.operands();
    if ops.is_empty() {
        return t.usage_error(b"missing operand", 1);
    }
    let umask = current_umask();
    let (mode, bits) = match &mode_arg {
        Some(m) => match parse_mode(m) {
            Some(parsed) => apply_mode(0o777, true, umask, &parsed),
            None => {
                t.error(&bv(&[b"invalid mode ", &quote(m)]));
                return 1;
            }
        },
        None => (0o777, 0),
    };
    let s = sys::current();
    let mut out = Out::new(t);
    let mut ok = true;
    let announce = |out: &mut Out, p: &[u8]| {
        if verbose {
            out.put(&bv(&[&t.name, b": created directory ", &quoteaf(p), b"\n"]));
        }
    };
    for dir in &ops {
        if parents {
            // Ancestrais: 0777 menos a umask, mas sempre com u+wx. Cada um termina numa barra que
            // ainda tem componente depois.
            let comps: Vec<usize> = dir
                .iter()
                .enumerate()
                .filter(|&(i, &c)| c == b'/' && i > 0 && dir[i - 1] != b'/' && dir[i..].iter().any(|&b| b != b'/'))
                .map(|(i, _)| i)
                .collect();
            let mut failed = false;
            for end in comps {
                let prefix = &dir[..end];
                let last = &prefix[last_component(prefix)..];
                if last == b"." || last == b".." {
                    continue;
                }
                let old = s.umask(umask & !0o300);
                let r = s.mkdirat(Fd::CWD, prefix, 0o777);
                s.umask(old);
                match r {
                    Ok(()) => {
                        out.flush();
                        announce(&mut out, prefix);
                    }
                    Err(Errno::EEXIST) => match sys::stat(prefix) {
                        Ok(st) if is_dir(&st) => {}
                        Ok(_) => {
                            out.flush();
                            t.error_errno(&bv(&[b"cannot create directory ", &quote(prefix)]), Errno::ENOTDIR);
                            failed = true;
                            break;
                        }
                        Err(e) => {
                            out.flush();
                            t.error_errno(&bv(&[b"cannot create directory ", &quote(prefix)]), e);
                            failed = true;
                            break;
                        }
                    },
                    Err(e) => {
                        out.flush();
                        t.error_errno(&bv(&[b"cannot create directory ", &quote(prefix)]), e);
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                ok = false;
                continue;
            }
        }
        match mkdir_final(dir, mode, bits, mode_arg.is_some(), umask) {
            Ok(()) => announce(&mut out, dir),
            Err(Errno::EEXIST) if parents && sys::stat(dir).is_ok_and(|st| is_dir(&st)) => {}
            Err(e) => {
                out.flush();
                t.error_errno(&bv(&[b"cannot create directory ", &quote(dir)]), e);
                ok = false;
            }
        }
    }
    out.finish(if ok { 0 } else { 1 })
}

/// Cria o diretório final. Com `-m` o resultado é o modo pedido, não afetado pela umask; com bits
/// especiais ele nasce sem escrita para grupo e outros, e depois só os bits que o modo determinou
/// são acertados (é o que o GNU faz).
fn mkdir_final(dir: &[u8], mode: Mode, bits: Mode, explicit: bool, umask: Mode) -> SysResult<()> {
    let s = sys::current();
    if !explicit {
        return s.mkdirat(Fd::CWD, dir, 0o777);
    }
    let special = bits & (S_ISUID | S_ISGID) != 0 || mode & S_ISVTX != 0;
    let mut create = mode;
    if special {
        create &= !0o022;
    }
    // O mkdir do Linux ignora setuid e setgid no modo pedido.
    create &= !(S_ISUID | S_ISGID);
    let old = s.umask(umask & !mode);
    let r = s.mkdirat(Fd::CWD, dir, create);
    s.umask(old);
    r?;
    if bits != 0
        && let Ok(st) = sys::stat(dir)
        && (st.perm() ^ mode) & bits != 0
    {
        let _ = s.fchmodat(Fd::CWD, dir, mode | (st.perm() & !bits), AtFlags::empty());
    }
    Ok(())
}

// =====================================================================================================
// touch
// =====================================================================================================

/// O fuso é UTC? (as conversões de data daqui só sabem UTC).
fn tz_is_utc() -> bool {
    match sys::getenv("TZ") {
        None => true,
        Some(tz) => {
            let tz = tz.strip_prefix(b":").unwrap_or(&tz);
            matches!(tz, b"" | b"UTC" | b"UTC0" | b"GMT" | b"GMT0" | b"UCT" | b"Etc/UTC" | b"Etc/GMT" | b"Universal" | b"Zulu" | b"UTC+0" | b"UTC-0")
        }
    }
}

/// Dias desde 1970-01-01 no calendário gregoriano proléptico.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Ano (UTC) de um instante em segundos desde a época.
fn year_of(sec: i64) -> i64 {
    let days = sec.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + i64::from(month <= 2)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}

fn civil_to_epoch(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> Option<i64> {
    if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) || !(0..=23).contains(&h) || !(0..=59).contains(&mi) || !(0..=60).contains(&s) {
        return None;
    }
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s)
}

fn digits_value(s: &[u8]) -> Option<i64> {
    if s.is_empty() || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(s).ok()?.parse().ok()
}

/// `touch -t [[CC]YY]MMDDhhmm[.ss]` em UTC.
fn parse_touch_stamp(s: &[u8], now_year: i64) -> Option<i64> {
    let (main, secs) = match s.iter().position(|&b| b == b'.') {
        Some(p) => {
            let sec = &s[p + 1..];
            if sec.len() != 2 {
                return None;
            }
            (&s[..p], digits_value(sec)?)
        }
        None => (s, 0),
    };
    if !main.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let (year, rest) = match main.len() {
        8 => (now_year, main),
        10 => {
            let yy = digits_value(&main[..2])?;
            (if yy >= 69 { 1900 + yy } else { 2000 + yy }, &main[2..])
        }
        12 => (digits_value(&main[..4])?, &main[4..]),
        _ => return None,
    };
    let f = |a: usize| digits_value(&rest[a..a + 2]);
    civil_to_epoch(year, f(0)?, f(2)?, f(4)?, f(6)?, secs)
}

/// `touch -d`: `@SEGUNDOS[.frac]`, `now`, `AAAA-MM-DD`, `AAAA-MM-DD HH:MM[:SS[.frac]]` (ou com `T`).
fn parse_touch_date(s: &[u8], now: TimeSpec) -> Option<TimeSpec> {
    let s = s.trim_ascii();
    if s == b"now" {
        return Some(now);
    }
    if let Some(e) = s.strip_prefix(b"@") {
        let (v, n) = strtod_prefix(e)?;
        if n != e.len() || !v.is_finite() {
            return None;
        }
        let sec = v.floor();
        return Some(TimeSpec { sec: sec as i64, nsec: ((v - sec) * 1e9).round().min(999_999_999.0) as u32 });
    }
    let (date, time) = match s.iter().position(|&b| b == b' ' || b == b'T') {
        Some(p) => (&s[..p], s[p + 1..].trim_ascii()),
        None => (s, &b""[..]),
    };
    let parts: Vec<&[u8]> = date.split(|&b| b == b'-').collect();
    if parts.len() != 3 {
        return None;
    }
    let (y, mo, d) = (digits_value(parts[0])?, digits_value(parts[1])?, digits_value(parts[2])?);
    let (mut h, mut mi, mut sec, mut nsec) = (0, 0, 0, 0u32);
    if !time.is_empty() {
        let time = time.strip_suffix(b"Z").unwrap_or(time);
        let tparts: Vec<&[u8]> = time.split(|&b| b == b':').collect();
        if !(2..=3).contains(&tparts.len()) {
            return None;
        }
        h = digits_value(tparts[0])?;
        mi = digits_value(tparts[1])?;
        if let Some(sp) = tparts.get(2) {
            let (whole, frac) = match sp.iter().position(|&b| b == b'.') {
                Some(p) => (&sp[..p], &sp[p + 1..]),
                None => (*sp, &b""[..]),
            };
            sec = digits_value(whole)?;
            if !frac.is_empty() {
                if !frac.iter().all(u8::is_ascii_digit) {
                    return None;
                }
                let mut ns = 0u32;
                for k in 0..9 {
                    ns = ns * 10 + frac.get(k).map_or(0, |&b| u32::from(b - b'0'));
                }
                nsec = ns;
            }
        }
    }
    Some(TimeSpec { sec: civil_to_epoch(y, mo, d, h, mi, sec)?, nsec })
}

fn touch(t: &Tool, args: &Argv) -> i32 {
    const KEY_TIME: u32 = 0x100;
    const LONGS: &[LongOpt] = &[
        long("date", HasArg::Req, b'd' as u32),
        long("no-create", HasArg::No, b'c' as u32),
        long("no-dereference", HasArg::No, b'h' as u32),
        long("reference", HasArg::Req, b'r' as u32),
        long("time", HasArg::Req, KEY_TIME),
    ];
    const TIMES: &[&str] = &["atime", "access", "use", "mtime", "modify"];
    let mut g = Getopt::new(t, args, b"acd:fhmr:t:", LONGS);
    let (mut ch_a, mut ch_m, mut no_create, mut no_deref) = (false, false, false, false);
    let (mut date, mut reference, mut stamp) = (None::<Vec<u8>>, None::<Vec<u8>>, None::<Vec<u8>>);
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, v) => match u8::try_from(k).unwrap_or(0) {
                b'a' => ch_a = true,
                b'c' => no_create = true,
                b'd' => date = v,
                b'f' => {}
                b'h' => no_deref = true,
                b'm' => ch_m = true,
                b'r' => reference = v,
                b't' => stamp = v,
                _ if k == KEY_TIME => {
                    let v = v.unwrap_or_default();
                    match argmatch(&v, TIMES) {
                        Ok(i) if i < 3 => ch_a = true,
                        Ok(_) => ch_m = true,
                        Err(amb) => return argmatch_fail(t, b"--time", &v, amb, TIMES, 1),
                    }
                }
                _ => return help_or_version(t, k, 1),
            },
        }
    }
    // -d com -r é permitido no GNU (data relativa à referência); -t não combina com nenhum.
    if stamp.is_some() && (date.is_some() || reference.is_some()) {
        return t.usage_error(b"cannot specify times from more than one source", 1);
    }
    if date.is_some() && reference.is_some() {
        return t.unsupported(b"-d together with -r", 1);
    }
    let s = sys::current();
    let now = s.clock_gettime(Clock::Realtime).unwrap_or_default();
    let mut times: Option<(TimeSpec, TimeSpec)> = None;
    if let Some(r) = &reference {
        let st = if no_deref { sys::lstat(r) } else { sys::stat(r) };
        match st {
            Ok(st) => times = Some((st.atime, st.mtime)),
            Err(e) => {
                t.error_errno(&bv(&[b"failed to get attributes of ", &quoteaf(r)]), e);
                return 1;
            }
        }
    }
    if let Some(st) = &stamp {
        if !tz_is_utc() {
            return t.unsupported(b"-t with TZ other than UTC", 1);
        }
        match parse_touch_stamp(st, year_of(now.sec)) {
            Some(sec) => times = Some((TimeSpec { sec, nsec: 0 }, TimeSpec { sec, nsec: 0 })),
            None => {
                t.error(&bv(&[b"invalid date format ", &quote(st)]));
                return 1;
            }
        }
    }
    if let Some(d) = &date {
        if !tz_is_utc() {
            return t.unsupported(b"-d with TZ other than UTC", 1);
        }
        match parse_touch_date(d, now) {
            Some(ts) => times = Some((ts, ts)),
            None => {
                t.error(&bv(&[b"unsupported date format ", &quote(d), b" (shell test miniutils)"]));
                return 1;
            }
        }
    }
    let (mut at, mut mt) = match times {
        Some((a, m)) => (SetTime::At(a), SetTime::At(m)),
        None => (SetTime::Now, SetTime::Now),
    };
    if ch_a && !ch_m {
        mt = SetTime::Omit;
    }
    if ch_m && !ch_a {
        at = SetTime::Omit;
    }
    let ops = g.operands();
    if ops.is_empty() {
        return t.usage_error(b"missing file operand", 1);
    }
    let mut ok = true;
    for f in &ops {
        if f == b"-" {
            match s.futimens(Fd::STDOUT, at, mt) {
                Ok(()) => {}
                Err(Errno::EBADF) if no_create => {}
                Err(e) => {
                    t.error_errno(&bv(&[b"setting times of ", &quoteaf(f)]), e);
                    ok = false;
                }
            }
            continue;
        }
        let mut open_err = None;
        let mut fd = None;
        if !(no_create || no_deref) {
            match sys::open(f, OFlags::WRONLY | OFlags::CREAT | OFlags::NONBLOCK | OFlags::NOCTTY | OFlags::CLOEXEC, 0o666) {
                Ok(x) => fd = Some(x),
                Err(e) => open_err = Some(e),
            }
        }
        let r = match fd {
            Some(x) => {
                let r = s.futimens(x, at, mt);
                let _ = s.close(x);
                r
            }
            None => s.utimensat(Fd::CWD, f, at, mt, if no_deref { AtFlags::SYMLINK_NOFOLLOW } else { AtFlags::empty() }),
        };
        if let Err(e) = r {
            match open_err {
                Some(oe) if oe != Errno::EISDIR => t.error_errno(&bv(&[b"cannot touch ", &quoteaf(f)]), oe),
                _ if no_create && e == Errno::ENOENT => continue,
                _ => t.error_errno(&bv(&[b"setting times of ", &quoteaf(f)]), e),
            }
            ok = false;
        }
    }
    if ok { 0 } else { 1 }
}

// =====================================================================================================
// rm
// =====================================================================================================

struct RmOpts {
    force: bool,
    recursive: bool,
    dir: bool,
    verbose: bool,
    preserve_root: bool,
}

fn rm(t: &Tool, args: &Argv) -> i32 {
    const KEY_PRESERVE_ROOT: u32 = 0x100;
    const KEY_NO_PRESERVE_ROOT: u32 = 0x101;
    const KEY_ONE_FS: u32 = 0x102;
    const KEY_INTERACTIVE: u32 = 0x103;
    const LONGS: &[LongOpt] = &[
        long("dir", HasArg::No, b'd' as u32),
        long("force", HasArg::No, b'f' as u32),
        long("interactive", HasArg::Opt, KEY_INTERACTIVE),
        long("no-preserve-root", HasArg::No, KEY_NO_PRESERVE_ROOT),
        long("one-file-system", HasArg::No, KEY_ONE_FS),
        long("preserve-root", HasArg::Opt, KEY_PRESERVE_ROOT),
        long("recursive", HasArg::No, b'r' as u32),
        long("verbose", HasArg::No, b'v' as u32),
    ];
    let mut g = Getopt::new(t, args, b"dfirvIR", LONGS);
    let mut o = RmOpts { force: false, recursive: false, dir: false, verbose: false, preserve_root: true };
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, v) => match u8::try_from(k).unwrap_or(0) {
                b'd' => o.dir = true,
                b'f' => o.force = true,
                b'r' | b'R' => o.recursive = true,
                b'v' => o.verbose = true,
                b'i' => return t.unsupported(b"-i", 1),
                b'I' => return t.unsupported(b"-I", 1),
                _ if k == KEY_INTERACTIVE => {
                    if v.as_deref() == Some(b"never") || v.as_deref() == Some(b"no") || v.as_deref() == Some(b"none") {
                        continue;
                    }
                    return t.unsupported(b"--interactive", 1);
                }
                _ if k == KEY_PRESERVE_ROOT => o.preserve_root = true,
                _ if k == KEY_NO_PRESERVE_ROOT => o.preserve_root = false,
                _ if k == KEY_ONE_FS => {}
                _ => return help_or_version(t, k, 1),
            },
        }
    }
    let ops = g.operands();
    if ops.is_empty() {
        if o.force {
            return 0;
        }
        return t.usage_error(b"missing operand", 1);
    }
    let mut out = Out::new(t);
    let mut ok = true;
    for f in &ops {
        if !rm_top(t, f, &o, &mut out) {
            ok = false;
        }
    }
    out.finish(if ok { 0 } else { 1 })
}

fn rm_top(t: &Tool, path: &[u8], o: &RmOpts, out: &mut Out) -> bool {
    let st = match sys::lstat(path) {
        Ok(st) => st,
        Err(e) => {
            if o.force && e == Errno::ENOENT {
                return true;
            }
            out.flush();
            t.error_errno(&bv(&[b"cannot remove ", &quoteaf(path)]), e);
            return false;
        }
    };
    if !is_dir(&st) {
        return rm_unlink(t, path, o, out);
    }
    let empty = || sys::read_dir(path).is_ok_and(|e| e.is_empty());
    if !o.recursive && !(o.dir && empty()) {
        out.flush();
        t.error_errno(&bv(&[b"cannot remove ", &quoteaf(path)]), if o.dir { Errno::ENOTEMPTY } else { Errno::EISDIR });
        return false;
    }
    let last = &path[last_component(path)..];
    let last = &last[..base_len(last)];
    if last == b"." || last == b".." {
        out.flush();
        t.error(&bv(&[b"refusing to remove ", &quoteaf(b"."), b" or ", &quoteaf(b".."), b" directory: skipping ", &quoteaf(path)]));
        return false;
    }
    if o.recursive && o.preserve_root && is_root_dir(path) {
        out.flush();
        warn_root(t, path);
        return false;
    }
    rm_tree(t, path, o, out)
}

fn rm_unlink(t: &Tool, path: &[u8], o: &RmOpts, out: &mut Out) -> bool {
    match sys::current().unlinkat(Fd::CWD, path, AtFlags::empty()) {
        Ok(()) => {
            if o.verbose {
                out.put(&bv(&[b"removed ", &quoteaf(path), b"\n"]));
            }
            true
        }
        Err(e) if o.force && e == Errno::ENOENT => true,
        Err(e) => {
            out.flush();
            t.error_errno(&bv(&[b"cannot remove ", &quoteaf(path)]), e);
            false
        }
    }
}

/// Remove uma árvore em pós-ordem, com pilha explícita. Se algo lá dentro falha, os ancestrais ficam
/// (sem mensagem extra), como no GNU.
fn rm_tree(t: &Tool, root: &[u8], o: &RmOpts, out: &mut Out) -> bool {
    struct Frame {
        path: Vec<u8>,
        children: Vec<Vec<u8>>,
        failed: bool,
    }
    let list = |p: &[u8]| -> SysResult<Vec<Vec<u8>>> {
        let mut entries = sys::read_dir(p)?;
        entries.reverse();
        Ok(entries.into_iter().map(|e| path_join(p, &e.name)).collect())
    };
    let s = sys::current();
    let mut ok = true;
    let mut stack: Vec<Frame> = Vec::new();
    match list(root) {
        Ok(children) => stack.push(Frame { path: root.to_vec(), children, failed: false }),
        Err(e) => {
            out.flush();
            t.error_errno(&bv(&[b"cannot remove ", &quoteaf(root)]), e);
            return false;
        }
    }
    while let Some(top) = stack.last_mut() {
        if let Some(child) = top.children.pop() {
            match sys::lstat(&child) {
                Ok(st) if is_dir(&st) => match list(&child) {
                    Ok(children) => stack.push(Frame { path: child, children, failed: false }),
                    Err(e) => {
                        out.flush();
                        t.error_errno(&bv(&[b"cannot remove ", &quoteaf(&child)]), e);
                        top.failed = true;
                        ok = false;
                    }
                },
                Ok(_) => {
                    if !rm_unlink(t, &child, o, out) {
                        top.failed = true;
                        ok = false;
                    }
                }
                Err(e) if e == Errno::ENOENT && o.force => {}
                Err(e) => {
                    out.flush();
                    t.error_errno(&bv(&[b"cannot remove ", &quoteaf(&child)]), e);
                    top.failed = true;
                    ok = false;
                }
            }
            continue;
        }
        let Some(done) = stack.pop() else { break };
        if done.failed {
            if let Some(parent) = stack.last_mut() {
                parent.failed = true;
            }
            continue;
        }
        match s.unlinkat(Fd::CWD, &done.path, AtFlags::REMOVEDIR) {
            Ok(()) => {
                if o.verbose {
                    out.put(&bv(&[b"removed directory ", &quoteaf(&done.path), b"\n"]));
                }
            }
            Err(e) => {
                out.flush();
                t.error_errno(&bv(&[b"cannot remove ", &quoteaf(&done.path)]), e);
                ok = false;
                if let Some(parent) = stack.last_mut() {
                    parent.failed = true;
                }
            }
        }
    }
    ok
}

// =====================================================================================================
// readlink
// =====================================================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum Canon {
    /// `-f`: todos os componentes menos o último precisam existir.
    AllButLast,
    /// `-e`: todos precisam existir.
    Existing,
    /// `-m`: nenhum precisa existir.
    Missing,
}

/// Caminho absoluto sem `.`, `..` nem links simbólicos.
fn canonicalize(path: &[u8], mode: Canon) -> SysResult<Vec<u8>> {
    if path.is_empty() {
        return Err(Errno::ENOENT);
    }
    let s = sys::current();
    let mut resolved: Vec<u8> = if path.starts_with(b"/") { b"/".to_vec() } else { s.getcwd()? };
    let mut pending: Vec<Vec<u8>> = path.split(|&b| b == b'/').filter(|c| !c.is_empty()).map(<[u8]>::to_vec).collect();
    pending.reverse();
    let trailing_slash = path.ends_with(b"/");
    let mut links = 0;
    let pop = |r: &mut Vec<u8>| {
        if r.len() > 1 {
            let cut = r.iter().rposition(|&b| b == b'/').unwrap_or(0);
            r.truncate(cut.max(1));
        }
    };
    while let Some(comp) = pending.pop() {
        if comp == b"." {
            continue;
        }
        if comp == b".." {
            pop(&mut resolved);
            continue;
        }
        let is_last = pending.is_empty();
        if !resolved.ends_with(b"/") {
            resolved.push(b'/');
        }
        resolved.extend_from_slice(&comp);
        match sys::lstat(&resolved) {
            Ok(st) => {
                if st.file_type() == FileType::Symlink {
                    links += 1;
                    if links > 40 {
                        return Err(Errno::ELOOP);
                    }
                    let target = s.readlinkat(Fd::CWD, &resolved)?;
                    pop(&mut resolved);
                    if target.starts_with(b"/") {
                        resolved = b"/".to_vec();
                    }
                    for c in target.split(|&b| b == b'/').filter(|c| !c.is_empty()).rev() {
                        pending.push(c.to_vec());
                    }
                } else if !is_dir(&st) && (!is_last || trailing_slash) && mode != Canon::Missing {
                    return Err(Errno::ENOTDIR);
                }
            }
            Err(e) => {
                let ok = match mode {
                    Canon::Missing => e == Errno::ENOENT || e == Errno::ENOTDIR,
                    Canon::AllButLast => e == Errno::ENOENT && is_last,
                    Canon::Existing => false,
                };
                if !ok {
                    return Err(e);
                }
            }
        }
    }
    Ok(resolved)
}

fn readlink(t: &Tool, args: &Argv) -> i32 {
    const LONGS: &[LongOpt] = &[
        long("canonicalize", HasArg::No, b'f' as u32),
        long("canonicalize-existing", HasArg::No, b'e' as u32),
        long("canonicalize-missing", HasArg::No, b'm' as u32),
        long("no-newline", HasArg::No, b'n' as u32),
        long("quiet", HasArg::No, b'q' as u32),
        long("silent", HasArg::No, b's' as u32),
        long("verbose", HasArg::No, b'v' as u32),
        long("zero", HasArg::No, b'z' as u32),
    ];
    let mut g = Getopt::new(t, args, b"efmnqsvz", LONGS);
    let (mut mode, mut no_newline, mut verbose, mut zero) = (None::<Canon>, false, false, false);
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, _) => match u8::try_from(k).unwrap_or(0) {
                b'e' => mode = Some(Canon::Existing),
                b'f' => mode = Some(Canon::AllButLast),
                b'm' => mode = Some(Canon::Missing),
                b'n' => no_newline = true,
                b'q' | b's' => verbose = false,
                b'v' => verbose = true,
                b'z' => zero = true,
                _ => return help_or_version(t, k, 1),
            },
        }
    }
    let ops = g.operands();
    if ops.is_empty() {
        return t.usage_error(b"missing operand", 1);
    }
    if ops.len() > 1 && no_newline {
        t.error(b"ignoring --no-newline with multiple arguments");
        no_newline = false;
    }
    let mut out = Out::new(t);
    let mut ok = true;
    let s = sys::current();
    for f in &ops {
        let r = match mode {
            Some(m) => canonicalize(f, m),
            None => s.readlinkat(Fd::CWD, f),
        };
        match r {
            Ok(v) => {
                out.put(&v);
                if !no_newline {
                    out.byte(if zero { 0 } else { b'\n' });
                }
            }
            Err(e) => {
                if verbose {
                    out.flush();
                    t.error_errno(&quotef(f), e);
                }
                ok = false;
            }
        }
    }
    out.finish(if ok { 0 } else { 1 })
}

// =====================================================================================================
// ls
// =====================================================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum Indicator {
    None,
    Slash,
    FileType,
    Classify,
}

struct LsOpts {
    /// 0 = esconde ocultos, 1 = -A, 2 = -a.
    all: u8,
    dirs_as_files: bool,
    reverse: bool,
    recursive: bool,
    indicator: Indicator,
}

fn ls(t: &Tool, args: &Argv) -> i32 {
    const KEY_COLOR: u32 = 0x100;
    const KEY_INDICATOR: u32 = 0x101;
    const KEY_CLASSIFY: u32 = 0x102;
    const KEY_FILE_TYPE: u32 = 0x103;
    const KEY_GROUP_DIRS: u32 = 0x104;
    const KEY_OTHER: u32 = 0x1FF;
    const LONGS: &[LongOpt] = &[
        long("all", HasArg::No, b'a' as u32),
        long("almost-all", HasArg::No, b'A' as u32),
        long("classify", HasArg::Opt, KEY_CLASSIFY),
        long("color", HasArg::Opt, KEY_COLOR),
        long("colour", HasArg::Opt, KEY_COLOR),
        long("directory", HasArg::No, b'd' as u32),
        long("file-type", HasArg::No, KEY_FILE_TYPE),
        long("group-directories-first", HasArg::No, KEY_GROUP_DIRS),
        long("indicator-style", HasArg::Req, KEY_INDICATOR),
        long("recursive", HasArg::No, b'R' as u32),
        long("reverse", HasArg::No, b'r' as u32),
        long("author", HasArg::No, KEY_OTHER),
        long("escape", HasArg::No, b'b' as u32),
        long("block-size", HasArg::Req, KEY_OTHER),
        long("ignore-backups", HasArg::No, b'B' as u32),
        long("dired", HasArg::No, b'D' as u32),
        long("format", HasArg::Req, KEY_OTHER),
        long("full-time", HasArg::No, KEY_OTHER),
        long("no-group", HasArg::No, b'G' as u32),
        long("human-readable", HasArg::No, b'h' as u32),
        long("si", HasArg::No, KEY_OTHER),
        long("dereference-command-line", HasArg::No, b'H' as u32),
        long("hide", HasArg::Req, KEY_OTHER),
        long("hyperlink", HasArg::Opt, KEY_OTHER),
        long("inode", HasArg::No, b'i' as u32),
        long("ignore", HasArg::Req, b'I' as u32),
        long("kibibytes", HasArg::No, b'k' as u32),
        long("dereference", HasArg::No, b'L' as u32),
        long("literal", HasArg::No, b'N' as u32),
        long("numeric-uid-gid", HasArg::No, b'n' as u32),
        long("hide-control-chars", HasArg::No, b'q' as u32),
        long("show-control-chars", HasArg::No, KEY_OTHER),
        long("quote-name", HasArg::No, b'Q' as u32),
        long("quoting-style", HasArg::Req, KEY_OTHER),
        long("size", HasArg::No, b's' as u32),
        long("sort", HasArg::Req, KEY_OTHER),
        long("time", HasArg::Req, KEY_OTHER),
        long("time-style", HasArg::Req, KEY_OTHER),
        long("tabsize", HasArg::Req, b'T' as u32),
        long("width", HasArg::Req, b'w' as u32),
        long("context", HasArg::No, b'Z' as u32),
        long("zero", HasArg::No, KEY_OTHER),
    ];
    const STYLES: &[&str] = &["none", "slash", "file-type", "classify"];
    const WHEN: &[&str] = &["always", "yes", "force", "never", "no", "none", "auto", "tty", "if-tty"];
    let mut g = Getopt::new(t, args, b"abcdfghiklmnopqrstuvw:xABCDFGHI:LNQRST:UXZ1", LONGS);
    let mut o = LsOpts { all: 0, dirs_as_files: false, reverse: false, recursive: false, indicator: Indicator::None };
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 2;
            }
            Got::Opt(k, v) => match u8::try_from(k).unwrap_or(0) {
                b'1' => {}
                b'a' => o.all = 2,
                b'A' => o.all = 1,
                b'd' => o.dirs_as_files = true,
                b'F' => o.indicator = Indicator::Classify,
                b'p' => o.indicator = Indicator::Slash,
                b'r' => o.reverse = true,
                b'R' => o.recursive = true,
                c if k < 0x100 => return t.unsupported(&[b'-', c], 2),
                _ if k == KEY_FILE_TYPE => o.indicator = Indicator::FileType,
                _ if k == KEY_INDICATOR => {
                    let v = v.unwrap_or_default();
                    match argmatch(&v, STYLES) {
                        Ok(0) => o.indicator = Indicator::None,
                        Ok(1) => o.indicator = Indicator::Slash,
                        Ok(2) => o.indicator = Indicator::FileType,
                        Ok(_) => o.indicator = Indicator::Classify,
                        Err(amb) => return argmatch_fail(t, b"--indicator-style", &v, amb, STYLES, 2),
                    }
                }
                _ if k == KEY_CLASSIFY || k == KEY_COLOR => {
                    // Sem terminal, "auto" equivale a "never"; "always" pediria o formato do terminal.
                    let when = match &v {
                        None => 0,
                        Some(v) => match argmatch(v, WHEN) {
                            Ok(i) => i,
                            Err(amb) => {
                                let name: &[u8] = if k == KEY_COLOR { b"--color" } else { b"--classify" };
                                return argmatch_fail(t, name, v, amb, WHEN, 2);
                            }
                        },
                    };
                    let always = when < 3;
                    if k == KEY_CLASSIFY {
                        if always {
                            o.indicator = Indicator::Classify;
                        }
                    } else if always {
                        return t.unsupported(b"--color=always", 2);
                    }
                }
                _ if k == KEY_GROUP_DIRS || k == KEY_OTHER => {
                    let tok = args.get(g.last_arg).cloned().unwrap_or_default();
                    return t.unsupported(&tok, 2);
                }
                _ => return help_or_version(t, k, 2),
            },
        }
    }
    let ops = g.operands();
    let n_operands = ops.len();
    let ops = if ops.is_empty() { vec![b".".to_vec()] } else { ops };
    let mut status = 0;
    let mut files: Vec<(Vec<u8>, Stat)> = Vec::new();
    let mut dirs: Vec<Vec<u8>> = Vec::new();
    // Sem -d, -F nem -l, um link simbólico na linha de comando que aponta para diretório é seguido.
    let deref_dirs = !o.dirs_as_files && o.indicator != Indicator::Classify;
    for op in &ops {
        let st = if deref_dirs {
            match sys::stat(op) {
                Ok(st) if is_dir(&st) => Ok(st),
                Ok(_) => sys::lstat(op),
                Err(e) if e == Errno::ENOENT || e == Errno::ELOOP => sys::lstat(op),
                Err(e) => Err(e),
            }
        } else {
            sys::lstat(op)
        };
        match st {
            Ok(st) if is_dir(&st) && !o.dirs_as_files => dirs.push(op.clone()),
            Ok(st) => files.push((op.clone(), st)),
            Err(e) => {
                t.error_errno(&bv(&[b"cannot access ", &quoteaf(op)]), e);
                status = 2;
            }
        }
    }
    let order = |a: &[u8], b: &[u8]| if o.reverse { b.cmp(a) } else { a.cmp(b) };
    files.sort_by(|a, b| order(&a.0, &b.0));
    dirs.sort_by(|a, b| order(a, b));
    let mut out = Out::new(t);
    for (name, st) in &files {
        out.put(name);
        out.put(ls_indicator(st, o.indicator));
        out.byte(b'\n');
    }
    if !files.is_empty() && !dirs.is_empty() {
        out.byte(b'\n');
    }
    let print_dir_name = !(files.is_empty() && n_operands <= 1 && dirs.len() == 1);
    // Pilha de (diretório, veio da linha de comando); o primeiro em ordem sai primeiro.
    let mut stack: Vec<(Vec<u8>, bool)> = dirs.into_iter().rev().map(|d| (d, true)).collect();
    let mut first = true;
    while let Some((dir, cmdline)) = stack.pop() {
        let entries = match sys::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                out.flush();
                t.error_errno(&bv(&[b"cannot open directory ", &quoteaf(&dir)]), e);
                status = if cmdline { 2 } else { status.max(1) };
                continue;
            }
        };
        if o.recursive || print_dir_name {
            if !first {
                out.byte(b'\n');
            }
            out.put(&dir);
            out.put(b":\n");
        }
        first = false;
        let mut names: Vec<(Vec<u8>, FileType)> = entries
            .into_iter()
            .filter(|e| o.all > 0 || !e.name.starts_with(b"."))
            .map(|e| (e.name, e.kind))
            .collect();
        if o.all == 2 {
            names.push((b".".to_vec(), FileType::Directory));
            names.push((b"..".to_vec(), FileType::Directory));
        }
        names.sort_by(|a, b| order(&a.0, &b.0));
        for (name, kind) in &names {
            out.put(name);
            if o.indicator != Indicator::None {
                match sys::lstat(&path_join(&dir, name)) {
                    Ok(st) => out.put(ls_indicator(&st, o.indicator)),
                    Err(_) => {
                        if *kind == FileType::Directory {
                            out.byte(b'/');
                        }
                    }
                }
            }
            out.byte(b'\n');
        }
        if o.recursive {
            for (name, kind) in names.iter().rev() {
                if *kind == FileType::Directory && name != b"." && name != b".." {
                    stack.push((path_join(&dir, name), false));
                }
            }
        }
    }
    out.finish(status)
}

fn ls_indicator(st: &Stat, style: Indicator) -> &'static [u8] {
    match (style, st.file_type()) {
        (Indicator::None, _) => b"",
        (_, FileType::Directory) => b"/",
        (Indicator::Slash, _) => b"",
        (_, FileType::Symlink) => b"@",
        (_, FileType::Fifo) => b"|",
        (_, FileType::Socket) => b"=",
        (Indicator::Classify, FileType::Regular) if st.perm() & 0o111 != 0 => b"*",
        _ => b"",
    }
}

// =====================================================================================================
// env
// =====================================================================================================

/// `env -S`: divide a string em argumentos (aspas simples e duplas, escapes, `${VAR}`, comentário
/// com `#`).
fn env_split(t: &Tool, s: &[u8], env: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, i32> {
    let die = |msg: &[u8]| -> Result<Vec<Vec<u8>>, i32> {
        t.error(msg);
        Err(125)
    };
    let mut args: Vec<Vec<u8>> = Vec::new();
    let mut cur: Option<Vec<u8>> = None;
    let (mut sq, mut dq) = (false, false);
    let mut sep = true;
    let mut i = 0;
    let start_arg = |cur: &mut Option<Vec<u8>>, args: &mut Vec<Vec<u8>>, sep: &mut bool| {
        if *sep {
            if let Some(c) = cur.take() {
                args.push(c);
            }
            *cur = Some(Vec::new());
            *sep = false;
        }
    };
    let mut terminated = false;
    while i < s.len() {
        let c = s[i];
        let mut ch = c;
        match c {
            b'\'' if !dq => {
                sq = !sq;
                start_arg(&mut cur, &mut args, &mut sep);
                i += 1;
                continue;
            }
            b'"' if !sq => {
                dq = !dq;
                start_arg(&mut cur, &mut args, &mut sep);
                i += 1;
                continue;
            }
            b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r' if !sq && !dq => {
                sep = true;
                while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') {
                    i += 1;
                }
                continue;
            }
            b'#' if sep && !sq && !dq => {
                terminated = true;
                break;
            }
            b'\\' if !(sq && !matches!(s.get(i + 1), Some(b'\\' | b'\''))) => {
                i += 1;
                let Some(&n) = s.get(i) else {
                    return die(b"invalid backslash at end of string in -S");
                };
                ch = match n {
                    b'"' | b'#' | b'$' | b'\'' | b'\\' => n,
                    b'_' if !dq => {
                        sep = true;
                        i += 1;
                        continue;
                    }
                    b'_' => b' ',
                    b'c' if dq => return die(b"'\\c' must not appear in double-quoted -S string"),
                    b'c' => {
                        terminated = true;
                        break;
                    }
                    b'f' => 0x0c,
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'v' => 0x0b,
                    other => return die(&bv(&[b"invalid sequence '\\", &[other], b"' in -S"])),
                };
            }
            b'$' if !sq => {
                let rest = &s[i..];
                let name_ok = rest.starts_with(b"${")
                    && rest.get(2).is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
                    && rest.iter().position(|&b| b == b'}').is_some_and(|end| rest[2..end].iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_'));
                if !name_ok {
                    return die(&bv(&[b"only ${VARNAME} expansion is supported, error at: ", rest]));
                }
                let end = rest.iter().position(|&b| b == b'}').unwrap_or(rest.len());
                if let Some(v) = env_lookup(env, &rest[2..end]) {
                    start_arg(&mut cur, &mut args, &mut sep);
                    if let Some(c) = cur.as_mut() {
                        c.extend_from_slice(v);
                    }
                }
                i += end + 1;
                continue;
            }
            _ => {}
        }
        start_arg(&mut cur, &mut args, &mut sep);
        if let Some(c) = cur.as_mut() {
            c.push(ch);
        }
        i += 1;
    }
    if !terminated && (sq || dq) {
        return die(b"no terminating quote in -S string");
    }
    if let Some(c) = cur {
        args.push(c);
    }
    Ok(args)
}

fn env(t: &Tool, args: &Argv) -> i32 {
    const KEY_SIGNAL: u32 = 0x100;
    const LONGS: &[LongOpt] = &[
        long("block-signal", HasArg::Opt, KEY_SIGNAL),
        long("chdir", HasArg::Req, b'C' as u32),
        long("debug", HasArg::No, b'v' as u32),
        long("default-signal", HasArg::Opt, KEY_SIGNAL),
        long("ignore-environment", HasArg::No, b'i' as u32),
        long("ignore-signal", HasArg::Opt, KEY_SIGNAL),
        long("list-signal-handling", HasArg::No, KEY_SIGNAL),
        long("null", HasArg::No, b'0' as u32),
        long("split-string", HasArg::Req, b'S' as u32),
        long("unset", HasArg::Req, b'u' as u32),
    ];
    let s = sys::current();
    let initial_env = s.environ();
    let mut cur: Vec<Vec<u8>> = args.to_vec();
    let (mut ignore_env, mut nul) = (false, false);
    let mut unset: Vec<Vec<u8>> = Vec::new();
    let mut newdir: Option<Vec<u8>> = None;
    let mut ops: Vec<Vec<u8>>;
    'parse: loop {
        let mut g = Getopt::new(t, &cur, b"+C:iS:u:v0", LONGS);
        loop {
            match g.next() {
                Got::End => {
                    ops = g.operands();
                    break 'parse;
                }
                Got::Fail => {
                    t.try_help();
                    return 125;
                }
                Got::Opt(k, v) => match u8::try_from(k).unwrap_or(0) {
                    b'i' => ignore_env = true,
                    b'0' => nul = true,
                    b'u' => unset.push(v.unwrap_or_default()),
                    b'C' => newdir = v,
                    b'v' => return t.unsupported(b"-v", 125),
                    b'S' => {
                        let mut split = match env_split(t, &v.unwrap_or_default(), &initial_env) {
                            Ok(a) => a,
                            Err(c) => return c,
                        };
                        let idx = g.idx;
                        split.extend(cur[idx..].iter().cloned());
                        cur = split;
                        continue 'parse;
                    }
                    _ if k == KEY_SIGNAL => {
                        let tok = cur.get(g.last_arg).cloned().unwrap_or_default();
                        return t.unsupported(&tok, 125);
                    }
                    _ => return help_or_version(t, k, 125),
                },
            }
        }
    }
    if ops.first().is_some_and(|o| o == b"-") {
        ignore_env = true;
        ops.remove(0);
    }
    let mut envv: Vec<Vec<u8>> = if ignore_env { Vec::new() } else { initial_env.clone() };
    if !ignore_env {
        for name in &unset {
            if name.is_empty() || name.contains(&b'=') {
                t.error_errno(&bv(&[b"cannot unset ", &quote(name)]), Errno::EINVAL);
                return 125;
            }
            envv.retain(|kv| !(kv.starts_with(name) && kv.get(name.len()) == Some(&b'=')));
        }
    }
    let mut k = 0;
    while k < ops.len() && ops[k].contains(&b'=') {
        let kv = &ops[k];
        let eq = kv.iter().position(|&b| b == b'=').unwrap_or(0);
        let name = &kv[..=eq];
        match envv.iter_mut().find(|e| e.starts_with(name)) {
            Some(slot) => *slot = kv.clone(),
            None => envv.push(kv.clone()),
        }
        k += 1;
    }
    let cmd = &ops[k..];
    if cmd.is_empty() {
        if newdir.is_some() {
            return t.usage_error(b"must specify command with --chdir (-C)", 125);
        }
        let mut out = Out::new(t);
        for e in &envv {
            out.put(e);
            out.byte(if nul { 0 } else { b'\n' });
        }
        return out.finish(0);
    }
    if nul {
        return t.usage_error(b"cannot specify --null (-0) with command", 125);
    }
    if let Some(d) = &newdir
        && let Err(e) = s.chdir(d)
    {
        t.error_errno(&bv(&[b"cannot change directory to ", &quoteaf(d)]), e);
        return 125;
    }
    let path = env_lookup(&envv, b"PATH").map(<[u8]>::to_vec);
    let err = match exec_search(&cmd[0], path.as_deref(), cmd, |p, argv| Err::<(), Errno>(s.execve(p, argv, Some(&envv)))) {
        Ok(()) => Errno::ENOENT,
        Err(e) => e,
    };
    t.error_errno(&quote(&cmd[0]), err);
    if err == Errno::ENOENT {
        if cmd[0].contains(&b' ') {
            t.error(b"use -[v]S to pass options in shebang lines");
        }
        127
    } else {
        126
    }
}

// =====================================================================================================
// timeout
// =====================================================================================================

fn timeout(t: &Tool, args: &Argv) -> i32 {
    const LONGS: &[LongOpt] = &[
        long("foreground", HasArg::No, b'f' as u32),
        long("kill-after", HasArg::Req, b'k' as u32),
        long("preserve-status", HasArg::No, b'p' as u32),
        long("signal", HasArg::Req, b's' as u32),
        long("verbose", HasArg::No, b'v' as u32),
    ];
    let mut g = Getopt::new(t, args, b"+fk:ps:v", LONGS);
    let (mut foreground, mut preserve, mut verbose) = (false, false, false);
    let mut kill_after: Option<f64> = None;
    let mut term = Signal::SIGTERM;
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 125;
            }
            Got::Opt(k, v) => {
                let v = v.unwrap_or_default();
                match u8::try_from(k).unwrap_or(0) {
                    b'f' => foreground = true,
                    b'p' => preserve = true,
                    b'v' => verbose = true,
                    b'k' => match parse_interval(&v) {
                        Some(d) => kill_after = Some(d),
                        None => return t.usage_error(&bv(&[b"invalid time interval ", &quote(&v)]), 125),
                    },
                    b's' => match std::str::from_utf8(&v).ok().and_then(Signal::parse).filter(|s| s.is_valid()) {
                        Some(sig) => term = sig,
                        None => return t.usage_error(&bv(&[&quote(&v), b": invalid signal"]), 125),
                    },
                    _ => return help_or_version(t, k, 125),
                }
            }
        }
    }
    let ops = g.operands();
    if ops.len() < 2 {
        t.try_help();
        return 125;
    }
    let Some(secs) = parse_interval(&ops[0]) else {
        return t.usage_error(&bv(&[b"invalid time interval ", &quote(&ops[0])]), 125);
    };
    let s = sys::current();
    if !foreground {
        let _ = s.setpgid(0, 0);
    }
    // Sinais que o timeout repassa ao comando (no filho o exec os devolve ao padrão).
    let forwarded = [Signal::SIGINT, Signal::SIGQUIT, Signal::SIGHUP, Signal::SIGTERM, term];
    for sig in forwarded {
        if !sig.is_uncatchable() {
            let _ = s.sigaction(sig, SigDisposition::Catch);
        }
    }
    let cmd = &ops[1..];
    let path = s.getenv(b"PATH");
    let spawned = exec_search(&cmd[0], path.as_deref(), cmd, |p, argv| {
        s.spawn(SpawnSpec { path: p.to_vec(), argv: argv.to_vec(), attrs: ProcAttrs { reset_signals: forwarded.to_vec(), ..ProcAttrs::default() } })
    });
    let child = match spawned {
        Ok(pid) => pid,
        Err(e) => {
            t.error_errno(&bv(&[b"failed to run command ", &quote(&cmd[0])]), e);
            return if e == Errno::ENOENT { 127 } else { 126 };
        }
    };
    let send = |sig: Signal| {
        let _ = s.kill(KillTarget::Pid(child), sig);
        if !foreground {
            // O próprio timeout ignora o sinal que manda ao grupo (SIGKILL não dá: ele morre junto).
            if !sig.is_uncatchable() {
                let _ = s.sigaction(sig, SigDisposition::Ignore);
            }
            let _ = s.kill(KillTarget::Group(0), sig);
            if sig != Signal::SIGKILL && sig != Signal::SIGCONT {
                let _ = s.kill(KillTarget::Pid(child), Signal::SIGCONT);
                let _ = s.kill(KillTarget::Group(0), Signal::SIGCONT);
            }
        }
    };
    let announce = |sig: Signal| {
        if verbose {
            let name = sig.name().unwrap_or_default();
            t.error(&bv(&[b"sending signal ", name.as_bytes(), b" to command ", &quote(&cmd[0])]));
        }
    };
    let start = monotonic();
    let deadline = if secs > 0.0 { seconds_to_duration(secs).map(|d| start + d) } else { None };
    let mut kill_deadline: Option<Duration> = None;
    let mut timed_out = false;
    let poll = Duration::from_millis(10);
    let status = loop {
        match s.wait4(WaitTarget::Pid(child), WaitOptions::NOHANG) {
            Ok(Some((_, st))) => break st,
            Ok(None) | Err(Errno::EINTR) => {}
            Err(e) => {
                t.error_errno(b"error waiting for command", e);
                return 125;
            }
        }
        for sig in s.take_caught_signals() {
            announce(sig);
            send(sig);
        }
        let now = monotonic();
        if !timed_out && deadline.is_some_and(|d| now >= d) {
            timed_out = true;
            announce(term);
            send(term);
            if let Some(ka) = kill_after.and_then(seconds_to_duration) {
                kill_deadline = Some(now + ka);
            }
        }
        if kill_deadline.is_some_and(|d| now >= d) {
            kill_deadline = None;
            announce(Signal::SIGKILL);
            send(Signal::SIGKILL);
        }
        let mut nap = poll;
        for d in [deadline.filter(|_| !timed_out), kill_deadline].into_iter().flatten() {
            nap = nap.min(d.saturating_sub(now));
        }
        let _ = s.nanosleep(nap.max(Duration::from_micros(100)));
    };
    let mut code = match status {
        WaitStatus::Exited(c) => c,
        WaitStatus::Signaled { signal, .. } => {
            if !timed_out {
                // Morre do mesmo sinal que matou o comando.
                let _ = s.sigaction(signal, SigDisposition::Default);
                let _ = s.kill(KillTarget::Pid(s.getpid()), signal);
            }
            if timed_out && signal == Signal::SIGKILL {
                preserve = true;
            }
            128 + signal.0
        }
        other => other.shell_status(),
    };
    if timed_out && !preserve {
        code = 124;
    }
    code
}

// =====================================================================================================
// printf de ponto flutuante (para o seq)
// =====================================================================================================

#[derive(Clone, Default, Debug)]
struct FloatSpec {
    minus: bool,
    plus: bool,
    space: bool,
    alt: bool,
    zero: bool,
    width: usize,
    prec: Option<usize>,
    conv: u8,
}

/// `%f` sem sinal.
fn fmt_fixed(a: f64, p: usize, alt: bool) -> String {
    let mut s = format!("{a:.p$}");
    if alt && p == 0 {
        s.push('.');
    }
    s
}

/// `%e` sem sinal: expoente com sinal e pelo menos dois dígitos.
fn fmt_exp(a: f64, p: usize, alt: bool) -> String {
    let s = format!("{a:.p$e}");
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let mut out = mant.to_string();
    if alt && p == 0 {
        out.push('.');
    }
    out.push('e');
    out.push(if exp < 0 { '-' } else { '+' });
    out.push_str(&format!("{:02}", exp.abs()));
    out
}

/// `%g` sem sinal.
fn fmt_general(a: f64, p: usize, alt: bool) -> String {
    let p = p.max(1);
    let x: i32 = if a == 0.0 {
        0
    } else {
        let s = format!("{:.*e}", p - 1, a);
        s.split_once('e').and_then(|(_, e)| e.parse().ok()).unwrap_or(0)
    };
    let strip = |s: String| -> String {
        if alt || !s.contains('.') {
            return s;
        }
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    if (x as i64) < p as i64 && x >= -4 {
        strip(fmt_fixed(a, (p as i64 - 1 - x as i64) as usize, alt))
    } else {
        let e = fmt_exp(a, p - 1, alt);
        let (mant, exp) = e.split_once('e').unwrap_or((&e, ""));
        format!("{}e{exp}", strip(mant.to_string()))
    }
}

/// Uma conversão `%[flags][largura][.precisão](f|e|g|F|E|G)` do printf.
fn c_float(v: f64, sp: &FloatSpec) -> Vec<u8> {
    let neg = v.is_sign_negative() && !v.is_nan();
    let a = v.abs();
    let finite = a.is_finite();
    let mut body = if a.is_infinite() {
        "inf".to_string()
    } else if a.is_nan() {
        "nan".to_string()
    } else {
        let p = sp.prec.unwrap_or(6);
        match sp.conv.to_ascii_lowercase() {
            b'f' => fmt_fixed(a, p, sp.alt),
            b'e' => fmt_exp(a, p, sp.alt),
            _ => fmt_general(a, p, sp.alt),
        }
    };
    if sp.conv.is_ascii_uppercase() {
        body = body.to_uppercase();
    }
    let sign = if neg {
        "-"
    } else if sp.plus {
        "+"
    } else if sp.space {
        " "
    } else {
        ""
    };
    let len = sign.len() + body.len();
    let pad = sp.width.saturating_sub(len);
    let mut out = String::with_capacity(len + pad);
    if sp.minus {
        out.push_str(sign);
        out.push_str(&body);
        out.push_str(&" ".repeat(pad));
    } else if sp.zero && finite {
        out.push_str(sign);
        out.push_str(&"0".repeat(pad));
        out.push_str(&body);
    } else {
        out.push_str(&" ".repeat(pad));
        out.push_str(sign);
        out.push_str(&body);
    }
    out.into_bytes()
}

// =====================================================================================================
// seq
// =====================================================================================================

/// Decimal exato: `mant × 10^-scale`.
#[derive(Clone, Copy, Debug)]
struct Dec {
    mant: i128,
    scale: u32,
}

impl Dec {
    fn rescale(self, scale: u32) -> Option<Dec> {
        let f = 10i128.checked_pow(scale.checked_sub(self.scale)?)?;
        Some(Dec { mant: self.mant.checked_mul(f)?, scale })
    }
}

struct SeqArg {
    value: f64,
    exact: Option<Dec>,
    /// Largura em caracteres com que o operando aparece (para o -w).
    width: i64,
    /// Casas decimais; `None` = indeterminada (hexadecimal, infinito): o formato vira `%g`.
    prec: Option<i64>,
}

fn seq_arg(t: &Tool, s: &[u8]) -> Result<SeqArg, i32> {
    let invalid = || -> Result<SeqArg, i32> { Err(t.usage_error(&bv(&[b"invalid floating point argument: ", &quote(s)]), 1)) };
    let Some((value, n)) = strtod_prefix(s) else { return invalid() };
    if n != s.len() {
        return invalid();
    }
    if value.is_nan() {
        return Err(t.usage_error(&bv(&[b"invalid ", &quote(b"not-a-number"), b" argument: ", &quote(s)]), 1));
    }
    let mut text = s;
    while let Some((&c, rest)) = text.split_first() {
        if is_space(c) || c == b'+' {
            text = rest;
        } else {
            break;
        }
    }
    let mut width = text.len() as i64;
    let hex = text.iter().any(|&b| b == b'x' || b == b'X');
    if hex || !value.is_finite() {
        return Ok(SeqArg { value, exact: None, width, prec: None });
    }
    let dot = text.iter().position(|&b| b == b'.');
    let e = text.iter().position(|&b| b == b'e' || b == b'E');
    let mut prec: i64 = 0;
    if let Some(d) = dot {
        let frac = &text[d + 1..e.unwrap_or(text.len())];
        prec = frac.len() as i64;
        if frac.is_empty() {
            width -= 1;
        } else if d == 0 || !text[d - 1].is_ascii_digit() {
            width += 1;
        }
    }
    if let Some(e) = e {
        let exp: i64 = std::str::from_utf8(&text[e + 1..]).ok().and_then(|x| x.parse().ok()).unwrap_or(0);
        prec = (prec - exp).max(0);
        // Com expoente, a largura é a do número escrito por extenso.
        width = fmt_fixed(value.abs(), prec as usize, false).len() as i64 + i64::from(value.is_sign_negative());
        return Ok(SeqArg { value, exact: None, width, prec: Some(prec) });
    }
    // Sem expoente: guarda o valor exato.
    let mut mant: i128 = 0;
    let mut ok = true;
    for &c in text.iter().filter(|c| c.is_ascii_digit()) {
        match mant.checked_mul(10).and_then(|m| m.checked_add(i128::from(c - b'0'))) {
            Some(m) => mant = m,
            None => {
                ok = false;
                break;
            }
        }
    }
    if text.first() == Some(&b'-') {
        mant = -mant;
    }
    let exact = ok.then_some(Dec { mant, scale: prec as u32 });
    Ok(SeqArg { value, exact, width, prec: Some(prec) })
}

/// Formato do `seq -f`: prefixo, a conversão e sufixo (com `%%` já resolvido).
fn seq_format(t: &Tool, fmt: &[u8]) -> Result<(Vec<u8>, FloatSpec, Vec<u8>), i32> {
    let die = |what: &[u8]| -> Result<(Vec<u8>, FloatSpec, Vec<u8>), i32> {
        t.error(&bv(&[b"format ", &quote(fmt), what]));
        Err(1)
    };
    let mut prefix = Vec::new();
    let mut i = 0;
    loop {
        match fmt.get(i) {
            None => return die(b" has no % directive"),
            Some(b'%') if fmt.get(i + 1) == Some(&b'%') => {
                prefix.push(b'%');
                i += 2;
            }
            Some(b'%') => break,
            Some(&c) => {
                prefix.push(c);
                i += 1;
            }
        }
    }
    i += 1;
    let mut sp = FloatSpec::default();
    while let Some(&c) = fmt.get(i) {
        match c {
            b'-' => sp.minus = true,
            b'+' => sp.plus = true,
            b' ' => sp.space = true,
            b'#' => sp.alt = true,
            b'0' => sp.zero = true,
            b'\'' => {}
            _ => break,
        }
        i += 1;
    }
    while let Some(&c) = fmt.get(i).filter(|c| c.is_ascii_digit()) {
        sp.width = sp.width.saturating_mul(10).saturating_add(usize::from(c - b'0'));
        i += 1;
    }
    if fmt.get(i) == Some(&b'.') {
        i += 1;
        let mut p = 0usize;
        while let Some(&c) = fmt.get(i).filter(|c| c.is_ascii_digit()) {
            p = p.saturating_mul(10).saturating_add(usize::from(c - b'0'));
            i += 1;
        }
        sp.prec = Some(p);
    }
    if fmt.get(i) == Some(&b'L') {
        i += 1;
    }
    let Some(&conv) = fmt.get(i) else { return die(b" ends in %") };
    if !b"efgaEFGA".contains(&conv) {
        return die(&bv(&[b" has unknown %", &[conv], b" directive"]));
    }
    if conv == b'a' || conv == b'A' {
        return Err(t.unsupported(b"-f %a", 1));
    }
    sp.conv = conv;
    i += 1;
    let mut suffix = Vec::new();
    while i < fmt.len() {
        if fmt[i] == b'%' {
            if fmt.get(i + 1) == Some(&b'%') {
                suffix.push(b'%');
                i += 2;
                continue;
            }
            return die(b" has too many % directives");
        }
        suffix.push(fmt[i]);
        i += 1;
    }
    Ok((prefix, sp, suffix))
}

fn seq(t: &Tool, args: &Argv) -> i32 {
    const LONGS: &[LongOpt] = &[
        long("equal-width", HasArg::No, b'w' as u32),
        long("format", HasArg::Req, b'f' as u32),
        long("separator", HasArg::Req, b's' as u32),
    ];
    let mut g = Getopt::new(t, args, b"+f:s:w", LONGS);
    g.stop_at = Some(|a: &[u8]| a.len() > 1 && a[0] == b'-' && (a[1] == b'.' || a[1].is_ascii_digit()));
    let (mut fmt, mut sep, mut equal) = (None::<Vec<u8>>, b"\n".to_vec(), false);
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, v) => match u8::try_from(k).unwrap_or(0) {
                b'f' => fmt = v,
                b's' => sep = v.unwrap_or_default(),
                b'w' => equal = true,
                _ => return help_or_version(t, k, 1),
            },
        }
    }
    let ops = g.operands();
    if ops.is_empty() {
        return t.usage_error(b"missing operand", 1);
    }
    if ops.len() > 3 {
        return t.usage_error(&bv(&[b"extra operand ", &quote(&ops[3])]), 1);
    }
    let user_fmt = match &fmt {
        Some(f) => match seq_format(t, f) {
            Ok(x) => Some(x),
            Err(c) => return c,
        },
        None => None,
    };
    let mut parsed = Vec::new();
    for op in &ops {
        match seq_arg(t, op) {
            Ok(a) => parsed.push(a),
            Err(c) => return c,
        }
    }
    let one = || SeqArg { value: 1.0, exact: Some(Dec { mant: 1, scale: 0 }), width: 1, prec: Some(0) };
    let (first, step, last) = match parsed.len() {
        1 => (one(), one(), parsed.remove(0)),
        2 => {
            let last = parsed.remove(1);
            (parsed.remove(0), one(), last)
        }
        _ => {
            let last = parsed.remove(2);
            let step = parsed.remove(1);
            (parsed.remove(0), step, last)
        }
    };
    if step.value == 0.0 {
        let text = if ops.len() == 3 { &ops[1] } else { &ops[0] };
        return t.usage_error(&bv(&[b"invalid Zero increment value: ", &quote(text)]), 1);
    }
    if user_fmt.is_some() && equal {
        return t.usage_error(b"format string may not be specified when printing equal width strings", 1);
    }
    let mut out = Out::new(t);
    // Formato padrão: casas decimais do primeiro e do passo; com -w, preenchido com zeros até a
    // largura do primeiro ou do último operando.
    let default_spec = match (first.prec, step.prec, last.prec) {
        (Some(fp), Some(sp), Some(lp)) => {
            let prec = fp.max(sp);
            let mut spec = FloatSpec { prec: Some(prec as usize), conv: b'f', ..FloatSpec::default() };
            if equal {
                let fw = first.width + (prec - fp) + i64::from(fp == 0 && prec > 0);
                let lw = last.width + (prec - lp) - i64::from(lp > 0 && prec == 0) + i64::from(lp == 0 && prec > 0);
                spec.width = fw.max(lw).max(0) as usize;
                spec.zero = true;
            }
            spec
        }
        _ => FloatSpec { conv: b'g', ..FloatSpec::default() },
    };
    if user_fmt.is_none()
        && default_spec.conv == b'f'
        && let (Some(f), Some(s), Some(l)) = (first.exact, step.exact, last.exact)
        && let Some(()) = seq_exact(f, s, l, &default_spec, &sep, &mut out)
    {
        return out.finish(0);
    }
    let (prefix, spec, suffix) = user_fmt.unwrap_or((Vec::new(), default_spec, Vec::new()));
    let render = |v: f64| bv(&[&prefix, &c_float(v, &spec), &suffix]);
    let core = |v: f64| c_float(v, &spec);
    let (f, s, l) = (first.value, step.value, last.value);
    let beyond = |x: f64| if s < 0.0 { x < l } else { l < x };
    if beyond(f) {
        return out.finish(0);
    }
    let mut x = f;
    let mut i = 1f64;
    loop {
        out.put(&render(x));
        let next = f + i * s;
        i += 1.0;
        if beyond(next) {
            // Se o número logo depois do fim imprime igual ao último operando (e diferente do
            // anterior), ele entra: arredondamento de ponto flutuante não deve encurtar a sequência.
            let next_txt = core(next);
            let extra = strtod_prefix(&next_txt).is_some_and(|(v, n)| n == next_txt.len() && v == l) && next_txt != core(x);
            if !extra {
                break;
            }
            out.put(&sep);
            out.put(&render(next));
            break;
        }
        out.put(&sep);
        x = next;
    }
    out.put(b"\n");
    out.finish(0)
}

/// A sequência em decimal exato; `None` se algo não couber em 128 bits (aí vale o ponto flutuante).
fn seq_exact(first: Dec, step: Dec, last: Dec, spec: &FloatSpec, sep: &[u8], out: &mut Out) -> Option<()> {
    let scale = first.scale.max(step.scale).max(last.scale);
    let (f, s, l) = (first.rescale(scale)?.mant, step.rescale(scale)?.mant, last.rescale(scale)?.mant);
    let prec = spec.prec.unwrap_or(0) as u32;
    let div = 10i128.checked_pow(scale.checked_sub(prec)?)?;
    let beyond = |x: i128| if s < 0 { x < l } else { l < x };
    // Valida o fim antes de escrever qualquer coisa, para não misturar com o caminho em float.
    let count = if beyond(f) { 0 } else { (l - f) / s + 1 };
    f.checked_add(s.checked_mul(count)?)?;
    let mut buf = Vec::new();
    let mut x = f;
    for k in 0..count {
        if k > 0 {
            buf.extend_from_slice(sep);
        }
        let v = x / div;
        let neg = v < 0;
        let digits = v.unsigned_abs().to_string();
        let digits = if digits.len() <= prec as usize { format!("{}{digits}", "0".repeat(prec as usize + 1 - digits.len())) } else { digits };
        let (int, frac) = digits.split_at(digits.len() - prec as usize);
        let mut body = int.to_string();
        if prec > 0 {
            body.push('.');
            body.push_str(frac);
        }
        let len = body.len() + usize::from(neg);
        if neg {
            buf.push(b'-');
        }
        if spec.zero && spec.width > len {
            buf.extend(std::iter::repeat_n(b'0', spec.width - len));
        }
        buf.extend_from_slice(body.as_bytes());
        out.put(&buf);
        buf.clear();
        x += s;
    }
    if count > 0 {
        out.put(b"\n");
    }
    Some(())
}

// =====================================================================================================
// od
// =====================================================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OdKind {
    Named,
    Char,
    Signed,
    Unsigned,
    Octal,
    Hex,
}

#[derive(Clone, Copy, Debug)]
struct OdSpec {
    kind: OdKind,
    size: usize,
    width: usize,
    trailer: bool,
}

fn od_spec(kind: OdKind, size: usize) -> OdSpec {
    let idx = match size {
        1 => 0,
        2 => 1,
        4 => 2,
        _ => 3,
    };
    let width = match kind {
        OdKind::Named | OdKind::Char => 3,
        OdKind::Signed => [4, 6, 11, 20][idx],
        OdKind::Unsigned => [3, 5, 10, 20][idx],
        OdKind::Octal => [3, 6, 11, 22][idx],
        OdKind::Hex => [2, 4, 8, 16][idx],
    };
    OdSpec { kind, size, width, trailer: false }
}

/// `-t TIPO` (pode trazer vários tipos colados: `-t x1z`, `-t cd1`).
fn od_parse_type(t: &Tool, s: &[u8], specs: &mut Vec<OdSpec>) -> Result<(), i32> {
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        i += 1;
        let mut spec = match c {
            b'a' => od_spec(OdKind::Named, 1),
            b'c' => od_spec(OdKind::Char, 1),
            b'd' | b'o' | b'u' | b'x' => {
                let kind = match c {
                    b'd' => OdKind::Signed,
                    b'o' => OdKind::Octal,
                    b'u' => OdKind::Unsigned,
                    _ => OdKind::Hex,
                };
                let size = match s.get(i) {
                    Some(b'C') => {
                        i += 1;
                        1
                    }
                    Some(b'S') => {
                        i += 1;
                        2
                    }
                    Some(b'I') => {
                        i += 1;
                        4
                    }
                    Some(b'L') => {
                        i += 1;
                        8
                    }
                    Some(d) if d.is_ascii_digit() => {
                        let start = i;
                        while s.get(i).is_some_and(u8::is_ascii_digit) {
                            i += 1;
                        }
                        let n: usize = std::str::from_utf8(&s[start..i]).ok().and_then(|x| x.parse().ok()).unwrap_or(0);
                        if ![1, 2, 4, 8].contains(&n) {
                            t.error(&bv(&[b"invalid type string ", &quote(s), b";\nthis system doesn't provide a ", &num(n), b"-byte integral type"]));
                            return Err(1);
                        }
                        n
                    }
                    _ => 4,
                };
                od_spec(kind, size)
            }
            b'f' => return Err(t.unsupported(b"-t f", 1)),
            other => {
                t.error(&bv(&[b"invalid character '", &[other], b"' in type string ", &quote(s)]));
                return Err(1);
            }
        };
        if s.get(i) == Some(&b'z') {
            spec.trailer = true;
            i += 1;
        }
        specs.push(spec);
    }
    Ok(())
}

/// Lê de vários arquivos em sequência, como uma entrada só.
struct ChainReader<'a> {
    t: &'a Tool,
    files: Vec<Vec<u8>>,
    next: usize,
    cur: Option<Input>,
    ok: bool,
}

impl ChainReader<'_> {
    /// Enche `buf` o quanto der; devolve quantos bytes vieram (menos que o pedido só no fim).
    fn read(&mut self, buf: &mut [u8]) -> usize {
        let mut got = 0;
        while got < buf.len() {
            if self.cur.is_none() {
                let Some(name) = self.files.get(self.next).cloned() else { return got };
                self.next += 1;
                match Input::open(&name) {
                    Ok(i) => self.cur = Some(i),
                    Err(e) => {
                        self.t.error_errno(&quotef(&name), e);
                        self.ok = false;
                    }
                }
                continue;
            }
            let Some(input) = self.cur.as_mut() else { continue };
            match input.fill() {
                Ok(true) => {
                    let avail = &input.buf[input.pos..input.end];
                    let n = avail.len().min(buf.len() - got);
                    buf[got..got + n].copy_from_slice(&avail[..n]);
                    input.pos += n;
                    got += n;
                }
                Ok(false) => {
                    if let Some(i) = self.cur.take() {
                        i.close();
                    }
                }
                Err(e) => {
                    let name = self.files[self.next - 1].clone();
                    self.t.error_errno(&quotef(&name), e);
                    self.ok = false;
                    if let Some(i) = self.cur.take() {
                        i.close();
                    }
                }
            }
        }
        got
    }
}

fn od(t: &Tool, args: &Argv) -> i32 {
    const KEY_TRADITIONAL: u32 = 0x100;
    const KEY_ENDIAN: u32 = 0x101;
    const LONGS: &[LongOpt] = &[
        long("address-radix", HasArg::Req, b'A' as u32),
        long("endian", HasArg::Req, KEY_ENDIAN),
        long("format", HasArg::Req, b't' as u32),
        long("output-duplicates", HasArg::No, b'v' as u32),
        long("read-bytes", HasArg::Req, b'N' as u32),
        long("skip-bytes", HasArg::Req, b'j' as u32),
        long("strings", HasArg::Opt, b'S' as u32),
        long("traditional", HasArg::No, KEY_TRADITIONAL),
        long("width", HasArg::Opt, b'w' as u32),
    ];
    let mut g = Getopt::new(t, args, b"A:aBbcDdeFfHhIij:LlN:OoS:st:vw::Xx", LONGS);
    let mut specs: Vec<OdSpec> = Vec::new();
    let mut radix = b'o';
    let (mut skip, mut limit, mut width, mut dups) = (0u64, None::<u64>, None::<usize>, false);
    let size_arg = |v: &[u8], opt: &[u8]| -> Result<u64, i32> {
        match xstrtoumax(v, 0, b"bEGKkMmPQRTYZ0") {
            Ok(n) => Ok(n),
            Err(NumErr::Overflow) => {
                t.error(&bv(&[b"invalid ", opt, b" argument '", v, b"': Value too large for defined data type"]));
                Err(1)
            }
            Err(NumErr::Invalid) => {
                let digits = v.iter().take_while(|b| b.is_ascii_digit()).count();
                let what: &[u8] = if digits > 0 && digits < v.len() { b"invalid suffix in " } else { b"invalid " };
                t.error(&bv(&[what, opt, b" argument '", v, b"'"]));
                Err(1)
            }
        }
    };
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, v) => {
                let v = v.unwrap_or_default();
                let simple = |kind, size| Some(od_spec(kind, size));
                let add = match u8::try_from(k).unwrap_or(0) {
                    b'A' => {
                        match v.as_slice() {
                            [r @ (b'd' | b'o' | b'x' | b'n')] => radix = *r,
                            _ => {
                                t.error(&bv(&[b"invalid output address radix '", &v, b"'; it must be one character from [doxn]"]));
                                return 1;
                            }
                        }
                        None
                    }
                    b'a' => simple(OdKind::Named, 1),
                    b'b' => simple(OdKind::Octal, 1),
                    b'c' => simple(OdKind::Char, 1),
                    b'd' => simple(OdKind::Unsigned, 2),
                    b'i' => simple(OdKind::Signed, 4),
                    b'l' => simple(OdKind::Signed, 8),
                    b'o' => simple(OdKind::Octal, 2),
                    b's' => simple(OdKind::Signed, 2),
                    b'x' => simple(OdKind::Hex, 2),
                    b'j' => {
                        skip = match size_arg(&v, b"-j") {
                            Ok(n) => n,
                            Err(c) => return c,
                        };
                        None
                    }
                    b'N' => {
                        limit = match size_arg(&v, b"-N") {
                            Ok(n) => Some(n),
                            Err(c) => return c,
                        };
                        None
                    }
                    b't' => {
                        if let Err(c) = od_parse_type(t, &v, &mut specs) {
                            return c;
                        }
                        None
                    }
                    b'v' => {
                        dups = true;
                        None
                    }
                    b'w' => {
                        width = Some(if v.is_empty() {
                            32
                        } else {
                            match size_arg(&v, b"-w") {
                                Ok(n) => usize::try_from(n).unwrap_or(usize::MAX),
                                Err(c) => return c,
                            }
                        });
                        None
                    }
                    c @ (b'B' | b'D' | b'e' | b'F' | b'f' | b'H' | b'h' | b'I' | b'L' | b'O' | b'X' | b'S') => {
                        return t.unsupported(&[b'-', c], 1);
                    }
                    _ if k == KEY_TRADITIONAL => return t.unsupported(b"--traditional", 1),
                    _ if k == KEY_ENDIAN => return t.unsupported(b"--endian", 1),
                    _ => return help_or_version(t, k, 1),
                };
                if let Some(s) = add {
                    specs.push(s);
                }
            }
        }
    }
    if specs.is_empty() {
        specs.push(od_spec(OdKind::Octal, 2));
    }
    let lcm = specs.iter().map(|s| s.size).fold(1, |a, b| a * b / gcd(a, b));
    let bpb = match width {
        Some(w) if w != 0 && w % lcm == 0 => w,
        Some(w) => {
            t.error(&bv(&[b"warning: invalid width ", &num(w), b"; using ", &num(lcm), b" instead"]));
            lcm
        }
        None if lcm < 16 => lcm * (16 / lcm),
        None => lcm,
    };
    let mut files = g.operands();
    if files.is_empty() {
        files.push(b"-".to_vec());
    }
    let mut reader = ChainReader { t, files, next: 0, cur: None, ok: true };
    // Pula os bytes pedidos.
    let mut scratch = vec![0u8; 8192];
    let mut to_skip = skip;
    while to_skip > 0 {
        let want = scratch.len().min(usize::try_from(to_skip).unwrap_or(usize::MAX));
        let n = reader.read(&mut scratch[..want]);
        if n == 0 {
            t.error(b"cannot skip past end of combined input");
            return 1;
        }
        to_skip -= n as u64;
    }
    // Largura de cada campo com o espaço que o separa; todas as linhas de um bloco se alinham.
    let line_width = specs.iter().map(|s| (s.width + 1) * (bpb / s.size)).max().unwrap_or(0);
    let addr_width = match radix {
        b'x' => 6,
        b'n' => 0,
        _ => 7,
    };
    let fmt_addr = |off: u64| -> Vec<u8> {
        match radix {
            b'd' => format!("{off:0addr_width$}").into_bytes(),
            b'x' => format!("{off:0addr_width$x}").into_bytes(),
            b'n' => Vec::new(),
            _ => format!("{off:0addr_width$o}").into_bytes(),
        }
    };
    let mut out = Out::new(t);
    let mut offset = skip;
    let mut remaining = limit;
    let mut prev: Option<Vec<u8>> = None;
    let mut starred = false;
    let mut block = vec![0u8; bpb];
    loop {
        let want = remaining.map_or(bpb, |r| bpb.min(usize::try_from(r).unwrap_or(usize::MAX)));
        if want == 0 {
            break;
        }
        let n = reader.read(&mut block[..want]);
        if n == 0 {
            break;
        }
        if let Some(r) = remaining.as_mut() {
            *r -= n as u64;
        }
        let rounded = n.div_ceil(lcm) * lcm;
        block[n..rounded].fill(0);
        let duplicate = !dups && n == bpb && prev.as_deref() == Some(&block[..]);
        if duplicate {
            if !starred {
                out.put(b"*\n");
                starred = true;
            }
        } else {
            starred = false;
            for (si, spec) in specs.iter().enumerate() {
                if si == 0 {
                    out.put(&fmt_addr(offset));
                } else {
                    out.put(&vec![b' '; addr_width]);
                }
                od_line(spec, &block[..rounded], n, bpb, line_width, &mut out);
            }
        }
        prev = Some(block.clone());
        offset += n as u64;
        if n < want {
            break;
        }
    }
    if radix != b'n' {
        out.put(&fmt_addr(offset));
        out.byte(b'\n');
    }
    out.finish(if reader.ok { 0 } else { 1 })
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Uma linha de um tipo: `data` já vem completada com zeros até um múltiplo do tamanho.
fn od_line(spec: &OdSpec, data: &[u8], real_len: usize, bpb: usize, line_width: usize, out: &mut Out) {
    const NAMES: [&str; 33] = [
        "nul", "soh", "stx", "etx", "eot", "enq", "ack", "bel", "bs", "ht", "nl", "vt", "ff", "cr", "so", "si", "dle",
        "dc1", "dc2", "dc3", "dc4", "nak", "syn", "etb", "can", "em", "sub", "esc", "fs", "gs", "rs", "us", "sp",
    ];
    let fields = bpb / spec.size;
    let shown = data.len() / spec.size;
    let blank = fields - shown;
    let pad = line_width - spec.width * fields;
    let mut pad_left = pad;
    let w = spec.width;
    for f in 0..shown {
        let next_pad = pad * (fields - f - 1) / fields;
        let field_pad = pad_left - next_pad;
        pad_left = next_pad;
        let chunk = &data[f * spec.size..(f + 1) * spec.size];
        let mut raw = [0u8; 8];
        raw[..spec.size].copy_from_slice(chunk);
        let u = u64::from_le_bytes(raw);
        let text = match spec.kind {
            OdKind::Named => {
                let c = chunk[0] & 0x7f;
                if c == 127 {
                    "del".to_string()
                } else if c <= 32 {
                    NAMES[usize::from(c)].to_string()
                } else {
                    (c as char).to_string()
                }
            }
            OdKind::Char => match chunk[0] {
                0 => "\\0".to_string(),
                7 => "\\a".to_string(),
                8 => "\\b".to_string(),
                12 => "\\f".to_string(),
                10 => "\\n".to_string(),
                13 => "\\r".to_string(),
                9 => "\\t".to_string(),
                11 => "\\v".to_string(),
                c @ 32..=126 => (c as char).to_string(),
                c => format!("{c:03o}"),
            },
            OdKind::Signed => {
                let bits = spec.size * 8;
                let v = if bits == 64 { u as i64 } else { ((u << (64 - bits)) as i64) >> (64 - bits) };
                v.to_string()
            }
            OdKind::Unsigned => u.to_string(),
            OdKind::Octal => format!("{u:0w$o}"),
            OdKind::Hex => format!("{u:0w$x}"),
        };
        out.put(&vec![b' '; field_pad]);
        out.put(format!("{text:>w$}").as_bytes());
    }
    if spec.trailer {
        let trailing = blank * w + pad * blank / fields;
        out.put(&vec![b' '; trailing]);
        out.put(b"  >");
        for &b in &data[..real_len] {
            out.byte(if (32..=126).contains(&b) { b } else { b'.' });
        }
        out.byte(b'<');
    }
    out.byte(b'\n');
}

// =====================================================================================================
// sort
// =====================================================================================================

use std::cmp::Ordering;

#[derive(Clone, Copy, Default, Debug)]
struct SortOpts {
    blanks_start: bool,
    blanks_end: bool,
    dictionary: bool,
    fold: bool,
    printable: bool,
    numeric: bool,
    general: bool,
    human: bool,
    month: bool,
    version: bool,
    reverse: bool,
}

impl SortOpts {
    /// Sem nenhuma opção de ordenação (o `r` não conta).
    fn plain(&self) -> bool {
        !(self.blanks_start
            || self.blanks_end
            || self.dictionary
            || self.fold
            || self.printable
            || self.numeric
            || self.general
            || self.human
            || self.month
            || self.version)
    }

    /// Aplica letras de ordenação (`bdfgiMhnrV`); devolve onde parou.
    fn set(&mut self, s: &[u8], end_pos: bool) -> usize {
        let mut i = 0;
        while let Some(&c) = s.get(i) {
            match c {
                b'b' if end_pos => self.blanks_end = true,
                b'b' => self.blanks_start = true,
                b'd' => self.dictionary = true,
                b'f' => self.fold = true,
                b'g' => self.general = true,
                b'h' => self.human = true,
                b'i' => self.printable = true,
                b'M' => self.month = true,
                b'n' => self.numeric = true,
                b'r' => self.reverse = true,
                b'V' => self.version = true,
                _ => break,
            }
            i += 1;
        }
        i
    }

    /// As letras para a mensagem de opções incompatíveis (sem b e r, como o GNU).
    fn letters(&self) -> Vec<u8> {
        let mut v = Vec::new();
        for (on, c) in [
            (self.dictionary, b'd'),
            (self.fold, b'f'),
            (self.general, b'g'),
            (self.human, b'h'),
            (self.printable, b'i'),
            (self.month, b'M'),
            (self.numeric, b'n'),
            (self.version, b'V'),
        ] {
            if on {
                v.push(c);
            }
        }
        v
    }
}

#[derive(Clone, Copy, Debug)]
struct SortKey {
    /// Campo inicial (0-based); `None` = começo da linha.
    sfield: Option<usize>,
    schar: usize,
    /// Campo final (0-based); `None` = fim da linha.
    efield: Option<usize>,
    /// 0 = até o fim do campo final.
    echar: usize,
    opts: SortOpts,
}

struct SortCfg {
    keys: Vec<SortKey>,
    tab: Option<u8>,
    reverse: bool,
    unique: bool,
    stable: bool,
}

fn sort_is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n'
}

fn key_begin(line: &[u8], k: &SortKey, tab: Option<u8>) -> usize {
    let len = line.len();
    let mut p = 0;
    if let Some(n) = k.sfield {
        for _ in 0..n {
            if p >= len {
                break;
            }
            match tab {
                Some(t) => {
                    while p < len && line[p] != t {
                        p += 1;
                    }
                    if p < len {
                        p += 1;
                    }
                }
                None => {
                    while p < len && sort_is_blank(line[p]) {
                        p += 1;
                    }
                    while p < len && !sort_is_blank(line[p]) {
                        p += 1;
                    }
                }
            }
        }
    }
    if k.opts.blanks_start {
        while p < len && sort_is_blank(line[p]) {
            p += 1;
        }
    }
    (p + k.schar).min(len)
}

fn key_limit(line: &[u8], k: &SortKey, tab: Option<u8>) -> usize {
    let len = line.len();
    let Some(efield) = k.efield else { return len };
    let mut remaining = if k.echar == 0 { efield + 1 } else { efield };
    let mut p = 0;
    while p < len && remaining > 0 {
        remaining -= 1;
        match tab {
            Some(t) => {
                while p < len && line[p] != t {
                    p += 1;
                }
                if p < len && (remaining > 0 || k.echar != 0) {
                    p += 1;
                }
            }
            None => {
                while p < len && sort_is_blank(line[p]) {
                    p += 1;
                }
                while p < len && !sort_is_blank(line[p]) {
                    p += 1;
                }
            }
        }
    }
    if k.echar != 0 {
        if k.opts.blanks_end {
            while p < len && sort_is_blank(line[p]) {
                p += 1;
            }
        }
        p = (p + k.echar).min(len);
    }
    p
}

/// Número no começo (depois de brancos): (negativo, parte inteira sem zeros à esquerda, fração sem
/// zeros à direita, posição logo depois do número).
fn sort_number(s: &[u8]) -> (bool, &[u8], &[u8], usize) {
    let mut i = 0;
    while i < s.len() && sort_is_blank(s[i]) {
        i += 1;
    }
    let neg = s.get(i) == Some(&b'-');
    if neg {
        i += 1;
    }
    let int_start = i;
    while s.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    let mut int = &s[int_start..i];
    while int.first() == Some(&b'0') {
        int = &int[1..];
    }
    let mut frac: &[u8] = &[];
    if s.get(i) == Some(&b'.') {
        let fs = i + 1;
        let mut j = fs;
        while s.get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
        frac = &s[fs..j];
        while frac.last() == Some(&b'0') {
            frac = &frac[..frac.len() - 1];
        }
        i = j;
    }
    (neg, int, frac, i)
}

/// `-n`: compara os números exatamente (sem número vale zero; -0 = 0).
fn numeric_cmp(a: &[u8], b: &[u8]) -> Ordering {
    let (na, ia, fa, _) = sort_number(a);
    let (nb, ib, fb, _) = sort_number(b);
    let za = ia.is_empty() && fa.is_empty();
    let zb = ib.is_empty() && fb.is_empty();
    let sa = if za { 0 } else if na { -1 } else { 1 };
    let sb = if zb { 0 } else if nb { -1 } else { 1 };
    if sa != sb {
        return sa.cmp(&sb);
    }
    if sa == 0 {
        return Ordering::Equal;
    }
    let mag = ia.len().cmp(&ib.len()).then_with(|| ia.cmp(ib)).then_with(|| fa.cmp(fb));
    if sa < 0 { mag.reverse() } else { mag }
}

/// `-h`: primeiro a ordem do sufixo (K, M, G...), depois o número.
fn human_cmp(a: &[u8], b: &[u8]) -> Ordering {
    let order = |s: &[u8]| -> i32 {
        let (neg, int, frac, end) = sort_number(s);
        if int.is_empty() && frac.is_empty() {
            return 0;
        }
        let o = match s.get(end) {
            Some(b'K' | b'k') => 1,
            Some(b'M') => 2,
            Some(b'G') => 3,
            Some(b'T') => 4,
            Some(b'P') => 5,
            Some(b'E') => 6,
            Some(b'Z') => 7,
            Some(b'Y') => 8,
            Some(b'R') => 9,
            Some(b'Q') => 10,
            _ => 0,
        };
        if neg { -o } else { o }
    };
    order(a).cmp(&order(b)).then_with(|| numeric_cmp(a, b))
}

/// `-g`: valor do strtod; quem não é número vem antes, depois NaN, depois os números.
fn general_cmp(a: &[u8], b: &[u8]) -> Ordering {
    let rank = |s: &[u8]| -> (u8, f64) {
        match strtod_prefix(s) {
            None => (0, 0.0),
            Some((v, _)) if v.is_nan() => (1, 0.0),
            Some((v, _)) => (2, v),
        }
    };
    let (ra, va) = rank(a);
    let (rb, vb) = rank(b);
    ra.cmp(&rb).then_with(|| va.partial_cmp(&vb).unwrap_or(Ordering::Equal))
}

fn month_of(s: &[u8]) -> u8 {
    const MONTHS: [&[u8]; 12] = [b"JAN", b"FEB", b"MAR", b"APR", b"MAY", b"JUN", b"JUL", b"AUG", b"SEP", b"OCT", b"NOV", b"DEC"];
    let mut i = 0;
    while i < s.len() && sort_is_blank(s[i]) {
        i += 1;
    }
    let Some(head) = s.get(i..i + 3) else { return 0 };
    let up = head.to_ascii_uppercase();
    MONTHS.iter().position(|m| *m == up.as_slice()).map_or(0, |p| p as u8 + 1)
}

/// Ordem de versões (a do `sort -V` e `ls -v`): partes não numéricas comparadas com `~` antes de
/// tudo e letras antes do resto, partes numéricas pelo valor; sufixos de arquivo (`.tar.gz`) só
/// desempatam; `.`, `..` e nomes ocultos primeiro.
fn version_cmp(a: &[u8], b: &[u8]) -> Ordering {
    if a.is_empty() || b.is_empty() {
        return (!a.is_empty()).cmp(&!b.is_empty());
    }
    let dot_rank = |s: &[u8]| -> u8 {
        if s == b"." {
            0
        } else if s == b".." {
            1
        } else if s.starts_with(b".") {
            2
        } else {
            3
        }
    };
    let (ra, rb) = (dot_rank(a), dot_rank(b));
    if ra != rb || ra < 2 {
        return ra.cmp(&rb);
    }
    let prefix_len = |s: &[u8]| -> usize {
        // Tira o sufixo mais longo da forma (\.[A-Za-z~][A-Za-z0-9~]*)*$.
        let mut prefix = 0;
        let mut i = 0;
        while i < s.len() {
            i += 1;
            prefix = i;
            while i + 1 < s.len() && s[i] == b'.' && (s[i + 1].is_ascii_alphabetic() || s[i + 1] == b'~') {
                i += 2;
                while i < s.len() && (s[i].is_ascii_alphanumeric() || s[i] == b'~') {
                    i += 1;
                }
            }
        }
        prefix
    };
    let (pa, pb) = (prefix_len(a), prefix_len(b));
    let r = verrevcmp(&a[..pa], &b[..pb]);
    if r != Ordering::Equal || (pa == a.len() && pb == b.len()) {
        return r;
    }
    verrevcmp(a, b)
}

fn verrevcmp(a: &[u8], b: &[u8]) -> Ordering {
    let order = |s: &[u8], i: usize| -> i32 {
        match s.get(i) {
            None => -1,
            Some(c) if c.is_ascii_digit() => 0,
            Some(c) if c.is_ascii_alphabetic() => i32::from(*c),
            Some(b'~') => -2,
            Some(c) => i32::from(*c) + 256,
        }
    };
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        while (i < a.len() && !a[i].is_ascii_digit()) || (j < b.len() && !b[j].is_ascii_digit()) {
            let (oa, ob) = (order(a, i), order(b, j));
            if oa != ob {
                return oa.cmp(&ob);
            }
            i += 1;
            j += 1;
        }
        while a.get(i) == Some(&b'0') {
            i += 1;
        }
        while b.get(j) == Some(&b'0') {
            j += 1;
        }
        let mut first_diff = Ordering::Equal;
        while i < a.len() && j < b.len() && a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            if first_diff == Ordering::Equal {
                first_diff = a[i].cmp(&b[j]);
            }
            i += 1;
            j += 1;
        }
        if a.get(i).is_some_and(u8::is_ascii_digit) {
            return Ordering::Greater;
        }
        if b.get(j).is_some_and(u8::is_ascii_digit) {
            return Ordering::Less;
        }
        if first_diff != Ordering::Equal {
            return first_diff;
        }
    }
    Ordering::Equal
}

fn key_cmp(a: &[u8], b: &[u8], k: &SortKey, tab: Option<u8>) -> Ordering {
    let (ba, la) = (key_begin(a, k, tab), key_limit(a, k, tab));
    let (bb, lb) = (key_begin(b, k, tab), key_limit(b, k, tab));
    let ka = &a[ba..la.max(ba)];
    let kb = &b[bb..lb.max(bb)];
    let o = &k.opts;
    // As comparações numéricas param no primeiro NUL, como as funções da libc.
    let cstr = |s: &[u8]| -> Vec<u8> { s.iter().copied().take_while(|&c| c != 0).collect() };
    if o.numeric {
        numeric_cmp(&cstr(ka), &cstr(kb))
    } else if o.general {
        general_cmp(&cstr(ka), &cstr(kb))
    } else if o.human {
        human_cmp(&cstr(ka), &cstr(kb))
    } else if o.month {
        month_of(ka).cmp(&month_of(kb))
    } else if o.version {
        version_cmp(ka, kb)
    } else if o.dictionary || o.printable || o.fold {
        let tr = |s: &[u8]| -> Vec<u8> {
            s.iter()
                .copied()
                .filter(|&c| !(o.dictionary && !(c.is_ascii_alphanumeric() || sort_is_blank(c))))
                .filter(|&c| !(o.printable && !(32..=126).contains(&c)))
                .map(|c| if o.fold { c.to_ascii_uppercase() } else { c })
                .collect()
        };
        tr(ka).cmp(&tr(kb))
    } else {
        ka.cmp(kb)
    }
}

fn sort_cmp(a: &[u8], b: &[u8], cfg: &SortCfg) -> Ordering {
    if !cfg.keys.is_empty() {
        for k in &cfg.keys {
            let d = key_cmp(a, b, k, cfg.tab);
            if d != Ordering::Equal {
                return if k.opts.reverse { d.reverse() } else { d };
            }
        }
        if cfg.unique || cfg.stable {
            return Ordering::Equal;
        }
    }
    let d = a.cmp(b);
    if cfg.reverse { d.reverse() } else { d }
}

/// `-k CAMPO[.CHAR][OPÇÕES][,CAMPO[.CHAR][OPÇÕES]]`.
fn parse_sort_key(t: &Tool, spec: &[u8]) -> Result<SortKey, i32> {
    let bad = |what: &[u8]| -> Result<SortKey, i32> {
        t.error(&bv(&[what, b": invalid field specification ", &quote(spec)]));
        Err(2)
    };
    let count = |s: &[u8], what: &[u8]| -> Result<(usize, usize), i32> {
        let n = s.iter().take_while(|b| b.is_ascii_digit()).count();
        if n == 0 {
            t.error(&bv(&[what, b": invalid count at start of ", &quote(s)]));
            return Err(2);
        }
        let v = std::str::from_utf8(&s[..n]).ok().and_then(|x| x.parse::<usize>().ok()).unwrap_or(usize::MAX);
        Ok((v, n))
    };
    let mut k = SortKey { sfield: None, schar: 0, efield: None, echar: 0, opts: SortOpts::default() };
    let mut i = 0;
    let (f, n) = count(spec, b"invalid number at field start")?;
    i += n;
    if f == 0 {
        return bad(b"field number is zero");
    }
    let mut schar = 1;
    if spec.get(i) == Some(&b'.') {
        let (c, n) = count(&spec[i + 1..], b"invalid number after '.'")?;
        if c == 0 {
            return bad(b"character offset is zero");
        }
        schar = c;
        i += 1 + n;
    }
    if f > 1 || schar > 1 {
        k.sfield = Some(f - 1);
        k.schar = schar - 1;
    }
    i += k.opts.set(&spec[i..], false);
    if spec.get(i) == Some(&b',') {
        let (e, n) = count(&spec[i + 1..], b"invalid number after ','")?;
        i += 1 + n;
        if e == 0 {
            return bad(b"field number is zero");
        }
        k.efield = Some(e - 1);
        if spec.get(i) == Some(&b'.') {
            let (c, n) = count(&spec[i + 1..], b"invalid number after '.'")?;
            k.echar = c;
            i += 1 + n;
        }
        i += k.opts.set(&spec[i..], true);
    }
    if i != spec.len() {
        return bad(b"stray character in field spec");
    }
    Ok(k)
}

fn sort(t: &Tool, args: &Argv) -> i32 {
    const KEY_SORT: u32 = 0x100;
    const KEY_IGNORED: u32 = 0x101;
    const KEY_RANDOM: u32 = 0x102;
    const KEY_DEBUG: u32 = 0x103;
    const KEY_FILES0: u32 = 0x104;
    const KEY_CHECK: u32 = 0x105;
    const LONGS: &[LongOpt] = &[
        long("ignore-leading-blanks", HasArg::No, b'b' as u32),
        long("check", HasArg::Opt, KEY_CHECK),
        long("compress-program", HasArg::Req, KEY_IGNORED),
        long("debug", HasArg::No, KEY_DEBUG),
        long("dictionary-order", HasArg::No, b'd' as u32),
        long("ignore-case", HasArg::No, b'f' as u32),
        long("files0-from", HasArg::Req, KEY_FILES0),
        long("general-numeric-sort", HasArg::No, b'g' as u32),
        long("ignore-nonprinting", HasArg::No, b'i' as u32),
        long("key", HasArg::Req, b'k' as u32),
        long("merge", HasArg::No, b'm' as u32),
        long("month-sort", HasArg::No, b'M' as u32),
        long("numeric-sort", HasArg::No, b'n' as u32),
        long("human-numeric-sort", HasArg::No, b'h' as u32),
        long("version-sort", HasArg::No, b'V' as u32),
        long("random-sort", HasArg::No, b'R' as u32),
        long("random-source", HasArg::Req, KEY_RANDOM),
        long("sort", HasArg::Req, KEY_SORT),
        long("output", HasArg::Req, b'o' as u32),
        long("reverse", HasArg::No, b'r' as u32),
        long("stable", HasArg::No, b's' as u32),
        long("batch-size", HasArg::Req, KEY_IGNORED),
        long("buffer-size", HasArg::Req, b'S' as u32),
        long("field-separator", HasArg::Req, b't' as u32),
        long("temporary-directory", HasArg::Req, b'T' as u32),
        long("unique", HasArg::No, b'u' as u32),
        long("zero-terminated", HasArg::No, b'z' as u32),
        long("parallel", HasArg::Req, KEY_IGNORED),
    ];
    const SORTS: &[&str] = &["general-numeric", "human-numeric", "month", "numeric", "random", "version"];
    const CHECKS: &[&str] = &["quiet", "silent", "diagnose-first"];
    let mut g = Getopt::new(t, args, b"-bcCdfghik:mMno:rRsS:t:T:uVy:z", LONGS);
    let mut global = SortOpts::default();
    let mut keys: Vec<SortKey> = Vec::new();
    let (mut check, mut merge, mut unique, mut stable, mut zero) = (None::<bool>, false, false, false, false);
    let mut tab: Option<u8> = None;
    let mut output: Option<Vec<u8>> = None;
    let mut extra_files: Vec<Vec<u8>> = Vec::new();
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 2;
            }
            Got::Opt(k, v) => {
                let v = v.unwrap_or_default();
                match u8::try_from(k).unwrap_or(0) {
                    1 => extra_files.push(v),
                    c @ (b'b' | b'd' | b'f' | b'g' | b'h' | b'i' | b'M' | b'n' | b'r' | b'V') => {
                        global.set(&[c], false);
                        if c == b'b' {
                            global.blanks_end = true;
                        }
                    }
                    b'R' => return t.unsupported(b"-R", 2),
                    b'c' => check = Some(true),
                    b'C' => check = Some(false),
                    b'k' => match parse_sort_key(t, &v) {
                        Ok(key) => keys.push(key),
                        Err(c) => return c,
                    },
                    b'm' => merge = true,
                    b'o' => {
                        if output.as_ref().is_some_and(|o| *o != v) {
                            t.error(b"multiple output files specified");
                            return 2;
                        }
                        output = Some(v);
                    }
                    b's' => stable = true,
                    b'S' | b'T' | b'y' => {}
                    b't' => {
                        let sep = if v == b"\\0" {
                            0
                        } else {
                            match v.as_slice() {
                                [] => {
                                    t.error(b"empty tab");
                                    return 2;
                                }
                                [c] => *c,
                                _ => {
                                    t.error(&bv(&[b"multi-character tab ", &quote(&v)]));
                                    return 2;
                                }
                            }
                        };
                        if tab.is_some_and(|old| old != sep) {
                            t.error(b"incompatible tabs");
                            return 2;
                        }
                        tab = Some(sep);
                    }
                    b'u' => unique = true,
                    b'z' => zero = true,
                    _ if k == KEY_IGNORED => {}
                    _ if k == KEY_SORT => match argmatch(&v, SORTS) {
                        Ok(0) => global.general = true,
                        Ok(1) => global.human = true,
                        Ok(2) => global.month = true,
                        Ok(3) => global.numeric = true,
                        Ok(4) => return t.unsupported(b"--sort=random", 2),
                        Ok(_) => global.version = true,
                        Err(amb) => return argmatch_fail(t, b"--sort", &v, amb, SORTS, 2),
                    },
                    _ if k == KEY_CHECK => {
                        check = Some(if v.is_empty() {
                            true
                        } else {
                            match argmatch(&v, CHECKS) {
                                Ok(2) => true,
                                Ok(_) => false,
                                Err(amb) => return argmatch_fail(t, b"--check", &v, amb, CHECKS, 2),
                            }
                        })
                    }
                    _ if k == KEY_RANDOM => return t.unsupported(b"--random-source", 2),
                    _ if k == KEY_DEBUG => return t.unsupported(b"--debug", 2),
                    _ if k == KEY_FILES0 => return t.unsupported(b"--files0-from", 2),
                    _ => return help_or_version(t, k, 2),
                }
            }
        }
    }
    let mut files = extra_files;
    files.extend(g.operands());
    // Chaves sem opção de ordenação herdam as globais (inclusive o -r).
    for k in &mut keys {
        if k.opts.plain() && !k.opts.reverse {
            k.opts = global;
        }
    }
    if keys.is_empty() && !global.plain() {
        keys.push(SortKey { sfield: None, schar: 0, efield: None, echar: 0, opts: global });
    }
    for k in &keys {
        let o = &k.opts;
        let kinds = [o.numeric, o.general, o.human, o.month, o.version || o.dictionary || o.printable].iter().filter(|&&b| b).count();
        if kinds > 1 {
            t.error(&bv(&[b"options '-", &o.letters(), b"' are incompatible"]));
            return 2;
        }
    }
    let cfg = SortCfg { keys, tab, reverse: global.reverse, unique, stable };
    if files.is_empty() {
        files.push(b"-".to_vec());
    }
    let term = if zero { 0 } else { b'\n' };
    if let Some(diagnose) = check {
        if files.len() > 1 {
            t.error(&bv(&[b"extra operand ", &quoteaf(&files[1]), b" not allowed with -c"]));
            return 2;
        }
        let lines = match sort_read(t, &files[0], term) {
            Ok(l) => l,
            Err(c) => return c,
        };
        for i in 1..lines.len() {
            let c = sort_cmp(&lines[i - 1], &lines[i], &cfg);
            if c == Ordering::Greater || (unique && c == Ordering::Equal) {
                if diagnose {
                    let mut msg = bv(&[&t.name, b": ", &files[0], b":", &num(i + 1), b": disorder: ", &lines[i]]);
                    msg.push(b'\n');
                    stderr(&msg);
                }
                return 1;
            }
        }
        return 0;
    }
    let mut per_file: Vec<Vec<Vec<u8>>> = Vec::new();
    for f in &files {
        match sort_read(t, f, term) {
            Ok(l) => per_file.push(l),
            Err(c) => return c,
        }
    }
    let lines: Vec<Vec<u8>> = if merge {
        // Intercala entradas já ordenadas; empate fica com o arquivo de antes.
        let mut idx = vec![0usize; per_file.len()];
        let mut merged = Vec::new();
        loop {
            let mut best: Option<usize> = None;
            for (fi, l) in per_file.iter().enumerate() {
                if idx[fi] < l.len() {
                    best = match best {
                        Some(b) if sort_cmp(&per_file[b][idx[b]], &l[idx[fi]], &cfg) != Ordering::Greater => Some(b),
                        _ => Some(fi),
                    };
                }
            }
            let Some(b) = best else { break };
            merged.push(std::mem::take(&mut per_file[b][idx[b]]));
            idx[b] += 1;
        }
        merged
    } else {
        let mut all: Vec<Vec<u8>> = per_file.into_iter().flatten().collect();
        all.sort_by(|a, b| sort_cmp(a, b, &cfg));
        all
    };
    let mut out = Out::new(t);
    if let Some(o) = &output {
        match sys::open(o, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC, 0o666) {
            Ok(fd) => out.fd = fd,
            Err(e) => {
                t.error_errno(&bv(&[b"open failed: ", o]), e);
                return 2;
            }
        }
    }
    let mut last: Option<&Vec<u8>> = None;
    for l in &lines {
        if unique && last.is_some_and(|p| sort_cmp(p, l, &cfg) == Ordering::Equal) {
            continue;
        }
        out.put(l);
        out.byte(term);
        last = Some(l);
    }
    let fd = out.fd;
    let code = out.finish(0);
    if fd != Fd::STDOUT {
        let _ = sys::close(fd);
    }
    code
}

/// Lê as linhas (sem o terminador) de um arquivo do sort.
fn sort_read(t: &Tool, name: &[u8], term: u8) -> Result<Vec<Vec<u8>>, i32> {
    let mut input = match Input::open(name) {
        Ok(i) => i,
        Err(e) => {
            t.error_errno(&bv(&[b"cannot read: ", name]), e);
            return Err(2);
        }
    };
    let data = input.read_all();
    input.close();
    let data = match data {
        Ok(d) => d,
        Err(e) => {
            t.error_errno(&bv(&[b"read failed: ", name]), e);
            return Err(2);
        }
    };
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let body = data.strip_suffix(&[term]).unwrap_or(&data);
    Ok(body.split(|&b| b == term).map(<[u8]>::to_vec).collect())
}

// =====================================================================================================
// tr
// =====================================================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TrClass {
    Alnum,
    Alpha,
    Blank,
    Cntrl,
    Digit,
    Graph,
    Lower,
    Print,
    Punct,
    Space,
    Upper,
    Xdigit,
}

impl TrClass {
    fn from_name(n: &[u8]) -> Option<TrClass> {
        Some(match n {
            b"alnum" => TrClass::Alnum,
            b"alpha" => TrClass::Alpha,
            b"blank" => TrClass::Blank,
            b"cntrl" => TrClass::Cntrl,
            b"digit" => TrClass::Digit,
            b"graph" => TrClass::Graph,
            b"lower" => TrClass::Lower,
            b"print" => TrClass::Print,
            b"punct" => TrClass::Punct,
            b"space" => TrClass::Space,
            b"upper" => TrClass::Upper,
            b"xdigit" => TrClass::Xdigit,
            _ => return None,
        })
    }

    /// Pertinência no C.UTF-8: classes ASCII (bytes acima de 127 não pertencem a nenhuma).
    fn has(self, b: u8) -> bool {
        match self {
            TrClass::Alnum => b.is_ascii_alphanumeric(),
            TrClass::Alpha => b.is_ascii_alphabetic(),
            TrClass::Blank => b == b' ' || b == b'\t',
            TrClass::Cntrl => b < 32 || b == 127,
            TrClass::Digit => b.is_ascii_digit(),
            TrClass::Graph => b.is_ascii_graphic(),
            TrClass::Lower => b.is_ascii_lowercase(),
            TrClass::Print => (32..=126).contains(&b),
            TrClass::Punct => b.is_ascii_punctuation(),
            TrClass::Space => is_space(b),
            TrClass::Upper => b.is_ascii_uppercase(),
            TrClass::Xdigit => b.is_ascii_hexdigit(),
        }
    }

    fn is_case(self) -> bool {
        matches!(self, TrClass::Upper | TrClass::Lower)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TrElem {
    Char(u8),
    Range(u8, u8),
    Class(TrClass),
    Equiv(u8),
    /// `[c*n]`; `None` = `[c*]` (ou `[c*0]`), que completa o tamanho do conjunto 1.
    Repeat(u8, Option<u64>),
}

/// Texto imprimível para mensagens (`\n`, `\\`, `\ooo`).
fn tr_printable(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &b in s {
        match b {
            b'\\' => out.extend_from_slice(b"\\\\"),
            32..=126 => out.push(b),
            _ => c_escape(b, &mut out),
        }
    }
    out
}

/// Desfaz os escapes; devolve os bytes e quais vieram de escape.
fn tr_unescape(t: &Tool, s: &[u8]) -> (Vec<u8>, Vec<bool>) {
    let (mut out, mut esc) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < s.len() {
        if s[i] != b'\\' {
            out.push(s[i]);
            esc.push(false);
            i += 1;
            continue;
        }
        let Some(&n) = s.get(i + 1) else {
            t.error(b"warning: an unescaped backslash at end of string is not portable");
            out.push(b'\\');
            esc.push(false);
            break;
        };
        let (c, used) = match n {
            b'\\' => (b'\\', 2),
            b'a' => (7, 2),
            b'b' => (8, 2),
            b'f' => (12, 2),
            b'n' => (b'\n', 2),
            b'r' => (b'\r', 2),
            b't' => (b'\t', 2),
            b'v' => (11, 2),
            b'0'..=b'7' => {
                let oct = |k: usize| s.get(i + k).filter(|c| (b'0'..=b'7').contains(*c)).map(|c| u32::from(c - b'0'));
                let mut v = u32::from(n - b'0');
                let mut used = 2;
                if let Some(d) = oct(2) {
                    v = v * 8 + d;
                    used = 3;
                    if let Some(d3) = oct(3) {
                        if v * 8 + d3 > 255 {
                            t.error(&bv(&[
                                b"warning: the ambiguous octal escape \\",
                                &s[i + 1..i + 4],
                                b" is being\n\tinterpreted as the 2-byte sequence \\0",
                                &s[i + 1..i + 3],
                                b", ",
                                &s[i + 3..i + 4],
                            ]));
                        } else {
                            v = v * 8 + d3;
                            used = 4;
                        }
                    }
                }
                (v as u8, used)
            }
            other => (other, 2),
        };
        out.push(c);
        esc.push(true);
        i += used;
    }
    (out, esc)
}

/// Interpreta um conjunto do tr. Os erros já saem impressos.
fn tr_parse(t: &Tool, raw: &[u8]) -> Result<Vec<TrElem>, ()> {
    let (s, esc) = tr_unescape(t, raw);
    let is = |i: usize, c: u8| s.get(i) == Some(&c) && !esc[i];
    let mut elems = Vec::new();
    let mut i = 0;
    // `[c*digitos]` a partir de `i` (o `[`): Ok(Some((c, n, fim))), Ok(None) se não é repetição.
    let repeat_at = |i: usize| -> Result<Option<(u8, Option<u64>, usize)>, ()> {
        if i + 2 >= s.len() || !is(i + 2, b'*') {
            return Ok(None);
        }
        let mut j = i + 3;
        while j < s.len() && !esc[j] {
            if s[j] == b']' {
                let digits = &s[i + 3..j];
                let count = if digits.is_empty() {
                    None
                } else {
                    let base = if digits[0] == b'0' { 8 } else { 10 };
                    let v = std::str::from_utf8(digits).ok().and_then(|d| u64::from_str_radix(d, base).ok());
                    match v {
                        Some(0) => None,
                        Some(n) => Some(n),
                        None => {
                            t.error(&bv(&[b"invalid repeat count ", &quote(&tr_printable(digits)), b" in [c*n] construct"]));
                            return Err(());
                        }
                    }
                };
                return Ok(Some((s[i + 1], count, j)));
            }
            j += 1;
        }
        Ok(None)
    };
    while i < s.len() {
        if is(i, b'[') {
            if (is(i + 1, b':') || is(i + 1, b'=')) && i + 2 < s.len() {
                let delim = s[i + 1];
                let close = (i + 2..s.len().saturating_sub(1)).find(|&j| s[j] == delim && s[j + 1] == b']' && !esc[j] && !esc[j + 1]);
                if let Some(j) = close {
                    let body = &s[i + 2..j];
                    let star_digits = body.first() == Some(&b'*') && body[1..].iter().all(u8::is_ascii_digit);
                    if delim == b':' {
                        match TrClass::from_name(body) {
                            Some(c) => {
                                elems.push(TrElem::Class(c));
                                i = j + 2;
                                continue;
                            }
                            None if star_digits => {}
                            None => {
                                t.error(&bv(&[b"invalid character class ", &quote(&tr_printable(body))]));
                                return Err(());
                            }
                        }
                    } else if body.len() == 1 {
                        elems.push(TrElem::Equiv(body[0]));
                        i = j + 2;
                        continue;
                    } else if !star_digits {
                        t.error(&bv(&[&tr_printable(body), b": equivalence class operand must be a single character"]));
                        return Err(());
                    }
                }
            }
            if let Some((c, n, end)) = repeat_at(i)? {
                elems.push(TrElem::Repeat(c, n));
                i = end + 1;
                continue;
            }
        }
        if i + 2 < s.len() && is(i + 1, b'-') {
            let (a, b) = (s[i], s[i + 2]);
            if a > b {
                t.error(&bv(&[b"range-endpoints of '", &tr_printable(&s[i..i + 3]), b"' are in reverse collating sequence order"]));
                return Err(());
            }
            elems.push(TrElem::Range(a, b));
            i += 3;
            continue;
        }
        elems.push(TrElem::Char(s[i]));
        i += 1;
    }
    Ok(elems)
}

/// Tamanho de um elemento (o `[c*]` conta zero aqui).
fn tr_elem_len(e: &TrElem) -> u64 {
    match *e {
        TrElem::Char(_) | TrElem::Equiv(_) => 1,
        TrElem::Range(a, b) => u64::from(b - a) + 1,
        TrElem::Class(c) => (0..=255u8).filter(|&b| c.has(b)).count() as u64,
        TrElem::Repeat(_, n) => n.unwrap_or(0),
    }
}

/// O conjunto expandido como sequência de (byte, quantas vezes), com o `[c*]` valendo `fill`.
fn tr_runs(elems: &[TrElem], fill: u64) -> Vec<(u8, u64)> {
    let mut runs = Vec::new();
    for e in elems {
        match *e {
            TrElem::Char(c) | TrElem::Equiv(c) => runs.push((c, 1)),
            TrElem::Range(a, b) => runs.extend((a..=b).map(|c| (c, 1))),
            TrElem::Class(cl) => runs.extend((0..=255u8).filter(|&b| cl.has(b)).map(|b| (b, 1))),
            TrElem::Repeat(c, n) => {
                let n = n.unwrap_or(fill);
                if n > 0 {
                    runs.push((c, n));
                }
            }
        }
    }
    runs
}

fn tr_set(runs: &[(u8, u64)], complement: bool) -> [bool; 256] {
    let mut set = [false; 256];
    for &(c, _) in runs {
        set[usize::from(c)] = true;
    }
    if complement {
        for v in &mut set {
            *v = !*v;
        }
    }
    set
}

fn tr(t: &Tool, args: &Argv) -> i32 {
    const LONGS: &[LongOpt] = &[
        long("complement", HasArg::No, b'c' as u32),
        long("delete", HasArg::No, b'd' as u32),
        long("squeeze-repeats", HasArg::No, b's' as u32),
        long("truncate-set1", HasArg::No, b't' as u32),
    ];
    let mut g = Getopt::new(t, args, b"+cCdst", LONGS);
    let (mut complement, mut delete, mut squeeze, mut truncate) = (false, false, false, false);
    loop {
        match g.next() {
            Got::End => break,
            Got::Fail => {
                t.try_help();
                return 1;
            }
            Got::Opt(k, _) => match u8::try_from(k).unwrap_or(0) {
                b'c' | b'C' => complement = true,
                b'd' => delete = true,
                b's' => squeeze = true,
                b't' => truncate = true,
                _ => return help_or_version(t, k, 1),
            },
        }
    }
    let ops = g.operands();
    let n = ops.len();
    let min_ops = if delete == squeeze { 2 } else { 1 };
    let max_ops = if delete && !squeeze { 1 } else { 2 };
    if n < min_ops {
        if n == 0 {
            return t.usage_error(b"missing operand", 1);
        }
        t.error(&bv(&[b"missing operand after ", &quote(&ops[n - 1])]));
        stderr(if squeeze {
            b"Two strings must be given when both deleting and squeezing repeats.\n"
        } else {
            b"Two strings must be given when translating.\n"
        });
        t.try_help();
        return 1;
    }
    if n > max_ops {
        t.error(&bv(&[b"extra operand ", &quote(&ops[max_ops])]));
        if n == 2 {
            stderr(b"Only one string may be given when deleting without squeezing repeats.\n");
        }
        t.try_help();
        return 1;
    }
    let translating = n == 2 && !delete;
    let Ok(s1) = tr_parse(t, &ops[0]) else { return 1 };
    let s2 = if n == 2 {
        match tr_parse(t, &ops[1]) {
            Ok(s) => Some(s),
            Err(()) => return 1,
        }
    } else {
        None
    };
    let die = |msg: &[u8]| -> i32 {
        t.error(msg);
        1
    };
    if s1.iter().any(|e| matches!(e, TrElem::Repeat(_, None))) {
        return die(b"the [c*] repeat construct may not appear in string1");
    }
    let s1_runs = tr_runs(&s1, 0);
    let s1_set = tr_set(&s1_runs, false);
    let s1_len: u64 = if complement { s1_set.iter().filter(|&&b| !b).count() as u64 } else { s1_runs.iter().map(|r| r.1).sum() };
    let mut s2_runs: Vec<(u8, u64)> = Vec::new();
    if let Some(s2) = &s2 {
        let fills = s2.iter().filter(|e| matches!(e, TrElem::Repeat(_, None))).count();
        if fills > 1 {
            return die(b"only one [c*] repeat construct may appear in string2");
        }
        let fixed: u64 = s2.iter().map(tr_elem_len).sum();
        let fill = s1_len.saturating_sub(fixed);
        if translating {
            if s2.iter().any(|e| matches!(e, TrElem::Equiv(_))) {
                return die(b"[=c=] expressions may not appear in string2 when translating");
            }
            if s2.iter().any(|e| matches!(e, TrElem::Class(c) if !c.is_case())) {
                return die(b"when translating, the only character classes that may appear in\nstring2 are 'upper' and 'lower'");
            }
            if !complement {
                // Cada [:upper:]/[:lower:] do conjunto 2 precisa começar junto com um do conjunto 1.
                let starts = |elems: &[TrElem], fill: u64| -> Vec<(u64, TrElem)> {
                    let mut pos = 0;
                    elems
                        .iter()
                        .map(|e| {
                            let here = pos;
                            pos += match e {
                                TrElem::Repeat(_, None) => fill,
                                other => tr_elem_len(other),
                            };
                            (here, *e)
                        })
                        .collect()
                };
                let p1 = starts(&s1, 0);
                for (pos, e) in starts(s2, fill) {
                    if let TrElem::Class(c) = e
                        && c.is_case()
                        && !p1.iter().any(|&(q, f)| q == pos && matches!(f, TrElem::Class(c1) if c1.is_case()))
                    {
                        return die(b"misaligned [:upper:] and/or [:lower:] construct");
                    }
                }
            }
            s2_runs = tr_runs(s2, fill);
            let s2_len: u64 = s2_runs.iter().map(|r| r.1).sum();
            if s1_len > s2_len && !truncate {
                if s2_len == 0 {
                    return die(b"when not truncating set1, string2 must be non-empty");
                }
                if matches!(s2.last(), Some(TrElem::Class(_))) {
                    return die(b"when translating with string1 longer than string2,\nthe latter string must not end with a character class");
                }
                let last = s2_runs.last().map_or(0, |r| r.0);
                s2_runs.push((last, s1_len - s2_len));
            }
            if complement && s1.iter().any(|e| matches!(e, TrElem::Class(_))) && s2_runs.iter().any(|r| r.0 != s2_runs[0].0) {
                return die(b"when translating with complemented character classes,\nstring2 must map all characters in the domain to one");
            }
        } else {
            if fills > 0 {
                return die(b"the [c*] construct may appear in string2 only when translating");
            }
            s2_runs = tr_runs(s2, 0);
        }
    }
    // Tabela de tradução, conjunto de remoção e conjunto de compressão.
    let mut map: [u8; 256] = std::array::from_fn(|i| i as u8);
    let mut del = [false; 256];
    let mut sq = [false; 256];
    if translating {
        let domain: Vec<(u8, u64)> = if complement {
            (0..=255u8).filter(|&b| !s1_set[usize::from(b)]).map(|b| (b, 1)).collect()
        } else {
            s1_runs.clone()
        };
        let (mut i1, mut i2) = (0usize, 0usize);
        let (mut r1, mut r2) = (domain.first().map_or(0, |r| r.1), s2_runs.first().map_or(0, |r| r.1));
        while i1 < domain.len() && i2 < s2_runs.len() {
            map[usize::from(domain[i1].0)] = s2_runs[i2].0;
            let k = r1.min(r2);
            r1 -= k;
            r2 -= k;
            if r1 == 0 {
                i1 += 1;
                r1 = domain.get(i1).map_or(0, |r| r.1);
            }
            if r2 == 0 {
                i2 += 1;
                r2 = s2_runs.get(i2).map_or(0, |r| r.1);
            }
        }
        if squeeze {
            sq = tr_set(&s2_runs, false);
        }
    } else if delete {
        del = tr_set(&s1_runs, complement);
        if squeeze {
            sq = tr_set(&s2_runs, false);
        }
    } else {
        sq = tr_set(&s1_runs, complement);
    }
    let mut out = Out::new(t);
    let mut input = Input::stdin();
    let mut last: Option<u8> = None;
    loop {
        let chunk = match input.chunk() {
            Ok(c) => c,
            Err(e) => {
                out.flush();
                t.error_errno(b"read error", e);
                return 1;
            }
        };
        if chunk.is_empty() {
            break;
        }
        let mut buf = Vec::with_capacity(chunk.len());
        for &b in chunk {
            if del[usize::from(b)] {
                continue;
            }
            let c = map[usize::from(b)];
            if sq[usize::from(c)] && last == Some(c) {
                continue;
            }
            last = Some(c);
            buf.push(c);
        }
        out.put(&buf);
    }
    out.finish(0)
}

