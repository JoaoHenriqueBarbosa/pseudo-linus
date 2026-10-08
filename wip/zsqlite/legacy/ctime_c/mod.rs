// Mesclado das partes traduzidas de ctime_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Versão do compilador que o `COMPILER=gcc-` do C estampa (`__VERSION__` do gcc do Debian 13).
pub const COMPILER_VERSION: &str = "14.2.0";

/// Primeira metade de `sqlite3azCompileOpt` (ordem A-Z do C, até `ENABLE_VFSTRACE`).
/// Os `#ifdef` já estão resolvidos para as opções de compilação do Debian 13: só entra o
/// que está definido. As opções ausentes (32BIT_ROWID, ..., ENABLE_VFSTRACE exceto as abaixo)
/// somem, como no binário real. Cada entrada que carrega valor traz o texto já montado.
pub const COMPILE_OPTIONS_PART_0: &[&str] = &[
    // "COMPILER=gcc-" __VERSION__ (o ramo clang e o msvc não existem no Debian)
    "COMPILER=gcc-14.2.0",
    "ENABLE_COLUMN_METADATA",
    "ENABLE_DBPAGE_VTAB",
    "ENABLE_DBSTAT_VTAB",
    "ENABLE_FTS3",
    "ENABLE_FTS3_PARENTHESIS",
    "ENABLE_FTS4",
    "ENABLE_FTS5",
    "ENABLE_GEOPOLY",
    "ENABLE_LOAD_EXTENSION",
    "ENABLE_MATH_FUNCTIONS",
    "ENABLE_PREUPDATE_HOOK",
    "ENABLE_RTREE",
    "ENABLE_SESSION",
    "ENABLE_STMTVTAB",
    "ENABLE_UNLOCK_NOTIFY",
];


// ---- part_001.rs ----

/// Segunda metade de `sqlite3azCompileOpt` (de `ENABLE_WHERETRACE` até `ZERO_MALLOC`), com os
/// `#ifdef` resolvidos para o Debian 13. `HAVE_ISNAN` entra porque `HAVE_ISNAN || SQLITE_HAVE_ISNAN`
/// vale no build autoconf. `SYSTEM_MALLOC` entra pela condição padrão (sem WIN32_MALLOC,
/// ZERO_MALLOC nem MEMDEBUG). `THREADSAFE=1` vem do ramo `SQLITE_THREADSAFE` ou do `#else`.
pub const COMPILE_OPTIONS_PART_1: &[&str] = &[
    "HAVE_ISNAN",
    "MAX_SCHEMA_RETRY=25",
    "MAX_VARIABLE_NUMBER=250000",
    "SECURE_DELETE",
    "SOUNDEX",
    "SYSTEM_MALLOC",
    "THREADSAFE=1",
];


// ---- part_002.rs ----

/// `sqlite3CompileOptions`: devolve a lista de nomes das opções de compilação, na ordem do C.
/// O `*pnOpt` do C é o `len()` do vetor devolvido.
pub fn compile_options() -> Vec<&'static str> {
    let mut all: Vec<&'static str> =
        Vec::with_capacity(COMPILE_OPTIONS_PART_0.len() + COMPILE_OPTIONS_PART_1.len());
    all.extend_from_slice(COMPILE_OPTIONS_PART_0);
    all.extend_from_slice(COMPILE_OPTIONS_PART_1);
    all
}

