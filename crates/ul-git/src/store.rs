//! Leitura tipada de objetos sobre o repositório: commits, trees, tags, descascar até um tipo,
//! achar caminho dentro de tree e montar trees a partir do índice.

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::error::{Fail, R};
use crate::hash::{Kind, Oid};
use crate::index::{IEntry, Index};
use crate::object::{self, Commit, Tag, TreeEntry};
use crate::repo::Repo;

impl Repo {
    /// Objeto ou erro fatal.
    pub fn read_object(&self, id: &Oid) -> R<(Kind, Rc<Vec<u8>>)> {
        match self.odb.read(id) {
            Ok(Some(o)) => Ok(o),
            Ok(None) => Err(Fail::Fatal(format!("unable to read {id}"))),
            Err(e) => Err(Fail::Fatal(e)),
        }
    }

    pub fn try_read(&self, id: &Oid) -> R<Option<(Kind, Rc<Vec<u8>>)>> {
        self.odb.read(id).map_err(Fail::Fatal)
    }

    pub fn object_kind(&self, id: &Oid) -> R<Option<Kind>> {
        Ok(self.try_read(id)?.map(|(k, _)| k))
    }

    pub fn read_commit(&self, id: &Oid) -> R<Commit> {
        let (k, d) = self.read_object(id)?;
        if k != Kind::Commit {
            return Err(Fail::Fatal(format!("object {id} is a {}, not a commit", k.name())));
        }
        object::parse_commit(&d).map_err(|_| Fail::Fatal(format!("could not parse commit {id}")))
    }

    pub fn read_tree(&self, id: &Oid) -> R<Vec<TreeEntry>> {
        let (k, d) = self.read_object(id)?;
        if k != Kind::Tree {
            return Err(Fail::Fatal(format!("object {id} is a {}, not a tree", k.name())));
        }
        object::parse_tree(&d).map_err(|e| Fail::Fatal(format!("{e} ({id})")))
    }

    pub fn read_tag(&self, id: &Oid) -> R<Tag> {
        let (k, d) = self.read_object(id)?;
        if k != Kind::Tag {
            return Err(Fail::Fatal(format!("object {id} is a {}, not a tag", k.name())));
        }
        object::parse_tag(&d).map_err(|e| Fail::Fatal(format!("{e} ({id})")))
    }

    pub fn write_object(&self, kind: Kind, data: &[u8]) -> R<Oid> {
        self.odb.write(kind, data).map_err(Fail::Fatal)
    }

    /// Descasca tags e, se pedido, chega no tipo `want` (commit -> tree). `None` se não dá.
    pub fn peel(&self, id: &Oid, want: Option<Kind>) -> R<Option<Oid>> {
        let mut cur = *id;
        for _ in 0..64 {
            let Some((k, d)) = self.try_read(&cur)? else { return Ok(None) };
            if Some(k) == want {
                return Ok(Some(cur));
            }
            match k {
                Kind::Tag => {
                    let t = object::parse_tag(&d).map_err(Fail::Fatal)?;
                    cur = t.object;
                }
                Kind::Commit if want == Some(Kind::Tree) => {
                    let c = object::parse_commit(&d).map_err(Fail::Fatal)?;
                    return Ok(Some(c.tree));
                }
                _ => return Ok(if want.is_none() { Some(cur) } else { None }),
            }
        }
        Ok(None)
    }

    pub fn peel_to_commit(&self, id: &Oid) -> R<Option<Oid>> {
        self.peel(id, Some(Kind::Commit))
    }

    pub fn peel_to_tree(&self, id: &Oid) -> R<Option<Oid>> {
        self.peel(id, Some(Kind::Tree))
    }

    /// Tree de um commit (ou de algo que descasca até tree).
    pub fn tree_of(&self, id: &Oid) -> R<Oid> {
        self.peel_to_tree(id)?.ok_or_else(|| Fail::Fatal(format!("unable to read tree ({id})")))
    }

    /// Entrada de um caminho dentro de uma tree.
    pub fn tree_lookup(&self, tree: &Oid, path: &[u8]) -> R<Option<(u32, Oid)>> {
        if path.is_empty() {
            return Ok(Some((object::MODE_TREE, *tree)));
        }
        let mut cur = *tree;
        let parts: Vec<&[u8]> = path.split(|c| *c == b'/').filter(|c| !c.is_empty()).collect();
        for (i, part) in parts.iter().enumerate() {
            let entries = self.read_tree(&cur)?;
            let Some(e) = entries.iter().find(|e| e.name == *part) else { return Ok(None) };
            if i + 1 == parts.len() {
                return Ok(Some((e.mode, e.oid)));
            }
            if !e.is_tree() {
                return Ok(None);
            }
            cur = e.oid;
        }
        Ok(Some((object::MODE_TREE, cur)))
    }

    /// Arquivos (não trees) de uma tree, recursivo: caminho -> (modo, id), em ordem de caminho.
    pub fn flatten_tree(&self, tree: &Oid) -> R<BTreeMap<Vec<u8>, (u32, Oid)>> {
        let mut out = BTreeMap::new();
        self.flatten_into(tree, b"", &mut out)?;
        Ok(out)
    }

    fn flatten_into(&self, tree: &Oid, prefix: &[u8], out: &mut BTreeMap<Vec<u8>, (u32, Oid)>) -> R<()> {
        for e in self.read_tree(tree)? {
            let mut path = prefix.to_vec();
            path.extend_from_slice(&e.name);
            if e.is_tree() {
                path.push(b'/');
                self.flatten_into(&e.oid, &path, out)?;
            } else {
                out.insert(path, (e.mode, e.oid));
            }
        }
        Ok(())
    }

    /// Índice com o conteúdo de uma tree (sem `stat`), como o `read-tree`.
    pub fn index_from_tree(&self, tree: &Oid) -> R<Index> {
        let mut idx = Index { version: 2, ..Index::default() };
        for (path, (mode, oid)) in self.flatten_tree(tree)? {
            idx.entries.push(IEntry::bare(path, oid, mode));
        }
        idx.sort();
        Ok(idx)
    }

    /// Grava as trees do índice (estágio 0) e devolve a raiz. Falha com conflitos.
    pub fn write_tree_from_index(&self, idx: &Index) -> R<Oid> {
        if idx.has_conflicts() {
            return Err(Fail::Exit(128));
        }
        let entries: Vec<(&[u8], u32, Oid)> = idx.entries.iter().filter(|e| !e.intent_to_add()).map(|e| (e.path.as_slice(), e.mode, e.oid)).collect();
        self.write_tree_entries(&entries)
    }

    /// Grava trees a partir de uma lista ordenada de (caminho, modo, id).
    pub fn write_tree_entries(&self, entries: &[(&[u8], u32, Oid)]) -> R<Oid> {
        let (id, _) = self.write_subtree(entries, 0, b"")?;
        Ok(id)
    }

    fn write_subtree(&self, entries: &[(&[u8], u32, Oid)], start: usize, base: &[u8]) -> R<(Oid, usize)> {
        let mut out: Vec<TreeEntry> = Vec::new();
        let mut i = start;
        while i < entries.len() {
            let (path, mode, oid) = entries[i];
            if !path.starts_with(base) {
                break;
            }
            let rest = &path[base.len()..];
            match rest.iter().position(|c| *c == b'/') {
                None => {
                    out.push(TreeEntry { mode, name: rest.to_vec(), oid });
                    i += 1;
                }
                Some(s) => {
                    let mut sub = base.to_vec();
                    sub.extend_from_slice(&rest[..=s]);
                    let (id, next) = self.write_subtree(entries, i, &sub)?;
                    out.push(TreeEntry { mode: object::MODE_TREE, name: rest[..s].to_vec(), oid: id });
                    i = next;
                }
            }
        }
        object::sort_tree(&mut out);
        let data = object::encode_tree(&out);
        let id = self.write_object(Kind::Tree, &data)?;
        Ok((id, i))
    }

    /// Pais de um commit.
    pub fn parents(&self, id: &Oid) -> R<Vec<Oid>> {
        Ok(self.read_commit(id)?.parents)
    }

    /// Abreviação única com pelo menos `min` dígitos.
    pub fn abbrev(&self, id: &Oid, min: usize) -> String {
        let hex = id.hex();
        if min >= 40 {
            return hex;
        }
        let mut len = min.max(4);
        let mut cands = Vec::new();
        self.odb.find_prefix(&hex.as_bytes()[..len], &mut cands);
        cands.retain(|c| c != id);
        while len < 40 && !cands.is_empty() {
            len += 1;
            let p = &hex.as_bytes()[..len];
            cands.retain(|c| c.hex_starts_with(p));
        }
        hex[..len].to_string()
    }

    /// Abreviação com o tamanho padrão do repositório.
    pub fn abbrev_default(&self, id: &Oid) -> String {
        self.abbrev(id, self.abbrev_len())
    }
}
