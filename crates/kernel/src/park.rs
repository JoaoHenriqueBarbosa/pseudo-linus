//! Espera e despertar.
//!
//! Cada pseudo-processo (e cada thread do host que espera um pipe ou um processo) tem um [`Parker`]:
//! uma ficha com flag, como o `park`/`unpark` da std, mas próprio (nenhum código de terceiros consome a
//! ficha). Quem espera um objeto se registra na [`WaitList`] dele, confere a condição sob a trava do
//! objeto e dorme; quem muda o objeto acorda todos os registrados. A flag impede despertar perdido entre
//! soltar a trava e dormir.

use std::sync::Arc;
use std::time::Instant;

use parking_lot::{Condvar, Mutex};

#[derive(Debug, Default)]
pub(crate) struct Parker {
    notified: Mutex<bool>,
    cv: Condvar,
}

impl Parker {
    pub(crate) fn new() -> Arc<Parker> {
        Arc::new(Parker::default())
    }

    pub(crate) fn unpark(&self) {
        let mut n = self.notified.lock();
        if !*n {
            *n = true;
            self.cv.notify_one();
        }
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

    pub(crate) fn unregister(&mut self, p: &Arc<Parker>) {
        self.waiters.retain(|w| !Arc::ptr_eq(w, p));
    }

    /// Tira todos pra acordar fora da trava.
    #[must_use]
    pub(crate) fn take(&mut self) -> Wake {
        Wake(std::mem::take(&mut self.waiters))
    }
}

/// Parkers a acordar depois de soltar a trava.
#[must_use]
#[derive(Debug, Default)]
pub(crate) struct Wake(Vec<Arc<Parker>>);

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
    fn wake_from_other_thread() {
        let p = Parker::new();
        let q = p.clone();
        let t = std::thread::spawn(move || q.unpark());
        p.park();
        t.join().unwrap();
    }
}
