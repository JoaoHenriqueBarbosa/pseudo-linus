//! Usuários e grupos do `/etc/passwd` e do `/etc/group` do pseudo-linus (o que a glibc faz com
//! `getpwuid`/`getgrgid` pelo NSS "files"), e as identidades do processo. Nada consulta o host.

use std::io;

use crate::proc;

/// Uma linha do `/etc/passwd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Passwd {
    pub name: String,
    pub passwd: String,
    pub uid: u32,
    pub gid: u32,
    pub gecos: String,
    pub dir: String,
    pub shell: String,
}

/// Uma linha do `/etc/group`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub passwd: String,
    pub gid: u32,
    pub members: Vec<String>,
}

fn lines(path: &str) -> Vec<Vec<String>> {
    let Ok(data) = crate::fs::read(path) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&data)
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split(':').map(str::to_string).collect())
        .collect()
}

fn parse_passwd(row: &[String]) -> Option<Passwd> {
    let f = |i: usize| row.get(i).cloned().unwrap_or_default();
    Some(Passwd {
        name: row.first()?.clone(),
        passwd: f(1),
        uid: row.get(2)?.parse().ok()?,
        gid: row.get(3)?.parse().ok()?,
        gecos: f(4),
        dir: f(5),
        shell: f(6),
    })
}

fn parse_group(row: &[String]) -> Option<Group> {
    Some(Group {
        name: row.first()?.clone(),
        passwd: row.get(1).cloned().unwrap_or_default(),
        gid: row.get(2)?.parse().ok()?,
        members: row.get(3).map(|m| m.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect()).unwrap_or_default(),
    })
}

/// Todas as entradas do `/etc/passwd`, na ordem do arquivo.
pub fn all_passwd() -> Vec<Passwd> {
    lines("/etc/passwd").iter().filter_map(|r| parse_passwd(r)).collect()
}

/// Todas as entradas do `/etc/group`, na ordem do arquivo.
pub fn all_groups() -> Vec<Group> {
    lines("/etc/group").iter().filter_map(|r| parse_group(r)).collect()
}

/// `getpwuid(3)`.
pub fn passwd_by_uid(uid: u32) -> Option<Passwd> {
    all_passwd().into_iter().find(|p| p.uid == uid)
}

/// `getpwnam(3)`.
pub fn passwd_by_name(name: &str) -> Option<Passwd> {
    all_passwd().into_iter().find(|p| p.name == name)
}

/// `getgrgid(3)`.
pub fn group_by_gid(gid: u32) -> Option<Group> {
    all_groups().into_iter().find(|g| g.gid == gid)
}

/// `getgrnam(3)`.
pub fn group_by_name(name: &str) -> Option<Group> {
    all_groups().into_iter().find(|g| g.name == name)
}

/// `getgrouplist(3)`: o grupo primário e os grupos em que o nome aparece como membro.
pub fn group_list(name: &str, primary: u32) -> Vec<u32> {
    let mut out = vec![primary];
    for g in all_groups() {
        if g.members.iter().any(|m| m == name) && !out.contains(&g.gid) {
            out.push(g.gid);
        }
    }
    out
}

fn not_found(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("{what} not found"))
}

pub fn uid2usr(uid: u32) -> io::Result<String> {
    passwd_by_uid(uid).map(|p| p.name).ok_or_else(|| not_found("uid"))
}

pub fn gid2grp(gid: u32) -> io::Result<String> {
    group_by_gid(gid).map(|g| g.name).ok_or_else(|| not_found("gid"))
}

pub fn usr2uid(name: &str) -> io::Result<u32> {
    passwd_by_name(name).map(|p| p.uid).ok_or_else(|| not_found("user"))
}

pub fn grp2gid(name: &str) -> io::Result<u32> {
    group_by_name(name).map(|g| g.gid).ok_or_else(|| not_found("group"))
}

pub fn getuid() -> u32 {
    proc::sys().getuid()
}

pub fn geteuid() -> u32 {
    proc::sys().geteuid()
}

pub fn getgid() -> u32 {
    proc::sys().getgid()
}

pub fn getegid() -> u32 {
    proc::sys().getegid()
}

/// `getgroups(2)`: grupos suplementares do processo.
pub fn getgroups() -> Vec<u32> {
    proc::sys().getgroups()
}
