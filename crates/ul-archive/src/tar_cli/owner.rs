//! Nomes de usuário e grupo pelo `/etc/passwd` e `/etc/group` do sandbox (o que o NSS "files" do glibc
//! faria), com cache.

use std::collections::HashMap;

use sysabi::sys;

#[derive(Default)]
pub struct Db {
    loaded: bool,
    users: Vec<(Vec<u8>, u32, u32)>,
    groups: Vec<(Vec<u8>, u32)>,
    uname_cache: HashMap<u32, Option<Vec<u8>>>,
    gname_cache: HashMap<u32, Option<Vec<u8>>>,
}

fn parse_id(s: &[u8]) -> Option<u32> {
    std::str::from_utf8(s).ok()?.parse().ok()
}

impl Db {
    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if sys::try_current().is_none() {
            return;
        }
        if let Ok(p) = sys::read_file(b"/etc/passwd") {
            for line in p.split(|&c| c == b'\n') {
                let f: Vec<&[u8]> = line.split(|&c| c == b':').collect();
                if f.len() >= 4
                    && let (Some(uid), Some(gid)) = (parse_id(f[2]), parse_id(f[3]))
                {
                    self.users.push((f[0].to_vec(), uid, gid));
                }
            }
        }
        if let Ok(g) = sys::read_file(b"/etc/group") {
            for line in g.split(|&c| c == b'\n') {
                let f: Vec<&[u8]> = line.split(|&c| c == b':').collect();
                if f.len() >= 3
                    && let Some(gid) = parse_id(f[2])
                {
                    self.groups.push((f[0].to_vec(), gid));
                }
            }
        }
    }

    /// Nome do usuário de um uid (primeira linha que bate, como o getpwuid).
    pub fn uname(&mut self, uid: u32) -> Option<Vec<u8>> {
        if let Some(v) = self.uname_cache.get(&uid) {
            return v.clone();
        }
        self.load();
        let v = self.users.iter().find(|u| u.1 == uid).map(|u| u.0.clone());
        self.uname_cache.insert(uid, v.clone());
        v
    }

    pub fn gname(&mut self, gid: u32) -> Option<Vec<u8>> {
        if let Some(v) = self.gname_cache.get(&gid) {
            return v.clone();
        }
        self.load();
        let v = self.groups.iter().find(|g| g.1 == gid).map(|g| g.0.clone());
        self.gname_cache.insert(gid, v.clone());
        v
    }

    /// uid de um nome de usuário.
    pub fn uid_of(&mut self, name: &[u8]) -> Option<u32> {
        self.load();
        self.users.iter().find(|u| u.0 == name).map(|u| u.1)
    }

    /// Grupo primário de um usuário.
    pub fn primary_gid_of(&mut self, name: &[u8]) -> Option<u32> {
        self.load();
        self.users.iter().find(|u| u.0 == name).map(|u| u.2)
    }

    pub fn gid_of(&mut self, name: &[u8]) -> Option<u32> {
        self.load();
        self.groups.iter().find(|g| g.0 == name).map(|g| g.1)
    }
}
