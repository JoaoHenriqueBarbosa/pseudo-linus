//! Criar corrotina do `may` dentro de bloco `unsafe`. Esperado: erro do lint `unsafe_code`.
#![forbid(unsafe_code)]

fn main() {
    let h = unsafe { may::coroutine::spawn(|| 1) };
    println!("{}", h.join().unwrap());
}
