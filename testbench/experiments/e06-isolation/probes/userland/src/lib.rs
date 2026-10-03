//! Sonda do E06 (H19): o crate de userland com o `clippy.toml` de isolamento. Nada aqui chama `std::fs`
//! diretamente; o I/O do host acontece dentro das dependências. Se o `cargo clippy -D warnings` passar
//! limpo, o lint não garante isolamento.

/// Lê um arquivo do host por uma dependência nossa (sonda `fs-reader`).
pub fn read_via_dependency(path: &str) -> usize {
    fs_reader::read_host_file(path).map(|bytes| bytes.len()).unwrap_or(0)
}

/// Lista um diretório do host por uma dependência real do crates.io (`walkdir`, que usa `std::fs::read_dir`).
pub fn walk_via_registry_crate(dir: &str) -> usize {
    walkdir::WalkDir::new(dir).max_depth(1).into_iter().filter_map(Result::ok).count()
}
