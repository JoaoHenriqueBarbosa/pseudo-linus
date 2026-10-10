//! O que os testes de integração do navegador compartilham.

use std::path::{Path, PathBuf};

/// As fontes da imagem do kernel: (caminho na VFS, bytes), o que o pseudo-linus tem em `/usr/share/fonts`.
pub fn image_fonts() -> Vec<(String, Vec<u8>)> {
    let image = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../kernel/image");
    let mut paths = Vec::new();
    collect(&image.join("usr/share/fonts"), &mut paths);
    paths.sort();
    paths.into_iter().map(|p| (format!("/{}", p.strip_prefix(&image).unwrap().display()), std::fs::read(&p).unwrap())).collect()
}

/// Instala as fontes da imagem na thread do teste, para a página rodada fora de um pseudo-processo medir texto como
/// no sistema de verdade.
#[allow(dead_code)]
pub fn install_image_fonts() {
    ul_chromium::fonts::install(image_fonts().into_iter().map(|(_, bytes)| bytes).collect());
}

/// O TestKit com as fontes da imagem na VFS.
#[allow(dead_code)]
pub fn with_image_fonts(mut kit: sysabi::testkit::TestKit) -> sysabi::testkit::TestKit {
    for (path, bytes) in image_fonts() {
        kit = kit.file(&path, bytes, 0o644);
    }
    kit
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "ttf" || e == "otf") {
            out.push(path);
        }
    }
}
