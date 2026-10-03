//! Evidência de código com arquivo e linha, conferida em tempo de execução.
//!
//! Cada afirmação sobre o código de uma crate ("o reedline escreve direto no stderr do host") vira uma
//! busca de um trecho exato num arquivo do código-fonte baixado pelo cargo. Se a crate mudar e o
//! trecho sumir, o experimento falha alto em vez de registrar uma linha que não existe mais.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct CodeEvidence {
    /// `nome-versão` da crate.
    pub krate: String,
    /// Caminho relativo à raiz da crate.
    pub file: String,
    pub line: usize,
    /// A linha como está no arquivo (sem espaços nas pontas).
    pub text: String,
    /// O que a linha prova, em português.
    pub claim: String,
}

/// Raiz do código-fonte de `name` na versão resolvida pelo workspace do experimento.
pub fn crate_root(name: &str) -> Result<PathBuf> {
    static ROOTS: OnceLock<BTreeMap<String, PathBuf>> = OnceLock::new();
    let roots = match ROOTS.get() {
        Some(r) => r,
        None => {
            let meta = cargo_metadata::MetadataCommand::new()
                .manifest_path(crate::common::manifest())
                .exec()
                .context("cargo metadata do experimento")?;
            let mut map: BTreeMap<String, (semver_key::Key, PathBuf)> = BTreeMap::new();
            for p in &meta.packages {
                let dir = p.manifest_path.parent().expect("dir").as_std_path().to_path_buf();
                let key = semver_key::Key::from(&p.version);
                match map.get(p.name.as_str()) {
                    Some((k, _)) if *k >= key => {}
                    _ => {
                        map.insert(p.name.to_string(), (key, dir));
                    }
                }
            }
            let _ = ROOTS.set(map.into_iter().map(|(k, (_, d))| (k, d)).collect());
            ROOTS.get().expect("recém preenchido")
        }
    };
    roots.get(name).cloned().with_context(|| format!("crate {name} não está no workspace do experimento"))
}

/// Ordenação de versões sem depender do crate semver diretamente.
mod semver_key {
    #[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Key(u64, u64, u64, String);

    impl From<&cargo_metadata::semver::Version> for Key {
        fn from(v: &cargo_metadata::semver::Version) -> Key {
            Key(v.major, v.minor, v.patch, v.pre.to_string())
        }
    }
}

/// Acha a primeira linha de `rel_file` (dentro da crate `name`) que contém `needle`.
pub fn find(name: &str, rel_file: &str, needle: &str, claim: &str) -> Result<CodeEvidence> {
    find_nth(name, rel_file, needle, 1, claim)
}

/// Como [`find`], mas pega a `n`-ésima ocorrência (1 = primeira).
pub fn find_nth(name: &str, rel_file: &str, needle: &str, n: usize, claim: &str) -> Result<CodeEvidence> {
    let root = crate_root(name)?;
    let krate = root.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let path = root.join(rel_file);
    let text = std::fs::read_to_string(&path).with_context(|| format!("ler {}", path.display()))?;
    let mut seen = 0;
    for (i, line) in text.lines().enumerate() {
        if line.contains(needle) {
            seen += 1;
            if seen == n {
                return Ok(CodeEvidence {
                    krate,
                    file: rel_file.to_string(),
                    line: i + 1,
                    text: line.trim().to_string(),
                    claim: claim.to_string(),
                });
            }
        }
    }
    bail!("{krate}/{rel_file}: trecho {needle:?} (ocorrência {n}) não encontrado")
}

pub fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned()
}
