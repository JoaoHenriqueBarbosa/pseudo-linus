//! `pidof` do sysvinit-utils 3.14 (no Debian 13 é `/usr/bin/pidof`, link pro `killall5`).
//!
//! Regras observadas no oráculo:
//!
//! - Cada nome casa com o `argv[0]` inteiro ou com o nome-base dele; quando o `argv[0]` tem espaço
//!   (o processo reescreveu a linha de comando, como o `sshd: user [priv]`), também com o nome do
//!   `stat`; processo sem linha de comando (thread de kernel) usa o nome do `stat` como `argv[0]`.
//! - Nome com `/`: o arquivo tem que existir (senão nada casa) e casa o processo cujo `exe` é o
//!   mesmo arquivo (dispositivo e inode) ou cujo `argv[0]` é exatamente o nome.
//! - `-x` também casa script: o primeiro argumento depois do `argv[0]` que não começa com `-` tem
//!   o nome-base igual ao procurado e igual ao nome do `stat`.
//! - Zumbi só com `-z`; o próprio pidof não é excluído.
//! - Saída: para cada nome, na ordem dos argumentos, os pids do maior pro menor (`-s`: só o
//!   primeiro), separados pelo primeiro caractere do `-d` (espaço por padrão), com `\n` no fim.
//! - Opção inválida ou sem argumento sai com 1 sem mensagem (o original usa `opterr = 0`).

use std::ffi::OsString;

use sysabi::{Ctx, Pid, sys};
use ul_misc::util::getopt::Getopt;
use ul_misc::util::io;

use crate::common::{self, out};
use crate::procfs::{self, Want};

const USAGE: &str = "pidof usage: [options] <program-name>\n\n -c           Return PIDs with the same root directory\n -d <sep>     Use the provided character as output separator\n -h           Display this help text\n -n           Avoid using stat system function on network shares\n -o <pid>     Omit results with a given PID\n -q           Quiet mode. Do not display output\n -s           Only return one PID\n -x           Return PIDs of shells running scripts with a matching name\n -z           List zombie and I/O waiting processes. May cause pidof to hang.\n\n";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn basename(s: &[u8]) -> &[u8] {
    s.rsplit(|b| *b == b'/').next().unwrap_or(s)
}

/// Dispositivo e inode de um caminho, seguindo links.
fn dev_ino(path: &[u8]) -> Option<(u64, u64)> {
    sys::stat(path).ok().map(|st| (st.dev, st.ino))
}

struct Candidate {
    pid: Pid,
    state: char,
    statname: Vec<u8>,
    argv0: Vec<u8>,
    argv1: Option<Vec<u8>>,
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let mut single = false;
    let mut scripts = false;
    let mut zombies = false;
    let mut quiet = false;
    let mut same_root = false;
    let mut sep: Vec<u8> = b" ".to_vec();
    let mut omit: Vec<Pid> = Vec::new();
    let mut g = Getopt::from_env(&argv[1..], "cd:hno:qsxz", &[]);
    while let Some(r) = g.next_opt() {
        let Ok(o) = r else { return 1 };
        match o.short() {
            Some('c') => same_root = true,
            Some('d') => {
                // Só o primeiro caractere vale (um argumento vazio vira o NUL).
                let a = o.arg.unwrap_or_default();
                sep = vec![a.first().copied().unwrap_or(0)];
            }
            Some('h') => {
                out(USAGE);
                return 0;
            }
            Some('n') => {}
            Some('o') => {
                for part in o.arg.unwrap_or_default().split(|b| *b == b',') {
                    if part == b"%PPID" {
                        omit.push(sys::current().getppid());
                    } else if let Some(v) = std::str::from_utf8(part).ok().and_then(common::parse_long) {
                        if v > 0 {
                            omit.push(v as Pid);
                        }
                    }
                }
            }
            Some('q') => quiet = true,
            Some('s') => single = true,
            Some('x') => scripts = true,
            Some('z') => zombies = true,
            _ => return 1,
        }
    }
    let names = g.operands();
    if names.is_empty() {
        return 1;
    }
    let snap = procfs::scan(Want { cmdline: true, ..Want::default() });
    let my_root = if same_root { dev_ino(b"/proc/self/root") } else { None };
    let mut cands = Vec::new();
    for p in &snap.procs {
        if omit.contains(&p.pid()) {
            continue;
        }
        if same_root {
            let theirs = dev_ino(format!("/proc/{}/root", p.pid()).as_bytes());
            if my_root.is_none() || theirs != my_root {
                continue;
            }
        }
        let args = p.args();
        let argv0 = match args.first() {
            Some(a) => a.clone(),
            None => p.comm().to_vec(),
        };
        let argv1 = args.iter().skip(1).find(|a| !a.starts_with(b"-")).cloned();
        cands.push(Candidate { pid: p.pid(), state: p.stat.state, statname: p.comm().to_vec(), argv0, argv1 });
    }
    let mut found: Vec<Pid> = Vec::new();
    for name in &names {
        let mut hits: Vec<Pid> = Vec::new();
        if name.contains(&b'/') {
            let Some(target) = dev_ino(name) else { continue };
            for c in &cands {
                if c.state == 'Z' && !zombies {
                    continue;
                }
                let exe = dev_ino(format!("/proc/{}/exe", c.pid).as_bytes());
                if exe == Some(target) || c.argv0 == *name {
                    hits.push(c.pid);
                }
            }
        } else {
            for c in &cands {
                if c.state == 'Z' && !zombies {
                    continue;
                }
                let base = basename(&c.argv0);
                let mut hit = base == name.as_slice() || c.argv0 == *name;
                if !hit && c.argv0.contains(&b' ') && c.statname == *name {
                    hit = true;
                }
                if !hit && scripts {
                    if let Some(a1) = &c.argv1 {
                        let b1 = basename(a1);
                        let short: Vec<u8> = b1.iter().take(15).copied().collect();
                        hit = b1 == name.as_slice() && c.statname == short;
                    }
                }
                if hit {
                    hits.push(c.pid);
                }
            }
        }
        hits.reverse();
        if single {
            hits.truncate(1);
        }
        found.extend(hits);
    }
    if found.is_empty() {
        return 1;
    }
    if !quiet {
        let mut line = Vec::new();
        for (i, pid) in found.iter().enumerate() {
            if i > 0 {
                line.extend_from_slice(&sep);
            }
            line.extend_from_slice(pid.to_string().as_bytes());
        }
        line.push(b'\n');
        out(line);
    }
    0
}
