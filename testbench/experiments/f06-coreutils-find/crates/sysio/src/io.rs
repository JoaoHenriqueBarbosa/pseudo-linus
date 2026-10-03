//! `stdin()`, `stdout()` e `stderr()` do processo corrente. Os traits (`Read`, `Write`, `BufRead`) e
//! os tipos puros (`BufReader`, `BufWriter`, `Error`) continuam os do std: só os handles mudam.

use std::io;
use std::sync::{Arc, Mutex};

use crate::proc::{self, Input, lock};

// Tudo que é puro em `std::io` (traits, `Error`, `BufReader`, `Cursor`...) passa direto, então
// `use std::io::{self, ...}` vira `use sysio::io::{self, ...}` sem mais nada. Os itens definidos
// aqui (`stdin`, `stdout`, `stderr`, `Stdin`, `Stdout`, `IsTerminal`...) sombreiam os do std.
pub use std::io::*;

/// `std::io::IsTerminal` é selado (não dá pra implementar fora do std), então o porte troca o import
/// por este. Nada no pseudo-processo é terminal nos casos da bancada.
pub trait IsTerminal {
    fn is_terminal(&self) -> bool;
}

// ---------------------------------------------------------------------------------------------
// stdin

#[derive(Clone, Debug)]
pub struct Stdin {
    input: Arc<Mutex<Input>>,
}

pub fn stdin() -> Stdin {
    Stdin { input: Arc::clone(&proc::current().stdin) }
}

impl Stdin {
    pub fn lock(&self) -> StdinLock<'static> {
        let (data, pos) = {
            let g = lock(&self.input);
            (Arc::clone(&g.data), g.pos)
        };
        StdinLock { input: Arc::clone(&self.input), data, pos, _p: std::marker::PhantomData }
    }

    pub fn read_line(&self, buf: &mut String) -> io::Result<usize> {
        self.lock().read_line(buf)
    }

    pub fn lines(self) -> io::Lines<StdinLock<'static>> {
        self.lock().lines()
    }
}

impl Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut g = lock(&self.input);
        let start = g.pos.min(g.data.len());
        let n = buf.len().min(g.data.len() - start);
        buf[..n].copy_from_slice(&g.data[start..start + n]);
        g.pos = start + n;
        Ok(n)
    }
}

impl IsTerminal for Stdin {
    fn is_terminal(&self) -> bool {
        false
    }
}

/// Leitor com buffer sobre a entrada: lê direto do `Arc<Vec<u8>>` e devolve a posição ao soltar.
#[derive(Debug)]
pub struct StdinLock<'a> {
    input: Arc<Mutex<Input>>,
    data: Arc<Vec<u8>>,
    pos: usize,
    _p: std::marker::PhantomData<&'a ()>,
}

impl Read for StdinLock<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let start = self.pos.min(self.data.len());
        let n = buf.len().min(self.data.len() - start);
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        self.pos = start + n;
        lock(&self.input).pos = self.pos;
        Ok(n)
    }
}

impl BufRead for StdinLock<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        let start = self.pos.min(self.data.len());
        Ok(&self.data[start..])
    }
    fn consume(&mut self, amt: usize) {
        self.pos = (self.pos + amt).min(self.data.len());
        lock(&self.input).pos = self.pos;
    }
}

impl IsTerminal for StdinLock<'_> {
    fn is_terminal(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------------------------
// stdout e stderr

#[derive(Clone, Debug)]
pub struct Stdout {
    buf: Arc<Mutex<Vec<u8>>>,
}

#[derive(Clone, Debug)]
pub struct Stderr {
    buf: Arc<Mutex<Vec<u8>>>,
}

pub fn stdout() -> Stdout {
    Stdout { buf: Arc::clone(&proc::current().stdout) }
}

pub fn stderr() -> Stderr {
    Stderr { buf: Arc::clone(&proc::current().stderr) }
}

/// "Lock" sem trava de verdade: cada `write` pega o mutex do buffer, como o std faz por linha.
pub type StdoutLock<'a> = Lock<'a>;
pub type StderrLock<'a> = Lock<'a>;

#[derive(Debug)]
pub struct Lock<'a> {
    buf: Arc<Mutex<Vec<u8>>>,
    _p: std::marker::PhantomData<&'a ()>,
}

impl Stdout {
    pub fn lock(&self) -> StdoutLock<'static> {
        Lock { buf: Arc::clone(&self.buf), _p: std::marker::PhantomData }
    }
}

impl Stderr {
    pub fn lock(&self) -> StderrLock<'static> {
        Lock { buf: Arc::clone(&self.buf), _p: std::marker::PhantomData }
    }
}

fn append(buf: &Mutex<Vec<u8>>, data: &[u8]) -> io::Result<usize> {
    lock(buf).extend_from_slice(data);
    Ok(data.len())
}

impl Write for Stdout {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        append(&self.buf, data)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for &Stdout {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        append(&self.buf, data)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for Stderr {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        append(&self.buf, data)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for &Stderr {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        append(&self.buf, data)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for Lock<'_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        append(&self.buf, data)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl IsTerminal for Stdout {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl IsTerminal for Stderr {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl IsTerminal for Lock<'_> {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl IsTerminal for crate::fs::File {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl crate::fs::Fstat for Stdin {
    fn fstat(&self) -> io::Result<crate::fs::Metadata> {
        Ok(crate::fs::pipe_metadata(1))
    }
}

impl crate::fs::Fstat for StdinLock<'_> {
    fn fstat(&self) -> io::Result<crate::fs::Metadata> {
        Ok(crate::fs::pipe_metadata(1))
    }
}

impl crate::fs::Fstat for Stdout {
    fn fstat(&self) -> io::Result<crate::fs::Metadata> {
        Ok(crate::fs::pipe_metadata(2))
    }
}

impl crate::fs::Fstat for Stderr {
    fn fstat(&self) -> io::Result<crate::fs::Metadata> {
        Ok(crate::fs::pipe_metadata(3))
    }
}

impl crate::fs::Fstat for Lock<'_> {
    fn fstat(&self) -> io::Result<crate::fs::Metadata> {
        Ok(crate::fs::pipe_metadata(2))
    }
}

/// Escreve formatado no stdout do processo (usado pelas macros `print!`/`println!`).
pub fn print_fmt(args: std::fmt::Arguments<'_>) {
    let _ = stdout().write_fmt(args);
}

/// Escreve formatado no stderr do processo (usado pelas macros `eprint!`/`eprintln!`).
pub fn eprint_fmt(args: std::fmt::Arguments<'_>) {
    let _ = stderr().write_fmt(args);
}
