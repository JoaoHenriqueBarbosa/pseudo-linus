//! `update-passwd` do base-passwd 3.6.7 (Debian 13).
//!
//! Compara `/etc/passwd` e `/etc/group` com os arquivos mestres de
//! `/usr/share/base-passwd` e acrescenta as entradas de sistema que faltam. O travamento do
//! original é ignorado (sandbox). Ajustes que exigem `chown` de arquivos de usuário não são feitos.

use std::ffi::OsString;
use std::io::Write;

use crate::groupmgmt::{
    fields, is_data, name_eq, parse, read_lines, usage, write_backup, write_lines, Spec,
};
use crate::util::io;

const USAGE: &str = "Usage: update-passwd [OPTION]...\n\nOptions:\n  -p, --passwd=FILE          Use FILE instead of /etc/passwd.\n  -g, --group=FILE           Use FILE instead of /etc/group.\n  -P, --passwd-master=FILE   Use FILE instead of /usr/share/base-passwd/passwd.master.\n  -G, --group-master=FILE    Use FILE instead of /usr/share/base-passwd/group.master.\n  -s, --sanity-check         Only check the files, do not change anything.\n  -n, --dry-run              Show what would be done, but change nothing.\n  -v, --verbose              Be verbose.\n  -h, --help                 Show this help.\n  -V, --version              Show the version.\n";

const VERSION: &str = "update-passwd 3.6.7\n";

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| update_passwd(args))
}

fn id_of(line: &[u8], idx: usize) -> Option<i64> {
    let f = fields(line);
    let t = std::str::from_utf8(f.get(idx)?).ok()?;
    t.parse().ok()
}

/// Sincroniza um arquivo com o seu mestre. `uid_idx` é a coluna do número (2 nos dois arquivos).
/// Devolve se o arquivo mudou.
fn sync(kind: &str, label: &str, master: &[Vec<u8>], sys: &mut Vec<Vec<u8>>, check: bool, out: &mut String) -> bool {
    let mut changed = false;
    for m in master.iter().filter(|l| is_data(l)) {
        let name = fields(m)[0].to_vec();
        let want = id_of(m, 2);
        let lname = io::lossy(&name);
        match sys.iter().find(|l| is_data(l) && name_eq(l, &name)) {
            Some(cur) => {
                let have = id_of(cur, 2);
                if have != want {
                    out.push_str(&format!(
                        "Warning: {kind} `{lname}' has {label} {}, but should have {}.\n",
                        have.map_or_else(|| "?".to_string(), |v| v.to_string()),
                        want.map_or_else(|| "?".to_string(), |v| v.to_string())
                    ));
                }
            }
            None => {
                let id = want.map_or_else(|| "?".to_string(), |v| v.to_string());
                if check {
                    out.push_str(&format!("Warning: {kind} `{lname}' ({label} {id}) is missing.\n"));
                    continue;
                }
                let taken = sys.iter().any(|l| is_data(l) && id_of(l, 2) == want);
                if taken {
                    out.push_str(&format!(
                        "Warning: {label} {id} is already in use, not adding {kind} `{lname}'.\n"
                    ));
                    continue;
                }
                out.push_str(&format!("Adding {kind} `{lname}' ({label} {id})...\n"));
                sys.push(m.clone());
                changed = true;
            }
        }
    }
    changed
}

fn update_passwd(args: &[OsString]) -> i32 {
    const P: &str = "update-passwd";
    let argv = io::args_bytes(args);
    let spec: Spec = &[
        (b'p', "passwd", true),
        (b'g', "group", true),
        (b'P', "passwd-master", true),
        (b'G', "group-master", true),
        (b's', "sanity-check", false),
        (b'n', "dry-run", false),
        (b'v', "verbose", false),
        (b'h', "help", false),
        (b'V', "version", false),
    ];
    let Some(o) = parse(P, &argv, spec) else {
        return usage(USAGE, 1);
    };
    if o.has(b'h') {
        return usage(USAGE, 0);
    }
    if o.has(b'V') {
        let _ = io::stdout().write_all(VERSION.as_bytes());
        return 0;
    }
    if !o.rest.is_empty() {
        return usage(USAGE, 1);
    }
    let passwd_path = o.get(b'p').unwrap_or_else(|| b"/etc/passwd".to_vec());
    let group_path = o.get(b'g').unwrap_or_else(|| b"/etc/group".to_vec());
    let pmaster_path =
        o.get(b'P').unwrap_or_else(|| b"/usr/share/base-passwd/passwd.master".to_vec());
    let gmaster_path =
        o.get(b'G').unwrap_or_else(|| b"/usr/share/base-passwd/group.master".to_vec());
    let check = o.has(b's') || o.has(b'n');

    let load = |path: &[u8]| match read_lines(path) {
        Ok(l) => Ok(l),
        Err(e) => {
            io::eprint(format!("{P}: {}: {}\n", io::lossy(path), e.message()));
            Err(1)
        }
    };
    let (pmaster, gmaster, mut pw, mut gr) = match (
        load(&pmaster_path),
        load(&gmaster_path),
        load(&passwd_path),
        load(&group_path),
    ) {
        (Ok(a), Ok(b), Ok(c), Ok(d)) => (a, b, c, d),
        _ => return 1,
    };

    let mut out = String::new();
    // Grupos antes dos usuários: os usuários novos referenciam o grupo principal.
    let old_gr = gr.clone();
    let old_pw = pw.clone();
    let gchanged = sync("group", "GID", &gmaster, &mut gr, check, &mut out);
    let pchanged = sync("user", "UID", &pmaster, &mut pw, check, &mut out);
    let _ = io::stdout().write_all(out.as_bytes());

    if check {
        return 0;
    }
    if gchanged && (!write_backup(&group_path, &old_gr) || !write_lines(&group_path, &gr)) {
        io::eprint(format!("{P}: cannot write {}\n", io::lossy(&group_path)));
        return 1;
    }
    if pchanged && (!write_backup(&passwd_path, &old_pw) || !write_lines(&passwd_path, &pw)) {
        io::eprint(format!("{P}: cannot write {}\n", io::lossy(&passwd_path)));
        return 1;
    }
    0
}
