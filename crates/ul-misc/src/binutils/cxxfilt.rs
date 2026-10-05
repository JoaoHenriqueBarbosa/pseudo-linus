//! `c++filt` do GNU binutils 2.44 (Debian 13): demangla nomes C++ (Itanium) dos argumentos ou,
//! sem argumentos, de cada palavra do stdin. O demangler está em [`super::demangle`].
//!
//! Escrito a partir do comportamento observado e da man page (o código do binutils e do
//! libiberty é GPL e não foi consultado). Palavra do stdin: letras, dígitos e `_ $ .`.
//! Divergências conhecidas: os estilos `java`, `gnat`, `dlang` e `rust` são aceitos mas só o
//! Itanium é demanglado; não há limite de recursão configurável.

use std::ffi::OsString;
use std::io::Write;

use sysabi::sys;

use crate::strings::expand_response_files;
use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

use super::demangle::demangle;

const SHORTOPTS: &str = "_pntirRs:hv?";

const ID_STRIP: i32 = 256;
const ID_NO_STRIP: i32 = 257;

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("strip-underscore", HasArg::No, ID_STRIP),
    LongOpt::new("no-strip-underscore", HasArg::No, ID_NO_STRIP),
    LongOpt::new("no-params", HasArg::No, 'p' as i32),
    LongOpt::new("types", HasArg::No, 't' as i32),
    LongOpt::new("no-verbose", HasArg::No, 'i' as i32),
    LongOpt::new("no-recurse-limit", HasArg::No, 'r' as i32),
    LongOpt::new("recurse-limit", HasArg::No, 'R' as i32),
    LongOpt::new("format", HasArg::Required, 's' as i32),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("version", HasArg::No, 'v' as i32),
];

const USAGE_BODY: &str = "Options are:\n\
\x20 [-_|--strip-underscore]     Ignore first leading underscore\n\
\x20 [-n|--no-strip-underscore]  Do not ignore a leading underscore (default)\n\
\x20 [-p|--no-params]            Do not display function arguments\n\
\x20 [-i|--no-verbose]           Do not show implementation details (if any)\n\
\x20 [-R|--recurse-limit]        Enable a limit on recursion whilst demangling.  [Default]\n\
\x20 ]-r|--no-recurse-limit]     Disable a limit on recursion whilst demangling\n\
\x20 [-t|--types]                Also attempt to demangle type encodings\n\
\x20 [-s|--format {none,auto,gnu-v3,java,gnat,dlang,rust}]\n\
\x20 [@<file>]                   Read extra options from <file>\n\
\x20 [-h|--help]                 Display this information\n\
\x20 [-v|--version]              Show the version information\n\
Demangled names are displayed to stdout.\n\
If a name cannot be demangled it is just echoed to stdout.\n\
If no names are provided on the command line, stdin is read.\n";

const STYLES: &[&[u8]] = &[
    b"none", b"auto", b"gnu-v3", b"java", b"gnat", b"dlang", b"rust",
];

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(prog: &str, to_stdout: bool) -> i32 {
    let mut text = format!("Usage: {prog} [options] [mangled names]\n");
    text.push_str(USAGE_BODY);
    if to_stdout {
        text.push_str("Report bugs to <https://sourceware.org/bugzilla/>.\n");
        let mut out = io::stdout();
        let _ = out.write_all(text.as_bytes());
        0
    } else {
        io::eprint(text);
        1
    }
}

struct Opts {
    strip_underscore: bool,
    no_params: bool,
    types: bool,
}

fn demangle_it(word: &[u8], o: &Opts, out: &mut Vec<u8>) {
    let mut skip = 0;
    if word.first().is_some_and(|&c| c == b'.' || c == b'$') {
        skip += 1;
    }
    if o.strip_underscore && word.get(skip) == Some(&b'_') {
        skip += 1;
    }
    match demangle(&word[skip..], o.no_params, o.types) {
        None => out.extend_from_slice(word),
        Some(r) => {
            if word.first() == Some(&b'.') {
                out.push(b'.');
            }
            out.extend_from_slice(r.as_bytes());
        }
    }
}

fn run(args: &[OsString]) -> i32 {
    let prog = io::argv0(args);
    let argv = match expand_response_files(&prog, io::args_bytes(args)) {
        Ok(a) => a,
        Err(c) => return c,
    };
    let rest: Vec<Vec<u8>> = argv.get(1..).unwrap_or(&[]).to_vec();
    let posix = sys::try_current().is_some_and(|s| s.getenv(b"POSIXLY_CORRECT").is_some());
    let mut g = Getopt::new(&rest, SHORTOPTS, LONGOPTS, posix);
    let mut o = Opts {
        strip_underscore: false,
        no_params: false,
        types: false,
    };
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(x) => x,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&prog)));
                return usage(&prog, false);
            }
        };
        let arg = opt.arg.clone().unwrap_or_default();
        match opt.id {
            ID_STRIP => o.strip_underscore = true,
            ID_NO_STRIP => o.strip_underscore = false,
            id => match u8::try_from(id).unwrap_or(0) {
                b'_' => o.strip_underscore = true,
                b'n' => o.strip_underscore = false,
                b'p' => o.no_params = true,
                b't' => o.types = true,
                b'i' | b'r' | b'R' => {}
                b's' => {
                    if !STYLES.contains(&arg.as_slice()) {
                        io::eprint(format!(
                            "{prog}: unknown demangling style `{}'\n",
                            io::lossy(&arg)
                        ));
                        return 1;
                    }
                }
                b'h' | b'?' => return usage(&prog, true),
                b'v' => {
                    super::ar::print_version("c++filt");
                    return 0;
                }
                _ => {}
            },
        }
    }
    let operands = g.operands();
    let mut out: Vec<u8> = Vec::new();
    if !operands.is_empty() {
        for a in &operands {
            demangle_it(a, &o, &mut out);
            out.push(b'\n');
        }
        let mut so = io::stdout();
        let _ = so.write_all(&out);
        return 0;
    }
    let input = io::read_stdin().unwrap_or_default();
    let mut word: Vec<u8> = Vec::new();
    for &c in &input {
        if c.is_ascii_alphanumeric() || matches!(c, b'_' | b'$' | b'.') {
            word.push(c);
        } else {
            if !word.is_empty() {
                demangle_it(&word, &o, &mut out);
                word.clear();
            }
            out.push(c);
        }
    }
    if !word.is_empty() {
        demangle_it(&word, &o, &mut out);
    }
    let mut so = io::stdout();
    let _ = so.write_all(&out);
    0
}
