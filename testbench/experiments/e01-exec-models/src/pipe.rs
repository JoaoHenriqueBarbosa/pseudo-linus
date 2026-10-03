//! Pipe com buffer circular de 64 KiB, igual nos três modelos.
//!
//! As operações são não bloqueantes no estilo `poll`: devolvem `Poll::Pending` e registram o `Waker` do
//! processo. Os modelos síncronos (A e B) transformam `Pending` em `block()`; o modelo C devolve `Pending`
//! pro executor. Semântica: leitura devolve 0 (EOF) quando o buffer está vazio e não há escritores;
//! escrita sem leitores dá EPIPE (o chamador transforma em SIGPIPE).

use std::collections::VecDeque;
use std::sync::Arc;
use std::task::{Poll, Waker};

use parking_lot::Mutex;

use crate::kernel::Errno;

pub const PIPE_CAPACITY: usize = 64 * 1024;

struct PipeState {
    buf: VecDeque<u8>,
    readers: usize,
    writers: usize,
    read_waiters: Vec<Waker>,
    write_waiters: Vec<Waker>,
}

pub struct Pipe {
    state: Mutex<PipeState>,
}

/// Registra um waker. Só deduplica contra o último (o caso comum de um processo que volta a esperar);
/// duplicata só custa um wake extra, e varrer a lista inteira seria O(n²) com milhares de esperando.
pub(crate) fn register(list: &mut Vec<Waker>, waker: &Waker) {
    if !list.last().is_some_and(|w| w.will_wake(waker)) {
        list.push(waker.clone());
    }
}

fn wake_all(list: Vec<Waker>) {
    for w in list {
        w.wake();
    }
}

impl Pipe {
    pub fn new_pair() -> (PipeReader, PipeWriter) {
        let pipe = Arc::new(Pipe {
            state: Mutex::new(PipeState {
                buf: VecDeque::with_capacity(PIPE_CAPACITY),
                readers: 1,
                writers: 1,
                read_waiters: Vec::new(),
                write_waiters: Vec::new(),
            }),
        });
        (PipeReader(pipe.clone()), PipeWriter(pipe))
    }

    /// Lê até `out.len()` bytes. `Ready(0)` é EOF.
    pub fn poll_read(&self, out: &mut [u8], waker: &Waker) -> Poll<usize> {
        let mut st = self.state.lock();
        if st.buf.is_empty() {
            if st.writers == 0 {
                return Poll::Ready(0);
            }
            register(&mut st.read_waiters, waker);
            return Poll::Pending;
        }
        let n = out.len().min(st.buf.len());
        let (a, b) = st.buf.as_slices();
        let na = n.min(a.len());
        out[..na].copy_from_slice(&a[..na]);
        if n > na {
            out[na..n].copy_from_slice(&b[..n - na]);
        }
        st.buf.drain(..n);
        let wakers = std::mem::take(&mut st.write_waiters);
        drop(st);
        wake_all(wakers);
        Poll::Ready(n)
    }

    /// Escreve o quanto couber (pelo menos 1 byte) ou registra o waker se o buffer está cheio.
    pub fn poll_write(&self, data: &[u8], waker: &Waker) -> Poll<Result<usize, Errno>> {
        let mut st = self.state.lock();
        if st.readers == 0 {
            return Poll::Ready(Err(Errno::Pipe));
        }
        let space = PIPE_CAPACITY - st.buf.len();
        if space == 0 {
            register(&mut st.write_waiters, waker);
            return Poll::Pending;
        }
        let n = space.min(data.len());
        st.buf.extend(&data[..n]);
        let wakers = std::mem::take(&mut st.read_waiters);
        drop(st);
        wake_all(wakers);
        Poll::Ready(Ok(n))
    }

    /// Quantos leitores estão esperando dados (usado pela bancada pra saber que um processo bloqueou).
    pub fn read_waiters(&self) -> usize {
        self.state.lock().read_waiters.len()
    }

    pub fn readers(&self) -> usize {
        self.state.lock().readers
    }

    pub fn writers(&self) -> usize {
        self.state.lock().writers
    }
}

/// Ponta de leitura. Clonar conta mais um leitor; o Drop é o `close`.
pub struct PipeReader(Arc<Pipe>);

/// Ponta de escrita. Clonar conta mais um escritor; o Drop é o `close`.
pub struct PipeWriter(Arc<Pipe>);

impl PipeReader {
    pub fn pipe(&self) -> &Arc<Pipe> {
        &self.0
    }
}

impl PipeWriter {
    pub fn pipe(&self) -> &Arc<Pipe> {
        &self.0
    }
}

impl Clone for PipeReader {
    fn clone(&self) -> PipeReader {
        self.0.state.lock().readers += 1;
        PipeReader(self.0.clone())
    }
}

impl Clone for PipeWriter {
    fn clone(&self) -> PipeWriter {
        self.0.state.lock().writers += 1;
        PipeWriter(self.0.clone())
    }
}

impl Drop for PipeReader {
    fn drop(&mut self) {
        let wakers = {
            let mut st = self.0.state.lock();
            st.readers -= 1;
            if st.readers == 0 { std::mem::take(&mut st.write_waiters) } else { Vec::new() }
        };
        wake_all(wakers);
    }
}

impl Drop for PipeWriter {
    fn drop(&mut self) {
        let wakers = {
            let mut st = self.0.state.lock();
            st.writers -= 1;
            if st.writers == 0 { std::mem::take(&mut st.read_waiters) } else { Vec::new() }
        };
        wake_all(wakers);
    }
}
