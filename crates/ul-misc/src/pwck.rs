//! `pwck` do shadow 4.17 (Debian 13), só o modo de leitura (`-r`): confere `/etc/passwd` (ou o
//! arquivo dado) sem travar nem reescrever nada.
//!
//! Confere o número de campos, entradas duplicadas, o grupo primário, o diretório pessoal e o
//! shell. Em modo leitura toda pergunta de remoção é respondida com "No". Sem `-r` o programa
//! precisaria do lock do banco e de escrita, então recusa como o original faz sem permissão.

use std::ffi::OsString;

use sysabi::{Errno, sys};

use crate::util::io;

fn usage() -> String {
    "Usage: pwck [options] [passwd [shadow]]\n\nOptions:\n  -h, --help                    display this help message and exit\n  -q, --quiet                   report errors only\n  -r, --read-only               display errors and warnings\n                                but do not change files\n  -R, --root CHROOT_DIR         directory to chroot into\n  -s, --sort                    sort entries by UID\n\n"
        .to_string()
}

fn exists(path: &[u8]) -> bool {
    !matches!(sys::read_file(path), Err(Errno::ENOENT))
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let mut read_only = false;
    let mut files: Vec<Vec<u8>> = Vec::new();
    for a in &argv[1..] {
        match a.as_slice() {
            b"-r" | b"--read-only" => read_only = true,
            b"-q" | b"--quiet" | b"-s" | b"--sort" => {}
            b"-h" | b"--help" => {
                io::eprint(usage());
                return 0;
            }
            s if s.starts_with(b"-") && s.len() > 1 => {
                io::eprint(format!("pwck: invalid option -- '{}'\n", io::lossy(&s[1..])));
                io::eprint(usage());
                return 2;
            }
            s => files.push(s.to_vec()),
        }
    }
    if files.len() > 2 {
        io::eprint(usage());
        return 2;
    }
    if !read_only {
        io::eprint("pwck: cannot lock /etc/passwd; try again later.\n".to_string());
        return 1;
    }
    let pw_path: Vec<u8> = files.first().cloned().unwrap_or_else(|| b"/etc/passwd".to_vec());
    let gr_path = b"/etc/group";
    let pw = match sys::read_file(&pw_path) {
        Ok(d) => d,
        Err(_) => {
            io::eprint(format!(
                "pwck: cannot open {}\n",
                io::lossy(&pw_path)
            ));
            return 3;
        }
    };
    let gids: Vec<u64> = sys::read_file(gr_path)
        .map(|g| {
            g.split(|b| *b == b'\n')
                .filter_map(|l| {
                    let f: Vec<&[u8]> = l.split(|b| *b == b':').collect();
                    f.get(2)
                        .and_then(|x| std::str::from_utf8(x).ok())
                        .and_then(|x| x.parse().ok())
                })
                .collect()
        })
        .unwrap_or_default();

    let mut errors = 0;
    let mut seen: Vec<Vec<u8>> = Vec::new();
    for line in pw.split(|b| *b == b'\n') {
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let shown = io::lossy(line);
        let f: Vec<&[u8]> = line.split(|b| *b == b':').collect();
        if f.len() != 7 {
            errors += 1;
            io::eprint(format!(
                "invalid password file entry\ndelete line '{shown}'? No\n"
            ));
            continue;
        }
        let name = io::lossy(f[0]);
        if seen.iter().any(|n| n.as_slice() == f[0]) {
            errors += 1;
            io::eprint(format!(
                "duplicate password entry\ndelete line '{shown}'? No\n"
            ));
            continue;
        }
        seen.push(f[0].to_vec());
        let gid = std::str::from_utf8(f[3]).ok().and_then(|x| x.parse::<u64>().ok());
        match gid {
            Some(g) if !gids.contains(&g) => {
                errors += 1;
                io::eprint(format!("user '{name}': no group {g}\n"));
            }
            None => {
                errors += 1;
                io::eprint(format!(
                    "invalid password file entry\ndelete line '{shown}'? No\n"
                ));
                continue;
            }
            _ => {}
        }
        if !f[5].is_empty() && !exists(f[5]) {
            errors += 1;
            io::eprint(format!(
                "user '{name}': directory '{}' does not exist\n",
                io::lossy(f[5])
            ));
        }
        if !f[6].is_empty() && !exists(f[6]) {
            errors += 1;
            io::eprint(format!(
                "user '{name}': program '{}' does not exist\n",
                io::lossy(f[6])
            ));
        }
    }
    if errors > 0 {
        io::eprint("pwck: no changes\n".to_string());
        2
    } else {
        0
    }
}
