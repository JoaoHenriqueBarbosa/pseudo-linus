//! Desenrolamento do pseudo-linus atravessando o SQLite.
//!
//! `sys::exit` e a morte por sinal terminam o pseudo-processo desenrolando a pilha (payloads
//! `ExitUnwind` e `KillUnwind`), e qualquer syscall pode levantar o segundo quando há sinal fatal
//! pendente. Os callbacks que o SQLite chama (VFS do sqlite-plugin, funções SQL, busy handler,
//! progress handler, authorizer) rodam dentro de quadros C: um unwind que chegasse até `extern "C"`
//! abortaria o processo host inteiro, e o rusqlite, onde captura, descarta o payload.
//!
//! A regra é: todo callback que pode fazer syscall roda dentro de [`guard`], que captura o payload,
//! guarda na thread e devolve `None` (o callback então devolve erro pro SQLite). Assim que o controle
//! volta pro Rust, o CLI chama [`reraise`], que relança o payload guardado com `resume_unwind`, e o
//! pseudo-processo termina como deveria.

use std::any::Any;
use std::cell::RefCell;
use std::panic::{self, AssertUnwindSafe};

thread_local! {
    static PENDING: RefCell<Option<Box<dyn Any + Send>>> = const { RefCell::new(None) };
}

/// Roda `f` num callback chamado pelo C. Se `f` desenrolar, o primeiro payload fica guardado e o
/// resultado é `None`.
pub fn guard<R>(f: impl FnOnce() -> R) -> Option<R> {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => Some(r),
        Err(payload) => {
            PENDING.with(|p| {
                let mut p = p.borrow_mut();
                if p.is_none() {
                    *p = Some(payload);
                }
            });
            None
        }
    }
}

/// `true` quando há um unwind guardado esperando pra ser relançado.
pub fn pending() -> bool {
    PENDING.with(|p| p.borrow().is_some())
}

/// Relança o unwind guardado, se houver.
pub fn reraise() {
    if let Some(payload) = PENDING.with(|p| p.borrow_mut().take()) {
        panic::resume_unwind(payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_keeps_first_payload_and_reraises() {
        assert_eq!(guard(|| 7), Some(7));
        assert!(guard(|| panic::resume_unwind(Box::new(1u8))).is_none());
        assert!(guard(|| panic::resume_unwind(Box::new(2u8))).is_none());
        assert!(pending());
        let r = panic::catch_unwind(reraise);
        let payload = r.expect_err("relança");
        assert_eq!(payload.downcast_ref::<u8>(), Some(&1));
        assert!(!pending());
    }
}
