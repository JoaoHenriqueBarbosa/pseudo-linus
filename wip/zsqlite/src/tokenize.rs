//! Tradução de tokenize.c: o tokenizador SQL.
//!
//! Aqui entra só o que não depende do `Parse` nem do parser lemon: `sqlite3GetToken`, a tabela
//! de classes de caracteres e o `IdChar`. Ficam adiadas `sqlite3RunParser`, `getToken`,
//! `analyzeWindowKeyword`, `analyzeOverKeyword`, `analyzeFilterKeyword` (usam
//! `sqlite3ParserFallback`) e `sqlite3Normalize` (`SQLITE_ENABLE_NORMALIZE` está desligado).
//!
//! O C lê a entrada como cadeia terminada em NUL e vai além do último byte útil sem medo. Aqui
//! `z` é uma fatia e toda leitura passa por [`at`], que devolve 0 depois do fim, o que reproduz
//! o terminador sem permitir leitura fora dos limites.

use crate::consts::*;
use crate::ctype::{is_digit, is_space, is_xdigit};
use crate::keywordhash::keyword_code_raw;

/// `sqlite3IsIdChar` / macro `IdChar`: verdadeiro se o byte pode aparecer num identificador
/// (qualquer byte com o bit alto ligado, mais letras, dígitos, `_` e `$`).
pub use crate::ctype::is_id_char;

// Classes de caracteres usadas por `get_token`.
const CC_X: u8 = 0; // A letra 'x', ou início de literal BLOB
const CC_KYWD0: u8 = 1; // Primeira letra de uma palavra-chave
const CC_KYWD: u8 = 2; // Alfabéticos ou '_', usáveis numa palavra-chave
const CC_DIGIT: u8 = 3; // Dígitos
const CC_DOLLAR: u8 = 4; // '$'
const CC_VARALPHA: u8 = 5; // '@', '#', ':'. Variáveis SQL alfabéticas
const CC_VARNUM: u8 = 6; // '?'. Variáveis SQL numéricas
const CC_SPACE: u8 = 7; // Caracteres de espaço
const CC_QUOTE: u8 = 8; // '"', '\'' ou '`'. Literais de texto, ids entre aspas
const CC_QUOTE2: u8 = 9; // '['. Ids no estilo [...]
const CC_PIPE: u8 = 10; // '|'. OR bit a bit ou concatenação
const CC_MINUS: u8 = 11; // '-'. Menos ou comentário no estilo SQL
const CC_LT: u8 = 12; // '<'. Parte de < ou <= ou <>
const CC_GT: u8 = 13; // '>'. Parte de > ou >=
const CC_EQ: u8 = 14; // '='. Parte de = ou ==
const CC_BANG: u8 = 15; // '!'. Parte de !=
const CC_SLASH: u8 = 16; // '/'. Divisão ou comentário no estilo C
const CC_LP: u8 = 17; // '('
const CC_RP: u8 = 18; // ')'
const CC_SEMI: u8 = 19; // ';'
const CC_PLUS: u8 = 20; // '+'
const CC_STAR: u8 = 21; // '*'
const CC_PERCENT: u8 = 22; // '%'
const CC_COMMA: u8 = 23; // ','
const CC_AND: u8 = 24; // '&'
const CC_TILDA: u8 = 25; // '~'
const CC_DOT: u8 = 26; // '.'
const CC_ID: u8 = 27; // caracteres unicode usáveis em ids
const CC_NUL: u8 = 29; // 0x00
const CC_BOM: u8 = 30; // Primeiro byte do BOM UTF-8: 0xEF 0xBB 0xBF

/// `aiClass[]` (versão ASCII).
static AI_CLASS: [u8; 256] = [
/*         x0  x1  x2  x3  x4  x5  x6  x7  x8  x9  xa  xb  xc  xd  xe  xf */
/* 0x */   29, 28, 28, 28, 28, 28, 28, 28, 28,  7,  7, 28,  7,  7, 28, 28,
/* 1x */   28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28,
/* 2x */    7, 15,  8,  5,  4, 22, 24,  8, 17, 18, 21, 20, 23, 11, 26, 16,
/* 3x */    3,  3,  3,  3,  3,  3,  3,  3,  3,  3,  5, 19, 12, 14, 13,  6,
/* 4x */    5,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,
/* 5x */    1,  1,  1,  1,  1,  1,  1,  1,  0,  2,  2,  9, 28, 28, 28,  2,
/* 6x */    8,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,  1,
/* 7x */    1,  1,  1,  1,  1,  1,  1,  1,  0,  2,  2, 28, 10, 28, 25, 28,
/* 8x */   27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
/* 9x */   27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
/* Ax */   27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
/* Bx */   27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
/* Cx */   27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
/* Dx */   27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
/* Ex */   27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 30,
/* Fx */   27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
];

/// Macro `charMap` (versão ASCII): mapeia alfabéticos para a minúscula equivalente. Usada pelo
/// hash de palavras-chave.
#[inline]
pub(crate) fn char_map(c: u8) -> u8 {
    crate::ctype::UPPER_TO_LOWER[c as usize]
}

/// Byte `i` de `z`, ou 0 (o NUL do C) depois do fim.
#[inline]
pub(crate) fn at(z: &[u8], i: usize) -> u8 {
    z.get(i).copied().unwrap_or(0)
}

/// Avança por uma sequência de dígitos (do predicado `is_digit_kind`) e separadores de dígitos
/// (`_`), a partir de `start`. Um separador marca o token como `TK_QNUMBER`. Devolve o índice do
/// primeiro byte que não pertence ao número. Corresponde aos quatro laços idênticos do C.
fn skip_digits(z: &[u8], start: usize, tk_type: &mut i32, is_digit_kind: fn(u8) -> bool) -> usize {
    let mut i = start;
    loop {
        let c = at(z, i);
        if !is_digit_kind(c) {
            if c == SQLITE_DIGIT_SEPARATOR {
                *tk_type = TK_QNUMBER as i32;
            } else {
                return i;
            }
        }
        i += 1;
    }
}

/// `sqlite3GetToken`: devolve o comprimento do token que começa em `z[0]` e grava o tipo dele
/// em `tk_type`.
pub fn get_token(z: &[u8], tk_type: &mut i32) -> i32 {
    let mut i: usize;
    let mut c: u8;
    // Despacho pela classe do primeiro byte do token (ver os CC_ acima).
    match AI_CLASS[at(z, 0) as usize] {
        CC_SPACE => {
            i = 1;
            while is_space(at(z, i)) {
                i += 1;
            }
            *tk_type = TK_SPACE as i32;
            return i as i32;
        }
        CC_MINUS => {
            if at(z, 1) == b'-' {
                i = 2;
                loop {
                    c = at(z, i);
                    if c == 0 || c == b'\n' {
                        break;
                    }
                    i += 1;
                }
                *tk_type = TK_SPACE as i32; // IMP: R-22934-25134
                return i as i32;
            } else if at(z, 1) == b'>' {
                *tk_type = TK_PTR as i32;
                return 2 + (at(z, 2) == b'>') as i32;
            }
            *tk_type = TK_MINUS as i32;
            return 1;
        }
        CC_LP => {
            *tk_type = TK_LP as i32;
            return 1;
        }
        CC_RP => {
            *tk_type = TK_RP as i32;
            return 1;
        }
        CC_SEMI => {
            *tk_type = TK_SEMI as i32;
            return 1;
        }
        CC_PLUS => {
            *tk_type = TK_PLUS as i32;
            return 1;
        }
        CC_STAR => {
            *tk_type = TK_STAR as i32;
            return 1;
        }
        CC_SLASH => {
            if at(z, 1) != b'*' || at(z, 2) == 0 {
                *tk_type = TK_SLASH as i32;
                return 1;
            }
            i = 3;
            c = at(z, 2);
            while (c != b'*' || at(z, i) != b'/') && {
                c = at(z, i);
                c != 0
            } {
                i += 1;
            }
            if c != 0 {
                i += 1;
            }
            *tk_type = TK_SPACE as i32; // IMP: R-22934-25134
            return i as i32;
        }
        CC_PERCENT => {
            *tk_type = TK_REM as i32;
            return 1;
        }
        CC_EQ => {
            *tk_type = TK_EQ as i32;
            return 1 + (at(z, 1) == b'=') as i32;
        }
        CC_LT => {
            c = at(z, 1);
            if c == b'=' {
                *tk_type = TK_LE as i32;
                return 2;
            } else if c == b'>' {
                *tk_type = TK_NE as i32;
                return 2;
            } else if c == b'<' {
                *tk_type = TK_LSHIFT as i32;
                return 2;
            } else {
                *tk_type = TK_LT as i32;
                return 1;
            }
        }
        CC_GT => {
            c = at(z, 1);
            if c == b'=' {
                *tk_type = TK_GE as i32;
                return 2;
            } else if c == b'>' {
                *tk_type = TK_RSHIFT as i32;
                return 2;
            } else {
                *tk_type = TK_GT as i32;
                return 1;
            }
        }
        CC_BANG => {
            if at(z, 1) != b'=' {
                *tk_type = TK_ILLEGAL as i32;
                return 1;
            } else {
                *tk_type = TK_NE as i32;
                return 2;
            }
        }
        CC_PIPE => {
            if at(z, 1) != b'|' {
                *tk_type = TK_BITOR as i32;
                return 1;
            } else {
                *tk_type = TK_CONCAT as i32;
                return 2;
            }
        }
        CC_COMMA => {
            *tk_type = TK_COMMA as i32;
            return 1;
        }
        CC_AND => {
            *tk_type = TK_BITAND as i32;
            return 1;
        }
        CC_TILDA => {
            *tk_type = TK_BITNOT as i32;
            return 1;
        }
        CC_QUOTE => {
            let delim = at(z, 0);
            i = 1;
            loop {
                c = at(z, i);
                if c == 0 {
                    break;
                }
                if c == delim {
                    if at(z, i + 1) == delim {
                        i += 1;
                    } else {
                        break;
                    }
                }
                i += 1;
            }
            if c == b'\'' {
                *tk_type = TK_STRING as i32;
                return i as i32 + 1;
            } else if c != 0 {
                *tk_type = TK_ID as i32;
                return i as i32 + 1;
            } else {
                *tk_type = TK_ILLEGAL as i32;
                return i as i32;
            }
        }
        // CC_DOT sem dígito depois é um ponto; com dígito, cai no caso CC_DIGIT (número
        // de ponto flutuante que começa com ".").
        class @ (CC_DOT | CC_DIGIT) => {
            if class == CC_DOT && !is_digit(at(z, 1)) {
                *tk_type = TK_DOT as i32;
                return 1;
            }
            *tk_type = TK_INTEGER as i32;
            if at(z, 0) == b'0'
                && (at(z, 1) == b'x' || at(z, 1) == b'X')
                && is_xdigit(at(z, 2))
            {
                i = skip_digits(z, 3, tk_type, is_xdigit);
            } else {
                i = skip_digits(z, 0, tk_type, is_digit);
                if at(z, i) == b'.' {
                    if *tk_type == TK_INTEGER as i32 {
                        *tk_type = TK_FLOAT as i32;
                    }
                    i = skip_digits(z, i + 1, tk_type, is_digit);
                }
                if (at(z, i) == b'e' || at(z, i) == b'E')
                    && (is_digit(at(z, i + 1))
                        || ((at(z, i + 1) == b'+' || at(z, i + 1) == b'-')
                            && is_digit(at(z, i + 2))))
                {
                    if *tk_type == TK_INTEGER as i32 {
                        *tk_type = TK_FLOAT as i32;
                    }
                    i = skip_digits(z, i + 2, tk_type, is_digit);
                }
            }
            while is_id_char(at(z, i)) {
                *tk_type = TK_ILLEGAL as i32;
                i += 1;
            }
            return i as i32;
        }
        CC_QUOTE2 => {
            i = 1;
            c = at(z, 0);
            while c != b']' && {
                c = at(z, i);
                c != 0
            } {
                i += 1;
            }
            *tk_type = if c == b']' { TK_ID as i32 } else { TK_ILLEGAL as i32 };
            return i as i32;
        }
        CC_VARNUM => {
            *tk_type = TK_VARIABLE as i32;
            i = 1;
            while is_digit(at(z, i)) {
                i += 1;
            }
            return i as i32;
        }
        CC_DOLLAR | CC_VARALPHA => {
            let mut n = 0;
            *tk_type = TK_VARIABLE as i32;
            i = 1;
            loop {
                c = at(z, i);
                if c == 0 {
                    break;
                }
                if is_id_char(c) {
                    n += 1;
                } else if c == b'(' && n > 0 {
                    loop {
                        i += 1;
                        c = at(z, i);
                        if !(c != 0 && !is_space(c) && c != b')') {
                            break;
                        }
                    }
                    if c == b')' {
                        i += 1;
                    } else {
                        *tk_type = TK_ILLEGAL as i32;
                    }
                    break;
                } else if c == b':' && at(z, i + 1) == b':' {
                    i += 1;
                } else {
                    break;
                }
                i += 1;
            }
            if n == 0 {
                *tk_type = TK_ILLEGAL as i32;
            }
            return i as i32;
        }
        CC_KYWD0 => {
            if AI_CLASS[at(z, 1) as usize] > CC_KYWD {
                i = 1;
            } else {
                i = 2;
                while AI_CLASS[at(z, i) as usize] <= CC_KYWD {
                    i += 1;
                }
                if is_id_char(at(z, i)) {
                    // O token começou com caracteres que cabem numa palavra-chave, mas z[i] é
                    // um caractere que ela não admite: é um identificador.
                    i += 1;
                } else {
                    *tk_type = TK_ID as i32;
                    return keyword_code_raw(&z[..i], tk_type);
                }
            }
        }
        CC_X => {
            if at(z, 1) == b'\'' {
                *tk_type = TK_BLOB as i32;
                i = 2;
                while is_xdigit(at(z, i)) {
                    i += 1;
                }
                if at(z, i) != b'\'' || i % 2 != 0 {
                    *tk_type = TK_ILLEGAL as i32;
                    while at(z, i) != 0 && at(z, i) != b'\'' {
                        i += 1;
                    }
                }
                if at(z, i) != 0 {
                    i += 1;
                }
                return i as i32;
            }
            // Se não é literal BLOB, é um id, pois nenhuma palavra-chave SQL começa com 'x'.
            i = 1;
        }
        CC_KYWD | CC_ID => {
            i = 1;
        }
        CC_BOM => {
            if at(z, 1) == 0xbb && at(z, 2) == 0xbf {
                *tk_type = TK_SPACE as i32;
                return 3;
            }
            i = 1;
        }
        CC_NUL => {
            *tk_type = TK_ILLEGAL as i32;
            return 0;
        }
        _ => {
            *tk_type = TK_ILLEGAL as i32;
            return 1;
        }
    }
    while is_id_char(at(z, i)) {
        i += 1;
    }
    *tk_type = TK_ID as i32;
    i as i32
}
