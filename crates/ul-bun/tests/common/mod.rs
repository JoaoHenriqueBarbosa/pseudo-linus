//! O que os testes de integração do `ul-bun` partilham: os programas do sandbox (o `bun` e o userland que os scripts
//! chamam) e a cópia de uma árvore do host para o VFS do sandbox.

use std::path::{Path, PathBuf};

use kernel::SandboxFs;

/// O `bun` e o userland que os scripts de `bun run` chamam, como numa sessão do pseudo-linus com o `bun` instalado.
pub fn programs() -> Vec<sysabi::Program> {
    let mut programs = userland::all_programs();
    programs.extend(ul_bun::programs());
    programs
}

/// Os arquivos de `dir` como `(caminho relativo, conteúdo)`.
pub fn read_tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).expect("diretório") {
            let path: PathBuf = entry.expect("entrada").path();
            if path.is_dir() {
                walk(&path, base, out);
            } else {
                let rel = path.strip_prefix(base).expect("relativo").to_string_lossy().into_owned();
                out.push((rel, std::fs::read(&path).expect("arquivo")));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

/// Grava `files` (de [`read_tree`]) em `dest` no VFS do sandbox, criando os diretórios.
pub fn install(fs: &SandboxFs<'_>, dest: &str, files: &[(String, Vec<u8>)]) {
    fs.mkdir_all(dest.as_bytes(), 0o755).expect("destino");
    for (rel, data) in files {
        if let Some((parent, _)) = rel.rsplit_once('/') {
            fs.mkdir_all(format!("{dest}/{parent}").as_bytes(), 0o755).expect("diretório");
        }
        fs.write_file(format!("{dest}/{rel}").as_bytes(), data, 0o644).expect("arquivo");
    }
}
