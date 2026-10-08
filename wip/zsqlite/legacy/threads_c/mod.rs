// Mesclado das partes traduzidas de threads_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// ================================ Unix Pthreads ================================
// Só o ramo pthreads existe (SQLITE_THREADSAFE=1 no Debian), então o ramo
// single-threaded do C (guardado por #ifndef SQLITE_THREADS_IMPLEMENTED) e o
// ramo Win32 não são traduzidos.

/// Uma thread em execução
pub struct SQLiteThread {
    /// Identificador da thread (o JoinHandle faz o papel do pthread_t)
    pub tid: Option<std::thread::JoinHandle<usize>>,
    /// Definida como verdadeira quando a thread termina
    pub done: bool,
    /// Resultado retornado pela thread
    pub p_out: usize,
    /// A rotina de thread a executar
    pub x_task: Option<fn(usize) -> usize>,
    /// Argumento passado para a thread
    pub p_in: usize,
}

/// Cria uma nova thread.
///
/// `pp_thread` recebe o objeto de thread criado. `x_task` é a rotina a executar
/// em uma thread separada e `p_in` é o argumento passado para ela.
pub fn thread_create(
    pp_thread: &mut Option<Box<SQLiteThread>>,
    x_task: fn(usize) -> usize,
    p_in: usize,
) -> i32 {
    // Esta rotina nunca é usada em modo single-threaded
    assert!(GLOBAL_CONFIG.b_core_mutex != 0);

    *pp_thread = None;
    let mut p = Box::new(SQLiteThread {
        tid: None,
        done: false,
        p_out: 0,
        x_task: Some(x_task),
        p_in,
    });
    // Se o callback SQLITE_TESTCTRL_FAULT_INSTALL está registrado para uma
    // função que retorna SQLITE_ERROR quando recebe o argumento 200, isso força
    // as threads de trabalho a rodarem em sequência e de forma determinística,
    // para fins de teste.
    let rc = if fault_sim(200) != 0 {
        1
    } else {
        match std::thread::Builder::new().spawn(move || x_task(p_in)) {
            Ok(handle) => {
                p.tid = Some(handle);
                0
            }
            Err(_) => 1,
        }
    };
    if rc != 0 {
        p.done = true;
        p.p_out = x_task(p_in);
    }
    *pp_thread = Some(p);
    SQLITE_OK
}

/// Obtém o resultado da thread e libera o objeto.
pub fn thread_join(p: Option<Box<SQLiteThread>>, pp_out: &mut usize) -> i32 {
    let mut p = match p {
        Some(p) => p,
        None => return SQLITE_NOMEM,
    };
    if p.done {
        *pp_out = p.p_out;
        SQLITE_OK
    } else {
        match p.tid.take().map(|handle| handle.join()) {
            Some(Ok(result)) => {
                *pp_out = result;
                SQLITE_OK
            }
            _ => SQLITE_ERROR,
        }
    }
}

