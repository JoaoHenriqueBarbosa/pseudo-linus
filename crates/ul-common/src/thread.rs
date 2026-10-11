//! Threads do mesmo pseudo-processo. Um interpretador recursivo (o `python3`, o `bun`) precisa de uma pilha bem
//! maior que a da thread do processo, e uma thread nova do SO não herda o pseudo-processo: sem ele `open`, stdin e
//! stderr não funcionam. [`run_with_stack`] cria a thread com a pilha pedida, instala nela o processo de quem chamou
//! e espera o trabalho.

use sysabi::sys;

/// Roda `job` numa thread de `stack_bytes` de pilha com o pseudo-processo de quem chamou instalado, e devolve o
/// resultado. Os desvios de controle do kernel (`execve`, `_exit`, morte por sinal) desempilham a thread e seguem até o
/// pseudo-processo, que é quem troca o programa ou termina; um panic de bug (mensagem `&str` ou `String`) vira `None`.
pub fn run_with_stack<T: Send + 'static>(stack_bytes: usize, job: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let current = sys::try_current();
    let spawned = std::thread::Builder::new().stack_size(stack_bytes).spawn(move || {
        if let Some(c) = current {
            sys::install(c);
        }
        job()
    });
    match spawned.map(|h| h.join()) {
        Ok(Ok(value)) => Some(value),
        Ok(Err(payload)) if !payload.is::<&'static str>() && !payload.is::<String>() => std::panic::resume_unwind(payload),
        _ => None,
    }
}
