//! Bancos de rede e demais bancos do `getent` lidos dos arquivos de `/etc` como os módulos `files` do
//! NSS da glibc 2.41: services, protocols, rpc, networks, ethers, hosts, aliases e netgroup.
//! Cada banco tem o parser da linha (os `LINE_PARSER` de `nss/nss_files/files-*.c`) e as buscas por
//! chave que o `getent` usa.

use sysabi::{Errno, sys};

use super::db::{Cursor, is_colon, read_db_lines};
use super::inet::{ether_ntoa, inet_network, is_space, ntop4, ntop6, pton4, pton6};

fn is_slash(b: u8) -> bool {
    b == b'/'
}

fn name_eq_ci(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

// ---- services ----

#[derive(Clone, Debug)]
pub struct Service {
    pub name: Vec<u8>,
    /// Porta em ordem de host (o `htons` do parser guarda 16 bits).
    pub port: u16,
    pub proto: Vec<u8>,
    pub aliases: Vec<Vec<u8>>,
}

/// `parse_line` de `files-service.c`: `nome porta/proto alias...`, porta em qualquer base do C.
pub fn parse_service(line: &[u8]) -> Option<Service> {
    let mut c = Cursor::new(line);
    let name = c.string_field(is_space, true).to_vec();
    let port = c.int_field(is_slash, true, 0)?;
    let proto = c.string_field(is_space, true).to_vec();
    let aliases = c.parse_list(0, is_space);
    Some(Service {
        name,
        port: port as u16,
        proto,
        aliases,
    })
}

pub fn read_services() -> Result<Vec<Service>, Errno> {
    Ok(read_db_lines(b"/etc/services", b"#")?
        .iter()
        .filter_map(|l| parse_service(l))
        .collect())
}

pub fn format_service(s: &Service) -> Vec<u8> {
    let mut out = s.name.clone();
    pad_to(&mut out, 21);
    out.push(b' ');
    out.extend_from_slice(format!("{}/", s.port).as_bytes());
    out.extend_from_slice(&s.proto);
    for a in &s.aliases {
        out.push(b' ');
        out.extend_from_slice(a);
    }
    out.push(b'\n');
    out
}

/// `%-Ns`: completa com espaços até a largura (sem cortar o que passar).
pub fn pad_to(out: &mut Vec<u8>, width: usize) {
    while out.len() < width {
        out.push(b' ');
    }
}

// ---- protocols ----

#[derive(Clone, Debug)]
pub struct Protocol {
    pub name: Vec<u8>,
    pub number: i32,
    pub aliases: Vec<Vec<u8>>,
}

/// `parse_line` de `files-proto.c`: `nome número alias...`.
pub fn parse_protocol(line: &[u8]) -> Option<Protocol> {
    let mut c = Cursor::new(line);
    let name = c.string_field(is_space, true).to_vec();
    let number = c.int_field(is_space, true, 10)?;
    let aliases = c.parse_list(0, is_space);
    Some(Protocol {
        name,
        number: number as u32 as i32,
        aliases,
    })
}

pub fn read_protocols() -> Result<Vec<Protocol>, Errno> {
    Ok(read_db_lines(b"/etc/protocols", b"#")?
        .iter()
        .filter_map(|l| parse_protocol(l))
        .collect())
}

pub fn format_protocol(p: &Protocol) -> Vec<u8> {
    let mut out = p.name.clone();
    pad_to(&mut out, 21);
    out.extend_from_slice(format!(" {}", p.number).as_bytes());
    for a in &p.aliases {
        out.push(b' ');
        out.extend_from_slice(a);
    }
    out.push(b'\n');
    out
}

// ---- rpc ----

#[derive(Clone, Debug)]
pub struct Rpc {
    pub name: Vec<u8>,
    pub number: i32,
    pub aliases: Vec<Vec<u8>>,
}

/// `parse_line` de `files-rpc.c`.
pub fn parse_rpc(line: &[u8]) -> Option<Rpc> {
    let mut c = Cursor::new(line);
    let name = c.string_field(is_space, true).to_vec();
    let number = c.int_field(is_space, true, 10)?;
    let aliases = c.parse_list(0, is_space);
    Some(Rpc {
        name,
        number: number as u32 as i32,
        aliases,
    })
}

pub fn read_rpc() -> Result<Vec<Rpc>, Errno> {
    Ok(read_db_lines(b"/etc/rpc", b"#")?
        .iter()
        .filter_map(|l| parse_rpc(l))
        .collect())
}

/// `print_rpc`: o número é seguido de um espaço extra antes do primeiro alias (como no getent).
pub fn format_rpc(r: &Rpc) -> Vec<u8> {
    let mut out = r.name.clone();
    pad_to(&mut out, 15);
    out.extend_from_slice(format!(" {}", r.number).as_bytes());
    if !r.aliases.is_empty() {
        out.push(b' ');
    }
    for a in &r.aliases {
        out.push(b' ');
        out.extend_from_slice(a);
    }
    out.push(b'\n');
    out
}

// ---- networks ----

#[derive(Clone, Debug)]
pub struct Network {
    pub name: Vec<u8>,
    /// Número da rede em ordem de host (`n_net`).
    pub net: u32,
    pub aliases: Vec<Vec<u8>>,
}

/// `parse_line` de `files-network.c`: completa o endereço com `.0` até quatro partes e usa
/// `inet_network`.
pub fn parse_network(line: &[u8]) -> Option<Network> {
    let mut c = Cursor::new(line);
    let name = c.string_field(is_space, true).to_vec();
    let addr = c.string_field(is_space, true);
    let mut n = 1;
    if let Some(p) = addr.iter().position(|b| *b == b'.') {
        n += 1;
        if let Some(p2) = addr[p + 1..].iter().position(|b| *b == b'.') {
            n += 1;
            if addr[p + 1 + p2 + 1..].contains(&b'.') {
                n += 1;
            }
        }
    }
    let mut full = addr.to_vec();
    while n < 4 {
        full.extend_from_slice(b".0");
        n += 1;
    }
    let net = inet_network(&full);
    let aliases = c.parse_list(0, is_space);
    Some(Network { name, net, aliases })
}

pub fn read_networks() -> Result<Vec<Network>, Errno> {
    Ok(read_db_lines(b"/etc/networks", b"#")?
        .iter()
        .filter_map(|l| parse_network(l))
        .collect())
}

pub fn format_network(n: &Network) -> Vec<u8> {
    let mut out = n.name.clone();
    pad_to(&mut out, 21);
    out.push(b' ');
    out.extend_from_slice(ntop4(&n.net.to_be_bytes()).as_bytes());
    for a in &n.aliases {
        out.push(b' ');
        out.extend_from_slice(a);
    }
    out.push(b'\n');
    out
}

// ---- ethers ----

#[derive(Clone, Debug)]
pub struct Ether {
    pub addr: [u8; 6],
    pub name: Vec<u8>,
}

/// `parse_line` de `files-ethers.c`: `aa:bb:cc:dd:ee:ff nome`, seis números em hexa.
pub fn parse_ether(line: &[u8]) -> Option<Ether> {
    let mut c = Cursor::new(line);
    let mut addr = [0u8; 6];
    for (i, slot) in addr.iter_mut().enumerate() {
        let number = if i < 5 {
            c.int_field(is_colon, false, 16)?
        } else {
            c.int_field(is_space, true, 16)?
        };
        if number > 0xff {
            return None;
        }
        *slot = number as u8;
    }
    let name = c.string_field(is_space, true).to_vec();
    Some(Ether { addr, name })
}

pub fn read_ethers() -> Result<Vec<Ether>, Errno> {
    Ok(read_db_lines(b"/etc/ethers", b"#")?
        .iter()
        .filter_map(|l| parse_ether(l))
        .collect())
}

/// `printf ("%s %s\n", ether_ntoa (ethp), name)`.
pub fn format_ether(addr: &[u8; 6], name: &[u8]) -> Vec<u8> {
    let mut out = ether_ntoa(addr).into_bytes();
    out.push(b' ');
    out.extend_from_slice(name);
    out.push(b'\n');
    out
}

// ---- hosts ----

/// Família pedida ao parser de `files-hosts.c`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Af {
    Unspec,
    Inet,
    Inet6,
}

/// Uma linha de `/etc/hosts` já no formato pedido: o endereço tem 4 ou 16 bytes conforme `af`.
#[derive(Clone, Debug)]
pub struct HostLine {
    pub af: Af,
    pub addr: Vec<u8>,
    pub name: Vec<u8>,
    pub aliases: Vec<Vec<u8>>,
}

/// O endereço de 16 bytes é um IPv4 mapeado (`::ffff:a.b.c.d`).
pub fn is_v4_mapped(a: &[u8]) -> bool {
    a.len() == 16 && a[..10].iter().all(|b| *b == 0) && a[10] == 0xff && a[11] == 0xff
}

fn is_loopback6(a: &[u8]) -> bool {
    a.len() == 16 && a[..15].iter().all(|b| *b == 0) && a[15] == 1
}

/// `v4` vira `::ffff:v4` (o `map_v4v6_address`).
pub fn map_v4(v4: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; 10];
    out.extend_from_slice(&[0xff, 0xff]);
    out.extend_from_slice(v4);
    out
}

/// `parse_line` de `files-hosts.c` para a família `af` e a marca `AI_V4MAPPED`.
pub fn parse_host(line: &[u8], af: Af, v4mapped: bool) -> Option<HostLine> {
    let mut c = Cursor::new(line);
    let addr_text = c.string_field(is_space, true);
    let want = if af == Af::Unspec { Af::Inet } else { af };
    let (res_af, addr): (Af, Vec<u8>) = if let Some(a) = parse_addr(want, addr_text) {
        (want, a)
    } else if af == Af::Inet6 && v4mapped && pton4(addr_text).is_some() {
        (Af::Inet6, map_v4(&pton4(addr_text)?))
    } else if af == Af::Inet {
        let a6 = pton6(addr_text)?;
        if is_v4_mapped(&a6) {
            (Af::Inet, a6[12..].to_vec())
        } else if is_loopback6(&a6) {
            (Af::Inet, vec![127, 0, 0, 1])
        } else {
            return None;
        }
    } else if af == Af::Unspec && pton6(addr_text).is_some() {
        (Af::Inet6, pton6(addr_text)?.to_vec())
    } else {
        return None;
    };
    let name = c.string_field(is_space, true).to_vec();
    let aliases = c.parse_list(0, is_space);
    Some(HostLine {
        af: res_af,
        addr,
        name,
        aliases,
    })
}

fn parse_addr(af: Af, text: &[u8]) -> Option<Vec<u8>> {
    match af {
        Af::Inet => pton4(text).map(|a| a.to_vec()),
        Af::Inet6 => pton6(text).map(|a| a.to_vec()),
        Af::Unspec => None,
    }
}

/// As linhas legíveis de `/etc/hosts` no formato pedido.
pub fn read_hosts(af: Af, v4mapped: bool) -> Result<Vec<HostLine>, Errno> {
    Ok(read_db_lines(b"/etc/hosts", b"#")?
        .iter()
        .filter_map(|l| parse_host(l, af, v4mapped))
        .collect())
}

/// O `HostLine` casa o nome (principal ou alias), sem diferenciar maiúsculas.
pub fn host_matches(h: &HostLine, name: &[u8]) -> bool {
    name_eq_ci(name, &h.name) || h.aliases.iter().any(|a| name_eq_ci(name, a))
}

/// Resultado de uma busca de host: nome, aliases e um ou mais endereços da mesma família (o
/// tamanho de cada endereço, 4 ou 16 bytes, já diz qual).
#[derive(Clone, Debug)]
pub struct Host {
    pub name: Vec<u8>,
    pub aliases: Vec<Vec<u8>>,
    pub addrs: Vec<Vec<u8>>,
}

/// `print_hosts`: uma linha por endereço, `%-15s nome alias...`.
pub fn format_host(h: &Host) -> Vec<u8> {
    let mut out = Vec::new();
    for a in &h.addrs {
        let ip = format_ip(a);
        let mut line = ip.into_bytes();
        pad_to(&mut line, 15);
        line.push(b' ');
        line.extend_from_slice(&h.name);
        for al in &h.aliases {
            line.push(b' ');
            line.extend_from_slice(al);
        }
        line.push(b'\n');
        out.extend_from_slice(&line);
    }
    out
}

/// `inet_ntop` pelo tamanho do endereço.
pub fn format_ip(a: &[u8]) -> String {
    if a.len() == 4 {
        ntop4(&[a[0], a[1], a[2], a[3]])
    } else {
        let mut b = [0u8; 16];
        b.copy_from_slice(&a[..16]);
        ntop6(&b)
    }
}

/// Se `/etc/host.conf` (ou o arquivo de `RESOLV_HOST_CONF`) liga `multi`, ou `RESOLV_MULTI` no
/// ambiente: com ele o nome acumula todos os endereços de `/etc/hosts` (`HCONF_FLAG_MULTI`).
pub fn host_conf_multi() -> bool {
    let name = sys::getenv("RESOLV_HOST_CONF").unwrap_or_else(|| b"/etc/host.conf".to_vec());
    let mut multi = false;
    if let Ok(data) = sys::read_file(&name) {
        for raw in data.split(|b| *b == b'\n') {
            let mut i = 0;
            while i < raw.len() && is_space(raw[i]) {
                i += 1;
            }
            let line = &raw[i..];
            if line.is_empty() || line[0] == b'#' {
                continue;
            }
            let mut j = 0;
            while j < line.len() && !is_space(line[j]) && line[j] != b'#' && line[j] != b',' {
                j += 1;
            }
            if !line[..j].eq_ignore_ascii_case(b"multi") {
                continue;
            }
            let mut k = j;
            while k < line.len() && is_space(line[k]) {
                k += 1;
            }
            let arg = &line[k..];
            if arg.len() >= 2 && arg[..2].eq_ignore_ascii_case(b"on") {
                multi = true;
            } else if arg.len() >= 3 && arg[..3].eq_ignore_ascii_case(b"off") {
                multi = false;
            }
        }
    }
    if let Some(v) = sys::getenv("RESOLV_MULTI") {
        if v.len() >= 2 && v[..2].eq_ignore_ascii_case(b"on") {
            multi = true;
        } else if v.len() >= 3 && v[..3].eq_ignore_ascii_case(b"off") {
            multi = false;
        }
    }
    multi
}

/// `_nss_files_gethostbyname3_r`: o primeiro registro que casa e, com `multi`, os endereços e
/// aliases dos demais (`gethostbyname3_multi`).
pub fn files_host_by_name(name: &[u8], af: Af, multi: bool) -> (super::nss::Status, Option<Host>) {
    use super::nss::Status;
    let lines = match read_hosts(af, false) {
        Ok(l) => l,
        Err(_) => return (Status::Unavail, None),
    };
    let mut iter = lines.into_iter().filter(|h| host_matches(h, name));
    let Some(first) = iter.next() else {
        return (Status::NotFound, None);
    };
    let mut host = Host {
        name: first.name.clone(),
        aliases: first.aliases.clone(),
        addrs: vec![first.addr.clone()],
    };
    if multi {
        for other in iter {
            host.addrs.push(other.addr.clone());
            host.aliases.extend(other.aliases.iter().cloned());
            if other.name != host.name {
                host.aliases.push(other.name.clone());
            }
        }
    }
    (Status::Success, Some(host))
}

/// `_nss_files_gethostbyaddr_r`: a primeira linha cujo endereço bate com `addr` (4 bytes pedem
/// família IPv4; 16, IPv6 com os IPv4 mapeados).
pub fn files_host_by_addr(addr: &[u8]) -> (super::nss::Status, Option<Host>) {
    use super::nss::Status;
    let (af, mapped) = if addr.len() == 16 {
        (Af::Inet6, true)
    } else {
        (Af::Inet, false)
    };
    let lines = match read_hosts(af, mapped) {
        Ok(l) => l,
        Err(_) => return (Status::Unavail, None),
    };
    for h in lines {
        if h.addr.len() == addr.len() && h.addr == addr {
            return (
                Status::Success,
                Some(Host {
                    name: h.name,
                    aliases: h.aliases,
                    addrs: vec![h.addr],
                }),
            );
        }
    }
    (Status::NotFound, None)
}

// ---- aliases ----

#[derive(Clone, Debug)]
pub struct Alias {
    pub name: Vec<u8>,
    pub members: Vec<Vec<u8>>,
}

/// Leitor com a semântica de `fgets`/`getc` sobre o arquivo inteiro.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn fgets(&mut self) -> Option<&'a [u8]> {
        if self.pos >= self.data.len() {
            return None;
        }
        let rest = &self.data[self.pos..];
        let end = rest
            .iter()
            .position(|b| *b == b'\n')
            .map(|p| p + 1)
            .unwrap_or(rest.len());
        self.pos += end;
        Some(&rest[..end])
    }

    fn getc(&mut self) -> Option<u8> {
        let b = self.data.get(self.pos).copied();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }

    fn ungetc(&mut self) {
        self.pos -= 1;
    }
}

/// Corta a linha no primeiro `#` ou `\n` (e em NUL, que termina a string C).
fn cut_comment(line: &[u8]) -> &[u8] {
    let end = line
        .iter()
        .position(|b| *b == b'#' || *b == b'\n' || *b == 0)
        .unwrap_or(line.len());
    &line[..end]
}

enum NextAlias {
    Found(Alias),
    /// Linha sem membros: o chamador lê a seguinte.
    Return,
    Eof,
}

/// Lê os membros de um arquivo `:include:`: um por vírgula, brancos antes de cada um, `#` comenta.
fn read_include(path: &[u8], members: &mut Vec<Vec<u8>>) {
    let Ok(data) = sys::read_file(path) else {
        return;
    };
    let mut r = Reader {
        data: &data,
        pos: 0,
    };
    while let Some(raw) = r.fgets() {
        let mut line = cut_comment(raw);
        while !line.is_empty() {
            let mut i = 0;
            while i < line.len() && is_space(line[i]) {
                i += 1;
            }
            line = &line[i..];
            let end = line.iter().position(|b| *b == b',').unwrap_or(line.len());
            let piece = &line[..end];
            line = if end < line.len() {
                &line[end + 1..]
            } else {
                &line[end..]
            };
            if !piece.is_empty() {
                members.push(piece.to_vec());
            }
        }
    }
}

/// `get_next_alias` de `files-alias.c`: com `matching`, só devolve o alias de nome igual (sem
/// diferenciar maiúsculas) e pula as demais linhas e continuações.
fn get_next_alias(r: &mut Reader<'_>, matching: Option<&[u8]>) -> NextAlias {
    let mut ignore = false;
    loop {
        let Some(raw) = r.fgets() else {
            return NextAlias::Eof;
        };
        if ignore && raw.first().is_some_and(|b| is_space(*b)) {
            continue;
        }
        let mut line = cut_comment(raw);
        let mut i = 0;
        while i < line.len() && is_space(line[i]) {
            i += 1;
        }
        line = &line[i..];
        let colon = line.iter().position(|b| *b == b':');
        let Some(colon) = colon else { continue };
        if colon == 0 {
            continue;
        }
        let name = line[..colon].to_vec();
        let mut rest: &[u8] = &line[colon + 1..];
        ignore = matching.is_some_and(|m| !name_eq_ci(&name, m));
        if ignore {
            continue;
        }
        let mut members: Vec<Vec<u8>> = Vec::new();
        loop {
            let mut j = 0;
            while j < rest.len() && is_space(rest[j]) {
                j += 1;
            }
            rest = &rest[j..];
            let end = rest.iter().position(|b| *b == b',').unwrap_or(rest.len());
            let piece = rest[..end].to_vec();
            if !piece.is_empty() {
                rest = if end < rest.len() {
                    &rest[end + 1..]
                } else {
                    &rest[end..]
                };
                if let Some(path) = piece.strip_prefix(b":include:") {
                    read_include(path, &mut members);
                } else {
                    members.push(piece);
                }
            } else if end < rest.len() {
                // Elemento vazio entre vírgulas: segue adiante (o original ficaria preso aqui).
                rest = &rest[end + 1..];
            }
            if rest.is_empty() {
                // Fim da linha: uma linha seguinte que começa com branco continua este alias.
                match r.getc() {
                    Some(ch) if ch != b'\n' && is_space(ch) => match r.fgets() {
                        // O primeiro branco já foi lido pelo `getc`; o resto da linha é a continuação.
                        Some(next) => rest = cut_comment(next),
                        None => rest = &[],
                    },
                    other => {
                        if other.is_some() {
                            r.ungetc();
                        }
                        return if members.is_empty() {
                            NextAlias::Return
                        } else {
                            NextAlias::Found(Alias { name, members })
                        };
                    }
                }
            }
        }
    }
}

/// Todos os aliases de `/etc/aliases` (o `getaliasent`).
pub fn read_aliases() -> Result<Vec<Alias>, Errno> {
    let data = sys::read_file(b"/etc/aliases")?;
    let mut r = Reader {
        data: &data,
        pos: 0,
    };
    let mut out = Vec::new();
    loop {
        match get_next_alias(&mut r, None) {
            NextAlias::Found(a) => out.push(a),
            NextAlias::Return => {}
            NextAlias::Eof => break,
        }
    }
    Ok(out)
}

/// `getaliasbyname`.
pub fn alias_by_name(name: &[u8]) -> (super::nss::Status, Option<Alias>) {
    use super::nss::Status;
    let data = match sys::read_file(b"/etc/aliases") {
        Ok(d) => d,
        Err(_) => return (Status::Unavail, None),
    };
    let mut r = Reader {
        data: &data,
        pos: 0,
    };
    loop {
        match get_next_alias(&mut r, Some(name)) {
            NextAlias::Found(a) => return (Status::Success, Some(a)),
            NextAlias::Return => {}
            NextAlias::Eof => return (Status::NotFound, None),
        }
    }
}

/// `print_aliases`: `nome:` e brancos até a coluna 14, depois os membros separados por `, `.
pub fn format_alias(a: &Alias) -> Vec<u8> {
    let mut out = a.name.clone();
    out.extend_from_slice(b": ");
    let mut i = a.name.len();
    while i < 14 {
        out.push(b' ');
        i += 1;
    }
    for (k, m) in a.members.iter().enumerate() {
        out.extend_from_slice(m);
        out.extend_from_slice(if k + 1 == a.members.len() {
            b"\n"
        } else {
            b", "
        });
    }
    out
}

// ---- netgroup ----

/// Um item de netgroup: uma tripla `(host,usuário,domínio)` (`None` é o campo vazio) ou o nome de
/// outro netgroup.
#[derive(Clone, Debug)]
pub enum NetItem {
    Triple(Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>),
    Group(Vec<u8>),
}

/// `_nss_files_setnetgrent`: o texto da definição de `group` em `/etc/netgroup`, com as
/// continuações `\` + nova linha emendadas por um espaço.
pub fn load_netgroup(group: &[u8]) -> (super::nss::Status, Option<Vec<u8>>) {
    use super::nss::Status;
    if group.is_empty() {
        return (Status::Unavail, None);
    }
    let data = match sys::read_file(b"/etc/netgroup") {
        Ok(d) => d,
        Err(_) => return (Status::Unavail, None),
    };
    let mut r = Reader {
        data: &data,
        pos: 0,
    };
    while let Some(line) = r.fgets() {
        let found =
            line.len() > group.len() && line.starts_with(group) && is_space(line[group.len()]);
        let mut text: Vec<u8> = Vec::new();
        if found {
            text.extend_from_slice(&line[group.len() + 1..]);
        }
        let mut cur: &[u8] = line;
        while cur.len() > 1 && cur[cur.len() - 1] == b'\n' && cur[cur.len() - 2] == b'\\' {
            if found {
                let keep = text.len().saturating_sub(2);
                text.truncate(keep);
            }
            match r.fgets() {
                Some(next) => {
                    if found {
                        text.push(b' ');
                        text.extend_from_slice(next);
                    }
                    cur = next;
                }
                None => break,
            }
        }
        if found {
            return (Status::Success, Some(text));
        }
    }
    (Status::NotFound, None)
}

fn strip_whitespace(s: &[u8]) -> Option<Vec<u8>> {
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let start = i;
    while i < s.len() && !is_space(s[i]) {
        i += 1;
    }
    if start == i {
        None
    } else {
        Some(s[start..i].to_vec())
    }
}

/// Os itens de uma definição (`_nss_netgroup_parseline` em laço): triplas e nomes de outros grupos;
/// uma tripla malformada encerra a lista.
pub fn parse_netgroup_items(text: &[u8]) -> Vec<NetItem> {
    let mut items = Vec::new();
    let mut cp = 0usize;
    let at = |i: usize| text.get(i).copied().unwrap_or(0);
    loop {
        while cp < text.len() && is_space(at(cp)) {
            cp += 1;
        }
        if cp >= text.len() {
            break;
        }
        if at(cp) != b'(' {
            let start = cp;
            while cp < text.len() && !is_space(at(cp)) {
                cp += 1;
            }
            items.push(NetItem::Group(text[start..cp].to_vec()));
            continue;
        }
        cp += 1;
        let host_start = cp;
        while cp < text.len() && at(cp) != b',' {
            cp += 1;
        }
        if cp >= text.len() {
            break;
        }
        let host_end = cp;
        cp += 1;
        let user_start = cp;
        while cp < text.len() && at(cp) != b',' {
            cp += 1;
        }
        if cp >= text.len() {
            break;
        }
        let user_end = cp;
        cp += 1;
        let dom_start = cp;
        while cp < text.len() && at(cp) != b')' {
            cp += 1;
        }
        if cp >= text.len() {
            break;
        }
        let dom_end = cp;
        cp += 1;
        items.push(NetItem::Triple(
            strip_whitespace(&text[host_start..host_end]),
            strip_whitespace(&text[user_start..user_end]),
            strip_whitespace(&text[dom_start..dom_end]),
        ));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_and_protocols() {
        let s = parse_service(b"http 80/tcp www").unwrap();
        assert_eq!(
            format_service(&s),
            b"http                  80/tcp www\n".to_vec()
        );
        let s = parse_service(b"x 0x50//udp a b").unwrap();
        assert_eq!((s.port, s.proto.as_slice()), (0x50, &b"udp"[..]));
        assert!(parse_service(b"ssh").is_none());
        let p = parse_protocol(b"tcp 6 TCP").unwrap();
        assert_eq!(
            format_protocol(&p),
            b"tcp                   6 TCP\n".to_vec()
        );
        let r = parse_rpc(b"portmapper 100000 portmap sunrpc").unwrap();
        assert_eq!(
            format_rpc(&r),
            b"portmapper      100000  portmap sunrpc\n".to_vec()
        );
    }

    #[test]
    fn networks_and_ethers() {
        let n = parse_network(b"loopback 127.0.0.0").unwrap();
        assert_eq!(n.net, 0x7f00_0000);
        assert_eq!(
            format_network(&n),
            b"loopback              127.0.0.0\n".to_vec()
        );
        let n = parse_network(b"net10 10").unwrap();
        assert_eq!(n.net, 0x0a00_0000);
        let e = parse_ether(b"00:11:22:33:44:55 host1").unwrap();
        assert_eq!(
            format_ether(&e.addr, &e.name),
            b"0:11:22:33:44:55 host1\n".to_vec()
        );
        assert!(parse_ether(b"00:11:22:33:44 host1").is_none());
    }

    #[test]
    fn hosts_parsing_by_family() {
        let h = parse_host(b"::1 localhost ip6-localhost", Af::Inet6, false).unwrap();
        assert_eq!(format_ip(&h.addr), "::1");
        let h = parse_host(b"::1 localhost ip6-localhost", Af::Inet, false).unwrap();
        assert_eq!(h.addr, vec![127, 0, 0, 1]);
        assert!(parse_host(b"fe00::0 ip6-localnet", Af::Inet, false).is_none());
        assert!(parse_host(b"127.0.0.1 localhost", Af::Inet6, false).is_none());
        let h = parse_host(b"127.0.0.1 localhost", Af::Inet6, true).unwrap();
        assert_eq!(format_ip(&h.addr), "::ffff:127.0.0.1");
    }

    #[test]
    fn netgroup_items() {
        let items = parse_netgroup_items(b"(h1,u1,d1) (,u2,) other (a b , c , )\n");
        assert_eq!(items.len(), 4);
        assert!(
            matches!(&items[3], NetItem::Triple(Some(h), Some(u), None) if h == b"a" && u == b"c")
        );
        assert!(matches!(&items[2], NetItem::Group(g) if g == b"other"));
    }
}
