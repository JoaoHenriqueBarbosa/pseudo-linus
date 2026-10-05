//! `localedef` da glibc 2.41 (pacote libc-bin do Debian 13).
//!
//! Cobre `--help`, `--usage`, `--version`, os erros de uso do argp, `--list-archive` e a abertura
//! do mapa de caracteres (`-f`) e do arquivo de definição (`-i`) com as mensagens do original para
//! arquivos inexistentes. A compilação de uma definição de locale de verdade (gravar
//! `/usr/lib/locale/<nome>`) não é feita: quando a entrada existe, o programa sai com 4 sem
//! escrever nada.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Errno;

use crate::util::io::{self, File};

const HELP: &str = "Usage: localedef [OPTION...] NAME
Create a locale object.

System's directory for character maps : /usr/share/i18n/charmaps
\t\t       repertoire maps: /usr/share/i18n/repertoiremaps
\t\t       locale path    : /usr/lib/locale:/usr/share/i18n

 Input Files:
  -c, --force                Create output even if warning messages were issued
  -f, --charmap=FILE         Symbolic character names defined in FILE
  -i, --inputfile=FILE       Source definitions are found in FILE
  -u, --repertoire-map=FILE  FILE contains mapping from symbolic names to UCS4
                             values

 Output control:
      --add-to-archive       Add locales named by parameters to archive
      --delete-from-archive  Delete locales named by parameters from archive
      --list-archive         List content of archive
      --no-archive           Do not use archive
      --prefix=PATH          Optional output file prefix
      --quiet                Suppress warnings and information messages
      --replace              Replace existing archive content
  -v, --verbose              Print more messages
      --warnings[=WARNINGS]  Comma-separated list of warnings to enable
  -?, --help                 Give this help list
      --usage                Give a short usage message
  -V, --version              Print program version

Mandatory or optional arguments to long options are also mandatory or optional
for any corresponding short options.

For bug reporting instructions, please see:
<http://www.debian.org/Bugs/>.
";

const USAGE: &str = "Usage: localedef [-cvV?] [-f FILE] [-i FILE] [-u FILE] [--force]
            [--charmap=FILE] [--inputfile=FILE] [--repertoire-map=FILE]
            [--add-to-archive] [--delete-from-archive] [--list-archive]
            [--no-archive] [--prefix=PATH] [--quiet] [--replace] [--verbose]
            [--warnings[=WARNINGS]] [--help] [--usage] [--version] NAME
";

const VERSION: &str = "localedef (Debian GLIBC 2.41-12+deb13u4) 2.41
Copyright (C) 2024 Free Software Foundation, Inc.
This is free software; see the source for copying conditions.  There is NO
warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
Written by Ulrich Drepper.
";

const CHARMAP_PATH: &str = "/usr/share/i18n/charmaps";
const ARCHIVE: &str = "/usr/lib/locale/locale-archive";

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn try_help() {
    io::eprint("Try `localedef --help' or `localedef --usage' for more information.\n");
}

/// Opções longas: (nome, recebe argumento: 0 não, 1 obrigatório, 2 opcional, código curto).
const LONG: &[(&str, u8, u8)] = &[
    ("force", 0, b'c'),
    ("charmap", 1, b'f'),
    ("inputfile", 1, b'i'),
    ("repertoire-map", 1, b'u'),
    ("add-to-archive", 0, 1),
    ("delete-from-archive", 0, 2),
    ("list-archive", 0, 3),
    ("no-archive", 0, 4),
    ("prefix", 1, 5),
    ("quiet", 0, 6),
    ("replace", 0, 7),
    ("verbose", 0, b'v'),
    ("warnings", 2, 8),
    ("help", 0, b'?'),
    ("usage", 0, 9),
    ("version", 0, b'V'),
];

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let mut charmap: Option<Vec<u8>> = None;
    let mut input: Option<Vec<u8>> = None;
    let mut prefix: Vec<u8> = Vec::new();
    let mut list_archive = false;
    let mut names: Vec<Vec<u8>> = Vec::new();

    let mut i = 1;
    let mut only_names = false;
    while i < argv.len() {
        let a = argv[i].clone();
        i += 1;
        if only_names || a.len() < 2 || a[0] != b'-' {
            names.push(a);
            continue;
        }
        if a.as_slice() == b"--" {
            only_names = true;
            continue;
        }
        // (código, valor)
        let mut items: Vec<(u8, Option<Vec<u8>>)> = Vec::new();
        if a.starts_with(b"--") {
            let body = &a[2..];
            let (name, val) = match body.iter().position(|b| *b == b'=') {
                Some(p) => (&body[..p], Some(body[p + 1..].to_vec())),
                None => (body, None),
            };
            let ns = io::lossy(name);
            let exact: Vec<_> = LONG.iter().filter(|o| o.0 == ns).collect();
            let cands: Vec<_> = if exact.is_empty() {
                LONG.iter().filter(|o| o.0.starts_with(&ns)).collect()
            } else {
                exact
            };
            if cands.is_empty() {
                io::eprint(format!("localedef: unrecognized option '--{ns}'\n"));
                try_help();
                return 64;
            }
            if cands.len() > 1 {
                let poss: Vec<String> = cands.iter().map(|o| format!("'--{}'", o.0)).collect();
                io::eprint(format!(
                    "localedef: option '--{ns}' is ambiguous; possibilities: {}\n",
                    poss.join(" ")
                ));
                try_help();
                return 64;
            }
            let o = cands[0];
            match (o.1, val) {
                (0, Some(_)) => {
                    io::eprint(format!("localedef: option '--{}' doesn't allow an argument\n", o.0));
                    try_help();
                    return 64;
                }
                (1, None) => {
                    if i < argv.len() {
                        i += 1;
                        items.push((o.2, Some(argv[i - 1].clone())));
                    } else {
                        io::eprint(format!("localedef: option '--{}' requires an argument\n", o.0));
                        try_help();
                        return 64;
                    }
                }
                (_, v) => items.push((o.2, v)),
            }
        } else {
            let mut j = 1;
            while j < a.len() {
                let c = a[j];
                j += 1;
                match c {
                    b'c' | b'v' | b'V' | b'?' => items.push((c, None)),
                    b'f' | b'i' | b'u' | b'A' => {
                        let v = if j < a.len() {
                            a[j..].to_vec()
                        } else if i < argv.len() {
                            i += 1;
                            argv[i - 1].clone()
                        } else {
                            io::eprint(format!("localedef: option requires an argument -- '{}'\n", c as char));
                            try_help();
                            return 64;
                        };
                        items.push((c, Some(v)));
                        break;
                    }
                    _ => {
                        io::eprint(format!("localedef: invalid option -- '{}'\n", c as char));
                        try_help();
                        return 64;
                    }
                }
            }
        }
        for (k, v) in items {
            match k {
                b'?' => {
                    let _ = io::stdout().write_all(HELP.as_bytes());
                    return 0;
                }
                9 => {
                    let _ = io::stdout().write_all(USAGE.as_bytes());
                    return 0;
                }
                b'V' => {
                    let _ = io::stdout().write_all(VERSION.as_bytes());
                    return 0;
                }
                b'f' => charmap = v,
                b'i' => input = v,
                5 => prefix = v.unwrap_or_default(),
                3 => list_archive = true,
                _ => {}
            }
        }
    }

    if list_archive {
        let mut path = prefix.clone();
        path.extend_from_slice(ARCHIVE.as_bytes());
        return match File::open(&path) {
            Ok(_) => 0,
            Err(e) => {
                io::eprint(format!(
                    "localedef: cannot open locale archive \"{}\": {}\n",
                    io::lossy(&path),
                    e.message()
                ));
                1
            }
        };
    }

    if names.len() != 1 {
        try_help();
        return 4;
    }

    if let Some(cm) = &charmap {
        if File::open(cm).is_err() {
            let mut found = false;
            if !cm.contains(&b'/') {
                let mut p = CHARMAP_PATH.as_bytes().to_vec();
                p.push(b'/');
                p.extend_from_slice(cm);
                found = File::open(&p).is_ok();
                if !found {
                    p.extend_from_slice(b".gz");
                    found = File::open(&p).is_ok();
                }
                if !found && File::open(CHARMAP_PATH.as_bytes()).is_err() {
                    io::eprint(format!(
                        "localedef: cannot read character map directory `{CHARMAP_PATH}': {}\n",
                        Errno::ENOENT.message()
                    ));
                }
            }
            if !found {
                io::eprint(format!("localedef: character map file `{}' not found\n", io::lossy(cm)));
                return 4;
            }
        }
    }

    if let Some(inp) = &input {
        if File::open(inp).is_err() {
            io::eprint(format!(
                "localedef: cannot open locale definition file `{}': {}\n",
                io::lossy(inp),
                Errno::ENOENT.message()
            ));
            return 4;
        }
    }
    // Compilar e gravar o locale não é suportado aqui.
    4
}
