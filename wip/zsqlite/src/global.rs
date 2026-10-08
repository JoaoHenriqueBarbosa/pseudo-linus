//! Estado global do SQLite (global.c, random.c global, `sqlite3_log`): tudo atrás de `Mutex`
//! seguro, sem `static mut`.
//!
//! O PRNG do processo é inicializado na primeira chamada de [`randomness`] com 44 bytes de
//! `xRandomness` do VFS padrão, como o `sqlite3_randomness` do C. `sqlite3_randomness(0, 0)`
//! vira [`randomness_reset`], que força a reinicialização na chamada seguinte.

use crate::os::{os_randomness, vfs_find};
use crate::printf::{render_log_msg, PrintfArg};
use crate::random::Prng;
use std::sync::Mutex;

/// Gancho de log do `SQLITE_CONFIG_LOG`: recebe o código e a mensagem já formatada.
pub type LogHook = Box<dyn Fn(i32, &[u8]) + Send>;

static PRNG: Mutex<Option<Prng>> = Mutex::new(None);
static LOGGER: Mutex<Option<LogHook>> = Mutex::new(None);
static PRNG_SEED: Mutex<u32> = Mutex::new(0);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// `sqlite3_randomness(N, pBuf)` com N > 0.
pub fn randomness(out: &mut [u8]) {
    if out.is_empty() {
        randomness_reset();
        return;
    }
    let mut guard = lock(&PRNG);
    if guard.is_none() {
        // Semente: 44 bytes do `xRandomness` do VFS padrão (zero se não houver VFS).
        let mut seed = [0u8; 256];
        if let Some(vfs) = vfs_find(None) {
            let prng_seed = *lock(&PRNG_SEED);
            os_randomness(&*vfs, &mut seed[..44], prng_seed);
        }
        *guard = Some(Prng::new(&seed));
    }
    if let Some(p) = guard.as_mut() {
        p.randomness(out);
    }
}

/// `sqlite3_randomness(0, 0)`: descarta o estado; a próxima chamada reinicializa com bytes novos.
pub fn randomness_reset() {
    *lock(&PRNG) = None;
}

/// `sqlite3GlobalConfig.iPrngSeed` (`SQLITE_CONFIG_PRNG_SEED`).
pub fn set_prng_seed(seed: u32) {
    *lock(&PRNG_SEED) = seed;
}

/// `sqlite3_config(SQLITE_CONFIG_LOG, ...)`.
pub fn set_logger(hook: Option<LogHook>) {
    *lock(&LOGGER) = hook;
}

/// `sqlite3_log`: formata a mensagem e a entrega ao gancho, se houver (sem gancho não faz nada).
pub fn log(code: i32, fmt: &[u8], args: &[PrintfArg]) {
    let guard = lock(&LOGGER);
    if let Some(hook) = guard.as_ref() {
        let msg = render_log_msg(fmt, args);
        hook(code, &msg);
    }
}
