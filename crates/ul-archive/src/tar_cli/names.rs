//! Nomes de membros: remoção dos prefixos perigosos (`/` e trechos com `..`), casamento dos operandos
//! com os membros (`-x`, `-t`, `-d`, `--delete`) e exclusão (`--exclude`, `-X`, `--exclude-vcs`).

use super::args::{Exclude, MatchFlags, NameArg};
use super::fnmatch::{self, Flags};

/// Separa o prefixo inseguro de um nome: barras iniciais e tudo até o último componente `..`.
/// Devolve (prefixo removido, resto). O resto vazio vira `.`.
pub fn unsafe_prefix(name: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut cut = 0usize;
    // Barras do começo.
    while cut < name.len() && name[cut] == b'/' {
        cut += 1;
    }
    let mut i = cut;
    while i < name.len() {
        let end = name[i..].iter().position(|&c| c == b'/').map(|p| i + p).unwrap_or(name.len());
        if &name[i..end] == b".." {
            let mut j = end;
            while j < name.len() && name[j] == b'/' {
                j += 1;
            }
            cut = j;
        }
        i = end;
        while i < name.len() && name[i] == b'/' {
            i += 1;
        }
    }
    let prefix = name[..cut].to_vec();
    let mut rest = name[cut..].to_vec();
    if rest.is_empty() && !prefix.is_empty() {
        rest = b".".to_vec();
    }
    (prefix, rest)
}

/// Tira `n` componentes do começo (`--strip-components`). `None` se o nome tem menos que isso.
pub fn strip_components(name: &[u8], n: usize) -> Option<Vec<u8>> {
    if n == 0 {
        return Some(name.to_vec());
    }
    let mut i = 0usize;
    let mut left = n;
    while left > 0 {
        while i < name.len() && name[i] == b'/' {
            i += 1;
        }
        if i >= name.len() {
            return None;
        }
        while i < name.len() && name[i] != b'/' {
            i += 1;
        }
        left -= 1;
    }
    while i < name.len() && name[i] == b'/' {
        i += 1;
    }
    if i >= name.len() {
        return None;
    }
    Some(name[i..].to_vec())
}

/// Remove barras finais (menos a da raiz).
pub fn trim_trailing_slashes(name: &[u8]) -> &[u8] {
    let mut end = name.len();
    while end > 1 && name[end - 1] == b'/' {
        end -= 1;
    }
    &name[..end]
}

/// Um operando de inclusão e quantas vezes casou.
pub struct NamePattern {
    pub arg: NameArg,
    pub pattern: Vec<u8>,
    pub found: u64,
}

/// Lista de operandos de inclusão.
pub struct NameList {
    pub items: Vec<NamePattern>,
}

impl NameList {
    pub fn new(names: &[NameArg]) -> NameList {
        NameList {
            items: names
                .iter()
                .map(|n| NamePattern { arg: n.clone(), pattern: trim_trailing_slashes(&n.name).to_vec(), found: 0 })
                .collect(),
        }
    }

    /// Índice do operando que casa o membro (o primeiro), ou `None`.
    pub fn find(&self, member: &[u8]) -> Option<usize> {
        let member = trim_trailing_slashes(member);
        self.items.iter().position(|it| name_matches(&it.pattern, member, it.arg.flags, it.arg.recursion, true))
    }

    /// Operandos que nunca casaram.
    pub fn unmatched(&self) -> impl Iterator<Item = &NamePattern> {
        self.items.iter().filter(|i| i.found == 0)
    }
}

/// Casamento de um operando de inclusão: sem wildcards por padrão, ancorado, com recursão (o operando
/// casa o membro e tudo debaixo dele).
pub fn name_matches(pattern: &[u8], member: &[u8], flags: MatchFlags, recursion: bool, inclusion: bool) -> bool {
    let wildcards = flags.wildcards.unwrap_or(!inclusion);
    let anchored = flags.anchored.unwrap_or(inclusion);
    let match_slash = flags.match_slash.unwrap_or(true);
    if wildcards && fnmatch::has_wildcards(pattern) {
        let f = Flags { pathname: !match_slash, leading_dir: recursion, casefold: flags.ignore_case, noescape: false };
        if fnmatch::fnmatch(pattern, member, f) {
            return true;
        }
        if !anchored {
            let mut i = 0;
            while let Some(p) = member[i..].iter().position(|&c| c == b'/') {
                i += p + 1;
                if fnmatch::fnmatch(pattern, &member[i..], f) {
                    return true;
                }
            }
        }
        return false;
    }
    let eq = |a: &[u8], b: &[u8]| if flags.ignore_case { a.eq_ignore_ascii_case(b) } else { a == b };
    let try_at = |m: &[u8]| -> bool {
        if m.len() < pattern.len() {
            return false;
        }
        if !eq(&m[..pattern.len()], pattern) {
            return false;
        }
        m.len() == pattern.len() || (recursion && (m[pattern.len()] == b'/' || pattern.ends_with(b"/")))
    };
    if try_at(member) {
        return true;
    }
    if !anchored {
        let mut i = 0;
        while let Some(p) = member[i..].iter().position(|&c| c == b'/') {
            i += p + 1;
            if try_at(&member[i..]) {
                return true;
            }
        }
    }
    false
}

/// Diretórios de controle de versão que o `--exclude-vcs` pula.
const VCS: &[&[u8]] = &[
    b"CVS", b"RCS", b"SCCS", b".git", b".gitignore", b".gitattributes", b".gitmodules", b".cvsignore",
    b".svn", b".arch-ids", b"{arch}", b"=RELEASE-ID", b"=meta-update", b"=update", b".bzr", b".bzrignore",
    b".bzrtags", b".hg", b".hgignore", b".hgtags", b"_darcs",
];

/// Conjunto de exclusões.
#[derive(Default, Clone)]
pub struct Excluder {
    pub patterns: Vec<Exclude>,
    pub vcs: bool,
    pub backups: bool,
}

impl Excluder {
    pub fn excluded(&self, path: &[u8]) -> bool {
        let path = trim_trailing_slashes(path);
        let base = match path.iter().rposition(|&c| c == b'/') {
            Some(p) => &path[p + 1..],
            None => path,
        };
        if self.vcs && VCS.contains(&base) {
            return true;
        }
        if self.backups
            && (base.ends_with(b"~") || base.starts_with(b".#") || (base.starts_with(b"#") && base.ends_with(b"#")))
        {
            return true;
        }
        self.patterns.iter().any(|e| name_matches(&e.pattern, path, e.flags, true, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_prefixes_like_gnu() {
        assert_eq!(unsafe_prefix(b"/tmp/d"), (b"/".to_vec(), b"tmp/d".to_vec()));
        assert_eq!(unsafe_prefix(b"d/../d/e/f"), (b"d/../".to_vec(), b"d/e/f".to_vec()));
        assert_eq!(unsafe_prefix(b"../x"), (b"../".to_vec(), b"x".to_vec()));
        assert_eq!(unsafe_prefix(b"a/b"), (Vec::new(), b"a/b".to_vec()));
        assert_eq!(unsafe_prefix(b"x/.."), (b"x/..".to_vec(), b".".to_vec()));
    }

    #[test]
    fn strip() {
        assert_eq!(strip_components(b"a/b/c", 1), Some(b"b/c".to_vec()));
        assert_eq!(strip_components(b"a/", 1), None);
        assert_eq!(strip_components(b"a/b/", 1), Some(b"b/".to_vec()));
    }

    #[test]
    fn inclusion_and_exclusion() {
        let f = MatchFlags::default();
        assert!(name_matches(b"d", b"d/a", f, true, true));
        assert!(!name_matches(b"d", b"da", f, true, true));
        assert!(!name_matches(b"a", b"d/a", f, true, true));
        assert!(name_matches(b"*.o", b"x/y.o", f, true, false));
        assert!(name_matches(b"y.o", b"x/y.o", f, true, false));
        assert!(name_matches(b"sub", b"x/sub/z", f, true, false));
    }
}
