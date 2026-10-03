//! Candidato: `jqf-resource` 0.1 (filip-41/jqf). O `CountingAlloc` cobra cada alocação da conta
//! (`RequestAccount`) instalada na thread corrente, recusa acima do teto e, pra não abortar de cara,
//! serve o primeiro estouro de um slab de emergência de 1 MiB e marca a conta; o checkpoint
//! (`cooperative_refusal`) transforma a marca em erro tipado.
//!
//! A conta é `!Send` e mora na thread: só o próprio processo lê (não há leitura de fora), e a liberação
//! é descontada da conta de quem libera (com piso em zero), não de quem alocou.

use std::alloc::System;
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

use e07_mem_accounting::{Accounting, Capabilities, Pid, cli, demos};
use jqf_resource::{CountingAlloc, RequestAccount, ResourceLimits};

#[global_allocator]
static GLOBAL: CountingAlloc<System> = CountingAlloc(System);

static NEXT_PID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static START: Cell<i64> = const { Cell::new(0) };
    static LAST: Cell<Option<i64>> = const { Cell::new(None) };
}

fn raw() -> Option<i64> {
    jqf_resource::with_account(|a| a.snapshot().memory_current_bytes() as i64)
}

struct Jqf;

impl Accounting for Jqf {
    fn name(&self) -> &'static str {
        "jqf-resource"
    }

    fn caps(&self) -> Capabilities {
        Capabilities {
            per_group: true,
            self_read: true,
            remote_read: false,
            kernel_scope: false,
            limit_flag: true,
            hard_limit: true,
            global_only: false,
        }
    }

    fn run_process<R>(&self, limit: Option<i64>, body: impl FnOnce(Pid) -> R) -> R {
        let ceiling = limit.map_or(u64::MAX, |l| l as u64 + RequestAccount::minimum_memory_bytes());
        let account = RequestAccount::try_new(ResourceLimits::new(u64::MAX, u64::MAX, ceiling, u64::MAX, u32::MAX))
            .expect("conta do processo");
        let guard = jqf_resource::install(account);
        let start = raw().unwrap_or(0);
        START.with(|c| c.set(start));
        let out = body(Pid(NEXT_PID.fetch_add(1, Ordering::Relaxed)));
        LAST.with(|c| c.set(raw().map(|v| v - start)));
        drop(guard);
        out
    }

    fn self_live(&self) -> Option<i64> {
        raw().map(|v| v - START.with(Cell::get))
    }

    fn last_process_net(&self) -> Option<i64> {
        LAST.with(Cell::get)
    }

    fn over_limit(&self, _pid: Pid) -> Option<bool> {
        Some(jqf_resource::tripped())
    }
}

/// Enche a conta até o teto de 32 MiB e então pede `ask` bytes. Até o tamanho do slab (1 MiB) o pedido
/// é servido e o checkpoint recusa com erro tipado; acima dele o allocator devolve nulo e o host aborta.
fn past_ceiling(ask: usize) -> serde_json::Value {
    demos::with_neighbors(move || {
        Jqf.run_process(Some(32 << 20), |_| {
            let mut held: Vec<Vec<u8>> = Vec::with_capacity(256);
            while !jqf_resource::tripped() {
                held.push(vec![1u8; 256 << 10]);
            }
            eprintln!("e07-demo: teto de 32 MiB estourado (marca ligada); pedindo {ask} bytes");
            let extra = vec![2u8; ask];
            let refusal = jqf_resource::cooperative_refusal();
            drop(extra);
            drop(held);
            match refusal {
                Ok(()) => Ok("o checkpoint não recusou".to_string()),
                Err(e) => Err(format!("checkpoint recusou: {e}")),
            }
        })
    })
}

fn main() {
    cli::run(&Jqf, |a| match a.cmd.as_str() {
        "past-ceiling" => Some(past_ceiling(a.get("ask", 4096))),
        "probe" => Some(serde_json::json!({"ceiling_enforced": jqf_resource::ceiling_enforced()})),
        _ => None,
    });
}
