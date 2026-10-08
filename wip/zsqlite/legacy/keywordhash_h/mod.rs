// Mesclado das partes traduzidas de keywordhash_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Palavra-chave texto comprimida em um único vetor de bytes.
// z_KW_TEXT[] codifica 1007 bytes de texto de palavra-chave em 667 bytes.
pub const Z_KW_TEXT: &[u8] = &[
  b'R',b'E',b'I',b'N',b'D',b'E',b'X',b'E',b'D',b'E',b'S',b'C',b'A',b'P',b'E',b'A',b'C',b'H',
  b'E',b'C',b'K',b'E',b'Y',b'B',b'E',b'F',b'O',b'R',b'E',b'I',b'G',b'N',b'O',b'R',b'E',b'G',
  b'E',b'X',b'P',b'L',b'A',b'I',b'N',b'S',b'T',b'E',b'A',b'D',b'D',b'A',b'T',b'A',b'B',b'A',
  b'S',b'E',b'L',b'E',b'C',b'T',b'A',b'B',b'L',b'E',b'F',b'T',b'H',b'E',b'N',b'D',b'E',b'F',
  b'E',b'R',b'R',b'A',b'B',b'L',b'E',b'L',b'S',b'E',b'X',b'C',b'L',b'U',b'D',b'E',b'L',b'E',
  b'T',b'E',b'M',b'P',b'O',b'R',b'A',b'R',b'Y',b'I',b'S',b'N',b'U',b'L',b'L',b'S',b'A',b'V',
  b'E',b'P',b'O',b'I',b'N',b'T',b'E',b'R',b'S',b'E',b'C',b'T',b'I',b'E',b'S',b'N',b'O',b'T',
  b'N',b'U',b'L',b'L',b'I',b'K',b'E',b'X',b'C',b'E',b'P',b'T',b'R',b'A',b'N',b'S',b'A',b'C',
  b'T',b'I',b'O',b'N',b'A',b'T',b'U',b'R',b'A',b'L',b'T',b'E',b'R',b'A',b'I',b'S',b'E',b'X',
  b'C',b'L',b'U',b'S',b'I',b'V',b'E',b'X',b'I',b'S',b'T',b'S',b'C',b'O',b'N',b'S',b'T',b'R',
  b'A',b'I',b'N',b'T',b'O',b'F',b'F',b'S',b'E',b'T',b'R',b'I',b'G',b'G',b'E',b'R',b'A',b'N',
  b'G',b'E',b'N',b'E',b'R',b'A',b'T',b'E',b'D',b'E',b'T',b'A',b'C',b'H',b'A',b'V',b'I',b'N',
  b'G',b'L',b'O',b'B',b'E',b'G',b'I',b'N',b'N',b'E',b'R',b'E',b'F',b'E',b'R',b'E',b'N',b'C',
  b'E',b'S',b'U',b'N',b'I',b'Q',b'U',b'E',b'R',b'Y',b'W',b'I',b'T',b'H',b'O',b'U',b'T',b'E',
  b'R',b'E',b'L',b'E',b'A',b'S',b'E',b'A',b'T',b'T',b'A',b'C',b'H',b'B',b'E',b'T',b'W',b'E',
  b'E',b'N',b'O',b'T',b'H',b'I',b'N',b'G',b'R',b'O',b'U',b'P',b'S',b'C',b'A',b'S',b'C',b'A',
  b'D',b'E',b'F',b'A',b'U',b'L',b'T',b'C',b'A',b'S',b'E',b'C',b'O',b'L',b'L',b'A',b'T',b'E',
  b'C',b'R',b'E',b'A',b'T',b'E',b'C',b'U',b'R',b'R',b'E',b'N',b'T',b'_',b'D',b'A',b'T',b'E',
  b'I',b'M',b'M',b'E',b'D',b'I',b'A',b'T',b'E',b'J',b'O',b'I',b'N',b'S',b'E',b'R',b'T',b'M',
  b'A',b'T',b'C',b'H',b'P',b'L',b'A',b'N',b'A',b'L',b'Y',b'Z',b'E',b'P',b'R',b'A',b'G',b'M',
  b'A',b'T',b'E',b'R',b'I',b'A',b'L',b'I',b'Z',b'E',b'D',b'E',b'F',b'E',b'R',b'R',b'E',b'D',
  b'I',b'S',b'T',b'I',b'N',b'C',b'T',b'U',b'P',b'D',b'A',b'T',b'E',b'V',b'A',b'L',b'U',b'E',
  b'S',b'V',b'I',b'R',b'T',b'U',b'A',b'L',b'W',b'A',b'Y',b'S',b'W',b'H',b'E',b'N',b'W',b'H',
  b'E',b'R',b'E',b'C',b'U',b'R',b'S',b'I',b'V',b'E',b'A',b'B',b'O',b'R',b'T',b'A',b'F',b'T',
  b'E',b'R',b'E',b'N',b'A',b'M',b'E',b'A',b'N',b'D',b'R',b'O',b'P',b'A',b'R',b'T',b'I',b'T',
  b'I',b'O',b'N',b'A',b'U',b'T',b'O',b'I',b'N',b'C',b'R',b'E',b'M',b'E',b'N',b'T',b'C',b'A',
  b'S',b'T',b'C',b'O',b'L',b'U',b'M',b'N',b'C',b'O',b'M',b'M',b'I',b'T',b'C',b'O',b'N',b'F',
  b'L',b'I',b'C',b'T',b'C',b'R',b'O',b'S',b'S',b'C',b'U',b'R',b'R',b'E',b'N',b'T',b'_',b'T',
  b'I',b'M',b'E',b'S',b'T',b'A',b'M',b'P',b'R',b'E',b'C',b'E',b'D',b'I',b'N',b'G',b'F',b'A',
  b'I',b'L',b'A',b'S',b'T',b'F',b'I',b'L',b'T',b'E',b'R',b'E',b'P',b'L',b'A',b'C',b'E',b'F',
  b'I',b'R',b'S',b'T',b'F',b'O',b'L',b'L',b'O',b'W',b'I',b'N',b'G',b'F',b'R',b'O',b'M',b'F',
  b'U',b'L',b'L',b'I',b'M',b'I',b'T',b'I',b'F',b'O',b'R',b'D',b'E',b'R',b'E',b'S',b'T',b'R',
  b'I',b'C',b'T',b'O',b'T',b'H',b'E',b'R',b'S',b'O',b'V',b'E',b'R',b'E',b'T',b'U',b'R',b'N',
  b'I',b'N',b'G',b'R',b'I',b'G',b'H',b'T',b'R',b'O',b'L',b'L',b'B',b'A',b'C',b'K',b'R',b'O',
  b'W',b'S',b'U',b'N',b'B',b'O',b'U',b'N',b'D',b'E',b'D',b'U',b'N',b'I',b'O',b'N',b'U',b'S',
  b'I',b'N',b'G',b'V',b'A',b'C',b'U',b'U',b'M',b'V',b'I',b'E',b'W',b'I',b'N',b'D',b'O',b'W',
  b'B',b'Y',b'I',b'N',b'I',b'T',b'I',b'A',b'L',b'L',b'Y',b'P',b'R',b'I',b'M',b'A',b'R',b'Y',
];

// a_KW_HASH[i] é o valor hash para a i-ésima palavra-chave.
pub const A_KW_HASH: &[u8] = &[
    84,  92, 134,  82, 105,  29,   0,   0,  94,   0,  85,  72,   0,
    53,  35,  86,  15,   0,  42,  97,  54,  89, 135,  19,   0,   0,
   140,   0,  40, 129,   0,  22, 107,   0,   9,   0,   0, 123,  80,
     0,  78,   6,   0,  65, 103, 147,   0, 136, 115,   0,   0,  48,
     0,  90,  24,   0,  17,   0,  27,  70,  23,  26,   5,  60, 142,
   110, 122,   0,  73,  91,  71, 145,  61, 120,  74,   0,  49,   0,
    11,  41,   0, 113,   0,   0,   0, 109,  10, 111, 116, 125,  14,
    50, 124,   0, 100,   0,  18, 121, 144,  56, 130, 139,  88,  83,
    37,  30, 126,   0,   0, 108,  51, 131, 128,   0,  34,   0,   0,
   132,   0,  98,  38,  39,   0,  20,  45, 117,  93,
];

// A_KW_NEXT[] forma a cadeia de colisão do hash. Se A_KW_HASH[i]==0,
// a i-ésima palavra-chave não tem mais colisões. Caso contrário, a próxima
// palavra-chave com o mesmo hash é A_KW_HASH[i]-1 (como no comentário do C).
pub const A_KW_NEXT: &[u8] = &[0,
     0,   0,   0,   0,   4,   0,  43,   0,   0, 106, 114,   0,   0,
     0,   2,   0,   0, 143,   0,   0,   0,  13,   0,   0,   0,   0,
   141,   0,   0, 119,  52,   0,   0, 137,  12,   0,   0,  62,   0,
   138,   0, 133,   0,   0,  36,   0,   0,  28,  77,   0,   0,   0,
     0,  59,   0,  47,   0,   0,   0,   0,   0,   0,   0,   0,   0,
     0,  69,   0,   0,   0,   0,   0, 146,   3,   0,  58,   0,   1,
    75,   0,   0,   0,  31,   0,   0,   0,   0,   0, 127,   0, 104,
     0,  64,  66,  63,   0,   0,   0,   0,   0,  46,   0,  16,   8,
     0,   0,   0,   0,   0,   0,   0,   0,   0,   0,  81, 101,   0,
   112,  21,   7,  67,   0,  79,  96, 118,   0,   0,  68,   0,   0,
    99,  44,   0,  55,   0,  76,   0,  95,  32,  33,  57,  25,   0,
   102,   0,   0,  87,
];

// a_KW_LEN[i] é o comprimento (em bytes) da i-ésima palavra-chave.
pub const A_KW_LEN: &[u8] = &[0,
     7,   7,   5,   4,   6,   4,   5,   3,   6,   7,   3,   6,   6,
     7,   7,   3,   8,   2,   6,   5,   4,   4,   3,  10,   4,   7,
     6,   9,   4,   2,   6,   5,   9,   9,   4,   7,   3,   2,   4,
     4,   6,  11,   6,   2,   7,   5,   5,   9,   6,  10,   4,   6,
     2,   3,   7,   5,   9,   6,   6,   4,   5,   5,  10,   6,   5,
     7,   4,   5,   7,   6,   7,   7,   6,   5,   7,   3,   7,   4,
     7,   6,  12,   9,   4,   6,   5,   4,   7,   6,  12,   8,   8,
     2,   6,   6,   7,   6,   4,   5,   9,   5,   5,   6,   3,   4,
     9,  13,   2,   2,   4,   6,   6,   8,   5,  17,  12,   7,   9,
     4,   4,   6,   7,   5,   9,   4,   4,   5,   2,   5,   8,   6,
     4,   9,   5,   8,   4,   3,   9,   5,   5,   6,   4,   6,   2,
     2,   9,   3,   7,
];

// a_KW_OFFSET[i] é o índice em z_KW_TEXT[] do início do texto
// para a i-ésima palavra-chave.
pub const A_KW_OFFSET: &[u16] = &[0,
     0,   2,   2,   8,   9,  14,  16,  20,  23,  25,  25,  29,  33,
    36,  41,  46,  48,  53,  54,  59,  62,  65,  67,  69,  78,  81,
    86,  90,  90,  94,  99, 101, 105, 111, 119, 123, 123, 123, 126,
   129, 132, 137, 142, 146, 147, 152, 156, 160, 168, 174, 181, 184,
   184, 187, 189, 195, 198, 206, 211, 216, 219, 222, 226, 236, 239,
   244, 244, 248, 252, 259, 265, 271, 277, 277, 283, 284, 288, 295,
   299, 306, 312, 324, 333, 335, 341, 346, 348, 355, 359, 370, 377,
   378, 385, 391, 397, 402, 408, 412, 415, 424, 429, 433, 439, 441,
   444, 453, 455, 457, 466, 470, 476, 482, 490, 495, 495, 495, 511,
   520, 523, 527, 532, 539, 544, 553, 557, 560, 565, 567, 571, 579,
   585, 588, 597, 602, 610, 610, 614, 623, 628, 633, 639, 642, 645,
   648, 650, 655, 659,
];

// a_KW_CODE[i] é o código de símbolo do analisador para a i-ésima palavra-chave.
pub const A_KW_CODE: &[u8] = &[0,
  TK_REINDEX,    TK_INDEXED,    TK_INDEX,      TK_DESC,       TK_ESCAPE,
  TK_EACH,       TK_CHECK,      TK_KEY,        TK_BEFORE,     TK_FOREIGN,
  TK_FOR,        TK_IGNORE,     TK_LIKE_KW,    TK_EXPLAIN,    TK_INSTEAD,
  TK_ADD,        TK_DATABASE,   TK_AS,         TK_SELECT,     TK_TABLE,
  TK_JOIN_KW,    TK_THEN,       TK_END,        TK_DEFERRABLE, TK_ELSE,
  TK_EXCLUDE,    TK_DELETE,     TK_TEMP,       TK_TEMP,       TK_OR,
  TK_ISNULL,     TK_NULLS,      TK_SAVEPOINT,  TK_INTERSECT,  TK_TIES,
  TK_NOTNULL,    TK_NOT,        TK_NO,         TK_NULL,       TK_LIKE_KW,
  TK_EXCEPT,     TK_TRANSACTION,TK_ACTION,     TK_ON,         TK_JOIN_KW,
  TK_ALTER,      TK_RAISE,      TK_EXCLUSIVE,  TK_EXISTS,     TK_CONSTRAINT,
  TK_INTO,       TK_OFFSET,     TK_OF,         TK_SET,        TK_TRIGGER,
  TK_RANGE,      TK_GENERATED,  TK_DETACH,     TK_HAVING,     TK_LIKE_KW,
  TK_BEGIN,      TK_JOIN_KW,    TK_REFERENCES, TK_UNIQUE,     TK_QUERY,
  TK_WITHOUT,    TK_WITH,       TK_JOIN_KW,    TK_RELEASE,    TK_ATTACH,
  TK_BETWEEN,    TK_NOTHING,    TK_GROUPS,     TK_GROUP,      TK_CASCADE,
  TK_ASC,        TK_DEFAULT,    TK_CASE,       TK_COLLATE,    TK_CREATE,
  TK_CTIME_KW,   TK_IMMEDIATE,  TK_JOIN,       TK_INSERT,     TK_MATCH,
  TK_PLAN,       TK_ANALYZE,    TK_PRAGMA,     TK_MATERIALIZED, TK_DEFERRED,
  TK_DISTINCT,   TK_IS,         TK_UPDATE,     TK_VALUES,     TK_VIRTUAL,
  TK_ALWAYS,     TK_WHEN,       TK_WHERE,      TK_RECURSIVE,  TK_ABORT,
  TK_AFTER,      TK_RENAME,     TK_AND,        TK_DROP,       TK_PARTITION,
  TK_AUTOINCR,   TK_TO,         TK_IN,         TK_CAST,       TK_COLUMNKW,
  TK_COMMIT,     TK_CONFLICT,   TK_JOIN_KW,    TK_CTIME_KW,   TK_CTIME_KW,
  TK_CURRENT,    TK_PRECEDING,  TK_FAIL,       TK_LAST,       TK_FILTER,
  TK_REPLACE,    TK_FIRST,      TK_FOLLOWING,  TK_FROM,       TK_JOIN_KW,
  TK_LIMIT,      TK_IF,         TK_ORDER,      TK_RESTRICT,   TK_OTHERS,
  TK_OVER,       TK_RETURNING,  TK_JOIN_KW,    TK_ROLLBACK,   TK_ROWS,
  TK_ROW,        TK_UNBOUNDED,  TK_UNION,      TK_USING,      TK_VACUUM,
  TK_VIEW,       TK_WINDOW,     TK_DO,         TK_BY,         TK_INITIALLY,
  TK_ALL,        TK_PRIMARY,
];

/// Verifica se z[0..n-1] é uma palavra-chave. Se for, escreve o código do
/// símbolo do analisador em p_type. Sempre retorna n (o comprimento do token).
#[inline]
pub fn keyword_code_lookup(z: &[u8], n: usize, p_type: &mut u8) -> usize {
    assert!(n >= 2);

    let mut i = ((char_map(z[0]) as usize * 4) ^ (char_map(z[n - 1]) as usize * 3) ^ (n * 1)) % 127;
    i = A_KW_HASH[i] as usize;

    while i > 0 {
        if A_KW_LEN[i] as usize != n {
            i = A_KW_NEXT[i] as usize;
            continue;
        }

        let z_kw = &Z_KW_TEXT[A_KW_OFFSET[i] as usize..];

        // Caminho ASCII: comparação sem diferenciar caixa, mascarando o bit 0x20.
        if (z[0] & !0x20) != z_kw[0] {
            i = A_KW_NEXT[i] as usize;
            continue;
        }
        if (z[1] & !0x20) != z_kw[1] {
            i = A_KW_NEXT[i] as usize;
            continue;
        }

        let mut j = 2;
        while j < n && (z[j] & !0x20) == z_kw[j] {
            j += 1;
        }

        if j < n {
            i = A_KW_NEXT[i] as usize;
            continue;
        }

        *p_type = A_KW_CODE[i];
        break;
    }

    n
}


// ---- part_001.rs ----

/// Obtém o código do token para uma palavra-chave a partir de uma string.
///
/// Se o texto for uma palavra-chave conhecida, retorna seu código de token.
/// Caso contrário, retorna `TK_ID`.
pub fn keyword_code(z: &[u8], n: i32) -> i32 {
    let mut id: u8 = TK_ID as u8;
    if n >= 2 {
        keyword_code_lookup(z, n as usize, &mut id);
    }
    id as i32
}

pub const N_KEYWORD: i32 = 147;

/// Retorna o nome e comprimento de uma palavra-chave pelo índice.
///
/// Se o índice estiver fora do intervalo válido (menor que 0 ou maior/igual a N_KEYWORD),
/// retorna `SQLITE_ERROR`. Caso contrário, preenche `pz_name` com um fatia do texto
/// da palavra-chave e `pn_name` com o seu comprimento em bytes, e retorna `SQLITE_OK`.
pub fn keyword_name(i: i32, pz_name: &mut &[u8], pn_name: &mut i32) -> i32 {
    if i < 0 || i >= N_KEYWORD {
        return SQLITE_ERROR;
    }
    let idx = (i + 1) as usize;
    let start = A_KW_OFFSET[idx] as usize;
    let length = A_KW_LEN[idx] as usize;
    let end = start + length;
    *pz_name = &Z_KW_TEXT[start..end];
    *pn_name = A_KW_LEN[idx] as i32;
    SQLITE_OK
}

/// Retorna o número total de palavras-chave registradas.
pub fn keyword_count() -> i32 {
    N_KEYWORD
}

/// Verifica se um texto é uma palavra-chave reconhecida.
///
/// Retorna não-zero (verdadeiro) se for uma palavra-chave conhecida,
/// zero (falso) caso contrário.
pub fn keyword_check(z_name: &[u8], n_name: i32) -> i32 {
    if TK_ID as i32 != keyword_code(z_name, n_name) {
        1
    } else {
        0
    }
}

