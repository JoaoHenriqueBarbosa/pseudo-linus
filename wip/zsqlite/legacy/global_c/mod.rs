// Mesclado das partes traduzidas de global_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Tabela que mapeia todos os caracteres em maiúsculas para seus equivalentes em minúsculas.
///
/// O SQLite considera apenas caracteres US-ASCII (ou EBCDIC). Não tratamos conversões
/// de maiúsculas/minúsculas para conjuntos de caracteres UTF-8 já que as tabelas seriam
/// quase tão grandes quanto o próprio SQLite. Só o ramo SQLITE_ASCII existe (o Debian é ASCII).
///
/// Os 18 inteiros finais não têm relação com a conversão: são anexados ao array para evitar
/// avisos de UBSAN. As comparações SQL (<>, =, >, <=, <, >=) usam `mem_compare(A,B)` e consultam
/// `A_LTB[opcode]`, `A_EQB[opcode]` ou `A_GTB[opcode]` conforme `compare(A,B)` seja negativo,
/// zero ou positivo. Isso só funciona porque os opcodes de comparação são consecutivos e na
/// ordem NE EQ GT LE LT GE.
pub const UPPER_TO_LOWER: &[u8] = &[
    0,  1,  2,  3,  4,  5,  6,  7,  8,  9, 10, 11, 12, 13, 14, 15, 16, 17,
   18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35,
   36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53,
   54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 97, 98, 99,100,101,102,103,
  104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,
  122, 91, 92, 93, 94, 95, 96, 97, 98, 99,100,101,102,103,104,105,106,107,
  108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,
  126,127,128,129,130,131,132,133,134,135,136,137,138,139,140,141,142,143,
  144,145,146,147,148,149,150,151,152,153,154,155,156,157,158,159,160,161,
  162,163,164,165,166,167,168,169,170,171,172,173,174,175,176,177,178,179,
  180,181,182,183,184,185,186,187,188,189,190,191,192,193,194,195,196,197,
  198,199,200,201,202,203,204,205,206,207,208,209,210,211,212,213,214,215,
  216,217,218,219,220,221,222,223,224,225,226,227,228,229,230,231,232,233,
  234,235,236,237,238,239,240,241,242,243,244,245,246,247,248,249,250,251,
  252,253,254,255,
  // NE  EQ  GT  LE  LT  GE
  1,  0,  0,  1,  1,  0,  // aLTb[]: usar quando compare(A,B) for menor que zero
  0,  1,  0,  1,  0,  1,  // aEQb[]: usar quando compare(A,B) for igual a zero
  1,  0,  1,  0,  0,  1   // aGTb[]: usar quando compare(A,B) for maior que zero
];

/// Equivalente de `sqlite3aLTb = &sqlite3UpperToLower[256-OP_Ne]`: tabela de resultado
/// "menor que" indexada pelo opcode de comparação.
#[inline]
pub fn a_ltb() -> &'static [u8] {
    &UPPER_TO_LOWER[256 - OP_NE as usize..]
}

/// Equivalente de `sqlite3aEQb = &sqlite3UpperToLower[256+6-OP_Ne]`: tabela de resultado
/// "igual a" indexada pelo opcode de comparação.
#[inline]
pub fn a_eqb() -> &'static [u8] {
    &UPPER_TO_LOWER[256 + 6 - OP_NE as usize..]
}

/// Equivalente de `sqlite3aGTb = &sqlite3UpperToLower[256+12-OP_Ne]`: tabela de resultado
/// "maior que" indexada pelo opcode de comparação.
#[inline]
pub fn a_gtb() -> &'static [u8] {
    &UPPER_TO_LOWER[256 + 12 - OP_NE as usize..]
}

/// Tabela de busca de 256 bytes que sustenta os equivalentes embutidos das funções da
/// biblioteca padrão:
///
/// - `isspace()`: 0x01
/// - `isalpha()`: 0x02
/// - `isdigit()`: 0x04
/// - `isalnum()`: 0x06
/// - `isxdigit()`: 0x08
/// - `toupper()`: 0x20
/// - caractere de identificador SQLite: 0x40 (`$`, `_` ou não ASCII)
/// - caractere de aspas: 0x80
///
/// O bit 0x20 vale 1 se o caractere exige tradução para maiúscula, ou seja, se é uma letra
/// ASCII minúscula. Nesse caso o equivalente maiúsculo é `x - 0x20`, então `toupper()` é
/// `x & !(map[x] & 0x20)`. O bit 0x40 marca o caractere não alfanumérico que pode aparecer em
/// identificador: identificadores são alfanuméricos, `_`, `$` e qualquer caractere UTF não ASCII,
/// por isso o teste de "faz parte de identificador" é 0x46.
pub const CTYPE_MAP: [u8; 256] = [
  0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  /* 00..07 */
  0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00,  /* 08..0f */
  0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  /* 10..17 */
  0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  /* 18..1f */
  0x01, 0x00, 0x80, 0x00, 0x40, 0x00, 0x00, 0x80,  /* 20..27 */
  0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  /* 28..2f */
  0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x0c,  /* 30..37 */
  0x0c, 0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  /* 38..3f */

  0x00, 0x0a, 0x0a, 0x0a, 0x0a, 0x0a, 0x0a, 0x02,  /* 40..47 */
  0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02,  /* 48..4f */
  0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02,  /* 50..57 */
  0x02, 0x02, 0x02, 0x80, 0x00, 0x00, 0x00, 0x40,  /* 58..5f */
  0x80, 0x2a, 0x2a, 0x2a, 0x2a, 0x2a, 0x2a, 0x22,  /* 60..67 */
  0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,  /* 68..6f */
  0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,  /* 70..77 */
  0x22, 0x22, 0x22, 0x00, 0x00, 0x00, 0x00, 0x00,  /* 78..7f */

  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,  /* 80..87 */
  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,  /* 88..8f */
  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,  /* 90..97 */
  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,  /* 98..9f */
  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,  /* a0..a7 */
  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,  /* a8..af */
  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,  /* b0..b7 */
  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,  /* b8..bf */

  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,  /* c0..c7 */
  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,  /* c8..cf */
  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,  /* d0..d7 */
  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,  /* d8..df */
  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,  /* e0..e7 */
  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,  /* e8..ef */
  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,  /* f0..f7 */
  0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40   /* f8..ff */
];

/// Compatibilidade com aplicações antigas: o nome de arquivo em formato URI vem desabilitado
/// por padrão (SQLITE_USE_URI=0 no Debian).
pub const SQLITE_USE_URI: i32 = 0;

/// A varredura de índice de cobertura vem ligada por padrão.
pub const SQLITE_ALLOW_COVERING_INDEX_SCAN: i32 = 1;

/// O tamanho mínimo de um PMA do ordenador é este valor multiplicado pelo tamanho da página
/// do banco em bytes.
pub const SQLITE_SORTER_PMASZ: i32 = 250;

/// O journal de instrução vai para o disco quando seu tamanho passa deste limite (em bytes).
/// 0 cria o journal e grava em disco imediatamente; -1 mantém tudo em memória.
pub const SQLITE_STMTJRNL_SPILL: i32 = 64 * 1024;

/// Configuração padrão do lookaside, no formato "SZ,N": SZ bytes por slot e N slots.
/// Com o lookaside de dois tamanhos, 1200,40 dá 30 slots de 1200 bytes e 93 de 128 bytes
/// (48 KB de memória).
pub const SQLITE_DEFAULT_LOOKASIDE_SZ: i32 = 1200;
pub const SQLITE_DEFAULT_LOOKASIDE_N: i32 = 40;

/// Tamanho máximo padrão de um banco em memória criado com `deserialize`.
pub const SQLITE_MEMDB_DEFAULT_MAXSIZE: i64 = 1073741824;

/// Valor inicial do singleton de configuração global da biblioteca (`sqlite3Config`).
/// Estado global mutável não cabe em `static` imutável: o integrador guarda o resultado onde
/// preferir (por exemplo atrás de um `Mutex` ou `thread_local!`).
pub fn config_default() -> Sqlite3Config {
    Sqlite3Config {
        b_memstat: SQLITE_DEFAULT_MEMSTATUS as _,
        b_core_mutex: 1,
        b_full_mutex: (SQLITE_THREADSAFE == 1) as _,
        b_open_uri: SQLITE_USE_URI as _,
        b_use_cis: SQLITE_ALLOW_COVERING_INDEX_SCAN as _,
        b_small_malloc: 0,
        b_extra_schema_checks: 1,
        // sizeof(long double) > 8 no x86-64 do Debian (16 bytes).
        b_use_long_double: 1,
        mx_strlen: 0x7ffffffe,
        never_corrupt: 0,
        sz_lookaside: SQLITE_DEFAULT_LOOKASIDE_SZ as _,
        n_lookaside: SQLITE_DEFAULT_LOOKASIDE_N as _,
        n_stmt_spill: SQLITE_STMTJRNL_SPILL as _,
        m: Default::default(),
        mutex: Default::default(),
        pcache2: Default::default(),
        p_heap: None,
        n_heap: 0,
        mn_heap: 0,
        mx_heap: 0,
        sz_mmap: SQLITE_DEFAULT_MMAP_SIZE as _,
        mx_mmap: SQLITE_MAX_MMAP_SIZE as _,
        p_page: None,
        sz_page: 0,
        n_page: SQLITE_DEFAULT_PCACHE_INITSZ as _,
        mx_parser_stack: 0,
        shared_cache_enabled: 0,
        sz_pma: SQLITE_SORTER_PMASZ as _,
        is_init: 0,
        in_progress: 0,
        is_mutex_init: 0,
        is_malloc_init: 0,
        is_p_cache_init: 0,
        n_ref_init_mutex: 0,
        p_init_mutex: None,
        x_log: None,
        p_log_arg: None,
        mx_memdb_size: SQLITE_MEMDB_DEFAULT_MAXSIZE as _,
        x_test_callback: None,
        b_localtime_fault: 0,
        x_alt_localtime: None,
        i_once_reset_threshold: 0x7ffffffe,
        sz_sorter_ref: SQLITE_DEFAULT_SORTERREF_SIZE as _,
        i_prng_seed: 0,
    }
}

/// Tabela hash das funções globais (comuns a todas as conexões). Depois da inicialização é
/// somente leitura. O integrador decide onde ela mora (o `Rc` não é `Sync`).
pub fn builtin_functions_new() -> FuncDefHash {
    FuncDefHash {
        a: std::array::from_fn(|_| None),
    }
}

/// O byte "pendente" deve valer 0x40000000 (1 byte depois do limite de 1 GiB) em um banco
/// compatível. O SQLite nunca lê nem grava a página que contém esse byte: ela fica reservada
/// para a camada de VFS gerenciar travas de arquivo. Mudar o valor gera arquivo incompatível;
/// só `test_control` o move, e nunca durante a operação.
pub static PENDING_BYTE: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(0x40000000);

/// Flags de rastreamento definidas por SQLITE_TESTCTRL_TRACEFLAGS.
pub static TREE_TRACE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub static WHERE_TRACE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Propriedades dos opcodes (`OPFLG_INITIALIZER`, gerado a partir de opcodes.h).
pub const OPCODE_PROPERTY: &[u8] = &OPFLG_INITIALIZER;

/// Nome da sequência de ordenação padrão.
pub const STR_BINARY: &[u8] = b"BINARY";

/// Comprimento (em bytes) de cada entrada de `STD_TYPE`.
pub const STD_TYPE_LEN: &[u8] = &[3, 4, 3, 7, 4, 4];

/// Afinidade associada a cada entrada de `STD_TYPE`.
pub const STD_TYPE_AFFINITY: &[u8] = &[
  SQLITE_AFF_NUMERIC as u8,
  SQLITE_AFF_BLOB as u8,
  SQLITE_AFF_INTEGER as u8,
  SQLITE_AFF_INTEGER as u8,
  SQLITE_AFF_REAL as u8,
  SQLITE_AFF_TEXT as u8
];


// ---- part_001.rs ----

/// Tipos padrão do SQLite.
pub const STD_TYPE: &[&str] = &[
    "ANY",
    "BLOB",
    "INT",
    "INTEGER",
    "REAL",
    "TEXT",
];

