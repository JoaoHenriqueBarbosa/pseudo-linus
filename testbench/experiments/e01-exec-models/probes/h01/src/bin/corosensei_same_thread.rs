//! Controle: corrotina corosensei usada só na thread que a criou. Tem que compilar.
#![forbid(unsafe_code)]

use corosensei::{Coroutine, CoroutineResult};

fn main() {
    let mut co: Coroutine<(), i32, i32> = Coroutine::new(|y, ()| {
        y.suspend(1);
        2
    });
    assert!(matches!(co.resume(()), CoroutineResult::Yield(1)));
    assert!(matches!(co.resume(()), CoroutineResult::Return(2)));
    println!("ok");
}
