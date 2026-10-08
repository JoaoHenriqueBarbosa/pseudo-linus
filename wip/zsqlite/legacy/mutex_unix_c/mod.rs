// Mesclado das partes traduzidas de mutex_unix_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Implementação de mutex para pthreads em Unix (mutex_unix.c). O mutex recursivo do pthread
// vira um estado protegido por `std::sync::Mutex` mais uma `Condvar`, sem ponteiros e sem unsafe.

use std::sync::{Arc, Condvar, Mutex as StdMutex, OnceLock};
use std::thread::ThreadId;

/// Estado interno de um mutex: quem o detém e quantas vezes entrou.
pub struct MutexState {
    /// Número de entradas (nRef)
    pub n_ref: i32,
    /// Thread que está dentro do mutex (owner)
    pub owner: Option<ThreadId>,
}

/// Mutex do SQLite (`sqlite3_mutex`). `id` é o tipo (SQLITE_MUTEX_FAST, RECURSIVE ou estático 2..13).
pub struct PthreadMutex {
    /// Tipo do mutex
    pub id: i32,
    /// Estado protegido
    state: StdMutex<MutexState>,
    /// Acorda quem espera a liberação
    cond: Condvar,
}

impl PthreadMutex {
    /// Cria um mutex livre do tipo informado.
    fn new(id: i32) -> Self {
        PthreadMutex {
            id,
            state: StdMutex::new(MutexState { n_ref: 0, owner: None }),
            cond: Condvar::new(),
        }
    }
}

/// Inicializa o subsistema de mutex.
fn pthread_mutex_init() -> i32 {
    SQLITE_OK
}

/// Encerra o subsistema de mutex.
fn pthread_mutex_end() -> i32 {
    SQLITE_OK
}

/// Só existe com SQLITE_DEBUG: verdadeiro se a thread atual está dentro do mutex.
#[cfg(debug_assertions)]
fn pthread_mutex_held(p: &PthreadMutex) -> bool {
    let state = p.state.lock().unwrap_or_else(|e| e.into_inner());
    state.n_ref != 0 && state.owner == Some(std::thread::current().id())
}

/// Só existe com SQLITE_DEBUG: verdadeiro se a thread atual NÃO está dentro do mutex.
#[cfg(debug_assertions)]
fn pthread_mutex_notheld(p: &PthreadMutex) -> bool {
    let state = p.state.lock().unwrap_or_else(|e| e.into_inner());
    state.n_ref == 0 || state.owner != Some(std::thread::current().id())
}

/// Barreira de memória (`sqlite3MemoryBarrier`, o `__sync_synchronize` do GCC).
pub fn memory_barrier() {
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
}

/// Os doze mutexes estáticos (tipos 2 a 13), criados uma única vez.
fn static_mutexes() -> &'static Vec<Arc<PthreadMutex>> {
    static STATIC_MUTEXES: OnceLock<Vec<Arc<PthreadMutex>>> = OnceLock::new();
    STATIC_MUTEXES.get_or_init(|| (2..=13).map(|id| Arc::new(PthreadMutex::new(id))).collect())
}

/// Aloca um mutex. FAST e RECURSIVE criam um novo a cada chamada; os demais tipos devolvem
/// sempre o mesmo mutex estático. `None` equivale ao NULL do C.
fn pthread_mutex_alloc(i_type: i32) -> Option<Arc<PthreadMutex>> {
    match i_type {
        SQLITE_MUTEX_RECURSIVE => Some(Arc::new(PthreadMutex::new(SQLITE_MUTEX_RECURSIVE))),
        SQLITE_MUTEX_FAST => Some(Arc::new(PthreadMutex::new(SQLITE_MUTEX_FAST))),
        _ => {
            if i_type < 2 {
                return None;
            }
            static_mutexes().get((i_type - 2) as usize).cloned()
        }
    }
}

/// Libera um mutex dinâmico: o dono solta a referência e o `Drop` faz o destroy.
fn pthread_mutex_free(p: Arc<PthreadMutex>) {
    debug_assert_eq!(p.state.lock().unwrap_or_else(|e| e.into_inner()).n_ref, 0);
    drop(p);
}

/// Entra no mutex. O recursivo reentra na mesma thread; os demais bloqueiam até a liberação.
fn pthread_mutex_enter(p: &PthreadMutex) {
    let current_thread = std::thread::current().id();
    let mut state = p.state.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if state.n_ref == 0 {
            state.owner = Some(current_thread);
            state.n_ref = 1;
            return;
        }
        if p.id == SQLITE_MUTEX_RECURSIVE && state.owner == Some(current_thread) {
            state.n_ref += 1;
            return;
        }
        state = p.cond.wait(state).unwrap_or_else(|e| e.into_inner());
    }
}

/// Tenta entrar sem bloquear: SQLITE_OK em caso de sucesso, SQLITE_BUSY se outra thread detém.
fn pthread_mutex_try(p: &PthreadMutex) -> i32 {
    let current_thread = std::thread::current().id();
    let mut state = p.state.lock().unwrap_or_else(|e| e.into_inner());
    if state.n_ref == 0 {
        state.owner = Some(current_thread);
        state.n_ref = 1;
        return SQLITE_OK;
    }
    if p.id == SQLITE_MUTEX_RECURSIVE && state.owner == Some(current_thread) {
        state.n_ref += 1;
        return SQLITE_OK;
    }
    SQLITE_BUSY
}

/// Sai do mutex entrado antes pela mesma thread; ao zerar a contagem acorda um esperando.
fn pthread_mutex_leave(p: &PthreadMutex) {
    let mut state = p.state.lock().unwrap_or_else(|e| e.into_inner());
    debug_assert!(state.n_ref > 0 && state.owner == Some(std::thread::current().id()));
    state.n_ref -= 1;
    if state.n_ref == 0 {
        state.owner = None;
        p.cond.notify_one();
    }
}


// ---- part_001.rs ----

/// Tabela de métodos de mutex padrão (`sqlite3DefaultMutex`). Os métodos held e notheld só
/// existem com SQLITE_DEBUG, como no C; sem ele ficam nulos.
pub fn default_mutex() -> &'static Sqlite3MutexMethods {
    static S_MUTEX: Sqlite3MutexMethods = Sqlite3MutexMethods {
        x_mutex_init: Some(pthread_mutex_init),
        x_mutex_end: Some(pthread_mutex_end),
        x_mutex_alloc: Some(pthread_mutex_alloc),
        x_mutex_free: Some(pthread_mutex_free),
        x_mutex_enter: Some(pthread_mutex_enter),
        x_mutex_try: Some(pthread_mutex_try),
        x_mutex_leave: Some(pthread_mutex_leave),
        #[cfg(debug_assertions)]
        x_mutex_held: Some(pthread_mutex_held),
        #[cfg(not(debug_assertions))]
        x_mutex_held: None,
        #[cfg(debug_assertions)]
        x_mutex_notheld: Some(pthread_mutex_notheld),
        #[cfg(not(debug_assertions))]
        x_mutex_notheld: None,
    };

    &S_MUTEX
}

