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

use crate::park::{Parker, Wake, WaitList};

pub(crate) const PIPE_BUF: usize = 4096;
pub(crate) const PIPE_CAPACITY: usize = 65536;
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

#[derive(Debug, Default)]
struct State {
    buf: VecDeque<u8>,
    readers: u32,
    writers: u32,
    /// Quantas vezes um leitor/escritor abriu (FIFO).
    r_counter: u64,
    w_counter: u64,
    rwait: WaitList,
    wwait: WaitList,
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

    /// Leitura: dados, EOF (0) sem escritores, EAGAIN não bloqueante, ou espera.
    pub(crate) fn try_read(&self, buf: &mut [u8], nonblock: bool, waiter: &Arc<Parker>) -> Try<Result<usize, Errno>> {
        let (r, wake) = {
            let mut s = self.st.lock();
            if buf.is_empty() {
                return Try::Ready(Ok(0));
            }
            if !s.buf.is_empty() {
                let n = buf.len().min(s.buf.len());
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
                s.rwait.unregister(waiter);
                (Try::Ready(Ok(n)), s.wwait.take())
            } else if s.writers == 0 {
                s.rwait.unregister(waiter);
                (Try::Ready(Ok(0)), Wake::none())
            } else if nonblock {
                (Try::Ready(Err(Errno::EAGAIN)), Wake::none())
            } else {
                s.rwait.register(waiter);
                (Try::Pending, Wake::none())
            }
        };
        wake.run();
        r
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
        let (r, wake) = {
            let mut s = self.st.lock();
            if s.readers == 0 {
                s.wwait.unregister(waiter);
                return Try::Ready(Err(WriteError::BrokenPipe { written: *done }));
            }
            let free = PIPE_CAPACITY.saturating_sub(s.buf.len());
            let rest = &data[*done..];
            let atomic = data.len() <= PIPE_BUF;
            let mut wake = Wake::none();
            if atomic {
                if free >= rest.len() {
                    s.buf.extend(rest);
                    *done += rest.len();
                }
            } else if free > 0 {
                let n = free.min(rest.len());
                s.buf.extend(&rest[..n]);
                *done += n;
            }
            if *done > 0 && !s.buf.is_empty() {
                wake.merge(s.rwait.take());
            }
            if *done == data.len() {
                s.wwait.unregister(waiter);
                (Try::Ready(Ok(*done)), wake)
            } else if nonblock {
                let r = if *done > 0 { Ok(*done) } else { Err(WriteError::Again) };
                (Try::Ready(r), wake)
            } else {
                s.wwait.register(waiter);
                (Try::Pending, wake)
            }
        };
        wake.run();
        r
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
        }
        if write_end {
            if s.readers == 0 {
                ev |= PollEvents::ERR;
            } else if PIPE_CAPACITY - s.buf.len() >= PIPE_BUF {
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
