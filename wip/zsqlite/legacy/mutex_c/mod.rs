// Mesclado das partes traduzidas de mutex_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Premissas sobre o prelude (o tech lead as implementa em um só lugar):
// - `sqlite3_mutex` é um tipo com mutabilidade interior; o handle dono é `Rc<sqlite3_mutex>`.
// - `Sqlite3MutexMethods` é `Clone` e tem os campos `x_mutex_init`, `x_mutex_end`, `x_mutex_alloc`,
//   `x_mutex_free`, `x_mutex_enter`, `x_mutex_try`, `x_mutex_leave`, `x_mutex_held` e
//   `x_mutex_notheld`, todos `Option<fn(..)>` (o C testa ponteiro nulo).
// - `with_global_config(|cfg| ..)` e `with_global_config_mut(|cfg| ..)` dão acesso a
//   `sqlite3GlobalConfig` sem `static mut` (campos `cfg.mutex` e `cfg.b_core_mutex`).
// - `default_mutex()` e `noop_mutex()` devolvem `Sqlite3MutexMethods` por valor.
//
// Ramos removidos: SQLITE_ENABLE_MULTITHREADED_CHECKS não está nas opções do Debian 13, então o
// bloco CheckMutex inteiro e `sqlite3MutexWarnOnContention` somem; SQLITE_DEBUG some (o
// `mutexIsInit` só existia nele). SQLITE_OMIT_AUTOINIT não está definido, as chamadas de
// autoinicialização ficam.

/// Inicializa o subsistema de mutex.
pub fn mutex_init() -> i32 {
    let mut rc = SQLITE_OK;
    let has_alloc = with_global_config(|cfg| cfg.mutex.x_mutex_alloc.is_some());
    if !has_alloc {
        // Se x_mutex_alloc não foi definido, o usuário não instalou uma implementação de mutex
        // via sqlite3_config() antes de sqlite3_initialize(). Este bloco copia as funções da
        // implementação padrão para a configuração global.
        let core_mutex = with_global_config(|cfg| cfg.b_core_mutex);
        let p_from = if core_mutex {
            default_mutex()
        } else {
            noop_mutex()
        };
        with_global_config_mut(|cfg| {
            cfg.mutex.x_mutex_init = p_from.x_mutex_init;
            cfg.mutex.x_mutex_end = p_from.x_mutex_end;
            cfg.mutex.x_mutex_free = p_from.x_mutex_free;
            cfg.mutex.x_mutex_enter = p_from.x_mutex_enter;
            cfg.mutex.x_mutex_try = p_from.x_mutex_try;
            cfg.mutex.x_mutex_leave = p_from.x_mutex_leave;
            cfg.mutex.x_mutex_held = p_from.x_mutex_held;
            cfg.mutex.x_mutex_notheld = p_from.x_mutex_notheld;
        });
        memory_barrier();
        with_global_config_mut(|cfg| {
            cfg.mutex.x_mutex_alloc = p_from.x_mutex_alloc;
        });
    }
    let x_init = with_global_config(|cfg| cfg.mutex.x_mutex_init);
    assert!(x_init.is_some());
    if let Some(f) = x_init {
        rc = f();
    }

    memory_barrier();
    rc
}

/// Encerra o subsistema de mutex. Libera os recursos alocados por `mutex_init`.
pub fn mutex_end() -> i32 {
    let mut rc = SQLITE_OK;
    let x_end = with_global_config(|cfg| cfg.mutex.x_mutex_end);
    if let Some(f) = x_end {
        rc = f();
    }
    rc
}

/// Devolve um mutex estático ou aloca um dinâmico novo (API pública `sqlite3_mutex_alloc`).
pub fn mutex_alloc_api(id: i32) -> Option<Rc<sqlite3_mutex>> {
    if id <= SQLITE_MUTEX_RECURSIVE && api::initialize() != 0 {
        return None;
    }
    if id > SQLITE_MUTEX_RECURSIVE && mutex_init() != 0 {
        return None;
    }
    let x_alloc = with_global_config(|cfg| cfg.mutex.x_mutex_alloc);
    assert!(x_alloc.is_some());
    match x_alloc {
        Some(f) => f(id),
        None => None,
    }
}

/// Versão interna de alocação de mutex (`sqlite3MutexAlloc`).
pub fn mutex_alloc(id: i32) -> Option<Rc<sqlite3_mutex>> {
    if !with_global_config(|cfg| cfg.b_core_mutex) {
        return None;
    }
    let x_alloc = with_global_config(|cfg| cfg.mutex.x_mutex_alloc);
    assert!(x_alloc.is_some());
    match x_alloc {
        Some(f) => f(id),
        None => None,
    }
}

/// Libera um mutex dinâmico.
pub fn mutex_free(p: Option<Rc<sqlite3_mutex>>) {
    if let Some(p) = p {
        let x_free = with_global_config(|cfg| cfg.mutex.x_mutex_free);
        assert!(x_free.is_some());
        if let Some(f) = x_free {
            f(p);
        }
    }
}

/// Obtém o mutex `p`. Se outra thread o tem, bloqueia até conseguir.
pub fn mutex_enter(p: Option<&sqlite3_mutex>) {
    if let Some(p) = p {
        let x_enter = with_global_config(|cfg| cfg.mutex.x_mutex_enter);
        assert!(x_enter.is_some());
        if let Some(f) = x_enter {
            f(p);
        }
    }
}

/// Obtém o mutex `p` sem bloquear. Devolve SQLITE_OK se conseguiu, SQLITE_BUSY se outra thread o
/// mantém.
pub fn mutex_try(p: Option<&sqlite3_mutex>) -> i32 {
    let rc = SQLITE_OK;
    if let Some(p) = p {
        let x_try = with_global_config(|cfg| cfg.mutex.x_mutex_try);
        assert!(x_try.is_some());
        if let Some(f) = x_try {
            return f(p);
        }
    }
    rc
}

/// Sai de um mutex obtido antes pela mesma thread. Com `None` não faz nada.
pub fn mutex_leave(p: Option<&sqlite3_mutex>) {
    if let Some(p) = p {
        let x_leave = with_global_config(|cfg| cfg.mutex.x_mutex_leave);
        assert!(x_leave.is_some());
        if let Some(f) = x_leave {
            f(p);
        }
    }
}

/// Usada dentro de assert(): não-zero se `p` é nulo ou o mutex está mantido.
pub fn mutex_held(p: Option<&sqlite3_mutex>) -> i32 {
    match p {
        None => 1,
        Some(p) => {
            let x_held = with_global_config(|cfg| cfg.mutex.x_mutex_held);
            assert!(x_held.is_some());
            match x_held {
                Some(f) => f(p),
                None => 0,
            }
        }
    }
}


// ---- part_001.rs ----

/// Usada dentro de assert(): não-zero se `p` é nulo ou o mutex não está mantido pela thread
/// chamadora.
pub fn mutex_notheld(p: Option<&sqlite3_mutex>) -> i32 {
    match p {
        None => 1,
        Some(p) => {
            let x_notheld = with_global_config(|cfg| cfg.mutex.x_mutex_notheld);
            assert!(x_notheld.is_some());
            match x_notheld {
                Some(f) => f(p),
                None => 0,
            }
        }
    }
}

