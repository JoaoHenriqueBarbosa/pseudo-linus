//! Variante: `tracking-allocator` 0.4 embrulhando o mimalloc em vez do System. Mesma semântica de
//! grupos; mede se um allocator interno mais rápido paga o custo da contabilidade.
//! `serve-sort-header-only`: ver `cand-tracking-allocator.rs`.

use e07_mem_accounting::cli;
use e07_mem_accounting::group_table::TrackingAdapter;
use tracking_allocator::AllocationRegistry;

#[global_allocator]
static GLOBAL: tracking_allocator::Allocator<mimalloc::MiMalloc> =
    tracking_allocator::Allocator::from_allocator(mimalloc::MiMalloc);

fn main() {
    TrackingAdapter::install();
    let adapter = TrackingAdapter { name: "tracking-allocator+mimalloc" };
    cli::run(&adapter, |a| match a.cmd.as_str() {
        "serve-sort-header-only" => {
            AllocationRegistry::disable_tracking();
            Some(cli::serve_sort(&adapter, a))
        }
        _ => None,
    });
}
