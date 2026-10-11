//! `chromium --headless` do pseudo-linus.

pub mod blink_css;
pub mod bridge;
pub mod canvas;
pub mod canvas_gradient;
pub mod canvas_text;
pub mod cli;
pub mod devtools;
pub mod disk_profile;
pub mod document;
pub mod encoding;
pub mod fetch;
pub mod fonts;
pub mod history;
pub mod http_cache;
pub mod idb;
pub mod imaging;
pub mod local_storage;
pub mod preferences;
pub mod page;
pub mod session;
pub mod sessions;
pub mod session_storage;
pub mod storage_state;
pub mod sys_env;
pub mod webp;

/// Os programas deste crate.
pub fn programs() -> Vec<sysabi::Program> {
    vec![sysabi::Program::bin("chromium", cli::main)]
}
