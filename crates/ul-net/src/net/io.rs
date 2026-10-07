//! Conexão TCP sobre o `net_connect` do kernel, com prazos por `poll`.
//!
//! A política de rede (allowlist, bloqueio de endereço interno) é do kernel: daqui só sai o pedido
//! "host:porta" e voltam o fd e os endereços. Leitura e escrita respeitam um prazo absoluto no relógio
//! monotônico do sandbox (o `--max-time` do curl e o `-T` do wget); prazo vencido vira
//! `io::ErrorKind::TimedOut`. Os erros do kernel chegam como `io::Error` com o errno original, pra
//! que cada programa escreva a mensagem que o original escreveria.

use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::time::Duration;

use sysabi::{Clock, Errno, Fd, PollEvents, PollFd, SysResult, sys};

/// Agora no relógio monotônico do sandbox.
pub fn now() -> Duration {
    match sys::current().clock_gettime(Clock::Monotonic) {
        Ok(t) => Duration::new(t.sec.max(0) as u64, t.nsec),
        Err(_) => Duration::ZERO,
    }
}

/// Agora no relógio de parede do sandbox (segundos e nanossegundos desde a época).
pub fn wall() -> (i64, u32) {
    match sys::current().clock_gettime(Clock::Realtime) {
        Ok(t) => (t.sec, t.nsec),
        Err(_) => (0, 0),
    }
}

/// Prazo absoluto (ou nenhum).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Deadline(Option<Duration>);

impl Deadline {
    pub const NONE: Deadline = Deadline(None);

    /// Prazo `d` a partir de agora.
    pub fn after(d: Option<Duration>) -> Deadline {
        Deadline(d.map(|d| now() + d))
    }

    /// O mais cedo dos dois.
    pub fn min(self, other: Deadline) -> Deadline {
        match (self.0, other.0) {
            (Some(a), Some(b)) => Deadline(Some(a.min(b))),
            (a, b) => Deadline(a.or(b)),
        }
    }

    /// Tempo que falta (`None` = sem prazo).
    pub fn remaining(&self) -> Option<Duration> {
        self.0.map(|at| at.saturating_sub(now()))
    }

    pub fn is_set(&self) -> bool {
        self.0.is_some()
    }

    pub fn expired(&self) -> bool {
        self.remaining().is_some_and(|r| r.is_zero())
    }
}

fn errno_io(e: Errno) -> io::Error {
    io::Error::from_raw_os_error(e.0)
}

/// Espera o fd ficar pronto pra `ev` até o prazo.
fn wait_ready(fd: Fd, ev: PollEvents, deadline: Deadline) -> io::Result<()> {
    let Some(rem) = deadline.remaining() else { return Ok(()) };
    if rem.is_zero() {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "timeout"));
    }
    let s = sys::current();
    loop {
        let mut fds = [PollFd { fd, events: ev, revents: PollEvents::empty() }];
        match s.poll(&mut fds, deadline.remaining()) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::TimedOut, "timeout")),
            Ok(_) => return Ok(()),
            Err(Errno::EINTR) => {
                if deadline.expired() {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "timeout"));
                }
            }
            Err(e) => return Err(errno_io(e)),
        }
    }
}

/// Conexão TCP aberta pelo kernel.
#[derive(Debug)]
pub struct Tcp {
    fd: Fd,
    pub peer: SocketAddr,
    pub local: SocketAddr,
    pub deadline: Deadline,
    /// Bytes lidos e escritos (pros contadores de transferência).
    pub read_bytes: u64,
    pub written_bytes: u64,
}

/// Abre uma conexão. O erro é o errno do kernel (ENOENT: nome não resolve; EACCES: política).
pub fn connect(host: &[u8], port: u16, timeout: Option<Duration>) -> SysResult<Tcp> {
    let s = sys::current();
    loop {
        match s.net_connect(host, port, timeout) {
            Ok(c) => {
                return Ok(Tcp {
                    fd: c.fd,
                    peer: c.peer,
                    local: c.local,
                    deadline: Deadline::NONE,
                    read_bytes: 0,
                    written_bytes: 0,
                });
            }
            Err(Errno::EINTR) => continue,
            Err(e) => return Err(e),
        }
    }
}

impl Tcp {
    pub fn fd(&self) -> Fd {
        self.fd
    }
}

impl Read for Tcp {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        wait_ready(self.fd, PollEvents::IN, self.deadline)?;
        let s = sys::current();
        loop {
            match s.read(self.fd, buf) {
                Ok(n) => {
                    self.read_bytes += n as u64;
                    return Ok(n);
                }
                Err(Errno::EINTR) => continue,
                Err(Errno::EAGAIN) => {
                    // O testkit devolve EAGAIN quando o prazo de leitura do socket vence.
                    if self.deadline.is_set() {
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "timeout"));
                    }
                    wait_ready(self.fd, PollEvents::IN, self.deadline)?;
                }
                Err(e) => return Err(errno_io(e)),
            }
        }
    }
}

impl Write for Tcp {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        wait_ready(self.fd, PollEvents::OUT, self.deadline)?;
        let s = sys::current();
        loop {
            match s.write(self.fd, buf) {
                Ok(n) => {
                    self.written_bytes += n as u64;
                    return Ok(n);
                }
                Err(Errno::EINTR) => continue,
                Err(Errno::EAGAIN) => wait_ready(self.fd, PollEvents::OUT, self.deadline)?,
                Err(e) => return Err(errno_io(e)),
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Tcp {
    fn drop(&mut self) {
        if let Some(s) = sys::try_current() {
            let _ = s.close(self.fd);
        }
    }
}
