//! Acoplamento ao host e unsafe de cada candidato, via `depscan` (com cache em disco, porque a
//! árvore do zawk tem centenas de crates).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanSummary {
    pub package: String,
    pub version: String,
    pub manifest: String,
    pub own_category: String,
    pub tree_category: String,
    pub own_host_touch: usize,
    pub own_unsafe: usize,
    pub tree_deps: usize,
    pub tree_host_touch: usize,
    pub tree_unsafe: usize,
    pub c_deps: Vec<String>,
    /// Categoria da árvore sem os falsos positivos de `links` (ver [`refined_is_c`]).
    pub tree_category_refined: String,
    /// Dependências que de fato compilam ou linkam C/C++ (`cc`, `cmake`, `bindgen`, `pkg-config` ou `-sys`).
    pub c_deps_refined: Vec<String>,
    /// As dez dependências que mais tocam o host.
    pub top_host_deps: Vec<(String, usize)>,
}

/// O depscan marca como (c) toda crate com `links = ...`; crates como `defmt`, `rayon-core` e
/// `wasm-bindgen-shared` usam `links` só pra impedir duas versões no grafo, sem código C, e crates
/// `-sys` como `linux-raw-sys` e `windows-sys` são Rust puro. Aqui conta como C o que tem ferramenta
/// de build de C (`cc`, `cmake`, `bindgen`, `pkg-config`) ou é `-sys` com `links` e `build.rs`.
fn refined_is_c(p: &depscan::PackageScan) -> bool {
    let sys = p.name.ends_with("-sys") || p.name.ends_with("_sys");
    !p.c_build_deps.is_empty() || (sys && p.links.is_some() && p.has_build_rs)
}

fn refined_letter(p: &depscan::PackageScan) -> &'static str {
    if refined_is_c(p) {
        "c"
    } else if p.counts.host_touch() > 0 {
        "b"
    } else {
        "a"
    }
}

pub fn summarize(manifest: &Path, s: depscan::TreeScan) -> ScanSummary {
    let mut host: Vec<(String, usize)> = s.host_touching_deps.into_iter().collect();
    host.sort_by_key(|h| std::cmp::Reverse(h.1));
    host.truncate(10);
    let mut refined = refined_letter(&s.root);
    let mut c_refined = Vec::new();
    for d in &s.deps {
        let l = refined_letter(d);
        if l > refined {
            refined = l;
        }
        if l == "c" {
            c_refined.push(format!("{} {}", d.name, d.version));
        }
    }
    ScanSummary {
        package: s.root.name.clone(),
        version: s.root.version.clone(),
        manifest: manifest.display().to_string(),
        own_category: s.root.category.letter().to_string(),
        tree_category: s.tree_category.letter().to_string(),
        own_host_touch: s.root.counts.host_touch(),
        own_unsafe: s.root.counts.unsafe_total(),
        tree_deps: s.deps.len(),
        tree_host_touch: s.totals.host_touch(),
        tree_unsafe: s.totals.unsafe_total(),
        c_deps: s.c_deps,
        tree_category_refined: refined.to_string(),
        c_deps_refined: c_refined,
        top_host_deps: host,
    }
}

/// Cache das varreduras brutas do depscan (o resumo é recalculado a cada execução).
#[derive(Default, Serialize, Deserialize)]
struct Cache(BTreeMap<String, depscan::TreeScan>);

/// Varre em paralelo; `items` = (chave, manifesto, pacote).
pub fn scan_all(items: Vec<(String, PathBuf, String)>, cache_path: &Path) -> BTreeMap<String, Result<ScanSummary, String>> {
    let cache: Cache = std::fs::read_to_string(cache_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let raw: Vec<Result<depscan::TreeScan, String>> = crate::exec::par_map(&items, 8, |(_, manifest, package)| {
        let ck = format!("{}#{package}", manifest.display());
        if let Some(hit) = cache.0.get(&ck) {
            return Ok(hit.clone());
        }
        depscan::scan(manifest, package).map_err(|e| format!("{e:#}"))
    });
    let mut new_cache = cache;
    let mut out = BTreeMap::new();
    for ((key, manifest, package), r) in items.iter().zip(raw) {
        match r {
            Ok(s) => {
                new_cache.0.insert(format!("{}#{package}", manifest.display()), s.clone());
                out.insert(key.clone(), Ok(summarize(manifest, s)));
            }
            Err(e) => {
                out.insert(key.clone(), Err(e));
            }
        }
    }
    if let Ok(t) = serde_json::to_string(&new_cache) {
        let _ = std::fs::write(cache_path, t);
    }
    out
}
