//! Pipes e FIFOs (`fs/pipe.c`, `fs/fifo.c`).
//!
//! - Capacidade de 65536 bytes; escrita de até PIPE_BUF (4096) bytes é atômica (espera caber inteira);
//!   acima disso a escrita é parcial e pode intercalar com outros escritores.
//! - Leitura de pipe vazio sem escritores é EOF; escrita sem leitores dá SIGPIPE e EPIPE (o chamador
//!   manda o sinal).
//! - FIFO: `open` só pra leitura espera um escritor e vice-versa (com os contadores `r_counter` e
//!   `w_counter` do kernel, pra um escritor que abre e fecha rápido ainda acordar o leitor); com
//!   O_NONBLOCK, leitura abre na hora e escrita sem leitor dá ENXIO; O_RDWR nunca espera.

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use sysabi::{Errno, Gid, PollEvents, Stat, TimeSpec, Uid};
use vfs::MagicObject;

use crate::park::{Parker, Wake, WaitList, key, locked};
use crate::scm::{Marks, Scm};

pub(crate) const PIPE_BUF: usize = 4096;
pub(crate) const PIPE_PAGE: usize = sysabi::PIPE_PAGE_SIZE;
pub(crate) const PIPE_CAPACITY: usize = sysabi::PIPE_DEFAULT_SIZE;
/// `st_dev` do pipefs (0:14 no Debian 13, como o `stat` de `/proc/self/fd/N` de um pipe mostra).
pub(crate) const PIPEFS_DEV: u64 = 0xe;

/// Resultado de uma tentativa que pode ter de esperar.
pub(crate) enum Try<T> {
    Ready(T),
    /// O parker foi registrado; dormir e tentar de novo.
    Pending,
}

#[derive(Debug)]
pub(crate) enum WriteError {
    /// Sem leitores: o chamador manda SIGPIPE. `written` já foi pro buffer.
    BrokenPipe { written: usize },
    Again,
}

#[derive(Debug)]
struct State {
    /// A capacidade em bytes (`max_usage` páginas): 64 KiB até um `F_SETPIPE_SZ`.
    cap: usize,
    buf: VecDeque<u8>,
    readers: u32,
    writers: u32,
    /// Quantas vezes um leitor/escritor abriu (FIFO).
    r_counter: u64,
    w_counter: u64,
    /// `RCV_SHUTDOWN` de um socket: o que já está no buffer sai e depois a leitura dá EOF mesmo com escritores.
    rcv_shut: bool,
    /// Escrever aqui é EPIPE mesmo com leitores: `shutdown(SHUT_RD)` de um socket Unix (o par vê `RCV_SHUTDOWN`
    /// do outro lado e o `unix_stream_sendmsg` recusa) ou `shutdown(SHUT_WR)` de quem escreve.
    wr_closed: bool,
    /// Dados auxiliares de um fluxo Unix (`SCM_RIGHTS`, credenciais) ao lado dos bytes.
    marks: Marks,
    rwait: WaitList,
    wwait: WaitList,
}

impl Default for State {
    fn default() -> State {
        State {
            cap: PIPE_CAPACITY,
            buf: VecDeque::new(),
            readers: 0,
            writers: 0,
            r_counter: 0,
            w_counter: 0,
            rcv_shut: false,
            wr_closed: false,
            marks: Marks::default(),
            rwait: WaitList::default(),
            wwait: WaitList::default(),
        }
    }
}

/// Um pipe (anônimo ou o objeto por trás de uma FIFO).
#[derive(Debug)]
pub(crate) struct Pipe {
    pub ino: u64,
    pub uid: Uid,
    pub gid: Gid,
    pub ctime: TimeSpec,
    st: Mutex<State>,
}

impl Pipe {
    pub(crate) fn new(ino: u64, uid: Uid, gid: Gid, ctime: TimeSpec) -> Arc<Pipe> {
        Arc::new(Pipe { ino, uid, gid, ctime, st: Mutex::new(State::default()) })
    }

    /// Abre uma ponta (pipe anônimo: as duas pontas já nascem abertas).
    pub(crate) fn attach(self: &Arc<Self>, read: bool, write: bool) -> PipeEnd {
        let w = {
            let mut s = self.st.lock();
            let mut w = Wake::none();
            if read {
                s.readers += 1;
                s.r_counter += 1;
                w.merge(s.wwait.take());
            }
            if write {
                s.writers += 1;
                s.w_counter += 1;
                w.merge(s.rwait.take());
            }
            w
        };
        w.run();
        PipeEnd { pipe: self.clone(), read, write }
    }

    pub(crate) fn stat(&self, fifo_stat: Option<&Stat>) -> Stat {
        if let Some(st) = fifo_stat {
            return st.clone();
        }
        Stat {
            dev: PIPEFS_DEV,
            ino: self.ino,
            mode: sysabi::mode::S_IFIFO | 0o600,
            nlink: 1,
            uid: self.uid,
            gid: self.gid,
            rdev: 0,
            size: 0,
            blksize: 4096,
            blocks: 0,
            atime: self.ctime,
            mtime: self.ctime,
            ctime: self.ctime,
            btime: None,
        }
    }

    /// Leitura: dados, EOF (0) sem escritores, EAGAIN não bloqueante, ou espera. Os descritores de um fluxo
    /// Unix que a leitura toca fecham (um `read` sem `msg_control` os descarta).
    pub(crate) fn try_read(&self, buf: &mut [u8], nonblock: bool, waiter: &Arc<Parker>) -> Try<Result<usize, Errno>> {
        match self.try_read_scm(buf, nonblock, waiter, false) {
            Try::Ready(r) => Try::Ready(r.map(|(n, _)| n)),
            Try::Pending => Try::Pending,
        }
    }

    /// [`Pipe::try_read`] com os dados auxiliares: a leitura para onde o `sk_buff` com descritores acaba e,
    /// com `passcred`, onde o remetente muda (ver [`Marks::window`]).
    pub(crate) fn try_read_scm(&self, buf: &mut [u8], nonblock: bool, waiter: &Arc<Parker>, passcred: bool) -> Try<Result<(usize, Scm), Errno>> {
        locked(&self.st, |s| {
            if buf.is_empty() {
                return (Try::Ready(Ok((0, Scm::default()))), Wake::none());
            }
            if !s.buf.is_empty() {
                let window = s.marks.window(buf.len(), s.buf.len(), passcred);
                let n = window.len;
                let (a, b) = s.buf.as_slices();
                let na = n.min(a.len());
                buf[..na].copy_from_slice(&a[..na]);
                if n > na {
                    buf[na..n].copy_from_slice(&b[..n - na]);
                }
                s.buf.drain(..n);
                if s.buf.is_empty() {
                    s.buf.shrink_to(PIPE_BUF);
                }
                let scm = Scm { fds: s.marks.consume(&window), cred: window.cred };
                s.rwait.unregister(waiter);
                (Try::Ready(Ok((n, scm))), s.wwait.take_key(key::PIPE_WRITE))
            } else if s.writers == 0 || s.rcv_shut {
                s.rwait.unregister(waiter);
                (Try::Ready(Ok((0, Scm::default()))), Wake::none())
            } else if nonblock {
                (Try::Ready(Err(Errno::EAGAIN)), Wake::none())
            } else {
                s.rwait.register(waiter);
                (Try::Pending, Wake::none())
            }
        })
    }

    /// `shutdown(SHUT_RD)` de quem lê este pipe (`RCV_SHUTDOWN`): o que já chegou segue legível e a fila vazia
    /// dá EOF sem esperar o par. Quem espera para ler acorda na hora, com a marca posta sob a mesma trava em que
    /// ele se registra. `refuse_writes` (Unix) faz o par ver EPIPE e acorda os escritores bloqueados; no TCP o
    /// par continua escrevendo.
    pub(crate) fn shutdown_read(&self, refuse_writes: bool) {
        locked(&self.st, |s| {
            s.rcv_shut = true;
            let mut wake = s.rwait.take();
            if refuse_writes {
                s.wr_closed = true;
                wake.merge(s.wwait.take());
            }
            ((), wake)
        });
    }

    /// `shutdown(SHUT_WR)` de quem escreve neste pipe (`SEND_SHUTDOWN`): a escrita bloqueada por falta de espaço
    /// acorda e dá EPIPE (`sk_stream_wait_memory`).
    pub(crate) fn shutdown_write(&self) {
        locked(&self.st, |s| {
            s.wr_closed = true;
            ((), s.wwait.take())
        });
    }

    /// `shutdown(SHUT_RD)` já foi feito por quem lê.
    pub(crate) fn rcv_shut(&self) -> bool {
        self.st.lock().rcv_shut
    }

    /// Ainda há quem aceite escrita: leitor aberto e nenhum `shutdown` fechando o sentido.
    pub(crate) fn accepts_writes(&self) -> bool {
        let s = self.st.lock();
        s.readers > 0 && !s.wr_closed
    }

    /// `MSG_PEEK` de um socket de fluxo: copia para `buf` o que está na fila, sem consumi-lo, e devolve quantos
    /// bytes. Com menos de `want` bytes e escritores, a leitura bloqueante espera mais e a não bloqueante leva o
    /// que tem (`MSG_WAITALL`); fila vazia sem escritores é EOF (0); com escritores, EAGAIN (`nonblock`) ou
    /// registra o parker. Os descritores do `sk_buff` que a leitura tocaria vêm duplicados e continuam na fila.
    pub(crate) fn try_peek(&self, buf: &mut [u8], want: usize, nonblock: bool, waiter: &Arc<Parker>, passcred: bool) -> Try<Result<(usize, Scm), Errno>> {
        locked(&self.st, |s| {
            let have = s.buf.len();
            if have > 0 && (have >= want || s.writers == 0 || s.rcv_shut || nonblock) {
                let window = s.marks.window(buf.len(), have, passcred);
                for (dst, src) in buf[..window.len].iter_mut().zip(s.buf.iter()) {
                    *dst = *src;
                }
                s.rwait.unregister(waiter);
                (Try::Ready(Ok((window.len, Scm { fds: s.marks.peek_fds(&window), cred: window.cred }))), Wake::none())
            } else if have == 0 && (s.writers == 0 || s.rcv_shut) {
                s.rwait.unregister(waiter);
                (Try::Ready(Ok((0, Scm::default()))), Wake::none())
            } else if nonblock {
                (Try::Ready(Err(Errno::EAGAIN)), Wake::none())
            } else {
                s.rwait.register(waiter);
                (Try::Pending, Wake::none())
            }
        })
    }

    /// Escrita, retomável: `done` conta o que já foi escrito em tentativas anteriores da mesma
    /// chamada. Até PIPE_BUF a escrita espera caber inteira.
    pub(crate) fn try_write(
        &self,
        data: &[u8],
        done: &mut usize,
        nonblock: bool,
        waiter: &Arc<Parker>,
    ) -> Try<Result<usize, WriteError>> {
        self.try_write_with(data, done, nonblock, waiter, None)
    }

    /// [`Pipe::try_write`] de um `sendmsg` Unix: `send` leva as credenciais de quem escreve e os descritores
    /// (que ficam ao lado do primeiro `sk_buff`, o primeiro trecho escrito).
    pub(crate) fn try_write_with(
        &self,
        data: &[u8],
        done: &mut usize,
        nonblock: bool,
        waiter: &Arc<Parker>,
        send: Option<&Scm>,
    ) -> Try<Result<usize, WriteError>> {
        locked(&self.st, |s| {
            if s.readers == 0 || s.wr_closed {
                s.wwait.unregister(waiter);
                return (Try::Ready(Err(WriteError::BrokenPipe { written: *done })), Wake::none());
            }
            let free = s.cap.saturating_sub(s.buf.len());
            let first = *done == 0;
            let rest = &data[*done..];
            let atomic = data.len() <= PIPE_BUF;
            let mut wake = Wake::none();
            if atomic {
                if free >= rest.len() {
                    s.buf.extend(rest);
                    s.marks.wrote(rest.len(), send, first, data.len());
                    *done += rest.len();
                }
            } else if free > 0 {
                let n = free.min(rest.len());
                s.buf.extend(&rest[..n]);
                s.marks.wrote(n, send, first, data.len());
                *done += n;
            }
            if *done > 0 && !s.buf.is_empty() {
                wake.merge(s.rwait.take_key(key::PIPE_READ));
            }
            (s.wwait.write_outcome(*done, data.len(), nonblock, WriteError::Again, waiter), wake)
        })
    }

    /// Prontidão pro `poll`, registrando o parker nas duas filas quando pedido.
    pub(crate) fn poll(&self, read_end: bool, write_end: bool, waiter: Option<&Arc<Parker>>) -> PollEvents {
        let mut s = self.st.lock();
        let mut ev = PollEvents::empty();
        if read_end {
            if !s.buf.is_empty() {
                ev |= PollEvents::IN;
            }
            if s.writers == 0 && s.w_counter > 0 {
                ev |= PollEvents::HUP;
            }
            if s.rcv_shut {
                ev |= PollEvents::IN | PollEvents::RDHUP;
            }
        }
        if write_end {
            if s.readers == 0 || s.wr_closed {
                ev |= PollEvents::ERR;
            } else if s.cap.saturating_sub(s.buf.len()) >= PIPE_BUF {
                ev |= PollEvents::OUT;
            }
        }
        if let Some(w) = waiter {
            if read_end {
                s.rwait.register(w);
            }
            if write_end {
                s.wwait.register(w);
            }
        }
        ev
    }

    pub(crate) fn unregister(&self, waiter: &Arc<Parker>) {
        let mut s = self.st.lock();
        s.rwait.unregister(waiter);
        s.wwait.unregister(waiter);
    }

    /// Encontro de uma FIFO: conta a ponta e diz se ainda precisa esperar a outra. `seen` é o contador
    /// da outra ponta no momento em que a espera começou.
    pub(crate) fn fifo_wait_peer(&self, want_writer: bool, seen: u64, waiter: &Arc<Parker>) -> bool {
        let mut s = self.st.lock();
        let ready = if want_writer { s.writers > 0 || s.w_counter != seen } else { s.readers > 0 || s.r_counter != seen };
        if ready {
            s.rwait.unregister(waiter);
            s.wwait.unregister(waiter);
            return true;
        }
        if want_writer {
            s.rwait.register(waiter);
        } else {
            s.wwait.register(waiter);
        }
        false
    }

    pub(crate) fn counters(&self) -> (u32, u32, u64, u64) {
        let s = self.st.lock();
        (s.readers, s.writers, s.r_counter, s.w_counter)
    }

    /// Bytes escritos e ainda não lidos.
    pub(crate) fn pending(&self) -> usize {
        self.st.lock().buf.len()
    }

    /// `F_GETPIPE_SZ`: a capacidade em bytes.
    pub(crate) fn capacity(&self) -> usize {
        self.st.lock().cap
    }

    /// `pipe_set_size`: a capacidade passa a `round_pipe_size(size)` e devolve ela. Subir além de `max_size`
    /// pede `privileged` (`CAP_SYS_RESOURCE`, EPERM); encolher abaixo das páginas ocupadas é EBUSY; o que
    /// estoura 2^31 é EINVAL. Mais espaço acorda os escritores que esperavam.
    pub(crate) fn resize(&self, size: u32, max_size: u32, privileged: bool) -> Result<usize, Errno> {
        let rounded = sysabi::round_pipe_size(size).ok_or(Errno::EINVAL)? as usize;
        let mut s = self.st.lock();
        if rounded > s.cap && rounded > max_size as usize && !privileged {
            return Err(Errno::EPERM);
        }
        if rounded / PIPE_PAGE < s.buf.len().div_ceil(PIPE_PAGE) {
            return Err(Errno::EBUSY);
        }
        s.cap = rounded;
        let w = s.wwait.take();
        drop(s);
        w.run();
        Ok(rounded)
    }
}

/// Uma ponta aberta (dentro de uma descrição de arquivo aberto). Fechar a última ponta de um lado acorda
/// o outro (EOF pro leitor, EPIPE pro escritor).
#[derive(Debug)]
pub(crate) struct PipeEnd {
    pub pipe: Arc<Pipe>,
    pub read: bool,
    pub write: bool,
}

impl Drop for PipeEnd {
    fn drop(&mut self) {
        let w = {
            let mut s = self.pipe.st.lock();
            let mut w = Wake::none();
            if self.read {
                s.readers -= 1;
                if s.readers == 0 {
                    w.merge(s.wwait.take());
                }
            }
            if self.write {
                s.writers -= 1;
                if s.writers == 0 {
                    w.merge(s.rwait.take());
                }
            }
            w
        };
        w.run();
    }
}

/// O pipe como objeto de magic link (`/proc/self/fd/N`): reabrir dá uma ponta nova do mesmo pipe.
#[derive(Debug)]
pub(crate) struct PipeObject {
    pub pipe: Arc<Pipe>,
    /// `stat` do inode da FIFO, quando o pipe é de uma FIFO nomeada.
    pub fifo_stat: Option<Stat>,
}

impl MagicObject for PipeObject {
    fn stat(&self) -> Stat {
        self.pipe.stat(self.fifo_stat.as_ref())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
