//! `std::io::stdout` chamado direto.

use std::io::Write;

pub fn say(msg: &str) -> bool {
    std::io::stdout().write_all(msg.as_bytes()).is_ok()
}
