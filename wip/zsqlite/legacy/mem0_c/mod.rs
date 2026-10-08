// Mesclado das partes traduzidas de mem0_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Tradução de mem0.c (trecho 000). Todo o corpo do arquivo C está sob
// `#ifdef SQLITE_ZERO_MALLOC`, opção que o build do Debian 13 não define.
// Por isso o `#ifdef` resolvido some por inteiro: não existem aqui os drivers
// de alocação sem operação (sqlite3MemMalloc, sqlite3MemFree, sqlite3MemRealloc,
// sqlite3MemSize, sqlite3MemRoundup, sqlite3MemInit, sqlite3MemShutdown) nem
// sqlite3MemSetDefault. O alocador padrão vive em mem1.c (módulo mem1_c).

