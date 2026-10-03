//! `std::fs::read` chamado direto.

pub fn read_len(path: &str) -> usize {
    std::fs::read(path).map(|bytes| bytes.len()).unwrap_or(0)
}
