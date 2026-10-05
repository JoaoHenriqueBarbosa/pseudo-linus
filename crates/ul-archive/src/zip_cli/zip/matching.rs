//! Casamento de curingas e comparação de nomes (util.c): `shmatch`, `namecmp`, `isshexp`.

/// `recmatch`: devolve 1 se casa, 0 se não, 2 se a recursão deve desistir (erro de sintaxe do
/// padrão ou `*` sem como casar).
fn recmatch(p: &[u8], s: &[u8], cs: bool, no_wild: bool, wild_stop_at_dir: bool, allow_regex: bool) -> i32 {
    let mut p = p;
    let mut s = s;
    loop {
        // Primeiro caractere do padrão. Fim do padrão: casa se a cadeia também acabou.
        let c = match p.first() {
            None => return (s.is_empty()) as i32,
            Some(&c) => c,
        };
        p = &p[1..];

        // '?' casa qualquer caractere (mas não a cadeia vazia).
        if c == b'?' {
            if wild_stop_at_dir {
                if !s.is_empty() && s[0] != b'/' {
                    s = &s[1..];
                    continue;
                }
                return 0;
            }
            if !s.is_empty() {
                s = &s[1..];
                continue;
            }
            return 0;
        }

        // '*' casa qualquer quantidade de caracteres, inclusive zero.
        if !no_wild && c == b'*' {
            if wild_stop_at_dir {
                if p.first() != Some(&b'*') {
                    // Um '*' só: não casa barras.
                    let mut i = 0usize;
                    while i < s.len() && s[i] != b'/' {
                        let r = recmatch(p, &s[i..], cs, no_wild, wild_stop_at_dir, allow_regex);
                        if r != 0 {
                            return r;
                        }
                        i += 1;
                    }
                    if p.is_empty() {
                        return (i >= s.len()) as i32;
                    }
                    let next_is_dir = p[0] == b'/' || (p[0] == b'\\' && p.get(1) == Some(&b'/'));
                    return if next_is_dir { recmatch(p, &s[i..], cs, no_wild, wild_stop_at_dir, allow_regex) } else { 2 };
                }
                // "**": casa as barras também; segue com o código normal.
                p = &p[1..];
            }
            if p.is_empty() {
                return 1;
            }
            if isshexp(p, no_wild, allow_regex).is_none() {
                // O resto do padrão é literal: compara com o fim da cadeia.
                if s.len() < p.len() {
                    return 0;
                }
                let srest = &s[s.len() - p.len()..];
                return if cs { (p == srest) as i32 } else { (namecmp(p, srest) == 0) as i32 };
            }
            let mut i = 0usize;
            while i < s.len() {
                let r = recmatch(p, &s[i..], cs, no_wild, wild_stop_at_dir, allow_regex);
                if r != 0 {
                    return r;
                }
                i += 1;
            }
            return 2;
        }

        // Lista entre colchetes.
        if !no_wild && allow_regex && c == b'[' {
            if s.is_empty() {
                return 0;
            }
            let rev = matches!(p.first(), Some(b'!') | Some(b'^'));
            if rev {
                p = &p[1..];
            }
            // Procura o colchete que fecha.
            let mut e = false;
            let mut q = 0usize;
            while q < p.len() {
                if e {
                    e = false;
                } else if p[q] == b'\\' {
                    e = true;
                } else if p[q] == b']' {
                    break;
                }
                q += 1;
            }
            if q >= p.len() {
                return 0;
            }
            let mut cc_range: u8 = 0;
            let mut e = p.first() == Some(&b'-');
            let mut pi = 0usize;
            while pi < q {
                if !e && p[pi] == b'\\' {
                    e = true;
                } else if !e && p[pi] == b'-' {
                    cc_range = if pi > 0 { p[pi - 1] } else { 0 };
                } else {
                    let cc = s[0];
                    if p.get(pi + 1) != Some(&b'-') {
                        let mut uc = if cc_range != 0 { cc_range } else { p[pi] };
                        while uc <= p[pi] {
                            if uc == cc {
                                return if rev { 0 } else { recmatch(&p[q + 1..], &s[1..], cs, no_wild, wild_stop_at_dir, allow_regex) };
                            }
                            if uc == 255 {
                                break;
                            }
                            uc += 1;
                        }
                    }
                    cc_range = 0;
                    e = false;
                }
                pi += 1;
            }
            return if rev { recmatch(&p[q + 1..], &s[1..], cs, no_wild, wild_stop_at_dir, allow_regex) } else { 0 };
        }

        // Escape: compara o caractere seguinte literalmente.
        let mut c = c;
        if !no_wild && c == b'\\' {
            match p.first() {
                None => return 0,
                Some(&n) => {
                    c = n;
                    p = &p[1..];
                }
            }
        }

        // Um caractere comum.
        if !s.is_empty() && c == s[0] {
            s = &s[1..];
            continue;
        }
        return 0;
    }
}

/// `shmatch`: o padrão `p` casa com `s`? `cs` força a comparação com distinção de maiúsculas.
pub fn shmatch(p: &[u8], s: &[u8], cs: bool, no_wild: bool, wild_stop_at_dir: bool, allow_regex: bool) -> bool {
    recmatch(p, s, cs, no_wild, wild_stop_at_dir, allow_regex) == 1
}

/// `isshexp`: a posição do primeiro caractere especial do padrão, se houver.
pub fn isshexp(p: &[u8], _no_wild: bool, _allow_regex: bool) -> Option<usize> {
    let mut i = 0;
    while i < p.len() {
        if p[i] == b'\\' && i + 1 < p.len() {
            i += 1;
        } else if p[i] == b'?' || p[i] == b'*' || p[i] == b'[' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `namecmp`: diferença do primeiro byte diferente (como `strcmp`; no Unix não ignora maiúsculas).
pub fn namecmp(a: &[u8], b: &[u8]) -> i32 {
    let mut i = 0;
    loop {
        let x = a.get(i).copied().unwrap_or(0) as i32;
        let y = b.get(i).copied().unwrap_or(0) as i32;
        let d = x - y;
        if d != 0 || x == 0 || y == 0 {
            return d;
        }
        i += 1;
    }
}
