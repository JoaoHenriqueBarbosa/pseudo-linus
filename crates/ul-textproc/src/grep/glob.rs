//! `--include`, `--exclude` e `--exclude-dir`: o `fnmatch` do glibc sem flags (o `*` atravessa
//! `/`) e a regra do `excluded_file_name` do gnulib (vale o último padrão que casa; sem nenhum
//! casando, vale o contrário do primeiro).

use regex_posix::ast::{PosixClass, Unit, decode};
use regex_posix::charclass::{class_contains, posix_class};

#[derive(Default)]
pub struct Excludes {
    /// (padrão, é `--include`).
    pats: Vec<(Vec<Unit>, bool)>,
}

impl Excludes {
    pub fn add(&mut self, pattern: &[u8], include: bool) {
        self.pats.push((decode(pattern), include));
    }

    /// `true` se `name` fica de fora. `anchored`: o nome inteiro (entrada da recursão, só o nome
    /// base); senão também cada sufixo depois de `/` (operando da linha de comando).
    pub fn excluded(&self, name: &[u8], anchored: bool) -> bool {
        let Some(first) = self.pats.first() else { return false };
        let name = decode(name);
        let matches = |pat: &[Unit]| -> bool {
            if fnmatch(pat, &name) {
                return true;
            }
            if !anchored {
                for i in 0..name.len() {
                    if name[i] == Unit::Char('/') && name.get(i + 1) != Some(&Unit::Char('/')) && fnmatch(pat, &name[i + 1..]) {
                        return true;
                    }
                }
            }
            false
        };
        match self.pats.iter().rev().find(|(p, _)| matches(p)) {
            Some((_, include)) => !include,
            None => first.1,
        }
    }
}

/// `fnmatch(pattern, string, 0)`.
pub fn fnmatch(p: &[Unit], s: &[Unit]) -> bool {
    let (mut pi, mut si) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    loop {
        if pi < p.len() {
            match p[pi] {
                Unit::Char('*') => {
                    while pi < p.len() && p[pi] == Unit::Char('*') {
                        pi += 1;
                    }
                    star = Some((pi, si));
                    continue;
                }
                Unit::Char('?') if si < s.len() => {
                    pi += 1;
                    si += 1;
                    continue;
                }
                Unit::Char('[') if si < s.len() => {
                    if let Some((matched, next)) = bracket(p, pi, s[si]) {
                        if matched {
                            pi = next;
                            si += 1;
                            continue;
                        }
                    } else if s[si] == Unit::Char('[') {
                        pi += 1;
                        si += 1;
                        continue;
                    }
                }
                Unit::Char('\\') if pi + 1 < p.len() => {
                    if si < s.len() && s[si] == p[pi + 1] {
                        pi += 2;
                        si += 1;
                        continue;
                    }
                }
                u => {
                    if si < s.len() && s[si] == u && !matches!(u, Unit::Char('?' | '[')) {
                        pi += 1;
                        si += 1;
                        continue;
                    }
                }
            }
        } else if si == s.len() {
            return true;
        }
        match star {
            Some((sp, ss)) if ss < s.len() => {
                star = Some((sp, ss + 1));
                pi = sp;
                si = ss + 1;
            }
            _ => return false,
        }
    }
}

/// Expressão de colchetes em `p[pi]` (`[`); devolve (casou, índice depois do `]`), ou `None` se
/// não fecha (aí o `[` é literal).
fn bracket(p: &[Unit], pi: usize, c: Unit) -> Option<(bool, usize)> {
    let mut i = pi + 1;
    let negate = matches!(p.get(i), Some(Unit::Char('!' | '^')));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    loop {
        let u = *p.get(i)?;
        if u == Unit::Char(']') && !first {
            i += 1;
            break;
        }
        first = false;
        // Classe [:nome:].
        if u == Unit::Char('[')
            && p.get(i + 1) == Some(&Unit::Char(':'))
            && let Some(end) = (i + 2..p.len().saturating_sub(1)).find(|&k| p[k] == Unit::Char(':') && p[k + 1] == Unit::Char(']'))
        {
            let name: String = p[i + 2..end].iter().filter_map(|u| u.as_char()).collect();
            if let (Some(k), Unit::Char(ch)) = (PosixClass::from_name(&name), c) {
                matched |= class_contains(posix_class(k), ch);
            }
            i = end + 2;
            continue;
        }
        let lo = if u == Unit::Char('\\') {
            i += 1;
            *p.get(i)?
        } else {
            u
        };
        i += 1;
        if p.get(i) == Some(&Unit::Char('-')) && p.get(i + 1).is_some_and(|x| *x != Unit::Char(']')) {
            let mut hi = p[i + 1];
            i += 2;
            if hi == Unit::Char('\\') {
                hi = *p.get(i)?;
                i += 1;
            }
            if let (Unit::Char(a), Unit::Char(b), Unit::Char(x)) = (lo, hi, c) {
                matched |= a <= x && x <= b;
            }
        } else {
            matched |= lo == c;
        }
    }
    Some((matched != negate, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, s: &str) -> bool {
        fnmatch(&decode(p.as_bytes()), &decode(s.as_bytes()))
    }

    #[test]
    fn fnmatch_basics() {
        assert!(m("*.rs", "main.rs"));
        assert!(m("*.rs", "src/main.rs"));
        assert!(!m("*.rs", "main.py"));
        assert!(m("[ab]?.txt", "ax.txt"));
        assert!(m("[!a]*", "b"));
        assert!(!m("[!a]*", "a"));
        assert!(m("[[:digit:]]x", "5x"));
        assert!(m("a\\*", "a*"));
        assert!(m("[", "["));
    }

    #[test]
    fn include_exclude_rules() {
        let mut e = Excludes::default();
        e.add(b"*.c", true);
        assert!(!e.excluded(b"a.c", true));
        assert!(e.excluded(b"a.h", true));
        let mut e = Excludes::default();
        e.add(b"a*", false);
        e.add(b"*b", true);
        assert!(!e.excluded(b"zzz", true));
        assert!(e.excluded(b"ax", true));
        assert!(!e.excluded(b"ab", true));
    }
}
