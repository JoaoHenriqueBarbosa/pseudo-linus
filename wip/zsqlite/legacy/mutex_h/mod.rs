// Mesclado das partes traduzidas de mutex_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Arquivo de origem: mutex.h
// Cabeçalho comum para todas as implementações de mutex do SQLite.
//
// Este arquivo define a interface pública de mutex e escolhe qual implementação
// será usada com base nas opções de compilação. Para Debian 13 com SQLITE_THREADSAFE=1
// em ambiente Unix, a implementação pthreads é selecionada automaticamente.

// As diferentes estratégias de implementação:
// - SQLITE_MUTEX_OMIT: sem lógica de mutex (sequer stubs); não pode ser substituída em runtime
// - SQLITE_MUTEX_NOOP: para aplicações single-threaded; pode ser substituída em runtime
// - SQLITE_MUTEX_PTHREADS: para aplicações multi-threaded em Unix
// - SQLITE_MUTEX_W32: para aplicações multi-threaded em Win32

// Para Debian 13, a configuração é: SQLITE_THREADSAFE=1 e SQLITE_OS_UNIX,
// o que resulta em SQLITE_MUTEX_PTHREADS.

/// Macro que envolve código que deve ser compilado apenas quando mutex está habilitado.
/// Em implementações com mutex, isto é um passthrough; em SQLITE_MUTEX_OMIT, expande para nada.
/// Para Debian 13, esta macro é ativa (expande o argumento como está).
#[inline]
pub fn mutex_logic<T>(x: T) -> T {
    x
}

// A função sqlite3_mutex_held é declarada aqui como parte da interface pública de mutex.
// Sua implementação está no módulo mutex_unix (mutex_pthreads.c), como `api::mutex_held`.
//
// Assinatura esperada (sem ponteiros): pub fn mutex_held(m: &MutexRef) -> i32
// Esta função consulta se um mutex está atualmente mantido (locked) pela thread chamadora.
// Retorna não-zero (verdadeiro) se o mutex é mantido, zero caso contrário.

