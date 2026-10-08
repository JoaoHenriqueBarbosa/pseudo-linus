//! Timers de intervalo do processo (`ITIMER_REAL`, `ITIMER_VIRTUAL`, `ITIMER_PROF`), as regras de
//! `kernel/time/itimer.c`. Cada relógio armado tem uma thread do host que dorme até o prazo, entrega o sinal
//! (`SIGALRM`, `SIGVTALRM`, `SIGPROF`) e dorme de novo se o timer é periódico. O de parede dorme direto até o
//! prazo; os de CPU conferem o tempo de CPU do processo a cada tick (o sandbox não separa tempo de sistema, então
//! `ITIMER_VIRTUAL` e `ITIMER_PROF` contam o mesmo relógio). Um `fork` cria um `Proc` sem timers e o `execve`
//! mantém o `Proc`, então os timers somem no filho e atravessam o exec como no Linux.

use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use sysabi::itimer::TICK_NS;
use sysabi::{Itimer, ItimerSlot};

use crate::park::Parker;
use crate::proc::Proc;
use crate::sys::generate_signal;

/// Teto de uma soneca do relógio de parede: um prazo distante acorda de hora em hora e confere de novo.
const MAX_NAP_NS: u64 = 3_600_000_000_000;

#[derive(Default)]
struct Armed {
    state: ItimerSlot,
    /// A thread do host que espera o prazo; acordá-la a manda embora.
    ticker: Option<Arc<Parker>>,
}

impl Armed {
    fn stop_ticker(&mut self) {
        if let Some(ticker) = self.ticker.take() {
            ticker.unpark();
        }
    }
}

/// Os três relógios de um processo.
#[derive(Default)]
pub(crate) struct Itimers {
    slots: [Armed; 3],
}

impl Itimers {
    /// Desarma tudo e manda as threads embora (o processo terminou).
    pub(crate) fn cancel_all(&mut self) {
        for slot in &mut self.slots {
            slot.state.disarm();
            slot.stop_ticker();
        }
    }
}

impl Drop for Itimers {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

/// O relógio de `which`: nanossegundos de parede desde um marco qualquer, ou a CPU do processo.
fn clock(proc: &Proc, which: Itimer) -> u64 {
    if which.is_cpu() {
        return proc.cpu_ns();
    }
    static BASE: OnceLock<Instant> = OnceLock::new();
    u64::try_from(BASE.get_or_init(Instant::now).elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// `setitimer`: rearma o relógio e devolve o `(restava, intervalo)` de antes, em ns.
pub(crate) fn set(proc: &Arc<Proc>, which: Itimer, value: u64, interval: u64) -> (u64, u64) {
    let now = clock(proc, which);
    let mut timers = proc.itimers.lock();
    let slot = &mut timers.slots[which as usize];
    let old = slot.state.set(which, now, value, interval);
    slot.stop_ticker();
    if slot.state.expiry().is_some() {
        let ticker = Parker::new();
        slot.ticker = Some(ticker.clone());
        spawn_ticker(Arc::downgrade(proc), which, ticker);
    }
    old
}

/// `getitimer`: o `(resta, intervalo)` do relógio, em ns.
pub(crate) fn get(proc: &Proc, which: Itimer) -> (u64, u64) {
    let now = clock(proc, which);
    proc.itimers.lock().slots[which as usize].state.get(which, now)
}

/// A thread que espera o prazo, entrega o sinal e se refaz até o timer ficar desarmado, ser trocado por um
/// novo (o `Parker` deixou de ser o do relógio) ou o processo acabar.
fn spawn_ticker(proc: Weak<Proc>, which: Itimer, ticker: Arc<Parker>) {
    std::thread::spawn(move || {
        loop {
            let nap = {
                let Some(p) = proc.upgrade() else { return };
                let now = clock(&p, which);
                let (fired, left) = {
                    let mut timers = p.itimers.lock();
                    let slot = &mut timers.slots[which as usize];
                    if !slot.ticker.as_ref().is_some_and(|t| Arc::ptr_eq(t, &ticker)) {
                        return;
                    }
                    let fired = slot.state.fire(now);
                    let left = slot.state.expiry().map(|e| e.saturating_sub(now));
                    if left.is_none() {
                        slot.ticker = None;
                    }
                    (fired, left)
                };
                if fired {
                    generate_signal(&p, which.signal());
                }
                let Some(left) = left else { return };
                if p.nthreads() == 0 {
                    return;
                }
                Duration::from_nanos(left.min(if which.is_cpu() { TICK_NS } else { MAX_NAP_NS }))
            };
            if ticker.park_until(Instant::now() + nap) {
                return;
            }
        }
    });
}
