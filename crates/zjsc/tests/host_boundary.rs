//! A fronteira do motor com a máquina (REGRA SUPREMA do projeto: nada observável denuncia a simulação). Dentro do
//! pseudo-linus o `bun` lê o sistema de arquivos, o ambiente, os relógios, os processos e o stderr DO SANDBOX, e tudo
//! isso passa pelo trait `Host` (`src/runtime/host.rs`). Este teste varre o `src/` inteiro, sem lista de exceções, e
//! falha se o motor voltar a tocar o host real por `std`. O `Host` que lê a máquina de verdade existe só em
//! `tests/common/std_host.rs`.

use std::path::Path;

/// Trechos que dão acesso à máquina real. O relógio entra pelo `ul_common::time` (que usa o do pseudo-processo).
const FORBIDDEN: &[&str] = &[
    "std::fs",
    "std::net",
    "std::env",
    "std::process",
    "std::os::unix",
    "std::thread::sleep",
    "thread::available_parallelism",
    "Command::new",
    "Instant::now",
    "SystemTime::now",
    "std::time::SystemTime",
    "std::time::UNIX_EPOCH",
    "eprintln!",
    "eprint!",
    "println!",
    "print!(",
    "io::stdout",
    "io::stderr",
    "io::stdin",
];

/// Os módulos de `std` que dão acesso à máquina quando importados em chaves (`use std::{fs, io}`).
const FORBIDDEN_IN_BRACES: &[&str] = &["fs", "net", "env", "process"];

fn rust_files(directory: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn the_engine_never_touches_the_real_machine() {
    let mut files = Vec::new();
    rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    assert!(files.len() > 500, "o `src/` não foi achado: {} arquivos", files.len());
    let mut violations = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        for (number, line) in text.lines().enumerate() {
            let mut hits: Vec<&str> = FORBIDDEN.iter().copied().filter(|pattern| line.contains(pattern)).collect();
            if let Some(start) = line.find("std::{") {
                let inside = line[start + "std::{".len()..].split('}').next().unwrap_or("");
                hits.extend(inside.split(',').map(str::trim).filter_map(|item| FORBIDDEN_IN_BRACES.iter().copied().find(|module| item == *module || item.starts_with(&format!("{module}::")))));
            }
            if !hits.is_empty() {
                violations.push(format!("{}:{}: {:?}: {}", file.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap().display(), number + 1, hits, line.trim()));
            }
        }
    }
    assert!(violations.is_empty(), "o motor tocou a máquina real, use o `Host` (src/runtime/host.rs):\n{}", violations.join("\n"));
}
