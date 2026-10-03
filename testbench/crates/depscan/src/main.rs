//! `cargo run -p depscan -- --manifest-path experiments/f05-jq/Cargo.toml --package jaq-core`

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
struct Cli {
    /// Manifesto em cuja árvore a crate aparece.
    #[arg(long, default_value = "Cargo.toml")]
    manifest_path: PathBuf,
    /// Nome da crate a varrer.
    #[arg(long)]
    package: String,
    /// Saída JSON completa em vez do resumo.
    #[arg(long)]
    json: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let scan = depscan::scan(&cli.manifest_path, &cli.package)?;
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&scan)?);
        return Ok(());
    }
    let r = &scan.root;
    println!("{} {}: categoria própria ({}), árvore ({})",r.name, r.version, r.category.letter(), scan.tree_category.letter());
    println!("  próprio: host={} unsafe={} arquivos={} erros_parse={}", r.counts.host_touch(), r.counts.unsafe_total(), r.counts.files, r.counts.parse_errors);
    println!("  árvore: {} deps, host={} unsafe={}", scan.deps.len(), scan.totals.host_touch(), scan.totals.unsafe_total());
    if !scan.c_deps.is_empty() {
        println!("  deps com C: {}", scan.c_deps.join(", "));
    }
    for (dep, n) in &scan.host_touching_deps {
        println!("  toca o host: {dep} ({n})");
    }
    Ok(())
}
