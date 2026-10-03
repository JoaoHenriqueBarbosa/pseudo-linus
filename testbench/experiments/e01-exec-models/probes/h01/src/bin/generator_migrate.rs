//! `generator` 0.8 aceita migrar a corrotina sem unsafe nosso (ele tem `unsafe impl Send` próprio).
//! Esta sonda compila e roda: a corrotina pega uma referência ao thread-local da thread 1, suspende,
//! migra pra thread 2 e continua usando a mesma referência. Imprime os endereços em JSON:
//!
//! - `held_on_thread2`: a referência pega antes da migração, usada depois dela;
//! - `inline_access_on_thread2`: um acesso "novo" ao thread-local, feito dentro da corrotina depois da
//!   migração com a função de acesso inlinada (o LLVM trata o endereço de thread-local como constante
//!   dentro da função e pode reaproveitar o da thread 1);
//! - `noinline_access_on_thread2`: o mesmo acesso por uma função que não é inlinada;
//! - `thread2_tls`: o endereço real do thread-local da thread 2, lido fora da corrotina.
//!
//! Se a corrotina usa o endereço da thread 1 rodando na thread 2, um `Cell` (que é `!Sync`) fica
//! acessível por duas threads: é a falha de soundness do issue #58.
#![forbid(unsafe_code)]

use std::cell::Cell;

use generator::Gn;

thread_local! {
    static SLOT: Cell<u64> = const { Cell::new(0) };
}

#[inline(always)]
fn slot_addr_inline() -> usize {
    SLOT.with(|c| c as *const Cell<u64> as usize)
}

#[inline(never)]
fn slot_addr_noinline() -> usize {
    SLOT.with(|c| c as *const Cell<u64> as usize)
}

fn thread_name() -> String {
    format!("{:?}", std::thread::current().id())
}

fn main() {
    let thread1_tls = slot_addr_noinline();
    let thread1 = thread_name();
    let mut g = Gn::<()>::new_scoped(|mut s| {
        SLOT.with(|c| {
            let held = c as *const Cell<u64> as usize;
            s.yield_((held, 0, 0, thread_name()));
            // Daqui pra frente a corrotina está rodando na thread 2, ainda segurando `c`.
            let held_after = c as *const Cell<u64> as usize;
            s.yield_((held_after, slot_addr_inline(), slot_addr_noinline(), thread_name()));
        });
        (0, 0, 0, String::new())
    });
    let (held1, _, _, inside1) = g.resume().expect("primeiro yield");
    let (thread2_tls, thread2, (held2, inline2, noinline2, inside2)) = std::thread::spawn(move || {
        let real = slot_addr_noinline();
        let r = g.resume().expect("segundo yield");
        let _ = g.resume();
        (real, thread_name(), r)
    })
    .join()
    .expect("thread 2");
    println!(
        "{{\"thread1\":\"{thread1}\",\"thread1_tls\":{thread1_tls},\"held_on_thread1\":{held1},\
         \"coroutine_thread_before\":\"{inside1}\",\"thread2\":\"{thread2}\",\"thread2_tls\":{thread2_tls},\
         \"coroutine_thread_after\":\"{inside2}\",\"held_on_thread2\":{held2},\
         \"inline_access_on_thread2\":{inline2},\"noinline_access_on_thread2\":{noinline2}}}"
    );
}
