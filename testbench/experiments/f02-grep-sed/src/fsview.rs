//! Visão somente leitura da árvore de um caso (`harness::MemTree`), com resolução de symlink, pro
//! grep montado em memória. Caminhos relativos ao diretório do caso, como o programa veria.

use harness::{Entry, MemTree};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Errno {
    NotFound,
    IsDir,
    Loop,
}

impl Errno {
    pub fn message(self) -> &'static str {
        match self {
            Errno::NotFound => "No such file or directory",
            Errno::IsDir => "Is a directory",
            Errno::Loop => "Too many levels of symbolic links",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
}

pub struct FsView<'a> {
    pub tree: &'a MemTree,
}

/// Normaliza um caminho relativo ao caso (`./a/../b` vira `b`); `None` se sair da raiz.
pub fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

impl<'a> FsView<'a> {
    pub fn new(tree: &'a MemTree) -> Self {
        FsView { tree }
    }

    /// Resolve symlinks (todos os componentes) e devolve o caminho canônico e o tipo.
    pub fn resolve(&self, path: &str, follow_last: bool) -> Result<(String, Option<String>, Kind), Errno> {
        let mut pending: Vec<String> = path.split('/').filter(|p| !p.is_empty() && *p != ".").map(str::to_string).collect();
        pending.reverse();
        let mut cur: Vec<String> = Vec::new();
        let mut hops = 0;
        while let Some(part) = pending.pop() {
            if part == ".." {
                cur.pop();
                continue;
            }
            cur.push(part);
            let key = cur.join("/");
            match self.tree.entries.get(&key) {
                None => return Err(Errno::NotFound),
                Some(Entry::Symlink { target }) => {
                    if pending.is_empty() && !follow_last {
                        return Ok((key, Some(target.clone()), Kind::File));
                    }
                    hops += 1;
                    if hops > 40 {
                        return Err(Errno::Loop);
                    }
                    cur.pop();
                    if target.starts_with('/') {
                        return Err(Errno::NotFound);
                    }
                    for t in target.split('/').rev().filter(|p| !p.is_empty() && *p != ".") {
                        pending.push(t.to_string());
                    }
                }
                Some(Entry::Dir { .. }) => {}
                Some(Entry::File { .. }) => {
                    if !pending.is_empty() {
                        return Err(Errno::NotFound);
                    }
                }
            }
        }
        let key = cur.join("/");
        if key.is_empty() {
            return Ok((key, None, Kind::Dir));
        }
        match self.tree.entries.get(&key) {
            Some(Entry::Dir { .. }) => Ok((key, None, Kind::Dir)),
            Some(Entry::File { .. }) => Ok((key, None, Kind::File)),
            _ => Err(Errno::NotFound),
        }
    }

    /// Lê um arquivo (seguindo symlinks).
    pub fn read(&self, path: &str) -> Result<&'a [u8], Errno> {
        let (key, _, kind) = self.resolve(path, true)?;
        if kind == Kind::Dir {
            return Err(Errno::IsDir);
        }
        self.tree.entries.get(&key).and_then(Entry::data).ok_or(Errno::NotFound)
    }

    /// Entradas diretas de um diretório (já resolvido), em ordem de nome.
    pub fn list(&self, dir_key: &str) -> Vec<(String, bool)> {
        let prefix = if dir_key.is_empty() { String::new() } else { format!("{dir_key}/") };
        self.tree
            .entries
            .iter()
            .filter_map(|(k, e)| {
                let rest = k.strip_prefix(&prefix)?;
                (!rest.is_empty() && !rest.contains('/')).then(|| (rest.to_string(), matches!(e, Entry::Symlink { .. })))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_symlinks_and_lists() {
        let mut t = MemTree::new();
        t.insert("d/a.txt", Entry::file("A", 0o644));
        t.insert("d/link.txt", Entry::symlink("../out.txt"));
        t.insert("out.txt", Entry::file("O", 0o644));
        t.insert("d/sub/b", Entry::file("B", 0o644));
        let v = FsView::new(&t);
        assert_eq!(v.read("d/link.txt").unwrap(), b"O");
        assert_eq!(v.read("./d/sub/../a.txt").unwrap(), b"A");
        assert_eq!(v.read("d"), Err(Errno::IsDir));
        assert_eq!(v.read("nope"), Err(Errno::NotFound));
        let names: Vec<String> = v.list("d").into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["a.txt", "link.txt", "sub"]);
        assert_eq!(normalize("./a/../b"), Some("b".into()));
    }
}
