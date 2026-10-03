//! Candidato: `allocation-counter` 0.8. A própria crate declara o `#[global_allocator]` (por isso só
//! este binário a referencia). Conta numa pilha `thread_local` dentro de `measure(|| ...)` e só entrega
//! o resultado quando o fecho termina: não há leitura no meio do processo nem de fora.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

use e07_mem_accounting::{Accounting, Capabilities, Pid, cli};

static NEXT_PID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static LAST: Cell<Option<i64>> = const { Cell::new(None) };
}

struct AllocationCounter;

impl Accounting for AllocationCounter {
    fn name(&self) -> &'static str {
        "allocation-counter"
    }

    fn caps(&self) -> Capabilities {
        Capabilities { per_group: true, ..Capabilities::default() }
    }

    fn run_process<R>(&self, _limit: Option<i64>, body: impl FnOnce(Pid) -> R) -> R {
        let pid = Pid(NEXT_PID.fetch_add(1, Ordering::Relaxed));
        let mut out = None;
        let info = allocation_counter::measure(|| out = Some(body(pid)));
        LAST.with(|c| c.set(Some(info.bytes_current)));
        out.expect("o processo terminou sem resultado")
    }

    fn last_process_net(&self) -> Option<i64> {
        LAST.with(Cell::get)
    }
}

fn main() {
    cli::run(&AllocationCounter, |_| None);
}
