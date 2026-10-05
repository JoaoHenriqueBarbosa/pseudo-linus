//! Tabelas do ps (output.c): `format_array`, `macro_array`, `aix_array` e `shortsort_array`.
//!
//! O `FORMAT_ARRAY` e o `MACRO_ARRAY` foram gerados do `output.c` do procps-ng 4.0.4 por um awk (são
//! 275 linhas de dados); a ordem é a do upstream, que a busca binária dele exige ordenada por
//! `strcmp`. Aqui a busca é linear sobre a mesma tabela.

use super::output::*;
use super::{Fmt, Item};
use super::{AIX, BSD, DEC, HPU, LNX, SCO, SGI, SOE, SUN, TST, U98, XXX};

const USER: u32 = super::CF_USER;
const LEFT: u32 = super::CF_LEFT;
const RIGHT: u32 = super::CF_RIGHT;
const UNLIMITED: u32 = super::CF_UNLIMITED;
const WCHAN: u32 = super::CF_WCHAN;
const SIGNAL: u32 = super::CF_SIGNAL;
const PIDMAX: u32 = super::CF_PIDMAX;
const TO: u32 = super::CF_PRINT_THREAD_ONLY;
const PO: u32 = super::CF_PRINT_PROCESS_ONLY;
const ET: u32 = super::CF_PRINT_EVERY_TIME;
const AN: u32 = super::CF_PRINT_AS_NEEDED;

/// Procura um especificador de formato (`search_format_array`).
pub fn search_format_array(spec: &[u8]) -> Option<&'static Fmt> {
    FORMAT_ARRAY.iter().find(|f| f.spec.as_bytes() == spec)
}

/// Procura uma macro de formato (`search_macro_array`).
pub fn search_macro_array(spec: &[u8]) -> Option<&'static str> {
    MACRO_ARRAY.iter().find(|m| m.0.as_bytes() == spec).map(|m| m.1)
}

/// Código AIX (`%a`) para o especificador e o cabeçalho (`search_aix_array`).
pub fn search_aix_array(code: u8) -> Option<(&'static str, &'static str)> {
    AIX_ARRAY.iter().find(|a| a.0 == code).map(|a| (a.1, a.2))
}

/// Código de ordenação curto (`O`, `k`) para o especificador (`search_shortsort_array`).
pub fn search_shortsort_array(code: u8) -> Option<&'static str> {
    SHORTSORT_ARRAY.iter().find(|a| a.0 == code).map(|a| a.1)
}

const AIX_ARRAY: &[(u8, &str, &str)] = &[
    (b'C', "pcpu", "%CPU"),
    (b'G', "group", "GROUP"),
    (b'P', "ppid", "PPID"),
    (b'U', "user", "USER"),
    (b'a', "args", "COMMAND"),
    (b'c', "comm", "COMMAND"),
    (b'g', "rgroup", "RGROUP"),
    (b'n', "nice", "NI"),
    (b'p', "pid", "PID"),
    (b'r', "pgid", "PGID"),
    (b't', "etime", "ELAPSED"),
    (b'u', "ruser", "RUSER"),
    (b'x', "time", "TIME"),
    (b'y', "tty", "TTY"),
    (b'z', "vsz", "VSZ"),
];

const SHORTSORT_ARRAY: &[(u8, &str)] = &[
    (b'C', "pcpu"),
    (b'G', "tpgid"),
    (b'J', "cstime"),
    (b'M', "maj_flt"),
    (b'N', "cmaj_flt"),
    (b'P', "ppid"),
    (b'R', "resident"),
    (b'S', "share"),
    (b'T', "start_time"),
    (b'U', "uid"),
    (b'c', "cmd"),
    (b'f', "flags"),
    (b'g', "pgrp"),
    (b'j', "cutime"),
    (b'k', "utime"),
    (b'm', "min_flt"),
    (b'n', "cmin_flt"),
    (b'o', "session"),
    (b'p', "pid"),
    (b'r', "rss"),
    (b's', "size"),
    (b't', "tty"),
    (b'u', "user"),
    (b'v', "vsize"),
    (b'y', "priority"),
];

/// `format_array` do output.c, na ordem do upstream.
pub static FORMAT_ARRAY: &[Fmt] = &[
