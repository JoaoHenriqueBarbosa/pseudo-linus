//! `std::process::Command` e `std::process::exit` chamados direto.

pub fn run_true() -> bool {
    std::process::Command::new("true").status().is_ok_and(|s| s.success())
}

pub fn quit(code: i32) -> ! {
    std::process::exit(code)
}
