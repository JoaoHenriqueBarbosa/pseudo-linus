//! Espera e despertar.
//!
//! Cada pseudo-processo (e cada thread do host que espera um pipe ou um processo) tem um [`Parker`]:
//! uma ficha com flag, como o `park`/`unpark` da std, mas próprio (nenhum código de terceiros consome a
//! ficha). Quem espera um objeto se registra na [`WaitList`] dele, confere a condição sob a trava do
//! objeto e dorme; quem muda o objeto acorda todos os registrados. A flag impede despertar perdido entre
//! soltar a trava e dormir.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use parking_lot::{Condvar, Mutex};

/// As chaves de despertar (`key` do `wake_up_interruptible_sync_poll`) que as filas dos arquivos passam: os
/// bits do `EPOLL*` do evento que mudou. Chave 0 é o despertar sem chave (`wake_up_all`, close, HUP): acorda
/// todos.
pub(crate) mod key {
    use sysabi::epoll as ev;

    /// `sock_def_readable`.
    pub(crate) const READ: u32 = ev::IN | ev::PRI | ev::RDNORM | ev::RDBAND;
    /// `sk_stream_write_space` e `sock_def_write_space`.
    pub(crate) const WRITE: u32 = ev::OUT | ev::WRNORM | ev::WRBAND;
    /// `pipe_write` acorda os leitores.
    pub(crate) const PIPE_READ: u32 = ev::IN | ev::RDNORM;
    /// `pipe_read` acorda os escritores.
    pub(crate) const PIPE_WRITE: u32 = ev::OUT | ev::WRNORM;
}

#[derive(Debug, Default)]
pub(crate) struct Parker {
    notified: Mutex<bool>,
    cv: Condvar,
    /// O `epi->event.events` de uma entrada de epoll: o `ep_poll_callback` descarta o despertar cuja chave não
    /// cruza a máscara. 0 aceita qualquer chave (o `poll(2)`, que só ganha despertares a mais).
    filter: AtomicU32,
}

impl Parker {
    pub(crate) fn new() -> Arc<Parker> {
        Arc::new(Parker::default())
    }

    /// Passa a ignorar despertares cuja chave não cruza `mask` (0 volta a aceitar todos).
    pub(crate) fn set_filter(&self, mask: u32) {
        self.filter.store(mask, Ordering::Relaxed);
    }

    /// O despertar de chave `key` chega a este parker: `if (pollflags && !(pollflags & epi->event.events))`.
    fn accepts(&self, key: u32) -> bool {
        let mask = self.filter.load(Ordering::Relaxed);
        key == 0 || mask == 0 || key & mask != 0
    }

    pub(crate) fn unpark(&self) {
        let mut n = self.notified.lock();
        if !*n {
            *n = true;
            self.cv.notify_one();
        }
    }

    /// Houve `unpark` desde a última consulta (ou `park`): consome a ficha sem dormir. O `EPOLLET` usa um
    /// parker por entrada para saber que o arquivo acordou os observadores (a chegada de dado novo).
    pub(crate) fn take_notified(&self) -> bool {
        std::mem::take(&mut *self.notified.lock())
    }

    /// Como [`Parker::take_notified`], sem consumir a ficha.
    pub(crate) fn is_notified(&self) -> bool {
        *self.notified.lock()
    }

    /// Dorme até um `unpark` (ou volta na hora se já houve um desde o último `park`).
    pub(crate) fn park(&self) {
        let mut n = self.notified.lock();
        while !*n {
            self.cv.wait(&mut n);
        }
        *n = false;
    }

    /// Como [`Parker::park`], com prazo. Devolve `false` se o prazo venceu sem `unpark`.
    pub(crate) fn park_until(&self, deadline: Instant) -> bool {
        let mut n = self.notified.lock();
        while !*n {
            if self.cv.wait_until(&mut n, deadline).timed_out() {
                if *n {
                    break;
                }
                return false;
            }
        }
        *n = false;
        true
    }
}

/// Lista de quem espera um objeto. Fica dentro do estado do objeto (sob a trava dele).
#[derive(Debug, Default)]
pub(crate) struct WaitList {
    waiters: Vec<Arc<Parker>>,
}

impl WaitList {
    /// Registra (sem duplicar o mesmo parker).
    pub(crate) fn register(&mut self, p: &Arc<Parker>) {
        if !self.waiters.iter().any(|w| Arc::ptr_eq(w, p)) {
            self.waiters.push(p.clone());
        }
    }

    /// Remove por identidade do parker (o `remove_wait_queue` do `ep_remove_wait_queue`). `register` não
    /// duplica, então há no máximo um: a busca para no primeiro e a ordem dos demais (a ordem de despertar da
    /// fila) se mantém.
    pub(crate) fn unregister(&mut self, p: &Arc<Parker>) {
        if let Some(at) = self.waiters.iter().position(|w| Arc::ptr_eq(w, p)) {
            self.waiters.remove(at);
        }
    }

    /// O parker `p` está registrado (para os testes de vazamento).
    #[cfg(test)]
    pub(crate) fn contains(&self, p: &Arc<Parker>) -> bool {
        self.waiters.iter().any(|w| Arc::ptr_eq(w, p))
    }

    /// Tira todos pra acordar fora da trava (despertar sem chave).
    pub(crate) fn take(&mut self) -> Wake {
        self.take_key(0)
    }

    /// Tira para acordar os que aceitam a chave `key`; quem a filtra (uma entrada de epoll que não pediu esses
    /// eventos) continua na fila, como na fila de espera do kernel.
    pub(crate) fn take_key(&mut self, key: u32) -> Wake {
        let (hit, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.waiters).into_iter().partition(|p| p.accepts(key));
        self.waiters = keep;
        Wake(hit)
    }

    /// Desfecho de uma escrita retomável que já pôs `done` de `len` bytes: completa, parcial (ou
    /// `again` se nada foi) quando não bloqueia, ou espera registrada nesta lista.
    pub(crate) fn write_outcome<E>(
        &mut self,
        done: usize,
        len: usize,
        nonblock: bool,
        again: E,
        waiter: &Arc<Parker>,
    ) -> crate::pipe::Try<Result<usize, E>> {
        use crate::pipe::Try;
        if done == len {
            self.unregister(waiter);
            Try::Ready(Ok(done))
        } else if nonblock {
            Try::Ready(if done > 0 { Ok(done) } else { Err(again) })
        } else {
            self.register(waiter);
            Try::Pending
        }
    }
}

/// Parkers a acordar depois de soltar a trava.
#[must_use]
#[derive(Debug, Default)]
pub(crate) struct Wake(Vec<Arc<Parker>>);

/// Roda `f` com o estado travado e acorda quem ele devolver só depois de soltar a trava.
pub(crate) fn locked<S, T>(m: &Mutex<S>, f: impl FnOnce(&mut S) -> (T, Wake)) -> T {
    let (r, w) = f(&mut m.lock());
    w.run();
    r
}

impl Wake {
    pub(crate) fn none() -> Wake {
        Wake(Vec::new())
    }

    pub(crate) fn merge(&mut self, other: Wake) {
        self.0.extend(other.0);
    }

    pub(crate) fn run(self) {
        for p in self.0 {
            p.unpark();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn unpark_before_park_is_not_lost() {
        let p = Parker::new();
        p.unpark();
        p.park();
        assert!(!p.park_until(Instant::now() + Duration::from_millis(5)));
    }

    #[test]
    fn unregister_removes_only_that_parker_and_keeps_order() {
        let (a, b, c) = (Parker::new(), Parker::new(), Parker::new());
        let mut list = WaitList::default();
        list.register(&a);
        list.register(&b);
        list.register(&c);
        list.unregister(&b);
        list.unregister(&b);
        assert!(list.contains(&a) && !list.contains(&b) && list.contains(&c));
        list.take().run();
        assert!(a.take_notified() && c.take_notified() && !b.take_notified());
    }

    #[test]
    fn wake_from_other_thread() {
        let p = Parker::new();
        let q = p.clone();
        let t = std::thread::spawn(move || q.unpark());
        p.park();
        t.join().unwrap();
    }
}
