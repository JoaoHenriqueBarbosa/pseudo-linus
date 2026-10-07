//! `getent` da glibc 2.41 (Debian 13), portado de `nss/getent.c`: consulta os bancos administrativos
//! (passwd, group, shadow, gshadow, hosts, ahosts*, services, protocols, networks, ethers, rpc,
//! aliases, netgroup e initgroups) pelo mecanismo do NSS, lendo os arquivos de `/etc` do sandbox como
//! o módulo `files`.
//!
//! - Opções no estilo do argp da glibc (`-s`, `-i`, `-A`, `-?`/`--help`, `--usage`, `-V`), com os
//!   textos de ajuda byte a byte, "Try `getent --help'..." e o código 64 para opção inválida.
//! - Busca por nome, por número e enumeração; códigos de saída 0, 1 (uso), 2 (chave não achada) e 3
//!   (enumeração não suportada).
//! - `/etc/nsswitch.conf` decide as fontes de cada banco. Só `files` (e `compat`) existem; as demais
//!   (`db`, `dns`, `nis`...) respondem como indisponíveis, o que o NSS trata como "continue".
//! - `ahosts*` emulam o `getaddrinfo` do sandbox: só `/etc/hosts` e endereços numéricos, sem
//!   interface de rede além do loopback (por isso `AI_ADDRCONFIG` esvazia as consultas de família
//!   fixa, como no contêiner do oráculo sem rede).

mod db;
mod inet;
mod netdb;
mod nss;

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno};

use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

use self::db::{
    format_group, format_gshadow, format_passwd, format_shadow, parse_group, parse_gshadow,
    parse_passwd, parse_shadow, read_records, strtoull,
};
use self::inet::{inet_aton, pton4, pton6};
use self::netdb::{
    Af, Host, NetItem, NumberedDb, alias_by_name, files_host_by_addr, files_host_by_name,
    format_alias, format_ether, format_host, format_network, format_service, host_conf_multi,
    load_netgroup, map_v4, named, pad_to, format_ip, parse_netgroup_items, read_aliases, read_ethers,
    read_hosts, read_networks, read_services,
};
use self::nss::{NssConf, Status};

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("service", HasArg::Required, 's' as i32),
    LongOpt::new("no-idn", HasArg::No, 'i' as i32),
    LongOpt::new("no-addrconfig", HasArg::No, 'A' as i32),
    LongOpt::new("help", HasArg::No, '?' as i32),
    LongOpt::new("usage", HasArg::No, 256),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

const HELP: &str = "Usage: getent [OPTION...] database [key ...]
Get entries from administrative database.

  -A, --no-addrconfig        do not filter out unsupported IPv4/IPv6 addresses
                             (with ahosts*)
  -i, --no-idn               disable IDN encoding
  -s, --service=CONFIG       Service configuration to be used
  -?, --help                 Give this help list
      --usage                Give a short usage message
  -V, --version              Print program version

Mandatory or optional arguments to long options are also mandatory or optional
for any corresponding short options.

Supported databases:
ahosts ahostsv4 ahostsv6 aliases ethers group gshadow hosts initgroups
netgroup networks passwd protocols rpc services shadow

For bug reporting instructions, please see:
<http://www.debian.org/Bugs/>.
";

const USAGE: &str = "Usage: getent [-Ai?V] [-s CONFIG] [--no-addrconfig] [--no-idn]
            [--service=CONFIG] [--help] [--usage] [--version]
            database [key ...]
";

const VERSION: &str = "getent (Debian GLIBC 2.41-12+deb13u4) 2.41
Copyright (C) 2024 Free Software Foundation, Inc.
This is free software; see the source for copying conditions.  There is NO
warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
Written by Thorsten Kukuk.
";

/// Os bancos, na ordem da tabela `databases[]` do `getent.c`.
const DATABASES: &[&str] = &[
    "ahosts",
    "ahostsv4",
    "ahostsv6",
    "aliases",
    "ethers",
    "group",
    "gshadow",
    "hosts",
    "initgroups",
    "netgroup",
    "networks",
    "passwd",
    "protocols",
    "rpc",
    "services",
    "shadow",
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// O que as funções de banco precisam do contexto.
struct Env {
    conf: NssConf,
    addrconfig: bool,
}

fn short_name(argv0: &str) -> &str {
    argv0.rsplit('/').next().unwrap_or(argv0)
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let name = short_name(&argv0).to_string();
    let mut out = io::stdout();
    let mut env = Env {
        conf: NssConf::load(),
        addrconfig: true,
    };

    let mut getopt = Getopt::from_env(&argv[1..], "s:iAV?", LONGOPTS);
    while let Some(r) = getopt.next_opt() {
        let opt = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!(
                    "{}\nTry `{name} --help' or `{name} --usage' for more information.\n",
                    e.message(&argv0)
                ));
                return 64;
            }
        };
        match opt.id {
            id if id == 's' as i32 => {
                let arg = opt.arg.clone().unwrap_or_default();
                if let Err(code) = configure_service(&mut env.conf, &arg, &argv0) {
                    return code;
                }
            }
            id if id == 'i' as i32 => {}
            id if id == 'A' as i32 => env.addrconfig = false,
            id if id == '?' as i32 => {
                let _ = out.write_all(HELP.replace("getent", &name).as_bytes());
                return 0;
            }
            256 => {
                let _ = out.write_all(USAGE.replace("getent", &name).as_bytes());
                return 0;
            }
            id if id == 'V' as i32 => {
                let _ = out.write_all(VERSION.replace("getent", &name).as_bytes());
                return 0;
            }
            _ => {}
        }
    }
    let operands = getopt.operands();
    let see = format!("Try `{name} --help' or `{name} --usage' for more information.\n");
    if operands.is_empty() {
        io::eprint(format!("{argv0}: wrong number of arguments\n"));
        let _ = out.write_all(see.as_bytes());
        return 1;
    }
    let keys = &operands[1..];
    let out: &mut dyn Write = &mut out;
    match operands[0].as_slice() {
        b"ahosts" => ahosts_keys(&env, out, Af::Unspec, keys),
        b"ahostsv4" => ahosts_keys(&env, out, Af::Inet, keys),
        b"ahostsv6" => ahosts_keys(&env, out, Af::Inet6, keys),
        b"aliases" => aliases_keys(&env, out, keys),
        b"ethers" => ethers_keys(&env, out, keys),
        b"group" => accounts(&env, "group", out, keys, parse_group, format_group, |g| {
            (&g.name, Some(g.gid))
        }),
        b"gshadow" => accounts(&env, "gshadow", out, keys, parse_gshadow, format_gshadow, |g| {
            (&g.name, None)
        }),
        b"hosts" => hosts_keys(&env, out, keys),
        b"initgroups" => initgroups_keys(&env, out, keys),
        b"netgroup" => netgroup_keys(&env, out, keys),
        b"networks" => networks_keys(&env, out, keys),
        b"passwd" => accounts(&env, "passwd", out, keys, parse_passwd, format_passwd, |p| {
            (&p.name, Some(p.uid))
        }),
        b"protocols" => numbered_keys(&env, out, keys, NumberedDb::Protocols),
        b"rpc" => numbered_keys(&env, out, keys, NumberedDb::Rpc),
        b"services" => services_keys(&env, out, keys),
        b"shadow" => accounts(&env, "shadow", out, keys, parse_shadow, format_shadow, |s| {
            (&s.name, None)
        }),
        other => {
            io::eprint(format!("Unknown database: {}\n", io::lossy(other)));
            let _ = out.write_all(see.as_bytes());
            1
        }
    }
}

/// O tratamento de `-s` do `parse_option`: sem `:` vale pra todos os bancos; com `BANCO:CONFIG` o
/// primeiro banco da tabela cujo nome começa com o texto antes do `:`.
fn configure_service(conf: &mut NssConf, arg: &[u8], argv0: &str) -> Result<(), i32> {
    match arg.iter().position(|b| *b == b':') {
        None => {
            for db in DATABASES {
                conf.configure(db, arg);
            }
            Ok(())
        }
        Some(p) => {
            let prefix = &arg[..p];
            for db in DATABASES {
                if db.as_bytes().starts_with(prefix) {
                    conf.configure(db, &arg[p + 1..]);
                    return Ok(());
                }
            }
            io::eprint(format!("{argv0}: Unknown database name\n"));
            Err(1)
        }
    }
}

// ---- utilitários comuns ----

/// Consulta pontual num arquivo: o primeiro registro que satisfaz `pred`.
fn find_in<T>(records: Result<Vec<T>, Errno>, pred: impl Fn(&T) -> bool) -> (Status, Option<T>) {
    match records {
        Ok(v) => match v.into_iter().find(|r| pred(r)) {
            Some(r) => (Status::Success, Some(r)),
            None => (Status::NotFound, None),
        },
        Err(_) => (Status::Unavail, None),
    }
}

/// Enumeração de um arquivo: todos os registros, ou fonte indisponível.
fn all_in<T>(records: Result<Vec<T>, Errno>) -> (Status, Vec<T>) {
    match records {
        Ok(v) => (Status::Success, v),
        Err(_) => (Status::Unavail, Vec::new()),
    }
}

fn write_bytes(out: &mut dyn Write, bytes: &[u8]) {
    let _ = out.write_all(bytes);
}

/// Imprime uma entrada de passwd, group, shadow ou gshadow; se o `putXXent` recusou (campo com `:`),
/// avisa no stderr e não imprime nada, como o `print_passwd` e companhia.
fn print_entry(out: &mut dyn Write, entry: Result<Vec<u8>, ()>, what: &str) {
    match entry {
        Ok(bytes) => write_bytes(out, &bytes),
        Err(()) => io::eprint(format!("error writing {what} entry: Invalid argument\n")),
    }
}

/// A chave numérica de passwd e group: `strtoul (key, &ep, 10)` consumindo a string toda, truncada
/// para 32 bits como a atribuição a `uid_t`/`gid_t`.
fn numeric_id(key: &[u8]) -> Option<u32> {
    if key.is_empty() {
        return None;
    }
    let (v, end) = strtoull(key, 0, 10);
    if end == key.len() {
        Some(v as u32)
    } else {
        None
    }
}

fn is_nis_name(name: &[u8]) -> bool {
    matches!(name.first(), Some(b'+') | Some(b'-'))
}

/// `atol` guardado em `int` (o que `getprotobynumber` e `getrpcbynumber` recebem).
fn atol_int(key: &[u8]) -> i32 {
    let (v, _) = strtoull(key, 0, 10);
    let v = v.min(i64::MAX as u64);
    v as i64 as i32
}

fn first_digit(key: &[u8]) -> bool {
    key.first().is_some_and(|b| b.is_ascii_digit())
}

// ---- o laço de todos os bancos ----

/// O laço de todo banco do getent: sem chave, `list` imprime a enumeração ou devolve `false` quando
/// o banco não a suporta (e o getent sai com 3); com chaves, cada uma passa por `find`, o achado sai
/// por `show` e a chave sem resposta faz o código de saída ser 2.
fn run_db<T>(
    db: &str,
    out: &mut dyn Write,
    keys: &[Vec<u8>],
    list: impl FnOnce(&mut dyn Write) -> bool,
    mut find: impl FnMut(&[u8]) -> Option<T>,
    show: impl Fn(&mut dyn Write, &T),
) -> i32 {
    if keys.is_empty() {
        if list(out) {
            return 0;
        }
        io::eprint(format!("Enumeration not supported on {db}\n"));
        return 3;
    }
    let mut result = 0;
    for key in keys {
        match find(key) {
            Some(found) => show(out, &found),
            None => result = 2,
        }
    }
    result
}

/// A enumeração dos bancos que o getent não sabe percorrer.
fn no_listing(_: &mut dyn Write) -> bool {
    false
}

/// A enumeração de um banco de arquivo pelas fontes do NSS, impressa por `show`.
fn listing<'a, T>(
    env: &'a Env,
    db: &'a str,
    read: impl Fn() -> Result<Vec<T>, Errno> + 'a,
    show: impl Fn(&mut dyn Write, &T) + 'a,
) -> impl FnOnce(&mut dyn Write) -> bool + 'a {
    move |out| {
        for record in &env.conf.enumerate(db, |_| all_in(read())) {
            show(out, record);
        }
        true
    }
}

/// Um banco de arquivo que se enumera inteiro pelas fontes do NSS: `read` lê os registros,
/// `matches` decide se um registro responde à chave e `show` o imprime.
fn files_db<T>(
    env: &Env,
    db: &str,
    out: &mut dyn Write,
    keys: &[Vec<u8>],
    read: impl Fn() -> Result<Vec<T>, Errno>,
    matches: impl Fn(&[u8], &T) -> bool,
    show: impl Fn(&mut dyn Write, &T),
) -> i32 {
    let find = |key: &[u8]| env.conf.lookup(db, |_| find_in(read(), |r| matches(key, r)));
    run_db(db, out, keys, listing(env, db, &read, &show), find, &show)
}

// ---- passwd, group, shadow e gshadow ----

/// O nome de uma conta e, em passwd e group, o número que a chave numérica procura.
type AccountKey<T> = for<'a> fn(&'a T) -> (&'a [u8], Option<u32>);

/// Um banco de contas (`/etc/<db>`): chave numérica procura o uid/gid, as demais o nome, e entradas
/// NIS (`+`/`-`) nunca respondem; a entrada que o `putXXent` recusa vira aviso, como no getent.
fn accounts<T>(
    env: &Env,
    db: &str,
    out: &mut dyn Write,
    keys: &[Vec<u8>],
    parse: fn(&[u8]) -> Option<T>,
    format: fn(&T) -> Result<Vec<u8>, ()>,
    account_key: AccountKey<T>,
) -> i32 {
    let path = format!("/etc/{db}");
    files_db(
        env,
        db,
        out,
        keys,
        || read_records(path.as_bytes(), b"", parse),
        |key, record| {
            let (name, id) = account_key(record);
            match (numeric_id(key), id) {
                (Some(wanted), Some(id)) => id == wanted && !is_nis_name(name),
                _ => !is_nis_name(key) && name == key,
            }
        },
        |out, record| print_entry(out, format(record), db),
    )
}

// ---- initgroups ----

/// Os grupos de `user` em `/etc/group` (o `_nss_files_initgroups_dyn` com `group` = -1): os que o
/// listam entre os membros, na ordem do arquivo.
fn group_ids_of(user: &[u8]) -> (Status, Vec<u32>) {
    match read_records(b"/etc/group", b"", parse_group) {
        Ok(groups) => {
            let ids: Vec<u32> = groups
                .iter()
                .filter(|g| g.gid != u32::MAX && g.members.iter().any(|m| m == user))
                .map(|g| g.gid)
                .collect();
            if ids.is_empty() {
                (Status::NotFound, ids)
            } else {
                (Status::Success, ids)
            }
        }
        Err(_) => (Status::Unavail, Vec::new()),
    }
}

/// `getgrouplist (user, -1, ...)`: a lista de gids (sem o -1 inicial), com os duplicados contra as
/// fontes anteriores removidos.
fn get_group_list(env: &Env, user: &[u8]) -> Vec<u32> {
    let init = env.conf.sources("initgroups");
    let (sources, use_initgroups_entry) = if !init.is_empty() {
        (init, true)
    } else {
        (env.conf.sources("group"), false)
    };
    let mut list: Vec<u32> = Vec::new();
    for src in sources {
        let (status, ids) = if src.is_files() {
            group_ids_of(user)
        } else {
            (Status::Unavail, Vec::new())
        };
        let prev = list.clone();
        for id in ids {
            if !prev.contains(&id) {
                list.push(id);
            }
        }
        if (use_initgroups_entry || status != Status::Success) && src.stops_on(status) {
            break;
        }
    }
    list
}

fn initgroups_keys(env: &Env, out: &mut dyn Write, keys: &[Vec<u8>]) -> i32 {
    let find = |key: &[u8]| {
        let mut line = key.to_vec();
        pad_to(&mut line, 21);
        for gid in get_group_list(env, key) {
            if gid != u32::MAX {
                line.extend_from_slice(format!(" {}", i64::from(gid)).as_bytes());
            }
        }
        line.push(b'\n');
        Some(line)
    };
    run_db("initgroups", out, keys, no_listing, find, show_line)
}

/// O `show` dos bancos que já entregam a linha pronta.
fn show_line(out: &mut dyn Write, line: &Vec<u8>) {
    write_bytes(out, line);
}

// ---- services ----

fn services_keys(env: &Env, out: &mut dyn Write, keys: &[Vec<u8>]) -> i32 {
    let matches = |key: &[u8], s: &netdb::Service| {
        let (name, proto): (&[u8], Option<&[u8]>) = match key.iter().position(|b| *b == b'/') {
            Some(p) => (&key[..p], Some(&key[p + 1..])),
            None => (key, None),
        };
        if proto.is_some_and(|p| s.proto != p) {
            return false;
        }
        let (port, end) = strtoull(name, 0, 10);
        if first_digit(name) && end == name.len() && port <= 65535 {
            s.port == port as u16
        } else {
            named(&s.name, &s.aliases, name)
        }
    };
    let show = |out: &mut dyn Write, s: &netdb::Service| write_bytes(out, &format_service(s));
    files_db(env, "services", out, keys, read_services, matches, show)
}

// ---- protocols e rpc ----

/// Chave que começa com dígito procura o número (`atol` guardado em `int`); as demais, o nome ou
/// um alias.
fn numbered_keys(env: &Env, out: &mut dyn Write, keys: &[Vec<u8>], db: NumberedDb) -> i32 {
    let matches = |key: &[u8], n: &netdb::Numbered| {
        if first_digit(key) {
            n.number == atol_int(key)
        } else {
            named(&n.name, &n.aliases, key)
        }
    };
    let show = |out: &mut dyn Write, n: &netdb::Numbered| write_bytes(out, &db.format(n));
    files_db(env, db.name(), out, keys, || db.read(), matches, show)
}

// ---- networks ----

fn networks_keys(env: &Env, out: &mut dyn Write, keys: &[Vec<u8>]) -> i32 {
    let matches = |key: &[u8], n: &netdb::Network| {
        if first_digit(key) {
            // `getnetbyaddr (ntohl (inet_addr (key)), AF_UNSPEC)`; inet_addr falha com 0xffffffff.
            n.net == inet_aton(key, false).unwrap_or(0xffff_ffff)
        } else {
            n.name.eq_ignore_ascii_case(key) || n.aliases.iter().any(|a| a.eq_ignore_ascii_case(key))
        }
    };
    let show = |out: &mut dyn Write, n: &netdb::Network| write_bytes(out, &format_network(n));
    files_db(env, "networks", out, keys, read_networks, matches, show)
}

// ---- ethers ----

fn ethers_keys(env: &Env, out: &mut dyn Write, keys: &[Vec<u8>]) -> i32 {
    let find = |key: &[u8]| match inet::ether_aton(key) {
        // `ether_ntohost`: o nome vem do arquivo.
        Some(addr) => env
            .conf
            .lookup("ethers", |_| find_in(read_ethers(), |e| e.addr == addr)),
        // `ether_hostton`: o nome impresso é a própria chave.
        None => env
            .conf
            .lookup("ethers", |_| find_in(read_ethers(), |e| e.name.eq_ignore_ascii_case(key)))
            .map(|e| netdb::Ether {
                name: key.to_vec(),
                ..e
            }),
    };
    let show = |out: &mut dyn Write, e: &netdb::Ether| write_bytes(out, &format_ether(&e.addr, &e.name));
    run_db("ethers", out, keys, no_listing, find, show)
}

// ---- aliases ----

fn aliases_keys(env: &Env, out: &mut dyn Write, keys: &[Vec<u8>]) -> i32 {
    let show = |out: &mut dyn Write, a: &netdb::Alias| write_bytes(out, &format_alias(a));
    let find = |key: &[u8]| env.conf.lookup("aliases", |_| alias_by_name(key));
    run_db("aliases", out, keys, listing(env, "aliases", read_aliases, show), find, show)
}

// ---- netgroup ----

/// Uma tripla de netgroup: (host, usuário, domínio), cada campo podendo faltar.
type NetgroupTriple = (Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>);

/// Expande um netgroup como `getnetgrent`: as triplas do grupo na ordem e, depois, as dos grupos
/// citados (uma pilha: o último citado vem primeiro), cada um uma vez só. `None` se o grupo inicial
/// não existe em nenhuma fonte.
fn netgroup_triples(env: &Env, group: &[u8]) -> Option<Vec<NetgroupTriple>> {
    let load = |g: &[u8]| env.conf.lookup("netgroup", |_| load_netgroup(g));
    let mut current = load(group)?;
    let mut known: Vec<Vec<u8>> = vec![group.to_vec()];
    let mut needed: Vec<Vec<u8>> = Vec::new();
    let mut triples = Vec::new();
    loop {
        for item in parse_netgroup_items(&current) {
            match item {
                NetItem::Triple(h, u, d) => triples.push((h, u, d)),
                NetItem::Group(g) => {
                    if !known.contains(&g) && !needed.contains(&g) {
                        needed.insert(0, g);
                    }
                }
            }
        }
        let mut next = None;
        while next.is_none() && !needed.is_empty() {
            let g = needed.remove(0);
            known.insert(0, g.clone());
            next = load(&g[..]);
        }
        match next {
            Some(text) => current = text,
            None => break,
        }
    }
    Some(triples)
}

fn netgroup_keys(env: &Env, out: &mut dyn Write, keys: &[Vec<u8>]) -> i32 {
    if keys.is_empty() {
        io::eprint("Enumeration not supported on netgroup\n");
        return 3;
    }
    let mut result = 0;
    if keys.len() == 4 {
        let wild = |k: &Vec<u8>| if k == b"*" { None } else { Some(k.clone()) };
        let (host, user, domain) = (wild(&keys[1]), wild(&keys[2]), wild(&keys[3]));
        let matched = match netgroup_triples(env, &keys[0]) {
            Some(ts) => ts.iter().any(|(h, u, d)| {
                // Campo que falta casa com tudo; host e domínio sem diferenciar maiúsculas.
                let same = |a: &Option<Vec<u8>>, b: &Option<Vec<u8>>, fold: bool| match (a, b) {
                    (Some(x), Some(y)) if fold => x.eq_ignore_ascii_case(y),
                    (Some(x), Some(y)) => x == y,
                    _ => true,
                };
                same(h, &host, true) && same(u, &user, false) && same(d, &domain, true)
            }),
            None => false,
        };
        let mut line = keys[0].clone();
        pad_to(&mut line, 21);
        push_triple(&mut line, &host, &user, &domain, b"");
        line.extend_from_slice(format!(" = {}\n", i32::from(matched)).as_bytes());
        write_bytes(out, &line);
    } else if keys.len() == 1 {
        match netgroup_triples(env, &keys[0]) {
            None => result = 2,
            Some(ts) => {
                let mut line = keys[0].clone();
                pad_to(&mut line, 21);
                for (h, u, d) in ts {
                    push_triple(&mut line, &h, &u, &d, b" ");
                }
                line.push(b'\n');
                write_bytes(out, &line);
            }
        }
    }
    result
}

/// ` (host,usuário,domínio)`, com `no_host` no lugar do host que falta.
fn push_triple(
    line: &mut Vec<u8>,
    host: &Option<Vec<u8>>,
    user: &Option<Vec<u8>>,
    domain: &Option<Vec<u8>>,
    no_host: &[u8],
) {
    line.extend_from_slice(b" (");
    line.extend_from_slice(host.as_deref().unwrap_or(no_host));
    line.push(b',');
    line.extend_from_slice(user.as_deref().unwrap_or(b""));
    line.push(b',');
    line.extend_from_slice(domain.as_deref().unwrap_or(b""));
    line.push(b')');
}

// ---- hosts ----

/// Um host numérico sintetizado pelo `__nss_hostname_digits_dots`.
fn numeric_host(name: &[u8], addr: Vec<u8>) -> Host {
    Host {
        name: name.to_vec(),
        aliases: Vec::new(),
        addrs: vec![addr],
    }
}

/// `__nss_hostname_digits_dots` para `AF_INET` ou `AF_INET6` (sem `RES_USE_INET6`): `Some(resultado)`
/// quando o nome é tratado ali, `None` para seguir à consulta normal.
fn digits_dots(name: &[u8], af: Af) -> Option<Option<Host>> {
    let first = name.first().copied().unwrap_or(0);
    if !(first.is_ascii_digit() || first.is_ascii_hexdigit() || first == b':') {
        return None;
    }
    if first.is_ascii_digit() {
        let mut numeric = true;
        for &c in name {
            if !c.is_ascii_digit() && c != b'.' {
                numeric = false;
                break;
            }
        }
        if numeric && name.last() != Some(&b'.') {
            if af == Af::Inet {
                return Some(
                    inet_aton(name, true).map(|v| numeric_host(name, v.to_be_bytes().to_vec())),
                );
            }
            return Some(pton6(name).map(|a| numeric_host(name, a.to_vec())));
        }
    }
    if (first.is_ascii_hexdigit() && name.contains(&b':')) || first == b':' {
        if af == Af::Inet {
            return Some(None);
        }
        let all_hex = name
            .iter()
            .all(|c| c.is_ascii_hexdigit() || *c == b':' || *c == b'.');
        if all_hex && name.last() != Some(&b'.') {
            return Some(pton6(name).map(|a| numeric_host(name, a.to_vec())));
        }
    }
    None
}

fn gethostbyname2(env: &Env, name: &[u8], af: Af) -> Option<Host> {
    if let Some(r) = digits_dots(name, af) {
        return r;
    }
    let multi = host_conf_multi();
    env.conf
        .lookup("hosts", |_| files_host_by_name(name, af, multi))
}

fn gethostbyaddr(env: &Env, addr: &[u8]) -> Option<Host> {
    // O endereço IPv6 não especificado nunca é consultado.
    if addr.len() == 16 && addr.iter().all(|b| *b == 0) {
        return None;
    }
    env.conf.lookup("hosts", |_| files_host_by_addr(addr))
}

/// As linhas de `/etc/hosts` como o `gethostent` as entrega (família IPv4), impressas como o
/// `print_hosts`: a enumeração de `hosts` e de `ahosts*`.
fn list_hosts(env: &Env) -> impl FnOnce(&mut dyn Write) -> bool + '_ {
    let read = || Ok(read_hosts(Af::Inet, false)?.into_iter().map(Host::from).collect());
    listing(env, "hosts", read, show_host)
}

fn show_host(out: &mut dyn Write, h: &Host) {
    write_bytes(out, &format_host(h));
}

fn hosts_keys(env: &Env, out: &mut dyn Write, keys: &[Vec<u8>]) -> i32 {
    let find = |key: &[u8]| {
        if let Some(a) = pton6(key) {
            gethostbyaddr(env, &a)
        } else if let Some(a) = pton4(key) {
            gethostbyaddr(env, &a)
        } else {
            gethostbyname2(env, key, Af::Inet6).or_else(|| gethostbyname2(env, key, Af::Inet))
        }
    };
    run_db("hosts", out, keys, list_hosts(env), find, show_host)
}

// ---- ahosts (getaddrinfo) ----

/// Um resultado de `getaddrinfo`: família, endereço (4 ou 16 bytes), escopo IPv6 e nome canônico.
struct AddrInfo {
    v6: bool,
    addr: Vec<u8>,
    scope: u32,
    canon: Option<Vec<u8>>,
}

impl AddrInfo {
    fn plain(v6: bool, addr: Vec<u8>) -> AddrInfo {
        AddrInfo {
            v6,
            addr,
            scope: 0,
            canon: None,
        }
    }

    /// Um endereço numérico: a família sai do tamanho, e o nome canônico é a própria chave.
    fn numeric(addr: Vec<u8>, scope: u32, key: &[u8]) -> AddrInfo {
        AddrInfo {
            scope,
            canon: Some(key.to_vec()),
            ..AddrInfo::plain(addr.len() == 16, addr)
        }
    }

    /// Os endereços de um host, cada um passado por `convert`, com o nome do host como canônico do
    /// primeiro.
    fn of_host(h: &Host, v6: bool, convert: impl Fn(&Vec<u8>) -> Vec<u8>) -> Vec<AddrInfo> {
        let mut list: Vec<AddrInfo> = h.addrs.iter().map(|a| AddrInfo::plain(v6, convert(a))).collect();
        if let Some(first) = list.first_mut() {
            first.canon = Some(h.name.clone());
        }
        list
    }
}

/// A precedência de destino da tabela padrão do `getaddrinfo` (RFC 6724), para a ordenação.
fn precedence(a: &[u8; 16]) -> u32 {
    let zeros = |n: usize| a[..n].iter().all(|b| *b == 0);
    if zeros(15) && a[15] == 1 {
        50
    } else if zeros(10) && a[10] == 0xff && a[11] == 0xff {
        35
    } else if a[0] == 0x20 && a[1] == 0x02 {
        30
    } else if a[0] == 0x20 && a[1] == 0x01 && a[2] == 0 && a[3] == 0 {
        5
    } else if a[0] & 0xfe == 0xfc {
        3
    } else if zeros(12) || (a[0] == 0xfe && a[1] & 0xc0 == 0xc0) || (a[0] == 0x3f && a[1] == 0xfe) {
        1
    } else {
        40
    }
}

/// O índice de interface de um nome (só o `lo` existe no sandbox).
fn if_nametoindex(name: &[u8]) -> u32 {
    if name == b"lo" { 1 } else { 0 }
}

/// Destino alcançável pelo `connect` do `rfc3484_sort`: no sandbox só o loopback (127.0.0.0/8, `::1`
/// e o endereço não especificado) tem rota; o resto é "inutilizável" e vai pro fim da lista.
fn reachable(a: &[u8; 16]) -> bool {
    let zeros = |n: usize| a[..n].iter().all(|b| *b == 0);
    if zeros(15) && a[15] <= 1 {
        return true;
    }
    zeros(10) && a[10] == 0xff && a[11] == 0xff && (a[12] == 127 || a[12..].iter().all(|b| *b == 0))
}

/// O escopo de um destino no RFC 4007: multicast pelo nibble, link-local e loopback 2, site-local 5,
/// o resto 14; IPv4 mapeado vale 2 em 127/8 e 169.254/16 e 14 nos demais.
fn dest_scope(a: &[u8; 16]) -> u8 {
    if netdb::is_v4_mapped(a) {
        return if a[12] == 127 || (a[12] == 169 && a[13] == 254) {
            2
        } else {
            14
        };
    }
    if a[0] == 0xff {
        return a[1] & 0x0f;
    }
    let loopback = a[..15].iter().all(|b| *b == 0) && a[15] == 1;
    if loopback || (a[0] == 0xfe && a[1] & 0xc0 == 0x80) {
        2
    } else if a[0] == 0xfe && a[1] & 0xc0 == 0xc0 {
        5
    } else {
        14
    }
}

/// `getaddrinfo (key, NULL, hints)` com `AI_V4MAPPED | AI_CANONNAME` (e `AI_ADDRCONFIG` conforme o
/// `-A`), sobre `/etc/hosts` e endereços numéricos.
fn getaddrinfo(env: &Env, key: &[u8], af: Af) -> Option<Vec<AddrInfo>> {
    // Sem endereço configurado além do loopback, uma família fixa com AI_ADDRCONFIG não tem resposta.
    if env.addrconfig && af != Af::Unspec {
        return None;
    }
    // IPv4 numérico (inclusive "10.1" e "0x7f.1"); com família IPv6 vira endereço IPv4 mapeado.
    if let Some(v) = inet_aton(key, true) {
        let v4 = v.to_be_bytes();
        let addr = if af == Af::Inet6 { map_v4(&v4) } else { v4.to_vec() };
        return Some(vec![AddrInfo::numeric(addr, 0, key)]);
    }
    // IPv6 numérico, com `%escopo` opcional.
    if key.contains(&b':') {
        let (host, scope) = match key.iter().position(|b| *b == b'%') {
            Some(p) => (&key[..p], Some(&key[p + 1..])),
            None => (key, None),
        };
        if let Some(a) = pton6(host) {
            if af == Af::Inet {
                return None;
            }
            let scope_id = match scope {
                None => 0,
                Some(s) if !s.is_empty() && s.iter().all(|c| c.is_ascii_digit()) => {
                    let (v, _) = strtoull(s, 0, 10);
                    v as u32
                }
                Some(s) => {
                    let idx = if_nametoindex(s);
                    if idx == 0 {
                        return None;
                    }
                    idx
                }
            };
            return Some(vec![AddrInfo::numeric(a.to_vec(), scope_id, key)]);
        }
    }
    // Nome: /etc/hosts pelas fontes de `hosts`.
    let multi = host_conf_multi();
    let found: Option<Vec<AddrInfo>> = env.conf.lookup("hosts", |_| {
        let lines = match read_hosts(Af::Unspec, false) {
            Ok(l) => l,
            Err(_) => return (Status::Unavail, None),
        };
        let list = match af {
            Af::Unspec => {
                let mut matches = lines.iter().filter(|h| netdb::host_matches(h, key));
                matches.next().map(|first| {
                    let rest = if multi { matches.collect() } else { Vec::new() };
                    let mut list: Vec<AddrInfo> = std::iter::once(first)
                        .chain(rest)
                        .map(|h| AddrInfo::plain(h.af == Af::Inet6, h.addr.clone()))
                        .collect();
                    list[0].canon = Some(first.name.clone());
                    list
                })
            }
            Af::Inet => match files_host_by_name(key, Af::Inet, multi) {
                (Status::Success, Some(h)) => Some(AddrInfo::of_host(&h, false, |a| a.clone())),
                (st, _) => return (st, None),
            },
            Af::Inet6 => match files_host_by_name(key, Af::Inet6, multi) {
                (Status::Success, Some(mut h)) => {
                    // Com AI_V4MAPPED sem AI_ALL, os endereços mapeados que já vieram como IPv6 são
                    // descartados (e então a busca IPv4 nem é tentada).
                    h.addrs.retain(|a| !netdb::is_v4_mapped(a));
                    Some(AddrInfo::of_host(&h, true, |a| a.clone()))
                }
                _ => match files_host_by_name(key, Af::Inet, multi) {
                    (Status::Success, Some(h)) => Some(AddrInfo::of_host(&h, true, |a| map_v4(a))),
                    _ => None,
                },
            }
            .filter(|list| !list.is_empty()),
        };
        match list {
            Some(list) => (Status::Success, Some(list)),
            None => (Status::NotFound, None),
        }
    });
    let mut list = found?;
    if list.len() > 1 {
        // Ordenação do RFC 3484 (`rfc3484_sort`), estável nos empates: destino utilizável primeiro,
        // maior precedência, menor escopo. O nome canônico fica no primeiro da lista final.
        let canon = list.iter().find_map(|a| a.canon.clone());
        list.sort_by_key(|a| {
            let mut full = [0u8; 16];
            if a.v6 {
                full.copy_from_slice(&a.addr[..16]);
            } else {
                full[10] = 0xff;
                full[11] = 0xff;
                full[12..].copy_from_slice(&a.addr[..4]);
            }
            (
                !reachable(&full),
                std::cmp::Reverse(precedence(&full)),
                dest_scope(&full),
            )
        });
        for a in list.iter_mut() {
            a.canon = None;
        }
        list[0].canon = canon;
    }
    Some(list)
}

fn ahosts_keys(env: &Env, out: &mut dyn Write, af: Af, keys: &[Vec<u8>]) -> i32 {
    let find = |key: &[u8]| getaddrinfo(env, key, af);
    run_db(
        "ahosts",
        out,
        keys,
        list_hosts(env),
        find,
        |out, list: &Vec<AddrInfo>| {
            for ai in list {
                show_addrinfo(out, ai);
            }
        },
    )
}

/// As três linhas de um resultado do `getaddrinfo`, uma por tipo de socket, com o nome canônico
/// só na primeira.
fn show_addrinfo(out: &mut dyn Write, ai: &AddrInfo) {
    for (i, sock) in ["STREAM", "DGRAM", "RAW"].iter().enumerate() {
        let buf = format_ip(&ai.addr);
        let scope = if ai.v6 && ai.scope != 0 {
            format!("%{}", ai.scope)
        } else {
            String::new()
        };
        // `printf ("%s%-*s %-6s %s\n", buf, pad, scope, ...)` com pad = 15 - |buf| - |scope|.
        let pad = 15usize.saturating_sub(buf.len() + scope.len());
        let mut line = buf.into_bytes();
        line.extend_from_slice(scope.as_bytes());
        line.extend(std::iter::repeat_n(b' ', pad.saturating_sub(scope.len())));
        line.push(b' ');
        line.extend_from_slice(format!("{sock:<6}").as_bytes());
        line.push(b' ');
        if i == 0
            && let Some(c) = &ai.canon
        {
            line.extend_from_slice(c);
        }
        line.push(b'\n');
        write_bytes(out, &line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sysabi::Program;
    use sysabi::testkit::TestKit;

    const PASSWD: &str = "root:x:0:0:root:/root:/bin/bash\nbin:x:2:2:bin:/bin:/usr/sbin/nologin\n";
    const GROUP: &str = "root:x:0:\nstaff:x:50:bin,ana\nwheel:x:60:ana\n";
    const HOSTS: &str =
        "127.0.0.1\tlocalhost\n::1\tlocalhost ip6-localhost ip6-loopback\nff02::1\tip6-allnodes\n";

    fn kit() -> TestKit {
        TestKit::new()
            .programs([Program::bin("getent", main)])
            .file("/etc/passwd", PASSWD, 0o644)
            .file("/etc/group", GROUP, 0o644)
            .file("/etc/hosts", HOSTS, 0o644)
            .file(
                "/etc/nsswitch.conf",
                "passwd: files\ngroup: files\nhosts: files dns\n",
                0o644,
            )
    }

    #[test]
    fn account_databases() {
        let k = kit();
        let r = k.run(&["getent", "passwd", "root", "2", "nobody"], b"");
        assert_eq!(
            r.stdout_str(),
            "root:x:0:0:root:/root:/bin/bash\nbin:x:2:2:bin:/bin:/usr/sbin/nologin\n"
        );
        assert_eq!(r.code(), 2);
        let r = k.run(&["getent", "group", "50"], b"");
        assert_eq!(r.stdout_str(), "staff:x:50:bin,ana\n");
        let r = k.run(&["getent", "initgroups", "ana"], b"");
        assert_eq!(r.stdout_str(), format!("{:<21} 50 60\n", "ana"));
    }

    #[test]
    fn hosts_and_usage() {
        let k = kit();
        let r = k.run(&["getent", "hosts", "localhost"], b"");
        assert_eq!(
            r.stdout_str(),
            "::1             localhost ip6-localhost ip6-loopback\n"
        );
        let r = k.run(&["getent", "hosts", "1234"], b"");
        assert_eq!(r.stdout_str(), "0.0.4.210       1234\n");
        let r = k.run(&["getent", "ahosts", "127.0.0.1"], b"");
        assert_eq!(
            r.stdout_str(),
            "127.0.0.1       STREAM 127.0.0.1\n127.0.0.1       DGRAM  \n127.0.0.1       RAW    \n"
        );
        let r = k.run(&["getent"], b"");
        assert_eq!(
            (r.stderr_str().as_str(), r.code()),
            ("getent: wrong number of arguments\n", 1)
        );
        assert_eq!(
            r.stdout_str(),
            "Try `getent --help' or `getent --usage' for more information.\n"
        );
        let r = k.run(&["getent", "-Z", "passwd"], b"");
        assert_eq!(r.code(), 64);
    }
}
