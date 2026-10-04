//! `stdin()`, `stdout()` e `stderr()` do pseudo-processo, sobre os fds 0, 1 e 2 do `sysabi`. Os
//! traits (`Read`, `Write`, `BufRead`) e os tipos puros (`BufReader`, `BufWriter`, `Error`,
//! `Cursor`...) continuam os do std: só os handles mudam.
//!
//! Bufferização igual à do stdio da glibc, que é o que os programas GNU usam:
//!
//! - stdout dentro de [`crate::run`]: por linha se o fd 1 é terminal, em bloco de 4096 bytes se não
//!   é (pipe, arquivo). Descarrega no `flush`, ao encher, no fim do programa, no `exit` e antes de
//!   criar um processo filho (`process::Command`), pra que a saída do pai e a do filho não troquem
//!   de ordem. Fora de um `run`, escreve direto.
//! - stderr: sem buffer; cada `write!`/`eprintln!` vira um único `write` (a glibc monta a linha
//!   inteira antes de escrever, então mensagens de processos diferentes não se misturam).
//! - stdin: buffer de 8 KiB do processo, compartilhado por todos os handles.

use std::fmt;
use std::sync::Arc;

use sysabi::{Errno, Fd};

use crate::errno::from_errno;
use crate::proc::{self, Frame, lock};

// Tudo que é puro em `std::io` passa direto, então `use std::io::{self, ...}` vira
// `use sysio::io::{self, ...}` sem mais nada. Os itens definidos aqui sombreiam os do std.
pub use std::io::*;

const OUT_CAP: usize = 4096;
const IN_CAP: usize = 8192;

/// `std::io::IsTerminal` é selado (não dá pra implementar fora do std), então o porte troca o import
/// por este, que pergunta ao kernel (`isatty`).
pub trait IsTerminal {
    fn is_terminal(&self) -> bool;
}

impl<T: IsTerminal + ?Sized> IsTerminal for &T {
    fn is_terminal(&self) -> bool {
        (**self).is_terminal()
    }
}

impl<T: IsTerminal + ?Sized> IsTerminal for &mut T {
    fn is_terminal(&self) -> bool {
        (**self).is_terminal()
    }
}

pub(crate) fn isatty(fd: i32) -> bool {
    proc::sys().isatty(Fd(fd))
}

/// `write(2)` repetido até escrever tudo; EINTR é repetido.
pub(crate) fn write_all_fd(fd: i32, mut buf: &[u8]) -> Result<()> {
    let sys = proc::sys();
    while !buf.is_empty() {
        match sys.write(Fd(fd), buf) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(n) => buf = &buf[n..],
            Err(Errno::EINTR) => {}
            Err(e) => return Err(from_errno(e)),
        }
    }
    Ok(())
}

/// `read(2)` com EINTR repetido.
pub(crate) fn read_fd(fd: i32, buf: &mut [u8]) -> Result<usize> {
    let sys = proc::sys();
    loop {
        match sys.read(Fd(fd), buf) {
            Err(Errno::EINTR) => continue,
            r => return r.map_err(from_errno),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Buffers do frame

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Unbuffered,
    Line,
    Block,
}

/// Buffer do stdout de um frame.
#[derive(Debug)]
pub(crate) struct OutBuf {
    data: Vec<u8>,
    mode: Option<Mode>,
    /// Tamanho do bloco no modo `Block`.
    cap: usize,
    /// Erro de escrita engolido por `print!`/`println!` (que não devolvem erro): o fim do programa
    /// avisa `write error`, como o `close_stdout` do gnulib faz com o `ferror(stdout)`.
    swallowed: Option<i32>,
}

impl Default for OutBuf {
    fn default() -> Self {
        OutBuf { data: Vec::new(), mode: None, cap: OUT_CAP, swallowed: None }
    }
}

/// Modo pedido pelo `stdbuf` do pai: o GNU passa `_STDBUF_O`/`_STDBUF_E`/`_STDBUF_I` pro
/// `libstdbuf.so`, que chama `setvbuf` no início do programa; aqui o `sysio::run` lê as mesmas
/// variáveis. `0` = sem buffer, `L` = por linha, número (com sufixo K, M, G...) = bloco desse tamanho.
pub(crate) fn stdbuf_mode(var: &str) -> Option<(Mode, usize)> {
    let v = proc::sys().getenv(var.as_bytes())?;
    let v = String::from_utf8_lossy(&v).into_owned();
    match v.as_str() {
        "0" => Some((Mode::Unbuffered, 0)),
        "L" => Some((Mode::Line, OUT_CAP)),
        s => {
            let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
            let n: usize = digits.parse().ok()?;
            let mult: usize = match &s[digits.len()..] {
                "" => 1,
                "K" | "KiB" => 1 << 10,
                "M" | "MiB" => 1 << 20,
                "G" | "GiB" => 1 << 30,
                "kB" | "KB" => 1000,
                "MB" => 1_000_000,
                "GB" => 1_000_000_000,
                _ => return None,
            };
            let size = n.checked_mul(mult)?;
            (size > 0).then_some((Mode::Block, size))
        }
    }
}

impl OutBuf {
    fn mode(&mut self, buffered: bool) -> Mode {
        if let Some(m) = self.mode {
            return m;
        }
        let m = if !buffered {
            Mode::Unbuffered
        } else if let Some((m, cap)) = stdbuf_mode("_STDBUF_O") {
            self.cap = cap.max(1);
            m
        } else if isatty(1) {
            Mode::Line
        } else {
            Mode::Block
        };
        self.mode = Some(m);
        m
    }

    fn flush(&mut self) -> Result<()> {
        if self.data.is_empty() {
            return Ok(());
        }
        let r = write_all_fd(1, &self.data);
        self.data.clear();
        r
    }

    fn write(&mut self, buffered: bool, buf: &[u8]) -> Result<usize> {
        match self.mode(buffered) {
            Mode::Unbuffered => {
                self.flush()?;
                write_all_fd(1, buf)?;
            }
            Mode::Block => self.push_block(buf)?,
            Mode::Line => match buf.iter().rposition(|b| *b == b'\n') {
                Some(i) => {
                    self.data.extend_from_slice(&buf[..=i]);
                    self.flush()?;
                    self.push_block(&buf[i + 1..])?;
                }
                None => self.push_block(buf)?,
            },
        }
        Ok(buf.len())
    }

    fn push_block(&mut self, buf: &[u8]) -> Result<()> {
        if self.data.len() + buf.len() > self.cap {
            self.flush()?;
        }
        if buf.len() >= self.cap {
            write_all_fd(1, buf)
        } else {
            self.data.extend_from_slice(buf);
            Ok(())
        }
    }
}

/// Descarrega o stdout de um frame (fim do programa); devolve também um erro que `print!` engoliu.
pub(crate) fn flush_frame_stdout(frame: &Frame) -> Result<()> {
    let mut out = lock(&frame.stdout);
    out.flush()?;
    match out.swallowed.take() {
        Some(code) => Err(Error::from_raw_os_error(code)),
        None => Ok(()),
    }
}

/// Descarrega o stdout do processo corrente (antes de criar filho, de `exec` e de `exit`).
pub fn flush_stdout() -> Result<()> {
    let frame = proc::frame();
    lock(&frame.stdout).flush()
}

/// Buffer do stdin de um frame.
#[derive(Debug)]
pub(crate) struct InBuf {
    buf: Box<[u8]>,
    pos: usize,
    filled: usize,
}

impl Default for InBuf {
    fn default() -> Self {
        InBuf { buf: vec![0u8; IN_CAP].into_boxed_slice(), pos: 0, filled: 0 }
    }
}

impl InBuf {
    /// Buffer do stdin de um programa: `_STDBUF_I` do `stdbuf` (`0` = sem buffer, lê um byte por
    /// vez; número = tamanho), senão 8 KiB.
    pub(crate) fn for_program() -> InBuf {
        let cap = match stdbuf_mode("_STDBUF_I") {
            Some((Mode::Unbuffered, _)) => 1,
            Some((Mode::Block, n)) => n.min(64 << 20),
            _ => IN_CAP,
        };
        InBuf { buf: vec![0u8; cap].into_boxed_slice(), pos: 0, filled: 0 }
    }
}

impl InBuf {
    fn fill(&mut self) -> Result<&[u8]> {
        if self.pos >= self.filled {
            self.filled = read_fd(0, &mut self.buf)?;
            self.pos = 0;
        }
        Ok(&self.buf[self.pos..self.filled])
    }

    fn consume(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.filled);
    }

    fn read(&mut self, out: &mut [u8]) -> Result<usize> {
        if self.pos >= self.filled && out.len() >= self.buf.len() {
            return read_fd(0, out);
        }
        let avail = self.fill()?;
        let n = avail.len().min(out.len());
        out[..n].copy_from_slice(&avail[..n]);
        self.consume(n);
        Ok(n)
    }
}

// ---------------------------------------------------------------------------------------------
// stdin

#[derive(Clone, Debug)]
pub struct Stdin {
    frame: Arc<Frame>,
}

pub fn stdin() -> Stdin {
    Stdin { frame: proc::frame() }
}

impl Stdin {
    /// Leitor com buffer. O `'static` é o mesmo do std (o handle não empresta nada); enquanto o
    /// lock existe ele fica com o buffer do processo e devolve ao soltar.
    pub fn lock(&self) -> StdinLock<'static> {
        let taken = lock(&self.frame.stdin).take();
        StdinLock { buf: Some(taken.unwrap_or_default()), frame: Arc::clone(&self.frame), _p: std::marker::PhantomData }
    }

    pub fn read_line(&self, buf: &mut String) -> Result<usize> {
        self.lock().read_line(buf)
    }

    pub fn lines(self) -> Lines<StdinLock<'static>> {
        self.lock().lines()
    }
}

impl Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.lock().read(buf)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<usize> {
        self.lock().read_to_end(buf)
    }

    fn read_to_string(&mut self, buf: &mut String) -> Result<usize> {
        self.lock().read_to_string(buf)
    }
}

impl Read for &Stdin {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.lock().read(buf)
    }
}

impl IsTerminal for Stdin {
    fn is_terminal(&self) -> bool {
        isatty(0)
    }
}

#[derive(Debug)]
pub struct StdinLock<'a> {
    buf: Option<InBuf>,
    frame: Arc<Frame>,
    _p: std::marker::PhantomData<&'a ()>,
}

impl StdinLock<'_> {
    fn inner(&mut self) -> &mut InBuf {
        self.buf.get_or_insert_with(InBuf::default)
    }
}

impl Drop for StdinLock<'_> {
    fn drop(&mut self) {
        if let Some(b) = self.buf.take() {
            let mut slot = lock(&self.frame.stdin);
            if slot.is_none() {
                *slot = Some(b);
            }
        }
    }
}

impl Read for StdinLock<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.inner().read(buf)
    }
}

impl BufRead for StdinLock<'_> {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        self.buf.get_or_insert_with(InBuf::default).fill()
    }

    fn consume(&mut self, amt: usize) {
        self.inner().consume(amt);
    }
}

impl IsTerminal for StdinLock<'_> {
    fn is_terminal(&self) -> bool {
        isatty(0)
    }
}

// ---------------------------------------------------------------------------------------------
// stdout

#[derive(Clone, Debug)]
pub struct Stdout {
    frame: Arc<Frame>,
}

pub fn stdout() -> Stdout {
    Stdout { frame: proc::frame() }
}

/// "Lock" sem trava de verdade: cada `write` pega o mutex do buffer do processo, como o std faz.
#[derive(Debug)]
pub struct StdoutLock<'a> {
    frame: Arc<Frame>,
    _p: std::marker::PhantomData<&'a ()>,
}

impl Stdout {
    pub fn lock(&self) -> StdoutLock<'static> {
        StdoutLock { frame: Arc::clone(&self.frame), _p: std::marker::PhantomData }
    }
}

fn out_write(frame: &Frame, data: &[u8]) -> Result<usize> {
    lock(&frame.stdout).write(frame.buffered, data)
}

fn out_flush(frame: &Frame) -> Result<()> {
    lock(&frame.stdout).flush()
}

fn out_fmt(frame: &Frame, args: fmt::Arguments<'_>) -> Result<()> {
    match args.as_str() {
        Some(s) => out_write(frame, s.as_bytes()).map(|_| ()),
        None => out_write(frame, fmt::format(args).as_bytes()).map(|_| ()),
    }
}

impl Write for Stdout {
    fn write(&mut self, data: &[u8]) -> Result<usize> {
        out_write(&self.frame, data)
    }
    fn flush(&mut self) -> Result<()> {
        out_flush(&self.frame)
    }
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<()> {
        out_fmt(&self.frame, args)
    }
}

impl Write for &Stdout {
    fn write(&mut self, data: &[u8]) -> Result<usize> {
        out_write(&self.frame, data)
    }
    fn flush(&mut self) -> Result<()> {
        out_flush(&self.frame)
    }
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<()> {
        out_fmt(&self.frame, args)
    }
}

impl Write for StdoutLock<'_> {
    fn write(&mut self, data: &[u8]) -> Result<usize> {
        out_write(&self.frame, data)
    }
    fn flush(&mut self) -> Result<()> {
        out_flush(&self.frame)
    }
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<()> {
        out_fmt(&self.frame, args)
    }
}

impl IsTerminal for Stdout {
    fn is_terminal(&self) -> bool {
        isatty(1)
    }
}

impl IsTerminal for StdoutLock<'_> {
    fn is_terminal(&self) -> bool {
        isatty(1)
    }
}

// ---------------------------------------------------------------------------------------------
// stderr

#[derive(Clone, Copy, Debug, Default)]
pub struct Stderr {
    _priv: (),
}

pub fn stderr() -> Stderr {
    Stderr { _priv: () }
}

#[derive(Debug)]
pub struct StderrLock<'a> {
    _p: std::marker::PhantomData<&'a ()>,
}

impl Stderr {
    pub fn lock(&self) -> StderrLock<'static> {
        StderrLock { _p: std::marker::PhantomData }
    }
}

fn err_fmt(args: fmt::Arguments<'_>) -> Result<()> {
    match args.as_str() {
        Some(s) => write_all_fd(2, s.as_bytes()),
        None => write_all_fd(2, fmt::format(args).as_bytes()),
    }
}

impl Write for Stderr {
    fn write(&mut self, data: &[u8]) -> Result<usize> {
        write_all_fd(2, data).map(|()| data.len())
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<()> {
        err_fmt(args)
    }
}

impl Write for &Stderr {
    fn write(&mut self, data: &[u8]) -> Result<usize> {
        write_all_fd(2, data).map(|()| data.len())
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<()> {
        err_fmt(args)
    }
}

impl Write for StderrLock<'_> {
    fn write(&mut self, data: &[u8]) -> Result<usize> {
        write_all_fd(2, data).map(|()| data.len())
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<()> {
        err_fmt(args)
    }
}

impl IsTerminal for Stderr {
    fn is_terminal(&self) -> bool {
        isatty(2)
    }
}

impl IsTerminal for StderrLock<'_> {
    fn is_terminal(&self) -> bool {
        isatty(2)
    }
}

// ---------------------------------------------------------------------------------------------
// macros

/// `print!`/`println!`: como no std, não devolvem erro; um erro de escrita fica registrado e o fim
/// do programa avisa (`prog: write error: ...`), como o `close_stdout` do GNU.
pub fn print_fmt(args: fmt::Arguments<'_>) {
    let frame = proc::frame();
    if let Err(e) = out_fmt(&frame, args) {
        let code = e.raw_os_error().unwrap_or(crate::errno::EIO);
        lock(&frame.stdout).swallowed.get_or_insert(code);
    }
}

/// `eprint!`/`eprintln!`.
pub fn eprint_fmt(args: fmt::Arguments<'_>) {
    let _ = err_fmt(args);
}
