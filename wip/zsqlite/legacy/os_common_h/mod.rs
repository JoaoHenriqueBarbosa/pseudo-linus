// Mesclado das partes traduzidas de os_common_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Início da marcação de tempo de desempenho (`TIMER_START`). Expande para nada sem
/// `SQLITE_PERFORMANCE_TRACE`, que o Debian 13 não define.
#[inline]
pub fn timer_start() {
    // SQLITE_PERFORMANCE_TRACE desabilitado no Debian 13.
}

/// Finaliza marcação de tempo de desempenho.
#[inline]
pub fn timer_end() {
    // SQLITE_PERFORMANCE_TRACE desabilitado no Debian 13.
}

/// Tempo decorrido da marcação anterior.
pub const TIMER_ELAPSED: u64 = 0;

