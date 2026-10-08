//! Tradução de keywordhash.h (gerado por tool/mkkeywordhash.c), incluído no meio de tokenize.c.
//!
//! Decide se um identificador é palavra-chave SQL e devolve o código do símbolo do parser.
//! As tabelas são cópia exata das do C; o índice 0 de cada tabela de 148 entradas é o sentinela
//! "sem palavra-chave", como no original.

use crate::consts::*;
use crate::tokenize::char_map;

/// Número de palavras-chave (`SQLITE_N_KEYWORD`).
const SQLITE_N_KEYWORD: i32 = 147;

/// `zKWText[]`: texto das palavras-chave, sobrepostas.
static KW_TEXT: [u8; 666] = *b"\
REINDEXEDESCAPEACH\
ECKEYBEFOREIGNOREG\
EXPLAINSTEADDATABA\
SELECTABLEFTHENDEF\
ERRABLELSEXCLUDELE\
TEMPORARYISNULLSAV\
EPOINTERSECTIESNOT\
NULLIKEXCEPTRANSAC\
TIONATURALTERAISEX\
CLUSIVEXISTSCONSTR\
AINTOFFSETRIGGERAN\
GENERATEDETACHAVIN\
GLOBEGINNEREFERENC\
ESUNIQUERYWITHOUTE\
RELEASEATTACHBETWE\
ENOTHINGROUPSCASCA\
DEFAULTCASECOLLATE\
CREATECURRENT_DATE\
IMMEDIATEJOINSERTM\
ATCHPLANALYZEPRAGM\
ATERIALIZEDEFERRED\
ISTINCTUPDATEVALUE\
SVIRTUALWAYSWHENWH\
ERECURSIVEABORTAFT\
ERENAMEANDROPARTIT\
IONAUTOINCREMENTCA\
STCOLUMNCOMMITCONF\
LICTCROSSCURRENT_T\
IMESTAMPRECEDINGFA\
ILASTFILTEREPLACEF\
IRSTFOLLOWINGFROMF\
ULLIMITIFORDERESTR\
ICTOTHERSOVERETURN\
INGRIGHTROLLBACKRO\
WSUNBOUNDEDUNIONUS\
INGVACUUMVIEWINDOW\
BYINITIALLYPRIMARY";

/// `aKWHash[i]`: valor de hash da i-ésima palavra-chave.
static KW_HASH: [u8; 127] = [
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

/// `aKWNext[]`: cadeia de colisões. Se `KW_HASH[i]==0` a palavra não tem mais colisões; senão a
/// próxima com o mesmo hash é `KW_HASH[i]-1`.
static KW_NEXT: [u8; 148] = [0,
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

/// `aKWLen[i]`: comprimento em bytes da i-ésima palavra-chave.
static KW_LEN: [u8; 148] = [0,
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

/// `aKWOffset[i]`: índice em `KW_TEXT` do início do texto da i-ésima palavra-chave.
static KW_OFFSET: [u16; 148] = [0,
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

/// `aKWCode[i]`: código do símbolo do parser da i-ésima palavra-chave.
static KW_CODE: [u8; 148] = [0,
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

/// `keywordCode`: verifica se `z[0..n-1]` (com `n = z.len() >= 2`) é palavra-chave. Se for,
/// grava o código do símbolo em `tk_type`. Devolve sempre `n`, o comprimento do token.
pub(crate) fn keyword_code_raw(z: &[u8], tk_type: &mut i32) -> i32 {
    let n = z.len();
    debug_assert!(n >= 2);
    let h = ((char_map(z[0]) as usize * 4) ^ (char_map(z[n - 1]) as usize * 3) ^ n) % 127;
    let mut i = KW_HASH[h] as usize;
    while i > 0 {
        if KW_LEN[i] as usize == n {
            let kw = &KW_TEXT[KW_OFFSET[i] as usize..];
            if (z[0] & !0x20) == kw[0] && (z[1] & !0x20) == kw[1] {
                let mut j = 2;
                while j < n && (z[j] & !0x20) == kw[j] {
                    j += 1;
                }
                if j >= n {
                    *tk_type = KW_CODE[i] as i32;
                    break;
                }
            }
        }
        i = KW_NEXT[i] as usize;
    }
    n as i32
}

/// `sqlite3KeywordCode`: código do token da palavra `z`, ou `TK_ID` se não for palavra-chave.
pub fn keyword_code(z: &[u8]) -> i32 {
    let mut id = TK_ID as i32;
    if z.len() >= 2 {
        keyword_code_raw(z, &mut id);
    }
    id
}

/// `sqlite3_keyword_name`: nome da i-ésima palavra-chave (0 <= i < 147), ou `SQLITE_ERROR`.
pub fn keyword_name(i: i32) -> Result<&'static [u8], i32> {
    if !(0..SQLITE_N_KEYWORD).contains(&i) {
        return Err(SQLITE_ERROR);
    }
    let i = (i + 1) as usize;
    let off = KW_OFFSET[i] as usize;
    Ok(&KW_TEXT[off..off + KW_LEN[i] as usize])
}

/// `sqlite3_keyword_count`.
pub fn keyword_count() -> i32 {
    SQLITE_N_KEYWORD
}

/// `sqlite3_keyword_check`: verdadeiro se `name` é palavra-chave SQL.
pub fn keyword_check(name: &[u8]) -> bool {
    TK_ID as i32 != keyword_code(name)
}
