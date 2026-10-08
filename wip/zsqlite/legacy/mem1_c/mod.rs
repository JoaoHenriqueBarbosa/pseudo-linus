// Mesclado das partes traduzidas de mem1_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

// No Debian (Linux, glibc) o configure define HAVE_MALLOC_H e HAVE_MALLOC_USABLE_SIZE, logo
// SQLITE_MALLOCSIZE vale malloc_usable_size(x) e vale o ramo "#ifdef SQLITE_MALLOCSIZE" de cada
// função. O ramo da Apple e o do prefixo de 8 bytes (sem SQLITE_MALLOCSIZE) somem.

/// Pool de alocações do sistema. Sem ponteiros: cada alocação é um identificador opaco (usize,
/// nunca 0) que aponta para um `Vec<u8>`; o segundo campo é o próximo identificador livre.
static ALLOC_POOL: Mutex<(BTreeMap<usize, Vec<u8>>, usize)> = Mutex::new((BTreeMap::new(), 1));

/// Trava o pool, tolerando envenenamento (o malloc do C não tem esse conceito).
fn lock_pool() -> MutexGuard<'static, (BTreeMap<usize, Vec<u8>>, usize)> {
    ALLOC_POOL.lock().unwrap_or_else(|e| e.into_inner())
}

/// Tamanho utilizável que o malloc_usable_size() da glibc devolveria para um pedido de `n` bytes
/// (chunk de 16 em 16 com cabeçalho de 8 bytes, mínimo de 24 utilizáveis).
fn usable_size(n: usize) -> usize {
    let chunk = (n + 8 + 15) & !15usize;
    chunk.max(32) - 8
}

/// Como malloc(), mas lembra o tamanho da alocação para encontrá-lo depois com mem_size().
///
/// Nesta rotina de baixo nível há garantia de que n_byte>0, porque os casos n_byte<=0 são
/// interceptados por rotinas de nível superior. Devolve 0 quando falha.
fn mem_malloc(n_byte: i32) -> usize {
    let n = n_byte as u32 as usize;
    let mut pool = lock_pool();
    let handle = pool.1;
    let mut v: Vec<u8> = Vec::new();
    let size = usable_size(n);
    if v.try_reserve_exact(size).is_err() {
        drop(pool);
        log(
            SQLITE_NOMEM,
            &format!("failed to allocate {} bytes of memory", n_byte as u32),
        );
        return 0;
    }
    v.resize(size, 0);
    pool.1 = handle + 1;
    pool.0.insert(handle, v);
    handle
}

/// Como free(), mas vale para alocações obtidas de mem_malloc() ou mem_realloc().
///
/// Nesta rotina de baixo nível já se sabe que p_prior!=0, porque os casos p_prior==0 são
/// interceptados por rotinas de nível superior.
fn mem_free(p_prior: usize) {
    debug_assert!(p_prior != 0);
    lock_pool().0.remove(&p_prior);
}

/// Informa o tamanho alocado de um retorno anterior de x_malloc() ou x_realloc().
fn mem_size(p_prior: usize) -> i32 {
    debug_assert!(p_prior != 0);
    lock_pool().0.get(&p_prior).map_or(0, |v| v.len() as i32)
}

/// Como realloc(): redimensiona uma alocação obtida de mem_malloc().
///
/// Nesta interface de baixo nível sabe-se que p_prior!=0 (os casos com p_prior==0 são redirecionados
/// para x_malloc) e que n_byte>0 (os casos n_byte<=0 são redirecionados para x_free). Em falha
/// devolve 0 e a alocação anterior continua válida, como no realloc().
fn mem_realloc(p_prior: usize, n_byte: i32) -> usize {
    let n = n_byte as u32 as usize;
    let mut pool = lock_pool();
    let old_size = match pool.0.get(&p_prior) {
        Some(v) => v.len(),
        None => return 0,
    };
    let size = usable_size(n);
    let ok = {
        let v = pool.0.get_mut(&p_prior).unwrap();
        if size > old_size && v.try_reserve_exact(size - old_size).is_err() {
            false
        } else {
            v.resize(size, 0);
            v.shrink_to_fit();
            true
        }
    };
    if !ok {
        drop(pool);
        log(
            SQLITE_NOMEM,
            &format!(
                "failed memory resize {} to {} bytes",
                old_size as u32, n_byte as u32
            ),
        );
        return 0;
    }
    p_prior
}

/// Arredonda o tamanho de um pedido para o próximo tamanho de alocação válido.
fn mem_roundup(n: i32) -> i32 {
    round8(n)
}

/// Inicializa este módulo.
fn mem_init(_not_used: usize) -> i32 {
    SQLITE_OK
}

/// Desinicializa este módulo.
fn mem_shutdown(_not_used: usize) {}

/// Única rotina deste arquivo com ligação externa.
///
/// Preenche os ponteiros de alocação de baixo nível de sqlite3GlobalConfig.m com as rotinas
/// deste arquivo, via sqlite3_config(SQLITE_CONFIG_MALLOC, &defaultMethods).
pub fn mem_set_default() {
    let default_methods = Sqlite3MemMethods {
        x_malloc: Rc::new(mem_malloc),
        x_free: Rc::new(mem_free),
        x_realloc: Rc::new(mem_realloc),
        x_size: Rc::new(mem_size),
        x_roundup: Rc::new(mem_roundup),
        x_init: Rc::new(mem_init),
        x_shutdown: Rc::new(mem_shutdown),
        p_app_data: 0,
    };
    api::config(SQLITE_CONFIG_MALLOC, ConfigArg::Malloc(default_methods));
}

