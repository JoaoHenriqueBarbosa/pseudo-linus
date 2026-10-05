//! `dpkg-statoverride` do dpkg 1.22 (Debian 13): mantém o banco de sobreposições de dono, grupo e
//! modo em `--admindir`/`statoverride` (uma linha `dono grupo modo caminho` por entrada, com o modo
//! em octal).
//!
//! Comandos: `--add`, `--remove` e `--list`. Opções: `--admindir`, `--update`, `--force`, `--quiet`,
//! `--help` e `--version`.
//!
//! Divergências conhecidas: o `--list` sai na ordem do arquivo e o arquivo `lock` do admindir não é
//! criado.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{AtFlags, Ctx, Fd, OFlags, RenameFlags, sys};

use crate::util::io;

const PROG: &str = "dpkg-statoverride";

const USAGE: &str = "Usage: dpkg-statoverride [<option>...] <command>

Commands:
  --add <owner> <group> <mode> <path>
                           add a new entry into the database.
  --remove <path>          remove an entry from the database.
  --list [<glob-pattern>]  list current overrides in the database.
  --help                   show this help message.
  --version                show the version.

Options:
  --admindir <directory>   set the directory with the statoverride file.
  --update                 immediately update <path> permissions.
  --force                  force an action even if a sanity check fails.
  --quiet                  quiet operation, minimal output.
";

fn version_text() -> String {
    format!(
        "Debian {PROG} version 1.22.22 (amd64).\n\
This is free software; see the GNU General Public License version 2 or\n\
later for copying conditions. There is NO warranty.\n"
    )
}

type R<T> = Result<T, i32>;

fn out(s: &str) {
    let mut o = io::stdout();
    let _ = o.write_all(s.as_bytes());
}

fn ohshit(msg: &str) -> i32 {
    let _ = io::flush_stdout();
    io::eprint(format!("{PROG}: error: {msg}\n"));
    2
}

fn warning(msg: &str) {
    let _ = io::flush_stdout();
    io::eprint(format!("{PROG}: warning: {msg}\n"));
}

fn badusage(msg: &str) -> i32 {
    let _ = io::flush_stdout();
    io::eprint(format!(
        "{PROG}: error: {msg}\n\nUse '{PROG} --help' for program usage information.\n"
    ));
    2
}

#[derive(Clone, Debug)]
struct Entry {
    owner: String,
    group: String,
    mode: u32,
    path: String,
}

impl Entry {
    fn line(&self) -> String {
        format!("{} {} {:o} {}", self.owner, self.group, self.mode, self.path)
    }
}

fn glob(p: &[u8], s: &[u8]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some(b'*') => (0..=s.len()).any(|i| glob(&p[1..], &s[i..])),
        Some(b'?') => !s.is_empty() && glob(&p[1..], &s[1..]),
        Some(b'\\') if p.len() > 1 => !s.is_empty() && s[0] == p[1] && glob(&p[2..], &s[1..]),
        Some(&c) => !s.is_empty() && s[0] == c && glob(&p[1..], &s[1..]),
    }
}

/// Procura `name` em `/etc/passwd` ou `/etc/group` (campo 3 é o id).
fn lookup_id(file: &[u8], name: &str) -> Option<u32> {
    let data = sys::read_file(file).ok()?;
    let text = String::from_utf8_lossy(&data).into_owned();
    for l in text.lines() {
        let f: Vec<&str> = l.split(':').collect();
        if f.len() >= 3 && f[0] == name {
            return f[2].parse().ok();
        }
    }
    None
}

fn parse_id(kind: &str, s: &str) -> R<u32> {
    if let Some(n) = s.strip_prefix('#') {
        return n.parse().map_err(|_| ohshit(&format!("{kind} '{s}' does not exist")));
    }
    let file: &[u8] = if kind == "user" { b"/etc/passwd" } else { b"/etc/group" };
    lookup_id(file, s).ok_or_else(|| ohshit(&format!("{kind} '{s}' does not exist")))
}

struct State {
    admindir: String,
    quiet: bool,
    force: bool,
    update: bool,
}

impl State {
    fn db(&self) -> String {
        format!("{}/statoverride", self.admindir)
    }

    fn load(&self) -> R<Vec<Entry>> {
        let file = self.db();
        let data = match sys::read_file(file.as_bytes()) {
            Ok(d) => d,
            Err(sysabi::Errno::ENOENT) => return Ok(Vec::new()),
            Err(e) => return Err(ohshit(&format!("cannot open statoverride file '{file}': {}", e.message()))),
        };
        let text = String::from_utf8_lossy(&data).into_owned();
        let mut v = Vec::new();
        for l in text.lines() {
            let mut p = l.splitn(4, ' ');
            let (Some(o), Some(g), Some(m), Some(path)) = (p.next(), p.next(), p.next(), p.next()) else {
                return Err(ohshit(&format!("syntax error in statoverride file")));
            };
            let mode = u32::from_str_radix(m, 8)
                .map_err(|_| ohshit(&format!("syntax error in statoverride file")))?;
            v.push(Entry { owner: o.to_string(), group: g.to_string(), mode, path: path.to_string() });
        }
        Ok(v)
    }

    fn save(&self, list: &[Entry]) -> R<()> {
        let mut s = String::new();
        for e in list {
            s.push_str(&e.line());
            s.push('\n');
        }
        let file = self.db();
        let new = format!("{file}-new");
        let old = format!("{file}-old");
        let fd = sys::open(new.as_bytes(), OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, 0o644)
            .map_err(|e| ohshit(&format!("cannot open new statoverride file '{new}': {}", e.message())))?;
        let w = sys::write_all(fd, s.as_bytes());
        let _ = sys::close(fd);
        if let Err(e) = w {
            return Err(ohshit(&format!("cannot write new statoverride file '{new}': {}", e.message())));
        }
        let sc = sys::current();
        if sys::lstat(file.as_bytes()).is_ok() {
            let _ = sc.unlinkat(Fd::CWD, old.as_bytes(), AtFlags::empty());
            let _ = sc.linkat(Fd::CWD, file.as_bytes(), Fd::CWD, old.as_bytes(), AtFlags::empty());
        }
        sc.renameat2(Fd::CWD, new.as_bytes(), Fd::CWD, file.as_bytes(), RenameFlags::empty())
            .map_err(|e| ohshit(&format!("cannot install new statoverride file '{file}': {}", e.message())))
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| match run(args) {
        Ok(c) => c,
        Err(c) => c,
    })
}

fn run(args: &[OsString]) -> R<i32> {
    let argv: Vec<String> = io::args_bytes(args)
        .iter()
        .skip(1)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect();
    let mut st = State { admindir: "/var/lib/dpkg".to_string(), quiet: false, force: false, update: false };
    let mut action: Option<&'static str> = None;
    let mut i = 0usize;
    while i < argv.len() {
        let a = argv[i].clone();
        if a == "--" {
            i += 1;
            break;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        i += 1;
        let (name, inline) = match a.strip_prefix("--") {
            Some(l) => match l.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (l.to_string(), None),
            },
            None => {
                if a == "-?" {
                    out(USAGE);
                    return Ok(0);
                }
                return Err(badusage(&format!("unknown option {a}")));
            }
        };
        let mut set = |act: &'static str| -> R<()> {
            if let Some(p) = action {
                return Err(badusage(&format!("conflicting actions --{act} and --{p}")));
            }
            action = Some(act);
            Ok(())
        };
        match name.as_str() {
            "help" => {
                out(USAGE);
                return Ok(0);
            }
            "version" => {
                out(&version_text());
                return Ok(0);
            }
            "add" => set("add")?,
            "remove" => set("remove")?,
            "list" => set("list")?,
            "admindir" => {
                st.admindir = match inline {
                    Some(v) => v,
                    None => {
                        if i < argv.len() {
                            i += 1;
                            argv[i - 1].clone()
                        } else {
                            return Err(badusage("--admindir option takes a value"));
                        }
                    }
                }
            }
            "update" => st.update = true,
            "force" => st.force = true,
            "quiet" => st.quiet = true,
            _ => return Err(badusage(&format!("unknown option --{name}"))),
        }
    }
    let rest: Vec<String> = argv[i..].to_vec();
    let Some(action) = action else {
        return Err(badusage("need an action option"));
    };
    let mut list = st.load()?;
    match action {
        "list" => {
            if rest.len() > 1 {
                return Err(badusage("--list takes at most one argument"));
            }
            let pat = rest.first().cloned().unwrap_or_else(|| "*".to_string());
            let mut found = false;
            let mut s = String::new();
            for e in &list {
                if glob(pat.as_bytes(), e.path.as_bytes()) {
                    found = true;
                    s.push_str(&e.line());
                    s.push('\n');
                }
            }
            out(&s);
            Ok(if found { 0 } else { 1 })
        }
        "remove" => {
            if rest.len() != 1 {
                return Err(badusage("--remove needs a single argument"));
            }
            let path = clean(&rest[0]);
            let Some(pos) = list.iter().position(|e| e.path == path) else {
                if !st.quiet {
                    warning("no override present");
                }
                return Ok(0);
            };
            list.remove(pos);
            st.save(&list)?;
            Ok(0)
        }
        _ => {
            if rest.len() != 4 {
                return Err(badusage("--add needs four arguments"));
            }
            let (owner, group, modestr) = (&rest[0], &rest[1], &rest[2]);
            let path = clean(&rest[3]);
            let uid = parse_id("user", owner)?;
            let gid = parse_id("group", group)?;
            let mode = match u32::from_str_radix(modestr, 8) {
                Ok(m) if m <= 0o7777 && !modestr.is_empty() => m,
                _ => return Err(badusage(&format!("mode '{modestr}' is not valid"))),
            };
            let entry = Entry { owner: owner.clone(), group: group.clone(), mode, path: path.clone() };
            if let Some(pos) = list.iter().position(|e| e.path == path) {
                if !st.force {
                    return Err(ohshit(&format!("An override for '{path}' already exists, aborting")));
                }
                warning(&format!(
                    "An override for '{path}' already exists, but --force specified so will be ignored"
                ));
                list.remove(pos);
            }
            let exists = sys::lstat(path.as_bytes()).is_ok();
            if st.update && !exists {
                warning(&format!("--update given but '{path}' does not exist"));
            }
            list.push(entry);
            st.save(&list)?;
            if st.update && exists {
                let sc = sys::current();
                let _ = sc.fchownat(Fd::CWD, path.as_bytes(), Some(uid), Some(gid), AtFlags::SYMLINK_NOFOLLOW);
                let _ = sc.fchmodat(Fd::CWD, path.as_bytes(), mode, AtFlags::empty());
            }
            Ok(0)
        }
    }
}

/// Tira barras repetidas e a barra final do caminho.
fn clean(p: &str) -> String {
    let mut out = String::new();
    let mut prev_slash = false;
    for c in p.chars() {
        if c == '/' {
            if prev_slash {
                continue;
            }
            prev_slash = true;
        } else {
            prev_slash = false;
        }
        out.push(c);
    }
    while out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleaning() {
        assert_eq!(clean("//a//b/"), "/a/b");
        assert_eq!(clean("/"), "/");
    }

    #[test]
    fn line_format() {
        let e = Entry { owner: "root".into(), group: "root".into(), mode: 0o4755, path: "/x".into() };
        assert_eq!(e.line(), "root root 4755 /x");
    }
}
