// Mesclado das partes traduzidas de os_setup_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Detecção de sistema operacional (os_setup.h).
//
// No C, exatamente um entre SQLITE_OS_KV, SQLITE_OS_OTHER, SQLITE_OS_UNIX e
// SQLITE_OS_WIN vale 1 e os demais valem 0. O alvo do porte é o Debian 13, então
// o `#ifdef` resolvido é o do Unix: as variantes Windows, KV e OTHER somem, junto
// com as opções SQLITE_OMIT_* e SQLITE_TEMP_STORE que só o ramo KV impõe.

pub const SQLITE_OS_KV: i32 = 0;
pub const SQLITE_OS_OTHER: i32 = 0;
pub const SQLITE_OS_UNIX: i32 = 1;
pub const SQLITE_OS_WIN: i32 = 0;

