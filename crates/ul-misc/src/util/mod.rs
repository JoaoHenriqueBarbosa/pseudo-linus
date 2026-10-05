//! Peças comuns dos programas do crate: `getopt_long` da glibc, E/S sobre `sysabi`, hora local e
//! largura de exibição.

pub mod getopt;
pub mod io;
pub mod time;
pub mod ul;

pub use getopt::{Getopt, GetoptError, HasArg, LongOpt, Opt};

/// Largura de exibição de um texto UTF-8 em C.UTF-8 (o `wcswidth` da glibc): caractere de controle
/// conta 0 aqui, quem precisa de outra regra trata antes.
pub fn display_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthChar;
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// Largura de exibição de bytes: sequência UTF-8 inválida conta um por byte, como o `mbsnwidth` do
/// gnulib com `MBSW_ACCEPT_INVALID`.
pub fn display_width_bytes(b: &[u8]) -> usize {
    let mut total = 0;
    for chunk in b.utf8_chunks() {
        total += display_width(chunk.valid());
        total += chunk.invalid().len();
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        assert_eq!(display_width("ção"), 3);
        assert_eq!(display_width("日本"), 4);
        assert_eq!(display_width_bytes(b"a\xffb"), 3);
    }
}
