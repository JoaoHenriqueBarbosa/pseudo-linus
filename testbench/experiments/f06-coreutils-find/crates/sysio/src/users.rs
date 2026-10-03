//! Usuários e grupos lidos do `/etc/passwd` e `/etc/group` do VFS (o que a glibc faria com
//! `getpwuid`/`getgrgid` via NSS "files"), em vez de `libc` consultando o host.

use std::io;

fn table(path: &str) -> Vec<(String, u32)> {
    let Ok(text) = crate::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split(':');
            let name = parts.next()?.to_string();
            let _passwd = parts.next()?;
            let id: u32 = parts.next()?.parse().ok()?;
            Some((name, id))
        })
        .collect()
}

fn not_found(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("{what} not found"))
}

pub fn uid2usr(uid: u32) -> io::Result<String> {
    table("/etc/passwd").into_iter().find(|(_, id)| *id == uid).map(|(n, _)| n).ok_or_else(|| not_found("uid"))
}

pub fn gid2grp(gid: u32) -> io::Result<String> {
    table("/etc/group").into_iter().find(|(_, id)| *id == gid).map(|(n, _)| n).ok_or_else(|| not_found("gid"))
}

pub fn usr2uid(name: &str) -> io::Result<u32> {
    table("/etc/passwd").into_iter().find(|(n, _)| n == name).map(|(_, id)| id).ok_or_else(|| not_found("user"))
}

pub fn grp2gid(name: &str) -> io::Result<u32> {
    table("/etc/group").into_iter().find(|(n, _)| n == name).map(|(_, id)| id).ok_or_else(|| not_found("group"))
}

/// Usuário efetivo do processo (todo pseudo-processo da bancada é root).
pub fn geteuid() -> u32 {
    0
}

pub fn getegid() -> u32 {
    0
}
