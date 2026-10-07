//! Funções de texto do shell.c: aspas SQL, C, JSON, HTML e CSV, largura em caracteres UTF-8,
//! `isNumber`, `integerValue`, `booleanValue`, `resolve_backslashes` e `quoteChar`.

/// Palavras-chave do SQLite 3.46.1 (`sqlite3_keyword_check`).
const KEYWORDS: &[&str] = &[
    "ABORT", "ACTION", "ADD", "AFTER", "ALL", "ALTER", "ALWAYS", "ANALYZE", "AND", "AS", "ASC", "ATTACH",
    "AUTOINCREMENT", "BEFORE", "BEGIN", "BETWEEN", "BY", "CASCADE", "CASE", "CAST", "CHECK", "COLLATE",
    "COLUMN", "COMMIT", "CONFLICT", "CONSTRAINT", "CREATE", "CROSS", "CURRENT", "CURRENT_DATE",
    "CURRENT_TIME", "CURRENT_TIMESTAMP", "DATABASE", "DEFAULT", "DEFERRABLE", "DEFERRED", "DELETE", "DESC",
    "DETACH", "DISTINCT", "DO", "DROP", "EACH", "ELSE", "END", "ESCAPE", "EXCEPT", "EXCLUDE", "EXCLUSIVE",
    "EXISTS", "EXPLAIN", "FAIL", "FILTER", "FIRST", "FOLLOWING", "FOR", "FOREIGN", "FROM", "FULL",
    "GENERATED", "GLOB", "GROUP", "GROUPS", "HAVING", "IF", "IGNORE", "IMMEDIATE", "IN", "INDEX", "INDEXED",
    "INITIALLY", "INNER", "INSERT", "INSTEAD", "INTERSECT", "INTO", "IS", "ISNULL", "JOIN", "KEY", "LAST",
    "LEFT", "LIKE", "LIMIT", "MATCH", "MATERIALIZED", "NATURAL", "NO", "NOT", "NOTHING", "NOTNULL", "NULL",
    "NULLS", "OF", "OFFSET", "ON", "OR", "ORDER", "OTHERS", "OUTER", "OVER", "PARTITION", "PLAN", "PRAGMA",
    "PRECEDING", "PRIMARY", "QUERY", "RAISE", "RANGE", "RECURSIVE", "REFERENCES", "REGEXP", "REINDEX",
    "RELEASE", "RENAME", "REPLACE", "RESTRICT", "RETURNING", "RIGHT", "ROLLBACK", "ROW", "ROWS", "SAVEPOINT",
    "SELECT", "SET", "TABLE", "TEMP", "TEMPORARY", "THEN", "TIES", "TO", "TRANSACTION", "TRIGGER",
    "UNBOUNDED", "UNION", "UNIQUE", "UPDATE", "USING", "VACUUM", "VALUES", "VIEW", "VIRTUAL", "WHEN", "WHERE",
    "WINDOW", "WITH", "WITHOUT",
];

pub fn is_keyword(z: &[u8]) -> bool {
    KEYWORDS.iter().any(|k| k.as_bytes().eq_ignore_ascii_case(z))
}

/// `quoteChar`: `"` quando o identificador precisa de aspas.
pub fn needs_quote(name: &[u8]) -> bool {
    let Some(&first) = name.first() else { return true };
    if !first.is_ascii_alphabetic() && first != b'_' {
        return true;
    }
    if name.iter().any(|&c| !c.is_ascii_alphanumeric() && c != b'_') {
        return true;
    }
    is_keyword(name)
}

/// `%w` entre aspas duplas: `"a""b"`.
pub fn dquote(z: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(z.len() + 2);
    out.push(b'"');
    for &c in z {
        out.push(c);
        if c == b'"' {
            out.push(b'"');
        }
    }
    out.push(b'"');
    out
}

/// `%Q`/`%q` entre aspas simples: `'it''s'`.
pub fn squote(z: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(z.len() + 2);
    out.push(b'\'');
    for &c in z {
        out.push(c);
        if c == b'\'' {
            out.push(b'\'');
        }
    }
    out.push(b'\'');
    out
}

/// Identificador citado só quando precisa (como o `set_table_name`).
pub fn quote_ident_if_needed(z: &[u8]) -> Vec<u8> {
    if needs_quote(z) { dquote(z) } else { z.to_vec() }
}

/// Corta no primeiro NUL, como as funções que recebem `char*`.
pub use ul_common::ctype::cstr;

/// `output_quoted_string`.
pub fn quoted_string(z: &[u8]) -> Vec<u8> {
    squote(cstr(z))
}

/// `unused_string`.
fn unused_string(z: &[u8], a: &str, b: &str) -> String {
    let contains = |needle: &[u8]| z.windows(needle.len()).any(|w| w == needle);
    if !contains(a.as_bytes()) {
        return a.to_string();
    }
    if !contains(b.as_bytes()) {
        return b.to_string();
    }
    let mut i = 0u32;
    loop {
        let cand = format!("({a}{i})");
        i += 1;
        if !contains(cand.as_bytes()) {
            return cand;
        }
    }
}

/// `output_quoted_escaped_string`: como o anterior, mas \n e \r viram `replace(..., char(10))`.
pub fn quoted_escaped_string(z: &[u8]) -> Vec<u8> {
    let z = cstr(z);
    if !z.iter().any(|&c| c == b'\'' || c == b'\n' || c == b'\r') {
        return squote(z);
    }
    let n_nl = z.iter().filter(|&&c| c == b'\n').count();
    let n_cr = z.iter().filter(|&&c| c == b'\r').count();
    let mut out = Vec::new();
    let mut nl = String::new();
    let mut cr = String::new();
    if n_nl > 0 {
        out.extend_from_slice(b"replace(");
        nl = unused_string(z, "\\n", "\\012");
    }
    if n_cr > 0 {
        out.extend_from_slice(b"replace(");
        cr = unused_string(z, "\\r", "\\015");
    }
    out.push(b'\'');
    for &c in z {
        match c {
            b'\'' => out.extend_from_slice(b"''"),
            b'\n' => out.extend_from_slice(nl.as_bytes()),
            b'\r' => out.extend_from_slice(cr.as_bytes()),
            _ => out.push(c),
        }
    }
    out.push(b'\'');
    if n_cr > 0 {
        out.extend_from_slice(format!(",'{cr}',char(13))").as_bytes());
    }
    if n_nl > 0 {
        out.extend_from_slice(format!(",'{nl}',char(10))").as_bytes());
    }
    out
}

/// Comprimento do prefixo que é UTF-8 válido sem caractere de controle (`zSkipValidUtf8` com
/// `ccm = ~0`).
fn skip_valid_utf8(z: &[u8]) -> usize {
    let mut i = 0;
    while i < z.len() {
        let c = z[i];
        if c & 0x80 == 0 {
            if c < 0x20 {
                return i;
            }
            i += 1;
        } else if c & 0xc0 != 0xc0 {
            return i;
        } else {
            let mut j = i + 1;
            let mut lead = c;
            loop {
                if j >= z.len() {
                    return i;
                }
                let ct = z[j];
                j += 1;
                if ct == 0 || j - i > 4 || ct & 0xc0 != 0x80 {
                    return i;
                }
                lead <<= 1;
                if lead & 0x40 != 0x40 {
                    break;
                }
            }
            i = j;
        }
    }
    i
}

/// `output_c_string` (modo tcl e `.auth`).
pub fn c_string(z: &[u8]) -> Vec<u8> {
    let mut z = cstr(z);
    let mut out = vec![b'"'];
    while !z.is_empty() {
        let past = skip_valid_utf8(z);
        let special = z.iter().position(|&c| c == b'"' || c == b'\\' || c == 0x7f);
        let end = match special {
            Some(p) if p < past => p,
            _ => past,
        };
        out.extend_from_slice(&z[..end]);
        if end >= z.len() {
            break;
        }
        let c = z[end];
        z = &z[end + 1..];
        let say = match c {
            b'\\' | b'"' => Some(c),
            b'\t' => Some(b't'),
            b'\n' => Some(b'n'),
            b'\r' => Some(b'r'),
            0x0c => Some(b'f'),
            _ => None,
        };
        if let Some(s) = say {
            out.push(b'\\');
            out.push(s);
        } else if !(0x20..0x7f).contains(&c) {
            out.extend_from_slice(format!("\\{:03o}", c).as_bytes());
        } else {
            out.push(c);
        }
    }
    out.push(b'"');
    out
}

/// `output_json_string`. `whole` diz se é texto C (para no NUL) ou um blob com tamanho.
pub fn json_string(z: &[u8], whole: bool) -> Vec<u8> {
    let mut z = if whole { z } else { cstr(z) };
    let mut out = vec![b'"'];
    while !z.is_empty() {
        let past = skip_valid_utf8(z);
        let special = z.iter().position(|&c| c == b'"' || c == b'\\');
        let end = match special {
            Some(p) if p < past => p,
            _ => past,
        };
        out.extend_from_slice(&z[..end]);
        if end >= z.len() {
            break;
        }
        let c = z[end];
        z = &z[end + 1..];
        let say = match c {
            b'"' | b'\\' => Some(c),
            0x08 => Some(b'b'),
            0x0c => Some(b'f'),
            b'\n' => Some(b'n'),
            b'\r' => Some(b'r'),
            b'\t' => Some(b't'),
            _ => None,
        };
        if let Some(s) = say {
            out.push(b'\\');
            out.push(s);
        } else if c <= 0x1f {
            // O shell.c 3.46.1 escreve "u%04x" sem a barra (é assim mesmo no original).
            out.extend_from_slice(format!("u{:04x}", c).as_bytes());
        } else {
            out.push(c);
        }
    }
    out.push(b'"');
    out
}

/// `output_html_string`.
pub fn html_string(z: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &c in cstr(z) {
        match c {
            b'<' => out.extend_from_slice(b"&lt;"),
            b'&' => out.extend_from_slice(b"&amp;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            b'\'' => out.extend_from_slice(b"&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// `needCsvQuote` do shell.c.
fn need_csv_quote(c: u8) -> bool {
    c <= 0x20 || c == b'"' || c == b'\'' || c >= 0x7f
}

/// Um termo de CSV (`output_csv` sem o separador).
pub fn csv_field(z: Option<&[u8]>, null_value: &[u8], col_sep: &[u8]) -> Vec<u8> {
    let Some(z) = z else { return null_value.to_vec() };
    let z = cstr(z);
    let quote = z.is_empty()
        || z.iter().any(|&c| need_csv_quote(c))
        || (!col_sep.is_empty() && z.windows(col_sep.len()).any(|w| w == col_sep));
    if quote { dquote(z) } else { z.to_vec() }
}

/// `strlenChar`: número de caracteres UTF-8 (bytes que não são de continuação).
pub fn strlen_char(z: &[u8]) -> usize {
    cstr(z).iter().filter(|&&c| c & 0xc0 != 0x80).count()
}

/// `utf8_width_print`: `w` caracteres, à direita se `w` < 0, cortando o que passar.
pub fn width_print(w: i32, z: &[u8]) -> Vec<u8> {
    let z = cstr(z);
    let aw = w.unsigned_abs() as usize;
    let mut n = 0;
    let mut i = 0;
    while i < z.len() {
        if z[i] & 0xc0 != 0x80 {
            n += 1;
            if n == aw {
                i += 1;
                while i < z.len() && z[i] & 0xc0 == 0x80 {
                    i += 1;
                }
                break;
            }
        }
        i += 1;
    }
    let mut out = Vec::new();
    if n >= aw {
        out.extend_from_slice(&z[..i]);
    } else if w < 0 {
        out.resize(aw - n, b' ');
        out.extend_from_slice(z);
    } else {
        out.extend_from_slice(z);
        out.resize(out.len() + aw - n, b' ');
    }
    out
}

/// `isNumber`.
pub fn is_number(z: &[u8]) -> bool {
    let z = cstr(z);
    let mut i = 0;
    if matches!(z.first(), Some(b'-' | b'+')) {
        i += 1;
    }
    if !z.get(i).is_some_and(u8::is_ascii_digit) {
        return false;
    }
    while z.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    if z.get(i) == Some(&b'.') {
        i += 1;
        if !z.get(i).is_some_and(u8::is_ascii_digit) {
            return false;
        }
        while z.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    if matches!(z.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(z.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if !z.get(i).is_some_and(u8::is_ascii_digit) {
            return false;
        }
        while z.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    i == z.len()
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// `integerValue`: decimal ou 0x, com sufixos KiB, MiB, GiB, KB, MB, GB, K, M, G.
pub fn integer_value(z: &[u8]) -> i64 {
    let mut z = z;
    let mut neg = false;
    if z.first() == Some(&b'-') {
        neg = true;
        z = &z[1..];
    } else if z.first() == Some(&b'+') {
        z = &z[1..];
    }
    let mut v: i64 = 0;
    if z.starts_with(b"0x") {
        z = &z[2..];
        while let Some(d) = z.first().and_then(|&c| hex_digit(c)) {
            v = v.wrapping_shl(4).wrapping_add(i64::from(d));
            z = &z[1..];
        }
    } else {
        while let Some(&c) = z.first().filter(|c| c.is_ascii_digit()) {
            v = v.wrapping_mul(10).wrapping_add(i64::from(c - b'0'));
            z = &z[1..];
        }
    }
    const MULT: &[(&str, i64)] = &[
        ("KiB", 1024),
        ("MiB", 1024 * 1024),
        ("GiB", 1024 * 1024 * 1024),
        ("KB", 1000),
        ("MB", 1_000_000),
        ("GB", 1_000_000_000),
        ("K", 1000),
        ("M", 1_000_000),
        ("G", 1_000_000_000),
    ];
    if let Some((_, m)) = MULT.iter().find(|(s, _)| s.as_bytes().eq_ignore_ascii_case(z)) {
        v = v.wrapping_mul(*m);
    }
    if neg { v.wrapping_neg() } else { v }
}

/// `booleanValue`: devolve o valor e, quando o texto não é booleano, a mensagem de aviso.
pub fn boolean_value(z: &[u8]) -> (i32, Option<String>) {
    let digits = if z.starts_with(b"0x") {
        2 + z[2..].iter().take_while(|&&c| hex_digit(c).is_some()).count()
    } else {
        z.iter().take_while(|c| c.is_ascii_digit()).count()
    };
    if digits > 0 && digits == z.len() {
        return ((integer_value(z) & 0xffff_ffff) as i32, None);
    }
    if z.eq_ignore_ascii_case(b"on") || z.eq_ignore_ascii_case(b"yes") {
        return (1, None);
    }
    if z.eq_ignore_ascii_case(b"off") || z.eq_ignore_ascii_case(b"no") {
        return (0, None);
    }
    (0, Some(format!("ERROR: Not a boolean value: \"{}\". Assuming \"no\".\n", String::from_utf8_lossy(z))))
}

/// `resolve_backslashes`.
pub fn resolve_backslashes(z: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(z.len());
    let mut i = 0;
    while i < z.len() {
        let mut c = z[i];
        if c == b'\\' && i + 1 < z.len() {
            i += 1;
            c = z[i];
            c = match c {
                b'a' => 0x07,
                b'b' => 0x08,
                b't' => b'\t',
                b'n' => b'\n',
                b'v' => 0x0b,
                b'f' => 0x0c,
                b'r' => b'\r',
                b'"' => b'"',
                b'\'' => b'\'',
                b'\\' => b'\\',
                b'x' => {
                    let mut hv: u8 = 0;
                    let mut n = 0;
                    while n < 2 {
                        match z.get(i + 1 + n).and_then(|&d| hex_digit(d)) {
                            Some(d) => {
                                hv = (hv << 4) | d;
                                n += 1;
                            }
                            None => break,
                        }
                    }
                    i += n;
                    hv
                }
                b'0'..=b'7' => {
                    let mut v = c - b'0';
                    if let Some(&d) = z.get(i + 1).filter(|d| (b'0'..=b'7').contains(*d)) {
                        i += 1;
                        v = (v << 3).wrapping_add(d - b'0');
                        if let Some(&d) = z.get(i + 1).filter(|d| (b'0'..=b'7').contains(*d)) {
                            i += 1;
                            v = (v << 3).wrapping_add(d - b'0');
                        }
                    }
                    v
                }
                other => other,
            };
        }
        out.push(c);
        i += 1;
    }
    // A string C acaba no primeiro NUL produzido por um escape.
    match out.iter().position(|&b| b == 0) {
        Some(p) => out[..p].to_vec(),
        None => out,
    }
}

/// `%w`-escape pra dentro de aspas duplas (sem as aspas em volta).
pub fn escape_dq(z: &[u8]) -> Vec<u8> {
    let d = dquote(z);
    d[1..d.len() - 1].to_vec()
}

/// `%q`-escape pra dentro de aspas simples (sem as aspas em volta).
pub fn escape_sq(z: &[u8]) -> Vec<u8> {
    let d = squote(z);
    d[1..d.len() - 1].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert!(needs_quote(b"select"));
        assert!(needs_quote(b"a b"));
        assert!(!needs_quote(b"abc_1"));
        assert_eq!(quoted_escaped_string(b"a\nb"), b"replace('a\\nb','\\n',char(10))");
        assert_eq!(c_string(b"a\"b\x01"), b"\"a\\\"b\\001\"");
        assert_eq!(json_string(b"a\"\n", false), b"\"a\\\"\\n\"");
        assert_eq!(csv_field(Some(b"x,y"), b"", b","), b"\"x,y\"");
        assert_eq!(csv_field(Some(b"plain"), b"", b","), b"plain");
        assert_eq!(width_print(5, b"ab"), b"ab   ");
        assert_eq!(width_print(-5, b"ab"), b"   ab");
        assert_eq!(width_print(2, b"abcd"), b"ab");
        assert!(is_number(b"-1.5e3"));
        assert!(!is_number(b"1."));
        assert_eq!(integer_value(b"2KiB"), 2048);
        assert_eq!(resolve_backslashes(b"a\\tb\\x41\\101"), b"a\tbAA");
    }
}
