//! `passwd` do shadow 4.17 (Debian 13).
//!
//! Faz as operações que não exigem `crypt(3)`: `-S`, `-a -S`, `-l`, `-u`, `-d`, `-e`, `-i`, `-n`,
//! `-x`, `-w`. Edita `/etc/shadow` (ou `/etc/passwd`, quando não há sombra), sob `-R`/`-P` com o
//! prefixo. O travamento do original é ignorado (sandbox). A troca interativa de senha falha como
//! o original falharia sem terminal.

use std::ffi::OsString;
use std::io::Write;

use sysabi::sys;
use ul_common::ctype::parse_i64 as parse_long;
use ul_common::time::civil_from_days;

use crate::groupmgmt::{
    fields, is_data, name_eq, parse, read_lines, usage, write_backup, write_lines, Spec,
};
use crate::util::io;

const USAGE: &str = "Usage: passwd [options] [LOGIN]\n\nOptions:\n  -a, --all                     report password status on all accounts\n  -d, --delete                  delete the password for the named account\n  -e, --expire                  force expire the password for the named account\n  -h, --help                    display this help message and exit\n  -k, --keep-tokens             change password only if expired\n  -i, --inactive INACTIVE       set password inactive after expiration\n                                to INACTIVE\n  -l, --lock                    lock the password of the named account\n  -n, --mindays MIN_DAYS        set minimum number of days before password\n                                change to MIN_DAYS\n  -q, --quiet                   quiet mode\n  -r, --repository REPOSITORY   change password in REPOSITORY repository\n  -R, --root CHROOT_DIR         directory to chroot into\n  -P, --prefix PREFIX_DIR       directory prefix\n  -S, --status                  report password status on the named account\n  -u, --unlock                  unlock the password of the named account\n  -w, --warndays WARN_DAYS      set expiration warning days to WARN_DAYS\n  -x, --maxdays MAX_DAYS        set maximum number of days before password\n                                change to MAX_DAYS\n  -s, --stdin                   read new token from stdin\n\n";

/// Códigos de saída do `passwd.c`.
const E_NOPERM: i32 = 1;
const E_USAGE: i32 = 2;
const E_FAILURE: i32 = 3;
const E_BAD_ARG: i32 = 6;

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| passwd(args))
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

/// `pw_status` do shadow: `NP` sem senha, `L` bloqueada, `P` com senha.
fn pw_status(pass: &[u8]) -> &'static str {
    if pass.is_empty() {
        "NP"
    } else if pass[0] == b'*' || pass[0] == b'!' {
        "L"
    } else {
        "P"
    }
}

/// `%m/%d/%Y` para um número de dias desde a época.
fn mdy(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{m:02}/{d:02}/{y}")
}

fn today() -> i64 {
    sys::current()
        .clock_gettime(sysabi::Clock::Realtime)
        .map(|t| t.sec / 86400)
        .unwrap_or(0)
}

/// Linha de status (`-S`) de uma entrada do passwd.
fn status_line(pw: &[u8], shadow: &[Vec<u8>]) -> String {
    let f = fields(pw);
    let name = io::lossy(f.first().copied().unwrap_or(b""));
    let pass = f.get(1).copied().unwrap_or(b"");
    if pass == b"x" {
        if let Some(s) = shadow.iter().find(|l| is_data(l) && name_eq(l, f[0])) {
            let v = owned(s, 9);
            let lst = num_field(&v, 2);
            return format!(
                "{name} {} {} {} {} {} {}\n",
                pw_status(&v[1]),
                mdy(lst.max(-1)),
                num_field(&v, 3),
                num_field(&v, 4),
                num_field(&v, 5),
                num_field(&v, 6)
            );
        }
    }
    format!("{name} {}\n", pw_status(pass))
}

fn passwd(args: &[OsString]) -> i32 {
    const P: &str = "passwd";
    let argv = io::args_bytes(args);
    let spec: Spec = &[
        (b'a', "all", false),
        (b'd', "delete", false),
        (b'e', "expire", false),
        (b'h', "help", false),
        (b'k', "keep-tokens", false),
        (b'i', "inactive", true),
        (b'l', "lock", false),
        (b'n', "mindays", true),
        (b'q', "quiet", false),
        (b'r', "repository", true),
        (b'R', "root", true),
        (b'P', "prefix", true),
        (b'S', "status", false),
        (b'u', "unlock", false),
        (b'w', "warndays", true),
        (b'x', "maxdays", true),
        (b's', "stdin", false),
    ];
    let Some(o) = parse(P, &argv, spec) else {
        return usage(USAGE, E_BAD_ARG);
    };
    // O prefixo é conferido antes de tudo: no oráculo, `-P` exige caminho absoluto.
    if let Some(pfx) = o.get(b'P') {
        if pfx.first() != Some(&b'/') {
            io::eprint(format!("{P}: prefix must be an absolute path\n"));
            return E_FAILURE;
        }
    }
    if o.has(b'h') {
        return usage(USAGE, 0);
    }
    // Argumentos numéricos, na ordem em que foram dados.
    let mut aging: Vec<(usize, i64)> = Vec::new();
    for (k, v) in &o.vals {
        let idx = match *k {
            b'n' => 3,
            b'x' => 4,
            b'w' => 5,
            b'i' => 6,
            _ => continue,
        };
        let arg = v.as_deref().unwrap_or(b"");
        match parse_long(arg) {
            Some(n) => aging.push((idx, n)),
            None => {
                io::eprint(format!("{P}: invalid numeric argument '{}'\n", io::lossy(arg)));
                return usage(USAGE, E_BAD_ARG);
            }
        }
    }
    let (aflg, dflg, eflg, lflg, uflg, sflg) =
        (o.has(b'a'), o.has(b'd'), o.has(b'e'), o.has(b'l'), o.has(b'u'), o.has(b'S'));
    if o.rest.len() > 1 {
        return usage(USAGE, E_USAGE);
    }
    if aflg && (!sflg || !o.rest.is_empty()) {
        return usage(USAGE, E_USAGE);
    }
    if sflg && (dflg || eflg || lflg || uflg || !aging.is_empty()) {
        return usage(USAGE, E_USAGE);
    }
    if usize::from(dflg) + usize::from(lflg) + usize::from(uflg) > 1 {
        return usage(USAGE, E_USAGE);
    }
    if let Some(repo) = o.get(b'r') {
        io::eprint(format!("{P}: repository {} not supported\n", io::lossy(&repo)));
        return E_NOPERM;
    }
    let quiet = o.has(b'q');
    let prefix = o.get(b'P').or_else(|| o.get(b'R')).unwrap_or_default();
    // Como o chage: o prefixo é concatenado sem normalizar.
    let raw = |p: &str| {
        let mut v = prefix.clone();
        if !v.is_empty() {
            v.push(b'/');
        }
        v.extend_from_slice(p.as_bytes());
        v
    };
    let ppath = raw("/etc/passwd");
    let spath = raw("/etc/shadow");
    let mut passwd = match read_lines(&ppath) {
        Ok(p) => p,
        Err(_) => {
            io::eprint(format!("{P}: cannot open {}\n", io::lossy(&ppath)));
            return E_NOPERM;
        }
    };
    let mut shadow = read_lines(&spath).ok();

    if aflg {
        let empty = Vec::new();
        let sh = shadow.as_ref().unwrap_or(&empty);
        let mut out = String::new();
        for l in passwd.iter().filter(|l| is_data(l) && l[0] != b'+' && l[0] != b'-') {
            out.push_str(&status_line(l, sh));
        }
        let _ = io::stdout().write_all(out.as_bytes());
        return 0;
    }

    // Sem LOGIN vale o usuário do processo, que no sandbox é o dono do uid 0.
    let name: Vec<u8> = match o.rest.first() {
        Some(n) => n.clone(),
        None => passwd
            .iter()
            .find(|l| is_data(l) && fields(l).get(2).is_some_and(|f| *f == b"0"))
            .map(|l| fields(l)[0].to_vec())
            .unwrap_or_else(|| b"root".to_vec()),
    };
    let Some(pi) = passwd.iter().position(|l| is_data(l) && name_eq(l, &name)) else {
        io::eprint(format!("{P}: user '{}' does not exist\n", io::lossy(&name)));
        return E_NOPERM;
    };

    if sflg {
        let empty = Vec::new();
        let out = status_line(&passwd[pi], shadow.as_ref().unwrap_or(&empty));
        let _ = io::stdout().write_all(out.as_bytes());
        return 0;
    }

    let modes = dflg || eflg || lflg || uflg || !aging.is_empty();
    if !modes {
        // Troca de senha: sem terminal e sem crypt(3) o original falha na conversa do PAM.
        let _ = io::stdout().write_all(b"New password: ");
        io::eprint(format!(
            "\n{P}: Authentication token manipulation error\n{P}: password unchanged\n"
        ));
        return E_NOPERM;
    }

    let mut pwf = owned(&passwd[pi], 7);
    let shadowed = pwf[1] == b"x" && shadow.is_some();
    let mut si = None;
    let mut sf: Vec<Vec<u8>> = Vec::new();
    if shadowed {
        let sh = shadow.as_ref().unwrap();
        match sh.iter().position(|l| is_data(l) && name_eq(l, &name)) {
            Some(i) => {
                si = Some(i);
                sf = owned(&sh[i], 9);
            }
            None => {
                io::eprint(format!(
                    "{P}: user '{}' does not exist in {}\n",
                    io::lossy(&name),
                    io::lossy(&spath)
                ));
                return E_NOPERM;
            }
        }
    }

    // Campo da senha: o do sombra quando há, senão o do passwd.
    let mut pass = if shadowed { sf[1].clone() } else { pwf[1].clone() };
    let mut update_age = false;
    if dflg {
        pass = Vec::new();
        update_age = true;
    }
    if uflg && pass.first() == Some(&b'!') {
        if pass.len() == 1 {
            io::eprint(format!(
                "{P}: unlocking the password would result in a passwordless account.\nYou should set a password with usermod -p to unlock the password of this account.\n"
            ));
            return E_NOPERM;
        }
        pass.remove(0);
    }
    if lflg && pass.first() != Some(&b'!') {
        pass.insert(0, b'!');
    }

    let old_passwd = passwd.clone();
    let old_shadow = shadow.clone();
    if shadowed {
        sf[1] = pass;
        for (i, n) in &aging {
            put_num(&mut sf, *i, *n);
        }
        if update_age {
            let d = today();
            sf[2] = if d == 0 { Vec::new() } else { d.to_string().into_bytes() };
        }
        if eflg {
            sf[2] = b"0".to_vec();
        }
        let line = sf.join(&b':');
        if let (Some(sh), Some(i)) = (shadow.as_mut(), si) {
            sh[i] = line;
        }
        let sh = shadow.as_ref().unwrap();
        if !write_backup(&spath, old_shadow.as_deref().unwrap_or(&[])) || !write_lines(&spath, sh) {
            io::eprint(format!("{P}: failure while writing changes to /etc/shadow\n"));
            return E_NOPERM;
        }
    } else {
        pwf[1] = pass;
        passwd[pi] = pwf.join(&b':');
        if !write_backup(&ppath, &old_passwd) || !write_lines(&ppath, &passwd) {
            io::eprint(format!("{P}: failure while writing changes to /etc/passwd\n"));
            return E_NOPERM;
        }
    }
    if !quiet {
        let _ = io::stdout().write_all(format!("{P}: password expiry information changed.\n").as_bytes());
    }
    0
}
