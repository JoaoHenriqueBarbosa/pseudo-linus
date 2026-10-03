//! H19: `disallowed_methods` garante isolamento em tempo de compilação?
//!
//! O crate `userland` (sonda) tem o `clippy.toml` de isolamento e só chama dependências que fazem I/O
//! de host por dentro (`fs-reader`, sonda nossa, e `walkdir`, do crates.io). O `userland-direct` toca o
//! host direto, um módulo por forma de chamada. Os dois passam pelo `cargo clippy -D warnings`, e
//! depois pelo depscan.

use std::collections::BTreeMap;

use anyhow::Result;
use harness::Verdict;
use serde::{Deserialize, Serialize};

use crate::cargo_probe::{self, CargoRun};
use crate::layout;

/// Módulos do `userland-direct` e se cada um é controle (o lint precisa pegar).
pub const DIRECT_MODULES: &[(&str, bool, &str)] = &[
    ("direct_fs", true, "std::fs::read chamado direto"),
    ("direct_process", true, "std::process::Command e std::process::exit"),
    ("direct_net", true, "std::net::TcpStream::connect"),
    ("direct_env", true, "std::env::var"),
    ("direct_stdout", true, "std::io::stdout"),
    ("direct_print_macro", true, "println! (só disallowed-macros pega)"),
    ("file_type_path", true, "std::fs::File::open (tipo proibido)"),
    ("fn_pointer", true, "std::fs::read como ponteiro de função"),
    ("handle_from_dep", false, "File do host vindo de dependência, lido pelo trait Read"),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModuleResult {
    pub module: String,
    pub description: String,
    pub control: bool,
    /// Lints `clippy::disallowed_*` que dispararam no arquivo do módulo.
    pub lints: Vec<String>,
    pub caught: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DepscanSummary {
    pub package: String,
    pub root_host_touch: usize,
    pub root_unsafe: usize,
    pub tree_category: String,
    pub host_touching_deps: BTreeMap<String, usize>,
}

impl DepscanSummary {
    fn of(scan: &depscan::TreeScan) -> DepscanSummary {
        DepscanSummary {
            package: format!("{} {}", scan.root.name, scan.root.version),
            root_host_touch: scan.root.counts.host_touch(),
            root_unsafe: scan.root.counts.unsafe_total(),
            tree_category: scan.tree_category.letter().to_string(),
            host_touching_deps: scan.host_touching_deps.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct H19Outcome {
    pub indirect: CargoRun,
    pub indirect_disallowed: usize,
    pub direct: CargoRun,
    pub direct_modules: Vec<ModuleResult>,
    pub depscan_indirect: DepscanSummary,
    pub depscan_direct: DepscanSummary,
    pub verdict: Verdict,
    pub summary: String,
}

fn clippy_args(package: &str) -> Vec<String> {
    let mut args = cargo_probe::probe_args("clippy", package, None);
    args.extend(["--".to_string(), "-D".to_string(), "warnings".to_string()]);
    args
}

fn is_disallowed(code: Option<&str>) -> bool {
    code.is_some_and(|c| c.starts_with("clippy::disallowed_"))
}

pub fn run() -> Result<H19Outcome> {
    let indirect = cargo_probe::run(&clippy_args("userland"))?;
    let indirect_disallowed = indirect.diagnostics_for("userland").filter(|d| is_disallowed(d.code.as_deref())).count();

    let direct = cargo_probe::run(&clippy_args("userland-direct"))?;
    let mut direct_modules = Vec::new();
    for &(module, control, description) in DIRECT_MODULES {
        let suffix = format!("src/{module}.rs");
        let mut lints: Vec<String> = direct
            .diagnostics_for("userland_direct")
            .filter(|d| is_disallowed(d.code.as_deref()) && d.file.as_deref().is_some_and(|f| f.ends_with(&suffix)))
            .filter_map(|d| d.code.clone())
            .collect();
        lints.sort();
        lints.dedup();
        direct_modules.push(ModuleResult {
            module: module.to_string(),
            description: description.to_string(),
            control,
            caught: !lints.is_empty(),
            lints,
        });
    }

    let manifest = layout::probes_manifest();
    let depscan_indirect = DepscanSummary::of(&depscan::scan(&manifest, "userland")?);
    let depscan_direct = DepscanSummary::of(&depscan::scan(&manifest, "userland-direct")?);

    let fs_control = direct_modules.iter().find(|m| m.module == "direct_fs").is_some_and(|m| m.caught);
    let controls_caught = direct_modules.iter().filter(|m| m.control && m.caught).count();
    let controls_total = direct_modules.iter().filter(|m| m.control).count();
    let handle_from_dep_caught = direct_modules.iter().find(|m| m.module == "handle_from_dep").is_some_and(|m| m.caught);
    let deps_flagged: Vec<String> = depscan_indirect.host_touching_deps.keys().cloned().collect();
    let indirect_clean = indirect.success() && indirect_disallowed == 0;

    let compile_errors: Vec<String> = indirect
        .diagnostics
        .iter()
        .chain(direct.diagnostics.iter())
        .filter(|d| d.level == "error" && !d.code.as_deref().is_some_and(|c| c.starts_with("clippy::")))
        .map(|d| format!("{} {}:{}", d.message, d.file.clone().unwrap_or_default(), d.line.unwrap_or(0)))
        .collect();

    let (verdict, summary) = if !compile_errors.is_empty() {
        (Verdict::Inconclusive, format!("Sonda quebrada, erro de compilação: {}", compile_errors.join("; ")))
    } else if !fs_control {
        (
            Verdict::Inconclusive,
            "O controle falhou: o clippy não pegou nem o std::fs::read direto, então a configuração não está ativa.".to_string(),
        )
    } else if indirect_clean {
        (
            Verdict::Refuted,
            format!(
                "O cargo clippy -D warnings passou limpo (exit 0, 0 diagnósticos disallowed) no crate que lê o host \
                 por fs-reader e walkdir, e pegou {controls_caught} de {controls_total} formas de chamada direta; \
                 o handle de arquivo vindo de dependência {}. O depscan aponta o I/O de host nas dependências: {}.",
                if handle_from_dep_caught { "foi pego" } else { "também passou" },
                deps_flagged.join(", ")
            ),
        )
    } else {
        (
            Verdict::Confirmed,
            format!("O clippy reprovou o crate que só usa dependências ({indirect_disallowed} diagnósticos disallowed)."),
        )
    };

    Ok(H19Outcome {
        indirect,
        indirect_disallowed,
        direct,
        direct_modules,
        depscan_indirect,
        depscan_direct,
        verdict,
        summary,
    })
}
