//! bc: implementações Rust que não existem como biblioteca no crates.io. O experimento clona cada
//! uma numa revisão fixa em `corpus/upstream/bc/` (gitignored), compila o binário com cargo num target
//! próprio e roda contra o golden do GNU bc 1.07.1 via [`crate::common::run_external`]. O esforço de
//! fork é medido no fonte: depscan do pacote, linhas e pontos de host do motor separado do CLI.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use harness::{Candidate, Invocation, Outcome};
use serde::Serialize;

/// Um projeto upstream com um binário `bc`.
pub struct Upstream {
    pub label: &'static str,
    pub repo: &'static str,
    pub rev: &'static str,
    pub dir: &'static str,
    pub package: &'static str,
    pub bin: &'static str,
    /// Arquivos do motor (relativos à raiz do clone): o que um fork copiaria.
    pub engine_files: &'static [&'static str],
    /// Arquivos do CLI deles (o que seria substituído pelo nosso front-end).
    pub cli_files: &'static [&'static str],
    /// Pacote que contém só o motor, quando existe (depscan).
    pub engine_package: Option<&'static str>,
}

pub const POSIXUTILS: Upstream = Upstream {
    label: "posixutils-rs bc",
    repo: "https://github.com/rustcoreutils/posixutils-rs",
    rev: "4073af047104a44442f44047c76c1167f43a8c67",
    dir: "posixutils-rs",
    package: "posixutils-calc",
    bin: "bc",
    engine_files: &[
        "calc/bc_util/instructions.rs",
        "calc/bc_util/interpreter.rs",
        "calc/bc_util/lexer.rs",
        "calc/bc_util/mod.rs",
        "calc/bc_util/number.rs",
        "calc/bc_util/output.rs",
        "calc/bc_util/parser.rs",
    ],
    cli_files: &["calc/bc.rs"],
    engine_package: None,
};

pub const BC_CLONE: Upstream = Upstream {
    label: "bc_clone_rs",
    repo: "https://github.com/takayuki-nagata/bc_clone_rs",
    rev: "d0e3f445bfa0a412879b7600b094934beaad85a0",
    dir: "bc_clone_rs",
    package: "bc_clone",
    bin: "bc_clone",
    engine_files: &[
        "crates/bc_core/src/lib.rs",
        "crates/bc_core/src/eval.rs",
        "crates/bc_core/src/math.rs",
        "crates/bc_core/src/parser.rs",
    ],
    cli_files: &["crates/bc_cli/src/main.rs"],
    engine_package: Some("bc_core"),
};

pub fn upstream_root() -> PathBuf {
    harness::paths::corpus_dir().join("upstream").join("bc")
}

fn run_checked(cmd: &mut Command, what: &str) -> Result<String> {
    let out = cmd.output().with_context(|| what.to_string())?;
    if !out.status.success() {
        bail!("{what} falhou: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Garante o clone na revisão fixa e devolve o diretório.
pub fn ensure_checkout(u: &Upstream) -> Result<PathBuf> {
    let root = upstream_root();
    std::fs::create_dir_all(&root)?;
    let dir = root.join(u.dir);
    if !dir.join(".git").exists() {
        run_checked(Command::new("git").args(["clone", "--quiet", u.repo]).arg(&dir), "git clone")?;
    }
    let head = run_checked(Command::new("git").arg("-C").arg(&dir).args(["rev-parse", "HEAD"]), "git rev-parse")?;
    if head != u.rev {
        let _ = Command::new("git").arg("-C").arg(&dir).args(["fetch", "--quiet", "origin"]).status();
        run_checked(
            Command::new("git").arg("-C").arg(&dir).args(["checkout", "--quiet", "--detach", u.rev]),
            "git checkout",
        )?;
    }
    Ok(dir)
}

/// Compila o binário (no-op quando já está em dia) num target fora do experimento.
pub fn ensure_built(u: &Upstream, dir: &Path) -> Result<PathBuf> {
    let target = upstream_root().join("target");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    run_checked(
        Command::new(cargo)
            .args(["build", "--release", "--quiet", "-p", u.package, "--bin", u.bin, "--manifest-path"])
            .arg(dir.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(&target)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS"),
        &format!("cargo build {}", u.package),
    )?;
    let bin = target.join("release").join(u.bin);
    if !bin.exists() {
        bail!("binário {} não apareceu", bin.display());
    }
    Ok(bin)
}

/// Um `bc` externo: troca o `argv[0]` do caso pelo binário compilado.
pub struct ExternalBc {
    pub name: String,
    pub path: PathBuf,
}

impl Candidate for ExternalBc {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        if inv.script.is_some() {
            return Outcome::unsupported("caso script");
        }
        crate::common::run_external(&self.path, inv)
    }
}

/// Medidas de fonte de um grupo de arquivos (motor ou CLI).
#[derive(Clone, Debug, Default, Serialize)]
pub struct SourceStats {
    pub files: usize,
    pub lines: usize,
    /// Linhas dentro de `#[cfg(test)] mod tests` (estimativa: do marcador até o fim do arquivo).
    pub test_lines: usize,
    pub host_touch: usize,
    pub unsafe_sites: usize,
    pub panic_sites: usize,
    pub unwrap_sites: usize,
}

pub fn source_stats(root: &Path, files: &[&str]) -> SourceStats {
    let mut s = SourceStats::default();
    for f in files {
        let path = root.join(f);
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        s.files += 1;
        let lines: Vec<&str> = text.lines().collect();
        s.lines += lines.len();
        if let Some(pos) = lines.iter().position(|l| l.trim() == "#[cfg(test)]") {
            s.test_lines += lines.len() - pos;
        }
        let mut counts = depscan::Counts::default();
        depscan::scan_file(&path, &mut counts);
        s.host_touch += counts.host_touch();
        s.unsafe_sites += counts.unsafe_total();
        s.panic_sites += text.matches("panic!(").count();
        s.unwrap_sites += text.matches(".unwrap()").count() + text.matches(".expect(").count();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_stats_counts_lines_and_markers() {
        let dir = crate::common::scratch().join("bc-stats-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.rs"),
            "fn f() { std::fs::read(\"x\").unwrap(); panic!(\"boom\"); }\n#[cfg(test)]\nmod tests {}\n",
        )
        .unwrap();
        let s = source_stats(&dir, &["a.rs", "missing.rs"]);
        assert_eq!(s.files, 1);
        assert_eq!(s.lines, 3);
        assert_eq!(s.test_lines, 2);
        assert_eq!(s.panic_sites, 1);
        assert_eq!(s.unwrap_sites, 1);
        assert!(s.host_touch >= 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
