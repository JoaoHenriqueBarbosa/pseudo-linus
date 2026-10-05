//! `chfn` do shadow 4.17 (Debian 13).
//!
//! Troca o campo GECOS (nome, sala, telefones e outros) em `/etc/passwd` (sob `-R`, com o prefixo).
//! Roda como root: sem PAM nem senha, e pode mudar qualquer campo. Sem opções de campo, faz as
//! perguntas interativas lendo o stdin. O `-h` é o telefone residencial; a ajuda é `-u`/`--help`,
//! como no original.

use std::ffi::OsString;
use std::io::Write;

use crate::chsh::{find_user, stdin_lines};
use crate::groupmgmt::{Spec, fields, join, parse, read_lines, usage};
use crate::usermgmt::write_with_backup;
use crate::util::io;

const USAGE: &str = "Usage: chfn [options] [LOGIN]\n\nOptions:\n  -f, --full-name FULL_NAME     change user's full name\n  -h, --home-phone HOME_PHONE   change user's home phone number\n  -o, --other OTHER_INFO        change user's other GECOS information\n  -r, --room ROOM_NUMBER        change user's room number\n  -R, --root CHROOT_DIR         directory to chroot into\n  -u, --help                    display this help message and exit\n  -w, --work-phone WORK_PHONE   change user's work phone number\n\n";

/// `valid_field(s, ":,=")` do shadow: sem `:`, `,`, `=` nem caracteres de controle.
fn valid(s: &[u8]) -> bool {
    !s.iter().any(|b| *b == b':' || *b == b',' || *b == b'=' || b.is_ascii_control())
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    const P: &str = "chfn";
    let argv = io::args_bytes(args);
    let spec: Spec = &[
        (b'f', "full-name", true),
        (b'h', "home-phone", true),
        (b'o', "other", true),
        (b'r', "room", true),
        (b'R', "root", true),
        (b'u', "help", false),
        (b'w', "work-phone", true),
    ];
    let Some(o) = parse(P, &argv, spec) else {
        return usage(USAGE, 1);
    };
    if o.has(b'u') {
        return usage(USAGE, 0);
    }
    if o.rest.len() > 1 {
        return usage(USAGE, 1);
    }
    // Validação na ordem em que as opções aparecem, como o getopt do original.
    for (k, v) in &o.vals {
        let v = v.clone().unwrap_or_default();
        let shown = io::lossy(&v);
        let ok = valid(&v);
        let msg = match *k {
            b'f' => format!("{P}: invalid name: \"{shown}\"\n"),
            b'r' => format!("{P}: invalid room number: \"{shown}\"\n"),
            b'w' => format!("{P}: invalid work phone: \"{shown}\"\n"),
            b'h' => format!("{P}: invalid home phone: \"{shown}\"\n"),
            b'o' => format!("{P}: invalid name: \"{shown}\"\n"),
            _ => continue,
        };
        if !ok {
            io::eprint(msg);
            return 1;
        }
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

    // GECOS: nome,sala,telefone comercial,telefone residencial[,outros (com vírgulas)].
    let mut parts = pf[4].splitn(5, |b| *b == b',').map(<[u8]>::to_vec);
    let mut full = parts.next().unwrap_or_default();
    let mut room = parts.next().unwrap_or_default();
    let mut work = parts.next().unwrap_or_default();
    let mut home = parts.next().unwrap_or_default();
    let mut other = parts.next().unwrap_or_default();

    let has_field = [b'f', b'h', b'o', b'r', b'w'].iter().any(|c| o.has(*c));
    if has_field {
        if let Some(v) = o.get(b'f') {
            full = v;
        }
        if let Some(v) = o.get(b'r') {
            room = v;
        }
        if let Some(v) = o.get(b'w') {
            work = v;
        }
        if let Some(v) = o.get(b'h') {
            home = v;
        }
        if let Some(v) = o.get(b'o') {
            other = v;
        }
    } else {
        let mut answers = stdin_lines().into_iter();
        let _ = io::stdout().write_all(
            format!(
                "Changing the user information for {}\nEnter the new value, or press ENTER for the default\n",
                io::lossy(&pf[0])
            )
            .as_bytes(),
        );
        let prompts: [(&str, &mut Vec<u8>); 5] = [
            ("Full Name", &mut full),
            ("Room Number", &mut room),
            ("Work Phone", &mut work),
            ("Home Phone", &mut home),
            ("Other", &mut other),
        ];
        for (label, slot) in prompts {
            let _ = io::stdout()
                .write_all(format!("\t{label} [{}]: ", io::lossy(slot)).as_bytes());
            if let Some(a) = answers.next() {
                if a.as_slice() == b"none" {
                    slot.clear();
                } else if !a.is_empty() {
                    if !valid(&a) {
                        let msg = match label {
                            "Full Name" | "Other" => "invalid name",
                            "Room Number" => "invalid room number",
                            "Work Phone" => "invalid work phone",
                            _ => "invalid home phone",
                        };
                        io::eprint(format!("{P}: {msg}: \"{}\"\n", io::lossy(&a)));
                        return 1;
                    }
                    *slot = a;
                }
            }
        }
    }

    let mut gecos = Vec::new();
    for (i, part) in [&full, &room, &work, &home].iter().enumerate() {
        if i > 0 {
            gecos.push(b',');
        }
        gecos.extend_from_slice(part);
    }
    if !other.is_empty() {
        gecos.push(b',');
        gecos.extend_from_slice(&other);
    }
    pf[4] = gecos;
    let mut new_passwd = passwd.clone();
    new_passwd[idx] = pf.join(&b':');
    if !write_with_backup(&ppath, &passwd, &new_passwd) {
        io::eprint(format!("{P}: failure while writing changes to /etc/passwd\n"));
        return 1;
    }
    0
}
