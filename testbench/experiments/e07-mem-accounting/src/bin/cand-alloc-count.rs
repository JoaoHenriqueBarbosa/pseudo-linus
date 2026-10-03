//! Candidato: `alloc_count` 0.4. Contadores em `thread_local` (`Cell<AllocStats>`) mais seis
//! contadores globais atômicos atualizados a cada evento. Só a própria thread lê os números dela; a
//! liberação conta pra quem libera. O escopo de exclusão existe (`IgnoreGuard`), mas é API escondida
//! da documentação (`#[doc(hidden)]`).

use std::alloc::System;
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

use alloc_count::{AllocCounter, IgnoreGuard};
use e07_mem_accounting::{Accounting, Capabilities, Pid, cli};

#[global_allocator]
static GLOBAL: AllocCounter<System> = AllocCounter(System);

static NEXT_PID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static START: Cell<i64> = const { Cell::new(0) };
    static LAST: Cell<Option<i64>> = const { Cell::new(None) };
}

fn raw() -> i64 {
    alloc_count::stats().net_bytes() as i64
}

struct AllocCount;

impl Accounting for AllocCount {
    fn name(&self) -> &'static str {
        "alloc_count"
    }

    fn caps(&self) -> Capabilities {
        Capabilities { per_group: true, self_read: true, kernel_scope: true, ..Capabilities::default() }
    }

    fn run_process<R>(&self, _limit: Option<i64>, body: impl FnOnce(Pid) -> R) -> R {
        let start = raw();
        START.with(|c| c.set(start));
        let out = body(Pid(NEXT_PID.fetch_add(1, Ordering::Relaxed)));
        LAST.with(|c| c.set(Some(raw() - start)));
        out
    }

    fn self_live(&self) -> Option<i64> {
        Some(raw() - START.with(Cell::get))
    }

    fn global_live(&self) -> Option<i64> {
        Some(alloc_count::global_stats().net_bytes() as i64)
    }

    fn last_process_net(&self) -> Option<i64> {
        LAST.with(Cell::get)
    }

    fn kernel_scope<R>(&self, f: impl FnOnce() -> R) -> R {
        let _g = IgnoreGuard::new();
        f()
    }
}

fn main() {
    cli::run(&AllocCount, |_| None);
}
