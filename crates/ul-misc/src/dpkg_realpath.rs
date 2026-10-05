//! `dpkg-realpath` do dpkg 1.22 (Debian 13): imprime o caminho canônico de um arquivo, resolvendo os
//! links simbólicos dentro de um diretório raiz (`--root` ou `DPKG_ROOT`; os destinos absolutos dos
//! links valem a partir da raiz).
//!
//! Divergências conhecidas: o texto do `--help` e as mensagens de erro são de memória.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Fd, FileType, sys};

use crate::util::io;

const PROG: &str = "dpkg-realpath";

const USAGE: &str = "Usage: dpkg-realpath [<option>...] <pathname>

Options:
      --root=<directory>  set the root directory.
  -?, --help              show this help message.
      --version           show the version.
";

const VERSION: &str = "Debian dpkg-realpath version 1.22.22.

This is free software; see the GNU General Public License version 2 or
later for copying conditions. There is NO warranty.
";

fn out(s: &str) {
    let mut o = io::stdout();
    let _ = o.write_all(s.as_bytes());
}

fn uerr(msg: &str) -> i32 {
    let _ = io::flush_stdout();
    io::eprint(format!(
        "{PROG}: error: {msg}\n\nUse '{PROG} --help' for program usage information.\n"
    ));
    2
}

/// Resolve `path` (absoluto dentro da raiz) e devolve o caminho canônico, também dentro da raiz.
fn resolve(root: &str, path: &str) -> Result<String, sysabi::Errno> {
    let mut pending: Vec<String> = path.split('/').rev().filter(|c| !c.is_empty()).map(str::to_string).collect();
    let mut cur: Vec<String> = Vec::new();
    let mut links = 0;
    while let Some(c) = pending.pop() {
        match c.as_str() {
            "." => continue,
            ".." => {
                cur.pop();
                continue;
            }
            _ => {}
        }
        let mut cand = cur.clone();
        cand.push(c.clone());
        let fs = format!("{root}/{}", cand.join("/"));
        let st = sys::lstat(fs.as_bytes())?;
        if st.file_type() == FileType::Symlink {
            links += 1;
            if links > 40 {
                return Err(sysabi::Errno::ELOOP);
            }
            let target = sys::current().readlinkat(Fd::CWD, fs.as_bytes())?;
            let target = String::from_utf8_lossy(&target).into_owned();
            if target.starts_with('/') {
                cur.clear();
            }
            for comp in target.split('/').rev().filter(|c| !c.is_empty()) {
                pending.push(comp.to_string());
            }
        } else {
            cur.push(c);
        }
    }
    // O destino final precisa existir.
    let fs = format!("{root}/{}", cur.join("/"));
    sys::stat(fs.as_bytes())?;
    Ok(format!("/{}", cur.join("/")))
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv: Vec<String> = io::args_bytes(args)
        .iter()
        .skip(1)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect();
    let mut root: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < argv.len() {
        let a = argv[i].clone();
        i += 1;
        if a == "--" {
            rest.extend_from_slice(&argv[i..]);
            break;
        }
        if let Some(l) = a.strip_prefix("--") {
            let (name, inline) = match l.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (l, None),
            };
            match name {
                "help" => {
                    out(USAGE);
                    return 0;
                }
                "version" => {
                    out(VERSION);
                    return 0;
                }
                "root" => match inline {
                    Some(v) => root = Some(v),
                    None => {
                        if i < argv.len() {
                            i += 1;
                            root = Some(argv[i - 1].clone());
                        } else {
                            return uerr("option 'root' requires an argument");
                        }
                    }
                },
                _ => return uerr(&format!("unknown option or argument {a}")),
            }
        } else if a == "-?" {
            out(USAGE);
            return 0;
        } else if a.starts_with('-') && a.len() > 1 {
            return uerr(&format!("unknown option or argument {a}"));
        } else {
            rest.push(a);
        }
    }
    if rest.len() != 1 {
        return uerr("takes exactly one pathname");
    }
    let root = root
        .or_else(|| {
            sys::try_current()
                .and_then(|s| s.getenv(b"DPKG_ROOT"))
                .map(|v| String::from_utf8_lossy(&v).into_owned())
        })
        .unwrap_or_default();
    let root = root.trim_end_matches('/').to_string();
    let path = &rest[0];
    let abs = if path.starts_with('/') {
        path.clone()
    } else {
        let cwd = sys::current().getcwd().map(|c| String::from_utf8_lossy(&c).into_owned()).unwrap_or_default();
        format!("{cwd}/{path}")
    };
    match resolve(&root, &abs) {
        Ok(r) => {
            out(&format!("{root}{r}\n"));
            0
        }
        Err(e) => {
            let _ = io::flush_stdout();
            io::eprint(format!("{PROG}: error: cannot resolve pathname {path}: {}\n", e.message()));
            255
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn usage_mentions_root() {
        assert!(super::USAGE.contains("--root"));
    }
}
