//! `fnmatch(3)` da glibc sem flags: `*`, `?`, listas `[...]` (negação com `!` ou `^`, faixas, classes
//! `[:alpha:]`...) e `\` escapando o caractere seguinte. `/` e o ponto inicial não têm tratamento
//! especial. Opera sobre caracteres (UTF-8 inválido vira U+FFFD), como a glibc em C.UTF-8.

pub fn fnmatch(pattern: &[u8], name: &[u8]) -> bool {
    let p: Vec<char> = String::from_utf8_lossy(pattern).chars().collect();
    let s: Vec<char> = String::from_utf8_lossy(name).chars().collect();
    matches(&p, 0, &s, 0)
}

fn class_matches(class: &str, c: char) -> Option<bool> {
    Some(match class {
        "alpha" => c.is_alphabetic(),
        "digit" => c.is_ascii_digit(),
        "alnum" => c.is_alphanumeric(),
        "upper" => c.is_uppercase(),
        "lower" => c.is_lowercase(),
        "space" => matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'),
        "blank" => c == ' ' || c == '\t',
        "punct" => {
            c.is_ascii_punctuation()
                || (!c.is_ascii() && !c.is_alphanumeric() && !c.is_whitespace() && !c.is_control())
        }
        "print" => !c.is_control(),
        "graph" => !c.is_control() && !c.is_whitespace(),
        "cntrl" => c.is_control(),
        "xdigit" => c.is_ascii_hexdigit(),
        _ => return None,
    })
}

/// Lista `[...]` que começa em `p[start] == '['`: `Some((casou, índice depois do ']'))`, ou `None`
/// quando a lista não fecha (e o `[` vale como caractere comum).
fn bracket(p: &[char], start: usize, c: char) -> Option<(bool, usize)> {
    let mut i = start + 1;
    let negate = matches!(p.get(i), Some('!') | Some('^'));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    loop {
        let ch = *p.get(i)?;
        if ch == ']' && !first {
            i += 1;
            break;
        }
        first = false;
        if ch == '['
            && p.get(i + 1) == Some(&':')
            && let Some(end) =
                (i + 2..p.len().saturating_sub(1)).find(|&k| p[k] == ':' && p[k + 1] == ']')
        {
            let name: String = p[i + 2..end].iter().collect();
            if let Some(m) = class_matches(&name, c) {
                matched |= m;
                i = end + 2;
                continue;
            }
        }
        let lo = if ch == '\\' {
            i += 1;
            *p.get(i)?
        } else {
            ch
        };
        i += 1;
        if p.get(i) == Some(&'-') && p.get(i + 1).is_some_and(|&n| n != ']') {
            let mut hi = p[i + 1];
            i += 2;
            if hi == '\\' {
                hi = *p.get(i)?;
                i += 1;
            }
            if lo <= c && c <= hi {
                matched = true;
            }
        } else if lo == c {
            matched = true;
        }
    }
    Some((matched != negate, i))
}

fn matches(p: &[char], mut pi: usize, s: &[char], mut si: usize) -> bool {
    loop {
        let Some(&pc) = p.get(pi) else {
            return si == s.len();
        };
        match pc {
            '*' => {
                while p.get(pi) == Some(&'*') {
                    pi += 1;
                }
                if pi == p.len() {
                    return true;
                }
                return (si..=s.len()).any(|k| matches(p, pi, s, k));
            }
            '?' => {
                if si >= s.len() {
                    return false;
                }
                pi += 1;
                si += 1;
            }
            '[' => {
                let Some(&c) = s.get(si) else { return false };
                match bracket(p, pi, c) {
                    Some((true, next)) => {
                        pi = next;
                        si += 1;
                    }
                    Some((false, _)) => return false,
                    None => {
                        if c != '[' {
                            return false;
                        }
                        pi += 1;
                        si += 1;
                    }
                }
            }
            '\\' => {
                let Some(&lit) = p.get(pi + 1) else {
                    return false;
                };
                if s.get(si) != Some(&lit) {
                    return false;
                }
                pi += 2;
                si += 1;
            }
            c => {
                if s.get(si) != Some(&c) {
                    return false;
                }
                pi += 1;
                si += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fnmatch;

    #[test]
    fn globs() {
        assert!(fnmatch(b"l*", b"ls"));
        assert!(fnmatch(b"*", b""));
        assert!(fnmatch(b"?s", b"ls"));
        assert!(!fnmatch(b"?s", b"s"));
        assert!(fnmatch(b"[a-c]x", b"bx"));
        assert!(!fnmatch(b"[!a-c]x", b"bx"));
        assert!(fnmatch(b"[[:digit:]]*", b"7up"));
        assert!(fnmatch(b"a\\*b", b"a*b"));
        assert!(!fnmatch(b"a\\*b", b"axb"));
        assert!(fnmatch(b"[]]", b"]"));
        assert!(fnmatch(b"[", b"["));
        assert!(fnmatch(b".*", b".hidden"));
    }
}
