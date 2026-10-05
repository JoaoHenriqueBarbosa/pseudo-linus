//! `addr2line` do GNU binutils 2.44 (Debian 13) sobre ELF64 x86_64.
//!
//! Escrito a partir do comportamento observado e da man page (o código do binutils é GPL e não
//! foi consultado). Não há leitor de DWARF: toda consulta responde como um binário sem
//! informação de depuração, `??` para a função e `??:0` para arquivo e linha, que é o que o
//! original faz com um executável sem `-g` (o caso do `/bin/true` do Debian).
//!
//! Divergências conhecidas: binário com DWARF não resolve nada; `-j` e `-b` são aceitos sem
//! efeito; endereço malformado vale 0 em vez de gerar o erro do original.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, FileType, sys};

use crate::strings::{TARGETS, expand_response_files};
use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

use super::elf::Elf;

const SHORTOPTS: &str = "ab:Ce:fHhij:pRrsVv";

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("addresses", HasArg::No, 'a' as i32),
    LongOpt::new("basenames", HasArg::No, 's' as i32),
    LongOpt::new("demangle", HasArg::Optional, 'C' as i32),
    LongOpt::new("exe", HasArg::Required, 'e' as i32),
    LongOpt::new("functions", HasArg::No, 'f' as i32),
    LongOpt::new("inlines", HasArg::No, 'i' as i32),
    LongOpt::new("section", HasArg::Required, 'j' as i32),
    LongOpt::new("target", HasArg::Required, 'b' as i32),
    LongOpt::new("pretty-print", HasArg::No, 'p' as i32),
    LongOpt::new("recurse-limit", HasArg::No, 'r' as i32),
    LongOpt::new("no-recurse-limit", HasArg::No, 'R' as i32),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

const USAGE_BODY: &str = " Convert addresses into line number/file name pairs.\n\
\x20If no addresses are specified on the command line, they will be read from stdin\n\
\x20The options are:\n\
\x20 @<file>                Read options from <file>\n\
\x20 -a --addresses         Show addresses\n\
\x20 -b --target=<bfdname>  Set the binary file format\n\
\x20 -e --exe=<executable>  Set the input file name (default is a.out)\n\
\x20 -i --inlines           Unwind inlined functions\n\
\x20 -j --section=<name>    Read section-relative offsets instead of addresses\n\
\x20 -p --pretty-print      Make the output easier to read for humans\n\
\x20 -s --basenames         Strip directory names\n\
\x20 -f --functions         Show function names\n\
\x20 -C --demangle[=style]  Demangle function names\n\
\x20 -h --help              Display this information\n\
\x20 -r --recurse-limit     Enable a limit on recursion whilst demangling.  [Default]\n\
\x20 -R --no-recurse-limit  Disable a limit on recursion whilst demangling\n\
\x20 -H --help              Display this information\n\
\x20 -V --version           Display the version of addr2line\n";

struct Opts {
    addresses: bool,
    functions: bool,
    pretty: bool,
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(prog: &str, to_stdout: bool) -> i32 {
    let mut text = format!("Usage: {prog} [option(s)] [addr(s)]\n");
    text.push_str(USAGE_BODY);
    text.push_str(&format!(
        "{prog}: supported targets: {}\n",
        TARGETS.join(" ")
    ));
    if to_stdout {
        text.push_str("Report bugs to <https://sourceware.org/bugzilla/>\n");
        let mut out = io::stdout();
        let _ = out.write_all(text.as_bytes());
        0
    } else {
        io::eprint(text);
        1
    }
}

/// Interpreta um endereço em hexadecimal (com ou sem `0x`); lixo vale 0.
fn parse_addr(s: &[u8]) -> u64 {
    let t = String::from_utf8_lossy(s);
    let t = t.trim();
    let h = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .unwrap_or(t);
    let digits: String = h.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    u64::from_str_radix(&digits, 16).unwrap_or(0)
}

fn emit(out: &mut Vec<u8>, o: &Opts, addr: u64) {
    if o.addresses {
        out.extend_from_slice(format!("0x{addr:016x}").as_bytes());
        out.extend_from_slice(if o.pretty { b": " } else { b"\n" });
    }
    if o.functions {
        out.extend_from_slice(b"??");
        out.push(if o.pretty { b' ' } else { b'\n' });
    }
    out.extend_from_slice(b"??:0\n");
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
        addresses: false,
        functions: false,
        pretty: false,
    };
    let mut exe: Vec<u8> = b"a.out".to_vec();
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(x) => x,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&prog)));
                return usage(&prog, false);
            }
        };
        let arg = opt.arg.clone().unwrap_or_default();
        match u8::try_from(opt.id).unwrap_or(0) {
            b'a' => o.addresses = true,
            b'f' => o.functions = true,
            b'p' => o.pretty = true,
            b'e' => exe = arg,
            b'b' | b'j' | b'C' | b'i' | b's' | b'r' | b'R' => {}
            b'h' | b'H' => return usage(&prog, true),
            b'V' | b'v' => {
                super::ar::print_version("addr2line");
                return 0;
            }
            _ => {}
        }
    }
    let addrs = g.operands();
    match sys::stat(&exe) {
        Err(Errno::ENOENT) => {
            io::eprint(format!("{prog}: '{}': No such file\n", io::lossy(&exe)));
            return 1;
        }
        Err(e) => {
            io::eprint(format!(
                "{prog}: {}: {}\n",
                io::lossy(&exe),
                e.message()
            ));
            return 1;
        }
        Ok(st) if st.file_type() == FileType::Directory => {
            io::eprint(format!(
                "{prog}: Warning: '{}' is a directory\n",
                io::lossy(&exe)
            ));
            return 1;
        }
        Ok(_) => {}
    }
    let data = match io::read_path(&exe) {
        Ok(d) => d,
        Err(e) => {
            io::eprint(format!(
                "{prog}: {}: {}\n",
                io::lossy(&exe),
                e.message()
            ));
            return 1;
        }
    };
    if Elf::parse(&data).is_none() {
        io::eprint(format!(
            "{prog}: {}: file format not recognized\n",
            io::lossy(&exe)
        ));
        return 1;
    }
    let mut out: Vec<u8> = Vec::new();
    if addrs.is_empty() {
        let input = io::read_stdin().unwrap_or_default();
        for line in input.split(|&b| b == b'\n') {
            let t = line.trim_ascii();
            if t.is_empty() {
                continue;
            }
            emit(&mut out, &o, parse_addr(t));
        }
    } else {
        for a in &addrs {
            emit(&mut out, &o, parse_addr(a));
        }
    }
    let mut so = io::stdout();
    let _ = so.write_all(&out);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_forms() {
        assert_eq!(parse_addr(b"0x10"), 16);
        assert_eq!(parse_addr(b"ff"), 255);
        assert_eq!(parse_addr(b"zz"), 0);
    }

    #[test]
    fn unknown_output() {
        let o = Opts {
            addresses: true,
            functions: true,
            pretty: true,
        };
        let mut v = Vec::new();
        emit(&mut v, &o, 0x1234);
        assert_eq!(v, b"0x0000000000001234: ?? ??:0\n");
    }
}
