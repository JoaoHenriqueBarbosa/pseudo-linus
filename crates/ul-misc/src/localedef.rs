//! `localedef` da glibc 2.41 (pacote libc-bin do Debian 13).
//!
//! Cobre `--help`, `--usage`, `--version`, os erros de uso do argp, `--list-archive` e a abertura
//! do mapa de caracteres (`-f`, ou o padrão `ANSI_X3.4-1968`) e do arquivo de definição (`-i`) com as
//! mensagens do original para arquivos inexistentes. A compilação de uma definição de locale de
//! verdade (gravar `/usr/lib/locale/<nome>`) não é feita: quando a entrada existe, o programa sai com
//! 4 sem escrever nada.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Errno;

use crate::util::io::{self, File};

const HELP: &str = concat!(
    "Usage: localedef [OPTION...] NAME\n",
    "  or:  localedef [OPTION...] [--add-to-archive|--delete-from-archive] FILE...\n",
    "  or:  localedef [OPTION...] --list-archive [FILE]\n",
    "Compile locale specification\n",
    "\n",
    " Input Files:\n",
    "  -f, --charmap=FILE         Symbolic character names defined in FILE\n",
    "  -i, --inputfile=FILE       Source definitions are found in FILE\n",
    "  -u, --repertoire-map=FILE  FILE contains mapping from symbolic names to UCS4\n",
    "                             values\n",
    "\n",
    " Output control:\n",
    "  -c, --force                Create output even if warning messages were issued\n",
    "                            \n",
    "      --no-hard-links        Do not create hard links between installed\n",
    "                             locales\n",
    "      --no-warnings=<warnings>   Comma-separated list of warnings to disable;\n",
    "                             supported warnings are: ascii, intcurrsym\n",
    "      --posix                Strictly conform to POSIX\n",
    "      --prefix=PATH          Optional output file prefix\n",
    "      --quiet                Suppress warnings and information messages\n",
    "  -v, --verbose              Print more messages\n",
    "      --warnings=<warnings>  Comma-separated list of warnings to enable;\n",
    "                             supported warnings are: ascii, intcurrsym\n",
    "\n",
    " Archive control:\n",
    "      --add-to-archive       Add locales named by parameters to archive\n",
    "  -A, --alias-file=FILE      locale.alias file to consult when making archive\n",
    "      --big-endian           Generate big-endian output\n",
    "      --delete-from-archive  Remove locales named by parameters from archive\n",
    "      --list-archive         List content of archive\n",
    "      --little-endian        Generate little-endian output\n",
    "      --no-archive           Don't add new data to archive\n",
    "      --replace              Replace existing archive content\n",
    "\n",
    "  -?, --help                 Give this help list\n",
    "      --usage                Give a short usage message\n",
    "  -V, --version              Print program version\n",
    "\n",
    "Mandatory or optional arguments to long options are also mandatory or optional\n",
    "for any corresponding short options.\n",
    "\n",
    "System's directory for character maps : /usr/share/i18n/charmaps\n",
    "\t\t       repertoire maps: /usr/share/i18n/repertoiremaps\n",
    "\t\t       locale path    : /usr/lib/locale:/usr/share/i18n\n",
    "For bug reporting instructions, please see:\n",
    "<http://www.debian.org/Bugs/>.\n",
);

const USAGE: &str = concat!(
    "Usage: localedef [-cv?V] [-f FILE] [-i FILE] [-u FILE] [-A FILE]\n",
    "            [--charmap=FILE] [--inputfile=FILE] [--repertoire-map=FILE]\n",
    "            [--force] [--no-hard-links] [--no-warnings=<warnings>] [--posix]\n",
    "            [--prefix=PATH] [--quiet] [--verbose] [--warnings=<warnings>]\n",
    "            [--add-to-archive] [--alias-file=FILE] [--big-endian]\n",
    "            [--delete-from-archive] [--list-archive] [--little-endian]\n",
    "            [--no-archive] [--replace] [--help] [--usage] [--version] NAME\n",
    "  or:  localedef [OPTION...] [--add-to-archive|--delete-from-archive] FILE...\n",
    "  or:  localedef [OPTION...] --list-archive [FILE]\n",
);

const VERSION: &str = "localedef (Debian GLIBC 2.41-12+deb13u4) 2.41
Copyright (C) 2024 Free Software Foundation, Inc.
This is free software; see the source for copying conditions.  There is NO
warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
Written by Ulrich Drepper.
";

const CHARMAP_PATH: &str = "/usr/share/i18n/charmaps";
const DEFAULT_CHARMAP: &str = "ANSI_X3.4-1968";
const ARCHIVE: &str = "/usr/lib/locale/locale-archive";

/// Erro de uso do argp: o localedef sai com 4 (`EXIT_FAILURE` do argp é sobrescrito).
const EXIT_USAGE: i32 = 4;

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
    ("no-hard-links", 0, 11),
    ("no-warnings", 1, 12),
    ("posix", 0, 10),
    ("prefix", 1, 5),
    ("quiet", 0, 6),
    ("verbose", 0, b'v'),
    ("warnings", 1, 8),
    ("add-to-archive", 0, 1),
    ("alias-file", 1, b'A'),
    ("big-endian", 0, 13),
    ("delete-from-archive", 0, 2),
    ("list-archive", 0, 3),
    ("little-endian", 0, 14),
    ("no-archive", 0, 4),
    ("replace", 0, 7),
    ("help", 0, b'?'),
    ("usage", 0, 9),
    ("version", 0, b'V'),
];

/// Procura `name` (com o `.gz` opcional) no diretório de mapas de caracteres.
fn charmap_in_dir(name: &[u8]) -> bool {
    let mut p = CHARMAP_PATH.as_bytes().to_vec();
    p.push(b'/');
    p.extend_from_slice(name);
    if File::open(&p).is_ok() {
        return true;
    }
    p.extend_from_slice(b".gz");
    File::open(&p).is_ok()
}

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
                return EXIT_USAGE;
            }
            if cands.len() > 1 {
                let poss: Vec<String> = cands.iter().map(|o| format!("'--{}'", o.0)).collect();
                io::eprint(format!(
                    "localedef: option '--{ns}' is ambiguous; possibilities: {}\n",
                    poss.join(" ")
                ));
                try_help();
                return EXIT_USAGE;
            }
            let o = cands[0];
            match (o.1, val) {
                (0, Some(_)) => {
                    io::eprint(format!("localedef: option '--{}' doesn't allow an argument\n", o.0));
                    try_help();
                    return EXIT_USAGE;
                }
                (1, None) => {
                    if i < argv.len() {
                        i += 1;
                        items.push((o.2, Some(argv[i - 1].clone())));
                    } else {
                        io::eprint(format!("localedef: option '--{}' requires an argument\n", o.0));
                        try_help();
                        return EXIT_USAGE;
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
                            return EXIT_USAGE;
                        };
                        items.push((c, Some(v)));
                        break;
                    }
                    _ => {
                        io::eprint(format!("localedef: invalid option -- '{}'\n", c as char));
                        try_help();
                        return EXIT_USAGE;
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
        // Sem arquivo de archive não há nada a listar: o original sai com 0 sem imprimir.
        let mut path = prefix.clone();
        path.extend_from_slice(ARCHIVE.as_bytes());
        return match File::open(&path) {
            Ok(_) => 0,
            Err(e) if e.0 == Errno::ENOENT.0 => 0,
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

    match &charmap {
        Some(cm) => {
            if File::open(cm).is_err() {
                let found = !cm.contains(&b'/') && charmap_in_dir(cm);
                if !found {
                    io::eprint(format!(
                        "[error] character map file `{}' not found: {}\n",
                        io::lossy(cm),
                        Errno::ENOENT.message()
                    ));
                    if !cm.contains(&b'/') && File::open(CHARMAP_PATH.as_bytes()).is_err() {
                        io::eprint(format!(
                            "[error] cannot read character map directory `{CHARMAP_PATH}': {}\n",
                            Errno::ENOENT.message()
                        ));
                    }
                    return 1;
                }
            }
        }
        None => {
            // Sem `-f`, o original usa o mapa padrão `ANSI_X3.4-1968` do diretório de mapas.
            if !charmap_in_dir(DEFAULT_CHARMAP.as_bytes()) {
                io::eprint(format!(
                    "[error] default character map file `{DEFAULT_CHARMAP}' not found: {}\n",
                    Errno::ENOENT.message()
                ));
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
