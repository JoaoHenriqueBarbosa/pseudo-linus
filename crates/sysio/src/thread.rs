//! Threads de um pseudo-processo, criadas pelo kernel (`sysabi::Syscalls::spawn_thread`): a thread
//! nova roda no escalonador do pseudo-linus com o processo instalado, e morre junto com ele. Com
//! `std::thread::spawn` direto ela nasceria fora do processo e a primeira syscall daria panic.
//!
//! A thread herda também o estado de userland do processo (buffer do stdout, código de saída,
//! `proc_local`). `exit` em qualquer thread termina o processo inteiro (o `exit_group` do Linux);
//! um panic comum fica guardado e volta no `join`, como no std.

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::{Arc, Mutex};

// O resto de `std::thread` (`Result`, `current`, `panicking`...) passa direto; os itens abaixo
// sombreiam os do std.
pub use std::thread::*;

use crate::proc::{self, lock};

type Slot<T> = Arc<Mutex<Option<std::thread::Result<T>>>>;

/// `std::thread::JoinHandle` de uma thread do pseudo-processo.
pub struct JoinHandle<T> {
    tid: sysabi::Tid,
    slot: Slot<T>,
}

impl<T> std::fmt::Debug for JoinHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoinHandle").field("tid", &self.tid).finish_non_exhaustive()
    }
}

impl<T> JoinHandle<T> {
    /// Espera a thread e devolve o resultado (ou o panic dela).
    pub fn join(self) -> std::thread::Result<T> {
        let sys = proc::sys();
        while let Err(sysabi::Errno::EINTR) = sys.join_thread(self.tid) {}
        lock(&self.slot).take().unwrap_or_else(|| Err(Box::new("a thread terminou sem resultado")))
    }

    pub fn is_finished(&self) -> bool {
        lock(&self.slot).is_some()
    }

    /// Id da thread no kernel (`gettid` dentro dela).
    pub fn tid(&self) -> sysabi::Tid {
        self.tid
    }
}

/// `std::thread::spawn` dentro do pseudo-processo corrente.
pub fn spawn<F, T>(f: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let frame = proc::frame();
    let slot: Slot<T> = Arc::new(Mutex::new(None));
    let out = Arc::clone(&slot);
    let body: sysabi::ThreadFn = Box::new(move || {
        let _adopted = proc::adopt(frame);
        match catch_unwind(AssertUnwindSafe(f)) {
            Ok(v) => *lock(&out) = Some(Ok(v)),
            Err(payload) => {
                // `exit`, morte por sinal e `execve` são do processo, não da thread: seguem pro kernel.
                if payload.is::<sysabi::ExitUnwind>() || payload.is::<sysabi::KillUnwind>() || payload.is::<sysabi::ExecUnwind>() {
                    resume_unwind(payload);
                }
                *lock(&out) = Some(Err(payload));
            }
        }
    });
    match proc::sys().spawn_thread(body) {
        Ok(tid) => JoinHandle { tid, slot },
        // O std também entra em pânico quando não consegue criar a thread.
        Err(e) => panic!("failed to spawn thread: {}", e.message()),
    }
}

/// `std::thread::sleep` com o relógio do pseudo-kernel.
pub use crate::time::sleep;

/// `sched_yield(2)` do pseudo-kernel.
pub fn yield_now() {
    proc::sys().sched_yield();
}

/// CPUs que o pseudo-processo pode usar (`sched_getaffinity`, as CPUs virtuais do sandbox).
pub fn available_parallelism() -> std::io::Result<std::num::NonZeroUsize> {
    let n = proc::sys().sched_getaffinity().len();
    Ok(std::num::NonZeroUsize::new(n).unwrap_or(std::num::NonZeroUsize::MIN))
}
