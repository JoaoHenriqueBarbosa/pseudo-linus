//! Caminhos do experimento.

use std::path::PathBuf;

/// `testbench/experiments/e06-isolation`.
pub fn experiment_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Manifesto do workspace das crates-sonda.
pub fn probes_manifest() -> PathBuf {
    experiment_dir().join("probes").join("Cargo.toml")
}

/// Target das sondas, separado do target do experimento (o lock de build de um não trava o outro).
pub fn probes_target_dir() -> PathBuf {
    experiment_dir().join("target").join("probes")
}

/// Rascunho do E06 (`testbench/scratch/e06`, gitignored). Todo arquivo de teste fica aqui dentro.
pub fn scratch() -> PathBuf {
    harness::paths::scratch_dir("e06")
}
