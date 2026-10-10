//! `chromium --headless` do pseudo-linus.

pub mod blink_css;
pub mod bridge;
pub mod canvas_text;
pub mod cli;
pub mod document;
pub mod encoding;
pub mod fetch;
pub mod fonts;
pub mod imaging;
pub mod page;

/// Os programas deste crate.
pub fn programs() -> Vec<sysabi::Program> {
    vec![sysabi::Program::bin("chromium", cli::main)]
}
