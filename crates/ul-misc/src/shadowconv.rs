//! `pwconv`, `pwunconv`, `grpconv`, `grpunconv`, `chpasswd`, `chgpasswd` e `expiry` do shadow 4.17
//! (Debian 13).
//!
//! Editam `/etc/passwd`, `/etc/shadow`, `/etc/group` e `/etc/gshadow` (sob `-R`/`-P`, com o prefixo).
//! O travamento do original é ignorado (sandbox). Sem `crypt(3)` no sandbox, `chpasswd` e
//! `chgpasswd` só aceitam senha já cifrada (`-e`) ou `-c NONE`.

use std::ffi::OsString;

use sysabi::sys;

use crate::groupmgmt::{
    fields, is_data, join, name_eq, parse, read_lines, usage, write_backup, write_lines, Spec,
};
use crate::util::io;

const SPEC_BASIC: Spec = &[(b'h', "help", false), (b'R', "root", true), (b'P', "prefix", true)];

const SPEC_CHPASSWD: Spec = &[
    (b'c', "crypt-method", true),
    (b'e', "encrypted", false),
    (b'h', "help", false),
    (b'm', "md5", false),
    (b'R', "root", true),
    (b'P', "prefix", true),
    (b's', "sha-rounds", true),
];

fn conv_usage(p: &str) -> String {
    format!(
        "Usage: {p} [options]\n\nOptions:\n  -h, --help                    display this help message and exit\n  -R, --root CHROOT_DIR         directory to chroot into\n  -P, --prefix PREFIX_DIR       directory prefix\n\n"
    )
}

fn chpasswd_usage(p: &str) -> String {
    format!(
        "Usage: {p} [options]\n\nOptions:\n  -c, --crypt-method METHOD     the crypt method (one of NONE DES MD5 SHA256 SHA512 YESCRYPT)\n  -e, --encrypted               supplied passwords are encrypted\n  -h, --help                    display this help message and exit\n  -m, --md5                     encrypt the clear text password using\n                                the MD5 algorithm\n  -R, --root CHROOT_DIR         directory to chroot into\n  -P, --prefix PREFIX_DIR       directory prefix\n  -s, --sha-rounds              number of rounds for the SHA, BCRYPT\n                                or YESCRYPT crypt algorithms\n\n"
    )
}

fn expiry_usage() -> &'static str {
    "Usage: expiry {-c|-f}\n\nOptions:\n  -c, --check                   check the user's password expiration\n  -f, --force                   force password change if the user's password\n                                has expired\n  -h, --help                    display this help message and exit\n  -R, --root CHROOT_DIR         directory to chroot into\n  -P, --prefix PREFIX_DIR       directory prefix\n\n"
}

/// Copia os campos de uma linha, completando até `min` campos.
fn owned(line: &[u8], min: usize) -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = fields(line).iter().map(|x| x.to_vec()).collect();
    while v.len() < min {
        v.push(Vec::new());
    }
    v
}

fn with_field(line: &[u8], idx: usize, min: usize, val: &[u8]) -> Vec<u8> {
    let mut f = owned(line, min);
    f[idx] = val.to_vec();
    f.join(&b':')
}

fn field_of(line: &[u8], idx: usize) -> Vec<u8> {
    fields(line).get(idx).map(|f| f.to_vec()).unwrap_or_default()
}

fn prefix_of(o: &crate::groupmgmt::Opts) -> Vec<u8> {
    o.get(b'P').or_else(|| o.get(b'R')).unwrap_or_default()
}

fn fail_write(p: &str, name: &str) -> i32 {
    io::eprint(format!("{p}: failure while writing changes to /etc/{name}\n"));
    1
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Passwd,
    Group,
}

impl Kind {
    fn main_name(self) -> &'static str {
        match self {
            Kind::Passwd => "passwd",
            Kind::Group => "group",
        }
    }
    fn shadow_name(self) -> &'static str {
        match self {
            Kind::Passwd => "shadow",
            Kind::Group => "gshadow",
        }
    }
    /// Campo da senha no arquivo principal e no sombra.
    fn is_entry(self, line: &[u8]) -> bool {
        if !is_data(line) {
            return false;
        }
        !(self == Kind::Passwd && (line[0] == b'+' || line[0] == b'-'))
    }
}

fn parse_conv(p: &str, args: &[OsString]) -> Result<Vec<u8>, i32> {
    let argv = io::args_bytes(args);
    let Some(o) = parse(p, &argv, SPEC_BASIC) else {
        return Err(usage(&conv_usage(p), 2));
    };
    if o.has(b'h') {
        return Err(usage(&conv_usage(p), 0));
    }
    if !o.rest.is_empty() {
        return Err(usage(&conv_usage(p), 2));
    }
    Ok(prefix_of(&o))
}

fn convert(p: &str, kind: Kind, prefix: &[u8]) -> i32 {
    let mpath = join(prefix, &format!("/etc/{}", kind.main_name()));
    let spath = join(prefix, &format!("/etc/{}", kind.shadow_name()));
    let mut main = match read_lines(&mpath) {
        Ok(m) => m,
        Err(_) => {
            io::eprint(format!("{p}: cannot open {}\n", io::lossy(&mpath)));
            return 1;
        }
    };
    let old_shadow = read_lines(&spath).ok();
    let old_main = main.clone();
    let mut shadow: Vec<Vec<u8>> = Vec::new();
    // Entradas do sombra que continuam valendo, na ordem original.
    let existing: Vec<Vec<u8>> = old_shadow.clone().unwrap_or_default();
    let mut seen: Vec<Vec<u8>> = Vec::new();
    for l in &main {
        if kind.is_entry(l) {
            seen.push(field_of(l, 0));
        }
    }
    for l in &existing {
        if !is_data(l) {
            shadow.push(l.clone());
            continue;
        }
        let n = field_of(l, 0);
        let Some(m) = main.iter().find(|x| kind.is_entry(x) && name_eq(x, &n)) else {
            continue;
        };
        let pw = field_of(m, 1);
        if pw != b"x" {
            shadow.push(with_field(l, 1, if kind == Kind::Passwd { 9 } else { 4 }, &pw));
        } else {
            shadow.push(l.clone());
        }
    }
    for l in main.iter_mut() {
        if !kind.is_entry(l) {
            continue;
        }
        let n = field_of(l, 0);
        let pw = field_of(l, 1);
        let has = existing.iter().any(|x| is_data(x) && name_eq(x, &n));
        if !has {
            let mut e = n.clone();
            e.push(b':');
            e.extend_from_slice(&pw);
            match kind {
                Kind::Passwd => e.extend_from_slice(b"::0:99999:7:::"),
                Kind::Group => {
                    e.extend_from_slice(b"::");
                    e.extend_from_slice(&field_of(l, 3));
                }
            }
            shadow.push(e);
        }
        if pw != b"x" {
            *l = with_field(l, 1, if kind == Kind::Passwd { 7 } else { 4 }, b"x");
        }
    }
    if old_shadow.is_some_and(|o| !write_backup(&spath, &o)) {
        return fail_write(p, kind.shadow_name());
    }
    if !write_lines(&spath, &shadow) {
        return fail_write(p, kind.shadow_name());
    }
    if !write_backup(&mpath, &old_main) || !write_lines(&mpath, &main) {
        return fail_write(p, kind.main_name());
    }
    0
}

fn unconvert(p: &str, kind: Kind, prefix: &[u8]) -> i32 {
    let mpath = join(prefix, &format!("/etc/{}", kind.main_name()));
    let spath = join(prefix, &format!("/etc/{}", kind.shadow_name()));
    let Ok(shadow) = read_lines(&spath) else {
        // Sem arquivo sombra não há o que desfazer.
        return 0;
    };
    let mut main = match read_lines(&mpath) {
        Ok(m) => m,
        Err(_) => {
            io::eprint(format!("{p}: cannot open {}\n", io::lossy(&mpath)));
            return 1;
        }
    };
    let old_main = main.clone();
    for l in main.iter_mut() {
        if !kind.is_entry(l) {
            continue;
        }
        let n = field_of(l, 0);
        let Some(s) = shadow.iter().find(|x| is_data(x) && name_eq(x, &n)) else {
            continue;
        };
        let pw = field_of(s, 1);
        *l = with_field(l, 1, if kind == Kind::Passwd { 7 } else { 4 }, &pw);
    }
    if !write_backup(&mpath, &old_main) || !write_lines(&mpath, &main) {
        return fail_write(p, kind.main_name());
    }
    if sys::current().unlinkat(sysabi::Fd::CWD, &spath, sysabi::AtFlags::empty()).is_err() {
        io::eprint(format!("{p}: cannot delete {}\n", io::lossy(&spath)));
        return 1;
    }
    0
}

pub fn pwconv_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| match parse_conv("pwconv", args) {
        Ok(pfx) => convert("pwconv", Kind::Passwd, &pfx),
        Err(c) => c,
    })
}

pub fn pwunconv_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| match parse_conv("pwunconv", args) {
        Ok(pfx) => unconvert("pwunconv", Kind::Passwd, &pfx),
        Err(c) => c,
    })
}

pub fn grpconv_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| match parse_conv("grpconv", args) {
        Ok(pfx) => convert("grpconv", Kind::Group, &pfx),
        Err(c) => c,
    })
}

pub fn grpunconv_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| match parse_conv("grpunconv", args) {
        Ok(pfx) => unconvert("grpunconv", Kind::Group, &pfx),
        Err(c) => c,
    })
}

// ---------------------------------------------------------------- chpasswd / chgpasswd

pub fn chpasswd_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| chpasswd("chpasswd", Kind::Passwd, args))
}

pub fn chgpasswd_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| chpasswd("chgpasswd", Kind::Group, args))
}

fn chpasswd(p: &str, kind: Kind, args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let Some(o) = parse(p, &argv, SPEC_CHPASSWD) else {
        return usage(&chpasswd_usage(p), 1);
    };
    if o.has(b'h') {
        return usage(&chpasswd_usage(p), 0);
    }
    if !o.rest.is_empty() {
        return usage(&chpasswd_usage(p), 1);
    }
    let encrypted = o.has(b'e');
    if let Some(m) = o.get(b'c') {
        if encrypted && !o.has(b'm') {
            // -e e -c juntos: o original recusa.
        }
        if !matches!(&m[..], b"NONE" | b"DES" | b"MD5" | b"SHA256" | b"SHA512" | b"YESCRYPT") {
            io::eprint(format!("{p}: unsupported crypt method: {}\n", io::lossy(&m)));
            return usage(&chpasswd_usage(p), 1);
        }
    }
    if encrypted && (o.has(b'c') || o.has(b'm')) {
        io::eprint(format!("{p}: the -e and -c/-m flags are exclusive\n"));
        return usage(&chpasswd_usage(p), 1);
    }
    let plain_none = o.get(b'c').is_some_and(|m| m == b"NONE");
    if !encrypted && !plain_none {
        io::eprint(format!("{p}: this build cannot encrypt passwords; use -e or -c NONE\n"));
        return 1;
    }
    let prefix = prefix_of(&o);
    let mpath = join(&prefix, &format!("/etc/{}", kind.main_name()));
    let spath = join(&prefix, &format!("/etc/{}", kind.shadow_name()));
    let mut main = match read_lines(&mpath) {
        Ok(m) => m,
        Err(_) => {
            io::eprint(format!("{p}: cannot open {}\n", io::lossy(&mpath)));
            return 1;
        }
    };
    let mut shadow = read_lines(&spath).ok();
    let old_main = main.clone();
    let old_shadow = shadow.clone();
    let Ok(input) = io::read_stdin() else {
        return 1;
    };
    let mut errors = 0;
    let mut main_changed = false;
    let mut shadow_changed = false;
    let mut lineno = 0;
    let mut chunks: Vec<&[u8]> = input.split(|b| *b == b'\n').collect();
    if chunks.last().is_some_and(|l| l.is_empty()) {
        chunks.pop();
    }
    let what = if kind == Kind::Passwd { "user" } else { "group" };
    for line in chunks {
        lineno += 1;
        let Some(c) = line.iter().position(|b| *b == b':') else {
            io::eprint(format!("{p}: line {lineno}: missing new password\n"));
            errors += 1;
            continue;
        };
        let name = &line[..c];
        let pass = &line[c + 1..];
        if let Some(l) = main.iter_mut().find(|l| kind.is_entry(l) && name_eq(l, name)) {
            let in_shadow = shadow
                .as_ref()
                .and_then(|s| s.iter().position(|x| is_data(x) && name_eq(x, name)));
            match (in_shadow, shadow.as_mut()) {
                (Some(i), Some(s)) => {
                    s[i] = with_field(&s[i], 1, if kind == Kind::Passwd { 9 } else { 4 }, pass);
                    shadow_changed = true;
                }
                _ => {
                    *l = with_field(l, 1, if kind == Kind::Passwd { 7 } else { 4 }, pass);
                    main_changed = true;
                }
            }
        } else {
            io::eprint(format!(
                "{p}: line {lineno}: {what} '{}' does not exist\n",
                io::lossy(name)
            ));
            errors += 1;
        }
    }
    if errors > 0 {
        io::eprint(format!("{p}: error detected, changes ignored\n"));
        return 1;
    }
    if shadow_changed {
        if let (Some(s), Some(old)) = (shadow.as_ref(), old_shadow.as_ref()) {
            if !write_backup(&spath, old) || !write_lines(&spath, s) {
                return fail_write(p, kind.shadow_name());
            }
        }
    }
    if main_changed && (!write_backup(&mpath, &old_main) || !write_lines(&mpath, &main)) {
        return fail_write(p, kind.main_name());
    }
    0
}

// ---------------------------------------------------------------- expiry

pub fn expiry_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| expiry(args))
}

fn expiry(args: &[OsString]) -> i32 {
    const P: &str = "expiry";
    let argv = io::args_bytes(args);
    let spec: Spec = &[
        (b'c', "check", false),
        (b'f', "force", false),
        (b'h', "help", false),
        (b'R', "root", true),
        (b'P', "prefix", true),
    ];
    let Some(o) = parse(P, &argv, spec) else {
        return usage(expiry_usage(), 1);
    };
    if o.has(b'h') {
        return usage(expiry_usage(), 0);
    }
    if !o.rest.is_empty() || o.has(b'c') == o.has(b'f') {
        return usage(expiry_usage(), 1);
    }
    // Checar o vencimento exige o uid real e a data de hoje, que o sandbox ainda não expõe.
    io::eprint(format!("{P}: unknown user: 0\n"));
    1
}
