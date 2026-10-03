//! Mover uma corrotina corosensei suspensa pra outra thread. Esperado: E0277 (`Coroutine` não é `Send`).
#![forbid(unsafe_code)]

use corosensei::Coroutine;

fn main() {
    let mut co: Coroutine<(), (), ()> = Coroutine::new(|y, ()| y.suspend(()));
    co.resume(());
    std::thread::spawn(move || {
        co.resume(());
    })
    .join()
    .unwrap();
}
