//! Compartilhar uma corrotina corosensei entre threads atrás de `Arc<Mutex<_>>`. Esperado: E0277.
#![forbid(unsafe_code)]

use std::sync::{Arc, Mutex};

use corosensei::Coroutine;

fn main() {
    let co: Coroutine<(), (), ()> = Coroutine::new(|y, ()| y.suspend(()));
    let shared = Arc::new(Mutex::new(co));
    let s2 = shared.clone();
    std::thread::spawn(move || {
        s2.lock().unwrap().resume(());
    })
    .join()
    .unwrap();
}
