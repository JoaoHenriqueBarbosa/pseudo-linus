//! Aspas de caminho no estilo C do git (`quote_c_style`), com `core.quotePath`.

/// Precisa de aspas?
fn must_quote(c: u8, fully: bool) -> bool {
    match c {
        0..=0x1f | b'"' | b'\\' | 0x7f => true,
        0x80..=0xff => fully,
        _ => false,
    }
}

fn escape(c: u8) -> Option<u8> {
    Some(match c {
        7 => b'a',
        8 => b'b',
        9 => b't',
        10 => b'n',
        11 => b'v',
        12 => b'f',
        13 => b'r',
        b'"' => b'"',
        b'\\' => b'\\',
        _ => return None,
    })
}

/// Caminho entre aspas se precisar (`"a\tb"`, `"\303\251"`), ou como está.
pub fn quote_c(path: &[u8], fully: bool) -> Vec<u8> {
    if !path.iter().any(|c| must_quote(*c, fully)) {
        return path.to_vec();
    }
    let mut out = vec![b'"'];
    out.extend_from_slice(&quote_body(path, fully));
    out.push(b'"');
    out
}

/// Só o miolo escapado (sem as aspas de fora).
pub fn quote_body(path: &[u8], fully: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for &c in path {
        if must_quote(c, fully) {
            out.push(b'\\');
            match escape(c) {
                Some(e) => out.push(e),
                None => out.extend_from_slice(format!("{c:03o}").as_bytes()),
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn needs_quote(path: &[u8], fully: bool) -> bool {
    path.iter().any(|c| must_quote(*c, fully))
}

/// `prefix` + caminho com aspas no todo (`"a/b c"` vira `"a/\303..."`), como os cabeçalhos do diff.
pub fn quote_two(prefix: &[u8], path: &[u8], fully: bool) -> Vec<u8> {
    let mut whole = prefix.to_vec();
    whole.extend_from_slice(path);
    quote_c(&whole, fully)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(quote_c(b"a.txt", true), b"a.txt");
        assert_eq!(quote_c(b"a\tb", true), b"\"a\\tb\"");
        assert_eq!(quote_c("é".as_bytes(), true), b"\"\\303\\251\"");
        assert_eq!(quote_c("é".as_bytes(), false), "é".as_bytes());
        assert_eq!(quote_c(b"a b", true), b"a b");
    }
}
