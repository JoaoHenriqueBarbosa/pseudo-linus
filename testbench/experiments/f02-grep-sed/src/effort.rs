//! Esforço de porte medido no código-fonte de cada candidato: quantas linhas e arquivos fazem I/O do
//! host (o que teria de passar pelo nosso `Ctx`) e quantos acoplam ao motor de regex (o que teria de
//! trocar pro motor do F01). É contagem de pontos de toque, não estimativa de horas.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize)]
pub struct Effort {
    pub rust_files: usize,
    pub code_lines: usize,
    pub host_io_lines: usize,
    pub host_io_files: Vec<String>,
    pub regex_lines: usize,
    pub regex_files: Vec<String>,
}

const HOST_IO: &[&str] = &[
    "std::fs",
    "fs::File",
    "File::open",
    "File::create",
    "OpenOptions",
    "io::stdout(",
    "io::stdin(",
    "io::stderr(",
    "stdout().lock",
    "stdin().lock",
    "std::process::",
    "std::env::",
    "libc::",
    "rustix::",
    "memmap2",
    "tempfile",
];

const REGEX: &[&str] = &[
    "regex::",
    "Regex::new",
    "RegexBuilder",
    "fancy_regex",
    "onig::",
    "crate::regex::",
    "regex::Matcher",
    "Matcher::compile",
];

/// Varre os `.rs` de `dir` (recursivo; caminhos com algum trecho de `exclude` ficam de fora; código
/// depois de `#[cfg(test)]` também) e conta os pontos de toque.
pub fn scan(dir: &Path, exclude: &[&str]) -> Effort {
    let mut e = Effort::default();
    let mut host_files: BTreeMap<String, usize> = BTreeMap::new();
    let mut regex_files: BTreeMap<String, usize> = BTreeMap::new();
    // Um arquivo só (ex.: o grep.rs do bashkit) ou um diretório inteiro.
    let (crate_dir, mut stack, single) = if dir.is_file() {
        (dir.parent().unwrap_or(dir), Vec::new(), Some(dir.to_path_buf()))
    } else {
        (dir, vec![dir.to_path_buf()], None)
    };
    let mut first = single;
    loop {
        let paths: Vec<std::path::PathBuf> = if let Some(f) = first.take() {
            vec![f]
        } else if let Some(d) = stack.pop() {
            match std::fs::read_dir(&d) {
                Ok(rd) => rd.flatten().map(|i| i.path()).collect(),
                Err(_) => continue,
            }
        } else {
            break;
        };
        for p in paths {
            let rel = p.strip_prefix(crate_dir).unwrap_or(&p).to_string_lossy().into_owned();
            if exclude.iter().any(|x| rel.contains(x)) {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if p.extension().is_none_or(|x| x != "rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            e.rust_files += 1;
            let mut in_tests = false;
            for line in text.lines() {
                let t = line.trim();
                if t.starts_with("#[cfg(test)]") {
                    in_tests = true;
                }
                if in_tests || t.is_empty() || t.starts_with("//") {
                    continue;
                }
                e.code_lines += 1;
                if HOST_IO.iter().any(|n| t.contains(n)) {
                    e.host_io_lines += 1;
                    *host_files.entry(rel.clone()).or_default() += 1;
                }
                if REGEX.iter().any(|n| t.contains(n)) {
                    e.regex_lines += 1;
                    *regex_files.entry(rel.clone()).or_default() += 1;
                }
            }
        }
    }
    e.host_io_files = host_files.into_iter().map(|(f, n)| format!("{f} ({n})")).collect();
    e.regex_files = regex_files.into_iter().map(|(f, n)| format!("{f} ({n})")).collect();
    e
}
