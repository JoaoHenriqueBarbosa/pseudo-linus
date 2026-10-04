//! Regras de exclusão: `.gitignore` de cada diretório, `.git/info/exclude`, `core.excludesFile` e
//! padrões da linha de comando, com a precedência e a semântica do `dir.c` do git.

use std::collections::HashMap;
use std::rc::Rc;

use crate::os;
use crate::repo::Repo;
use crate::wildmatch::{self, PATHNAME};

#[derive(Clone, Debug)]
pub struct Pattern {
    pub pattern: Vec<u8>,
    pub negative: bool,
    pub dir_only: bool,
    /// Sem `/` no meio: casa só com o nome base.
    pub basename_only: bool,
    /// Diretório do arquivo de origem relativo ao topo (sem barra no fim; vazio no topo).
    pub base: Vec<u8>,
    /// Origem e linha, pro `check-ignore -v`.
    pub source: Vec<u8>,
    pub line: usize,
    /// Texto original da linha.
    pub text: Vec<u8>,
}

/// Lê as linhas de um arquivo de padrões.
pub fn parse_patterns(data: &[u8], base: &[u8], source: &[u8]) -> Vec<Pattern> {
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    let mut out = Vec::new();
    for (n, raw) in data.split(|c| *c == b'\n').enumerate() {
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        if raw.is_empty() || raw[0] == b'#' {
            continue;
        }
        let line = trim_trailing_spaces(raw);
        if line.is_empty() {
            continue;
        }
        let mut p = line.as_slice();
        let mut negative = false;
        if p[0] == b'!' {
            negative = true;
            p = &p[1..];
        } else if p.starts_with(b"\\!") || p.starts_with(b"\\#") {
            p = &p[1..];
        }
        if p.is_empty() {
            continue;
        }
        let mut dir_only = false;
        let mut pat = p.to_vec();
        if pat.len() > 1 && pat.ends_with(b"/") {
            dir_only = true;
            pat.pop();
        } else if pat == b"/" {
            continue;
        }
        let basename_only = !pat.contains(&b'/');
        out.push(Pattern {
            pattern: pat,
            negative,
            dir_only,
            basename_only,
            base: base.to_vec(),
            source: source.to_vec(),
            line: n + 1,
            text: raw.to_vec(),
        });
    }
    out
}

/// Tira espaços do fim, a não ser que escapados com `\`.
fn trim_trailing_spaces(s: &[u8]) -> Vec<u8> {
    let mut end = s.len();
    let mut last_space: Option<usize> = None;
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b' ' => {
                if last_space.is_none() {
                    last_space = Some(i);
                }
            }
            b'\\' => {
                i += 1;
                last_space = None;
            }
            _ => last_space = None,
        }
        i += 1;
    }
    if let Some(ls) = last_space {
        end = ls;
    }
    s[..end.min(s.len())].to_vec()
}

impl Pattern {
    /// O padrão casa com `path` (relativo ao topo)?
    pub fn matches(&self, path: &[u8], is_dir: bool, icase: bool) -> bool {
        if self.dir_only && !is_dir {
            return false;
        }
        let flags = PATHNAME | if icase { wildmatch::CASEFOLD } else { 0 };
        if self.basename_only {
            let name = os::basename(path);
            return wildmatch::wildmatch(&self.pattern, name, flags);
        }
        let pat = self.pattern.strip_prefix(b"/").unwrap_or(&self.pattern);
        let rel: &[u8] = if self.base.is_empty() {
            path
        } else {
            if path.len() < self.base.len() + 1 || !path.starts_with(&self.base) || path[self.base.len()] != b'/' {
                return false;
            }
            &path[self.base.len() + 1..]
        };
        let pre = wildmatch::literal_prefix_len(pat);
        if pre == pat.len() {
            return if icase { pat.eq_ignore_ascii_case(rel) } else { pat == rel };
        }
        if pre > rel.len() || !prefix_eq(&pat[..pre], &rel[..pre], icase) {
            return false;
        }
        wildmatch::wildmatch(&pat[pre..], &rel[pre..], flags)
    }
}

fn prefix_eq(a: &[u8], b: &[u8], icase: bool) -> bool {
    if icase { a.eq_ignore_ascii_case(b) } else { a == b }
}

/// O conjunto de regras de um repositório.
pub struct Ignores {
    pub cmdline: Vec<Pattern>,
    /// `info/exclude` primeiro, depois `core.excludesFile` (é a ordem de consulta do git).
    pub files: Vec<Vec<Pattern>>,
    per_dir: HashMap<Vec<u8>, Rc<Vec<Pattern>>>,
    top: Vec<u8>,
    use_dir_files: bool,
    pub icase: bool,
}

impl Ignores {
    /// As regras padrão (`setup_standard_excludes`).
    pub fn standard(repo: &Repo) -> Ignores {
        let mut files = Vec::new();
        let info = repo.common("info/exclude");
        if let Ok(Some(d)) = os::read_opt(&info) {
            files.push(parse_patterns(&d, b"", b".git/info/exclude"));
        }
        let excl = match repo.config.get_bytes("core.excludesfile") {
            Some(p) => Some(expand_user(&p)),
            None => match os::getenv("XDG_CONFIG_HOME") {
                Some(x) if !x.is_empty() => Some(os::join(&x, b"git/ignore")),
                _ => os::getenv("HOME").map(|h| os::join(&h, b".config/git/ignore")),
            },
        };
        if let Some(p) = excl
            && let Ok(Some(d)) = os::read_opt(&p)
        {
            files.push(parse_patterns(&d, b"", &p));
        }
        Ignores {
            cmdline: Vec::new(),
            files,
            per_dir: HashMap::new(),
            top: repo.work_tree.clone().unwrap_or_default(),
            use_dir_files: true,
            icase: repo.config.get_bool("core.ignorecase").ok().flatten().unwrap_or(false),
        }
    }

    /// Sem regra nenhuma (pra `--no-standard`, ou quem não quer exclusões).
    pub fn none() -> Ignores {
        Ignores { cmdline: Vec::new(), files: Vec::new(), per_dir: HashMap::new(), top: Vec::new(), use_dir_files: false, icase: false }
    }

    pub fn add_cmdline(&mut self, pat: &[u8]) {
        self.cmdline.extend(parse_patterns(pat, b"", b"<command line>"));
    }

    pub fn add_file_patterns(&mut self, data: &[u8], source: &[u8]) {
        self.files.insert(0, parse_patterns(data, b"", source));
    }

    fn dir_patterns(&mut self, dir: &[u8]) -> Rc<Vec<Pattern>> {
        if let Some(p) = self.per_dir.get(dir) {
            return p.clone();
        }
        let file = if dir.is_empty() { b".gitignore".to_vec() } else { os::join(dir, b".gitignore") };
        let abs = os::join(&self.top, &file);
        let pats = match os::lstat(&abs) {
            Ok(st) if st.file_type() == sysabi::FileType::Regular => match os::read_opt(&abs) {
                Ok(Some(d)) => parse_patterns(&d, dir, &file),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        let rc = Rc::new(pats);
        self.per_dir.insert(dir.to_vec(), rc.clone());
        rc
    }

    /// Último padrão que casa com `path` olhando só o próprio caminho (sem os pais).
    pub fn last_match(&mut self, path: &[u8], is_dir: bool) -> Option<Pattern> {
        let icase = self.icase;
        for p in self.cmdline.iter().rev() {
            if p.matches(path, is_dir, icase) {
                return Some(p.clone());
            }
        }
        if self.use_dir_files {
            // Do diretório mais fundo pro topo.
            let mut dirs: Vec<Vec<u8>> = vec![Vec::new()];
            let mut k = 0;
            while let Some(s) = path[k..].iter().position(|c| *c == b'/') {
                dirs.push(path[..k + s].to_vec());
                k += s + 1;
            }
            for d in dirs.iter().rev() {
                let pats = self.dir_patterns(d);
                for p in pats.iter().rev() {
                    if p.matches(path, is_dir, icase) {
                        return Some(p.clone());
                    }
                }
            }
        }
        for list in &self.files {
            for p in list.iter().rev() {
                if p.matches(path, is_dir, icase) {
                    return Some(p.clone());
                }
            }
        }
        None
    }

    /// O caminho está excluído, por ele mesmo ou por um diretório pai excluído?
    pub fn is_ignored(&mut self, path: &[u8], is_dir: bool) -> bool {
        self.matching(path, is_dir).is_some_and(|p| !p.negative)
    }

    /// Padrão que decide (considerando os pais), pro `check-ignore`.
    pub fn matching(&mut self, path: &[u8], is_dir: bool) -> Option<Pattern> {
        let mut k = 0;
        while let Some(s) = path[k..].iter().position(|c| *c == b'/') {
            let parent = path[..k + s].to_vec();
            if let Some(p) = self.last_match(&parent, true)
                && !p.negative
            {
                return Some(p);
            }
            k += s + 1;
        }
        self.last_match(path, is_dir)
    }

    /// Só o caminho (sem pais), pra quem já está descendo a árvore e sabe que os pais passaram.
    pub fn is_ignored_here(&mut self, path: &[u8], is_dir: bool) -> bool {
        self.last_match(path, is_dir).is_some_and(|p| !p.negative)
    }
}

/// `~/x` e `~user/x`.
pub fn expand_user(p: &[u8]) -> Vec<u8> {
    if let Some(rest) = p.strip_prefix(b"~/") {
        if let Some(h) = os::getenv("HOME") {
            return os::join(&h, rest);
        }
    } else if p == b"~" {
        if let Some(h) = os::getenv("HOME") {
            return h;
        }
    }
    p.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pats: &str, path: &str, dir: bool) -> Option<bool> {
        let list = parse_patterns(pats.as_bytes(), b"", b"x");
        list.iter().rev().find(|p| p.matches(path.as_bytes(), dir, false)).map(|p| !p.negative)
    }

    #[test]
    fn gitignore_semantics() {
        assert_eq!(m("*.log\n", "a/b/x.log", false), Some(true));
        assert_eq!(m("/build\n", "build", true), Some(true));
        assert_eq!(m("/build\n", "sub/build", true), None);
        assert_eq!(m("build/\n", "build", false), None);
        assert_eq!(m("build/\n", "x/build", true), Some(true));
        assert_eq!(m("*.log\n!keep.log\n", "keep.log", false), Some(false));
        assert_eq!(m("doc/*.txt\n", "doc/a.txt", false), Some(true));
        assert_eq!(m("doc/*.txt\n", "doc/x/a.txt", false), None);
        assert_eq!(m("**/tmp\n", "a/b/tmp", true), Some(true));
        assert_eq!(m("foo\\ \n", "foo ", false), Some(true));
        assert_eq!(m("foo  \n", "foo", false), Some(true));
    }
}
