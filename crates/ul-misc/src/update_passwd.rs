//! `update-passwd` do base-passwd 3.6.7 (Debian 13).
//!
//! Compara o `passwd` e o `group` do sistema com os arquivos mestres de
//! `/usr/share/base-passwd`: acrescenta as entradas de sistema que faltam, corrige o número das
//! que divergem e remove as de número baixo (menor que 100) que o mestre não conhece. O travamento
//! do original é ignorado (sandbox). Ajustes que exigem `chown` de arquivos de usuário não são feitos.
//!
//! Papéis das opções, como no original: `-p`/`-g` são os mestres, `-P`/`-G`/`-S` são os arquivos
//! do sistema.

use std::ffi::OsString;
use std::io::Write;

use crate::groupmgmt::{
    fields, is_data, name_eq, parse, read_lines, usage, write_backup, write_lines, Spec,
};
use crate::util::io;

const USAGE: &str = "Usage: update-passwd [OPTION]...\n\n  -p, --passwd-master=file  Use file as the master account list\n  -g, --group-master=file   Use file as the master group list\n  -P, --passwd=file         Use file as the system passwd file\n  -S, --shadow=file         Use file as the system shadow file\n  -G, --group=file          Use file as the system group file\n  -s, --sanity-check        Only perform sanity-checks\n  -v, --verbose             Show details about what we are doing (recommended)\n  -n, --dry-run             Just say what we would do but do nothing\n  -L, --no-locking          Don't try to lock files\n  -h, --help                Display this information and exit\n  -V, --version             Show version number and exit\n\n File locations used:\n   master passwd: /usr/share/base-passwd/passwd.master\n   master group : /usr/share/base-passwd/group.master\n   system passwd: /etc/passwd\n   system shadow: /etc/shadow\n   system group : /etc/group\n\nReport bugs to the Debian bug tracking system, package \"base-passwd\".\n\n";

const VERSION: &str = "update-passwd 3.6.7\n";

/// Entradas de sistema com número abaixo deste limite somem se o mestre não as conhece.
const SYSTEM_ID_LIMIT: i64 = 100;

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| update_passwd(args))
}

fn id_of(line: &[u8], idx: usize) -> Option<i64> {
    let f = fields(line);
    let t = std::str::from_utf8(f.get(idx)?).ok()?;
    t.parse().ok()
}

/// Resultado da sincronização de um arquivo com o seu mestre.
struct Outcome {
    changes: usize,
    removed: Vec<Vec<u8>>,
}

/// Sincroniza o arquivo do sistema com o mestre. O número fica na coluna 2 nos dois arquivos.
fn sync(kind: &str, master: &[Vec<u8>], sys: &mut Vec<Vec<u8>>, show: bool, out: &mut String) -> Outcome {
    let mut res = Outcome { changes: 0, removed: Vec::new() };
    for m in master.iter().filter(|l| is_data(l)) {
        let name = fields(m)[0].to_vec();
        let want = id_of(m, 2);
        match sys.iter().position(|l| is_data(l) && name_eq(l, &name)) {
            Some(pos) => {
                if id_of(&sys[pos], 2) != want {
                    let mut f: Vec<Vec<u8>> = fields(&sys[pos]).iter().map(|x| x.to_vec()).collect();
                    if f.len() > 2 {
                        f[2] = fields(m)[2].to_vec();
                        sys[pos] = f.join(&b':');
                        res.changes += 1;
                    }
                }
            }
            None => {
                if sys.iter().any(|l| is_data(l) && id_of(l, 2) == want) {
                    continue;
                }
                if show {
                    out.push_str(&format!(
                        "Adding {kind} \"{}\" ({})\n",
                        io::lossy(&name),
                        want.map_or_else(|| "?".to_string(), |v| v.to_string())
                    ));
                }
                sys.push(m.clone());
                res.changes += 1;
            }
        }
    }
    let mut i = 0;
    while i < sys.len() {
        let line = &sys[i];
        if is_data(line) {
            let name = fields(line)[0].to_vec();
            let known = master.iter().any(|l| is_data(l) && name_eq(l, &name));
            if let Some(id) = id_of(line, 2) {
                if !known && id < SYSTEM_ID_LIMIT {
                    if show {
                        out.push_str(&format!("Removing {kind} \"{}\" ({id})\n", io::lossy(&name)));
                    }
                    res.removed.push(name);
                    res.changes += 1;
                    sys.remove(i);
                    continue;
                }
            }
        }
        i += 1;
    }
    res
}

fn update_passwd(args: &[OsString]) -> i32 {
    const P: &str = "update-passwd";
    let argv = io::args_bytes(args);
    let spec: Spec = &[
        (b'p', "passwd-master", true),
        (b'g', "group-master", true),
        (b'P', "passwd", true),
        (b'S', "shadow", true),
        (b'G', "group", true),
        (b's', "sanity-check", false),
        (b'v', "verbose", false),
        (b'n', "dry-run", false),
        (b'L', "no-locking", false),
        (b'h', "help", false),
        (b'V', "version", false),
    ];
    let Some(o) = parse(P, &argv, spec) else {
        io::eprint("Internal error: getopt_long returned unexpected value '?'\n".to_string());
        return 1;
    };
    if o.has(b'h') {
        return usage(USAGE, 0);
    }
    if o.has(b'V') {
        let _ = io::stdout().write_all(VERSION.as_bytes());
        return 0;
    }
    let pmaster_path =
        o.get(b'p').unwrap_or_else(|| b"/usr/share/base-passwd/passwd.master".to_vec());
    let gmaster_path =
        o.get(b'g').unwrap_or_else(|| b"/usr/share/base-passwd/group.master".to_vec());
    let passwd_path = o.get(b'P').unwrap_or_else(|| b"/etc/passwd".to_vec());
    let shadow_path = o.get(b'S').unwrap_or_else(|| b"/etc/shadow".to_vec());
    let group_path = o.get(b'G').unwrap_or_else(|| b"/etc/group".to_vec());
    let dry = o.has(b'n');
    let sanity = o.has(b's');
    let show = o.has(b'v') || dry;

    // Falha no primeiro arquivo ilegível: mestre do passwd, mestre do group, depois o sistema.
    let load = |what: &str, path: &[u8]| match read_lines(path) {
        Ok(l) => Ok(l),
        Err(e) => {
            io::eprint(format!(
                "Error opening {what} file {}: {}\n",
                io::lossy(path),
                e.message()
            ));
            Err(2)
        }
    };
    let pmaster = match load("passwd", &pmaster_path) {
        Ok(v) => v,
        Err(c) => return c,
    };
    let gmaster = match load("group", &gmaster_path) {
        Ok(v) => v,
        Err(c) => return c,
    };
    let mut pw = match load("passwd", &passwd_path) {
        Ok(v) => v,
        Err(c) => return c,
    };
    let mut gr = match load("group", &group_path) {
        Ok(v) => v,
        Err(c) => return c,
    };
    if sanity {
        return 0;
    }

    let mut out = String::new();
    let old_gr = gr.clone();
    let old_pw = pw.clone();
    // Grupos antes dos usuários: os usuários novos referenciam o grupo principal.
    let g = sync("group", &gmaster, &mut gr, show, &mut out);
    let u = sync("user", &pmaster, &mut pw, show, &mut out);
    let total = g.changes + u.changes;

    if dry {
        if total > 0 {
            out.push_str(&format!("Would commit {total} changes\n"));
        }
        let _ = io::stdout().write_all(out.as_bytes());
        return if total > 0 { 2 } else { 0 };
    }
    if total == 0 {
        let _ = io::stdout().write_all(out.as_bytes());
        return 0;
    }
    out.push_str(&format!("{total} changes have been made, rewriting files\n"));
    let _ = io::stdout().write_all(out.as_bytes());

    if g.changes > 0 && (!write_backup(&group_path, &old_gr) || !write_lines(&group_path, &gr)) {
        io::eprint(format!("{P}: cannot write {}\n", io::lossy(&group_path)));
        return 1;
    }
    if u.changes > 0 && (!write_backup(&passwd_path, &old_pw) || !write_lines(&passwd_path, &pw)) {
        io::eprint(format!("{P}: cannot write {}\n", io::lossy(&passwd_path)));
        return 1;
    }
    // Quem saiu do passwd sai também do sombra, se ele existir.
    if !u.removed.is_empty() {
        if let Ok(shadow) = read_lines(&shadow_path) {
            let kept: Vec<Vec<u8>> = shadow
                .iter()
                .filter(|l| !(is_data(l) && u.removed.iter().any(|n| name_eq(l, n))))
                .cloned()
                .collect();
            if kept.len() != shadow.len() && (!write_backup(&shadow_path, &shadow) || !write_lines(&shadow_path, &kept)) {
                io::eprint(format!("{P}: cannot write {}\n", io::lossy(&shadow_path)));
                return 1;
            }
        }
    }
    0
}
