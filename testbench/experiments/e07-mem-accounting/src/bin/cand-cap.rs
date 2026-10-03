//! Candidato: `cap` 0.1. Um contador global atômico e um teto global: acima do teto o allocator
//! devolve nulo. É o candidato da demonstração do limite duro (o nulo vira `handle_alloc_error`, que
//! aborta o host inteiro). Não tem grupos: não serve pra contar por processo.

use std::alloc::System;

use cap::Cap;
use e07_mem_accounting::{Accounting, Capabilities, Pid, cli, demos};

#[global_allocator]
static GLOBAL: Cap<System> = Cap::new(System, usize::MAX);

struct CapAdapter;

impl Accounting for CapAdapter {
    fn name(&self) -> &'static str {
        "cap"
    }

    fn caps(&self) -> Capabilities {
        Capabilities { hard_limit: true, global_only: true, ..Capabilities::default() }
    }

    fn run_process<R>(&self, _limit: Option<i64>, body: impl FnOnce(Pid) -> R) -> R {
        body(Pid(0))
    }

    fn global_live(&self) -> Option<i64> {
        Some(GLOBAL.allocated() as i64)
    }
}

fn arm(limit: usize) {
    // `set_limit` só falha se o teto novo ficar abaixo do que já está alocado.
    let _ = GLOBAL.set_limit(limit);
}

fn main() {
    cli::run(&CapAdapter, |a| match a.cmd.as_str() {
        "hard-limit" => Some(demos::hard_limit(arm, || GLOBAL.allocated(), false)),
        "hard-limit-try" => Some(demos::hard_limit(arm, || GLOBAL.allocated(), true)),
        _ => None,
    });
}
