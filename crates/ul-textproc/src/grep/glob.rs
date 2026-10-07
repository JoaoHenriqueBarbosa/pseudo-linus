//! `--include`, `--exclude` e `--exclude-dir`: o `fnmatch` do glibc sem flags (o `*` atravessa
//! `/`) e a regra do `excluded_file_name` do gnulib (vale o último padrão que casa; sem nenhum
//! casando, vale o contrário do primeiro).

use regex_posix::ast::{PosixClass, Unit, decode};
use regex_posix::charclass::{class_contains, posix_class};
use ul_common::fnmatch::{Alphabet, Flags, fnmatch};

/// As unidades do regex-posix (caractere UTF-8 ou byte inválido) como texto do `fnmatch`: classes
/// pelo `charclass` do regex-posix, faixa só entre caracteres, byte inválido só casa com ele mesmo.
struct Units;

impl Alphabet for Units {
    type Unit = Unit;

    fn is(unit: Unit, ascii: u8) -> bool {
        unit.is(char::from(ascii))
    }

    fn in_range(lo: Unit, hi: Unit, c: Unit) -> bool {
        matches!((lo, hi, c), (Unit::Char(a), Unit::Char(b), Unit::Char(x)) if a <= x && x <= b)
    }

    fn in_class(name: &[Unit], c: Unit) -> Option<bool> {
        let name: String = name.iter().filter_map(|u| u.as_char()).collect();
        Some(match (PosixClass::from_name(&name), c) {
            (Some(k), Unit::Char(ch)) => class_contains(posix_class(k), ch),
            _ => false,
        })
    }
}

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
            if fnmatch::<Units>(pat, &name, Flags::TRAILING_BACKSLASH_LITERAL) {
                return true;
            }
            if !anchored {
                for i in 0..name.len() {
                    if name[i] == Unit::Char('/')
                        && name.get(i + 1) != Some(&Unit::Char('/'))
                        && fnmatch::<Units>(pat, &name[i + 1..], Flags::TRAILING_BACKSLASH_LITERAL)
                    {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, s: &str) -> bool {
        fnmatch::<Units>(&decode(p.as_bytes()), &decode(s.as_bytes()), Flags::TRAILING_BACKSLASH_LITERAL)
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
