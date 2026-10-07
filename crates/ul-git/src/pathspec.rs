//! Pathspecs: caminhos relativos ao cwd, curingas (`*` cruza `/`, como no git), magias curtas
//! (`:/`, `:!`, `:^`) e longas (`:(top,exclude,glob,icase,literal)`).

use crate::error::{Fail, R};
use crate::os;
use crate::wildmatch::{self, CASEFOLD, PATHNAME};

pub const TOP: u32 = 1;
pub const LITERAL: u32 = 2;
pub const GLOB: u32 = 4;
pub const ICASE: u32 = 8;
pub const EXCLUDE: u32 = 16;

#[derive(Clone, Debug)]
pub struct Item {
    /// Como o usuário escreveu.
    pub orig: Vec<u8>,
    /// Relativo ao topo da árvore (pode terminar em `/`; vazio = tudo).
    pub path: Vec<u8>,
    pub magic: u32,
    /// Quantos bytes do começo não têm curinga.
    pub nowild: usize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Hit {
    Fnmatch,
    Recursive,
    Exact,
}

#[derive(Clone, Debug, Default)]
pub struct Pathspec {
    pub items: Vec<Item>,
}

/// Normaliza um caminho relativo (`a/./b/../c` -> `a/c`); `None` se sobe acima do topo.
pub fn normalize_rel(p: &[u8]) -> Option<Vec<u8>> {
    let trailing = p.ends_with(b"/");
    let mut parts: Vec<&[u8]> = Vec::new();
    for c in p.split(|b| *b == b'/') {
        match c {
            b"" | b"." => {}
            b".." => {
                parts.pop()?;
            }
            _ => parts.push(c),
        }
    }
    let mut out = parts.join(&b'/');
    if trailing && !out.is_empty() {
        out.push(b'/');
    }
    Some(out)
}

pub struct Opts {
    pub literal: bool,
    pub glob: bool,
    pub noglob: bool,
    pub icase: bool,
}

impl Pathspec {

    /// Lê os argumentos (`prefix` é o diretório atual relativo ao topo, com `/`).
    pub fn parse(args: &[Vec<u8>], prefix: &[u8], top: &[u8], opts: &Opts) -> R<Pathspec> {
        let mut items = Vec::new();
        for a in args {
            items.push(parse_item(a, prefix, top, opts)?);
        }
        Ok(Pathspec { items })
    }

    pub fn simple(paths: Vec<Vec<u8>>) -> Pathspec {
        Pathspec { items: paths.into_iter().map(|p| Item { nowild: p.len(), orig: p.clone(), path: p, magic: LITERAL }).collect() }
    }

    /// Melhor casamento de `name` (relativo ao topo) com algum item positivo, se nenhum item de
    /// exclusão casar. `seen` marca os itens que casaram.
    pub fn matches(&self, name: &[u8], is_dir: bool, seen: Option<&mut Vec<bool>>) -> Option<Hit> {
        if self.items.is_empty() {
            return Some(Hit::Recursive);
        }
        let mut best: Option<Hit> = None;
        let mut hits: Vec<usize> = Vec::new();
        let mut any_positive = false;
        for (i, it) in self.items.iter().enumerate() {
            if it.magic & EXCLUDE != 0 {
                continue;
            }
            any_positive = true;
            if let Some(h) = match_item(it, name, is_dir) {
                hits.push(i);
                best = Some(best.map_or(h, |b| b.max(h)));
            }
        }
        if !any_positive {
            best = Some(Hit::Recursive);
        }
        let best = best?;
        for it in self.items.iter().filter(|i| i.magic & EXCLUDE != 0) {
            if match_item(it, name, is_dir).is_some() {
                return None;
            }
        }
        if let Some(s) = seen {
            if s.len() < self.items.len() {
                s.resize(self.items.len(), false);
            }
            for i in hits {
                s[i] = true;
            }
        }
        Some(best)
    }

    pub fn matches_simple(&self, name: &[u8]) -> bool {
        self.matches(name, false, None).is_some()
    }

    /// Pode haver algo dentro do diretório `dir` (sem barra no fim) que case?
    pub fn may_match_under(&self, dir: &[u8]) -> bool {
        if self.items.is_empty() {
            return true;
        }
        let mut d = dir.to_vec();
        d.push(b'/');
        let mut any_positive = false;
        for it in &self.items {
            if it.magic & EXCLUDE != 0 {
                continue;
            }
            any_positive = true;
            let lit = &it.path[..it.nowild.min(it.path.len())];
            if it.path.is_empty() {
                return true;
            }
            // O item está dentro do diretório, ou o diretório está dentro do item.
            if lit.len() >= d.len() {
                if lit.starts_with(&d) || (it.magic & ICASE != 0 && lit[..d.len()].eq_ignore_ascii_case(&d)) {
                    return true;
                }
            } else if d.starts_with(lit) {
                if it.nowild < it.path.len() {
                    return true;
                }
                // Item literal mais curto: casa se é o próprio diretório ou um pai.
                if lit.ends_with(b"/") || d[lit.len()] == b'/' || lit.len() + 1 == d.len() {
                    return true;
                }
            }
            if lit == dir {
                return true;
            }
        }
        !any_positive
    }

    /// Itens positivos que não casaram com nada (pro "did not match any file(s)").
    pub fn unmatched<'a>(&'a self, seen: &[bool]) -> Vec<&'a Item> {
        self.items.iter().enumerate().filter(|(i, it)| it.magic & EXCLUDE == 0 && !seen.get(*i).copied().unwrap_or(false)).map(|(_, it)| it).collect()
    }
}

fn parse_item(arg: &[u8], prefix: &[u8], top: &[u8], opts: &Opts) -> R<Item> {
    let mut magic = 0;
    let mut rest: &[u8] = arg;
    if !opts.literal && arg.first() == Some(&b':') {
        if arg.get(1) == Some(&b'(') {
            let close = arg.iter().position(|c| *c == b')').ok_or_else(|| Fail::Fatal(format!("Missing ')' at the end of pathspec magic in '{}'", os::lossy(arg))))?;
            for word in arg[2..close].split(|c| *c == b',') {
                magic |= match word {
                    b"top" => TOP,
                    b"literal" => LITERAL,
                    b"glob" => GLOB,
                    b"icase" => ICASE,
                    b"exclude" => EXCLUDE,
                    b"" => 0,
                    w if w.starts_with(b"attr:") => return Err(Fail::Fatal("attr spec must not be empty".into())),
                    w if w.starts_with(b"prefix:") => 0,
                    w => return Err(Fail::Fatal(format!("Invalid pathspec magic '{}' in '{}'", os::lossy(w), os::lossy(arg)))),
                };
            }
            rest = &arg[close + 1..];
        } else {
            let mut i = 1;
            while i < arg.len() {
                match arg[i] {
                    b'/' => magic |= TOP,
                    b'!' | b'^' => magic |= EXCLUDE,
                    b':' => {
                        i += 1;
                        break;
                    }
                    _ => break,
                }
                i += 1;
            }
            rest = &arg[i..];
        }
    }
    if opts.literal {
        magic |= LITERAL;
    }
    if opts.glob && magic & LITERAL == 0 {
        magic |= GLOB;
    }
    if opts.icase {
        magic |= ICASE;
    }
    if opts.noglob && magic & GLOB == 0 {
        magic |= LITERAL;
    }
    let joined = if magic & TOP != 0 { rest.to_vec() } else { [prefix, rest].concat() };
    let path = match normalize_rel(&joined) {
        Some(p) => p,
        None => {
            return Err(Fail::Fatal(format!(
                "{}: '{}' is outside repository at '{}'",
                os::lossy(arg),
                os::lossy(arg),
                os::lossy(top)
            )));
        }
    };
    let nowild = if magic & LITERAL != 0 { path.len() } else { wildmatch::literal_prefix_len(&path) };
    Ok(Item { orig: arg.to_vec(), path, magic, nowild })
}

fn eq_bytes(a: &[u8], b: &[u8], icase: bool) -> bool {
    if icase { a.eq_ignore_ascii_case(b) } else { a == b }
}

/// O `match_pathspec_item` do git.
pub fn match_item(it: &Item, name: &[u8], is_dir: bool) -> Option<Hit> {
    let m = &it.path;
    if m.is_empty() {
        return Some(Hit::Recursive);
    }
    let icase = it.magic & ICASE != 0;
    let ml = m.len();
    if ml <= name.len() && eq_bytes(&m[..], &name[..ml], icase) {
        if ml == name.len() {
            return Some(Hit::Exact);
        }
        if m[ml - 1] == b'/' || name[ml] == b'/' {
            return Some(Hit::Recursive);
        }
    } else if is_dir && m[ml - 1] == b'/' && name.len() == ml - 1 && eq_bytes(&m[..ml - 1], name, icase) {
        return Some(Hit::Exact);
    }
    if it.nowild < ml {
        let pre = it.nowild;
        if name.len() < pre || !eq_bytes(&m[..pre], &name[..pre], icase) {
            return None;
        }
        let flags = (if it.magic & GLOB != 0 { PATHNAME } else { 0 }) | if icase { CASEFOLD } else { 0 };
        if wildmatch::wildmatch(&m[pre..], &name[pre..], flags) {
            return Some(Hit::Fnmatch);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ps(args: &[&str], prefix: &str) -> Pathspec {
        let a: Vec<Vec<u8>> = args.iter().map(|s| s.as_bytes().to_vec()).collect();
        Pathspec::parse(&a, prefix.as_bytes(), b"/w", &Opts { literal: false, glob: false, noglob: false, icase: false }).ok().unwrap()
    }

    #[test]
    fn matching() {
        let p = ps(&["src"], "");
        assert_eq!(p.matches(b"src/a.rs", false, None), Some(Hit::Recursive));
        assert_eq!(p.matches(b"src", false, None), Some(Hit::Exact));
        assert_eq!(p.matches(b"srcx", false, None), None);
        let p = ps(&["*.rs"], "");
        assert_eq!(p.matches(b"a/b.rs", false, None), Some(Hit::Fnmatch));
        let p = ps(&["../x"], "sub/");
        assert_eq!(p.items[0].path, b"x");
        let p = ps(&["."], "sub/");
        assert_eq!(p.items[0].path, b"sub");
        assert!(p.matches_simple(b"sub/q"));
        assert!(!p.matches_simple(b"other"));
        let p = ps(&[".", ":!*.log"], "");
        assert!(p.matches_simple(b"a.txt"));
        assert!(!p.matches_simple(b"a.log"));
        let p = ps(&[":(exclude)b"], "");
        assert!(p.matches_simple(b"a"));
        assert!(!p.matches_simple(b"b/c"));
        assert!(ps(&["a/b/c"], "").may_match_under(b"a"));
        assert!(!ps(&["a/b/c"], "").may_match_under(b"x"));
        assert!(ps(&["a"], "").may_match_under(b"a/b"));
    }
}
