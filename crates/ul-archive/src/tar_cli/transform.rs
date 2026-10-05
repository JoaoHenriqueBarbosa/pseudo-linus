//! `--transform`/`--xform`: expressões `s/regex/substituição/flags` do sed aplicadas aos nomes,
//! separadas por `;`, com as flags do GNU tar (`g`, `i`, `x`, número da ocorrência, e `r`/`R`, `s`/`S`,
//! `h`/`H` pra escolher se vale pra nomes comuns, alvos de link simbólico e alvos de link físico).
//! Regex pelo crate `regex-posix` (BRE, ou ERE com `x`).

use regex_posix::{Regex, Syntax};

use super::Tar;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Name,
    Symlink,
    Hardlink,
}

#[derive(Debug)]
enum Piece {
    Lit(Vec<u8>),
    Group(usize),
    Whole,
    /// \L \U \l \u \E
    Case(u8),
}

#[derive(Debug)]
struct Subst {
    re: Regex,
    repl: Vec<Piece>,
    global: bool,
    nth: usize,
    names: bool,
    symlinks: bool,
    hardlinks: bool,
}

/// Uma ou mais substituições de um `--transform`.
#[derive(Debug)]
pub struct Expr {
    subs: Vec<Subst>,
}

fn parse_repl(r: &[u8]) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut lit = Vec::new();
    let mut i = 0;
    while i < r.len() {
        let c = r[i];
        if c == b'\\' && i + 1 < r.len() {
            let n = r[i + 1];
            i += 2;
            match n {
                b'0'..=b'9' => {
                    if !lit.is_empty() {
                        out.push(Piece::Lit(std::mem::take(&mut lit)));
                    }
                    out.push(Piece::Group((n - b'0') as usize));
                }
                b'L' | b'U' | b'l' | b'u' | b'E' => {
                    if !lit.is_empty() {
                        out.push(Piece::Lit(std::mem::take(&mut lit)));
                    }
                    out.push(Piece::Case(n));
                }
                b'n' => lit.push(b'\n'),
                b't' => lit.push(b'\t'),
                other => lit.push(other),
            }
            continue;
        }
        if c == b'&' {
            if !lit.is_empty() {
                out.push(Piece::Lit(std::mem::take(&mut lit)));
            }
            out.push(Piece::Whole);
            i += 1;
            continue;
        }
        lit.push(c);
        i += 1;
    }
    if !lit.is_empty() {
        out.push(Piece::Lit(lit));
    }
    out
}

impl Expr {
    /// Analisa o argumento de `--transform`.
    pub fn parse(arg: &[u8]) -> Result<Expr, Vec<u8>> {
        let mut subs = Vec::new();
        let mut rest = arg;
        loop {
            if rest.is_empty() {
                break;
            }
            if rest[0] != b's' || rest.len() < 2 {
                let mut m = b"Invalid transform expression".to_vec();
                if rest.first() != Some(&b's') {
                    m = "Invalid transform expression".to_string().into_bytes();
                }
                return Err(m);
            }
            let delim = rest[1];
            let mut parts: Vec<Vec<u8>> = Vec::new();
            let mut cur = Vec::new();
            let mut i = 2;
            while i < rest.len() && parts.len() < 2 {
                let c = rest[i];
                if c == b'\\' && i + 1 < rest.len() {
                    if rest[i + 1] == delim {
                        cur.push(delim);
                    } else {
                        cur.push(c);
                        cur.push(rest[i + 1]);
                    }
                    i += 2;
                    continue;
                }
                if c == delim {
                    parts.push(std::mem::take(&mut cur));
                    i += 1;
                    continue;
                }
                cur.push(c);
                i += 1;
            }
            if parts.len() < 2 {
                return Err(b"Invalid transform expression".to_vec());
            }
            let mut global = false;
            let mut nth = 0usize;
            let mut icase = false;
            let mut extended = false;
            let (mut names, mut symlinks, mut hardlinks) = (true, true, true);
            while i < rest.len() && rest[i] != b';' {
                match rest[i] {
                    b'g' => global = true,
                    b'i' => icase = true,
                    b'x' => extended = true,
                    b'r' => names = true,
                    b'R' => names = false,
                    b's' => symlinks = true,
                    b'S' => symlinks = false,
                    b'h' => hardlinks = true,
                    b'H' => hardlinks = false,
                    d @ b'0'..=b'9' => nth = nth * 10 + (d - b'0') as usize,
                    other => {
                        return Err(format!("Unknown flag in transform expression: {}", other as char).into_bytes());
                    }
                }
                i += 1;
            }
            let syntax = if extended { Syntax::POSIX_EXTENDED } else { Syntax::POSIX_BASIC };
            let re = Regex::builder(syntax).icase(icase).build(&parts[0]).map_err(|e| {
                let mut m = b"Invalid transform expression: ".to_vec();
                m.extend_from_slice(e.message().as_bytes());
                m
            })?;
            subs.push(Subst { re, repl: parse_repl(&parts[1]), global, nth, names, symlinks, hardlinks });
            rest = if i < rest.len() { &rest[i + 1..] } else { &[] };
        }
        Ok(Expr { subs })
    }

    /// Aplica as substituições a um nome.
    pub fn apply(&self, name: &[u8], target: Target) -> Vec<u8> {
        let mut cur = name.to_vec();
        for s in &self.subs {
            let ok = match target {
                Target::Name => s.names,
                Target::Symlink => s.symlinks,
                Target::Hardlink => s.hardlinks,
            };
            if ok {
                cur = s.run(&cur);
            }
        }
        cur
    }
}

fn apply_case(out: &mut Vec<u8>, text: &[u8], mode: u8, one: &mut u8) {
    for &c in text {
        let mut c = c;
        if *one == b'l' {
            c = c.to_ascii_lowercase();
            *one = 0;
        } else if *one == b'u' {
            c = c.to_ascii_uppercase();
            *one = 0;
        } else if mode == b'L' {
            c = c.to_ascii_lowercase();
        } else if mode == b'U' {
            c = c.to_ascii_uppercase();
        }
        out.push(c);
    }
}

impl Subst {
    fn run(&self, s: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut pos = 0usize;
        let mut count = 0usize;
        let mut replaced_any = false;
        while pos <= s.len() {
            let Some(caps) = self.re.captures_at(s, pos) else { break };
            let m = caps.whole();
            count += 1;
            let wanted = if self.nth > 0 { count == self.nth || (self.global && count > self.nth) } else { true };
            out.extend_from_slice(&s[pos..m.start]);
            if wanted {
                let mut mode = 0u8;
                let mut one = 0u8;
                for p in &self.repl {
                    match p {
                        Piece::Lit(l) => apply_case(&mut out, l, mode, &mut one),
                        Piece::Whole => apply_case(&mut out, &s[m.start..m.end], mode, &mut one),
                        Piece::Group(g) => {
                            if let Some(gm) = caps.get(*g) {
                                apply_case(&mut out, &s[gm.start..gm.end], mode, &mut one);
                            }
                        }
                        Piece::Case(b'E') => {
                            mode = 0;
                            one = 0;
                        }
                        Piece::Case(c @ (b'l' | b'u')) => one = *c,
                        Piece::Case(c) => mode = *c,
                    }
                }
                replaced_any = true;
            } else {
                out.extend_from_slice(&s[m.start..m.end]);
            }
            if m.end == m.start {
                if m.end < s.len() {
                    out.push(s[m.end]);
                }
                pos = m.end + 1;
            } else {
                pos = m.end;
            }
            if !self.global && replaced_any {
                break;
            }
        }
        if pos <= s.len() {
            out.extend_from_slice(&s[pos..]);
        }
        out
    }
}

/// Aplica todas as `--transform` a um nome.
pub fn apply_all(t: &Tar, name: &[u8], target: Target) -> Vec<u8> {
    let mut cur = name.to_vec();
    for e in &t.transforms {
        cur = e.apply(&cur, target);
    }
    cur
}

/// Nome mostrado na listagem de leitura (`-t`/`-x`): o do arquivo, ou o transformado com
/// `--show-transformed-names`.
pub fn display_name(t: &Tar, name: &[u8]) -> Vec<u8> {
    if t.o.show_transformed && !t.transforms.is_empty() {
        return apply_all(t, name, Target::Name);
    }
    name.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sed_like_substitutions() {
        let e = Expr::parse(b"s/^d/D/").unwrap();
        assert_eq!(e.apply(b"d/a", Target::Name), b"D/a");
        let e = Expr::parse(b"s,a,X,g;s/x/y/").unwrap();
        assert_eq!(e.apply(b"aaxa", Target::Name), b"XXyX");
        let e = Expr::parse(b"s/\\(.*\\)\\.txt/\\1.md/").unwrap();
        assert_eq!(e.apply(b"d/a.txt", Target::Name), b"d/a.md");
        let e = Expr::parse(b"s/.*/\\U&/").unwrap();
        assert_eq!(e.apply(b"ab", Target::Name), b"AB");
    }
}
