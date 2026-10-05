//! Padrões de nome no estilo do sh (match.c): `?`, `*`, `[...]` e `\`. Com `sepc` (a opção `-W`),
//! `?` e `*` não casam a barra e `**` casa.

/// O padrão casa o nome inteiro.
pub fn matches(name: &[u8], pattern: &[u8], ignore_case: bool, sepc: Option<u8>) -> bool {
    recmatch(pattern, name, ignore_case, sepc) == 1
}

/// Tem algum caractere especial fora de escape (`iswild`).
pub fn is_wild(p: &[u8]) -> bool {
    shexp(p).is_some()
}

/// Posição do primeiro caractere especial (`isshexp`).
fn shexp(p: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < p.len() {
        if p[i] == b'\\' && i + 1 < p.len() {
            i += 1;
        } else if matches!(p[i], b'?' | b'*' | b'[') {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn case(c: u8, ic: bool) -> u8 {
    if ic { c.to_ascii_lowercase() } else { c }
}

/// 1 casa, 0 não casa, 2 desiste (nenhum sufixo vai casar).
fn recmatch(p: &[u8], s: &[u8], ic: bool, sepc: Option<u8>) -> i32 {
    let Some((&c, p)) = p.split_first() else { return i32::from(s.is_empty()) };
    match c {
        b'?' => match s.first() {
            Some(&x) if Some(x) != sepc => recmatch(p, &s[1..], ic, sepc),
            _ => 0,
        },
        b'*' => star(p, s, ic, sepc),
        b'[' => bracket(p, s, ic, sepc),
        // Depois de `\`, o próximo caractere vale literalmente; `\` no fim é erro de sintaxe.
        b'\\' => match p.split_first() {
            Some((&c, p)) => literal(c, p, s, ic, sepc),
            None => 0,
        },
        _ => literal(c, p, s, ic, sepc),
    }
}

/// `*` (já consumido) seguido do resto `p`.
fn star(mut p: &[u8], s: &[u8], ic: bool, sepc: Option<u8>) -> i32 {
    if let Some(sep) = sepc {
        if p.first() != Some(&b'*') {
            // `*` sozinho não atravessa a barra.
            let mut i = 0;
            while i < s.len() && s[i] != sep {
                let c = recmatch(p, &s[i..], ic, sepc);
                if c != 0 {
                    return c;
                }
                i += 1;
            }
            if p.is_empty() {
                return i32::from(i == s.len());
            }
            return if p[0] == sep || (p[0] == b'\\' && p.get(1) == Some(&sep)) { recmatch(p, &s[i..], ic, sepc) } else { 2 };
        }
        p = &p[1..];
    }
    if p.is_empty() {
        return 1;
    }
    if shexp(p).is_none() {
        // O resto do padrão é literal: compara com o fim do nome, sem tratar os escapes.
        if s.len() < p.len() {
            return 0;
        }
        let tail = &s[s.len() - p.len()..];
        return i32::from(if ic { tail.eq_ignore_ascii_case(p) } else { tail == p });
    }
    for i in 0..s.len() {
        let c = recmatch(p, &s[i..], ic, sepc);
        if c != 0 {
            return c;
        }
    }
    2
}

/// `[...]` (o `[` já consumido).
fn bracket(p: &[u8], s: &[u8], ic: bool, sepc: Option<u8>) -> i32 {
    let Some(&x) = s.first() else { return 0 };
    let reverse = matches!(p.first(), Some(b'!' | b'^'));
    let start = usize::from(reverse);
    let mut q = start;
    let mut esc = false;
    while q < p.len() {
        if esc {
            esc = false;
        } else if p[q] == b'\\' {
            esc = true;
        } else if p[q] == b']' {
            break;
        }
        q += 1;
    }
    if q >= p.len() {
        return 0;
    }
    let cc = case(x, ic);
    let mut lo: u32 = 0;
    let mut esc = p.get(start) == Some(&b'-');
    for i in start..q {
        if !esc && p[i] == b'\\' {
            esc = true;
        } else if !esc && p[i] == b'-' {
            lo = u32::from(p[i - 1]);
        } else {
            if p[i + 1] != b'-' {
                let mut c = if lo != 0 { lo } else { u32::from(p[i]) };
                while c <= u32::from(p[i]) {
                    if case(c as u8, ic) == cc {
                        return if reverse { 0 } else { recmatch(&p[q + 1..], &s[1..], ic, sepc) };
                    }
                    c += 1;
                }
            }
            lo = 0;
            esc = false;
        }
    }
    if reverse { recmatch(&p[q + 1..], &s[1..], ic, sepc) } else { 0 }
}

fn literal(c: u8, p: &[u8], s: &[u8], ic: bool, sepc: Option<u8>) -> i32 {
    match s.first() {
        Some(&x) if case(x, ic) == case(c, ic) => recmatch(p, &s[1..], ic, sepc),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn patterns() {
        let m = |s: &str, p: &str| matches(s.as_bytes(), p.as_bytes(), false, None);
        assert!(m("a.txt", "*.txt"));
        assert!(m("dir/a.txt", "*.txt"));
        assert!(!m("a.txt", "*.TXT"));
        assert!(matches(b"a.txt", b"*.TXT", true, None));
        assert!(m("b1", "[a-c]?"));
        assert!(!m("d1", "[a-c]?"));
        assert!(m("d1", "[!a-c]1"));
        assert!(m("a*", "a\\*"));
        assert!(!m("ab", "a\\*"));
        // `**` sem -W exige um caractere a mais (o laço não testa a posição vazia).
        assert!(!m("a", "a**"));
        assert!(!matches(b"d/a", b"*a", false, Some(b'/')));
        assert!(matches(b"d/a", b"**a", false, Some(b'/')));
        assert!(matches(b"d/a", b"*/a", false, Some(b'/')));
    }
}
