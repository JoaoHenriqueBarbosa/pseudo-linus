//! Callbacks de saída do processo (`Global.rs` do upstream: `add_exit_callback` e `run_exit_callbacks`).
//!
//! No upstream o registro passa por `Bun__atexit`, exportado para o C++. Aqui não há C++: quem registra chama
//! `add_exit_callback`, e o ponto de saída do programa chama `run_exit_callbacks` antes de encerrar.

use std::sync::Mutex;

pub type ExitFn = fn();

static ON_EXIT_CALLBACKS: Mutex<Vec<ExitFn>> = Mutex::new(Vec::new());

/// Registra `function` uma vez só; registrar de novo a mesma função não a repete.
pub fn add_exit_callback(function: ExitFn) {
    let mut cbs = ON_EXIT_CALLBACKS.lock().unwrap_or_else(|e| e.into_inner());
    if !cbs.iter().any(|f| *f as usize == function as usize) {
        cbs.push(function);
    }
}

/// Esvazia a lista sob a trava e roda fora dela, porque um callback pode registrar outro.
pub fn run_exit_callbacks() {
    let cbs = core::mem::take(&mut *ON_EXIT_CALLBACKS.lock().unwrap_or_else(|e| e.into_inner()));
    for callback in &cbs {
        callback();
    }
}
