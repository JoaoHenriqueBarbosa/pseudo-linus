//! Embrulho com `unsafe impl Send` pra forçar a migração. Esperado: erro do lint `unsafe_code`.
#![forbid(unsafe_code)]

use corosensei::Coroutine;

struct Movable(Coroutine<(), (), ()>);

unsafe impl Send for Movable {}

fn main() {
    let mut m = Movable(Coroutine::new(|y, ()| y.suspend(())));
    m.0.resume(());
    std::thread::spawn(move || {
        // Rebind pra capturar o embrulho inteiro (e não só o campo, que o closure capturaria sozinho).
        let mut m = m;
        m.0.resume(());
    })
    .join()
    .unwrap();
}
