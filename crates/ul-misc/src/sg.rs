//! `newgrp` e `sg` do shadow 4.17 (Debian 13).
//!
//! Faz o parse dos argumentos, a busca do grupo em `/etc/group` e a checagem de pertencimento como o
//! original. O sandbox não tem `setgid`: trocar para um grupo diferente do atual falha como o original
//! num contêiner sem a capacidade (`setgid: Operation not permitted`); quando o grupo pedido já é o
//! atual, o shell (ou o comando do `sg -c`) é executado de verdade. Ficam de fora a senha de grupo e o
//! reset de ambiente do `newgrp -`.

use std::ffi::OsString;

use sysabi::{Errno, sys};

use crate::groupmgmt::{fields, gid_of, is_data, name_eq, parse_id, read_lines};
use crate::setsid::execvp;
use crate::usermgmt::valid_field;
use crate::util::io;
use crate::util::ul;

/// Entrada de `/etc/group`.
pub(crate) struct Grp {
    pub(crate) gid: u64,
    pub(crate) members: Vec<Vec<u8>>,
}

/// `getgrnam` e, se o texto for numérico, `getgrgid`, sobre `/etc/group`.
pub(crate) fn lookup_group(spec: &[u8]) -> Option<Grp> {
    let lines = read_lines(b"/etc/group").ok()?;
    let line = lines.iter().find(|l| is_data(l) && name_eq(l, spec)).or_else(|| {
        let g = parse_id(spec)?;
        lines.iter().find(|l| is_data(l) && gid_of(l) == Some(g))
    })?;
    let f = fields(line);
    if f.len() < 3 {
        return None;
    }
    Some(Grp {
        gid: parse_id(f[2])?,
        members: f
            .get(3)
            .map(|m| m.split(|b| *b == b',').filter(|x| !x.is_empty()).map(<[u8]>::to_vec).collect())
            .unwrap_or_default(),
    })
}

/// Campos de `/etc/passwd` do usuário com o uid dado.
pub(crate) fn passwd_by_uid(uid: u64) -> Option<Vec<Vec<u8>>> {
    let lines = read_lines(b"/etc/passwd").ok()?;
    lines
        .iter()
        .filter(|l| is_data(l))
        .map(|l| fields(l).iter().map(|x| x.to_vec()).collect::<Vec<_>>())
        .find(|f| f.len() >= 7 && parse_id(&f[2]) == Some(uid))
}

/// Executa o programa; só volta em erro, devolvendo o código de saída do original.
pub(crate) fn exec_or_fail(short: &str, msg: String, file: &[u8], argv: &[Vec<u8>]) -> i32 {
    let _ = io::flush_stdout();
    let e = execvp(file, argv);
    ul::warn(short, msg, e);
    if e == Errno::ENOENT { 127 } else { 126 }
}

fn base(path: &[u8]) -> Vec<u8> {
    path.rsplit(|b| *b == b'/').next().unwrap_or(path).to_vec()
}

pub fn newgrp_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args, false))
}

pub fn sg_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args, true))
}

fn run(args: &[OsString], is_sg: bool) -> i32 {
    let p = if is_sg { "sg" } else { "newgrp" };
    let argv = io::args_bytes(args);
    let usage_text = if is_sg {
        "Usage: sg group [[-c] command]\n"
    } else {
        "Usage: newgrp [-] [group]\n"
    };
    let sys = sys::current();
    let euid = sys.geteuid() as u64;
    let egid = sys.getegid() as u64;
    let Some(me) = passwd_by_uid(euid) else {
        io::eprint(format!("{p}: Cannot determine your user name.\n"));
        return 1;
    };

    let mut rest: Vec<Vec<u8>> = argv[1..].to_vec();
    let mut login = false;
    let group_arg: Option<Vec<u8>>;
    let mut command: Option<Vec<u8>> = None;
    if is_sg {
        if rest.is_empty() || rest[0].first() == Some(&b'-') {
            io::eprint(usage_text.to_string());
            return 1;
        }
        group_arg = Some(rest.remove(0));
        if !rest.is_empty() {
            if rest[0].as_slice() == b"-c" {
                if rest.len() != 2 {
                    io::eprint(usage_text.to_string());
                    return 1;
                }
                command = Some(rest.remove(1));
            } else if rest.len() == 1 {
                command = Some(rest.remove(0));
            } else {
                io::eprint(usage_text.to_string());
                return 1;
            }
        }
    } else {
        if rest.first().is_some_and(|a| a.as_slice() == b"-") {
            login = true;
            rest.remove(0);
        }
        if rest.len() > 1 || rest.first().is_some_and(|a| a.first() == Some(&b'-')) {
            io::eprint(usage_text.to_string());
            return 1;
        }
        group_arg = rest.first().cloned();
    }

    let gid = match &group_arg {
        None => parse_id(&me[3]).unwrap_or(egid),
        Some(g) => {
            let Some(grp) = lookup_group(g) else {
                io::eprint(format!("{p}: group '{}' does not exist\n", io::lossy(g)));
                return 1;
            };
            let member = grp.members.contains(&me[0]) || parse_id(&me[3]) == Some(grp.gid);
            if euid != 0 && !member {
                io::eprint("Permission denied.\n".to_string());
                return 1;
            }
            grp.gid
        }
    };
    if gid != egid {
        io::eprint("setgid: Operation not permitted\n".to_string());
        return 1;
    }

    let shell = if valid_field(&me[6]) && !me[6].is_empty() { me[6].clone() } else { b"/bin/sh".to_vec() };
    let mut name = base(&shell);
    if login {
        name.insert(0, b'-');
    }
    let mut exec_argv = vec![name];
    if let Some(c) = command {
        exec_argv.push(b"-c".to_vec());
        exec_argv.push(c);
    }
    let msg = format!("Cannot execute {}", io::lossy(&shell));
    exec_or_fail(p, msg, &shell, &exec_argv)
}
