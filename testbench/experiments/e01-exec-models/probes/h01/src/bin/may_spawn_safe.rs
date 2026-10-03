//! Criar corrotina do `may` sem `unsafe`. Esperado: E0133 (`may::coroutine::spawn` é `unsafe fn`).
#![forbid(unsafe_code)]

fn main() {
    let h = may::coroutine::spawn(|| 1);
    println!("{}", h.join().unwrap());
}
