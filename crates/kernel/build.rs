//! Gera a lista de itens da imagem base que vêm de árvores inteiras copiadas do oráculo (o
//! `/usr/share/zoneinfo` do tzdata, o locale `C.utf8` do libc-bin e o banco terminfo do
//! ncurses-base do Debian 13): diretórios, arquivos (embutidos com `include_bytes!`) e links
//! simbólicos, em ordem estável. O oráculo tem o `locales-all` inteiro (238 MB); só o `C.utf8`,
//! que todo Debian tem, entra na imagem.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Árvores copiadas do oráculo, relativas a `image/`.
///
/// `usr/share/terminfo`, `usr/share/tabset` e `etc/terminfo` são o conteúdo do ncurses-base
/// (6.5+20250216-2): as descrições compiladas de terminais, os tabsets do `tabs` e o README do
/// diretório do administrador.
///
/// `usr/share/fonts`, `usr/share/fontconfig` e `etc/fonts` vêm do fonts-dejavu-core, do
/// fonts-dejavu-mono e do fontconfig-config, que o Pillow do Debian puxa.
///
/// `usr/lib/python3.13`, `etc/python3.13` e `usr/lib/python3/dist-packages` são a stdlib do
/// libpython3.13-stdlib e os pacotes Python do oráculo (Pillow, PyYAML, pip, packaging, wheel...),
/// com os `__pycache__` que o py3compile gera na instalação.
const TREES: &[&str] = &[
    "usr/share/zoneinfo",
    "usr/lib/locale",
    "usr/share/terminfo",
    "usr/share/tabset",
    "etc/terminfo",
    "usr/share/fonts",
    "usr/share/fontconfig",
    "etc/fonts",
    "usr/lib/python3.13",
    "etc/python3.13",
    "usr/lib/python3",
    // Os wheels do python3-pip-whl e do python3-setuptools-whl, que o `ensurepip` do Debian instala
    // no venv.
    "usr/share/python-wheels",
    // O `README` do dpkg; os links de `/etc/alternatives` saem de `real/links.txt`.
    "etc/alternatives",
    // OpenSSL e ca-certificates: `openssl.cnf`, os links de `/usr/lib/ssl` e os certificados da Mozilla com
    // o `ca-certificates.crt` e os links por hash que o `update-ca-certificates` gera.
    "etc/ssl",
    "usr/lib/ssl",
    "usr/share/ca-certificates",
    "etc/ca-certificates",
    "etc/ca-certificates.conf",
];

/// Árvores cujos arquivos são scripts executáveis do oráculo (`/usr/bin/zgrep`, `/usr/sbin/service`...),
/// instalados com modo 0o755. Os links simbólicos (`bzcmp -> bzdiff`) vêm junto.
const EXEC_TREES: &[&str] = &["usr/bin", "usr/sbin"];

/// Diretórios vazios das árvores acima, que o git não guarda, com o modo do oráculo.
const EMPTY_DIRS: &[(&str, u32)] = &[("/etc/ssl/private", 0o700), ("/etc/ca-certificates/update.d", 0o755)];

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
    for (trees, file_mode) in [(TREES, "0o644"), (EXEC_TREES, "0o755")] {
        for tree in trees {
            use std::os::unix::fs::PermissionsExt as _;
            let top = fs::symlink_metadata(image.join(tree)).expect("árvore da imagem");
            if top.is_file() {
                println!("cargo:rerun-if-changed=image/{tree}");
                let abs = image.join(tree);
                let _ = writeln!(code, "    File(\"/{tree}\", include_bytes!({:?}), {file_mode}),", abs.display().to_string());
                continue;
            }
            let _ = writeln!(code, "    Dir(\"/{tree}\", 0o755),");
            let mut items = Vec::new();
            walk(&image, Path::new(tree), &mut items);
            // O `rerun-if-changed` de um diretório faz o cargo varrer a árvore seguindo os symlinks, e os
            // absolutos da imagem (`/usr/lib/ssl/private -> /etc/ssl/private`) apontam para o sistema de
            // quem compila. Com symlink na árvore, a declaração vai por arquivo; sem, pelo diretório,
            // que também pega arquivo novo.
            if items.iter().any(|(_, md)| md.file_type().is_symlink()) {
                for (rel, md) in &items {
                    if md.is_file() {
                        println!("cargo:rerun-if-changed=image/{}", rel.display());
                    }
                }
            } else {
                println!("cargo:rerun-if-changed=image/{tree}");
            }
            for (rel, md) in items {
                let path = format!("/{}", rel.display());
                if md.file_type().is_symlink() {
                    let target = fs::read_link(image.join(&rel)).expect("link");
                    let _ = writeln!(code, "    Link({path:?}, {:?}),", target.display().to_string());
                } else if md.is_dir() {
                    let _ = writeln!(code, "    Dir({path:?}, 0o755),");
                } else {
                    let abs = image.join(&rel);
                    // Os scripts executáveis da stdlib (`base64.py`, `pdb.py`...) mantêm o 0o755.
                    let mode = if md.permissions().mode() & 0o100 != 0 { "0o755" } else { file_mode };
                    let _ = writeln!(code, "    File({path:?}, include_bytes!({:?}), {mode}),", abs.display().to_string());
                }
            }
        }
    }
    for (dir, mode) in EMPTY_DIRS {
        let _ = writeln!(code, "    Dir({dir:?}, 0o{mode:o}),");
    }
    code.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("copied_trees.rs");
    fs::write(out, code).expect("gravar copied_trees.rs");
}
