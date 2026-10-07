//! Pedaços da libc que o porte do libmagic usa com a semântica exata do C: `ctype` por byte no
//! locale C.UTF-8 (byte acima de 0x7f nunca é classe nenhuma), `strtol`/`strtoull` com base 0
//! (prefixo `0x` e `0`), `strtod` e cadeias terminadas em NUL. O que não é específico do libmagic
//! mora em `ul_common::ctype` e entra aqui por reexportação.

pub use ul_common::ctype::{Conv, at, cstr, is_print, is_space, strtod, strtof, strtol, strtoull};

pub fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

pub fn is_alpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

pub fn is_alnum(c: u8) -> bool {
    c.is_ascii_alphanumeric()
}

pub fn is_upper(c: u8) -> bool {
    c.is_ascii_uppercase()
}

pub fn is_lower(c: u8) -> bool {
    c.is_ascii_lowercase()
}
