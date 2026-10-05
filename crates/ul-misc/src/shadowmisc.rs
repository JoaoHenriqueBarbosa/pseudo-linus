//! `chage` e `gpasswd` do shadow 4.17 (Debian 13).
//!
//! Editam `/etc/shadow`, `/etc/group` e `/etc/gshadow` (sob `-R`/`-P`, com o prefixo). O travamento
//! do original é ignorado (sandbox). O modo interativo (sem opções) não é suportado.

use std::ffi::OsString;
use std::io::Write;

use crate::groupmgmt::{
    fields, is_data, join, name_eq, parse, read_lines, usage, write_backup, write_lines, Spec,
};
use crate::util::io;

fn parse_long(s: &[u8]) -> Option<i64> {
    let t = std::str::from_utf8(s).ok()?;
    let digits = t.strip_prefix('-').unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse().ok()
}

/// Dias desde 1970-01-01 para uma data civil (algoritmo de Howard Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Número de dias ou data `YYYY-MM-DD`, como o `strtoday` do shadow.
fn parse_day(s: &[u8]) -> Option<i64> {
    if let Some(n) = parse_long(s) {
        return (n >= -1).then_some(n);
    }
    let t = std::str::from_utf8(s).ok()?;
    let p: Vec<&str> = t.split('-').collect();
    if p.len() != 3 || p.iter().any(|x| x.is_empty() || !x.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || y < 1970 {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

fn fmt_date(days: i64, iso: bool) -> String {
    const MON: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (y, m, d) = civil_from_days(days);
    if iso {
        format!("{y:04}-{m:02}-{d:02}")
    } else {
        format!("{} {d:02}, {y}", MON[(m - 1) as usize])
    }
}

fn owned(line: &[u8], min: usize) -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = fields(line).iter().map(|x| x.to_vec()).collect();
    while v.len() < min {
        v.push(Vec::new());
    }
    v
}

fn num_field(v: &[Vec<u8>], i: usize) -> i64 {
    v.get(i).and_then(|f| parse_long(f)).unwrap_or(-1)
}

fn put_num(v: &mut [Vec<u8>], i: usize, n: i64) {
    v[i] = if n < 0 { Vec::new() } else { n.to_string().into_bytes() };
}

// ---------------------------------------------------------------- chage

const CHAGE_USAGE: &str = "Usage: chage [options] LOGIN\n\nOptions:\n  -d, --lastday LAST_DAY        set date of last password change to LAST_DAY\n  -E, --expiredate EXPIRE_DATE  set account expiration date to EXPIRE_DATE\n  -h, --help                    display this help message and exit\n  -i, --iso8601                 use YYYY-MM-DD when printing dates\n  -I, --inactive INACTIVE       set password inactive after expiration\n                                to INACTIVE\n  -l, --list                    show account aging information\n  -m, --mindays MIN_DAYS        set minimum number of days before password\n                                change to MIN_DAYS\n  -M, --maxdays MAX_DAYS        set maximum number of days before password\n                                change to MAX_DAYS\n  -R, --root CHROOT_DIR         directory to chroot into\n  -P, --prefix PREFIX_DIR       directory prefix\n  -W, --warndays WARN_DAYS      set expiration warning days to WARN_DAYS\n\n";

pub fn chage_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| chage(args))
}

fn chage(args: &[OsString]) -> i32 {
    const P: &str = "chage";
    let argv = io::args_bytes(args);
    let spec: Spec = &[
        (b'd', "lastday", true),
        (b'E', "expiredate", true),
        (b'h', "help", false),
        (b'i', "iso8601", false),
        (b'I', "inactive", true),
        (b'l', "list", false),
        (b'm', "mindays", true),
        (b'M', "maxdays", true),
        (b'R', "root", true),
        (b'P', "prefix", true),
        (b'W', "warndays", true),
    ];
    let Some(o) = parse(P, &argv, spec) else {
        return usage(CHAGE_USAGE, 2);
    };
    // (campo no shadow, valor novo)
    let mut edits: Vec<(usize, i64)> = Vec::new();
    for (k, v) in &o.vals {
        let arg = v.as_deref().unwrap_or(b"");
        match *k {
            b'h' => return usage(CHAGE_USAGE, 0),
            b'd' | b'E' => match parse_day(arg) {
                Some(n) => edits.push((if *k == b'd' { 2 } else { 7 }, n)),
                None => {
                    io::eprint(format!("{P}: invalid date '{}'\n", io::lossy(arg)));
                    return usage(CHAGE_USAGE, 2);
                }
            },
            b'm' | b'M' | b'W' | b'I' => match parse_long(arg) {
                Some(n) if n >= -1 => {
                    let idx = match *k {
                        b'm' => 3,
                        b'M' => 4,
                        b'W' => 5,
                        _ => 6,
                    };
                    edits.push((idx, n));
                }
                _ => {
                    io::eprint(format!("{P}: invalid numeric argument '{}'\n", io::lossy(arg)));
                    return usage(CHAGE_USAGE, 2);
                }
            },
            _ => {}
        }
    }
    if o.rest.len() != 1 {
        return usage(CHAGE_USAGE, 2);
    }
    let list = o.has(b'l');
    if list && !edits.is_empty() {
        io::eprint(format!("{P}: do not include \"l\" with other flags\n"));
        return usage(CHAGE_USAGE, 2);
    }
    if !list && edits.is_empty() {
        // O modo interativo do original não é suportado.
        return usage(CHAGE_USAGE, 2);
    }
    let name = o.rest[0].clone();
    let prefix = o.get(b'P').or_else(|| o.get(b'R')).unwrap_or_default();
    // O oráculo concatena o prefixo sem normalizar (`/work/case//etc/passwd`).
    let raw = |p: &str| {
        let mut v = prefix.clone();
        v.extend_from_slice(p.as_bytes());
        v
    };
    let ppath = raw("/etc/passwd");
    let spath = raw("/etc/shadow");
    let passwd = read_lines(&ppath).unwrap_or_default();
    if !passwd.iter().any(|l| is_data(l) && name_eq(l, &name)) {
        io::eprint(format!(
            "{P}: user '{}' does not exist in {}\n",
            io::lossy(&name),
            io::lossy(&ppath)
        ));
        return 1;
    }
    let mut shadow = match read_lines(&spath) {
        Ok(s) => s,
        Err(_) => {
            io::eprint(format!("{P}: cannot open {}\n", io::lossy(&spath)));
            return 1;
        }
    };
    let Some(idx) = shadow.iter().position(|l| is_data(l) && name_eq(l, &name)) else {
        io::eprint(format!(
            "{P}: user '{}' does not exist in {}\n",
            io::lossy(&name),
            io::lossy(&spath)
        ));
        return 1;
    };
    let mut f = owned(&shadow[idx], 9);
    if list {
        let iso = o.has(b'i');
        let last = num_field(&f, 2);
        let min = num_field(&f, 3);
        let max = num_field(&f, 4);
        let warn = num_field(&f, 5);
        let inact = num_field(&f, 6);
        let expire = num_field(&f, 7);
        let must_change = last == 0;
        let never_exp = last < 0 || max >= 10000 || max < 0;
        let mut out = String::new();
        out.push_str("Last password change\t\t\t\t\t: ");
        if last < 0 {
            out.push_str("never\n");
        } else if last == 0 {
            out.push_str("password must be changed\n");
        } else {
            out.push_str(&fmt_date(last, iso));
            out.push('\n');
        }
        out.push_str("Password expires\t\t\t\t\t: ");
        if must_change {
            out.push_str("password must be changed\n");
        } else if never_exp {
            out.push_str("never\n");
        } else {
            out.push_str(&fmt_date(last + max, iso));
            out.push('\n');
        }
        out.push_str("Password inactive\t\t\t\t\t: ");
        if must_change {
            out.push_str("password must be changed\n");
        } else if never_exp || inact < 0 {
            out.push_str("never\n");
        } else {
            out.push_str(&fmt_date(last + max + inact, iso));
            out.push('\n');
        }
        out.push_str("Account expires\t\t\t\t\t\t: ");
        if expire < 0 {
            out.push_str("never\n");
        } else {
            out.push_str(&fmt_date(expire, iso));
            out.push('\n');
        }
        out.push_str(&format!("Minimum number of days between password change\t\t: {min}\n"));
        out.push_str(&format!("Maximum number of days between password change\t\t: {max}\n"));
        out.push_str(&format!("Number of days of warning before password expires\t: {warn}\n"));
        let _ = io::stdout().write_all(out.as_bytes());
        return 0;
    }
    for (i, n) in edits {
        put_num(&mut f, i, n);
    }
    let old_shadow = shadow.clone();
    shadow[idx] = f.join(&b':'.to_owned());
    if !write_backup(&spath, &old_shadow) || !write_lines(&spath, &shadow) {
        io::eprint(format!("{P}: failure while writing changes to /etc/shadow\n"));
        return 1;
    }
    0
}

// ---------------------------------------------------------------- gpasswd

const GPASSWD_USAGE: &str = "Usage: gpasswd [option] GROUP\n\nOptions:\n  -a, --add USER                add USER to GROUP\n  -d, --delete USER             remove USER from GROUP\n  -h, --help                    display this help message and exit\n  -Q, --root CHROOT_DIR         directory to chroot into\n  -r, --remove-password         remove the GROUP's password\n  -R, --restrict                restrict access to GROUP to its members\n  -M, --members USER,...        set the list of members of GROUP\n  -A, --administrators ADMIN,...\n                                set the list of administrators for GROUP\nExcept for the -A and -M options, the options cannot be combined.\n";

pub fn gpasswd_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| gpasswd(args))
}

fn list_of(field: &[u8]) -> Vec<Vec<u8>> {
    field.split(|b| *b == b',').filter(|x| !x.is_empty()).map(<[u8]>::to_vec).collect()
}

fn gpasswd(args: &[OsString]) -> i32 {
    const P: &str = "gpasswd";
    let argv = io::args_bytes(args);
    let spec: Spec = &[
        (b'a', "add", true),
        (b'd', "delete", true),
        (b'h', "help", false),
        (b'Q', "root", true),
        (b'r', "remove-password", false),
        (b'R', "restrict", false),
        (b'M', "members", true),
        (b'A', "administrators", true),
    ];
    let Some(o) = parse(P, &argv, spec) else {
        return usage(GPASSWD_USAGE, 2);
    };
    if o.has(b'h') {
        return usage(GPASSWD_USAGE, 0);
    }
    let single = [b'a', b'd', b'r', b'R'].iter().filter(|c| o.has(**c)).count()
        + usize::from(o.has(b'A') || o.has(b'M'));
    if o.rest.len() != 1 {
        return usage(GPASSWD_USAGE, 2);
    }
    let name = o.rest[0].clone();
    let nm = io::lossy(&name);
    let has_prefix = o.has(b'Q');
    let prefix = o.get(b'Q').unwrap_or_default();
    // Como o oráculo: sob -Q o uid 0 precisa existir no passwd do prefixo.
    if has_prefix {
        let pw = read_lines(&join(&prefix, "/etc/passwd")).unwrap_or_default();
        let root = pw
            .iter()
            .any(|l| is_data(l) && fields(l).get(2).is_some_and(|f| *f == b"0"));
        if !root {
            io::eprint(format!("{P}: Cannot determine your user name.\n"));
            return 1;
        }
    }
    if let Some(u) = o.get(b'a') {
        let pw = read_lines(&join(&prefix, "/etc/passwd")).unwrap_or_default();
        if !pw.iter().any(|l| is_data(l) && name_eq(l, &u)) {
            io::eprint(format!("{P}: user '{}' does not exist\n", io::lossy(&u)));
            return 3;
        }
    }
    if single > 1 || single == 0 {
        // A troca interativa de senha não é suportada.
        return usage(GPASSWD_USAGE, 2);
    }
    let gpath = join(&prefix, "/etc/group");
    let spath = join(&prefix, "/etc/gshadow");
    let ppath = join(&prefix, "/etc/passwd");
    let mut group = match read_lines(&gpath) {
        Ok(g) => g,
        Err(_) => {
            io::eprint(format!("{P}: cannot open {}\n", io::lossy(&gpath)));
            return 1;
        }
    };
    let mut gshadow = read_lines(&spath).ok();
    let Some(gi) = group.iter().position(|l| is_data(l) && name_eq(l, &name)) else {
        io::eprint(format!("{P}: group '{nm}' does not exist in {}\n", io::lossy(&gpath)));
        return 3;
    };
    let passwd = read_lines(&ppath).unwrap_or_default();
    let user_exists = |u: &[u8]| passwd.iter().any(|l| is_data(l) && name_eq(l, u));

    let mut g = owned(&group[gi], 4);
    let mut s: Option<Vec<Vec<u8>>> = gshadow.as_ref().map(|sh| {
        match sh.iter().find(|l| is_data(l) && name_eq(l, &name)) {
            Some(l) => owned(l, 4),
            None => vec![name.clone(), b"!".to_vec(), Vec::new(), g[3].clone()],
        }
    });

    if let Some(u) = o.get(b'a') {
        if !user_exists(&u) {
            io::eprint(format!("{P}: user '{}' does not exist\n", io::lossy(&u)));
            return 3;
        }
        let mut mem = list_of(&g[3]);
        if mem.contains(&u) {
            io::eprint(format!("{P}: user '{}' is already a member of '{nm}'\n", io::lossy(&u)));
            return 3;
        }
        mem.push(u.clone());
        g[3] = mem.join(&b','.to_owned());
        if let Some(s) = s.as_mut() {
            let mut sm = list_of(&s[3]);
            if !sm.contains(&u) {
                sm.push(u.clone());
            }
            s[3] = sm.join(&b','.to_owned());
        }
        let _ = io::stdout()
            .write_all(format!("Adding user {} to group {nm}\n", io::lossy(&u)).as_bytes());
    } else if let Some(u) = o.get(b'd') {
        let mut mem = list_of(&g[3]);
        if !mem.contains(&u) {
            io::eprint(format!("{P}: user '{}' is not a member of '{nm}'\n", io::lossy(&u)));
            return 3;
        }
        mem.retain(|m| *m != u);
        g[3] = mem.join(&b','.to_owned());
        if let Some(s) = s.as_mut() {
            let mut sm = list_of(&s[3]);
            sm.retain(|m| *m != u);
            s[3] = sm.join(&b','.to_owned());
        }
        let _ = io::stdout()
            .write_all(format!("Removing user {} from group {nm}\n", io::lossy(&u)).as_bytes());
    } else if o.has(b'r') {
        match s.as_mut() {
            Some(s) => s[1] = Vec::new(),
            None => g[1] = Vec::new(),
        }
    } else if o.has(b'R') {
        match s.as_mut() {
            Some(s) => s[1] = b"!".to_vec(),
            None => g[1] = b"!".to_vec(),
        }
    } else {
        if let Some(a) = o.get(b'A') {
            let list = list_of(&a);
            for u in &list {
                if !user_exists(u) {
                    io::eprint(format!("{P}: user '{}' does not exist\n", io::lossy(u)));
                    return 3;
                }
            }
            if let Some(s) = s.as_mut() {
                s[2] = list.join(&b','.to_owned());
            }
        }
        if let Some(m) = o.get(b'M') {
            let list = list_of(&m);
            for u in &list {
                if !user_exists(u) {
                    io::eprint(format!("{P}: user '{}' does not exist\n", io::lossy(u)));
                    return 3;
                }
            }
            g[3] = list.join(&b','.to_owned());
            if let Some(s) = s.as_mut() {
                s[3] = g[3].clone();
            }
        }
    }

    let old_group = group.clone();
    let old_gshadow = gshadow.clone();
    group[gi] = g.join(&b':'.to_owned());
    if !write_backup(&gpath, &old_group) || !write_lines(&gpath, &group) {
        io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&gpath)));
        return 1;
    }
    if let (Some(sh), Some(s)) = (gshadow.as_mut(), s) {
        let line = s.join(&b':'.to_owned());
        match sh.iter().position(|l| is_data(l) && name_eq(l, &name)) {
            Some(i) => sh[i] = line,
            None => sh.push(line),
        }
        let old = old_gshadow.unwrap_or_default();
        if !write_backup(&spath, &old) || !write_lines(&spath, sh) {
            io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&spath)));
            return 1;
        }
    }
    0
}
