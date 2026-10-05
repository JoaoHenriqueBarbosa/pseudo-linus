//! `dpkg-vendor` do dpkg 1.22 (Debian 13): consulta os arquivos de origem em `/etc/dpkg/origins`.
//!
//! Comandos: `--is`, `--derives-from`, `--query`, `--vendor-info`, `--help` e `--version`; opção
//! `--vendor` (equivale a definir `DEB_VENDOR`). Sem arquivo de origem no sandbox, o fornecedor
//! `Debian` usa o conteúdo padrão do pacote `base-files`/`dpkg`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;

const PROG: &str = "dpkg-vendor";

const USAGE: &str = "Usage: dpkg-vendor [<option>...] <command>

Commands:
  --is <vendor>              returns true if current vendor is <vendor>.
  --derives-from <vendor>    returns true if current vendor derives from <vendor>.
  --query <field>            print the content of the vendor-specific field.
  --vendor-info              print out the full vendor information.
  --help                     show this help message.
  --version                  show the version.

Options:
  --vendor <vendor>          assume <vendor> is the current vendor.
";

const VERSION: &str = "Debian dpkg-vendor version 1.22.22.

This is free software; see the GNU General Public License version 2 or
later for copying conditions. There is NO warranty.
";

const DEBIAN_ORIGIN: &str = "Vendor: Debian\nVendor-URL: https://www.debian.org/\nBugs: debbugs://bugs.debian.org\n";

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

/// Campos de um arquivo de origem, na ordem e com a grafia do arquivo.
type Info = Vec<(String, String)>;

fn get<'a>(info: &'a Info, name: &str) -> Option<&'a str> {
    info.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

fn parse_info(text: &str) -> Info {
    let mut v: Info = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = v.last_mut() {
                last.1.push('\n');
                last.1.push_str(line);
            }
            continue;
        }
        if let Some((k, val)) = line.split_once(':') {
            v.push((k.to_string(), val.trim().to_string()));
        }
    }
    v
}

fn ucfirst_lc(s: &str) -> String {
    let l = s.to_lowercase();
    let mut c = l.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// `get_vendor_info`: tenta o nome como veio, em minúsculas e com a inicial maiúscula.
fn vendor_info(name: &str) -> Option<Info> {
    let tries = [name.to_string(), name.to_lowercase(), ucfirst_lc(name)];
    for t in tries {
        if t.contains('/') {
            continue;
        }
        let path = format!("/etc/dpkg/origins/{t}");
        if let Ok(d) = sys::read_file(path.as_bytes()) {
            return Some(parse_info(&String::from_utf8_lossy(&d)));
        }
    }
    if name.eq_ignore_ascii_case("debian") || name.eq_ignore_ascii_case("default") {
        return Some(parse_info(DEBIAN_ORIGIN));
    }
    None
}

fn env_vendor() -> Option<String> {
    sys::try_current()
        .and_then(|s| s.getenv(b"DEB_VENDOR"))
        .map(|v| String::from_utf8_lossy(&v).into_owned())
        .filter(|v| !v.is_empty())
}

/// `get_current_vendor`.
fn current_vendor(opt: Option<&str>) -> String {
    let env = opt.map(str::to_string).or_else(env_vendor);
    if let Some(e) = env {
        if let Some(info) = vendor_info(&e) {
            if let Some(v) = get(&info, "Vendor") {
                return v.to_string();
            }
        }
    }
    if let Some(info) = vendor_info("default") {
        if let Some(v) = get(&info, "Vendor") {
            return v.to_string();
        }
    }
    "Default".to_string()
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
    let mut action: Option<(String, String)> = None;
    let mut vendor_opt: Option<String> = None;
    let mut i = 0usize;
    while i < argv.len() {
        let a = argv[i].clone();
        i += 1;
        let (name, inline) = match a.strip_prefix("--") {
            Some(l) => match l.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (l.to_string(), None),
            },
            None => {
                if a == "-?" {
                    out(USAGE);
                    return 0;
                }
                return uerr(&format!("unknown option or argument {a}"));
            }
        };
        let needs_value = matches!(name.as_str(), "is" | "derives-from" | "query" | "vendor");
        let mut val = String::new();
        if needs_value {
            match inline {
                Some(v) => val = v,
                None => {
                    if i < argv.len() {
                        i += 1;
                        val = argv[i - 1].clone();
                    } else {
                        return uerr(&format!("option '{name}' requires an argument"));
                    }
                }
            }
        }
        match name.as_str() {
            "help" => {
                out(USAGE);
                return 0;
            }
            "version" => {
                out(VERSION);
                return 0;
            }
            "vendor" => vendor_opt = Some(val),
            "is" | "derives-from" | "query" | "vendor-info" => {
                if let Some((p, _)) = &action {
                    return uerr(&format!("two commands specified: --{p} and --{name}"));
                }
                action = Some((name.clone(), val));
            }
            _ => return uerr(&format!("unknown option or argument {a}")),
        }
    }
    let Some((act, param)) = action else {
        return uerr("need an action option");
    };
    match act.as_str() {
        "is" => {
            if current_vendor(vendor_opt.as_deref()).eq_ignore_ascii_case(&param) { 0 } else { 1 }
        }
        "derives-from" => {
            let mut name = current_vendor(vendor_opt.as_deref());
            let mut seen: Vec<String> = Vec::new();
            loop {
                if name.eq_ignore_ascii_case(&param) {
                    return 0;
                }
                if seen.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
                    return 1;
                }
                seen.push(name.clone());
                let Some(info) = vendor_info(&name) else { return 1 };
                match get(&info, "Parent") {
                    Some(p) if !p.is_empty() => name = p.to_string(),
                    _ => return 1,
                }
            }
        }
        "query" => {
            let name = current_vendor(vendor_opt.as_deref());
            if let Some(info) = vendor_info(&name) {
                if let Some(v) = get(&info, &param) {
                    out(&format!("{v}\n"));
                }
            }
            0
        }
        _ => {
            let name = current_vendor(vendor_opt.as_deref());
            if let Some(info) = vendor_info(&name) {
                let mut s = String::new();
                for (k, v) in &info {
                    s.push_str(&format!("{k}: {v}\n"));
                }
                out(&s);
            }
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_origin() {
        let i = parse_info(DEBIAN_ORIGIN);
        assert_eq!(get(&i, "vendor"), Some("Debian"));
        assert_eq!(get(&i, "Vendor-URL"), Some("https://www.debian.org/"));
    }

    #[test]
    fn capitalizes() {
        assert_eq!(ucfirst_lc("dEBIAN"), "Debian");
    }
}
