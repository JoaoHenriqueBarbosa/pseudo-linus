//! Candidato: `stats_alloc` 0.1. Seis contadores globais atômicos; sem grupos, só o host inteiro.
//! Entra como medida do custo de um contador global compartilhado por todas as threads.

use std::alloc::System;

use e07_mem_accounting::{Accounting, Capabilities, Pid, cli};
use stats_alloc::{INSTRUMENTED_SYSTEM, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

struct StatsAdapter;

impl Accounting for StatsAdapter {
    fn name(&self) -> &'static str {
        "stats_alloc"
    }

    fn caps(&self) -> Capabilities {
        Capabilities { global_only: true, ..Capabilities::default() }
    }

    fn run_process<R>(&self, _limit: Option<i64>, body: impl FnOnce(Pid) -> R) -> R {
        body(Pid(0))
    }

    fn global_live(&self) -> Option<i64> {
        // O realloc da crate já soma a diferença em bytes_allocated ou bytes_deallocated;
        // bytes_reallocated é só o saldo das realocações, informativo.
        let s = GLOBAL.stats();
        Some(s.bytes_allocated as i64 - s.bytes_deallocated as i64)
    }
}

fn main() {
    cli::run(&StatsAdapter, |_| None);
}
