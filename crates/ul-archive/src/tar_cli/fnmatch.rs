//! `fnmatch(3)` com as flags que o tar usa (`FNM_PATHNAME` pra wildcards que não casam `/`,
//! `FNM_LEADING_DIR`, `FNM_CASEFOLD`, `FNM_NOESCAPE`), escrito a partir do POSIX: `*`, `?`, classes
//! `[...]` com `!`/`^`, faixas e classes nomeadas `[:alpha:]`, barra invertida como escape.

#[derive(Clone, Copy, Debug, Default)]
pub struct Flags {
    /// `*` e `?` não casam `/` (o contrário do `--wildcards-match-slash`).
    pub pathname: bool,
    /// Casa também se o padrão casar um prefixo de diretórios do nome (`a` casa `a/b/c`).
    pub leading_dir: bool,
    pub casefold: bool,
    pub noescape: bool,
}

fn lower(c: u8, fold: bool) -> u8 {
    if fold { c.to_ascii_lowercase() } else { c }
}

fn class_match(name: &[u8], c: u8) -> bool {
    match name {
        b"alpha" => c.is_ascii_alphabetic(),
        b"digit" => c.is_ascii_digit(),
        b"alnum" => c.is_ascii_alphanumeric(),
        b"upper" => c.is_ascii_uppercase(),
        b"lower" => c.is_ascii_lowercase(),
        b"space" => matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c),
        b"blank" => matches!(c, b' ' | b'\t'),
        b"punct" => c.is_ascii_punctuation(),
        b"print" => (0x20..0x7f).contains(&c),
        b"graph" => (0x21..0x7f).contains(&c),
        b"cntrl" => c < 0x20 || c == 0x7f,
        b"xdigit" => c.is_ascii_hexdigit(),
        _ => false,
    }
}

/// Tenta casar uma classe `[...]` começando em `p[0] == '['`; devolve (casou, bytes do padrão
/// consumidos) ou `None` se a classe não fecha (aí o `[` é literal).
fn bracket(p: &[u8], c: u8, f: Flags) -> Option<(bool, usize)> {
    let mut i = 1;
    let negate = matches!(p.get(i), Some(b'!') | Some(b'^'));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    let cc = lower(c, f.casefold);
    loop {
        let ch = *p.get(i)?;
        if ch == b']' && !first {
            i += 1;
            break;
        }
        first = false;
        if ch == b'[' && p.get(i + 1) == Some(&b':') {
            let rest = &p[i + 2..];
            if let Some(end) = rest.windows(2).position(|w| w == b":]") {
                if class_match(&rest[..end], c) {
                    matched = true;
                }
                i += 2 + end + 2;
                continue;
            }
        }
        let mut lo = ch;
        if lo == b'\\' && !f.noescape {
            i += 1;
            lo = *p.get(i)?;
        }
        i += 1;
        if p.get(i) == Some(&b'-') && p.get(i + 1).is_some_and(|&x| x != b']') {
            let mut hi = p[i + 1];
            i += 2;
            if hi == b'\\' && !f.noescape {
                hi = *p.get(i)?;
                i += 1;
            }
            let (l, h) = (lower(lo, f.casefold), lower(hi, f.casefold));
            if l <= cc && cc <= h {
                matched = true;
            }
        } else if lower(lo, f.casefold) == cc {
            matched = true;
        }
    }
    if f.pathname && c == b'/' {
        return Some((false, i));
    }
    Some((matched != negate, i))
}

fn do_match(p: &[u8], s: &[u8], f: Flags, depth: usize) -> bool {
    if depth > 10_000 {
        return false;
    }
    let (mut pi, mut si) = (0usize, 0usize);
    while pi < p.len() {
        match p[pi] {
            b'*' => {
                while pi < p.len() && p[pi] == b'*' {
                    pi += 1;
                }
                if pi == p.len() {
                    if f.pathname {
                        // Com FNM_LEADING_DIR o `*` pode parar na próxima barra e o resto vale.
                        return f.leading_dir || !s[si..].contains(&b'/');
                    }
                    return true;
                }
                let mut k = si;
                loop {
                    if do_match(&p[pi..], &s[k..], f, depth + 1) {
                        return true;
                    }
                    if k >= s.len() || (f.pathname && s[k] == b'/') {
                        return false;
                    }
                    k += 1;
                }
            }
            b'?' => {
                if si >= s.len() || (f.pathname && s[si] == b'/') {
                    return false;
                }
                pi += 1;
                si += 1;
            }
            b'[' => {
                if si >= s.len() {
                    return false;
                }
                match bracket(&p[pi..], s[si], f) {
                    Some((true, n)) => {
                        pi += n;
                        si += 1;
                    }
                    Some((false, _)) => return false,
                    None => {
                        if s[si] != b'[' {
                            return false;
                        }
                        pi += 1;
                        si += 1;
                    }
                }
            }
            b'\\' if !f.noescape && pi + 1 < p.len() => {
                if si >= s.len() || lower(p[pi + 1], f.casefold) != lower(s[si], f.casefold) {
                    return false;
                }
                pi += 2;
                si += 1;
            }
            ch => {
                if si >= s.len() || lower(ch, f.casefold) != lower(s[si], f.casefold) {
                    return false;
                }
                pi += 1;
                si += 1;
            }
        }
    }
    si == s.len() || (f.leading_dir && s[si] == b'/')
}

/// Casa `s` com o padrão `p`.
pub fn fnmatch(p: &[u8], s: &[u8], f: Flags) -> bool {
    do_match(p, s, f, 0)
}

/// O padrão tem caracteres de wildcard.
pub fn has_wildcards(p: &[u8]) -> bool {
    p.iter().any(|&c| matches!(c, b'*' | b'?' | b'[' | b'\\'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        let f = Flags::default();
        assert!(fnmatch(b"*.txt", b"a/b.txt", f));
        assert!(!fnmatch(b"*.txt", b"a/b.txt", Flags { pathname: true, ..f }));
        assert!(fnmatch(b"a", b"a/b/c", Flags { leading_dir: true, ..f }));
        assert!(!fnmatch(b"a", b"ab", Flags { leading_dir: true, ..f }));
        assert!(fnmatch(b"[a-c]?", b"bx", f));
        assert!(fnmatch(b"[!a]x", b"bx", f));
        assert!(fnmatch(b"[[:digit:]]*", b"9z", f));
        assert!(fnmatch(b"\\*", b"*", f));
        assert!(fnmatch(b"ABC", b"abc", Flags { casefold: true, ..f }));
    }
}
