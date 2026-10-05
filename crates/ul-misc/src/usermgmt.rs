//! `userdel` e `usermod` do shadow 4.17 (Debian 13).
//!
//! Editam `/etc/passwd`, `/etc/shadow`, `/etc/group` e `/etc/gshadow` (sob `-R`/`-P`, com o
//! prefixo), reaproveitando o parser de opções e os helpers de `groupmgmt`. O travamento do
//! original é ignorado (sandbox). Ficam de fora: varredura de processos do usuário (`userdel`
//! "currently used by process"), SELinux, subuid/subgid, `chown` recursivo do `-u` e a troca do
//! nome do spool de e-mail no `-l`.

use std::ffi::OsString;

use sysabi::{AtFlags, Clock, Fd, FileType, RenameFlags, sys};

use crate::groupmgmt::{
    Spec, fields, is_data, join, name_eq, parse, parse_id, read_lines, usage, valid_name,
    write_backup, write_lines,
};
use crate::util::io;

/// Campo livre de passwd/shadow: sem `:` nem quebra de linha.
pub(crate) fn valid_field(s: &[u8]) -> bool {
    !s.iter().any(|b| *b == b':' || *b == b'\n')
}

fn owned(line: &[u8], min: usize) -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = fields(line).iter().map(|x| x.to_vec()).collect();
    while v.len() < min {
        v.push(Vec::new());
    }
    v
}

fn join_fields(v: &[Vec<u8>]) -> Vec<u8> {
    v.join(&b':')
}

fn members(f: &[u8]) -> Vec<Vec<u8>> {
    f.split(|b| *b == b',').filter(|x| !x.is_empty()).map(<[u8]>::to_vec).collect()
}

fn join_members(v: &[Vec<u8>]) -> Vec<u8> {
    v.join(&b',')
}

fn rm_rf(path: &[u8]) -> bool {
    let Ok(st) = sys::lstat(path) else {
        return false;
    };
    if st.file_type() == FileType::Directory {
        let Ok(entries) = sys::read_dir(path) else {
            return false;
        };
        for e in entries {
            let mut child = path.to_vec();
            if child.last() != Some(&b'/') {
                child.push(b'/');
            }
            child.extend_from_slice(&e.name);
            if !rm_rf(&child) {
                return false;
            }
        }
        sys::current().unlinkat(Fd::CWD, path, AtFlags::REMOVEDIR).is_ok()
    } else {
        sys::current().unlinkat(Fd::CWD, path, AtFlags::empty()).is_ok()
    }
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY-MM-DD` ou número de dias desde a época; vazio e `-1` viram "sem expiração" (`Some(None)`).
pub(crate) fn parse_date(s: &[u8]) -> Option<Option<i64>> {
    let t = std::str::from_utf8(s).ok()?;
    if t.is_empty() || t == "-1" {
        return Some(None);
    }
    if t.bytes().all(|b| b.is_ascii_digit()) {
        return t.parse::<i64>().ok().map(Some);
    }
    let p: Vec<&str> = t.split('-').collect();
    if p.len() != 3 || p[0].len() != 4 || p[1].is_empty() || p[2].is_empty() {
        return None;
    }
    let y: i64 = p[0].parse().ok()?;
    let m: i64 = p[1].parse().ok()?;
    let d: i64 = p[2].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || y < 1970 {
        return None;
    }
    Some(Some(days_from_civil(y, m, d)))
}

pub(crate) fn today() -> i64 {
    sys::try_current()
        .and_then(|s| s.clock_gettime(Clock::Realtime).ok())
        .map_or(0, |t| t.sec / 86_400)
}

// ---------------------------------------------------------------- userdel

const USERDEL_USAGE: &str = "Usage: userdel [options] LOGIN\n\nOptions:\n  -f, --force                   force some actions that would fail otherwise\n                                e.g. removal of user still logged in\n                                or files, even if not owned by the user\n  -h, --help                    display this help message and exit\n  -r, --remove                  remove home directory and mail spool\n  -R, --root CHROOT_DIR         directory to chroot into\n  -P, --prefix PREFIX_DIR       prefix directory where are located the /etc/* files\n  -Z, --selinux-user            remove any SELinux user mapping for the user\n\n";

/// Caminho sob o prefixo como o original concatena: `PREFIXO` + `/` + caminho cru (barra dupla).
pub(crate) fn under_prefix(prefix: &[u8], p: &[u8]) -> Vec<u8> {
    if prefix.is_empty() {
        return p.to_vec();
    }
    let mut v = prefix.to_vec();
    v.push(b'/');
    v.extend_from_slice(p);
    v
}

/// Grava o backup `arquivo-` com o conteúdo anterior e depois o novo conteúdo.
pub(crate) fn write_with_backup(path: &[u8], old: &[Vec<u8>], new: &[Vec<u8>]) -> bool {
    write_backup(path, old) && write_lines(path, new)
}

pub fn userdel_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| userdel(args))
}

fn userdel(args: &[OsString]) -> i32 {
    const P: &str = "userdel";
    let argv = io::args_bytes(args);
    let spec: Spec = &[
        (b'f', "force", false),
        (b'h', "help", false),
        (b'r', "remove", false),
        (b'R', "root", true),
        (b'P', "prefix", true),
        (b'Z', "selinux-user", false),
    ];
    let Some(o) = parse(P, &argv, spec) else {
        return usage(USERDEL_USAGE, 2);
    };
    if o.has(b'h') {
        return usage(USERDEL_USAGE, 0);
    }
    if o.rest.len() != 1 {
        return usage(USERDEL_USAGE, 2);
    }
    let name = o.rest[0].clone();
    let shown = io::lossy(&name);
    let prefix = o.get(b'P').or_else(|| o.get(b'R')).unwrap_or_default();
    let ppath = join(&prefix, "/etc/passwd");
    let spath = join(&prefix, "/etc/shadow");
    let gpath = join(&prefix, "/etc/group");
    let gspath = join(&prefix, "/etc/gshadow");
    let passwd = match read_lines(&ppath) {
        Ok(p) => p,
        Err(_) => {
            io::eprint(format!("{P}: cannot open {}\n", io::lossy(&ppath)));
            return 1;
        }
    };
    let Some(entry) = passwd.iter().find(|l| is_data(l) && name_eq(l, &name)).cloned() else {
        io::eprint(format!("{P}: user '{shown}' does not exist\n"));
        return 6;
    };
    let pf = owned(&entry, 7);
    let uid = parse_id(&pf[2]);
    let home = pf[5].clone();

    let group = match read_lines(&gpath) {
        Ok(g) => g,
        Err(_) => {
            io::eprint(format!("{P}: cannot open {}\n", io::lossy(&gpath)));
            return 10;
        }
    };
    let gshadow = read_lines(&gspath).ok();

    // Sob prefixo o oráculo não remove o grupo privado: só tira o usuário das listas de membros.
    let new_passwd: Vec<Vec<u8>> =
        passwd.iter().filter(|l| !(is_data(l) && name_eq(l, &name))).cloned().collect();
    let strip = |line: &[u8], cols: &[usize]| -> Vec<u8> {
        let mut v = owned(line, 4);
        for c in cols {
            let m: Vec<Vec<u8>> = members(&v[*c]).into_iter().filter(|x| *x != name).collect();
            v[*c] = join_members(&m);
        }
        join_fields(&v)
    };
    let mut new_group: Vec<Vec<u8>> = Vec::new();
    for l in &group {
        if is_data(l) {
            new_group.push(strip(l, &[3]));
        } else {
            new_group.push(l.clone());
        }
    }
    let new_gshadow: Option<Vec<Vec<u8>>> = gshadow.as_ref().map(|sh| {
        let mut ns = Vec::new();
        for l in sh {
            if is_data(l) {
                ns.push(strip(l, &[2, 3]));
            } else {
                ns.push(l.clone());
            }
        }
        ns
    });
    let old_shadow = read_lines(&spath).ok();
    let new_shadow = old_shadow
        .as_ref()
        .map(|s| s.iter().filter(|l| !(is_data(l) && name_eq(l, &name))).cloned().collect::<Vec<_>>());

    if !write_with_backup(&ppath, &passwd, &new_passwd) {
        io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&ppath)));
        return 1;
    }
    if let (Some(s), Some(old)) = (&new_shadow, &old_shadow) {
        if !write_with_backup(&spath, old, s) {
            io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&spath)));
            return 1;
        }
    }
    if new_group != group && !write_with_backup(&gpath, &group, &new_group) {
        io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&gpath)));
        return 10;
    }
    if let (Some(sh), Some(old)) = (&new_gshadow, &gshadow) {
        if sh != old && !write_with_backup(&gspath, old, sh) {
            io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&gspath)));
            return 10;
        }
    }

    let mut rc = 0;
    if o.has(b'r') {
        let force = o.has(b'f');
        let mail = under_prefix(&prefix, format!("/var/mail/{shown}").as_bytes());
        match sys::lstat(&mail) {
            Err(_) => io::eprint(format!(
                "{P}: {shown} mail spool ({}) not found\n",
                io::lossy(&mail)
            )),
            Ok(st) => {
                if !force && Some(u64::from(st.uid)) != uid {
                    io::eprint(format!(
                        "{P}: {} not owned by {shown}, not removing\n",
                        io::lossy(&mail)
                    ));
                    rc = 12;
                } else if sys::current().unlinkat(Fd::CWD, &mail, AtFlags::empty()).is_err() {
                    io::eprint(format!("{P}: warning: can't remove {}\n", io::lossy(&mail)));
                    rc = 12;
                }
            }
        }
        let hpath = under_prefix(&prefix, &home);
        match sys::lstat(&hpath) {
            Err(_) => io::eprint(format!(
                "{P}: {shown} home directory ({}) not found\n",
                io::lossy(&hpath)
            )),
            Ok(st) => {
                if st.file_type() != FileType::Directory {
                    io::eprint(format!(
                        "{P}: {} is not a directory, not removing\n",
                        io::lossy(&hpath)
                    ));
                    rc = 12;
                } else if !force && Some(u64::from(st.uid)) != uid {
                    io::eprint(format!(
                        "{P}: {} not owned by {shown}, not removing\n",
                        io::lossy(&hpath)
                    ));
                    rc = 12;
                } else if !rm_rf(&hpath) {
                    io::eprint(format!("{P}: error removing directory {}\n", io::lossy(&hpath)));
                    rc = 12;
                }
            }
        }
    }
    rc
}

// ---------------------------------------------------------------- usermod

const USERMOD_USAGE: &str = "Usage: usermod [options] LOGIN\n\nOptions:\n  -a, --append                  append the user to the supplemental GROUPS\n                                mentioned by the -G option without removing\n                                the user from other groups\n  -b, --badname                 allow bad names (DEPRECATED)\n  -c, --comment COMMENT         new value of the GECOS field\n  -d, --home HOME_DIR           new home directory for the user account\n  -e, --expiredate EXPIRE_DATE  set account expiration date to EXPIRE_DATE\n  -f, --inactive INACTIVE       set password inactive after expiration\n                                to INACTIVE\n  -g, --gid GROUP               force use GROUP as new primary group\n  -G, --groups GROUPS           new list of supplementary GROUPS\n  -h, --help                    display this help message and exit\n  -l, --login NEW_LOGIN         new value of the login name\n  -L, --lock                    lock the user account\n  -m, --move-home               move contents of the home directory to the\n                                new location (use only with -d)\n  -o, --non-unique              allow using duplicate (non-unique) UID\n  -p, --password PASSWORD       use encrypted password for the new password\n  -P, --prefix PREFIX_DIR       prefix directory where are located the /etc/* files\n  -r, --remove                  remove the user from only the supplemental GROUPS\n                                mentioned by the -G option without removing\n                                the user from other groups\n  -R, --root CHROOT_DIR         directory to chroot into\n  -s, --shell SHELL             new login shell for the user account\n  -u, --uid UID                 new UID for the user account\n  -U, --unlock                  unlock the user account\n  -v, --add-subuids FIRST-LAST  add range of subordinate uids\n  -V, --del-subuids FIRST-LAST  remove range of subordinate uids\n  -w, --add-subgids FIRST-LAST  add range of subordinate gids\n  -W, --del-subgids FIRST-LAST  remove range of subordinate gids\n  -Z, --selinux-user SEUSER     new SELinux user mapping for the user account\n      --selinux-range SERANGE   new SELinux MLS range for the user account\n\n";

pub fn usermod_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| usermod(args))
}

fn lock_pw(pw: &[u8]) -> Vec<u8> {
    if pw.first() == Some(&b'!') {
        pw.to_vec()
    } else {
        let mut v = vec![b'!'];
        v.extend_from_slice(pw);
        v
    }
}

fn unlock_pw(pw: &[u8]) -> Vec<u8> {
    if pw.first() != Some(&b'!') {
        return pw.to_vec();
    }
    let rest = &pw[1..];
    if rest.is_empty() {
        io::eprint(
            "usermod: unlocking the user's password would result in a passwordless account.\nYou should set a password with usermod -p to unlock this user's password.\n",
        );
        return pw.to_vec();
    }
    rest.to_vec()
}

fn gid_of_line(l: &[u8]) -> Option<u64> {
    fields(l).get(2).and_then(|f| parse_id(f))
}

fn usermod(args: &[OsString]) -> i32 {
    const P: &str = "usermod";
    let argv = io::args_bytes(args);
    let spec: Spec = &[
        (b'a', "append", false),
        (b'b', "badname", false),
        (b'c', "comment", true),
        (b'd', "home", true),
        (b'e', "expiredate", true),
        (b'f', "inactive", true),
        (b'g', "gid", true),
        (b'G', "groups", true),
        (b'h', "help", false),
        (b'l', "login", true),
        (b'L', "lock", false),
        (b'm', "move-home", false),
        (b'o', "non-unique", false),
        (b'p', "password", true),
        (b'r', "remove", false),
        (b'R', "root", true),
        (b'P', "prefix", true),
        (b's', "shell", true),
        (b'u', "uid", true),
        (b'U', "unlock", false),
        (b'v', "add-subuids", true),
        (b'V', "del-subuids", true),
        (b'w', "add-subgids", true),
        (b'W', "del-subgids", true),
        (b'Z', "selinux-user", true),
    ];
    let Some(o) = parse(P, &argv, spec) else {
        return usage(USERMOD_USAGE, 2);
    };
    if o.has(b'h') {
        return usage(USERMOD_USAGE, 0);
    }
    let prefix = o.get(b'P').or_else(|| o.get(b'R')).unwrap_or_default();
    let ppath = join(&prefix, "/etc/passwd");
    let spath = join(&prefix, "/etc/shadow");
    let gpath = join(&prefix, "/etc/group");
    let gspath = join(&prefix, "/etc/gshadow");

    let need_groups = o.has(b'g') || o.has(b'G');
    let group_lines: Option<Vec<Vec<u8>>> = if need_groups {
        match read_lines(&gpath) {
            Ok(g) => Some(g),
            Err(_) => {
                io::eprint(format!("{P}: cannot open {}\n", io::lossy(&gpath)));
                return 10;
            }
        }
    } else {
        None
    };
    let groups_ref: &[Vec<u8>] = group_lines.as_deref().unwrap_or(&[]);

    let mut comment: Option<Vec<u8>> = None;
    let mut home: Option<Vec<u8>> = None;
    let mut expire: Option<Option<i64>> = None;
    let mut inactive: Option<Option<i64>> = None;
    let mut new_gid: Option<u64> = None;
    let mut sup: Option<Vec<Vec<u8>>> = None;
    let mut new_login: Option<Vec<u8>> = None;
    let mut shell: Option<Vec<u8>> = None;
    let mut new_uid: Option<u64> = None;
    for (k, v) in &o.vals {
        let v = v.clone().unwrap_or_default();
        match *k {
            b'c' => {
                if !valid_field(&v) {
                    io::eprint(format!("{P}: invalid field '{}'\n", io::lossy(&v)));
                    return 3;
                }
                comment = Some(v);
            }
            b'd' => {
                if !valid_field(&v) || v.first() != Some(&b'/') {
                    io::eprint(format!("{P}: invalid home directory '{}'\n", io::lossy(&v)));
                    return 3;
                }
                home = Some(v);
            }
            b'e' => match parse_date(&v) {
                Some(d) => expire = Some(d),
                None => {
                    io::eprint(format!("{P}: invalid date '{}'\n", io::lossy(&v)));
                    return 3;
                }
            },
            b'f' => {
                let t = io::lossy(&v);
                match t.parse::<i64>() {
                    Ok(n) if n >= -1 => inactive = Some((n >= 0).then_some(n)),
                    _ => {
                        io::eprint(format!("{P}: invalid numeric argument '{t}'\n"));
                        return 3;
                    }
                }
            }
            b'g' => {
                let gid = match groups_ref.iter().find(|l| is_data(l) && name_eq(l, &v)) {
                    Some(l) => gid_of_line(l),
                    None => parse_id(&v).filter(|g| {
                        groups_ref.iter().any(|l| is_data(l) && gid_of_line(l) == Some(*g))
                    }),
                };
                match gid {
                    Some(g) => new_gid = Some(g),
                    None => {
                        io::eprint(format!("{P}: group '{}' does not exist\n", io::lossy(&v)));
                        return 6;
                    }
                }
            }
            b'G' => {
                let mut list = Vec::new();
                for n in v.split(|b| *b == b',').filter(|n| !n.is_empty()) {
                    let found = groups_ref
                        .iter()
                        .find(|l| is_data(l) && name_eq(l, n))
                        .or_else(|| {
                            let g = parse_id(n)?;
                            groups_ref.iter().find(|l| is_data(l) && gid_of_line(l) == Some(g))
                        });
                    match found {
                        Some(l) => list.push(fields(l)[0].to_vec()),
                        None => {
                            io::eprint(format!("{P}: group '{}' does not exist\n", io::lossy(n)));
                            return 6;
                        }
                    }
                }
                sup = Some(list);
            }
            b'l' => {
                if !valid_name(&v) {
                    io::eprint(format!("{P}: invalid user name '{}'\n", io::lossy(&v)));
                    return 3;
                }
                new_login = Some(v);
            }
            b's' => {
                if !valid_field(&v) || (v.first() != Some(&b'/') && v.first() != Some(&b'*')) {
                    io::eprint(format!("{P}: invalid shell '{}'\n", io::lossy(&v)));
                    return 3;
                }
                if v.first() == Some(&b'/') {
                    let ok = sys::stat(&v)
                        .is_ok_and(|s| s.file_type() == FileType::Regular && (s.mode & 0o111) != 0);
                    if !ok {
                        io::eprint(format!(
                            "{P}: Warning: missing or non-executable shell '{}'\n",
                            io::lossy(&v)
                        ));
                    }
                }
                shell = Some(v);
            }
            b'u' => match parse_id(&v) {
                Some(u) => new_uid = Some(u),
                None => {
                    io::eprint(format!("{P}: invalid user ID '{}'\n", io::lossy(&v)));
                    return 3;
                }
            },
            _ => {}
        }
    }
    if o.rest.len() != 1 {
        return usage(USERMOD_USAGE, 2);
    }
    let name = o.rest[0].clone();
    let shown = io::lossy(&name);

    let mut passwd = match read_lines(&ppath) {
        Ok(p) => p,
        Err(_) => {
            io::eprint(format!("{P}: cannot open {}\n", io::lossy(&ppath)));
            return 1;
        }
    };
    let Some(idx) = passwd.iter().position(|l| is_data(l) && name_eq(l, &name)) else {
        io::eprint(format!("{P}: user '{shown}' does not exist\n"));
        return 6;
    };

    if !b"cdefgGlLpsuUmoarvVwWZ".iter().any(|c| o.has(*c)) {
        io::eprint(format!("{P}: no changes\n"));
        return 0;
    }
    for (flag, text) in [(b'a', "-a"), (b'r', "-r")] {
        if o.has(flag) && !o.has(b'G') {
            io::eprint(format!("{P}: {text} flag is only allowed with the -G flag\n"));
            return usage(USERMOD_USAGE, 2);
        }
    }
    if o.has(b'm') && !o.has(b'd') {
        io::eprint(format!("{P}: -m flag is only allowed with the -d flag\n"));
        return usage(USERMOD_USAGE, 2);
    }
    if o.has(b'L') && o.has(b'U') {
        io::eprint(format!("{P}: the -L and -U options are incompatible\n"));
        return usage(USERMOD_USAGE, 2);
    }
    if (o.has(b'L') || o.has(b'U')) && o.has(b'p') {
        io::eprint(format!("{P}: the -L and -U options are incompatible with the -p option\n"));
        return usage(USERMOD_USAGE, 2);
    }
    let mut shadow = read_lines(&spath).ok();
    let shadow_old = shadow.clone();
    if (expire.is_some() || inactive.is_some()) && shadow.is_none() {
        io::eprint(format!("{P}: shadow passwords required for -e and -f\n"));
        return 2;
    }
    if let Some(n) = &new_login {
        if *n != name && passwd.iter().any(|l| is_data(l) && name_eq(l, n)) {
            io::eprint(format!("{P}: user '{}' already exists\n", io::lossy(n)));
            return 9;
        }
    }
    let mut pf = owned(&passwd[idx], 7);
    let old_uid = parse_id(&pf[2]);
    if let Some(u) = new_uid {
        let dup = passwd.iter().any(|l| is_data(l) && gid_of_line(l) == Some(u));
        if !o.has(b'o') && old_uid != Some(u) && dup {
            io::eprint(format!("{P}: UID '{u}' already exists\n"));
            return 4;
        }
    }
    let old_home = pf[5].clone();
    let passwd_old = passwd.clone();
    let final_name = new_login.clone().unwrap_or_else(|| name.clone());

    let sidx = shadow.as_ref().and_then(|s| s.iter().position(|l| is_data(l) && name_eq(l, &name)));
    let change_pw = |cur: &[u8]| -> Vec<u8> {
        let mut pw = o.get(b'p').unwrap_or_else(|| cur.to_vec());
        if o.has(b'L') {
            pw = lock_pw(&pw);
        }
        if o.has(b'U') {
            pw = unlock_pw(&pw);
        }
        pw
    };
    pf[0] = final_name.clone();
    if let Some(u) = new_uid {
        pf[2] = u.to_string().into_bytes();
    }
    if let Some(g) = new_gid {
        pf[3] = g.to_string().into_bytes();
    }
    if let Some(c) = &comment {
        pf[4] = c.clone();
    }
    if let Some(h) = &home {
        pf[5] = h.clone();
    }
    if let Some(s) = &shell {
        pf[6] = s.clone();
    }
    if sidx.is_none() {
        pf[1] = change_pw(&pf[1]);
    }
    passwd[idx] = join_fields(&pf);

    if let (Some(sh), Some(i)) = (shadow.as_mut(), sidx) {
        let mut sf = owned(&sh[i], 9);
        sf[0] = final_name.clone();
        sf[1] = change_pw(&sf[1]);
        if o.has(b'p') {
            sf[2] = today().to_string().into_bytes();
        }
        if let Some(n) = inactive {
            sf[6] = n.map(|n| n.to_string().into_bytes()).unwrap_or_default();
        }
        if let Some(e) = expire {
            sf[7] = e.map(|n| n.to_string().into_bytes()).unwrap_or_default();
        }
        sh[i] = join_fields(&sf);
    }

    let append = o.has(b'a');
    let remove = o.has(b'r');
    let edit_members = |line: &[u8], is_gshadow: bool| -> Vec<u8> {
        let mut v = owned(line, 4);
        let in_list = sup.as_ref().is_some_and(|l| l.iter().any(|g| *g == v[0]));
        let cols: &[usize] = if is_gshadow { &[2, 3] } else { &[3] };
        for &c in cols {
            let mut m = members(&v[c]);
            if new_login.is_some() {
                let mut renamed = false;
                for x in std::mem::take(&mut m) {
                    if x == name {
                        renamed = true;
                    } else {
                        m.push(x);
                    }
                }
                if renamed && !m.contains(&final_name) {
                    m.push(final_name.clone());
                }
            }
            if sup.is_some() {
                let present = m.contains(&final_name);
                if remove {
                    if in_list {
                        m.retain(|x| *x != final_name);
                    }
                } else if in_list {
                    if c == 3 && !present {
                        m.push(final_name.clone());
                    }
                } else if !append {
                    m.retain(|x| *x != final_name);
                }
            }
            v[c] = join_members(&m);
        }
        join_fields(&v)
    };
    let mut group_out: Option<(Vec<Vec<u8>>, Vec<Vec<u8>>)> = None;
    let mut gshadow_out: Option<(Vec<Vec<u8>>, Vec<Vec<u8>>)> = None;
    if new_login.is_some() || sup.is_some() {
        let g = match group_lines.clone() {
            Some(g) => g,
            None => read_lines(&gpath).unwrap_or_default(),
        };
        let edited: Vec<Vec<u8>> =
            g.iter().map(|l| if is_data(l) { edit_members(l, false) } else { l.clone() }).collect();
        if edited != g {
            group_out = Some((g, edited));
        }
        if let Ok(gs) = read_lines(&gspath) {
            let edited: Vec<Vec<u8>> = gs
                .iter()
                .map(|l| if is_data(l) { edit_members(l, true) } else { l.clone() })
                .collect();
            if edited != gs {
                gshadow_out = Some((gs, edited));
            }
        }
    }

    // O original só regrava passwd quando algum campo dele muda (não em -e, -f nem só -G).
    let pw_changed = b"cdglsuLUp".iter().any(|c| o.has(*c));
    let sp_changed = b"lLUpef".iter().any(|c| o.has(*c));
    if pw_changed && !write_with_backup(&ppath, &passwd_old, &passwd) {
        io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&ppath)));
        return 1;
    }
    if let (Some(sh), Some(old), Some(_)) = (&shadow, &shadow_old, sidx) {
        if sp_changed && !write_with_backup(&spath, old, sh) {
            io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&spath)));
            return 1;
        }
    }
    if let Some((old, g)) = &group_out {
        if !write_with_backup(&gpath, old, g) {
            io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&gpath)));
            return 10;
        }
    }
    if let Some((old, g)) = &gshadow_out {
        if !write_with_backup(&gspath, old, g) {
            io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&gspath)));
            return 10;
        }
    }

    if o.has(b'm') {
        let new_home = home.clone().unwrap_or_default();
        if new_home != old_home {
            let from = under_prefix(&prefix, &old_home);
            let to = under_prefix(&prefix, &new_home);
            if sys::lstat(&to).is_ok() {
                io::eprint(format!("{P}: directory {} exists\n", io::lossy(&to)));
                return 12;
            }
            if sys::lstat(&from).is_ok() {
                let r = sys::current().renameat2(Fd::CWD, &from, Fd::CWD, &to, RenameFlags::empty());
                if r.is_err() {
                    io::eprint(format!(
                        "{P}: cannot rename directory {} to {}\n",
                        io::lossy(&from),
                        io::lossy(&to)
                    ));
                    return 12;
                }
            }
        }
    }
    0
}
