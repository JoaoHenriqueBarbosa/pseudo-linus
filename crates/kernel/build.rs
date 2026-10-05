//! Gera a lista de itens da imagem base que vêm de árvores inteiras copiadas do oráculo (o
//! `/usr/share/zoneinfo` do tzdata e o locale `C.utf8` do libc-bin do Debian 13): diretórios,
//! arquivos (embutidos com `include_bytes!`) e links simbólicos, em ordem estável. O oráculo tem o
//! `locales-all` inteiro (238 MB); só o `C.utf8`, que todo Debian tem, entra na imagem.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Árvores copiadas do oráculo, relativas a `image/`.
const TREES: &[&str] = &["usr/share/zoneinfo", "usr/lib/locale"];

fn walk(root: &Path, rel: &Path, out: &mut Vec<(PathBuf, fs::Metadata)>) {
    let dir = root.join(rel);
    let mut entries: Vec<_> = fs::read_dir(&dir).expect("árvore da imagem").map(|e| e.expect("entrada").file_name()).collect();
    entries.sort();
    for name in entries {
        let r = rel.join(&name);
        let md = fs::symlink_metadata(root.join(&r)).expect("metadados");
        let is_dir = md.is_dir();
        out.push((r.clone(), md));
        if is_dir {
            walk(root, &r, out);
        }
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let image = manifest.join("image");
    let mut code = String::from("/// Itens gerados pelo build.rs a partir das árvores copiadas do oráculo.\nconst COPIED_TREES: &[Item] = &[\n");
    for tree in TREES {
        println!("cargo:rerun-if-changed=image/{tree}");
        let _ = writeln!(code, "    Dir(\"/{tree}\", 0o755),");
        let mut items = Vec::new();
        walk(&image, Path::new(tree), &mut items);
        for (rel, md) in items {
            let path = format!("/{}", rel.display());
            if md.file_type().is_symlink() {
                let target = fs::read_link(image.join(&rel)).expect("link");
                let _ = writeln!(code, "    Link({path:?}, {:?}),", target.display().to_string());
            } else if md.is_dir() {
                let _ = writeln!(code, "    Dir({path:?}, 0o755),");
            } else {
                let abs = image.join(&rel);
                let _ = writeln!(code, "    File({path:?}, include_bytes!({:?}), 0o644),", abs.display().to_string());
            }
        }
    }
    code.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("copied_trees.rs");
    fs::write(out, code).expect("gravar copied_trees.rs");
}
