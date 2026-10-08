//! Resolução de caminho com o comportamento do Linux 6.12 (path_resolution(7)), conferido contra o kernel
//! do host pelo teste diferencial. Os nomes das funções do kernel aparecem só como referência (ver
//! `PROVENANCE.md`).
//!
//! - `link_path_walk` ([`Walker::walk_parent`]): anda por todos os componentes menos o último. Antes de
//!   cada componente (inclusive o último) checa permissão de busca (MAY_EXEC) no diretório corrente; um
//!   componente intermediário que não é diretório dá ENOTDIR; symlinks intermediários são seguidos.
//! - `lookup_last` ([`Walker::resolve_last`]): trata o último componente, seguindo o symlink final
//!   quando pedido ou quando há barra no fim (que também exige diretório).
//! - Limites: 40 symlinks por resolução (o 41º dá ELOOP), nome acima de NAME_MAX dá ENAMETOOLONG na hora
//!   de procurar aquele nome, `..` não sobe acima da raiz do processo e atravessa montagens.

use std::sync::Arc;

use crate::fs::{Link, MagicObject};
use crate::mount::{Loc, Namespace};
use crate::perm;
use crate::types::*;

/// Tipo do último componente (`LAST_NORM`, `LAST_DOT`, `LAST_DOTDOT`, `LAST_ROOT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LastType {
    Norm,
    Dot,
    DotDot,
    Root,
}

/// Resultado do `link_path_walk`: o diretório que contém o último componente.
#[derive(Clone, Debug)]
pub struct Parent {
    pub dir: Loc,
    pub last: Vec<u8>,
    pub ltype: LastType,
    /// Havia barra depois do último componente.
    pub trailing: bool,
}

/// O que um caminho resolvido aponta.
#[derive(Clone)]
pub enum Resolved {
    Loc(Loc, Stat),
    /// Objeto do kernel atrás de um magic link (pipe em `/proc/self/fd/N`).
    Object(Arc<dyn MagicObject>),
}

impl Resolved {
    pub fn stat(&self) -> Stat {
        match self {
            Resolved::Loc(_, st) => st.clone(),
            Resolved::Object(o) => o.stat(),
        }
    }
}

impl std::fmt::Debug for Resolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Resolved::Loc(l, _) => write!(f, "Resolved::Loc({l:?})"),
            Resolved::Object(_) => write!(f, "Resolved::Object"),
        }
    }
}

/// Estado de uma resolução (o `struct nameidata`).
pub struct Walker<'a> {
    pub ns: &'a Namespace,
    pub cx: &'a Caller,
    /// Symlinks seguidos até aqui (`total_link_count`).
    pub links: u32,
}

impl<'a> Walker<'a> {
    pub fn new(ns: &'a Namespace, cx: &'a Caller) -> Walker<'a> {
        Walker { ns, cx, links: 0 }
    }

    pub fn getattr(&self, loc: &Loc) -> SysResult<Stat> {
        loc.fs().getattr(self.cx, loc.ino)
    }

    /// `may_lookup`: permissão de busca no diretório.
    fn may_lookup(&self, st: &Stat) -> SysResult<()> {
        perm::permission(&self.cx.cred, st, MAY_EXEC)
    }

    /// Procura um nome sem atravessar montagens (unlink, rmdir e rename precisam do ponto de montagem).
    pub fn lookup_nomount(&self, dir: &Loc, name: &[u8]) -> SysResult<Loc> {
        if name.len() > NAME_MAX {
            return Err(Errno::ENAMETOOLONG);
        }
        let ino = dir.fs().lookup(self.cx, dir.ino, name)?;
        Ok(Loc { mnt: dir.mnt.clone(), ino })
    }

    /// Procura um nome e desce pelas montagens em cima dele.
    pub fn lookup_child(&self, dir: &Loc, name: &[u8]) -> SysResult<Loc> {
        let l = self.lookup_nomount(dir, name)?;
        Ok(self.ns.follow_mounts(l))
    }

    /// `follow_dotdot`: o pai, sem passar da raiz do processo, subindo pela raiz de uma montagem até o
    /// ponto onde ela está montada.
    pub fn dotdot(&self, loc: &Loc) -> SysResult<Loc> {
        let mut cur = loc.clone();
        loop {
            if cur == self.cx.root {
                return Ok(cur);
            }
            if cur.is_mount_root() {
                match &cur.mnt.parent {
                    Some((pm, mp)) => {
                        cur = Loc { mnt: pm.clone(), ino: *mp };
                        continue;
                    }
                    None => return Ok(cur),
                }
            }
            let p = cur.fs().parent(cur.ino)?;
            return Ok(self.ns.follow_mounts(Loc { mnt: cur.mnt.clone(), ino: p }));
        }
    }

    fn count_link(&mut self) -> SysResult<()> {
        self.links += 1;
        if self.links > MAXSYMLINKS {
            return Err(Errno::ELOOP);
        }
        Ok(())
    }

    /// `link_path_walk` a partir de `start` (ignorado se o caminho é absoluto). O caminho já foi
    /// validado (não vazio, menor que PATH_MAX).
    pub fn walk_parent(&mut self, start: &Loc, path: &[u8]) -> SysResult<Parent> {
        let (mut cur, rest) = if path.first() == Some(&b'/') { (self.cx.root.clone(), &path[1..]) } else { (start.clone(), path) };
        let comps: Vec<&[u8]> = rest.split(|b| *b == b'/').filter(|c| !c.is_empty()).collect();
        if comps.is_empty() {
            // "/" ou só barras.
            return Ok(Parent { dir: cur, last: Vec::new(), ltype: LastType::Root, trailing: false });
        }
        let trailing = path.last() == Some(&b'/');
        let mut cur_st = self.getattr(&cur)?;
        let n = comps.len();
        for (i, comp) in comps.into_iter().enumerate() {
            self.may_lookup(&cur_st)?;
            let ltype = match comp {
                b"." => LastType::Dot,
                b".." => LastType::DotDot,
                _ => LastType::Norm,
            };
            if i + 1 == n {
                return Ok(Parent { dir: cur, last: comp.to_vec(), ltype, trailing });
            }
            match ltype {
                LastType::Dot => continue,
                LastType::DotDot => {
                    cur = self.dotdot(&cur)?;
                    cur_st = self.getattr(&cur)?;
                }
                _ => {
                    let child = self.lookup_child(&cur, comp)?;
                    let st = self.getattr(&child)?;
                    if is_lnk(st.mode) {
                        cur = self.follow_intermediate(&cur, &child)?;
                        cur_st = self.getattr(&cur)?;
                    } else {
                        cur = child;
                        cur_st = st;
                    }
                }
            }
            if !is_dir(cur_st.mode) {
                return Err(Errno::ENOTDIR);
            }
        }
        unreachable!("o laço sempre devolve no último componente")
    }

    /// `pick_link`: conta o link e faz `touch_atime` nele (relatime), antes de ler o alvo.
    pub fn pick_link(&mut self, link: &Loc) -> SysResult<Link> {
        self.count_link()?;
        link.fs().touch_atime(self.cx, link.ino);
        link.fs().follow_link(self.cx, link.ino)
    }

    /// Segue um symlink que está no meio do caminho: devolve onde ele leva.
    fn follow_intermediate(&mut self, dir: &Loc, link: &Loc) -> SysResult<Loc> {
        match self.pick_link(link)? {
            Link::Path(t) => {
                if t.is_empty() {
                    return Err(Errno::ENOENT);
                }
                let p = self.walk_parent(dir, &t)?;
                match self.resolve_last(p, true, false)? {
                    Resolved::Loc(l, _) => Ok(l),
                    Resolved::Object(_) => Err(Errno::ENOTDIR),
                }
            }
            Link::Jump(l) => Ok(self.ns.follow_mounts(l)),
            Link::Object(_) => Err(Errno::ENOTDIR),
        }
    }

    /// `lookup_last`: resolve o último componente. `follow` segue o symlink final (`LOOKUP_FOLLOW`);
    /// `must_dir` exige diretório (`LOOKUP_DIRECTORY`). Barra no fim liga os dois.
    pub fn resolve_last(&mut self, p: Parent, follow: bool, must_dir: bool) -> SysResult<Resolved> {
        let must_dir = must_dir || p.trailing;
        let loc = match p.ltype {
            LastType::Root | LastType::Dot => p.dir,
            LastType::DotDot => self.dotdot(&p.dir)?,
            LastType::Norm => {
                let child = self.lookup_child(&p.dir, &p.last)?;
                let st = self.getattr(&child)?;
                if is_lnk(st.mode) && (follow || p.trailing) {
                    match self.pick_link(&child)? {
                        Link::Path(t) => {
                            if t.is_empty() {
                                return Err(Errno::ENOENT);
                            }
                            let p2 = self.walk_parent(&p.dir, &t)?;
                            return self.resolve_last(p2, true, must_dir);
                        }
                        Link::Jump(l) => self.ns.follow_mounts(l),
                        Link::Object(o) => {
                            if must_dir {
                                return Err(Errno::ENOTDIR);
                            }
                            return Ok(Resolved::Object(o));
                        }
                    }
                } else {
                    if must_dir && !is_dir(st.mode) {
                        return Err(Errno::ENOTDIR);
                    }
                    return Ok(Resolved::Loc(child, st));
                }
            }
        };
        let st = self.getattr(&loc)?;
        if must_dir && !is_dir(st.mode) {
            return Err(Errno::ENOTDIR);
        }
        Ok(Resolved::Loc(loc, st))
    }
}

/// Caminho absoluto de `loc` visto de `root` (`d_path`), subindo pelos nomes guardados no sistema de
/// arquivos e pelas montagens. ENOENT se algum ancestral não tem mais nome.
pub fn d_path(loc: &Loc, root: &Loc) -> SysResult<Vec<u8>> {
    d_path_reach(loc, root).map(|(path, _)| path)
}

/// Como [`d_path`], e diz se a subida passou por `root`. Falso quando `loc` está fora da raiz do
/// processo (o diretório corrente de quem fez `chroot` para dentro de uma árvore que não o contém): o
/// caminho vem então a partir da raiz real, e o `getcwd` do kernel o entrega como `(unreachable)/...`.
pub fn d_path_reach(loc: &Loc, root: &Loc) -> SysResult<(Vec<u8>, bool)> {
    let mut parts: Vec<Vec<u8>> = Vec::new();
    let mut cur = loc.clone();
    let mut reached = false;
    // Limite defensivo contra ciclo (não deve acontecer: diretórios têm um pai só).
    for _ in 0..PATH_MAX {
        if cur == *root {
            reached = true;
            break;
        }
        if cur.is_mount_root() {
            match &cur.mnt.parent {
                Some((pm, mp)) => {
                    cur = Loc { mnt: pm.clone(), ino: *mp };
                    continue;
                }
                None => break,
            }
        }
        let (parent, name) = cur.fs().name_of(cur.ino).ok_or(Errno::ENOENT)?;
        parts.push(name);
        cur = Loc { mnt: cur.mnt.clone(), ino: parent };
    }
    if parts.is_empty() {
        return Ok((b"/".to_vec(), reached));
    }
    let mut out = Vec::new();
    for p in parts.iter().rev() {
        out.push(b'/');
        out.extend_from_slice(p);
    }
    Ok((out, reached))
}
