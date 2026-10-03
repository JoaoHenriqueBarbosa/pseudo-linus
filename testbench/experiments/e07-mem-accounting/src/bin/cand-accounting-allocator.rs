//! Candidato: `accounting-allocator` 0.2. Contadores por thread (um `Arc` por thread registrado num
//! canal), mas a API só devolve o agregado do host (`count()`); o detalhe por thread só aparece no
//! `Display`, sem identificar a thread. Liberação conta pra quem libera.

use e07_mem_accounting::{Accounting, Capabilities, Pid, cli};

use accounting_allocator::AccountingAlloc;

#[global_allocator]
static GLOBAL: AccountingAlloc = AccountingAlloc::new();

struct AccountingAdapter;

impl Accounting for AccountingAdapter {
    fn name(&self) -> &'static str {
        "accounting-allocator"
    }

    fn caps(&self) -> Capabilities {
        Capabilities { global_only: true, ..Capabilities::default() }
    }

    fn run_process<R>(&self, _limit: Option<i64>, body: impl FnOnce(Pid) -> R) -> R {
        body(Pid(0))
    }

    fn global_live(&self) -> Option<i64> {
        let c = GLOBAL.count().all_time;
        Some(c.alloc as i64 - c.dealloc as i64)
    }
}

fn main() {
    cli::run(&AccountingAdapter, |_| None);
}
