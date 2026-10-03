//! Threads de um pseudo-processo herdam o contexto dele. Com `std::thread::spawn` direto, a thread
//! nova nasceria sem processo corrente e o primeiro I/O pelo shim daria panic.

// O resto de `std::thread` (`yield_now`, `sleep`, `JoinHandle`...) passa direto; `spawn` daqui
// sombreia o do std.
pub use std::thread::*;

pub fn spawn<F, T>(f: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let p = crate::proc::current();
    std::thread::spawn(move || crate::proc::enter(p, f))
}
