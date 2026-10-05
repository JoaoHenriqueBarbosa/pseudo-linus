//! `useradd` do shadow 4.17 (Debian 13).
//!
//! Cria a entrada em `/etc/passwd`, `/etc/shadow`, `/etc/group` (grupo privado e grupos
//! suplementares) e `/etc/gshadow` (sob `-R`/`-P`, com o prefixo), e com `-m` o diretório pessoal
//! com a cópia de `/etc/skel`. Os padrões vêm de `/etc/default/useradd` e `/etc/login.defs` sob o
//! prefixo. O travamento do original é ignorado (sandbox). Ficam de fora: `lastlog`/`faillog`
//! (`-l`), subuid/subgid (`-F`), spool de e-mail, SELinux (`-Z`) e subvolume btrfs.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{AtFlags, Errno, Fd, FileType, OFlags, sys};

use crate::groupmgmt::{
    Spec, fields, is_data, join, name_eq, parse, parse_id, read_lines, usage, valid_name,
    write_lines,
};
use crate::usermgmt::{parse_date, today, under_prefix, valid_field, write_with_backup};
use crate::util::io::{self, File};

const USAGE: &str = "Usage: useradd [options] LOGIN\n       useradd -D\n       useradd -D [options]\n\nOptions:\n      --badname                 do not check for bad names\n  -b, --base-dir BASE_DIR       base directory for the home directory of the\n                                new account\n      --btrfs-subvolume-home    use BTRFS subvolume for home directory\n  -c, --comment COMMENT         GECOS field of the new account\n  -d, --home-dir HOME_DIR       home directory of the new account\n  -D, --defaults                print or change default useradd configuration\n  -e, --expiredate EXPIRE_DATE  expiration date of the new account\n  -f, --inactive INACTIVE       password inactivity period of the new account\n  -F, --add-subids-for-system   add entries to sub[ug]id even when adding a system user\n  -g, --gid GROUP               name or ID of the primary group of the new\n                                account\n  -G, --groups GROUPS           list of supplementary groups of the new\n                                account\n  -h, --help                    display this help message and exit\n  -k, --skel SKEL_DIR           use this alternative skeleton directory\n  -K, --key KEY=VALUE           override /etc/login.defs defaults\n  -l, --no-log-init             do not add the user to the lastlog and\n                                faillog databases\n  -m, --create-home             create the user's home directory\n  -M, --no-create-home          do not create the user's home directory\n  -N, --no-user-group           do not create a group with the same name as\n                                the user\n  -o, --non-unique              allow to create users with duplicate\n                                (non-unique) UID\n  -p, --password PASSWORD       encrypted password of the new account\n  -r, --system                  create a system account\n  -R, --root CHROOT_DIR         directory to chroot into\n  -P, --prefix PREFIX_DIR       prefix directory where are located the /etc/* files\n  -s, --shell SHELL             login shell of the new account\n  -u, --uid UID                 user ID of the new account\n  -U, --user-group              create a group with the same name as the user\n  -Z, --selinux-user SEUSER     use a specific SEUSER for the SELinux user mapping\n      --selinux-range SERANGE   use a specific MLS range for the SELinux user mapping\n\n";

const BADNAME: u8 = 1;
const BTRFS: u8 = 2;
const SERANGE: u8 = 3;

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| useradd(args))
}

/// Lê `CHAVE=VALOR` (ou `CHAVE VALOR` no login.defs) ignorando comentários e linhas vazias.
fn read_conf(path: &[u8], sep_space: bool) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Ok(lines) = read_lines(path) else {
        return out;
    };
    for l in lines {
        let t = io::lossy(&l);
        let t = t.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let split = if sep_space { t.split_once(char::is_whitespace) } else { t.split_once('=') };
        if let Some((k, v)) = split {
            let v = v.trim();
            let v = v.trim_matches('"');
            out.push((k.trim().to_string(), v.to_string()));
        }
    }
    out
}

struct Defaults {
    group: u64,
    base: String,
    inactive: String,
    expire: String,
    shell: String,
    skel: String,
    mail: String,
}

impl Defaults {
    fn load(path: &[u8]) -> Defaults {
        let mut d = Defaults {
            group: 100,
            base: "/home".into(),
            inactive: "-1".into(),
            expire: String::new(),
            shell: "/bin/sh".into(),
            skel: "/etc/skel".into(),
            mail: "no".into(),
        };
        for (k, v) in read_conf(path, false) {
            match k.as_str() {
                "GROUP" => {
                    if let Some(g) = parse_id(v.as_bytes()) {
                        d.group = g;
                    }
                }
                "HOME" => d.base = v,
                "INACTIVE" => d.inactive = v,
                "EXPIRE" => d.expire = v,
                "SHELL" => d.shell = v,
                "SKEL" => d.skel = v,
                "CREATE_MAIL_SPOOL" => d.mail = v,
                _ => {}
            }
        }
        d
    }

    fn lines(&self) -> Vec<Vec<u8>> {
        [
            "# useradd defaults file".to_string(),
            format!("GROUP={}", self.group),
            format!("HOME={}", self.base),
            format!("INACTIVE={}", self.inactive),
            format!("EXPIRE={}", self.expire),
            format!("SHELL={}", self.shell),
            format!("SKEL={}", self.skel),
            format!("CREATE_MAIL_SPOOL={}", self.mail),
        ]
        .into_iter()
        .map(String::into_bytes)
        .collect()
    }
}

struct LoginDefs {
    vals: Vec<(String, String)>,
}

impl LoginDefs {
    fn get(&self, key: &str) -> Option<&str> {
        self.vals.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
    fn num(&self, key: &str, default: i64) -> i64 {
        self.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
    }
    fn yes(&self, key: &str) -> bool {
        self.get(key).is_some_and(|v| v.eq_ignore_ascii_case("yes"))
    }
}

fn gid_field(l: &[u8]) -> Option<u64> {
    fields(l).get(2).and_then(|f| parse_id(f))
}

fn uid_field(l: &[u8]) -> Option<u64> {
    gid_field(l)
}

fn find_group<'a>(group: &'a [Vec<u8>], key: &[u8]) -> Option<&'a Vec<u8>> {
    group
        .iter()
        .find(|l| is_data(l) && name_eq(l, key))
        .or_else(|| {
            let g = parse_id(key)?;
            group.iter().find(|l| is_data(l) && gid_field(l) == Some(g))
        })
}

fn pick_id(used: &[u64], preferred: Option<u64>, system: bool, min: u64, max: u64, smin: u64, smax: u64) -> Option<u64> {
    if let Some(p) = preferred {
        let (lo, hi) = if system { (smin, smax) } else { (min, max) };
        if (lo..=hi).contains(&p) && !used.contains(&p) {
            return Some(p);
        }
    }
    if system {
        (smin..=smax).rev().find(|g| !used.contains(g))
    } else {
        let top = used.iter().copied().filter(|g| *g >= min && *g <= max).max();
        match top {
            Some(t) if t < max => Some(t + 1),
            Some(_) => (min..=max).find(|g| !used.contains(g)),
            None => Some(min),
        }
    }
}

fn mkdir_p(path: &[u8]) -> bool {
    let mut i = 1;
    while i <= path.len() {
        if i == path.len() || path[i] == b'/' {
            let part = &path[..i];
            if !part.is_empty() && part.last() != Some(&b'/') {
                let r = sys::current().mkdirat(Fd::CWD, part, 0o755);
                if let Err(e) = r {
                    if e != Errno::EEXIST {
                        return false;
                    }
                }
            }
        }
        i += 1;
    }
    true
}

fn chown(path: &[u8], uid: u64, gid: u64) {
    let _ = sys::current().fchownat(
        Fd::CWD,
        path,
        Some(uid as u32),
        Some(gid as u32),
        AtFlags::SYMLINK_NOFOLLOW,
    );
}

fn copy_tree(src: &[u8], dst: &[u8], uid: u64, gid: u64) -> bool {
    let Ok(entries) = sys::read_dir(src) else {
        return false;
    };
    for e in entries {
        if e.name == b"." || e.name == b".." {
            continue;
        }
        let mut s = src.to_vec();
        if s.last() != Some(&b'/') {
            s.push(b'/');
        }
        s.extend_from_slice(&e.name);
        let mut d = dst.to_vec();
        if d.last() != Some(&b'/') {
            d.push(b'/');
        }
        d.extend_from_slice(&e.name);
        let Ok(st) = sys::lstat(&s) else {
            return false;
        };
        let perm = st.mode & 0o7777;
        match st.file_type() {
            FileType::Directory => {
                if sys::current().mkdirat(Fd::CWD, &d, perm).is_err() {
                    return false;
                }
                let _ = sys::current().fchmodat(Fd::CWD, &d, perm, AtFlags::empty());
                if !copy_tree(&s, &d, uid, gid) {
                    return false;
                }
                chown(&d, uid, gid);
            }
            FileType::Symlink => {
                let Ok(t) = sys::current().readlinkat(Fd::CWD, &s) else {
                    return false;
                };
                if sys::current().symlinkat(&t, Fd::CWD, &d).is_err() {
                    return false;
                }
                chown(&d, uid, gid);
            }
            FileType::Regular => {
                let Ok(data) = io::read_path(&s) else {
                    return false;
                };
                let Ok(mut f) = File::open_with(&d, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, perm)
                else {
                    return false;
                };
                if f.write_all(&data).is_err() {
                    return false;
                }
                let _ = sys::current().fchmodat(Fd::CWD, &d, perm, AtFlags::empty());
                chown(&d, uid, gid);
            }
            _ => {}
        }
    }
    true
}

fn days_str(v: Option<i64>) -> String {
    v.map(|n| n.to_string()).unwrap_or_default()
}

fn add_member(line: &[u8], user: &[u8], col: usize) -> Vec<u8> {
    let mut v: Vec<Vec<u8>> = fields(line).iter().map(|x| x.to_vec()).collect();
    while v.len() < 4 {
        v.push(Vec::new());
    }
    let mut m: Vec<Vec<u8>> =
        v[col].split(|b| *b == b',').filter(|x| !x.is_empty()).map(<[u8]>::to_vec).collect();
    if !m.iter().any(|x| x == user) {
        m.push(user.to_vec());
    }
    v[col] = m.join(&b',');
    v.join(&b':')
}

fn useradd(args: &[OsString]) -> i32 {
    const P: &str = "useradd";
    let argv = io::args_bytes(args);
    let spec: Spec = &[
        (BADNAME, "badname", false),
        (b'b', "base-dir", true),
        (BTRFS, "btrfs-subvolume-home", false),
        (b'c', "comment", true),
        (b'd', "home-dir", true),
        (b'D', "defaults", false),
        (b'e', "expiredate", true),
        (b'f', "inactive", true),
        (b'F', "add-subids-for-system", false),
        (b'g', "gid", true),
        (b'G', "groups", true),
        (b'h', "help", false),
        (b'k', "skel", true),
        (b'K', "key", true),
        (b'l', "no-log-init", false),
        (b'm', "create-home", false),
        (b'M', "no-create-home", false),
        (b'N', "no-user-group", false),
        (b'o', "non-unique", false),
        (b'p', "password", true),
        (b'r', "system", false),
        (b'R', "root", true),
        (b'P', "prefix", true),
        (b's', "shell", true),
        (b'u', "uid", true),
        (b'U', "user-group", false),
        (b'Z', "selinux-user", true),
        (SERANGE, "selinux-range", true),
    ];
    let Some(o) = parse(P, &argv, spec) else {
        return usage(USAGE, 2);
    };
    if o.has(b'h') {
        return usage(USAGE, 0);
    }
    let prefix = o.get(b'P').or_else(|| o.get(b'R')).unwrap_or_default();
    let ppath = join(&prefix, "/etc/passwd");
    let spath = join(&prefix, "/etc/shadow");
    let gpath = join(&prefix, "/etc/group");
    let gspath = join(&prefix, "/etc/gshadow");
    let dpath = join(&prefix, "/etc/default/useradd");
    let mut def = Defaults::load(&dpath);
    let mut ld = LoginDefs { vals: read_conf(&join(&prefix, "/etc/login.defs"), true) };

    let group_lines = read_lines(&gpath);
    let groups_ref: Vec<Vec<u8>> = group_lines.as_ref().map(Clone::clone).unwrap_or_default();

    let mut comment = Vec::new();
    let mut home: Option<Vec<u8>> = None;
    let mut expire: Option<Option<i64>> = None;
    let mut inactive: Option<String> = None;
    let mut gid_opt: Option<u64> = None;
    let mut sup: Vec<Vec<u8>> = Vec::new();
    let mut uid_opt: Option<u64> = None;
    let mut shell: Option<String> = None;
    let mut base: Option<String> = None;
    let mut skel: Option<Vec<u8>> = None;
    let mut changed_defaults = false;
    for (k, v) in &o.vals {
        let v = v.clone().unwrap_or_default();
        match *k {
            b'b' => {
                if v.first() != Some(&b'/') || !valid_field(&v) {
                    io::eprint(format!("{P}: invalid base directory '{}'\n", io::lossy(&v)));
                    return 3;
                }
                base = Some(io::lossy(&v));
                changed_defaults = true;
            }
            b'c' => {
                if !valid_field(&v) {
                    io::eprint(format!("{P}: invalid comment '{}'\n", io::lossy(&v)));
                    return 3;
                }
                comment = v;
            }
            b'd' => {
                if v.first() != Some(&b'/') || !valid_field(&v) {
                    io::eprint(format!("{P}: invalid home directory '{}'\n", io::lossy(&v)));
                    return 3;
                }
                home = Some(v);
            }
            b'e' => match parse_date(&v) {
                Some(d) => {
                    expire = Some(d);
                    changed_defaults = true;
                }
                None => {
                    io::eprint(format!("{P}: invalid date '{}'\n", io::lossy(&v)));
                    return 3;
                }
            },
            b'f' => {
                let t = io::lossy(&v);
                match t.parse::<i64>() {
                    Ok(n) if n >= -1 => {
                        inactive = Some(n.to_string());
                        changed_defaults = true;
                    }
                    _ => {
                        io::eprint(format!("{P}: invalid numeric argument '{t}'\n"));
                        return 3;
                    }
                }
            }
            b'g' => {
                if group_lines.is_err() {
                    io::eprint(format!("{P}: cannot open {}\n", io::lossy(&gpath)));
                    return 10;
                }
                match find_group(&groups_ref, &v).and_then(|l| gid_field(l)) {
                    Some(g) => {
                        gid_opt = Some(g);
                        changed_defaults = true;
                    }
                    None => {
                        io::eprint(format!("{P}: group '{}' does not exist\n", io::lossy(&v)));
                        return 6;
                    }
                }
            }
            b'G' => {
                if group_lines.is_err() {
                    io::eprint(format!("{P}: cannot open {}\n", io::lossy(&gpath)));
                    return 10;
                }
                for n in v.split(|b| *b == b',').filter(|n| !n.is_empty()) {
                    match find_group(&groups_ref, n) {
                        Some(l) => sup.push(fields(l)[0].to_vec()),
                        None => {
                            io::eprint(format!("{P}: group '{}' does not exist\n", io::lossy(n)));
                            return 6;
                        }
                    }
                }
            }
            b'k' => skel = Some(v),
            b'K' => {
                let Some(p) = v.iter().position(|b| *b == b'=') else {
                    io::eprint(format!("{P}: -K requires KEY=VALUE\n"));
                    return 1;
                };
                ld.vals.push((io::lossy(&v[..p]), io::lossy(&v[p + 1..])));
            }
            b's' => {
                if !valid_field(&v) || (v.first() != Some(&b'/') && v.first() != Some(&b'*') && !v.is_empty()) {
                    io::eprint(format!("{P}: invalid shell '{}'\n", io::lossy(&v)));
                    return 3;
                }
                shell = Some(io::lossy(&v));
                changed_defaults = true;
            }
            b'u' => match parse_id(&v) {
                Some(u) => uid_opt = Some(u),
                None => {
                    io::eprint(format!("{P}: invalid user ID '{}'\n", io::lossy(&v)));
                    return 3;
                }
            },
            _ => {}
        }
    }

    // ---- modo -D
    if o.has(b'D') {
        if !o.rest.is_empty() {
            return usage(USAGE, 2);
        }
        if changed_defaults {
            if let Some(g) = gid_opt {
                def.group = g;
            }
            if let Some(b) = base {
                def.base = b;
            }
            if let Some(i) = inactive {
                def.inactive = i;
            }
            if let Some(e) = expire {
                def.expire = days_str(e);
            }
            if let Some(s) = shell {
                def.shell = s;
            }
            if let Some(s) = &skel {
                def.skel = io::lossy(s);
            }
            if !write_lines(&dpath, &def.lines()) {
                io::eprint(format!("{P}: cannot create new defaults file\n"));
                return 1;
            }
            return 0;
        }
        let mut out = String::new();
        out.push_str(&format!("GROUP={}\n", def.group));
        out.push_str(&format!("HOME={}\n", def.base));
        out.push_str(&format!("INACTIVE={}\n", def.inactive));
        out.push_str(&format!("EXPIRE={}\n", def.expire));
        out.push_str(&format!("SHELL={}\n", def.shell));
        out.push_str(&format!("SKEL={}\n", def.skel));
        out.push_str(&format!("CREATE_MAIL_SPOOL={}\n", def.mail));
        let _ = io::stdout().write_all(out.as_bytes());
        return 0;
    }

    if o.rest.len() != 1 {
        return usage(USAGE, 2);
    }
    if o.has(b'o') && uid_opt.is_none() {
        return usage(USAGE, 2);
    }
    if o.has(b'N') && o.has(b'U') {
        return usage(USAGE, 2);
    }
    let name = o.rest[0].clone();
    let shown = io::lossy(&name);
    if (!valid_name(&name) || name.len() > 32) && !o.has(BADNAME) {
        io::eprint(format!("{P}: invalid user name '{shown}': use --badname to ignore\n"));
        return 3;
    }
    if o.has(BADNAME) && (name.is_empty() || !valid_field(&name)) {
        io::eprint(format!("{P}: invalid user name '{shown}'\n"));
        return 3;
    }

    let mut passwd = match read_lines(&ppath) {
        Ok(p) => p,
        Err(_) => {
            io::eprint(format!("{P}: cannot open {}\n", io::lossy(&ppath)));
            return 1;
        }
    };
    let mut group = match group_lines {
        Ok(g) => g,
        Err(_) => {
            io::eprint(format!("{P}: cannot open {}\n", io::lossy(&gpath)));
            return 10;
        }
    };
    if passwd.iter().any(|l| is_data(l) && name_eq(l, &name)) {
        io::eprint(format!("{P}: user '{shown}' already exists\n"));
        return 9;
    }

    let system = o.has(b'r');
    let uid_min = ld.num("UID_MIN", 1000) as u64;
    let uid_max = ld.num("UID_MAX", 60000) as u64;
    let suid_min = ld.num("SYS_UID_MIN", 101) as u64;
    let suid_max = ld.num("SYS_UID_MAX", if ld.get("UID_MIN").is_some() { uid_min.saturating_sub(1) as i64 } else { 999 }) as u64;
    let gid_min = ld.num("GID_MIN", 1000) as u64;
    let gid_max = ld.num("GID_MAX", 60000) as u64;
    let sgid_min = ld.num("SYS_GID_MIN", 101) as u64;
    let sgid_max = ld.num("SYS_GID_MAX", if ld.get("GID_MIN").is_some() { gid_min.saturating_sub(1) as i64 } else { 999 }) as u64;

    let usergroup = o.has(b'U') || (!o.has(b'N') && gid_opt.is_none() && ld.yes("USERGROUPS_ENAB"));
    if usergroup && gid_opt.is_none() && group.iter().any(|l| is_data(l) && name_eq(l, &name)) {
        io::eprint(format!(
            "{P}: group {shown} exists - if you want to add this user to that group, use -g.\n"
        ));
        return 9;
    }

    let used_uids: Vec<u64> = passwd.iter().filter(|l| is_data(l)).filter_map(|l| uid_field(l)).collect();
    let uid = match uid_opt {
        Some(u) => {
            if !o.has(b'o') && used_uids.contains(&u) {
                io::eprint(format!("{P}: UID '{u}' already exists\n"));
                return 4;
            }
            u
        }
        None => match pick_id(&used_uids, None, system, uid_min, uid_max, suid_min, suid_max) {
            Some(u) => u,
            None => {
                io::eprint(format!("{P}: could not find a free UID\n"));
                return 4;
            }
        },
    };

    let mut new_group_line: Option<(Vec<u8>, Vec<u8>)> = None;
    let gid = if let Some(g) = gid_opt {
        g
    } else if usergroup {
        let used: Vec<u64> = group.iter().filter(|l| is_data(l)).filter_map(|l| gid_field(l)).collect();
        match pick_id(&used, Some(uid), system, gid_min, gid_max, sgid_min, sgid_max) {
            Some(g) => {
                new_group_line =
                    Some((format!("{shown}:x:{g}:").into_bytes(), format!("{shown}:!::").into_bytes()));
                g
            }
            None => {
                io::eprint(format!("{P}: cannot find unused GID\n"));
                return 4;
            }
        }
    } else {
        def.group
    };

    let gshadow = read_lines(&gspath).ok();
    let shadow = read_lines(&spath).ok();
    let pw = o.get(b'p').map(|v| io::lossy(&v)).unwrap_or_else(|| "!".into());
    let home_dir = home.clone().unwrap_or_else(|| {
        let b = base.clone().unwrap_or_else(|| def.base.clone());
        let mut v = b.trim_end_matches('/').as_bytes().to_vec();
        v.push(b'/');
        v.extend_from_slice(&name);
        v
    });
    let shell = shell.unwrap_or_else(|| def.shell.clone());
    let comment_s = io::lossy(&comment);

    let old_passwd = passwd.clone();
    let pw_field = if shadow.is_some() { "x".to_string() } else { pw.clone() };
    passwd.push(
        format!("{shown}:{pw_field}:{uid}:{gid}:{comment_s}:{}:{shell}", io::lossy(&home_dir))
            .into_bytes(),
    );
    if !write_with_backup(&ppath, &old_passwd, &passwd) {
        io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&ppath)));
        return 1;
    }
    if let Some(mut sh) = shadow {
        let old = sh.clone();
        let min = ld.num("PASS_MIN_DAYS", -1);
        let max = ld.num("PASS_MAX_DAYS", -1);
        let warn = ld.num("PASS_WARN_AGE", -1);
        let inact = inactive.unwrap_or_else(|| def.inactive.clone());
        let inact = if inact == "-1" { String::new() } else { inact };
        let exp = match expire {
            Some(e) => days_str(e),
            None => match parse_date(def.expire.as_bytes()) {
                Some(e) => days_str(e),
                None => String::new(),
            },
        };
        let neg = |n: i64| if n < 0 { String::new() } else { n.to_string() };
        sh.push(
            format!("{shown}:{pw}:{}:{}:{}:{}:{inact}:{exp}:", today(), neg(min), neg(max), neg(warn))
                .into_bytes(),
        );
        if !write_with_backup(&spath, &old, &sh) {
            io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&spath)));
            return 1;
        }
    }

    let old_group = group.clone();
    if let Some((g, _)) = &new_group_line {
        group.push(g.clone());
    }
    for l in group.iter_mut() {
        if is_data(l) && sup.iter().any(|s| name_eq(l, s)) {
            *l = add_member(l, &name, 3);
        }
    }
    if group != old_group && !write_with_backup(&gpath, &old_group, &group) {
        io::eprint(format!("{P}: failure while writing changes to /etc/group\n"));
        return 10;
    }
    if let Some(mut gs) = gshadow {
        let old = gs.clone();
        if let Some((_, s)) = &new_group_line {
            gs.push(s.clone());
        }
        for l in gs.iter_mut() {
            if is_data(l) && sup.iter().any(|s| name_eq(l, s)) {
                *l = add_member(l, &name, 3);
            }
        }
        if gs != old && !write_with_backup(&gspath, &old, &gs) {
            io::eprint(format!("{P}: failure while writing changes to /etc/gshadow\n"));
            return 10;
        }
    }

    let create_home = o.has(b'm') || (!o.has(b'M') && ld.yes("CREATE_HOME"));
    if create_home {
        let hpath = under_prefix(&prefix, &home_dir);
        if sys::lstat(&hpath).is_ok() {
            io::eprint(format!(
                "{P}: warning: the home directory {} already exists.\n{P}: Not copying any file from skel directory into it.\n",
                io::lossy(&home_dir)
            ));
        } else {
            if !mkdir_p(&hpath) {
                io::eprint(format!("{P}: cannot create directory {}\n", io::lossy(&hpath)));
                return 12;
            }
            let umask = ld.get("UMASK").and_then(|v| u32::from_str_radix(v, 8).ok()).unwrap_or(0o022);
            let mode = ld
                .get("HOME_MODE")
                .and_then(|v| u32::from_str_radix(v, 8).ok())
                .unwrap_or(0o777 & !umask);
            let _ = sys::current().fchmodat(Fd::CWD, &hpath, mode, AtFlags::empty());
            chown(&hpath, uid, gid);
            let skel_dir = skel.unwrap_or_else(|| def.skel.clone().into_bytes());
            let skel_path = under_prefix(&prefix, &skel_dir);
            if sys::lstat(&skel_path).is_ok() {
                let _ = copy_tree(&skel_path, &hpath, uid, gid);
            }
        }
    }
    0
}
