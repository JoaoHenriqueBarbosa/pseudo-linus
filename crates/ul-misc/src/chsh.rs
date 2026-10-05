//! `chsh` do shadow 4.17 (Debian 13).
//!
//! Troca o shell de login em `/etc/passwd` (sob `-R`, com o prefixo; o `chroot` real não existe no
//! sandbox). Sem `-s`, faz a pergunta interativa lendo a resposta do stdin. Roda como root: sem PAM nem
//! senha. O travamento do original é ignorado.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{FileType, sys};

use crate::groupmgmt::{Spec, fields, is_data, join, name_eq, parse, parse_id, read_lines, usage};
use crate::usermgmt::{valid_field, write_with_backup};
use crate::util::io;

const USAGE: &str = "Usage: chsh [options] [LOGIN]\n\nOptions:\n  -h, --help                    display this help message and exit\n  -R, --root CHROOT_DIR         directory to chroot into\n  -s, --shell SHELL             new login shell for the user account\n\n";

/// Linhas do stdin (sem a quebra), lidas de uma vez.
pub(crate) fn stdin_lines() -> Vec<Vec<u8>> {
    let d = io::read_path(b"/dev/stdin").unwrap_or_default();
    let mut v: Vec<Vec<u8>> = d.split(|b| *b == b'\n').map(<[u8]>::to_vec).collect();
    if v.last().is_some_and(Vec::is_empty) {
        v.pop();
    }
    v
}

/// Índice da linha do usuário em `passwd`: o nome dado ou o dono do uid efetivo.
pub(crate) fn find_user(p: &str, passwd: &[Vec<u8>], name: Option<&Vec<u8>>, ppath: &[u8]) -> Result<usize, i32> {
    match name {
        Some(n) => passwd.iter().position(|l| is_data(l) && name_eq(l, n)).ok_or_else(|| {
            io::eprint(format!(
                "{p}: user '{}' does not exist in {}\n",
                io::lossy(n),
                io::lossy(ppath)
            ));
            1
        }),
        None => {
            let uid = sys::current().geteuid() as u64;
            passwd
                .iter()
                .position(|l| is_data(l) && fields(l).get(2).and_then(|f| parse_id(f)) == Some(uid))
                .ok_or_else(|| {
                    io::eprint(format!("{p}: Cannot determine your user name.\n"));
                    1
                })
        }
    }
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    const P: &str = "chsh";
    let argv = io::args_bytes(args);
    let spec: Spec = &[(b'h', "help", false), (b'R', "root", true), (b's', "shell", true)];
    let Some(o) = parse(P, &argv, spec) else {
        return usage(USAGE, 1);
    };
    if o.has(b'h') {
        return usage(USAGE, 0);
    }
    if o.rest.len() > 1 {
        return usage(USAGE, 1);
    }
    let prefix = o.get(b'R').unwrap_or_default();
    let ppath = join(&prefix, "/etc/passwd");
    let passwd = match read_lines(&ppath) {
        Ok(p) => p,
        Err(_) => {
            io::eprint(format!("{P}: cannot open {}\n", io::lossy(&ppath)));
            return 1;
        }
    };
    let idx = match find_user(P, &passwd, o.rest.first(), b"/etc/passwd") {
        Ok(i) => i,
        Err(c) => return c,
    };
    let mut pf: Vec<Vec<u8>> = fields(&passwd[idx]).iter().map(|x| x.to_vec()).collect();
    while pf.len() < 7 {
        pf.push(Vec::new());
    }
    let login = io::lossy(&pf[0]);

    let shell: Vec<u8> = match o.get(b's') {
        Some(s) => s,
        None => {
            let _ = io::stdout().write_all(
                format!(
                    "Changing the login shell for {login}\nEnter the new value, or press ENTER for the default\n\tLogin Shell [{}]: ",
                    io::lossy(&pf[6])
                )
                .as_bytes(),
            );
            match stdin_lines().into_iter().next() {
                Some(l) if !l.is_empty() => l,
                _ => pf[6].clone(),
            }
        }
    };

    let shown = io::lossy(&shell);
    if !valid_field(&shell) || shell.iter().any(|b| *b == b',' || *b == b'=' || b.is_ascii_control()) {
        io::eprint(format!("{P}: Invalid entry: {shown}\n"));
        return 1;
    }
    if shell.first() != Some(&b'/') && !shell.is_empty() {
        io::eprint(format!("{P}: Shell must be a full path name.\n"));
        return 1;
    }
    if !shell.is_empty() {
        let real = join(&prefix, &shown);
        match sys::stat(&real) {
            Err(_) => {
                io::eprint(format!("{P}: '{shown}' does not exist.\n"));
                return 1;
            }
            Ok(st) => {
                if st.file_type() == FileType::Directory || (st.mode & 0o111) == 0 {
                    io::eprint(format!("{P}: '{shown}' is not executable.\n"));
                    return 1;
                }
            }
        }
        let listed = read_lines(&join(&prefix, "/etc/shells"))
            .map(|l| l.iter().any(|s| is_data(s) && *s == shell))
            .unwrap_or(false);
        if !listed {
            io::eprint(format!("{P}: Warning: '{shown}' is not listed in /etc/shells.\n"));
        }
    }

    pf[6] = shell;
    let mut new_passwd = passwd.clone();
    new_passwd[idx] = pf.join(&b':');
    if !write_with_backup(&ppath, &passwd, &new_passwd) {
        io::eprint(format!("{P}: failure while writing changes to /etc/passwd\n"));
        return 1;
    }
    0
}
