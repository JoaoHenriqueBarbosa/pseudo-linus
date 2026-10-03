//! `std::fs::File::open`: método associado de um tipo proibido (pego por `disallowed-types`).

use std::io::Read;

pub fn read_len(path: &str) -> usize {
    let mut buf = Vec::new();
    match std::fs::File::open(path) {
        Ok(mut file) => file.read_to_end(&mut buf).unwrap_or(0),
        Err(_) => 0,
    }
}
