//! `println!` escreve no stdout do host sem nomear `std::io::stdout` (a macro chama `std::io::_print`).
//! Só o `disallowed-macros` pega.

pub fn say(msg: &str) {
    println!("{msg}");
}
