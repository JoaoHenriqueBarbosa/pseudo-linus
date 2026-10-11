//! Classificação de caracteres do SQLite (`global.c` e as macros `sqlite3Isxxx` do `sqliteInt.h`).
//!
//! O SQLite só considera US-ASCII (build ASCII, o do Debian). Os bytes `>= 0x80` contam como
//! caractere de identificador (bit `0x40`), como no C.

/// Tabela `sqlite3UpperToLower[]` (parte de 256 bytes): mapeia maiúscula ASCII para minúscula.
///
/// No C ela é seguida de 18 bytes não relacionados (as tabelas `aLTb`, `aEQb` e `aGTb`), que
/// aqui ficam em [`A_LT_B`], [`A_EQ_B`] e [`A_GT_B`].
pub const UPPER_TO_LOWER: [u8; 256] = build_upper_to_lower();

const fn build_upper_to_lower() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        // 'A'..='Z' (65..=90) mapeiam para 97..=122; o resto é a identidade.
        t[i] = if i >= 65 && i <= 90 { (i + 32) as u8 } else { i as u8 };
        i += 1;
    }
    t
}

/// `sqlite3aLTb[]`: usada quando `compare(A,B)` é menor que zero. A ordem das colunas é
/// `NE EQ GT LE LT GE` (os opcodes de comparação são consecutivos, `OP_Ne` a `OP_Ge`).
pub const A_LT_B: [u8; 6] = [1, 0, 0, 1, 1, 0];
/// `sqlite3aEQb[]`: usada quando `compare(A,B)` é igual a zero.
pub const A_EQ_B: [u8; 6] = [0, 1, 0, 1, 0, 1];
/// `sqlite3aGTb[]`: usada quando `compare(A,B)` é maior que zero.
pub const A_GT_B: [u8; 6] = [1, 0, 1, 0, 0, 1];

/// Equivalente a `sqlite3aLTb[opcode]` do C (o ponteiro do C já vem deslocado por `OP_Ne`).
#[inline]
pub fn a_lt_b(opcode: u8) -> u8 {
    A_LT_B[(opcode - crate::consts::OP_NE) as usize]
}

/// Equivalente a `sqlite3aEQb[opcode]` do C.
#[inline]
pub fn a_eq_b(opcode: u8) -> u8 {
    A_EQ_B[(opcode - crate::consts::OP_NE) as usize]
}

/// Equivalente a `sqlite3aGTb[opcode]` do C.
#[inline]
pub fn a_gt_b(opcode: u8) -> u8 {
    A_GT_B[(opcode - crate::consts::OP_NE) as usize]
}

/// Tabela `sqlite3CtypeMap[256]`, usada pelas rotinas embutidas equivalentes a `isspace()`,
/// `isalpha()`, `isdigit()`, `isalnum()`, `isxdigit()` e `toupper()`:
///
/// - `0x01` espaço
/// - `0x02` letra
/// - `0x04` dígito
/// - `0x06` alfanumérico
/// - `0x08` dígito hexadecimal
/// - `0x20` letra minúscula (precisa de tradução para maiúscula)
/// - `0x40` caractere de identificador (`$`, `_` ou não ASCII)
/// - `0x80` caractere de aspas
pub const CTYPE_MAP: [u8; 256] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, /* 00..07 */
    0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, /* 08..0f */
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, /* 10..17 */
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, /* 18..1f */
    0x01, 0x00, 0x80, 0x00, 0x40, 0x00, 0x00, 0x80, /* 20..27 */
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, /* 28..2f */
    0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, /* 30..37 */
    0x0c, 0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, /* 38..3f */
    0x00, 0x0a, 0x0a, 0x0a, 0x0a, 0x0a, 0x0a, 0x02, /* 40..47 */
    0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, /* 48..4f */
    0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, /* 50..57 */
    0x02, 0x02, 0x02, 0x80, 0x00, 0x00, 0x00, 0x40, /* 58..5f */
    0x80, 0x2a, 0x2a, 0x2a, 0x2a, 0x2a, 0x2a, 0x22, /* 60..67 */
    0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, /* 68..6f */
    0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, /* 70..77 */
    0x22, 0x22, 0x22, 0x00, 0x00, 0x00, 0x00, 0x00, /* 78..7f */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* 80..87 */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* 88..8f */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* 90..97 */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* 98..9f */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* a0..a7 */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* a8..af */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* b0..b7 */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* b8..bf */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* c0..c7 */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* c8..cf */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* d0..d7 */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* d8..df */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* e0..e7 */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* e8..ef */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* f0..f7 */
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, /* f8..ff */
];

/// `sqlite3Toupper(x)`: `x & ~(map[x] & 0x20)`.
#[inline]
pub fn to_upper(x: u8) -> u8 {
    x & !(CTYPE_MAP[x as usize] & 0x20)
}

/// `sqlite3Tolower(x)`: consulta `sqlite3UpperToLower[]`.
#[inline]
pub fn to_lower(x: u8) -> u8 {
    UPPER_TO_LOWER[x as usize]
}

/// `sqlite3Isspace(x)`.
#[inline]
pub fn is_space(x: u8) -> bool {
    CTYPE_MAP[x as usize] & 0x01 != 0
}

/// `sqlite3Isalnum(x)`.
#[inline]
pub fn is_alnum(x: u8) -> bool {
    CTYPE_MAP[x as usize] & 0x06 != 0
}

/// `sqlite3Isalpha(x)`.
#[inline]
pub fn is_alpha(x: u8) -> bool {
    CTYPE_MAP[x as usize] & 0x02 != 0
}

/// `sqlite3Isdigit(x)`.
#[inline]
pub fn is_digit(x: u8) -> bool {
    CTYPE_MAP[x as usize] & 0x04 != 0
}

/// `sqlite3Isxdigit(x)`.
#[inline]
pub fn is_xdigit(x: u8) -> bool {
    CTYPE_MAP[x as usize] & 0x08 != 0
}

/// `sqlite3Isquote(x)`: aspas `"`, `'`, `` ` `` ou o colchete `[`.
#[inline]
pub fn is_quote(x: u8) -> bool {
    CTYPE_MAP[x as usize] & 0x80 != 0
}

/// Macro `IdChar(C)` do `tokenize.c`: alfanumérico, `_`, `$` ou byte não ASCII (máscara `0x46`).
#[inline]
pub fn is_id_char(x: u8) -> bool {
    CTYPE_MAP[x as usize] & 0x46 != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upper_to_lower_matches_ascii() {
        assert_eq!(UPPER_TO_LOWER[b'A' as usize], b'a');
        assert_eq!(UPPER_TO_LOWER[b'Z' as usize], b'z');
        assert_eq!(UPPER_TO_LOWER[b'[' as usize], b'[');
        assert_eq!(UPPER_TO_LOWER[0xC0], 0xC0);
        assert_eq!(UPPER_TO_LOWER[255], 255);
    }

    #[test]
    fn classes() {
        assert!(is_space(b' ') && is_space(b'\n') && !is_space(0x0b + 3));
        assert!(is_digit(b'7') && is_xdigit(b'F') && is_xdigit(b'a') && !is_xdigit(b'g'));
        assert!(is_quote(b'[') && is_quote(b'`') && is_quote(b'"') && is_quote(b'\''));
        assert!(is_id_char(b'_') && is_id_char(b'$') && is_id_char(0xe9) && !is_id_char(b'-'));
        assert_eq!(to_upper(b'q'), b'Q');
        assert_eq!(to_upper(b'Q'), b'Q');
        assert_eq!(to_upper(b'1'), b'1');
    }
}
