//! Nomes de arquivos (fileio.c e unix.c): conversão do nome externo para o nome interno do zip,
//! `msname`, versão para exibição, leitura de nomes de um fluxo (`getnam`) e UTF-8.

/// `last(p, c)`: o começo do último componente do caminho.
pub fn last(p: &[u8], c: u8) -> &[u8] {
    match p.iter().rposition(|&b| b == c) {
        Some(i) => &p[i + 1..],
        None => p,
    }
}

/// `ex2in`: tira o `//host/share/` de um nome UNC, as barras iniciais e os `./` redundantes, e com
/// `pathput == false` só o último componente; com `dosify` aplica `msname`.
pub fn ex2in(x: &[u8], pathput: bool, dosify: bool) -> Vec<u8> {
    let mut t: &[u8] = x;
    let mut have_t = true;
    if x.len() >= 2 && &x[..2] == b"//" && x.get(2).is_some_and(|&c| c != 0 && c != b'/') {
        let mut n = 2usize;
        while n < x.len() && x[n] != b'/' {
            n += 1;
        }
        if n < x.len() {
            n += 1;
            while n < x.len() && x[n] != b'/' {
                n += 1;
            }
        }
        if n < x.len() {
            t = &x[n + 1..];
        } else {
            // O C deixa `t` nulo aqui e o resto do código assume nome vazio.
            t = &[];
            have_t = false;
        }
    }
    let _ = have_t;
    while t.first() == Some(&b'/') {
        t = &t[1..];
    }
    while t.len() >= 2 && t[0] == b'.' && t[1] == b'/' {
        t = &t[2..];
    }
    if !pathput {
        t = last(t, b'/');
    }
    let mut n = t.to_vec();
    if dosify {
        msname(&mut n);
    }
    n
}

/// `msname`: reduz cada componente do caminho a um nome 8.3 em maiúsculas do MS-DOS.
pub fn msname(n: &mut Vec<u8>) {
    let src = std::mem::take(n);
    let mut out: Vec<u8> = Vec::with_capacity(src.len());
    let mut f: i32 = 0;
    for &c in &src {
        if matches!(c, b' ' | b':' | b'"' | b'*' | b'+' | b',' | b';' | b'<' | b'=' | b'>' | b'?' | b'[' | b']' | b'|') {
            continue;
        } else if c == b'/' {
            out.push(c);
            f = 0;
        } else if c == b'.' {
            if f == 0 {
                continue;
            } else if f < 9 {
                out.push(c);
                f = 9;
            } else {
                f = 12;
            }
        } else if f < 12 && f != 8 {
            f += 1;
            out.push(c.to_ascii_uppercase());
        }
    }
    *n = out;
}

/// `local_to_display_string`: os caracteres de controle viram `^X`.
pub fn display_name(local: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(local.len() + 2);
    for &c in local {
        if c == 0 {
            break;
        }
        if c < b' ' {
            out.push(b'^');
            out.push(b'@'.wrapping_add(c));
        } else {
            out.push(c);
        }
    }
    out
}

/// `is_ascii_string`.
pub fn is_ascii(s: &[u8]) -> bool {
    s.iter().all(|&c| c <= 0x7f)
}

/// `local_to_utf8_string` num locale UTF-8: o próprio nome, se for UTF-8 válido (o `mbstowcs` do C
/// falha nos nomes inválidos, e aí não há nome Unicode).
pub fn local_to_utf8(local: &[u8]) -> Option<Vec<u8>> {
    std::str::from_utf8(local).ok().map(|_| local.to_vec())
}

/// `getnam`: o próximo nome de um fluxo, separado por `\n` ou `\r` (linhas vazias ignoradas). `pos`
/// avança. Devolve `None` no fim ou se o nome passa de 9000 bytes.
pub fn getnam(data: &[u8], pos: &mut usize) -> Option<Vec<u8>> {
    const GETNAM_MAX: usize = 9000;
    while *pos < data.len() && (data[*pos] == b'\n' || data[*pos] == b'\r') {
        *pos += 1;
    }
    if *pos >= data.len() {
        return None;
    }
    let mut name = Vec::new();
    while *pos < data.len() && data[*pos] != b'\n' && data[*pos] != b'\r' {
        if name.len() >= GETNAM_MAX {
            return None;
        }
        name.push(data[*pos]);
        *pos += 1;
    }
    Some(name)
}
