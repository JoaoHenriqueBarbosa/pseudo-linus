//! Binários de terceiros (candidatos que só existem como CLI ou cujo CLI é o que se mede).
//!
//! Cada um é instalado com `cargo install` num prefixo próprio dentro do scratch (gitignored). O
//! `cargo install` usa um target temporário e apaga depois, então o disco só guarda os executáveis.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, bail};

pub struct ToolSpec {
    /// Nome curto usado como diretório e nos resultados.
    pub key: &'static str,
    /// Crate (ou pacote dentro do repositório git).
    pub crate_name: &'static str,
    pub version: &'static str,
    /// Executável instalado.
    pub bin: &'static str,
    /// Argumentos extras do `cargo install`.
    pub install_args: &'static [&'static str],
    pub repo: &'static str,
}

pub const UUTILS_AWK_REV: &str = "e7873154aecff1e9c2329fc04cfbcb407d685efb";

pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        key: "uutils-awk",
        crate_name: "uu_awk",
        version: "git e787315 (2026-10-01)",
        bin: "awk",
        install_args: &["--git", "https://github.com/uutils/awk", "--rev", UUTILS_AWK_REV, "--locked", "uu_awk"],
        repo: "https://github.com/uutils/awk",
    },
    ToolSpec {
        key: "awk-rs",
        crate_name: "awk-rs",
        version: "0.2.0",
        bin: "awk-rs",
        install_args: &["awk-rs", "--version", "0.2.0", "--locked"],
        repo: "https://github.com/quinnjr/rawk",
    },
    ToolSpec {
        key: "awkrs",
        crate_name: "awkrs",
        version: "0.5.6",
        bin: "awkrs",
        install_args: &["awkrs", "--version", "0.5.6", "--locked"],
        repo: "https://github.com/MenkeTechnologies/awkrs",
    },
    ToolSpec {
        key: "frawk",
        crate_name: "frawk",
        version: "0.4.8",
        bin: "frawk",
        // O default liga o backend LLVM (precisa de LLVM 12 no host) e features nightly.
        install_args: &[
            "frawk",
            "--version",
            "0.4.8",
            "--locked",
            "--no-default-features",
            "--features",
            "use_jemalloc,allow_avx2",
        ],
        repo: "https://github.com/ezrosent/frawk",
    },
    ToolSpec {
        key: "zawk",
        crate_name: "zawk",
        version: "0.5.25",
        bin: "zawk",
        install_args: &["zawk", "--version", "0.5.25", "--locked"],
        repo: "https://github.com/linux-china/zawk",
    },
    ToolSpec {
        key: "jaq",
        crate_name: "jaq",
        version: "3.1.1",
        bin: "jaq",
        install_args: &["jaq", "--version", "3.1.1", "--locked"],
        repo: "https://github.com/01mf02/jaq",
    },
    ToolSpec {
        key: "xq",
        crate_name: "xq",
        version: "0.5.0",
        bin: "xq",
        install_args: &["xq", "--version", "0.5.0", "--locked", "--features", "build-binary"],
        repo: "https://github.com/MiSawa/xq",
    },
    ToolSpec {
        key: "qj",
        crate_name: "qj",
        version: "0.2.1",
        bin: "qj",
        install_args: &["qj", "--version", "0.2.1", "--locked"],
        repo: "https://github.com/6/qj",
    },
    ToolSpec {
        key: "tq",
        crate_name: "tq-cli",
        version: "0.3.0",
        bin: "tq",
        install_args: &["tq-cli", "--version", "0.3.0", "--locked"],
        repo: "https://github.com/commandzero/tq",
    },
];

pub fn spec(key: &str) -> &'static ToolSpec {
    TOOLS.iter().find(|t| t.key == key).expect("ferramenta conhecida")
}

pub fn tools_root(scratch: &Path) -> PathBuf {
    scratch.join("tools")
}

pub fn bin_path(scratch: &Path, spec: &ToolSpec) -> PathBuf {
    tools_root(scratch).join(spec.key).join("bin").join(spec.bin)
}

/// Garante que o binário existe, instalando se faltar. A saída do cargo vai direto pro terminal.
pub fn ensure(scratch: &Path, spec: &ToolSpec) -> Result<PathBuf> {
    let bin = bin_path(scratch, spec);
    if bin.exists() {
        return Ok(bin);
    }
    eprintln!("[f04] instalando {} ({}) com cargo install", spec.key, spec.version);
    let root = tools_root(scratch).join(spec.key);
    let status = Command::new("cargo")
        .arg("install")
        .arg("--root")
        .arg(&root)
        .args(spec.install_args)
        .current_dir(scratch)
        .status()?;
    if !status.success() || !bin.exists() {
        bail!("cargo install de {} falhou ({status})", spec.key);
    }
    Ok(bin)
}

/// Manifesto onde a crate aparece como pacote raiz, pra varredura do depscan.
pub fn manifest_for(spec: &ToolSpec) -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    if spec.key == "uutils-awk" {
        let checkouts = home.join(".cargo/git/checkouts");
        for entry in std::fs::read_dir(checkouts).ok()?.flatten() {
            if !entry.file_name().to_string_lossy().starts_with("awk-") {
                continue;
            }
            let candidate = entry.path().join(&UUTILS_AWK_REV[..7]).join("Cargo.toml");
            if candidate.exists() {
                return Some(candidate);
            }
        }
        return None;
    }
    registry_manifest(spec.crate_name, spec.version)
}

/// `~/.cargo/registry/src/<índice>/<crate>-<versão>/Cargo.toml`, se já foi baixada.
pub fn registry_manifest(name: &str, version: &str) -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let src = home.join(".cargo/registry/src");
    for index in std::fs::read_dir(src).ok()?.flatten() {
        let candidate = index.path().join(format!("{name}-{version}")).join("Cargo.toml");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}
