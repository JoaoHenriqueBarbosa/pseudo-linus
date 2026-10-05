//! `pldd` da glibc 2.41 (pacote libc-bin do Debian 13).
//!
//! Lista as bibliotecas dinâmicas de um processo anexando-se a ele com `ptrace`. O sandbox não tem
//! `ptrace`: processo inexistente falha na leitura de `/proc/<pid>` e processo existente falha no
//! anexo com `Operation not permitted`, as duas mensagens do original.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Errno;

use crate::util::io::{self, File};

const HELP: &str = "Usage: pldd [OPTION...] PID
Print list of dynamic shared objects loaded into a running process.

  -?, --help                 Give this help list
      --usage                Give a short usage message
  -V, --version              Print program version

Mandatory or optional arguments to long options are also mandatory or optional
for any corresponding short options.

For bug reporting instructions, please see:
<http://www.debian.org/Bugs/>.
";

const USAGE: &str = "Usage: pldd [-?V] [--help] [--usage] [--version] PID\n";

const VERSION: &str = "pldd (Debian GLIBC 2.41-12+deb13u4) 2.41
Copyright (C) 2024 Free Software Foundation, Inc.
This is free software; see the source for copying conditions.  There is NO
warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
Written by Ulrich Drepper.
";

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn try_help() {
    io::eprint("Try `pldd --help' or `pldd --usage' for more information.\n");
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let mut pids: Vec<Vec<u8>> = Vec::new();
    let mut only_names = false;
    for a in &argv[1..] {
        if only_names || a.len() < 2 || a[0] != b'-' {
            pids.push(a.clone());
            continue;
        }
        match a.as_slice() {
            b"--" => only_names = true,
            b"--help" | b"-?" => {
                let _ = io::stdout().write_all(HELP.as_bytes());
                return 0;
            }
            b"--usage" => {
                let _ = io::stdout().write_all(USAGE.as_bytes());
                return 0;
            }
            b"--version" | b"-V" => {
                let _ = io::stdout().write_all(VERSION.as_bytes());
                return 0;
            }
            _ if a.starts_with(b"--") => {
                let name = io::lossy(&a[2..]);
                let name = name.split('=').next().unwrap_or("");
                // Abreviações únicas das opções longas.
                let mut cands = Vec::new();
                for l in ["help", "usage", "version"] {
                    if !name.is_empty() && l.starts_with(name) {
                        cands.push(l);
                    }
                }
                match cands.as_slice() {
                    ["help"] => {
                        let _ = io::stdout().write_all(HELP.as_bytes());
                        return 0;
                    }
                    ["usage"] => {
                        let _ = io::stdout().write_all(USAGE.as_bytes());
                        return 0;
                    }
                    ["version"] => {
                        let _ = io::stdout().write_all(VERSION.as_bytes());
                        return 0;
                    }
                    [] => {
                        io::eprint(format!("pldd: unrecognized option '--{name}'\n"));
                        try_help();
                        return 64;
                    }
                    _ => {
                        io::eprint(format!("pldd: option '--{name}' is ambiguous\n"));
                        try_help();
                        return 64;
                    }
                }
            }
            _ => {
                io::eprint(format!("pldd: invalid option -- '{}'\n", a[1] as char));
                try_help();
                return 64;
            }
        }
    }

    if pids.len() != 1 {
        io::eprint("Exactly one parameter with process ID required.\n");
        try_help();
        return 1;
    }

    let text = io::lossy(&pids[0]);
    let pid: u64 = match text.parse() {
        Ok(p) if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => p,
        _ => {
            io::eprint(format!("pldd: invalid process ID '{text}'\n"));
            return 1;
        }
    };

    // do_it(): primeiro o /proc/<pid>/exe, que descreve o executável do processo.
    let exe = format!("/proc/{pid}/stat");
    if let Err(e) = File::open(exe.as_bytes()) {
        io::eprint(format!("pldd: cannot get information about process {pid}: {}\n", e.message()));
        return 1;
    }
    io::eprint(format!(
        "pldd: cannot attach to process {pid}: {}\n",
        Errno::EPERM.message()
    ));
    1
}
