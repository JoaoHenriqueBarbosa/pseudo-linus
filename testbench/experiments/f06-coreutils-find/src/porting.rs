//! Medidas do porte: linhas alteradas contra o pristino do registry, arquivos que o compilador de
//! fato usa (dep-info do cargo), auditoria de toque no host e de unsafe nesses arquivos antes e
//! depois (com o visitor do depscan), e quanto do uucore sobreviveu sem alteração.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cargo_metadata::MetadataCommand;
use depscan::Counts;
use serde::Serialize;
use similar::{ChangeTag, TextDiff};

pub fn experiment_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Um crate vendorizado e portado.
#[derive(Clone, Debug)]
pub struct Port {
    /// Nome do pacote original no crates.io.
    pub original: &'static str,
    pub version: &'static str,
    /// Nome do pacote portado neste workspace.
    pub ported: &'static str,
    /// Nome da lib (o mesmo do original).
    pub lib: &'static str,
}

pub const PORTS: &[Port] = &[
    Port { original: "uucore", version: "0.12.0", ported: "port-uucore", lib: "uucore" },
    Port { original: "uucore_procs", version: "0.12.0", ported: "port-uucore-procs", lib: "uucore_procs" },
    Port { original: "uu_cat", version: "0.12.0", ported: "port-uu-cat", lib: "uu_cat" },
    Port { original: "uu_head", version: "0.12.0", ported: "port-uu-head", lib: "uu_head" },
    Port { original: "uu_wc", version: "0.12.0", ported: "port-uu-wc", lib: "uu_wc" },
    Port { original: "uu_sort", version: "0.12.0", ported: "port-uu-sort", lib: "uu_sort" },
    Port { original: "uu_ls", version: "0.12.0", ported: "port-uu-ls", lib: "uu_ls" },
    Port { original: "lscolors", version: "0.21.0", ported: "port-lscolors", lib: "lscolors" },
    Port { original: "findutils", version: "0.10.0", ported: "port-findutils", lib: "findutils" },
    Port { original: "walkdir", version: "2.5.0", ported: "port-walkdir", lib: "walkdir" },
];

impl Port {
    pub fn ported_dir(&self) -> PathBuf {
        experiment_dir().join("ported").join(format!("{}-{}", self.original, self.version))
    }
}

/// Diretórios pristinos no registry, achados pelo `cargo metadata` do workspace.
pub fn pristine_dirs() -> Result<BTreeMap<String, PathBuf>> {
    let meta = MetadataCommand::new()
        .manifest_path(experiment_dir().join("Cargo.toml"))
        .exec()
        .context("cargo metadata")?;
    let mut out = BTreeMap::new();
    for p in PORTS {
        let found = meta.packages.iter().find(|pkg| {
            pkg.name.as_str() == p.original && pkg.version.to_string() == p.version && pkg.source.is_some()
        });
        if let Some(pkg) = found {
            let dir = pkg.manifest_path.parent().expect("pai").as_std_path().to_path_buf();
            out.insert(p.original.to_string(), dir);
        } else {
            // Dependência que só o porte usa (lscolors, walkdir): o registry tem a cópia do download.
            let guess = registry_src()?.join(format!("{}-{}", p.original, p.version));
            if guess.exists() {
                out.insert(p.original.to_string(), guess);
            }
        }
    }
    Ok(out)
}

fn registry_src() -> Result<PathBuf> {
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
        .context("CARGO_HOME")?;
    let src = home.join("registry").join("src");
    let first = std::fs::read_dir(&src)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("index.crates.io")))
        .context("registry do crates.io")?;
    Ok(first)
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct FileDiff {
    pub path: String,
    pub inserted: usize,
    pub deleted: usize,
    pub pristine_lines: usize,
    pub unchanged_lines: usize,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CrateDiff {
    pub krate: String,
    /// Só arquivos `.rs` (e o build.rs); manifesto à parte.
    pub rs_inserted: usize,
    pub rs_deleted: usize,
    pub files_changed: usize,
    pub files_added: Vec<String>,
    pub manifest_inserted: usize,
    pub manifest_deleted: usize,
    pub files: Vec<FileDiff>,
}

fn rs_files(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for sub in ["src", "build.rs"] {
        let start = root.join(sub);
        if !start.exists() {
            continue;
        }
        for e in walkdir::WalkDir::new(&start).into_iter().filter_map(|e| e.ok()) {
            if e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "rs") {
                let rel = e.path().strip_prefix(root).expect("dentro").to_string_lossy().into_owned();
                out.insert(rel);
            }
        }
    }
    out
}

pub fn diff_text(a: &str, b: &str) -> (usize, usize, usize) {
    let diff = TextDiff::from_lines(a, b);
    let (mut ins, mut del, mut eq) = (0, 0, 0);
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => ins += 1,
            ChangeTag::Delete => del += 1,
            ChangeTag::Equal => eq += 1,
        }
    }
    (ins, del, eq)
}

pub fn diff_crate(name: &str, pristine: &Path, ported: &Path) -> Result<CrateDiff> {
    let mut out = CrateDiff { krate: name.to_string(), ..CrateDiff::default() };
    let all: BTreeSet<String> = rs_files(pristine).union(&rs_files(ported)).cloned().collect();
    for rel in all {
        let a = std::fs::read_to_string(pristine.join(&rel)).unwrap_or_default();
        let b_path = ported.join(&rel);
        if !b_path.exists() {
            // Arquivo sumiu do porte: conta como apagado.
            let n = a.lines().count();
            out.rs_deleted += n;
            out.files_changed += 1;
            out.files.push(FileDiff { path: rel, deleted: n, pristine_lines: n, ..FileDiff::default() });
            continue;
        }
        let b = std::fs::read_to_string(&b_path)?;
        if a.is_empty() && !pristine.join(&rel).exists() {
            out.files_added.push(rel.clone());
        }
        let (ins, del, eq) = diff_text(&a, &b);
        if ins + del > 0 {
            out.files_changed += 1;
        }
        out.rs_inserted += ins;
        out.rs_deleted += del;
        out.files.push(FileDiff {
            path: rel,
            inserted: ins,
            deleted: del,
            pristine_lines: a.lines().count(),
            unchanged_lines: eq,
        });
    }
    let ma = std::fs::read_to_string(pristine.join("Cargo.toml")).unwrap_or_default();
    let mb = std::fs::read_to_string(ported.join("Cargo.toml")).unwrap_or_default();
    let (mi, md, _) = diff_text(&ma, &mb);
    out.manifest_inserted = mi;
    out.manifest_deleted = md;
    Ok(out)
}

/// Arquivos-fonte que o rustc leu pra compilar a lib `lib` a partir de `dir` (dep-info `.d` mais
/// novo em `target/release/deps` cujas fontes estão sob `dir`).
pub fn compiled_sources(lib: &str, dir: &Path) -> Result<BTreeSet<String>> {
    let deps = experiment_dir().join("target").join("release").join("deps");
    let mut best: Option<(std::time::SystemTime, BTreeSet<String>)> = None;
    let dir_str = dir.to_string_lossy().into_owned();
    for e in std::fs::read_dir(&deps)?.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.starts_with(&format!("{lib}-")) && name.ends_with(".d")) {
            continue;
        }
        let text = std::fs::read_to_string(e.path())?;
        let first = text.lines().next().unwrap_or_default();
        let Some((_, rest)) = first.split_once(": ") else { continue };
        // Dependência por caminho aparece relativa à raiz do workspace; a do registry, absoluta.
        let files: BTreeSet<String> = split_escaped(rest)
            .into_iter()
            .map(|f| {
                let p = PathBuf::from(&f);
                if p.is_absolute() { p } else { experiment_dir().join(p) }
            })
            .filter(|p| p.to_string_lossy().starts_with(&dir_str) && p.extension().is_some_and(|x| x == "rs"))
            .filter_map(|p| p.strip_prefix(dir).ok().map(|r| r.to_string_lossy().into_owned()))
            .collect();
        if files.is_empty() {
            continue;
        }
        let mtime = e.metadata()?.modified()?;
        if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
            best = Some((mtime, files));
        }
    }
    Ok(best.map(|(_, f)| f).unwrap_or_default())
}

/// Divide a lista de dependências do `.d` (espaço escapado com `\ `).
fn split_escaped(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&' ') => {
                cur.push(' ');
                chars.next();
            }
            ' ' => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            ch => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Audit {
    pub files: usize,
    pub before: Counts,
    pub after: Counts,
    pub host_touch_before: usize,
    pub host_touch_after: usize,
    pub unsafe_before: usize,
    pub unsafe_after: usize,
    /// Linhas dos arquivos compilados no pristino e quantas passaram sem alteração.
    pub compiled_pristine_lines: usize,
    pub compiled_unchanged_lines: usize,
    /// Chamadas de métodos de `Path` que tocam o FS do host (`.exists()`, `.metadata()`...), que não
    /// dá pra trocar por import: saldo removido − adicionado nas linhas alteradas.
    pub path_method_calls_replaced: isize,
    /// `static` de estado global (`OnceLock`, `LazyLock`, `Atomic*`, `thread_local!`) removidos.
    pub global_statics_removed: isize,
    /// Arquivos compilados do porte em que o depscan ainda conta toque no host ou unsafe, com os
    /// contadores do arquivo. O compilador garante zero unsafe no que compila (o
    /// `forbid(unsafe_code)` passa); o resto é código atrás de `cfg` de outra plataforma, ou
    /// `print!`/`eprintln!` que agora resolvem pro sysio (o depscan casa macro pelo nome).
    pub residual: BTreeMap<String, Counts>,
}

fn count_path_methods(text: &str) -> isize {
    const METHODS: &[&str] = &[
        ".exists()",
        ".is_dir()",
        ".is_file()",
        ".is_symlink()",
        ".metadata()",
        ".symlink_metadata()",
        ".read_link()",
        ".canonicalize()",
    ];
    METHODS.iter().map(|m| text.matches(m).count() as isize).sum()
}

fn count_statics(text: &str) -> isize {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("static ") || t.starts_with("pub static ") || t.starts_with("pub(crate) static ")
        })
        .filter(|l| ["OnceLock", "LazyLock", "Atomic", "Mutex", "LazyCell"].iter().any(|k| l.contains(k)))
        .count() as isize
}

/// Auditoria dos arquivos compilados do porte, comparando com os mesmos arquivos no pristino.
pub fn audit(pristine: &Path, ported: &Path, compiled: &BTreeSet<String>) -> Result<Audit> {
    let mut a = Audit { files: compiled.len(), ..Audit::default() };
    for rel in compiled {
        let before = std::fs::read_to_string(pristine.join(rel)).unwrap_or_default();
        let after = std::fs::read_to_string(ported.join(rel))?;
        depscan::scan_source(&before, &mut a.before);
        let mut file_after = Counts::default();
        depscan::scan_source(&after, &mut file_after);
        if file_after.host_touch() + file_after.unsafe_total() > 0 {
            a.residual.insert(rel.clone(), file_after.clone());
        }
        a.after.add(&file_after);
        let (_, _, eq) = diff_text(&before, &after);
        a.compiled_pristine_lines += before.lines().count();
        a.compiled_unchanged_lines += eq;
        let diff = TextDiff::from_lines(&before, &after);
        let (mut removed, mut added) = (String::new(), String::new());
        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Delete => removed.push_str(change.value()),
                ChangeTag::Insert => added.push_str(change.value()),
                ChangeTag::Equal => {}
            }
        }
        a.path_method_calls_replaced += count_path_methods(&removed) - count_path_methods(&added);
        a.global_statics_removed += count_statics(&removed) - count_statics(&added);
    }
    a.host_touch_before = a.before.host_touch();
    a.host_touch_after = a.after.host_touch();
    a.unsafe_before = a.before.unsafe_total();
    a.unsafe_after = a.after.unsafe_total();
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_counts_lines() {
        assert_eq!(diff_text("a\nb\nc\n", "a\nx\nc\n"), (1, 1, 2));
        assert_eq!(split_escaped("a b\\ c  d"), vec!["a", "b c", "d"]);
        assert_eq!(count_path_methods("p.exists() && q.metadata()"), 2);
        assert_eq!(count_statics("static X: OnceLock<u8> = OnceLock::new();\nstatic Y: &str = \"\";"), 1);
    }
}
