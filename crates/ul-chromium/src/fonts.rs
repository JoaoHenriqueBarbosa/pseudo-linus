//! As fontes do navegador: só as que a VFS tem em `/usr/share/fonts` (e `/usr/local/share/fonts`), lidas pelo
//! sysabi; as do hospedeiro nunca entram. A escolha de família reproduz o Chromium do oráculo (Debian 13 com
//! fonts-liberation e fonts-dejavu), medida com `testbench/oracle/measured/font-families.html`:
//!
//! - `sans-serif`, Arial e Helvetica são a Liberation Sans; `serif`, `cursive`, `fantasy`, Times e Times New Roman
//!   são a Liberation Serif; Courier e Courier New, a Liberation Mono (os aliases métricos do fontconfig);
//! - `monospace` é a DejaVu Sans Mono e `system-ui`, a DejaVu Sans;
//! - família desconhecida (e os `ui-*`, `emoji`, `math`) cai na fonte padrão, a Times New Roman, isto é, a
//!   Liberation Serif; o glifo que ela não tem vem da DejaVu Sans, a primeira do fallback do fontconfig.

use std::cell::RefCell;
use std::sync::Arc;

use parley::FontContext;
use parley::fontique::{Blob, Collection, CollectionOptions, FontInfoOverride, GenericFamily, Script, SourceCache};
use sysabi::sys;
use sysabi::types::FileType;

/// Onde o fontconfig do Debian procura fontes do sistema (o `<dir>` do `/etc/fonts/fonts.conf`).
const FONT_DIRS: &[&[u8]] = &[b"/usr/share/fonts", b"/usr/local/share/fonts"];

/// Família pedida pelo nome e a família de verdade que o fontconfig entrega (30-metric-aliases.conf).
const METRIC_ALIASES: &[(&str, &str)] = &[
    ("Arial", "Liberation Sans"),
    ("Helvetica", "Liberation Sans"),
    ("Times New Roman", "Liberation Serif"),
    ("Times", "Liberation Serif"),
    ("Courier New", "Liberation Mono"),
    ("Courier", "Liberation Mono"),
];

/// A família de cada genérico que o Chromium resolve para uma fonte própria; os outros ficam vazios e caem no
/// fallback, como uma família desconhecida.
const GENERICS: &[(GenericFamily, &str)] = &[
    (GenericFamily::SansSerif, "Liberation Sans"),
    (GenericFamily::Serif, "Liberation Serif"),
    (GenericFamily::Cursive, "Liberation Serif"),
    (GenericFamily::Fantasy, "Liberation Serif"),
    (GenericFamily::Monospace, "DejaVu Sans Mono"),
    (GenericFamily::SystemUi, "DejaVu Sans"),
];

/// A fonte padrão do Chromium e o que o fontconfig oferece para glifo que falta, nessa ordem.
const FALLBACK: &[&str] = &["Liberation Serif", "DejaVu Sans", "DejaVu Serif", "DejaVu Sans Mono"];

/// Os scripts que recebem o fallback: o latim e os comuns, mais os que a DejaVu cobre.
const FALLBACK_SCRIPTS: &[&[u8; 4]] = &[b"Latn", b"Zyyy", b"Zinh", b"Grek", b"Cyrl", b"Armn", b"Geor", b"Hebr", b"Arab", b"Thaa", b"Lao ", b"Ogam", b"Runr", b"Tfng", b"Copt", b"Brai", b"Cher", b"Cans", b"Nkoo"];

thread_local! {
    /// Os arquivos de fonte do processo, lidos uma vez (uma thread é um processo do pseudo-linus).
    static FILES: RefCell<Option<Arc<Vec<Blob<u8>>>>> = const { RefCell::new(None) };
}

/// Fixa as fontes do processo sem ler a VFS (os testes que rodam a página fora de um pseudo-processo passam as da
/// imagem do kernel).
pub fn install(files: Vec<Vec<u8>>) {
    let blobs = files.into_iter().map(|bytes| Blob::new(Arc::new(bytes))).collect();
    FILES.with(|f| *f.borrow_mut() = Some(Arc::new(blobs)));
}

/// Os arquivos de fonte da VFS, em ordem de caminho (a ordem de varredura do fontconfig não muda a escolha, que é
/// por nome de família).
fn read_vfs() -> Vec<Blob<u8>> {
    let mut paths = Vec::new();
    if sys::try_current().is_some() {
        for dir in FONT_DIRS {
            collect(dir.to_vec(), &mut paths);
        }
    }
    paths.sort();
    paths.into_iter().filter_map(|p| sys::read_file(&p).ok()).map(|bytes| Blob::new(Arc::new(bytes))).collect()
}

fn collect(dir: Vec<u8>, out: &mut Vec<Vec<u8>>) {
    let Ok(entries) = sys::read_dir(&dir) else { return };
    for entry in entries {
        let mut path = dir.clone();
        path.push(b'/');
        path.extend_from_slice(&entry.name);
        match entry.kind {
            FileType::Directory => collect(path, out),
            FileType::Regular | FileType::Symlink => {
                let lower = entry.name.to_ascii_lowercase();
                if [b".ttf".as_slice(), b".otf", b".ttc", b".otc"].iter().any(|ext| lower.ends_with(ext)) {
                    out.push(path);
                }
            }
            _ => {}
        }
    }
}

fn files() -> Arc<Vec<Blob<u8>>> {
    FILES.with(|f| f.borrow_mut().get_or_insert_with(|| Arc::new(read_vfs())).clone())
}

/// O contexto de fontes de um documento: a coleção sem fontes do sistema, com as da VFS, os aliases, os genéricos e
/// o fallback do Chromium do oráculo, mais a fonte de marcadores de lista do Blitz.
pub fn context() -> FontContext {
    let mut collection = Collection::new(CollectionOptions { shared: false, system_fonts: false });
    let mut by_family: Vec<(String, Blob<u8>)> = Vec::new();
    for blob in files().iter() {
        for (family, _) in collection.register_fonts(blob.clone(), None) {
            if let Some(name) = collection.family_name(family) {
                by_family.push((name.to_owned(), blob.clone()));
            }
        }
    }
    for (alias, real) in METRIC_ALIASES {
        for (_, blob) in by_family.iter().filter(|(name, _)| name == real) {
            collection.register_fonts(blob.clone(), Some(FontInfoOverride { family_name: Some(alias), ..FontInfoOverride::default() }));
        }
    }
    for (generic, family) in GENERICS {
        if let Some(id) = collection.family_id(family) {
            collection.set_generic_families(*generic, std::iter::once(id));
        }
    }
    let fallback: Vec<_> = FALLBACK.iter().filter_map(|name| collection.family_id(name)).collect();
    for script in FALLBACK_SCRIPTS {
        collection.set_fallbacks(Script::from_bytes(**script), fallback.iter().copied());
    }
    collection.register_fonts(Blob::new(Arc::new(blitz_dom::BULLET_FONT) as _), None);
    FontContext { collection, source_cache: SourceCache::new_shared() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regressão do vazamento: sem pseudo-processo e sem fontes instaladas, a coleção não tem família nenhuma (a do
    /// hospedeiro nunca entra), só a de marcadores do Blitz.
    #[test]
    fn host_fonts_never_load() {
        let mut ctx = context();
        let names: Vec<String> = ctx.collection.family_names().map(str::to_owned).collect();
        assert!(names.iter().all(|n| !n.contains("DejaVu") && !n.contains("Liberation")), "famílias do hospedeiro: {names:?}");
    }
}
