//! `dpkg-divert` do dpkg 1.22 (Debian 13): mantém o banco de desvios (`diversions`, três linhas por
//! desvio: arquivo original, nome de destino e pacote, ou `:` pra desvio local) em `--admindir`.
//!
//! Comandos: `--add` (padrão), `--remove`, `--list`, `--listpackage` e `--truename`. Opções:
//! `--admindir`, `--instdir`, `--root`, `--divert`, `--package`, `--local`, `--rename`, `--test`,
//! `--quiet`, `--help` e `--version`.
//!
//! Divergências conhecidas: o `--list` sai na ordem do arquivo (o original percorre a tabela hash do
//! dpkg), e o arquivo `lock` do admindir não é criado.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{AtFlags, Ctx, Fd, FileType, OFlags, RenameFlags, sys};
use ul_common::fnmatch::{Bytes, Flags, fnmatch};

use crate::util::io;

const PROG: &str = "dpkg-divert";

const USAGE: &str = "Usage: dpkg-divert [<option>...] <command>

Commands:
  [--add] <file>                   add a diversion.
  --remove <file>                  remove the diversion.
  --list [<glob-pattern>]          show file diversions.
  --listpackage <file>             show what package diverts the file.
  --truename <file>                return the diverted file.

Options:
  --admindir <directory>           set the directory with the diversions file.
  --instdir <directory>            set the root directory, but not the admin dir.
  --root <directory>               set the directory of the root filesystem.
  --divert <divert-to>             the name used by other packages' versions.
  --local                          all packages' versions are diverted.
  --package <package>              name of the package whose copy of <file>
                                     will not be diverted.
  --quiet                          quiet operation, minimal output.
  --rename                         actually move the file aside (or back).
  --test                           can't touch the file system, dry-run.
  --help                           show this help message.
  --version                        show the version.

When adding, default is --local and --divert <original>.distrib.
When removing, --package or --local and --divert must match if specified.
Package preinst/postrm scripts should always specify --package and --divert.
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

fn badusage(msg: &str) -> i32 {
    let _ = io::flush_stdout();
    io::eprint(format!(
        "{PROG}: error: {msg}\n\nUse '{PROG} --help' for program usage information.\n"
    ));
    2
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Diversion {
    original: String,
    to: String,
    /// `None` é desvio local.
    pkg: Option<String>,
}

fn describe(orig: &str, to: Option<&str>, pkg: Pkg<'_>) -> String {
    match (to, pkg) {
        (None, Pkg::Any) => format!("any diversion of {orig}"),
        (Some(t), Pkg::Local) => format!("local diversion of {orig} to {t}"),
        (Some(t), Pkg::Name(p)) => format!("diversion of {orig} to {t} by {p}"),
        (None, Pkg::Local) => format!("local diversion of {orig}"),
        (None, Pkg::Name(p)) => format!("diversion of {orig} by {p}"),
        (Some(t), Pkg::Any) => format!("diversion of {orig} to {t}"),
    }
}

#[derive(Clone, Copy)]
enum Pkg<'a> {
    Any,
    Local,
    Name(&'a str),
}

fn describe_div(d: &Diversion) -> String {
    let p = match &d.pkg {
        None => Pkg::Local,
        Some(n) => Pkg::Name(n.as_str()),
    };
    describe(&d.original, Some(&d.to), p)
}

struct State {
    admindir: String,
    instdir: String,
    quiet: bool,
    rename: bool,
    test: bool,
}

impl State {
    fn db(&self) -> String {
        format!("{}/diversions", self.admindir)
    }

    fn load(&self) -> R<Vec<Diversion>> {
        let file = self.db();
        let data = match sys::read_file(file.as_bytes()) {
            Ok(d) => d,
            Err(sysabi::Errno::ENOENT) => return Ok(Vec::new()),
            Err(e) => return Err(ohshit(&format!("cannot open diversions file '{file}': {}", e.message()))),
        };
        let text = String::from_utf8_lossy(&data).into_owned();
        let lines: Vec<&str> = text.lines().collect();
        let mut v = Vec::new();
        for c in lines.chunks(3) {
            if c.len() < 3 {
                return Err(ohshit(&format!("diversions file '{file}' is corrupt")));
            }
            v.push(Diversion {
                original: c[0].to_string(),
                to: c[1].to_string(),
                pkg: if c[2] == ":" { None } else { Some(c[2].to_string()) },
            });
        }
        Ok(v)
    }

    fn save(&self, list: &[Diversion]) -> R<()> {
        let mut s = String::new();
        for d in list {
            s.push_str(&format!("{}\n{}\n{}\n", d.original, d.to, d.pkg.as_deref().unwrap_or(":")));
        }
        let file = self.db();
        let new = format!("{file}-new");
        let old = format!("{file}-old");
        let fd = sys::open(new.as_bytes(), OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, 0o644)
            .map_err(|e| ohshit(&format!("cannot create new diversions file '{new}': {}", e.message())))?;
        let w = sys::write_all(fd, s.as_bytes());
        let _ = sys::close(fd);
        if let Err(e) = w {
            return Err(ohshit(&format!("cannot write new diversions file '{new}': {}", e.message())));
        }
        let sc = sys::current();
        if sys::lstat(file.as_bytes()).is_ok() {
            let _ = sc.unlinkat(Fd::CWD, old.as_bytes(), AtFlags::empty());
            let _ = sc.linkat(Fd::CWD, file.as_bytes(), Fd::CWD, old.as_bytes(), AtFlags::empty());
        }
        sc.renameat2(Fd::CWD, new.as_bytes(), Fd::CWD, file.as_bytes(), RenameFlags::empty())
            .map_err(|e| ohshit(&format!("cannot install new diversions file '{file}': {}", e.message())))
    }

    fn fs(&self, p: &str) -> String {
        format!("{}{}", self.instdir, p)
    }

    /// Move `from` pra `to` no sistema de arquivos (com `--rename`), tolerando a ausência de `from`.
    fn move_file(&self, from: &str, to: &str) -> R<()> {
        let f = self.fs(from);
        let t = self.fs(to);
        let sc = sys::current();
        match sys::lstat(f.as_bytes()) {
            Err(_) => return Ok(()),
            Ok(st) if st.file_type() == FileType::Directory => {
                return Err(ohshit(&format!("rename: cannot move directory '{from}'")));
            }
            Ok(_) => {}
        }
        if sys::lstat(t.as_bytes()).is_ok() {
            return Err(ohshit(&format!(
                "rename: target '{to}' already exists, not renaming '{from}'"
            )));
        }
        if self.test {
            if !self.quiet {
                out(&format!("Test mode: would rename '{from}' to '{to}'\n"));
            }
            return Ok(());
        }
        sc.renameat2(Fd::CWD, f.as_bytes(), Fd::CWD, t.as_bytes(), RenameFlags::NOREPLACE)
            .map_err(|e| ohshit(&format!("cannot rename '{from}' to '{to}': {}", e.message())))
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| match run(args) {
        Ok(()) => 0,
        Err(c) => c,
    })
}

fn run(args: &[OsString]) -> R<()> {
    let argv: Vec<String> = io::args_bytes(args)
        .iter()
        .skip(1)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect();
    let mut st = State {
        admindir: "/var/lib/dpkg".to_string(),
        instdir: String::new(),
        quiet: false,
        rename: false,
        test: false,
    };
    let mut action: Option<&'static str> = None;
    let mut divert_to: Option<String> = None;
    let mut package: Option<String> = None;
    let mut local = false;

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
                    return Ok(());
                }
                return Err(badusage(&format!("unknown option {a}")));
            }
        };
        let mut value = |inline: Option<String>| -> R<String> {
            match inline {
                Some(v) => Ok(v),
                None => {
                    if i < argv.len() {
                        i += 1;
                        Ok(argv[i - 1].clone())
                    } else {
                        Err(badusage(&format!("--{name} option takes a value")))
                    }
                }
            }
        };
        let set = |act: &'static str, cur: &mut Option<&'static str>| -> R<()> {
            if let Some(p) = *cur {
                return Err(badusage(&format!("conflicting actions --{act} and --{p}")));
            }
            *cur = Some(act);
            Ok(())
        };
        match name.as_str() {
            "help" => {
                out(USAGE);
                return Ok(());
            }
            "version" => {
                out(&version_text());
                return Ok(());
            }
            "add" => set("add", &mut action)?,
            "remove" => set("remove", &mut action)?,
            "list" => set("list", &mut action)?,
            "listpackage" => set("listpackage", &mut action)?,
            "truename" => set("truename", &mut action)?,
            "admindir" => st.admindir = value(inline)?,
            "instdir" => st.instdir = value(inline)?,
            "root" => {
                let r = value(inline)?;
                st.admindir = format!("{r}/var/lib/dpkg");
                st.instdir = r;
            }
            "divert" => divert_to = Some(value(inline)?),
            "package" => package = Some(value(inline)?),
            "local" => local = true,
            "quiet" => st.quiet = true,
            "rename" => st.rename = true,
            "test" => st.test = true,
            _ => return Err(badusage(&format!("unknown option --{name}"))),
        }
    }
    let rest: Vec<String> = argv[i..].to_vec();
    let action = action.unwrap_or("add");
    if local && package.is_some() {
        return Err(badusage("--local and --package are mutually exclusive"));
    }
    if let Some(p) = &package {
        if p.is_empty() {
            return Err(badusage("package name is empty"));
        }
    }

    if action == "list" {
        let pat = rest.first().cloned().unwrap_or_else(|| "*".to_string());
        if rest.len() > 1 {
            return Err(badusage("--list takes at most one argument"));
        }
        let mut s = String::new();
        for d in st.load()? {
            let pk = d.pkg.clone().unwrap_or_else(|| "LOCAL".to_string());
            let hit = [d.original.as_bytes(), d.to.as_bytes(), pk.as_bytes()]
                .iter()
                .any(|name| fnmatch::<Bytes>(pat.as_bytes(), name, Flags::NONE));
            if hit {
                s.push_str(&describe_div(&d));
                s.push('\n');
            }
        }
        out(&s);
        return Ok(());
    }

    if rest.len() != 1 {
        return Err(badusage(&format!("--{action} needs a single argument")));
    }
    let file = rest[0].clone();
    if !file.starts_with('/') {
        return Err(badusage(&format!("filename \"{file}\" is not absolute")));
    }
    if file.contains('\n') {
        return Err(badusage(&format!("filename \"{file}\" contains newline")));
    }
    if let Some(t) = &divert_to {
        if !t.starts_with('/') {
            return Err(badusage(&format!("filename \"{t}\" is not absolute")));
        }
        if t.contains('\n') {
            return Err(badusage(&format!("filename \"{t}\" contains newline")));
        }
    }
    if action == "add" && divert_to.as_deref() == Some(file.as_str()) {
        return Err(badusage(&format!("cannot divert file '{file}' to itself")));
    }

    let mut list = st.load()?;
    let pos = list.iter().position(|d| d.original == file);
    match action {
        "listpackage" => {
            if let Some(i) = pos {
                out(&format!("{}\n", list[i].pkg.as_deref().unwrap_or("LOCAL")));
            }
            Ok(())
        }
        "truename" => {
            match pos {
                Some(i) => out(&format!("{}\n", list[i].to)),
                None => out(&format!("{file}\n")),
            }
            Ok(())
        }
        "remove" => {
            let pk = if local {
                Pkg::Local
            } else {
                match &package {
                    Some(p) => Pkg::Name(p.as_str()),
                    None => Pkg::Any,
                }
            };
            let Some(i) = pos else {
                if !st.quiet {
                    out(&format!(
                        "No diversion '{}', none removed.\n",
                        describe(&file, divert_to.as_deref(), pk)
                    ));
                }
                return Ok(());
            };
            let found = list[i].clone();
            if let Some(t) = &divert_to {
                if *t != found.to {
                    return Err(ohshit(&format!(
                        "mismatch on divert-to\n  when removing '{}'\n  found '{}'",
                        describe(&file, divert_to.as_deref(), pk),
                        describe_div(&found)
                    )));
                }
            }
            if local && found.pkg.is_some() || package.is_some() && package != found.pkg {
                return Err(ohshit(&format!(
                    "mismatch on package\n  when removing '{}'\n  found '{}'",
                    describe(&file, divert_to.as_deref(), pk),
                    describe_div(&found)
                )));
            }
            if !st.quiet {
                out(&format!("Removing '{}'\n", describe_div(&found)));
            }
            list.remove(i);
            if !st.test {
                st.save(&list)?;
            }
            if st.rename {
                st.move_file(&found.to, &found.original)?;
            }
            Ok(())
        }
        _ => {
            let to = divert_to.clone().unwrap_or_else(|| format!("{file}.distrib"));
            let new = Diversion { original: file.clone(), to: to.clone(), pkg: package.clone() };
            if let Some(i) = pos {
                if list[i] == new {
                    if !st.quiet {
                        out(&format!("Leaving '{}'\n", describe_div(&new)));
                    }
                    return Ok(());
                }
                return Err(ohshit(&format!(
                    "'{}' clashes with '{}'",
                    describe_div(&new),
                    describe_div(&list[i])
                )));
            }
            if let Some(d) = list.iter().find(|d| d.to == to || d.original == to) {
                return Err(ohshit(&format!(
                    "'{}' clashes with '{}'",
                    describe_div(&new),
                    describe_div(d)
                )));
            }
            if !st.quiet {
                out(&format!("Adding '{}'\n", describe_div(&new)));
            }
            list.push(new);
            if !st.test {
                st.save(&list)?;
            }
            if st.rename {
                st.move_file(&file, &to)?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globbing() {
        let glob = |p: &[u8], s: &[u8]| fnmatch::<Bytes>(p, s, Flags::NONE);
        assert!(glob(b"*", b"/bin/x"));
        assert!(glob(b"/bin/*", b"/bin/x"));
        assert!(glob(b"/bin/?", b"/bin/x"));
        assert!(!glob(b"/bin/?", b"/bin/xy"));
        assert!(glob(b"[a-c]x", b"bx"));
        assert!(!glob(b"[!a-c]x", b"bx"));
    }

    #[test]
    fn descriptions() {
        assert_eq!(describe("/a", None, Pkg::Any), "any diversion of /a");
        assert_eq!(describe("/a", Some("/b"), Pkg::Local), "local diversion of /a to /b");
        assert_eq!(describe("/a", Some("/b"), Pkg::Name("p")), "diversion of /a to /b by p");
    }
}
