//! depscan: mede, no código-fonte, quanto uma crate (e a árvore de dependências dela) toca o host e
//! quanto unsafe ela tem. É o que dá a categoria objetiva de cada candidato:
//!
//! - **(a)** não toca o host: nenhum `std::fs`, `std::process`, `std::net`, `std::env`, `std::os`,
//!   `libc`, `nix`, `rustix`, nem `print!`/`stdout()`;
//! - **(b)** toca o host em Rust (precisaria de fork pra rodar sobre o nosso `Ctx`);
//! - **(c)** compila ou linka código C (`links = ...` ou build-dependency `cc`/`cmake`/`bindgen`/`pkg-config`).
//!
//! Limitações conhecidas: código atrás de `cfg` de outra plataforma também é contado; código dentro de
//! macros é varrido por tokens, não por AST.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use cargo_metadata::{DependencyKind, Metadata, MetadataCommand, Package, PackageId};
use serde::{Deserialize, Serialize};
use syn::visit::Visit;

/// Contadores por crate.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub files: usize,
    pub parse_errors: usize,
    pub unsafe_blocks: usize,
    pub unsafe_fns: usize,
    pub unsafe_impls: usize,
    pub unsafe_traits: usize,
    pub extern_blocks: usize,
    pub std_fs: usize,
    pub std_process: usize,
    pub std_net: usize,
    pub std_env: usize,
    pub std_os: usize,
    pub std_thread: usize,
    pub host_stdio: usize,
    pub libc: usize,
    pub nix: usize,
    pub rustix: usize,
}

impl Counts {
    pub fn add(&mut self, o: &Counts) {
        self.files += o.files;
        self.parse_errors += o.parse_errors;
        self.unsafe_blocks += o.unsafe_blocks;
        self.unsafe_fns += o.unsafe_fns;
        self.unsafe_impls += o.unsafe_impls;
        self.unsafe_traits += o.unsafe_traits;
        self.extern_blocks += o.extern_blocks;
        self.std_fs += o.std_fs;
        self.std_process += o.std_process;
        self.std_net += o.std_net;
        self.std_env += o.std_env;
        self.std_os += o.std_os;
        self.std_thread += o.std_thread;
        self.host_stdio += o.host_stdio;
        self.libc += o.libc;
        self.nix += o.nix;
        self.rustix += o.rustix;
    }

    /// Total de pontos que tocam o host diretamente.
    pub fn host_touch(&self) -> usize {
        self.std_fs
            + self.std_process
            + self.std_net
            + self.std_env
            + self.std_os
            + self.host_stdio
            + self.libc
            + self.nix
            + self.rustix
    }

    pub fn unsafe_total(&self) -> usize {
        self.unsafe_blocks + self.unsafe_fns + self.unsafe_impls + self.unsafe_traits + self.extern_blocks
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Category {
    A,
    B,
    C,
}

impl Category {
    pub fn letter(self) -> &'static str {
        match self {
            Category::A => "a",
            Category::B => "b",
            Category::C => "c",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackageScan {
    pub name: String,
    pub version: String,
    pub manifest_dir: PathBuf,
    pub counts: Counts,
    pub links: Option<String>,
    pub c_build_deps: Vec<String>,
    pub has_build_rs: bool,
    pub category: Category,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeScan {
    pub root: PackageScan,
    /// Dependências normais transitivas (sem dev-dependencies), sem a raiz.
    pub deps: Vec<PackageScan>,
    pub totals: Counts,
    /// Pior categoria da árvore inteira.
    pub tree_category: Category,
    /// Dependências que tocam o host, com o total de pontos.
    pub host_touching_deps: BTreeMap<String, usize>,
    /// Dependências com C.
    pub c_deps: Vec<String>,
}

const C_BUILD_TOOLS: &[&str] = &["cc", "cmake", "bindgen", "pkg-config", "autocfg-c", "vcpkg"];

/// Varre `package` como dependência resolvida do manifesto em `manifest_path`.
pub fn scan(manifest_path: &Path, package: &str) -> Result<TreeScan> {
    // Resolve só pra plataforma do host, pra não contar dependências de Windows/macOS.
    let mut cmd = MetadataCommand::new();
    cmd.manifest_path(manifest_path);
    if let Some(triple) = host_triple() {
        cmd.other_options(vec!["--filter-platform".to_string(), triple]);
    }
    let meta = cmd
        .exec()
        .with_context(|| format!("cargo metadata {}", manifest_path.display()))?;
    let root = find_package(&meta, package)?;
    let ids = transitive_normal_deps(&meta, &root.id)?;
    let mut scans: BTreeMap<PackageId, PackageScan> = BTreeMap::new();
    for id in ids.iter().chain(std::iter::once(&root.id)) {
        let pkg = &meta[id];
        scans.insert(id.clone(), scan_package(&meta, pkg)?);
    }
    let root_scan = scans.remove(&root.id).expect("raiz");
    let deps: Vec<PackageScan> = scans.into_values().collect();
    let mut totals = root_scan.counts.clone();
    let mut tree_category = root_scan.category;
    let mut host_touching_deps = BTreeMap::new();
    let mut c_deps = Vec::new();
    for d in &deps {
        totals.add(&d.counts);
        tree_category = tree_category.max(d.category);
        if d.counts.host_touch() > 0 {
            host_touching_deps.insert(format!("{} {}", d.name, d.version), d.counts.host_touch());
        }
        if d.category == Category::C {
            c_deps.push(format!("{} {}", d.name, d.version));
        }
    }
    Ok(TreeScan { root: root_scan, deps, totals, tree_category, host_touching_deps, c_deps })
}

/// Triple do host, lido do `rustc -vV` (linha `host:`).
fn host_triple() -> Option<String> {
    let out = std::process::Command::new("rustc").arg("-vV").output().ok()?;
    String::from_utf8(out.stdout)
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("host: ").map(str::to_string))
}

fn find_package<'a>(meta: &'a Metadata, name: &str) -> Result<&'a Package> {
    let found: Vec<&Package> = meta.packages.iter().filter(|p| p.name.as_str() == name).collect();
    match found.as_slice() {
        [p] => Ok(p),
        [] => bail!("pacote {name} não está na árvore"),
        many => {
            // Várias versões: pega a mais nova.
            Ok(many.iter().max_by(|a, b| a.version.cmp(&b.version)).copied().expect("não vazio"))
        }
    }
}

fn transitive_normal_deps(meta: &Metadata, root: &PackageId) -> Result<BTreeSet<PackageId>> {
    let resolve = meta.resolve.as_ref().context("metadata sem resolve")?;
    let nodes: BTreeMap<&PackageId, &cargo_metadata::Node> = resolve.nodes.iter().map(|n| (&n.id, n)).collect();
    let mut seen = BTreeSet::new();
    let mut stack = vec![root.clone()];
    while let Some(id) = stack.pop() {
        let node = nodes.get(&id).context("nó ausente no resolve")?;
        for dep in &node.deps {
            let normal = dep.dep_kinds.iter().any(|k| k.kind == DependencyKind::Normal);
            if normal && seen.insert(dep.pkg.clone()) {
                stack.push(dep.pkg.clone());
            }
        }
    }
    seen.remove(root);
    Ok(seen)
}

fn scan_package(meta: &Metadata, pkg: &Package) -> Result<PackageScan> {
    let manifest_dir = pkg.manifest_path.parent().expect("dir do manifesto").as_std_path().to_path_buf();
    let mut counts = Counts::default();
    let src = manifest_dir.join("src");
    let roots: Vec<PathBuf> = if src.exists() {
        vec![src]
    } else {
        pkg.targets.iter().map(|t| t.src_path.as_std_path().to_path_buf()).collect()
    };
    for root in roots {
        for entry in walkdir::WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().is_some_and(|x| x == "rs") && entry.file_type().is_file() {
                scan_file(path, &mut counts);
            }
        }
    }
    let has_build_rs = pkg.targets.iter().any(|t| t.kind.iter().any(|k| k.to_string() == "custom-build"));
    // Só build-dependencies que o resolve realmente ativou pra esta plataforma: uma dependência `cc`
    // opcional e desligada, ou só de outra plataforma, não torna a crate categoria (c).
    let c_build_deps: Vec<String> = meta
        .resolve
        .as_ref()
        .and_then(|r| r.nodes.iter().find(|n| n.id == pkg.id))
        .map(|node| {
            node.deps
                .iter()
                .filter(|d| d.dep_kinds.iter().any(|k| k.kind == DependencyKind::Build))
                .map(|d| meta[&d.pkg].name.to_string())
                .filter(|name| C_BUILD_TOOLS.contains(&name.as_str()))
                .collect()
        })
        .unwrap_or_default();
    let links = pkg.links.clone();
    let category = if links.is_some() || !c_build_deps.is_empty() {
        Category::C
    } else if counts.host_touch() > 0 {
        Category::B
    } else {
        Category::A
    };
    Ok(PackageScan {
        name: pkg.name.to_string(),
        version: pkg.version.to_string(),
        manifest_dir,
        counts,
        links,
        c_build_deps,
        has_build_rs,
        category,
    })
}

/// Varre um arquivo `.rs` e soma nos contadores.
pub fn scan_file(path: &Path, counts: &mut Counts) {
    counts.files += 1;
    let Ok(text) = std::fs::read_to_string(path) else {
        counts.parse_errors += 1;
        return;
    };
    scan_source(&text, counts);
}

/// Varre código-fonte Rust e soma nos contadores.
pub fn scan_source(text: &str, counts: &mut Counts) {
    match syn::parse_file(text) {
        Ok(file) => {
            let mut v = Visitor { counts };
            v.visit_file(&file);
        }
        Err(_) => {
            counts.parse_errors += 1;
            // Fallback por tokens, quando o arquivo nem tokeniza fica de fora.
            if let Ok(ts) = text.parse::<proc_macro2::TokenStream>() {
                scan_tokens(ts, counts);
            }
        }
    }
}

struct Visitor<'a> {
    counts: &'a mut Counts,
}

/// Tipos de `std::net` que fazem I/O ou DNS (os demais, como `IpAddr`, são só dados).
const NET_IO: &[&str] = &["TcpStream", "TcpListener", "UdpSocket", "ToSocketAddrs"];

fn classify_path(segments: &[String], counts: &mut Counts) {
    let s: Vec<&str> = segments.iter().map(String::as_str).collect();
    match s.as_slice() {
        ["std" | "core" | "alloc", "fs", ..] => counts.std_fs += 1,
        ["std", "process", ..] => counts.std_process += 1,
        ["std", "net"] => counts.std_net += 1,
        ["std", "net", rest @ ..] if rest.iter().any(|x| NET_IO.contains(x)) => counts.std_net += 1,
        ["std", "env", ..] => counts.std_env += 1,
        // std::os::{unix,linux,windows}::{fs,process,net} acessam o host; ffi, raw, fd e prelude são só tipos.
        ["std", "os", _, "fs" | "process" | "net" | "io", ..] => counts.std_os += 1,
        ["std", "thread", ..] => counts.std_thread += 1,
        ["std", "io", "stdout" | "stderr" | "stdin", ..] => counts.host_stdio += 1,
        ["libc", ..] => counts.libc += 1,
        ["nix", ..] => counts.nix += 1,
        ["rustix", ..] => counts.rustix += 1,
        _ => {}
    }
}

fn use_tree_paths(tree: &syn::UseTree, prefix: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    match tree {
        syn::UseTree::Path(p) => {
            prefix.push(p.ident.to_string());
            use_tree_paths(&p.tree, prefix, out);
            prefix.pop();
        }
        syn::UseTree::Name(n) => {
            let mut full = prefix.clone();
            full.push(n.ident.to_string());
            out.push(full);
        }
        syn::UseTree::Rename(r) => {
            let mut full = prefix.clone();
            full.push(r.ident.to_string());
            out.push(full);
        }
        syn::UseTree::Glob(_) => out.push(prefix.clone()),
        syn::UseTree::Group(g) => {
            for t in &g.items {
                use_tree_paths(t, prefix, out);
            }
        }
    }
}

/// `#[test]`, `#[cfg(test)]` e variantes: código de teste não conta como acoplamento.
fn is_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if a.path().is_ident("test") {
            return true;
        }
        if !a.path().is_ident("cfg") {
            return false;
        }
        let Ok(list) = a.meta.require_list() else { return false };
        let has_test = list.tokens.clone().into_iter().any(|t| matches!(t, proc_macro2::TokenTree::Ident(ref i) if i == "test"));
        let negated = list.tokens.to_string().contains("not");
        has_test && !negated
    })
}

impl<'ast> Visit<'ast> for Visitor<'_> {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        if !is_test_only(&i.attrs) {
            syn::visit::visit_item_mod(self, i);
        }
    }

    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        if !is_test_only(&i.attrs) {
            syn::visit::visit_item_fn(self, i);
        }
    }

    fn visit_expr_unsafe(&mut self, i: &'ast syn::ExprUnsafe) {
        self.counts.unsafe_blocks += 1;
        syn::visit::visit_expr_unsafe(self, i);
    }

    fn visit_signature(&mut self, i: &'ast syn::Signature) {
        if i.unsafety.is_some() {
            self.counts.unsafe_fns += 1;
        }
        syn::visit::visit_signature(self, i);
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        if i.unsafety.is_some() {
            self.counts.unsafe_impls += 1;
        }
        syn::visit::visit_item_impl(self, i);
    }

    fn visit_item_trait(&mut self, i: &'ast syn::ItemTrait) {
        if i.unsafety.is_some() {
            self.counts.unsafe_traits += 1;
        }
        syn::visit::visit_item_trait(self, i);
    }

    fn visit_item_foreign_mod(&mut self, i: &'ast syn::ItemForeignMod) {
        self.counts.extern_blocks += 1;
        syn::visit::visit_item_foreign_mod(self, i);
    }

    fn visit_item_use(&mut self, i: &'ast syn::ItemUse) {
        let mut out = Vec::new();
        use_tree_paths(&i.tree, &mut Vec::new(), &mut out);
        for p in out {
            let p: Vec<String> = p.into_iter().filter(|s| s != "crate" && s != "self").collect();
            classify_path(&p, self.counts);
        }
    }

    fn visit_path(&mut self, i: &'ast syn::Path) {
        let segments: Vec<String> = i.segments.iter().map(|s| s.ident.to_string()).collect();
        classify_path(&segments, self.counts);
        syn::visit::visit_path(self, i);
    }

    fn visit_macro(&mut self, i: &'ast syn::Macro) {
        if let Some(last) = i.path.segments.last() {
            let name = last.ident.to_string();
            if matches!(name.as_str(), "print" | "println" | "eprint" | "eprintln" | "dbg") {
                self.counts.host_stdio += 1;
            }
        }
        scan_tokens(i.tokens.clone(), self.counts);
        syn::visit::visit_macro(self, i);
    }
}

/// Varredura por tokens (corpo de macro, ou arquivo que o syn não parseia).
fn scan_tokens(ts: proc_macro2::TokenStream, counts: &mut Counts) {
    use proc_macro2::TokenTree;
    let tokens: Vec<TokenTree> = ts.into_iter().collect();
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            TokenTree::Group(g) => scan_tokens(g.stream(), counts),
            TokenTree::Ident(id) => {
                let name = id.to_string();
                if name == "unsafe" {
                    counts.unsafe_blocks += 1;
                } else {
                    // Lê uma sequência a :: b :: c a partir daqui.
                    let mut segs = vec![name];
                    let mut j = i + 1;
                    while j + 2 < tokens.len() + 1 {
                        let colon = matches!((&tokens.get(j), &tokens.get(j + 1)),
                            (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b))) if a.as_char() == ':' && b.as_char() == ':');
                        if !colon {
                            break;
                        }
                        match tokens.get(j + 2) {
                            Some(TokenTree::Ident(next)) => {
                                segs.push(next.to_string());
                                j += 3;
                            }
                            _ => break,
                        }
                    }
                    if segs.len() > 1 {
                        classify_path(&segs, counts);
                        i = j;
                        continue;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_host_and_unsafe() {
        let src = r#"
            use std::fs::{self, File};
            use std::io::{stdout, Write};
            fn a() { let _ = std::process::Command::new("x"); println!("hi"); }
            unsafe fn b() {}
            unsafe impl Send for X {}
            fn c() { unsafe { b() } }
            extern "C" { fn d(); }
            macro_rules! m { () => { std::env::var("X") } }
            fn e() { libc::getpid(); }
        "#;
        let mut c = Counts::default();
        scan_source(src, &mut c);
        assert_eq!(c.std_fs, 2, "{c:?}");
        assert_eq!(c.std_process, 1, "{c:?}");
        assert_eq!(c.std_env, 1, "{c:?}");
        assert!(c.host_stdio >= 2, "{c:?}");
        assert_eq!(c.unsafe_fns, 1, "{c:?}");
        assert_eq!(c.unsafe_impls, 1);
        assert_eq!(c.unsafe_blocks, 1);
        assert_eq!(c.extern_blocks, 1);
        assert_eq!(c.libc, 1);
    }
}
