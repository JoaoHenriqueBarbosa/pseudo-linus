//! Formato dos objetos: tree, commit, tag e identidades (`Nome <email> 1234567890 +0000`).

use crate::hash::{Kind, Oid};

// ---- tree -------------------------------------------------------------------------------------

pub const MODE_TREE: u32 = 0o040000;
pub const MODE_BLOB: u32 = 0o100644;
pub const MODE_EXEC: u32 = 0o100755;
pub const MODE_LINK: u32 = 0o120000;
pub const MODE_GITLINK: u32 = 0o160000;

pub fn is_tree_mode(m: u32) -> bool {
    m & 0o170000 == 0o040000
}

pub fn is_gitlink(m: u32) -> bool {
    m & 0o170000 == 0o160000
}

pub fn is_link(m: u32) -> bool {
    m & 0o170000 == 0o120000
}

pub fn is_reg(m: u32) -> bool {
    m & 0o170000 == 0o100000
}

/// Modo canônico de uma entrada (o que o git grava e mostra).
pub fn canon_mode(m: u32) -> u32 {
    match m & 0o170000 {
        0o100000 => {
            if m & 0o100 != 0 {
                MODE_EXEC
            } else {
                MODE_BLOB
            }
        }
        0o040000 => MODE_TREE,
        0o120000 => MODE_LINK,
        0o160000 => MODE_GITLINK,
        _ => m,
    }
}

/// Tipo de objeto apontado por uma entrada de tree com esse modo.
pub fn kind_of_mode(m: u32) -> Kind {
    if is_tree_mode(m) {
        Kind::Tree
    } else if is_gitlink(m) {
        Kind::Commit
    } else {
        Kind::Blob
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeEntry {
    pub mode: u32,
    pub name: Vec<u8>,
    pub oid: Oid,
}

impl TreeEntry {
    pub fn is_tree(&self) -> bool {
        is_tree_mode(self.mode)
    }
}

/// Lê uma tree. Entradas malformadas encerram a leitura com erro.
pub fn parse_tree(data: &[u8]) -> Result<Vec<TreeEntry>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let sp = data[i..].iter().position(|b| *b == b' ').ok_or("malformed mode in tree entry")? + i;
        let mut mode = 0u32;
        for &c in &data[i..sp] {
            if !(b'0'..=b'7').contains(&c) {
                return Err("malformed mode in tree entry".into());
            }
            mode = mode * 8 + (c - b'0') as u32;
        }
        let nul = data[sp + 1..].iter().position(|b| *b == 0).ok_or("malformed tree entry")? + sp + 1;
        if nul + 21 > data.len() {
            return Err("too-short tree object".into());
        }
        let name = data[sp + 1..nul].to_vec();
        if name.is_empty() {
            return Err("empty filename in tree entry".into());
        }
        let oid = Oid::from_bytes(&data[nul + 1..nul + 21]).expect("20 bytes");
        out.push(TreeEntry { mode, name, oid });
        i = nul + 21;
    }
    Ok(out)
}

/// Ordem das entradas de tree: diretório compara como se tivesse `/` no fim.
pub fn tree_entry_cmp(a_name: &[u8], a_tree: bool, b_name: &[u8], b_tree: bool) -> std::cmp::Ordering {
    let n = a_name.len().min(b_name.len());
    match a_name[..n].cmp(&b_name[..n]) {
        std::cmp::Ordering::Equal => {}
        o => return o,
    }
    let ca = a_name.get(n).copied().unwrap_or(if a_tree { b'/' } else { 0 });
    let cb = b_name.get(n).copied().unwrap_or(if b_tree { b'/' } else { 0 });
    ca.cmp(&cb)
}

pub fn sort_tree(entries: &mut [TreeEntry]) {
    entries.sort_by(|a, b| tree_entry_cmp(&a.name, a.is_tree(), &b.name, b.is_tree()));
}

/// Codifica uma tree (as entradas já devem estar na ordem do git).
pub fn encode_tree(entries: &[TreeEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    for e in entries {
        out.extend_from_slice(format!("{:o}", e.mode).as_bytes());
        out.push(b' ');
        out.extend_from_slice(&e.name);
        out.push(0);
        out.extend_from_slice(&e.oid.0);
    }
    out
}

// ---- identidade -------------------------------------------------------------------------------

/// `Nome <email> <segundos> <fuso>`, guardando os pedaços crus como o `split_ident_line`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Ident {
    pub name: Vec<u8>,
    pub email: Vec<u8>,
    /// Segundos desde a época; `None` se a linha não tem data válida.
    pub date: Option<i64>,
    /// Fuso como inteiro decimal `+hhmm` (ex.: `-0300` é -300).
    pub tz: i32,
    /// O fuso como escrito (`+0000`, `-0000`...).
    pub tz_raw: Vec<u8>,
}

impl Ident {
    /// Deslocamento do fuso em segundos.
    pub fn offset_secs(&self) -> i64 {
        tz_offset_secs(self.tz)
    }

    /// Linha completa como vai no objeto.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.name.clone();
        out.extend_from_slice(b" <");
        out.extend_from_slice(&self.email);
        out.push(b'>');
        if let Some(d) = self.date {
            out.extend_from_slice(format!(" {d} ").as_bytes());
            if self.tz_raw.is_empty() {
                out.extend_from_slice(format_tz(self.tz).as_bytes());
            } else {
                out.extend_from_slice(&self.tz_raw);
            }
        }
        out
    }

    /// `Nome <email>`.
    pub fn name_email(&self) -> Vec<u8> {
        let mut out = self.name.clone();
        out.extend_from_slice(b" <");
        out.extend_from_slice(&self.email);
        out.push(b'>');
        out
    }
}

pub fn tz_offset_secs(tz: i32) -> i64 {
    let sign = if tz < 0 { -1 } else { 1 };
    let a = tz.abs() as i64;
    sign * ((a / 100) * 3600 + (a % 100) * 60)
}

/// `+hhmm` a partir do inteiro decimal.
pub fn format_tz(tz: i32) -> String {
    let sign = if tz < 0 { '-' } else { '+' };
    format!("{sign}{:04}", tz.abs())
}

/// Port do `split_ident_line` do git.
pub fn parse_ident(line: &[u8]) -> Option<Ident> {
    let lt = line.iter().position(|b| *b == b'<')?;
    let mail_begin = lt + 1;
    let mut name_end = 0;
    let mut k = lt;
    while k > 0 {
        k -= 1;
        if !line[k].is_ascii_whitespace() {
            name_end = k + 1;
            break;
        }
    }
    let gt = line[mail_begin..].iter().position(|b| *b == b'>')? + mail_begin;
    let mut id = Ident { name: line[..name_end].to_vec(), email: line[mail_begin..gt].to_vec(), ..Ident::default() };
    // O último '>' da linha.
    let last_gt = line.iter().rposition(|b| *b == b'>')?;
    let mut cp = last_gt + 1;
    while cp < line.len() && line[cp].is_ascii_whitespace() {
        cp += 1;
    }
    let date_begin = cp;
    while cp < line.len() && line[cp].is_ascii_digit() {
        cp += 1;
    }
    if cp == date_begin {
        return Some(id);
    }
    let date_end = cp;
    while cp < line.len() && line[cp].is_ascii_whitespace() {
        cp += 1;
    }
    if cp >= line.len() || (line[cp] != b'+' && line[cp] != b'-') {
        return Some(id);
    }
    let tz_begin = cp;
    cp += 1;
    let digits_begin = cp;
    while cp < line.len() && line[cp].is_ascii_digit() {
        cp += 1;
    }
    if cp == digits_begin {
        return Some(id);
    }
    let date: i64 = std::str::from_utf8(&line[date_begin..date_end]).ok()?.parse().ok()?;
    let tz: i32 = std::str::from_utf8(&line[digits_begin..cp]).ok()?.parse().unwrap_or(0);
    id.date = Some(date);
    id.tz = if line[tz_begin] == b'-' { -tz } else { tz };
    id.tz_raw = line[tz_begin..cp].to_vec();
    Some(id)
}

// ---- commit -----------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Commit {
    pub tree: Oid,
    pub parents: Vec<Oid>,
    /// Linha crua depois de `author `.
    pub author: Vec<u8>,
    pub committer: Vec<u8>,
    pub encoding: Option<Vec<u8>>,
    /// Outros cabeçalhos (`gpgsig`, `mergetag`...) com as continuações, como vieram.
    pub extra: Vec<(Vec<u8>, Vec<u8>)>,
    pub message: Vec<u8>,
}

impl Commit {
    pub fn author_ident(&self) -> Ident {
        parse_ident(&self.author).unwrap_or_default()
    }

    pub fn committer_ident(&self) -> Ident {
        parse_ident(&self.committer).unwrap_or_default()
    }

    /// Data do committer (a que ordena o log).
    pub fn commit_date(&self) -> i64 {
        parse_ident(&self.committer).and_then(|i| i.date).unwrap_or(0)
    }

    /// Primeiro parágrafo da mensagem juntado numa linha (o `%s` do git).
    pub fn subject(&self) -> Vec<u8> {
        subject_of(&self.message)
    }
}

/// O `%s`: linhas do primeiro parágrafo (sem espaço no fim) juntadas por `sep`, como o
/// `format_subject` do git. Devolve também onde o parágrafo acabou.
pub fn subject_with(msg: &[u8], sep: &[u8]) -> (Vec<u8>, usize) {
    let mut out: Vec<u8> = Vec::new();
    let mut pos = skip_blank_lines(msg, 0);
    let mut first = true;
    while pos < msg.len() {
        let end = msg[pos..].iter().position(|b| *b == b'\n').map(|p| pos + p + 1).unwrap_or(msg.len());
        let line = rtrim(&msg[pos..end]);
        pos = end;
        if line.is_empty() {
            break;
        }
        if !first {
            out.extend_from_slice(sep);
        }
        out.extend_from_slice(line);
        first = false;
    }
    (out, pos)
}

pub fn subject_of(msg: &[u8]) -> Vec<u8> {
    subject_with(msg, b" ").0
}

/// Pula linhas só com espaço a partir de `pos`.
pub fn skip_blank_lines(msg: &[u8], mut pos: usize) -> usize {
    while pos < msg.len() {
        let end = msg[pos..].iter().position(|b| *b == b'\n').map(|p| pos + p + 1).unwrap_or(msg.len());
        if !rtrim(&msg[pos..end]).is_empty() {
            break;
        }
        pos = end;
    }
    pos
}

/// O `%b`: o resto depois do primeiro parágrafo, sem as linhas vazias do começo.
pub fn body_of(msg: &[u8]) -> Vec<u8> {
    let (_, end) = subject_with(msg, b" ");
    msg[skip_blank_lines(msg, end)..].to_vec()
}

pub fn rtrim(s: &[u8]) -> &[u8] {
    let end = s.iter().rposition(|b| !b.is_ascii_whitespace()).map(|e| e + 1).unwrap_or(0);
    &s[..end]
}

pub fn trim_ascii(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(s.len());
    let end = s.iter().rposition(|b| !b.is_ascii_whitespace()).map(|e| e + 1).unwrap_or(start);
    &s[start..end.max(start)]
}

pub fn parse_commit(data: &[u8]) -> Result<Commit, String> {
    let mut c = Commit::default();
    let (headers, message) = split_headers(data);
    c.message = message.to_vec();
    let mut have_tree = false;
    for (key, value) in headers {
        match key.as_slice() {
            b"tree" if !have_tree => {
                c.tree = Oid::from_hex(&value).ok_or("bad tree pointer")?;
                have_tree = true;
            }
            b"parent" => c.parents.push(Oid::from_hex(&value).ok_or("bad parent pointer")?),
            b"author" if c.author.is_empty() => c.author = value,
            b"committer" if c.committer.is_empty() => c.committer = value,
            b"encoding" => c.encoding = Some(value),
            _ => c.extra.push((key, value)),
        }
    }
    if !have_tree {
        return Err("bogus commit object".into());
    }
    Ok(c)
}

/// Pares (nome, valor) de cabeçalho.
pub type Headers = Vec<(Vec<u8>, Vec<u8>)>;

/// Separa os cabeçalhos (com continuação por linhas que começam com espaço) da mensagem.
pub fn split_headers(data: &[u8]) -> (Headers, &[u8]) {
    let mut headers: Headers = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let end = data[i..].iter().position(|b| *b == b'\n').map(|p| p + i).unwrap_or(data.len());
        let line = &data[i..end];
        let next = (end + 1).min(data.len());
        if line.is_empty() {
            return (headers, &data[next..]);
        }
        if line[0] == b' ' {
            if let Some(last) = headers.last_mut() {
                last.1.push(b'\n');
                last.1.extend_from_slice(&line[1..]);
            }
        } else {
            let sp = line.iter().position(|b| *b == b' ').unwrap_or(line.len());
            let value = if sp < line.len() { line[sp + 1..].to_vec() } else { Vec::new() };
            headers.push((line[..sp].to_vec(), value));
        }
        if end >= data.len() {
            return (headers, &data[data.len()..]);
        }
        i = end + 1;
    }
    (headers, &data[data.len()..])
}

pub fn encode_commit(c: &Commit) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("tree {}\n", c.tree).as_bytes());
    for p in &c.parents {
        out.extend_from_slice(format!("parent {p}\n").as_bytes());
    }
    out.extend_from_slice(b"author ");
    out.extend_from_slice(&c.author);
    out.extend_from_slice(b"\ncommitter ");
    out.extend_from_slice(&c.committer);
    out.push(b'\n');
    if let Some(e) = &c.encoding {
        out.extend_from_slice(b"encoding ");
        out.extend_from_slice(e);
        out.push(b'\n');
    }
    for (k, v) in &c.extra {
        out.extend_from_slice(k);
        out.push(b' ');
        for (i, line) in v.split(|b| *b == b'\n').enumerate() {
            if i > 0 {
                out.extend_from_slice(b"\n ");
            }
            out.extend_from_slice(line);
        }
        out.push(b'\n');
    }
    out.push(b'\n');
    out.extend_from_slice(&c.message);
    out
}

// ---- tag --------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub object: Oid,
    pub kind: Kind,
    pub name: Vec<u8>,
    pub tagger: Option<Vec<u8>>,
    pub message: Vec<u8>,
}

pub fn parse_tag(data: &[u8]) -> Result<Tag, String> {
    let (headers, message) = split_headers(data);
    let mut object = None;
    let mut kind = None;
    let mut name = None;
    let mut tagger = None;
    for (k, v) in headers {
        match k.as_slice() {
            b"object" => object = Oid::from_hex(&v),
            b"type" => kind = Kind::from_name(&v),
            b"tag" => name = Some(v),
            b"tagger" => tagger = Some(v),
            _ => {}
        }
    }
    Ok(Tag {
        object: object.ok_or("bad tag object")?,
        kind: kind.ok_or("bad tag type")?,
        name: name.unwrap_or_default(),
        tagger,
        message: message.to_vec(),
    })
}

pub fn encode_tag(t: &Tag) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("object {}\ntype {}\ntag ", t.object, t.kind.name()).as_bytes());
    out.extend_from_slice(&t.name);
    out.push(b'\n');
    if let Some(tg) = &t.tagger {
        out.extend_from_slice(b"tagger ");
        out.extend_from_slice(tg);
        out.push(b'\n');
    }
    out.push(b'\n');
    out.extend_from_slice(&t.message);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ident_split() {
        let id = parse_ident(b"Agent Smith <agent@example.com> 1768478400 -0300").unwrap();
        assert_eq!(id.name, b"Agent Smith");
        assert_eq!(id.email, b"agent@example.com");
        assert_eq!(id.date, Some(1768478400));
        assert_eq!(id.tz, -300);
        assert_eq!(id.offset_secs(), -3 * 3600);
        assert_eq!(id.to_bytes(), b"Agent Smith <agent@example.com> 1768478400 -0300");
        let id = parse_ident(b"<x@y> ").unwrap();
        assert_eq!(id.name, b"");
        assert_eq!(id.date, None);
    }

    #[test]
    fn tree_order() {
        use std::cmp::Ordering::*;
        assert_eq!(tree_entry_cmp(b"a", true, b"a.txt", false), Greater);
        assert_eq!(tree_entry_cmp(b"a", false, b"a.txt", false), Less);
        assert_eq!(tree_entry_cmp(b"a-b", false, b"a", true), Less);
    }

    #[test]
    fn subject_and_body() {
        let m = b"\nfirst line  \nsecond\n\nbody 1\nbody 2\n";
        assert_eq!(subject_of(m), b"first line second");
        assert_eq!(body_of(m), b"body 1\nbody 2\n");
    }
}
