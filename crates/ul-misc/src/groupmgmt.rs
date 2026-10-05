//! `groupadd`, `groupdel`, `groupmod` e `grpck -r` do shadow 4.17 (Debian 13).
//!
//! Editam `/etc/group` e `/etc/gshadow` (sob `-R`/`-P`, com o prefixo). O travamento do original é
//! ignorado (sandbox). `grpck` só tem o modo de leitura: sem `-r` recusa como o original sem lock.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, OFlags};

use crate::util::io::{self, File};

pub(crate) type Spec = &'static [(u8, &'static str, bool)];

pub(crate) struct Opts {
    pub(crate) vals: Vec<(u8, Option<Vec<u8>>)>,
    pub(crate) rest: Vec<Vec<u8>>,
}

impl Opts {
    pub(crate) fn has(&self, c: u8) -> bool {
        self.vals.iter().any(|(k, _)| *k == c)
    }
    pub(crate) fn get(&self, c: u8) -> Option<Vec<u8>> {
        self.vals
            .iter()
            .rev()
            .find(|(k, _)| *k == c)
            .and_then(|(_, v)| v.clone())
    }
}

/// getopt_long com permutação, abreviação única de opção longa e as mensagens da glibc.
pub(crate) fn parse(prog: &str, argv: &[Vec<u8>], spec: Spec) -> Option<Opts> {
    let mut vals = Vec::new();
    let mut rest = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        i += 1;
        if a.as_slice() == b"--" {
            rest.extend(argv[i..].iter().cloned());
            break;
        }
        if a.starts_with(b"--") {
            let body = &a[2..];
            let (name, val) = match body.iter().position(|b| *b == b'=') {
                Some(p) => (&body[..p], Some(body[p + 1..].to_vec())),
                None => (body, None),
            };
            let ns = io::lossy(name);
            let found = match spec.iter().find(|s| s.1 == ns) {
                Some(s) => s,
                None => {
                    let c: Vec<_> = spec.iter().filter(|s| s.1.starts_with(&ns)).collect();
                    if c.len() == 1 {
                        c[0]
                    } else if c.is_empty() {
                        io::eprint(format!("{prog}: unrecognized option '--{ns}'\n"));
                        return None;
                    } else {
                        let poss: Vec<String> = c.iter().map(|s| format!("'--{}'", s.1)).collect();
                        io::eprint(format!(
                            "{prog}: option '--{ns}' is ambiguous; possibilities: {}\n",
                            poss.join(" ")
                        ));
                        return None;
                    }
                }
            };
            if found.2 {
                let v = match val {
                    Some(v) => v,
                    None => {
                        if i < argv.len() {
                            i += 1;
                            argv[i - 1].clone()
                        } else {
                            io::eprint(format!(
                                "{prog}: option '--{}' requires an argument\n",
                                found.1
                            ));
                            return None;
                        }
                    }
                };
                vals.push((found.0, Some(v)));
            } else {
                if val.is_some() {
                    io::eprint(format!(
                        "{prog}: option '--{}' doesn't allow an argument\n",
                        found.1
                    ));
                    return None;
                }
                vals.push((found.0, None));
            }
        } else if a.len() > 1 && a[0] == b'-' {
            let mut j = 1;
            while j < a.len() {
                let c = a[j];
                j += 1;
                let Some(s) = spec.iter().find(|s| s.0 == c) else {
                    io::eprint(format!("{prog}: invalid option -- '{}'\n", c as char));
                    return None;
                };
                if s.2 {
                    let v = if j < a.len() {
                        a[j..].to_vec()
                    } else if i < argv.len() {
                        i += 1;
                        argv[i - 1].clone()
                    } else {
                        io::eprint(format!(
                            "{prog}: option requires an argument -- '{}'\n",
                            c as char
                        ));
                        return None;
                    };
                    vals.push((c, Some(v)));
                    break;
                }
                vals.push((c, None));
            }
        } else {
            rest.push(a.clone());
        }
    }
    Some(Opts { vals, rest })
}

pub(crate) fn usage(text: &str, code: i32) -> i32 {
    if code == 0 {
        let _ = io::stdout().write_all(text.as_bytes());
    } else {
        io::eprint(text.to_string());
    }
    code
}

pub(crate) fn join(prefix: &[u8], p: &str) -> Vec<u8> {
    let mut v = prefix.to_vec();
    while v.last() == Some(&b'/') {
        v.pop();
    }
    v.extend_from_slice(p.as_bytes());
    v
}

pub(crate) fn read_lines(path: &[u8]) -> Result<Vec<Vec<u8>>, Errno> {
    let d = io::read_path(path)?;
    let mut v: Vec<Vec<u8>> = d.split(|b| *b == b'\n').map(|l| l.to_vec()).collect();
    if v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    Ok(v)
}

pub(crate) fn write_lines(path: &[u8], lines: &[Vec<u8>]) -> bool {
    let mut data = Vec::new();
    for l in lines {
        data.extend_from_slice(l);
        data.push(b'\n');
    }
    match File::open_with(path, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, 0o644) {
        Ok(mut f) => f.write_all(&data).is_ok(),
        Err(_) => false,
    }
}

pub(crate) fn fields(line: &[u8]) -> Vec<&[u8]> {
    line.split(|b| *b == b':').collect()
}

pub(crate) fn parse_id(s: &[u8]) -> Option<u64> {
    let t = std::str::from_utf8(s).ok()?;
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v: u64 = t.parse().ok()?;
    (v <= u64::from(u32::MAX)).then_some(v)
}

/// `is_valid_name` do shadow (modo estrito) mais o limite de 32 bytes dos grupos.
pub(crate) fn valid_name(n: &[u8]) -> bool {
    if n.is_empty() || n.len() > 32 {
        return false;
    }
    if !(n[0].is_ascii_lowercase() || n[0] == b'_') {
        return false;
    }
    for (i, c) in n.iter().enumerate().skip(1) {
        let ok = c.is_ascii_lowercase()
            || c.is_ascii_digit()
            || *c == b'_'
            || *c == b'-'
            || (*c == b'$' && i == n.len() - 1);
        if !ok {
            return false;
        }
    }
    true
}

pub(crate) fn gid_of(line: &[u8]) -> Option<u64> {
    fields(line).get(2).and_then(|f| parse_id(f))
}

pub(crate) fn name_eq(line: &[u8], name: &[u8]) -> bool {
    fields(line).first().is_some_and(|f| *f == name)
}

pub(crate) fn is_data(line: &[u8]) -> bool {
    !line.is_empty() && line[0] != b'#'
}

// ---------------------------------------------------------------- groupadd

const GROUPADD_USAGE: &str = "Usage: groupadd [options] GROUP\n\nOptions:\n  -f, --force                   exit successfully if the group already exists,\n                                and cancel -g if the GID is already used\n  -g, --gid GID                 use GID for the new group\n  -h, --help                    display this help message and exit\n  -K, --key KEY=VALUE           override /etc/login.defs defaults\n  -o, --non-unique              allow to create groups with duplicate\n                                (non-unique) GID\n  -p, --password PASSWORD       use this encrypted password for the new group\n  -r, --system                  create a system account\n  -R, --root CHROOT_DIR         directory to chroot into\n  -P, --prefix PREFIX_DIR       directory prefix\n\n";

pub fn groupadd_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| groupadd(args))
}

fn groupadd(args: &[OsString]) -> i32 {
    const P: &str = "groupadd";
    let argv = io::args_bytes(args);
    let spec: Spec = &[
        (b'f', "force", false),
        (b'g', "gid", true),
        (b'h', "help", false),
        (b'K', "key", true),
        (b'o', "non-unique", false),
        (b'p', "password", true),
        (b'r', "system", false),
        (b'R', "root", true),
        (b'P', "prefix", true),
    ];
    let Some(o) = parse(P, &argv, spec) else {
        return usage(GROUPADD_USAGE, 2);
    };
    let mut gid_min = 1000u64;
    let mut gid_max = 60000u64;
    let mut sys_min = 100u64;
    let mut sys_max = 999u64;
    let mut gid: Option<u64> = None;
    let force = o.has(b'f');
    let system = o.has(b'r');
    for (k, v) in &o.vals {
        match *k {
            b'h' => return usage(GROUPADD_USAGE, 0),
            b'g' => match parse_id(v.as_deref().unwrap_or(b"")) {
                Some(g) => gid = Some(g),
                None => {
                    io::eprint(format!(
                        "{P}: invalid group ID '{}'\n",
                        io::lossy(v.as_deref().unwrap_or(b""))
                    ));
                    return 3;
                }
            },
            b'K' => {
                let kv = v.as_deref().unwrap_or(b"");
                let Some(p) = kv.iter().position(|b| *b == b'=') else {
                    io::eprint(format!("{P}: -K requires KEY=VALUE\n"));
                    return 1;
                };
                let key = &kv[..p];
                let val = parse_id(&kv[p + 1..]);
                let slot = match key {
                    b"GID_MIN" => &mut gid_min,
                    b"GID_MAX" => &mut gid_max,
                    b"SYS_GID_MIN" => &mut sys_min,
                    b"SYS_GID_MAX" => &mut sys_max,
                    _ => continue,
                };
                match val {
                    Some(n) => *slot = n,
                    None => {
                        io::eprint(format!("{P}: -K requires KEY=VALUE\n"));
                        return 1;
                    }
                }
            }
            _ => {}
        }
    }
    if o.rest.len() != 1 {
        return usage(GROUPADD_USAGE, 2);
    }
    let name = o.rest[0].clone();
    if !valid_name(&name) {
        io::eprint(format!("{P}: '{}' is not a valid group name\n", io::lossy(&name)));
        return 3;
    }
    if o.has(b'o') && gid.is_none() {
        return usage(GROUPADD_USAGE, 2);
    }
    let prefix = o.get(b'P').or_else(|| o.get(b'R')).unwrap_or_default();
    let gpath = join(&prefix, "/etc/group");
    let spath = join(&prefix, "/etc/gshadow");
    let mut group = match read_lines(&gpath) {
        Ok(g) => g,
        Err(_) => {
            io::eprint(format!("{P}: cannot open {}\n", io::lossy(&gpath)));
            return 10;
        }
    };
    let mut gshadow = read_lines(&spath).ok();
    if group.iter().any(|l| is_data(l) && name_eq(l, &name)) {
        if force {
            return 0;
        }
        io::eprint(format!("{P}: group '{}' already exists\n", io::lossy(&name)));
        return 9;
    }
    let used: Vec<u64> = group.iter().filter(|l| is_data(l)).filter_map(|l| gid_of(l)).collect();
    if let Some(g) = gid {
        if !o.has(b'o') && used.contains(&g) {
            if force {
                gid = None;
            } else {
                io::eprint(format!("{P}: GID '{g}' already exists\n"));
                return 4;
            }
        }
    }
    let gid = match gid {
        Some(g) => g,
        None => {
            let found = if system {
                (sys_min..=sys_max).rev().find(|g| !used.contains(g))
            } else {
                let top = used.iter().copied().filter(|g| *g >= gid_min && *g <= gid_max).max();
                match top {
                    Some(t) if t < gid_max => Some(t + 1),
                    Some(_) => (gid_min..=gid_max).find(|g| !used.contains(g)),
                    None => Some(gid_min),
                }
            };
            match found {
                Some(g) => g,
                None => {
                    io::eprint(format!("{P}: cannot find unused GID\n"));
                    return 4;
                }
            }
        }
    };
    let pw = o.get(b'p').map(|v| io::lossy(&v));
    let nm = io::lossy(&name);
    if let Some(sh) = gshadow.as_mut() {
        group.push(format!("{nm}:x:{gid}:").into_bytes());
        sh.push(format!("{nm}:{}::", pw.as_deref().unwrap_or("!")).into_bytes());
    } else {
        group.push(format!("{nm}:{}:{gid}:", pw.as_deref().unwrap_or("!")).into_bytes());
    }
    if !write_lines(&gpath, &group) {
        io::eprint(format!("{P}: failure while writing changes to /etc/group\n"));
        return 10;
    }
    if let Some(sh) = gshadow {
        if !write_lines(&spath, &sh) {
            io::eprint(format!("{P}: failure while writing changes to /etc/gshadow\n"));
            return 10;
        }
    }
    0
}

// ---------------------------------------------------------------- groupdel

const GROUPDEL_USAGE: &str = "Usage: groupdel [options] GROUP\n\nOptions:\n  -f, --force                   delete group even if it is the primary group of a user\n  -h, --help                    display this help message and exit\n  -R, --root CHROOT_DIR         directory to chroot into\n  -P, --prefix PREFIX_DIR       prefix directory where are located the /etc/* files\n\n";

pub fn groupdel_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| groupdel(args))
}

fn groupdel(args: &[OsString]) -> i32 {
    const P: &str = "groupdel";
    let argv = io::args_bytes(args);
    let spec: Spec = &[
        (b'f', "force", false),
        (b'h', "help", false),
        (b'R', "root", true),
        (b'P', "prefix", true),
    ];
    let Some(o) = parse(P, &argv, spec) else {
        return usage(GROUPDEL_USAGE, 2);
    };
    if o.has(b'h') {
        return usage(GROUPDEL_USAGE, 0);
    }
    if o.rest.len() != 1 {
        return usage(GROUPDEL_USAGE, 2);
    }
    let name = o.rest[0].clone();
    let prefix = o.get(b'P').or_else(|| o.get(b'R')).unwrap_or_default();
    let gpath = join(&prefix, "/etc/group");
    let spath = join(&prefix, "/etc/gshadow");
    let group = match read_lines(&gpath) {
        Ok(g) => g,
        Err(_) => {
            io::eprint(format!("{P}: cannot open {}\n", io::lossy(&gpath)));
            return 10;
        }
    };
    let Some(entry) = group.iter().find(|l| is_data(l) && name_eq(l, &name)) else {
        io::eprint(format!("{P}: group '{}' does not exist\n", io::lossy(&name)));
        return 6;
    };
    if !o.has(b'f') {
        if let Some(g) = gid_of(entry) {
            let pw = read_lines(&join(&prefix, "/etc/passwd")).unwrap_or_default();
            for l in pw.iter().filter(|l| is_data(l)) {
                let f = fields(l);
                if f.len() > 3 && parse_id(f[3]) == Some(g) {
                    io::eprint(format!(
                        "{P}: cannot remove the primary group of user '{}'\n",
                        io::lossy(f[0])
                    ));
                    return 8;
                }
            }
        }
    }
    let new_group: Vec<Vec<u8>> = group
        .iter()
        .filter(|l| !(is_data(l) && name_eq(l, &name)))
        .cloned()
        .collect();
    if !write_lines(&gpath, &new_group) {
        io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&gpath)));
        return 10;
    }
    if let Ok(sh) = read_lines(&spath) {
        let ns: Vec<Vec<u8>> = sh
            .iter()
            .filter(|l| !(is_data(l) && name_eq(l, &name)))
            .cloned()
            .collect();
        if !write_lines(&spath, &ns) {
            io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&spath)));
            return 10;
        }
    }
    0
}

// ---------------------------------------------------------------- groupmod

const GROUPMOD_USAGE: &str = "Usage: groupmod [options] GROUP\n\nOptions:\n  -a, --append                  append the users mentioned by -U option to the group \n                                without removing existing user members\n  -g, --gid GID                 change the group ID to GID\n  -h, --help                    display this help message and exit\n  -n, --new-name NEW_GROUP      change the name to NEW_GROUP\n  -o, --non-unique              allow to use a duplicate (non-unique) GID\n  -p, --password PASSWORD       change the password to this (encrypted)\n                                PASSWORD\n  -R, --root CHROOT_DIR         directory to chroot into\n  -P, --prefix PREFIX_DIR       prefix directory where are located the /etc/* files\n  -U, --users USERS             list of user members of this group\n\n";

pub fn groupmod_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| groupmod(args))
}

fn groupmod(args: &[OsString]) -> i32 {
    const P: &str = "groupmod";
    let argv = io::args_bytes(args);
    let spec: Spec = &[
        (b'a', "append", false),
        (b'g', "gid", true),
        (b'h', "help", false),
        (b'n', "new-name", true),
        (b'o', "non-unique", false),
        (b'p', "password", true),
        (b'R', "root", true),
        (b'P', "prefix", true),
        (b'U', "users", true),
    ];
    let Some(o) = parse(P, &argv, spec) else {
        return usage(GROUPMOD_USAGE, 2);
    };
    let mut new_gid: Option<u64> = None;
    for (k, v) in &o.vals {
        match *k {
            b'h' => return usage(GROUPMOD_USAGE, 0),
            b'g' => match parse_id(v.as_deref().unwrap_or(b"")) {
                Some(g) => new_gid = Some(g),
                None => {
                    io::eprint(format!(
                        "{P}: invalid group ID '{}'\n",
                        io::lossy(v.as_deref().unwrap_or(b""))
                    ));
                    return 3;
                }
            },
            _ => {}
        }
    }
    if o.rest.len() != 1 {
        return usage(GROUPMOD_USAGE, 2);
    }
    if o.has(b'a') && !o.has(b'U') {
        io::eprint(format!("{P}: {} flag is only allowed with the {} flag\n", "-a", "-U"));
        return usage(GROUPMOD_USAGE, 2);
    }
    let name = o.rest[0].clone();
    let prefix = o.get(b'P').or_else(|| o.get(b'R')).unwrap_or_default();
    let gpath = join(&prefix, "/etc/group");
    let spath = join(&prefix, "/etc/gshadow");
    let mut group = match read_lines(&gpath) {
        Ok(g) => g,
        Err(_) => {
            io::eprint(format!("{P}: cannot open {}\n", io::lossy(&gpath)));
            return 10;
        }
    };
    let mut gshadow = read_lines(&spath).ok();
    let Some(idx) = group.iter().position(|l| is_data(l) && name_eq(l, &name)) else {
        io::eprint(format!("{P}: group '{}' does not exist\n", io::lossy(&name)));
        return 6;
    };
    let new_name = o.get(b'n');
    if let Some(n) = &new_name {
        if !valid_name(n) {
            io::eprint(format!("{P}: '{}' is not a valid group name\n", io::lossy(n)));
            return 3;
        }
        if n != &name && group.iter().any(|l| is_data(l) && name_eq(l, n)) {
            io::eprint(format!("{P}: group '{}' already exists\n", io::lossy(n)));
            return 9;
        }
    }
    let cur_gid = gid_of(&group[idx]);
    if let Some(g) = new_gid {
        if !o.has(b'o') && cur_gid != Some(g) && group.iter().any(|l| is_data(l) && gid_of(l) == Some(g)) {
            io::eprint(format!("{P}: GID '{g}' already exists\n"));
            return 4;
        }
    }
    let users_arg = o.get(b'U');
    let mut users: Vec<Vec<u8>> = Vec::new();
    if let Some(u) = &users_arg {
        if !u.is_empty() {
            let pw = read_lines(&join(&prefix, "/etc/passwd")).unwrap_or_default();
            for n in u.split(|b| *b == b',') {
                if !pw.iter().any(|l| is_data(l) && name_eq(l, n)) {
                    io::eprint(format!("{P}: user '{}' does not exist\n", io::lossy(n)));
                    return 6;
                }
                users.push(n.to_vec());
            }
        }
    }
    if !(new_gid.is_some() || new_name.is_some() || o.has(b'p') || users_arg.is_some()) {
        return 0;
    }
    let merge = |old: &[u8]| -> Vec<u8> {
        let mut list: Vec<Vec<u8>> = if o.has(b'a') {
            old.split(|b| *b == b',').filter(|x| !x.is_empty()).map(<[u8]>::to_vec).collect()
        } else {
            Vec::new()
        };
        for u in &users {
            if !list.contains(u) {
                list.push(u.clone());
            }
        }
        list.join(&b','.to_owned())
    };
    let pw = o.get(b'p');
    let edit = |line: &[u8], shadow: bool, has_shadow: bool| -> Vec<u8> {
        let f = fields(line);
        let mut v: Vec<Vec<u8>> = f.iter().map(|x| x.to_vec()).collect();
        while v.len() < 4 {
            v.push(Vec::new());
        }
        if let Some(n) = &new_name {
            v[0] = n.clone();
        }
        if let Some(p) = &pw {
            if shadow || !has_shadow {
                v[1] = p.clone();
            }
        }
        if !shadow {
            if let Some(g) = new_gid {
                v[2] = g.to_string().into_bytes();
            }
        }
        if users_arg.is_some() {
            v[3] = merge(&v[3]);
        }
        v.join(&b':'.to_owned())
    };
    let has_shadow = gshadow.is_some();
    group[idx] = edit(&group[idx].clone(), false, has_shadow);
    if let Some(sh) = gshadow.as_mut() {
        if let Some(i) = sh.iter().position(|l| is_data(l) && name_eq(l, &name)) {
            sh[i] = edit(&sh[i].clone(), true, true);
        }
    }
    if !write_lines(&gpath, &group) {
        io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&gpath)));
        return 10;
    }
    if let Some(sh) = gshadow {
        if !write_lines(&spath, &sh) {
            io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&spath)));
            return 10;
        }
    }
    0
}

// ---------------------------------------------------------------- grpck

const GRPCK_USAGE: &str = "Usage: grpck [options] [group [gshadow]]\n\nOptions:\n  -h, --help                    display this help message and exit\n  -q, --quiet                   report errors only\n  -r, --read-only               display errors and warnings\n                                but do not change files\n  -R, --root CHROOT_DIR         directory to chroot into\n  -s, --sort                    sort entries by GID\n\n";

pub fn grpck_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| grpck(args))
}

fn grpck(args: &[OsString]) -> i32 {
    const P: &str = "grpck";
    let argv = io::args_bytes(args);
    let spec: Spec = &[
        (b'h', "help", false),
        (b'q', "quiet", false),
        (b'r', "read-only", false),
        (b'R', "root", true),
        (b's', "sort", false),
    ];
    let Some(o) = parse(P, &argv, spec) else {
        return usage(GRPCK_USAGE, 1);
    };
    if o.has(b'h') {
        return usage(GRPCK_USAGE, 0);
    }
    if o.rest.len() > 2 {
        return usage(GRPCK_USAGE, 1);
    }
    let prefix = o.get(b'R').unwrap_or_default();
    if !o.has(b'r') {
        io::eprint(format!("{P}: cannot lock /etc/group; try again later.\n"));
        return 4;
    }
    let gpath = o.rest.first().cloned().unwrap_or_else(|| join(&prefix, "/etc/group"));
    let spath = match o.rest.get(1) {
        Some(s) => Some(s.clone()),
        None if o.rest.is_empty() => Some(join(&prefix, "/etc/gshadow")),
        None => None,
    };
    let group = match read_lines(&gpath) {
        Ok(g) => g,
        Err(_) => {
            io::eprint(format!("{P}: cannot open file {}\n", io::lossy(&gpath)));
            return 3;
        }
    };
    let gshadow = match &spath {
        Some(p) => match read_lines(p) {
            Ok(s) => Some(s),
            Err(Errno::ENOENT) if o.rest.is_empty() => None,
            Err(_) => {
                io::eprint(format!("{P}: cannot open file {}\n", io::lossy(p)));
                return 3;
            }
        },
        None => None,
    };
    let passwd = read_lines(&join(&prefix, "/etc/passwd")).unwrap_or_default();
    let users: Vec<&[u8]> = passwd
        .iter()
        .filter(|l| is_data(l))
        .filter_map(|l| fields(l).first().copied())
        .collect();

    let mut errors = 0;
    let mut seen: Vec<Vec<u8>> = Vec::new();
    for line in group.iter().filter(|l| is_data(l)) {
        let shown = io::lossy(line);
        let f = fields(line);
        if f.len() != 4 {
            errors += 1;
            io::eprint(format!("invalid group file entry\ndelete line '{shown}'? No\n"));
            continue;
        }
        if seen.iter().any(|n| n.as_slice() == f[0]) {
            errors += 1;
            io::eprint(format!("duplicate group entry\ndelete line '{shown}'? No\n"));
            continue;
        }
        seen.push(f[0].to_vec());
        if !valid_name(f[0]) {
            errors += 1;
            io::eprint(format!("invalid group name '{}'\n", io::lossy(f[0])));
        }
        for m in f[3].split(|b| *b == b',').filter(|m| !m.is_empty()) {
            if !users.contains(&m) {
                errors += 1;
                io::eprint(format!(
                    "group '{}': user '{}' does not exist\ndelete member '{}'? No\n",
                    io::lossy(f[0]),
                    io::lossy(m),
                    io::lossy(m)
                ));
            }
        }
    }
    if let Some(sh) = &gshadow {
        let mut seen_s: Vec<Vec<u8>> = Vec::new();
        for line in sh.iter().filter(|l| is_data(l)) {
            let shown = io::lossy(line);
            let f = fields(line);
            if f.len() != 4 {
                errors += 1;
                io::eprint(format!(
                    "invalid shadow group file entry\ndelete line '{shown}'? No\n"
                ));
                continue;
            }
            if seen_s.iter().any(|n| n.as_slice() == f[0]) {
                errors += 1;
                io::eprint(format!(
                    "duplicate shadow group entry\ndelete line '{shown}'? No\n"
                ));
                continue;
            }
            seen_s.push(f[0].to_vec());
            if !group.iter().any(|l| is_data(l) && name_eq(l, f[0])) {
                errors += 1;
                io::eprint(format!(
                    "no matching group file entry in {}\ndelete line '{shown}'? No\n",
                    io::lossy(&gpath)
                ));
            }
        }
    }
    if errors > 0 {
        io::eprint(format!("{P}: no changes\n"));
        2
    } else {
        0
    }
}
