//! Nomes de arquivo do `patch`: leitura do nome e da data nos cabeçalhos (com nomes entre aspas no
//! estilo C, como o git escreve), datas na época que marcam arquivo inexistente, `-p`, nomes
//! perigosos, escolha do melhor nome e citação na saída (estilos do `quotearg` do gnulib).

use super::opts::Quoting;

/// Um nome lido de um cabeçalho, com o carimbo de tempo que veio depois dele.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct HeaderName {
    pub name: Vec<u8>,
    pub stamp: Vec<u8>,
}

impl HeaderName {
    /// `/dev/null` ou data na época: o lado não existe (criação ou remoção).
    pub fn says_nonexistent(&self) -> bool {
        self.name == b"/dev/null" || stamp_is_epoch(&self.stamp)
    }
}

fn unquote_c(s: &[u8]) -> Option<(Vec<u8>, usize)> {
    // s[0] == b'"'
    let mut out = Vec::new();
    let mut i = 1;
    while i < s.len() {
        match s[i] {
            b'"' => return Some((out, i + 1)),
            b'\\' => {
                i += 1;
                let c = *s.get(i)?;
                match c {
                    b'a' => out.push(7),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'v' => out.push(11),
                    b'0'..=b'7' => {
                        let mut v: u32 = 0;
                        let mut k = 0;
                        while k < 3 && i < s.len() && (b'0'..=b'7').contains(&s[i]) {
                            v = v * 8 + (s[i] - b'0') as u32;
                            i += 1;
                            k += 1;
                        }
                        out.push(v as u8);
                        continue;
                    }
                    b'x' => {
                        let mut v: u32 = 0;
                        let mut k = 0;
                        i += 1;
                        while k < 2 && i < s.len() && s[i].is_ascii_hexdigit() {
                            v = v * 16 + (s[i] as char).to_digit(16).unwrap_or(0);
                            i += 1;
                            k += 1;
                        }
                        if k == 0 {
                            return None;
                        }
                        out.push(v as u8);
                        continue;
                    }
                    other => out.push(other),
                }
                i += 1;
            }
            b'\n' => return None,
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    None
}

/// Lê o nome no começo de `rest` (o que vem depois de `--- `, `+++ `, `*** `): entre aspas no estilo C,
/// ou até o primeiro espaço ou tab. O resto da linha, sem o espaço inicial e sem o fim de linha, é o
/// carimbo de tempo.
pub fn fetch_name(rest: &[u8]) -> Option<HeaderName> {
    let line = strip_eol(rest);
    let start = line.iter().position(|&c| c != b' ' && c != b'\t')?;
    let s = &line[start..];
    let (name, used) = if s.first() == Some(&b'"') {
        match unquote_c(s) {
            Some(x) => x,
            None => {
                let end = s.iter().position(|&c| c == b' ' || c == b'\t').unwrap_or(s.len());
                (s[..end].to_vec(), end)
            }
        }
    } else {
        let end = s.iter().position(|&c| c == b' ' || c == b'\t').unwrap_or(s.len());
        (s[..end].to_vec(), end)
    };
    if name.is_empty() {
        return None;
    }
    let tail = &s[used..];
    let ts = tail.iter().position(|&c| c != b' ' && c != b'\t').map(|p| tail[p..].to_vec()).unwrap_or_default();
    Some(HeaderName { name, stamp: ts })
}

pub fn strip_eol(l: &[u8]) -> &[u8] {
    let l = l.strip_suffix(b"\n").unwrap_or(l);
    l.strip_suffix(b"\r").unwrap_or(l)
}

/// Data e hora de um cabeçalho em segundos e nanossegundos UTC. Aceita o formato do `diff -u`
/// (`2026-01-15 12:00:00.000000000 +0000`, zona opcional) e o do `ctime` (`Thu Jan  1 00:00:00 1970`).
/// Sem zona, usa `default_offset` (segundos a leste de UTC).
pub fn parse_stamp(stamp: &[u8], default_offset: Option<i64>) -> Option<(i64, u32)> {
    let s = std::str::from_utf8(stamp).ok()?.trim();
    if s.is_empty() {
        return None;
    }
    let parts: Vec<&str> = s.split_whitespace().collect();
    // ISO: data, hora[, zona]
    if let Some(date) = parts.first()
        && date.len() >= 8
        && date.as_bytes()[4] == b'-'
    {
        let mut d = date.split('-');
        let y: i64 = d.next()?.parse().ok()?;
        let mo: i64 = d.next()?.parse().ok()?;
        let da: i64 = d.next()?.parse().ok()?;
        let (h, mi, sec, nsec) = parse_clock(parts.get(1)?)?;
        let off = match parts.get(2) {
            Some(z) => parse_zone(z)?,
            None => default_offset?,
        };
        let days = days_from_civil(y, mo, da);
        return Some((days * 86_400 + h * 3600 + mi * 60 + sec - off, nsec));
    }
    // ctime: "Thu Jan  1 00:00:00 1970"
    if parts.len() >= 5 {
        const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        let mo = MONTHS.iter().position(|m| *m == parts[1])? as i64 + 1;
        let da: i64 = parts[2].parse().ok()?;
        let (h, mi, sec, nsec) = parse_clock(parts[3])?;
        let y: i64 = parts[4].parse().ok()?;
        let off = match parts.get(5) {
            Some(z) => parse_zone(z)?,
            None => default_offset?,
        };
        let days = days_from_civil(y, mo, da);
        return Some((days * 86_400 + h * 3600 + mi * 60 + sec - off, nsec));
    }
    None
}

fn parse_clock(t: &str) -> Option<(i64, i64, i64, u32)> {
    let (hms, frac) = match t.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (t, None),
    };
    let mut it = hms.split(':');
    let h: i64 = it.next()?.parse().ok()?;
    let mi: i64 = it.next()?.parse().ok()?;
    let sec: i64 = it.next().unwrap_or("0").parse().ok()?;
    let nsec = match frac {
        Some(f) if !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()) => {
            let mut digits: String = f.chars().take(9).collect();
            while digits.len() < 9 {
                digits.push('0');
            }
            digits.parse().ok()?
        }
        Some(_) => return None,
        None => 0,
    };
    Some((h, mi, sec, nsec))
}

fn parse_zone(z: &str) -> Option<i64> {
    let b = z.as_bytes();
    if b.len() == 5 && (b[0] == b'+' || b[0] == b'-') && b[1..].iter().all(|c| c.is_ascii_digit()) {
        let h: i64 = z[1..3].parse().ok()?;
        let m: i64 = z[3..5].parse().ok()?;
        let v = h * 3600 + m * 60;
        return Some(if b[0] == b'-' { -v } else { v });
    }
    match z {
        "UTC" | "GMT" | "Z" => Some(0),
        _ => None,
    }
}

/// Dias desde 1970-01-01 (algoritmo de Howard Hinnant).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// O carimbo diz "época" (o diff marca assim o lado inexistente).
pub fn stamp_is_epoch(stamp: &[u8]) -> bool {
    matches!(parse_stamp(stamp, Some(0)), Some((0, 0)))
}

/// Tira `n` componentes do começo (`-p`). `None` sem `-p`: fica só o último componente. Barras
/// repetidas contam como uma. Devolve `None` quando sobram menos componentes do que o pedido.
pub fn strip(name: &[u8], n: Option<usize>) -> Option<Vec<u8>> {
    match n {
        None => {
            let base = name.rsplit(|&c| c == b'/').next().unwrap_or(name);
            if base.is_empty() { None } else { Some(base.to_vec()) }
        }
        Some(0) => Some(name.to_vec()),
        Some(n) => {
            let mut i = 0usize;
            let mut stripped = 0usize;
            while stripped < n {
                {
                    let p = name[i..].iter().position(|&c| c == b'/')?;
                    i += p;
                    while i < name.len() && name[i] == b'/' {
                        i += 1;
                    }
                    stripped += 1;
                }
            }
            if i >= name.len() { None } else { Some(name[i..].to_vec()) }
        }
    }
}

/// Nome absoluto: o GNU recusa e avisa. Nome com `..`: recusa calado.
pub enum Danger {
    Safe,
    Absolute,
    DotDot,
}

pub fn danger(name: &[u8]) -> Danger {
    if name.starts_with(b"/") {
        return Danger::Absolute;
    }
    if name.split(|&c| c == b'/').any(|c| c == b"..") {
        return Danger::DotDot;
    }
    Danger::Safe
}

/// Componentes e tamanhos pra escolher o "melhor" nome: menos componentes, depois basename mais curto,
/// depois nome mais curto.
pub fn best_index(names: &[&[u8]]) -> Option<usize> {
    let key = |n: &[u8]| {
        let comps = n.split(|&c| c == b'/').filter(|c| !c.is_empty()).count();
        let base = n.rsplit(|&c| c == b'/').next().unwrap_or(n).len();
        (comps, base, n.len())
    };
    let mut best: Option<usize> = None;
    for (i, n) in names.iter().enumerate() {
        match best {
            None => best = Some(i),
            Some(b) if key(n) < key(names[b]) => best = Some(i),
            _ => {}
        }
    }
    best
}

fn shell_safe(c: u8, first: bool) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, b'%' | b'+' | b',' | b'-' | b'.' | b'/' | b':' | b'@' | b'_')
        || (!first && matches!(c, b'#' | b'~'))
        || c >= 0x80
}

/// Nome citado como o `quotearg` do gnulib no estilo pedido (o padrão do patch é `shell`).
pub fn quote(name: &[u8], style: Quoting) -> Vec<u8> {
    match style {
        Quoting::Literal => name.to_vec(),
        Quoting::Shell | Quoting::ShellAlways => {
            let needs = style == Quoting::ShellAlways
                || name.is_empty()
                || name.iter().enumerate().any(|(i, &c)| !shell_safe(c, i == 0));
            if !needs {
                return name.to_vec();
            }
            if !name.contains(&b'\'') {
                let mut v = b"'".to_vec();
                v.extend_from_slice(name);
                v.push(b'\'');
                return v;
            }
            if !name.iter().any(|c| matches!(c, b'$' | b'`' | b'"' | b'\\' | b'!')) {
                let mut v = b"\"".to_vec();
                v.extend_from_slice(name);
                v.push(b'"');
                return v;
            }
            let mut v = b"'".to_vec();
            for &c in name {
                if c == b'\'' {
                    v.extend_from_slice(b"'\\''");
                } else {
                    v.push(c);
                }
            }
            v.push(b'\'');
            v
        }
        Quoting::C | Quoting::Escape => {
            let mut v = Vec::new();
            if style == Quoting::C {
                v.push(b'"');
            }
            for &c in name {
                match c {
                    b'\\' => v.extend_from_slice(b"\\\\"),
                    b'"' if style == Quoting::C => v.extend_from_slice(b"\\\""),
                    b'\n' => v.extend_from_slice(b"\\n"),
                    b'\t' => v.extend_from_slice(b"\\t"),
                    b'\r' => v.extend_from_slice(b"\\r"),
                    7 => v.extend_from_slice(b"\\a"),
                    8 => v.extend_from_slice(b"\\b"),
                    12 => v.extend_from_slice(b"\\f"),
                    11 => v.extend_from_slice(b"\\v"),
                    b' ' if style == Quoting::Escape => v.extend_from_slice(b"\\ "),
                    c if c < 0x20 || c == 0x7f => v.extend_from_slice(format!("\\{c:03o}").as_bytes()),
                    c => v.push(c),
                }
            }
            if style == Quoting::C {
                v.push(b'"');
            }
            v
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetches_names_and_stamps() {
        let h = fetch_name(b"a/x.txt\t2026-01-15 12:00:00.000000000 +0000\n").unwrap();
        assert_eq!(h.name, b"a/x.txt");
        assert_eq!(h.stamp, b"2026-01-15 12:00:00.000000000 +0000");
        let h = fetch_name(b"\"a b.txt\"\n").unwrap();
        assert_eq!(h.name, b"a b.txt");
        let h = fetch_name(b"\"\\x66\\146\"\n").unwrap();
        assert_eq!(h.name, b"ff");
        let h = fetch_name(b"a b.txt\n").unwrap();
        assert_eq!(h.name, b"a");
    }

    #[test]
    fn epoch_detection() {
        assert!(stamp_is_epoch(b"1970-01-01 00:00:00.000000000 +0000"));
        assert!(stamp_is_epoch(b"1969-12-31 21:00:00.000000000 -0300"));
        assert!(stamp_is_epoch(b"Thu Jan  1 00:00:00 1970"));
        assert!(!stamp_is_epoch(b"2026-01-15 12:00:00.000000000 +0000"));
        assert!(!stamp_is_epoch(b""));
    }

    #[test]
    fn strip_rules() {
        assert_eq!(strip(b"a/b/c", None).unwrap(), b"c");
        assert_eq!(strip(b"a/b/c", Some(1)).unwrap(), b"b/c");
        assert_eq!(strip(b"a//f", Some(1)).unwrap(), b"f");
        assert_eq!(strip(b"/usr/f", Some(1)).unwrap(), b"usr/f");
        assert_eq!(strip(b"a/b/c", Some(9)), None);
    }

    #[test]
    fn shell_quoting() {
        assert_eq!(quote(b"a.txt", Quoting::Shell), b"a.txt");
        assert_eq!(quote(b"a b.txt", Quoting::Shell), b"'a b.txt'");
        assert_eq!(quote(b"it's.txt", Quoting::Shell), b"\"it's.txt\"");
        assert_eq!(quote(b"it's$x", Quoting::Shell), b"'it'\\''s$x'");
    }

    #[test]
    fn best_name() {
        let n: Vec<&[u8]> = vec![b"a/b/g.txt", b"x/f.txt"];
        assert_eq!(best_index(&n), Some(1));
        let n: Vec<&[u8]> = vec![b"ff.txt", b"f.txt"];
        assert_eq!(best_index(&n), Some(1));
    }
}
