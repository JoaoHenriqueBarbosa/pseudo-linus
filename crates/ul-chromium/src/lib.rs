//! `chromium --headless` do pseudo-linus.

pub mod cli;
pub mod document;
pub mod page;

/// Os programas deste crate.
pub fn programs() -> Vec<sysabi::Program> {
    vec![sysabi::Program::bin("chromium", cli::main)]
}
