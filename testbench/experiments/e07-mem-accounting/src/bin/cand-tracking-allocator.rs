//! Candidato principal: `tracking-allocator` 0.4 sobre o System allocator, com a nossa tabela de
//! contadores por grupo (`group_table`). Um grupo por pseudo-processo, entrado na thread dele.
//!
//! `serve-sort-header-only` roda o sort com o rastreamento desligado (`disable_tracking`): o allocator
//! continua pondo o cabeçalho de 8 bytes e fazendo o realloc por alocar e copiar, mas não chama o
//! tracker. A diferença pro `serve-sort` normal é o custo do rastreamento em si.

use e07_mem_accounting::cli;
use e07_mem_accounting::group_table::TrackingAdapter;
use tracking_allocator::AllocationRegistry;

#[global_allocator]
static GLOBAL: tracking_allocator::Allocator<std::alloc::System> = tracking_allocator::Allocator::system();

fn main() {
    TrackingAdapter::install();
    let adapter = TrackingAdapter { name: "tracking-allocator" };
    cli::run(&adapter, |a| match a.cmd.as_str() {
        "serve-sort-header-only" => {
            AllocationRegistry::disable_tracking();
            Some(cli::serve_sort(&adapter, a))
        }
        _ => None,
    });
}
