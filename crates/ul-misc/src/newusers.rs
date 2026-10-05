//! `newusers` do shadow 4.17 (Debian 13).
//!
//! Lê linhas `nome:senha:uid:gid:gecos:home:shell` (de um arquivo ou do stdin) e cria ou atualiza as
//! contas em `/etc/passwd`, `/etc/shadow`, `/etc/group` e `/etc/gshadow` (sob `-R`/`-P`, com o
//! prefixo), tudo ou nada: com qualquer erro nenhuma mudança é gravada. Sem `crypt(3)` no sandbox só o
//! método `NONE` grava a senha; os demais recusam a linha. Ficam de fora a criação do diretório
//! pessoal (com o esqueleto), subuid/subgid e o travamento do original.

use std::ffi::OsString;

use crate::groupmgmt::{Spec, fields, is_data, join, name_eq, parse, parse_id, read_lines, usage, valid_name};
use crate::usermgmt::{today, valid_field, write_with_backup};
use crate::util::io;

const USAGE: &str = r#"Usage: newusers [options]

Options:
  -b, --badname                 allow bad names (DEPRECATED)
  -h, --help                    display this help message and exit
  -r, --system                  create system accounts
  -R, --root CHROOT_DIR         directory to chroot into

"#;

const METHODS: [&str; 7] = ["NONE", "DES", "MD5", "SHA256", "SHA512", "YESCRYPT", "BCRYPT"];

/// Próximo id livre: de baixo para cima no maior usado da faixa (ou na primeira lacuna); contas de
/// sistema descem a partir do teto.
fn find_free(used: &[u64], min: u64, max: u64, system: bool) -> Option<u64> {
    if system {
        return (min..=max).rev().find(|v| !used.contains(v));
    }
    if let Some(top) = used.iter().copied().filter(|v| (min..=max).contains(v)).max() {
        if top < max {
            return Some(top + 1);
        }
    }
    (min..=max).find(|v| !used.contains(v))
}

struct Groups {
    group: Vec<Vec<u8>>,
    gshadow: Option<Vec<Vec<u8>>>,
    used: Vec<u64>,
    min: u64,
    max: u64,
    system: bool,
}

impl Groups {
    /// Devolve o gid do grupo `name`, criando-o (com o gid pedido, se livre) quando não existe.
    fn ensure(&mut self, name: &[u8], wanted: Option<u64>) -> Option<u64> {
        if let Some(l) = self.group.iter().find(|l| is_data(l) && name_eq(l, name)) {
            return fields(l).get(2).and_then(|f| parse_id(f));
        }
        let gid = match wanted {
            Some(g) if !self.used.contains(&g) => g,
            _ => find_free(&self.used, self.min, self.max, self.system)?,
        };
        let mut line = name.to_vec();
        line.extend_from_slice(format!(":x:{gid}:").as_bytes());
        self.group.push(line);
        if let Some(gs) = self.gshadow.as_mut() {
            let mut l = name.to_vec();
            l.extend_from_slice(b":!::");
            gs.push(l);
        }
        self.used.push(gid);
        Some(gid)
    }
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    const P: &str = "newusers";
    let argv = io::args_bytes(args);
    let spec: Spec = &[
        (b'b', "badname", false),
        (b'h', "help", false),
        (b'r', "system", false),
        (b'R', "root", true),
    ];
    let Some(o) = parse(P, &argv, spec) else {
        return usage(USAGE, 1);
    };
    let mut badname = false;
    let mut system = false;
    let mut method = "SHA512".to_string();
    for (k, v) in &o.vals {
        let t = io::lossy(v.as_deref().unwrap_or(b"")).to_string();
        match *k {
            b'h' => return usage(USAGE, 0),
            b'b' => badname = true,
            b'r' => system = true,
            b'c' => {
                if !METHODS.iter().any(|m| *m == t) {
                    io::eprint(format!("{P}: unsupported crypt method: {t}\n"));
                    return usage(USAGE, 1);
                }
                method = t;
            }
            b's' => {
                if t.parse::<i64>().is_err() {
                    io::eprint(format!("{P}: invalid numeric argument '{t}'\n"));
                    return usage(USAGE, 1);
                }
            }
            _ => {}
        }
    }
    if o.rest.len() > 1 {
        return usage(USAGE, 1);
    }
    let data = match o.rest.first() {
        Some(f) => match io::read_path(f) {
            Ok(d) => d,
            Err(_) => {
                io::eprint(format!("{}: No such file or directory\n", io::lossy(f)));
                return 1;
            }
        },
        None => io::read_path(b"/dev/stdin").unwrap_or_default(),
    };

    let prefix = o.get(b'P').or_else(|| o.get(b'R')).unwrap_or_default();
    let ppath = join(&prefix, "/etc/passwd");
    let spath = join(&prefix, "/etc/shadow");
    let gpath = join(&prefix, "/etc/group");
    let gspath = join(&prefix, "/etc/gshadow");
    let Ok(passwd_old) = read_lines(&ppath) else {
        io::eprint(format!("{P}: cannot open {}\n", io::lossy(&ppath)));
        return 1;
    };
    let Ok(group_old) = read_lines(&gpath) else {
        io::eprint(format!("{P}: cannot open {}\n", io::lossy(&gpath)));
        return 1;
    };
    let shadow_old = read_lines(&spath).ok();
    let gshadow_old = read_lines(&gspath).ok();

    let (min, max) = if system { (100, 999) } else { (1000, 60000) };
    let mut passwd = passwd_old.clone();
    let mut shadow = shadow_old.clone();
    let mut used_uids: Vec<u64> = passwd
        .iter()
        .filter(|l| is_data(l))
        .filter_map(|l| fields(l).get(2).and_then(|f| parse_id(f)))
        .collect();
    let mut gr = Groups {
        used: group_old
            .iter()
            .filter(|l| is_data(l))
            .filter_map(|l| fields(l).get(2).and_then(|f| parse_id(f)))
            .collect(),
        group: group_old.clone(),
        gshadow: gshadow_old.clone(),
        min,
        max,
        system,
    };

    let mut lines: Vec<&[u8]> = data.split(|b| *b == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let mut errors = 0;
    for (i, line) in lines.iter().enumerate() {
        let n = i + 1;
        let f: Vec<&[u8]> = line.split(|b| *b == b':').collect();
        if f.len() != 7 {
            io::eprint(format!("{P}: line {n}: invalid line\n"));
            errors += 1;
            continue;
        }
        let name = f[0];
        let ok_name = if badname { !name.is_empty() && valid_field(name) } else { valid_name(name) };
        if !ok_name {
            io::eprint(format!("{P}: line {n}: invalid user name '{}'\n", io::lossy(name)));
            errors += 1;
            continue;
        }
        if method != "NONE" {
            io::eprint(format!(
                "{P}: line {n}: cannot encrypt the password with {method}: crypt(3) is not available\n"
            ));
            errors += 1;
            continue;
        }
        let existing = passwd.iter().position(|l| is_data(l) && name_eq(l, name));
        let uid = if f[2].is_empty() {
            let old = existing
                .and_then(|e| fields(&passwd[e]).get(2).and_then(|x| parse_id(x)));
            match old.or_else(|| find_free(&used_uids, min, max, system)) {
                Some(u) => u,
                None => {
                    io::eprint(format!("{P}: line {n}: can't find new UID\n"));
                    errors += 1;
                    continue;
                }
            }
        } else {
            match parse_id(f[2]) {
                Some(u) => u,
                None => {
                    io::eprint(format!("{P}: line {n}: invalid user ID '{}'\n", io::lossy(f[2])));
                    errors += 1;
                    continue;
                }
            }
        };
        let gid = if f[3].is_empty() {
            gr.ensure(name, Some(uid))
        } else if let Some(g) = parse_id(f[3]) {
            let known = gr
                .group
                .iter()
                .any(|l| is_data(l) && fields(l).get(2).and_then(|x| parse_id(x)) == Some(g));
            if known {
                Some(g)
            } else {
                gr.ensure(name, Some(g))
            }
        } else {
            gr.ensure(f[3], None)
        };
        let Some(gid) = gid else {
            io::eprint(format!("{P}: line {n}: can't create group\n"));
            errors += 1;
            continue;
        };

        let hashed = shadow.is_some();
        let mut entry = name.to_vec();
        entry.extend_from_slice(b":");
        entry.extend_from_slice(if hashed { &b"x"[..] } else { f[1] });
        entry.extend_from_slice(format!(":{uid}:{gid}:").as_bytes());
        entry.extend_from_slice(f[4]);
        entry.push(b':');
        entry.extend_from_slice(f[5]);
        entry.push(b':');
        entry.extend_from_slice(f[6]);
        match existing {
            Some(e) => passwd[e] = entry,
            None => passwd.push(entry),
        }
        used_uids.push(uid);
        if let Some(sh) = shadow.as_mut() {
            let mut s = name.to_vec();
            s.push(b':');
            s.extend_from_slice(f[1]);
            s.extend_from_slice(format!(":{}:0:99999:7:::", today()).as_bytes());
            match sh.iter().position(|l| is_data(l) && name_eq(l, name)) {
                Some(e) => sh[e] = s,
                None => sh.push(s),
            }
        }
    }
    if errors > 0 {
        io::eprint(format!("{P}: error detected, changes ignored\n"));
        return 1;
    }

    if passwd != passwd_old && !write_with_backup(&ppath, &passwd_old, &passwd) {
        io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&ppath)));
        return 1;
    }
    if let (Some(new), Some(old)) = (&shadow, &shadow_old) {
        if new != old && !write_with_backup(&spath, old, new) {
            io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&spath)));
            return 1;
        }
    }
    if gr.group != group_old && !write_with_backup(&gpath, &group_old, &gr.group) {
        io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&gpath)));
        return 1;
    }
    if let (Some(new), Some(old)) = (&gr.gshadow, &gshadow_old) {
        if new != old && !write_with_backup(&gspath, old, new) {
            io::eprint(format!("{P}: cannot rewrite {}\n", io::lossy(&gspath)));
            return 1;
        }
    }
    0
}
