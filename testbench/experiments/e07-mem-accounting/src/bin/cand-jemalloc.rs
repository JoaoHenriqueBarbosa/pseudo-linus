//! Candidato: jemalloc (`tikv-jemallocator` 0.7) com os contadores por thread do próprio jemalloc
//! (`thread.allocatedp` e `thread.deallocatedp`, via `tikv-jemalloc-ctl` com a feature `stats`).
//!
//! Os contadores são acumulados da thread e já existem dentro do jemalloc, então a contabilidade não
//! acrescenta trabalho no caminho da alocação. O ponteiro devolvido é `!Send` (só a própria thread lê)
//! e a liberação conta pra quem libera. O escopo do kernel é uma camada nossa: lê os contadores antes e
//! depois do código do kernel e desconta a diferença (`kernel_scope` por colchetes).

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

use e07_mem_accounting::{Accounting, Capabilities, Pid, cli};
use tikv_jemalloc_ctl::thread::{ThreadLocal, allocatedp, deallocatedp};

#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

static NEXT_PID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static PTRS: Cell<Option<(ThreadLocal<u64>, ThreadLocal<u64>)>> = const { Cell::new(None) };
    static START: Cell<i64> = const { Cell::new(0) };
    /// Bytes alocados dentro de `kernel_scope` que ainda estão vivos (descontados da conta).
    static KERNEL: Cell<i64> = const { Cell::new(0) };
    static LAST: Cell<Option<i64>> = const { Cell::new(None) };
}

fn raw() -> i64 {
    let (a, d) = PTRS.with(|p| match p.get() {
        Some(x) => x,
        None => {
            let x = (allocatedp::read().expect("thread.allocatedp"), deallocatedp::read().expect("thread.deallocatedp"));
            p.set(Some(x));
            x
        }
    });
    a.get() as i64 - d.get() as i64
}

struct JemallocThread;

impl Accounting for JemallocThread {
    fn name(&self) -> &'static str {
        "jemalloc-thread-counters"
    }

    fn caps(&self) -> Capabilities {
        Capabilities { per_group: true, self_read: true, kernel_scope: true, ..Capabilities::default() }
    }

    fn run_process<R>(&self, _limit: Option<i64>, body: impl FnOnce(Pid) -> R) -> R {
        let start = raw();
        START.with(|c| c.set(start));
        KERNEL.with(|c| c.set(0));
        let out = body(Pid(NEXT_PID.fetch_add(1, Ordering::Relaxed)));
        LAST.with(|c| c.set(self.self_live()));
        out
    }

    fn self_live(&self) -> Option<i64> {
        Some(raw() - START.with(Cell::get) - KERNEL.with(Cell::get))
    }

    fn last_process_net(&self) -> Option<i64> {
        LAST.with(Cell::get)
    }

    fn kernel_scope<R>(&self, f: impl FnOnce() -> R) -> R {
        let before = raw();
        let out = f();
        let delta = raw() - before;
        KERNEL.with(|c| c.set(c.get() + delta));
        out
    }
}

fn main() {
    cli::run(&JemallocThread, |_| None);
}
