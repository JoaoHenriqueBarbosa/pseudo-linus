//! Caminhos do workspace da bancada.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::{Case, CaseFile, Outcome};

/// Raiz do workspace `testbench/`.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("raiz do testbench")
}

/// Raiz do repositório (`pseudo-linus/`).
pub fn repo_root() -> PathBuf {
    root().join("..").canonicalize().expect("raiz do repositório")
}

pub fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root().join("target"))
}

pub fn corpus_dir() -> PathBuf {
    root().join("corpus")
}

pub fn cases_dir(tool: &str) -> PathBuf {
    corpus_dir().join("cases").join(tool)
}

pub fn golden_dir(tool: &str) -> PathBuf {
    root().join("golden").join(tool)
}

pub fn results_dir() -> PathBuf {
    root().join("results")
}

/// Diretório de rascunho gitignored, pra artefatos grandes e detalhes.
pub fn scratch_dir(name: &str) -> PathBuf {
    let dir = root().join("scratch").join(name);
    std::fs::create_dir_all(&dir).expect("criar scratch");
    dir
}

/// Todos os arquivos de caso de uma ferramenta, em ordem.
pub fn case_files(tool: &str) -> Result<Vec<PathBuf>> {
    let dir = cases_dir(tool);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    Ok(files)
}

/// Ferramentas que têm casos no corpus.
pub fn tools() -> Result<Vec<String>> {
    let dir = corpus_dir().join("cases");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut tools: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    tools.sort();
    Ok(tools)
}

/// Caminho do golden correspondente a um arquivo de casos.
pub fn golden_for(case_file: &Path) -> PathBuf {
    let tool = case_file.parent().and_then(Path::file_name).expect("diretório da ferramenta");
    let stem = case_file.file_stem().expect("nome do arquivo");
    root()
        .join("golden")
        .join(tool)
        .join(format!("{}.json", stem.to_string_lossy()))
}

/// Carrega casos e golden de uma ferramenta. Casos sem golden são ignorados (e contados).
pub fn load_tool(tool: &str) -> Result<(Vec<(Case, Outcome)>, usize)> {
    let mut out = Vec::new();
    let mut missing = 0;
    for path in case_files(tool)? {
        let file = CaseFile::load(&path)?;
        let golden_path = golden_for(&path);
        let golden: std::collections::BTreeMap<String, Outcome> = if golden_path.exists() {
            let text = std::fs::read_to_string(&golden_path)?;
            serde_json::from_str(&text).with_context(|| format!("parse {}", golden_path.display()))?
        } else {
            Default::default()
        };
        for case in file.cases {
            match golden.get(&case.id) {
                Some(g) => out.push((case, g.clone())),
                None => missing += 1,
            }
        }
    }
    Ok((out, missing))
}
