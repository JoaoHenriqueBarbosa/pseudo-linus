use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Bytes;

/// Arquivos maiores que isso entram no retrato só com tamanho e sha256.
pub const CAPTURE_LIMIT: u64 = 1 << 20;

/// Uma entrada da árvore. Caminhos são relativos ao diretório do caso, separados por `/`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Entry {
    File {
        mode: u32,
        size: u64,
        sha256: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<Bytes>,
    },
    Dir {
        mode: u32,
    },
    Symlink {
        target: String,
    },
}

impl Entry {
    pub fn file(data: impl Into<Vec<u8>>, mode: u32) -> Entry {
        let data = data.into();
        let size = data.len() as u64;
        let sha256 = sha256_hex(&data);
        let data = (size <= CAPTURE_LIMIT).then_some(Bytes(data));
        Entry::File { mode, size, sha256, data }
    }

    pub fn dir(mode: u32) -> Entry {
        Entry::Dir { mode }
    }

    pub fn symlink(target: impl Into<String>) -> Entry {
        Entry::Symlink { target: target.into() }
    }

    /// Conteúdo do arquivo, quando capturado.
    pub fn data(&self) -> Option<&[u8]> {
        match self {
            Entry::File { data: Some(d), .. } => Some(d.as_slice()),
            _ => None,
        }
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Árvore de arquivos em memória.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MemTree {
    pub entries: BTreeMap<String, Entry>,
}

impl MemTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insere uma entrada, criando os diretórios intermediários que faltarem (modo 0755).
    pub fn insert(&mut self, path: &str, entry: Entry) {
        let path = normalize(path);
        let mut prefix = String::new();
        let parts: Vec<&str> = path.split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            self.entries.entry(prefix.clone()).or_insert(Entry::dir(0o755));
        }
        self.entries.insert(path, entry);
    }

    pub fn get(&self, path: &str) -> Option<&Entry> {
        self.entries.get(&normalize(path))
    }

    pub fn read(&self, path: &str) -> Option<&[u8]> {
        self.get(path).and_then(Entry::data)
    }

    /// Lê um diretório real e devolve o retrato dele (sem o próprio diretório raiz).
    pub fn capture(root: &Path) -> Result<MemTree> {
        use std::os::unix::fs::PermissionsExt;
        let mut tree = MemTree::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let read = std::fs::read_dir(&dir).with_context(|| format!("read_dir {}", dir.display()))?;
            for item in read {
                let item = item?;
                let path = item.path();
                let rel = path
                    .strip_prefix(root)
                    .expect("caminho dentro da raiz")
                    .to_string_lossy()
                    .into_owned();
                let meta = std::fs::symlink_metadata(&path)?;
                let mode = meta.permissions().mode() & 0o7777;
                let ft = meta.file_type();
                if ft.is_symlink() {
                    let target = std::fs::read_link(&path)?.to_string_lossy().into_owned();
                    tree.entries.insert(rel, Entry::symlink(target));
                } else if ft.is_dir() {
                    tree.entries.insert(rel, Entry::dir(mode));
                    stack.push(path);
                } else if ft.is_file() {
                    let data = std::fs::read(&path)?;
                    tree.entries.insert(rel, Entry::file(data, mode));
                } else {
                    // FIFO, socket, device: registra como arquivo vazio com modo especial.
                    tree.entries.insert(rel, Entry::file(Vec::new(), mode | 0o170000));
                }
            }
        }
        Ok(tree)
    }

    /// Materializa a árvore num diretório real, com mtime fixo.
    pub fn materialize(&self, root: &Path, mtime: std::time::SystemTime) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(root)?;
        // Diretórios primeiro (BTreeMap já ordena pai antes de filho), depois arquivos e symlinks.
        for (rel, entry) in &self.entries {
            let path = root.join(rel);
            match entry {
                Entry::Dir { .. } => std::fs::create_dir_all(&path)?,
                Entry::File { data, size, .. } => {
                    let bytes = match data {
                        Some(d) => d.0.clone(),
                        None => anyhow::bail!("{rel}: arquivo de {size} bytes sem conteúdo capturado"),
                    };
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&path, bytes)?;
                }
                Entry::Symlink { target } => {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::os::unix::fs::symlink(target, &path)?;
                }
            }
        }
        let times = std::fs::FileTimes::new().set_modified(mtime).set_accessed(mtime);
        // Modos e mtimes dos mais fundos pros mais rasos, pra não perder a permissão de escrita antes da hora.
        for (rel, entry) in self.entries.iter().rev() {
            let path = root.join(rel);
            match entry {
                Entry::File { mode, .. } | Entry::Dir { mode } => {
                    let f = std::fs::File::open(&path)?;
                    f.set_times(times)?;
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(*mode))?;
                }
                Entry::Symlink { .. } => {}
            }
        }
        Ok(())
    }

    /// Lista legível das diferenças entre duas árvores.
    pub fn diff(&self, other: &MemTree) -> Vec<String> {
        let mut out = Vec::new();
        for (path, a) in &self.entries {
            match other.entries.get(path) {
                None => out.push(format!("-{path}")),
                Some(b) if !same_entry(a, b) => out.push(format!("~{path}: {} != {}", describe(a), describe(b))),
                Some(_) => {}
            }
        }
        for path in other.entries.keys() {
            if !self.entries.contains_key(path) {
                out.push(format!("+{path}"));
            }
        }
        out
    }
}

fn same_entry(a: &Entry, b: &Entry) -> bool {
    match (a, b) {
        (
            Entry::File { mode: ma, sha256: sa, .. },
            Entry::File { mode: mb, sha256: sb, .. },
        ) => ma == mb && sa == sb,
        _ => a == b,
    }
}

fn describe(e: &Entry) -> String {
    match e {
        Entry::File { mode, size, sha256, .. } => format!("file {mode:o} {size}B {}", &sha256[..12]),
        Entry::Dir { mode } => format!("dir {mode:o}"),
        Entry::Symlink { target } => format!("symlink -> {target}"),
    }
}

fn normalize(path: &str) -> String {
    path.split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_creates_parents_and_diff_detects_changes() {
        let mut a = MemTree::new();
        a.insert("d/e/f.txt", Entry::file("x", 0o644));
        assert!(matches!(a.get("d"), Some(Entry::Dir { .. })));
        assert!(matches!(a.get("d/e"), Some(Entry::Dir { .. })));
        let mut b = a.clone();
        assert!(a.diff(&b).is_empty());
        b.insert("d/e/f.txt", Entry::file("y", 0o644));
        b.insert("g", Entry::symlink("d"));
        let d = a.diff(&b);
        assert_eq!(d.len(), 2, "{d:?}");
    }
}
