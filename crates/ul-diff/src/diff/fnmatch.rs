//! `fnmatch(3)` sem flags (como o `diff -x` usa nos nomes de arquivo): `*`, `?`, `[...]` com
//! negação `!`/`^`, faixas e classes `[:alpha:]`, e `\` escapando o caractere seguinte. Byte a byte.

pub fn fnmatch(pattern: &[u8], name: &[u8], ignore_case: bool) -> bool {
    matches(pattern, name, ignore_case, 0)
}

fn fold(b: u8, ignore_case: bool) -> u8 {
    if ignore_case { b.to_ascii_lowercase() } else { b }
}

fn matches(p: &[u8], s: &[u8], ic: bool, depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    let (mut pi, mut si) = (0usize, 0usize);
    // Retrocesso só pro último `*` (suficiente pro fnmatch sem FNM_PATHNAME).
    let mut star: Option<(usize, usize)> = None;
    loop {
        if pi < p.len() {
            match p[pi] {
                b'*' => {
                    while pi < p.len() && p[pi] == b'*' {
                        pi += 1;
                    }
                    if pi == p.len() {
                        return true;
                    }
                    star = Some((pi, si));
                    continue;
                }
                b'?' if si < s.len() => {
                    pi += 1;
                    si += 1;
                    continue;
                }
                b'[' if si < s.len() => {
                    if let Some((ok, next)) = bracket(p, pi, s[si], ic) {
                        if ok {
                            pi = next;
                            si += 1;
                            continue;
                        }
                    } else if s[si] == b'[' {
                        pi += 1;
                        si += 1;
                        continue;
                    }
                }
                b'\\' if pi + 1 < p.len() && si < s.len() => {
                    if fold(p[pi + 1], ic) == fold(s[si], ic) {
                        pi += 2;
                        si += 1;
                        continue;
                    }
                }
                c if si < s.len() && c != b'*' && c != b'?' && c != b'[' && c != b'\\' => {
                    if fold(c, ic) == fold(s[si], ic) {
                        pi += 1;
                        si += 1;
                        continue;
                    }
                }
                b'\\' if pi + 1 == p.len() && si < s.len() && s[si] == b'\\' => {
                    pi += 1;
                    si += 1;
                    continue;
                }
                _ => {}
            }
        } else if si == s.len() {
            return true;
        }
        // Falhou: volta pro último `*`, se houver.
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

/// Expressão de colchetes começando em `p[start] == '['`. Devolve (casou, índice depois do `]`), ou
/// `None` se não há `]` de fechamento (o `[` vira literal).
fn bracket(p: &[u8], start: usize, c: u8, ic: bool) -> Option<(bool, usize)> {
    let mut i = start + 1;
    let negate = matches!(p.get(i), Some(b'!') | Some(b'^'));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    loop {
        let b = *p.get(i)?;
        if b == b']' && !first {
            i += 1;
            break;
        }
        first = false;
        if b == b'[' && p.get(i + 1) == Some(&b':') {
            if let Some(end) = p[i + 2..].windows(2).position(|w| w == b":]") {
                let name = &p[i + 2..i + 2 + end];
                if class_matches(name, c) {
                    matched = true;
                }
                i += 2 + end + 2;
                continue;
            }
        }
        let lo = if b == b'\\' && i + 1 < p.len() {
            i += 1;
            p[i]
        } else {
            b
        };
        if p.get(i + 1) == Some(&b'-') && p.get(i + 2).is_some_and(|&n| n != b']') {
            let mut hi = p[i + 2];
            let mut adv = 3;
            if hi == b'\\' && i + 3 < p.len() {
                hi = p[i + 3];
                adv = 4;
            }
            let (cl, cc, ch) = (fold(lo, ic), fold(c, ic), fold(hi, ic));
            if (lo <= c && c <= hi) || (cl <= cc && cc <= ch) {
                matched = true;
            }
            i += adv;
        } else {
            if fold(lo, ic) == fold(c, ic) {
                matched = true;
            }
            i += 1;
        }
    }
    Some((matched != negate, i))
}

fn class_matches(name: &[u8], c: u8) -> bool {
    match name {
        b"alpha" => c.is_ascii_alphabetic(),
        b"digit" => c.is_ascii_digit(),
        b"alnum" => c.is_ascii_alphanumeric(),
        b"upper" => c.is_ascii_uppercase(),
        b"lower" => c.is_ascii_lowercase(),
        b"space" => matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c'),
        b"blank" => matches!(c, b' ' | b'\t'),
        b"punct" => c.is_ascii_punctuation(),
        b"print" => (0x20..0x7f).contains(&c),
        b"graph" => (0x21..0x7f).contains(&c),
        b"cntrl" => c < 0x20 || c == 0x7f,
        b"xdigit" => c.is_ascii_hexdigit(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        assert!(fnmatch(b"*.o", b"a.o", false));
        assert!(!fnmatch(b"*.o", b"a.c", false));
        assert!(fnmatch(b"*", b".hidden", false));
        assert!(fnmatch(b"a?c", b"abc", false));
        assert!(fnmatch(b"[a-c]x", b"bx", false));
        assert!(fnmatch(b"[!a-c]x", b"dx", false));
        assert!(fnmatch(b"[[:digit:]]*", b"9z", false));
        assert!(fnmatch(b"\\*", b"*", false));
        assert!(fnmatch(b"*a*b*", b"xxaxxbxx", false));
        assert!(fnmatch(b"ABC", b"abc", true));
        assert!(fnmatch(b"[", b"[", false));
    }
}
