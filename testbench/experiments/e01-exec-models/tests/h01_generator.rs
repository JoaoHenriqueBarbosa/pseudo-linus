//! H01: o `generator` 0.8 compila a migração de uma corrotina entre threads sem unsafe nosso, porque tem
//! `unsafe impl Send` próprio, e essa migração é unsound: a corrotina continua usando o thread-local da
//! thread em que começou. Este teste falha se o comportamento mudar (por exemplo, se uma versão nova
//! tirar o `Send`, ele nem compila, o que também é informação).
#![forbid(unsafe_code)]

use std::cell::Cell;

use generator::Gn;

thread_local! {
    static SLOT: Cell<u64> = const { Cell::new(0) };
}

#[inline(never)]
fn slot_addr() -> usize {
    SLOT.with(|c| c as *const Cell<u64> as usize)
}

#[test]
fn generator_migration_keeps_thread1_tls() {
    let tls1 = slot_addr();
    let mut g = Gn::<()>::new_scoped(|mut s| {
        SLOT.with(|c| {
            s.yield_(c as *const Cell<u64> as usize);
            s.yield_(c as *const Cell<u64> as usize);
        });
        0
    });
    let held1 = g.resume().expect("primeiro yield");
    let (tls2, held2) = std::thread::spawn(move || {
        let tls2 = slot_addr();
        let held2 = g.resume().expect("segundo yield");
        let _ = g.resume();
        (tls2, held2)
    })
    .join()
    .expect("thread 2");
    assert_eq!(held1, tls1);
    assert_ne!(tls1, tls2, "threads diferentes têm thread-locals diferentes");
    assert_eq!(held2, tls1, "rodando na thread 2, a corrotina ainda usa o thread-local da thread 1");
}
