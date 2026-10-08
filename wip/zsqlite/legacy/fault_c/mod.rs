// Mesclado das partes traduzidas de fault_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

use std::sync::Mutex;

// Suporte ao conceito de falhas de malloc "benignas" (quando o xMalloc() ou o
// xRealloc() de sqlite3_mem_methods não consegue alocar e devolve 0).
//
// A maioria das falhas de malloc não é benigna: depois delas o SQLite abandona
// a operação e devolve um código de erro (em geral SQLITE_NOMEM). Às vezes,
// porém, a falha não é fatal: se o malloc falha ao redimensionar uma tabela
// hash, basta não redimensionar e a tabela continua funcionando. Essa falha é
// benigna.

/// Ganchos chamados ao entrar e sair de uma seção de malloc benigno.
/// Ponteiros de função do C (`void (*)(void)`) viram `fn()`.
#[derive(Clone, Copy)]
pub struct BenignMallocHooks {
    /// Chamado no início de uma seção de malloc benigno.
    pub x_benign_begin: Option<fn()>,
    /// Chamado no fim de uma seção de malloc benigno.
    pub x_benign_end: Option<fn()>,
}

/// Estado global dos ganchos (`sqlite3Hooks` no C).
static SQLITE3_HOOKS: Mutex<BenignMallocHooks> = Mutex::new(BenignMallocHooks {
    x_benign_begin: None,
    x_benign_end: None,
});

/// Lê uma cópia dos ganchos sem segurar o lock durante a chamada deles.
fn load_hooks() -> BenignMallocHooks {
    match SQLITE3_HOOKS.lock() {
        Ok(hooks) => *hooks,
        Err(poisoned) => *poisoned.into_inner(),
    }
}

/// Registra os ganchos chamados por `begin_benign_malloc()` e
/// `end_benign_malloc()`, respectivamente.
pub fn benign_malloc_hooks(x_benign_begin: Option<fn()>, x_benign_end: Option<fn()>) {
    let mut hooks = match SQLITE3_HOOKS.lock() {
        Ok(hooks) => hooks,
        Err(poisoned) => poisoned.into_inner(),
    };
    hooks.x_benign_begin = x_benign_begin;
    hooks.x_benign_end = x_benign_end;
}

/// Chamada pelo código do SQLite para indicar que as falhas de malloc
/// seguintes são benignas.
pub fn begin_benign_malloc() {
    if let Some(callback) = load_hooks().x_benign_begin {
        callback();
    }
}

/// Indica que as falhas de malloc seguintes voltam a ser não benignas.
pub fn end_benign_malloc() {
    if let Some(callback) = load_hooks().x_benign_end {
        callback();
    }
}

