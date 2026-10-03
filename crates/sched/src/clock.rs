//! Relógio injetável. O escalonador nunca lê o tempo sozinho: toda operação pública começa com o
//! equivalente do `update_rq_clock`, que pergunta ao [`Clock`]. No kernel do sandbox o relógio é o
//! monotônico do host; no simulador e nos testes é um [`ManualClock`] que só anda quando mandam.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Fonte de tempo em nanossegundos, monotônica.
pub trait Clock {
    /// Instante atual em ns.
    fn now_ns(&self) -> u64;
}

/// Relógio controlado à mão. Clones compartilham o mesmo instante, então o simulador pode guardar um
/// clone e avançar o tempo que a runqueue enxerga.
#[derive(Clone, Debug, Default)]
pub struct ManualClock {
    now: Arc<AtomicU64>,
}

impl ManualClock {
    /// Relógio parado em `start_ns`.
    pub fn new(start_ns: u64) -> ManualClock {
        ManualClock { now: Arc::new(AtomicU64::new(start_ns)) }
    }

    /// Põe o relógio em `ns`.
    pub fn set(&self, ns: u64) {
        self.now.store(ns, Ordering::Relaxed);
    }

    /// Avança `delta_ns` e devolve o instante novo.
    pub fn advance(&self, delta_ns: u64) -> u64 {
        self.now.fetch_add(delta_ns, Ordering::Relaxed) + delta_ns
    }
}

impl Clock for ManualClock {
    fn now_ns(&self) -> u64 {
        self.now.load(Ordering::Relaxed)
    }
}

/// Relógio monotônico do host, contado a partir da criação.
#[derive(Clone, Copy, Debug)]
pub struct MonotonicClock {
    origin: Instant,
}

impl MonotonicClock {
    /// Relógio com origem agora.
    pub fn new() -> MonotonicClock {
        MonotonicClock { origin: Instant::now() }
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        MonotonicClock::new()
    }
}

impl Clock for MonotonicClock {
    fn now_ns(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_is_shared_between_clones() {
        let a = ManualClock::new(10);
        let b = a.clone();
        assert_eq!(b.advance(5), 15);
        assert_eq!(a.now_ns(), 15);
        a.set(100);
        assert_eq!(b.now_ns(), 100);
    }

    #[test]
    fn monotonic_clock_moves_forward() {
        let c = MonotonicClock::new();
        let t0 = c.now_ns();
        let t1 = c.now_ns();
        assert!(t1 >= t0);
    }
}
