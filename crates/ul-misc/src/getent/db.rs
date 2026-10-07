//! Leitura dos arquivos de `/etc` como os módulos `files` do NSS da glibc 2.41 (`files-parse.c`,
//! `nss_readline.c` e os `fget*ent_r.c`): cada linha vira um registro, linhas inválidas são puladas em
//! silêncio, e os registros se imprimem no formato de `putpwent`, `putgrent`, `putspent` e
//! `putsgent`. Aqui ficam os bancos de contas (passwd, group, shadow, gshadow); os de rede e os
//! demais estão em `netdb.rs`.

use sysabi::{Errno, sys};

use super::inet::is_space;

/// Um `char *line` do C: bytes com um NUL implícito no fim.
pub struct Cursor<'a> {
    pub s: &'a [u8],
    pub p: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(s: &'a [u8]) -> Cursor<'a> {
        Cursor { s, p: 0 }
    }

    /// O byte atual (0 no fim).
    pub fn peek(&self) -> u8 {
        self.s.get(self.p).copied().unwrap_or(0)
    }

    pub fn at_end(&self) -> bool {
        self.p >= self.s.len()
    }

    /// `STRING_FIELD`: lê até um terminador, engole o terminador (e, com `swallow`, os seguintes).
    pub fn string_field(&mut self, term: fn(u8) -> bool, swallow: bool) -> &'a [u8] {
        let s: &'a [u8] = self.s;
        let start = self.p;
        while !self.at_end() && !term(self.peek()) {
            self.p += 1;
        }
        let field = &s[start..self.p];
        if !self.at_end() {
            self.p += 1;
            while swallow && !self.at_end() && term(self.peek()) {
                self.p += 1;
            }
        }
        field
    }

    /// `INT_FIELD`: número obrigatório até um terminador. `None` é "linha inválida".
    pub fn int_field(&mut self, term: fn(u8) -> bool, swallow: bool, base: u32) -> Option<u64> {
        let (tmp, end) = strtoull(self.s, self.p, base);
        if tmp > u64::from(u32::MAX) {
            return None;
        }
        if end == self.p {
            return None;
        }
        let mut e = end;
        let at = |i: usize| self.s.get(i).copied().unwrap_or(0);
        if e < self.s.len() && term(at(e)) {
            e += 1;
            while swallow && e < self.s.len() && term(at(e)) {
                e += 1;
            }
        } else if e < self.s.len() {
            return None;
        }
        self.p = e;
        Some(tmp)
    }

    /// `INT_FIELD_MAYBE_NULL`: como `int_field`, mas campo vazio vale `default`; `None` se a linha
    /// acabou antes (espera-se mais entrada) ou o campo é inválido.
    pub fn int_field_maybe_null(
        &mut self,
        term: fn(u8) -> bool,
        swallow: bool,
        base: u32,
        default: u64,
    ) -> Option<u64> {
        if self.at_end() {
            return None;
        }
        let (tmp, end) = strtoull(self.s, self.p, base);
        if tmp > u64::from(u32::MAX) {
            return None;
        }
        let value = if end == self.p { default } else { tmp };
        let mut e = end;
        let at = |i: usize| self.s.get(i).copied().unwrap_or(0);
        if e < self.s.len() && term(at(e)) {
            e += 1;
            while swallow && e < self.s.len() && term(at(e)) {
                e += 1;
            }
        } else if e < self.s.len() {
            return None;
        }
        self.p = e;
        Some(value)
    }

    /// `parse_list`: elementos separados por `sep` ou, com `sep_space`, por espaço; a lista acaba no
    /// fim da linha ou no `terminator` (consumido). Espaços antes de cada elemento são pulados e
    /// elementos vazios somem.
    pub fn parse_list(&mut self, terminator: u8, sep: fn(u8) -> bool) -> Vec<Vec<u8>> {
        let mut list = Vec::new();
        loop {
            if self.at_end() {
                break;
            }
            if terminator != 0 && self.peek() == terminator {
                self.p += 1;
                break;
            }
            while !self.at_end() && is_space(self.peek()) {
                self.p += 1;
            }
            let elt = self.p;
            loop {
                let c = self.peek();
                if self.at_end() || (terminator != 0 && c == terminator) || sep(c) {
                    if self.p > elt {
                        list.push(self.s[elt..self.p].to_vec());
                    }
                    if !self.at_end() {
                        let endc = c;
                        self.p += 1;
                        if terminator != 0 && endc == terminator {
                            return list;
                        }
                    }
                    break;
                }
                self.p += 1;
            }
        }
        list
    }
}

pub fn is_colon(b: u8) -> bool {
    b == b':'
}

pub fn is_comma(b: u8) -> bool {
    b == b','
}

pub fn never(_b: u8) -> bool {
    false
}

/// `strtoull`: devolve o valor e a posição final; sem dígitos a posição é a inicial. Estouro dá
/// `u64::MAX`. Base 0 reconhece `0x` e `0`; base 16 aceita o prefixo `0x`.
pub fn strtoull(s: &[u8], start: usize, base: u32) -> (u64, usize) {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut i = start;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let mut negative = false;
    if at(i) == b'+' || at(i) == b'-' {
        negative = at(i) == b'-';
        i += 1;
    }
    let mut base = base;
    if (base == 0 || base == 16) && at(i) == b'0' && (at(i + 1) == b'x' || at(i + 1) == b'X') {
        // O prefixo só vale se um dígito hexa vem depois; senão o número é só o "0".
        if at(i + 2).is_ascii_hexdigit() {
            i += 2;
            base = 16;
        } else {
            return (0, i + 1);
        }
    } else if base == 0 {
        base = if at(i) == b'0' { 8 } else { 10 };
    }
    let digits_start = i;
    let mut val: u64 = 0;
    let mut overflow = false;
    while i < s.len() {
        let d = match s[i] {
            c @ b'0'..=b'9' => u32::from(c - b'0'),
            c @ b'a'..=b'z' => u32::from(c - b'a') + 10,
            c @ b'A'..=b'Z' => u32::from(c - b'A') + 10,
            _ => break,
        };
        if d >= base {
            break;
        }
        match val
            .checked_mul(u64::from(base))
            .and_then(|v| v.checked_add(u64::from(d)))
        {
            Some(v) => val = v,
            None => overflow = true,
        }
        i += 1;
    }
    if i == digits_start {
        return (0, start);
    }
    if overflow {
        return (u64::MAX, i);
    }
    (if negative { val.wrapping_neg() } else { val }, i)
}

/// Linhas úteis de um arquivo de banco, como o `__nss_readline`: pula brancos iniciais, linhas vazias
/// e comentários; corta no primeiro NUL (a linha é uma string C) e no primeiro caractere de
/// `eol_set` (o `strpbrk (line, EOLSET "\n")` do parser).
pub fn read_db_lines(path: &[u8], eol_set: &[u8]) -> Result<Vec<Vec<u8>>, Errno> {
    let data = sys::read_file(path)?;
    let mut out = Vec::new();
    for raw in data.split(|b| *b == b'\n') {
        let line = match raw.iter().position(|b| *b == 0) {
            Some(p) => &raw[..p],
            None => raw,
        };
        let Some(line) = useful_line(line) else {
            continue;
        };
        let cut = line
            .iter()
            .position(|b| eol_set.contains(b))
            .unwrap_or(line.len());
        out.push(line[..cut].to_vec());
    }
    Ok(out)
}

/// A linha sem os brancos iniciais, ou `None` se o que sobra é vazio ou comentário.
pub fn useful_line(line: &[u8]) -> Option<&[u8]> {
    let start = line
        .iter()
        .position(|b| !is_space(*b))
        .unwrap_or(line.len());
    let line = &line[start..];
    (!line.is_empty() && line[0] != b'#').then_some(line)
}

/// Lê e interpreta um arquivo de banco: cada linha útil passa por `parse`, e a que ele recusa é
/// pulada em silêncio. Arquivo ilegível = fonte indisponível.
pub fn read_records<T>(
    path: &[u8],
    eol_set: &[u8],
    parse: impl Fn(&[u8]) -> Option<T>,
) -> Result<Vec<T>, Errno> {
    Ok(read_db_lines(path, eol_set)?
        .iter()
        .filter_map(|l| parse(l))
        .collect())
}

/// `struct passwd`; os campos `None` são os ponteiros NULL das entradas especiais `+`/`-`.
#[derive(Clone, Debug)]
pub struct Passwd {
    pub name: Vec<u8>,
    pub passwd: Option<Vec<u8>>,
    pub uid: u32,
    pub gid: u32,
    pub gecos: Option<Vec<u8>>,
    pub dir: Option<Vec<u8>>,
    pub shell: Option<Vec<u8>>,
}

fn is_nis_name(name: &[u8]) -> bool {
    matches!(name.first(), Some(b'+') | Some(b'-'))
}

/// `parse_line` de `fgetpwent_r.c`.
pub fn parse_passwd(line: &[u8]) -> Option<Passwd> {
    let mut c = Cursor::new(line);
    let name = c.string_field(is_colon, false).to_vec();
    if c.at_end() && is_nis_name(&name) {
        return Some(Passwd {
            name,
            passwd: None,
            uid: 0,
            gid: 0,
            gecos: None,
            dir: None,
            shell: None,
        });
    }
    let passwd = c.string_field(is_colon, false).to_vec();
    let (uid, gid) = if is_nis_name(&name) {
        let u = c.int_field_maybe_null(is_colon, false, 10, 0)?;
        let g = c.int_field_maybe_null(is_colon, false, 10, 0)?;
        (u as u32, g as u32)
    } else {
        let u = c.int_field(is_colon, false, 10)?;
        let g = c.int_field(is_colon, false, 10)?;
        (u as u32, g as u32)
    };
    let gecos = c.string_field(is_colon, false).to_vec();
    let dir = c.string_field(is_colon, false).to_vec();
    let shell = c.s[c.p.min(c.s.len())..].to_vec();
    Some(Passwd {
        name,
        passwd: Some(passwd),
        uid,
        gid,
        gecos: Some(gecos),
        dir: Some(dir),
        shell: Some(shell),
    })
}

/// Campo válido para o arquivo de banco: sem `:` nem quebra de linha (o `__nss_valid_field`).
fn valid_field(f: &[u8]) -> bool {
    !f.iter().any(|b| *b == b':' || *b == b'\n')
}

fn valid_opt(f: &Option<Vec<u8>>) -> bool {
    f.as_deref().is_none_or(valid_field)
}

/// Lista válida: cada membro sem `:`, quebra de linha nem `,` (o `__nss_valid_list_field`).
fn valid_list(list: &[Vec<u8>]) -> bool {
    list.iter()
        .all(|m| !m.iter().any(|b| *b == b':' || *b == b'\n' || *b == b','))
}

/// `putpwent`; `Err` é o EINVAL de um campo com caractere proibido (o getent então não imprime a
/// entrada e avisa no stderr).
pub fn format_passwd(p: &Passwd) -> Result<Vec<u8>, ()> {
    if !valid_field(&p.name) || !valid_opt(&p.passwd) || !valid_opt(&p.dir) || !valid_opt(&p.shell)
    {
        return Err(());
    }
    let mut out = Vec::new();
    let s = |o: &Option<Vec<u8>>| o.clone().unwrap_or_default();
    out.extend_from_slice(&p.name);
    out.push(b':');
    out.extend_from_slice(&s(&p.passwd));
    if is_nis_name(&p.name) {
        out.extend_from_slice(b":::");
    } else {
        out.extend_from_slice(format!(":{}:{}:", p.uid, p.gid).as_bytes());
    }
    // O GECOS tem os caracteres proibidos trocados por espaço (nunca ocorrem vindos do arquivo).
    let gecos: Vec<u8> = s(&p.gecos)
        .iter()
        .map(|b| if *b == b':' || *b == b'\n' { b' ' } else { *b })
        .collect();
    out.extend_from_slice(&gecos);
    out.push(b':');
    out.extend_from_slice(&s(&p.dir));
    out.push(b':');
    out.extend_from_slice(&s(&p.shell));
    out.push(b'\n');
    Ok(out)
}

/// `struct group`.
#[derive(Clone, Debug)]
pub struct Group {
    pub name: Vec<u8>,
    pub passwd: Option<Vec<u8>>,
    pub gid: u32,
    pub members: Vec<Vec<u8>>,
}

/// `parse_line` de `fgetgrent_r.c`.
pub fn parse_group(line: &[u8]) -> Option<Group> {
    let mut c = Cursor::new(line);
    let name = c.string_field(is_colon, false).to_vec();
    let (passwd, gid) = if c.at_end() && is_nis_name(&name) {
        (None, 0u32)
    } else {
        let passwd = c.string_field(is_colon, false).to_vec();
        let gid = if is_nis_name(&name) {
            c.int_field_maybe_null(is_colon, false, 10, 0)?
        } else {
            c.int_field(is_colon, false, 10)?
        };
        (Some(passwd), gid as u32)
    };
    let members = c.parse_list(0, is_comma);
    Some(Group {
        name,
        passwd,
        gid,
        members,
    })
}

/// `putgrent`; `Err` é o EINVAL de campo ou membro com caractere proibido.
pub fn format_group(g: &Group) -> Result<Vec<u8>, ()> {
    if !valid_field(&g.name) || !valid_opt(&g.passwd) || !valid_list(&g.members) {
        return Err(());
    }
    let mut out = Vec::new();
    out.extend_from_slice(&g.name);
    out.push(b':');
    if let Some(p) = &g.passwd {
        out.extend_from_slice(p);
    }
    if is_nis_name(&g.name) {
        out.extend_from_slice(b"::");
    } else {
        out.extend_from_slice(format!(":{}:", g.gid).as_bytes());
    }
    for (i, m) in g.members.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.extend_from_slice(m);
    }
    out.push(b'\n');
    Ok(out)
}

/// `struct spwd`; `-1` nos campos numéricos é "ausente" e `flag == u64::MAX` é `~0ul`.
#[derive(Clone, Debug)]
pub struct Shadow {
    pub name: Vec<u8>,
    pub pwdp: Option<Vec<u8>>,
    pub lstchg: i64,
    pub min: i64,
    pub max: i64,
    pub warn: i64,
    pub inact: i64,
    pub expire: i64,
    pub flag: u64,
}

/// `(long int) (int) tmp` dos campos de shadow.
fn as_long_int(v: u64) -> i64 {
    i64::from(v as u32 as i32)
}

/// `parse_line` de `sgetspent_r.c`.
pub fn parse_shadow(line: &[u8]) -> Option<Shadow> {
    let mut c = Cursor::new(line);
    let name = c.string_field(is_colon, false).to_vec();
    let none = u64::MAX; // ~0ul, e o "default" -1 passa pelo `as_long_int`
    if c.at_end() && is_nis_name(&name) {
        return Some(Shadow {
            name,
            pwdp: None,
            lstchg: 0,
            min: 0,
            max: 0,
            warn: -1,
            inact: -1,
            expire: -1,
            flag: none,
        });
    }
    let pwdp = c.string_field(is_colon, false).to_vec();
    let lstchg = as_long_int(c.int_field_maybe_null(is_colon, false, 10, none)?);
    let min = as_long_int(c.int_field_maybe_null(is_colon, false, 10, none)?);
    let max = as_long_int(c.int_field_maybe_null(is_colon, false, 10, none)?);
    while !c.at_end() && is_space(c.peek()) {
        c.p += 1;
    }
    let (mut warn, mut inact, mut expire, mut flag) = (-1i64, -1i64, -1i64, none);
    if !c.at_end() {
        warn = as_long_int(c.int_field_maybe_null(is_colon, false, 10, none)?);
        inact = as_long_int(c.int_field_maybe_null(is_colon, false, 10, none)?);
        expire = as_long_int(c.int_field_maybe_null(is_colon, false, 10, none)?);
        if !c.at_end() {
            flag = c.int_field_maybe_null(never, false, 10, none)?;
        }
    }
    Some(Shadow {
        name,
        pwdp: Some(pwdp),
        lstchg,
        min,
        max,
        warn,
        inact,
        expire,
        flag,
    })
}

/// `putspent`; `Err` é o EINVAL de nome ou senha com caractere proibido.
pub fn format_shadow(s: &Shadow) -> Result<Vec<u8>, ()> {
    if !valid_field(&s.name) || !valid_opt(&s.pwdp) {
        return Err(());
    }
    let mut out = Vec::new();
    out.extend_from_slice(&s.name);
    out.push(b':');
    if let Some(p) = &s.pwdp {
        out.extend_from_slice(p);
    }
    out.push(b':');
    for v in [s.lstchg, s.min, s.max, s.warn, s.inact, s.expire] {
        if v != -1 {
            out.extend_from_slice(v.to_string().as_bytes());
        }
        out.push(b':');
    }
    if s.flag != u64::MAX {
        out.extend_from_slice((s.flag as i64).to_string().as_bytes());
    }
    out.push(b'\n');
    Ok(out)
}

/// `struct sgrp`.
#[derive(Clone, Debug)]
pub struct GShadow {
    pub name: Vec<u8>,
    pub passwd: Option<Vec<u8>>,
    pub adm: Vec<Vec<u8>>,
    pub members: Vec<Vec<u8>>,
}

/// `parse_line` de `sgetsgent_r.c`.
pub fn parse_gshadow(line: &[u8]) -> Option<GShadow> {
    let mut c = Cursor::new(line);
    let name = c.string_field(is_colon, false).to_vec();
    if c.at_end() && is_nis_name(&name) {
        return Some(GShadow {
            name,
            passwd: None,
            adm: Vec::new(),
            members: Vec::new(),
        });
    }
    let passwd = c.string_field(is_colon, false).to_vec();
    let adm = c.parse_list(b':', is_comma);
    let members = c.parse_list(0, is_comma);
    Some(GShadow {
        name,
        passwd: Some(passwd),
        adm,
        members,
    })
}

/// `putsgent`; `Err` é o EINVAL de campo ou membro com caractere proibido.
pub fn format_gshadow(g: &GShadow) -> Result<Vec<u8>, ()> {
    if !valid_field(&g.name)
        || !valid_opt(&g.passwd)
        || !valid_list(&g.adm)
        || !valid_list(&g.members)
    {
        return Err(());
    }
    let mut out = Vec::new();
    out.extend_from_slice(&g.name);
    out.push(b':');
    if let Some(p) = &g.passwd {
        out.extend_from_slice(p);
    }
    out.push(b':');
    for (i, m) in g.adm.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.extend_from_slice(m);
    }
    out.push(b':');
    for (i, m) in g.members.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.extend_from_slice(m);
    }
    out.push(b'\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwd_roundtrip() {
        let p = parse_passwd(b"root:x:0:0:root:/root:/bin/bash").unwrap();
        assert_eq!(
            format_passwd(&p).unwrap(),
            b"root:x:0:0:root:/root:/bin/bash\n"
        );
        assert!(parse_passwd(b"bad:x:zero:0:a:b:c").is_none());
        assert!(parse_passwd(b"short:x:1").is_none());
        let p = parse_passwd(b"+").unwrap();
        assert_eq!(format_passwd(&p).unwrap(), b"+::::::\n");
        let p = parse_passwd(b"x:y:1:2:g:/d:/bin/a:b").unwrap();
        assert!(format_passwd(&p).is_err());
    }

    #[test]
    fn group_and_gshadow() {
        let g = parse_group(b"adm:x:4:syslog, john ,,ana").unwrap();
        assert_eq!(
            g.members,
            vec![b"syslog".to_vec(), b"john ".to_vec(), b"ana".to_vec()]
        );
        assert_eq!(format_group(&g).unwrap(), b"adm:x:4:syslog,john ,ana\n");
        let s = parse_gshadow(b"adm:*:root,a:syslog").unwrap();
        assert_eq!(format_gshadow(&s).unwrap(), b"adm:*:root,a:syslog\n");
        let s = parse_gshadow(b"root:*::").unwrap();
        assert_eq!(format_gshadow(&s).unwrap(), b"root:*::\n");
    }

    #[test]
    fn shadow_forms() {
        let s = parse_shadow(b"root:*:20714:0:99999:7:::").unwrap();
        assert_eq!(format_shadow(&s).unwrap(), b"root:*:20714:0:99999:7:::\n");
        let s = parse_shadow(b"old:x:1:2:3").unwrap();
        assert_eq!(format_shadow(&s).unwrap(), b"old:x:1:2:3::::\n");
        assert!(parse_shadow(b"x:y:1").is_none());
    }

    #[test]
    fn strtoull_forms() {
        assert_eq!(strtoull(b"  42x", 0, 10), (42, 4));
        assert_eq!(strtoull(b"-1", 0, 10), (u64::MAX, 2));
        assert_eq!(strtoull(b"0x50/", 0, 0), (0x50, 4));
        assert_eq!(strtoull(b"x", 0, 10), (0, 0));
        assert_eq!(strtoull(b"99999999999999999999", 0, 10).0, u64::MAX);
        assert_eq!(strtoull(b"ff:", 0, 16), (255, 2));
    }
}
