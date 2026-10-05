//! `size` do GNU binutils 2.44 (Debian 13) sobre ELF64 x86_64, formatos Berkeley e System V.
//!
//! Escrito a partir do comportamento observado no oráculo e da man page (o código do binutils é
//! GPL e não foi consultado). Contas do formato Berkeley: seção alocada executável ou somente
//! leitura conta em `text`; alocada gravável com conteúdo em `data`; alocada sem conteúdo em
//! `bss`.
//!
//! Divergências conhecidas: arquivos `.a` não são abertos (saem como formato não reconhecido),
//! só existe ELF64 little-endian, e o formato `gnu` (`-G`) é tratado como Berkeley.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, FileType, sys};

use crate::strings::{TARGETS, expand_response_files};
use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

use super::elf::{Elf, SHF_ALLOC, SHF_EXECINSTR, SHF_WRITE, SHN_COMMON, SHT_NOBITS};

const SHORTOPTS: &str = "ABGHhVvodxt";

const ID_FORMAT: i32 = 256;
const ID_RADIX: i32 = 257;
const ID_COMMON: i32 = 258;
const ID_TARGET: i32 = 259;

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("format", HasArg::Required, ID_FORMAT),
    LongOpt::new("radix", HasArg::Required, ID_RADIX),
    LongOpt::new("totals", HasArg::No, 't' as i32),
    LongOpt::new("common", HasArg::No, ID_COMMON),
    LongOpt::new("target", HasArg::Required, ID_TARGET),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("version", HasArg::No, 'v' as i32),
];

const USAGE_BODY: &str = " Displays the sizes of sections inside binary files\n\
\x20If no input file(s) are specified, a.out is assumed\n\
\x20The options are:\n\
\x20 -A|-B|-G  --format={sysv|berkeley|gnu}  Select output style (default is berkeley)\n\
\x20 -o|-d|-x  --radix={8|10|16}         Display numbers in octal, decimal or hex\n\
\x20 -t        --totals                  Display the total sizes (Berkeley only)\n\
\x20 -f                                  Ignored.\n\
\x20           --common                  Display total size for *COM* syms\n\
\x20           --target=<bfdname>        Set the binary file format\n\
\x20           @<file>                   Read options from <file>\n\
\x20 -h|-H|-?  --help                    Display this information\n\
\x20 -v|-V     --version                 Display the program's version\n\
\n";

#[derive(Copy, Clone, PartialEq, Eq)]
enum Format {
    Berkeley,
    SysV,
}

struct Opts {
    format: Format,
    radix: u32,
    totals: bool,
    common: bool,
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(prog: &str, to_stdout: bool) -> i32 {
    let mut text = format!("Usage: {prog} [option(s)] [file(s)]\n");
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
        format: Format::Berkeley,
        radix: 10,
        totals: false,
        common: false,
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
            ID_FORMAT => match arg.to_ascii_lowercase().as_slice() {
                b"sysv" => o.format = Format::SysV,
                b"berkeley" | b"gnu" => o.format = Format::Berkeley,
                _ => {
                    io::eprint(format!(
                        "{prog}: invalid argument to --format: {}\n",
                        io::lossy(&arg)
                    ));
                    return usage(&prog, false);
                }
            },
            ID_RADIX => match arg.as_slice() {
                b"8" => o.radix = 8,
                b"10" => o.radix = 10,
                b"16" => o.radix = 16,
                _ => {
                    io::eprint(format!(
                        "{prog}: Invalid radix: {}\n",
                        io::lossy(&arg)
                    ));
                    return usage(&prog, false);
                }
            },
            ID_COMMON => o.common = true,
            ID_TARGET => {}
            id => match u8::try_from(id).unwrap_or(0) {
                b'A' => o.format = Format::SysV,
                b'B' | b'G' => o.format = Format::Berkeley,
                b'o' => o.radix = 8,
                b'd' => o.radix = 10,
                b'x' => o.radix = 16,
                b't' => o.totals = true,
                b'h' | b'H' => return usage(&prog, true),
                b'v' | b'V' => {
                    super::ar::print_version("size");
                    return 0;
                }
                _ => {}
            },
        }
    }
    let mut files = g.operands();
    if files.is_empty() {
        files.push(b"a.out".to_vec());
    }
    let mut status = 0;
    let mut header_done = false;
    let mut totals = (0u64, 0u64, 0u64);
    let mut count = 0;
    for f in &files {
        match process(&prog, f, &o, &mut header_done) {
            Ok(t) => {
                totals.0 += t.0;
                totals.1 += t.1;
                totals.2 += t.2;
                count += 1;
            }
            Err(c) => status = c,
        }
    }
    if o.format == Format::Berkeley && o.totals && count > 0 {
        let mut out = io::stdout();
        let _ = out.write_all(berkeley_row(totals, &o, b"(TOTALS)").as_bytes());
    }
    status
}

fn berkeley_row(t: (u64, u64, u64), o: &Opts, name: &[u8]) -> String {
    let sum = t.0 + t.1 + t.2;
    let num = |v: u64| match o.radix {
        8 => format!("{:>7}", format!("{v:#o}")),
        16 => format!("{:>7}", format!("{v:#x}")),
        _ => format!("{v:>7}"),
    };
    let hex = match o.radix {
        10 => format!("{sum:>7x}"),
        _ => format!("{:>7}", format!("{sum:#x}")),
    };
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\n",
        num(t.0),
        num(t.1),
        num(t.2),
        num(sum),
        hex,
        io::lossy(name)
    )
}

fn process(
    prog: &str,
    path: &[u8],
    o: &Opts,
    header_done: &mut bool,
) -> Result<(u64, u64, u64), i32> {
    let say = |parts: &[&[u8]]| {
        let mut m = format!("{prog}: ").into_bytes();
        for p in parts {
            m.extend_from_slice(p);
        }
        m.push(b'\n');
        io::eprint(m);
    };
    match sys::stat(path) {
        Err(Errno::ENOENT) => {
            say(&[b"'", path, b"': No such file"]);
            return Err(1);
        }
        Err(e) => {
            say(&[path, b": ", e.message().as_bytes()]);
            return Err(1);
        }
        Ok(st) if st.file_type() == FileType::Directory => {
            say(&[b"Warning: '", path, b"' is a directory"]);
            return Err(1);
        }
        Ok(_) => {}
    }
    let data = match io::read_path(path) {
        Ok(d) => d,
        Err(e) => {
            say(&[path, b": ", e.message().as_bytes()]);
            return Err(1);
        }
    };
    let Some(elf) = Elf::parse(&data) else {
        say(&[path, b": file format not recognized"]);
        return Err(3);
    };
    let mut text = 0u64;
    let mut dat = 0u64;
    let mut bss = 0u64;
    let mut common = 0u64;
    if o.common {
        if let Some(syms) = elf.symbols() {
            common = syms
                .iter()
                .filter(|s| s.shndx == SHN_COMMON)
                .map(|s| s.size)
                .sum();
        }
    }
    let mut out = io::stdout();
    match o.format {
        Format::Berkeley => {
            for s in elf.sections.iter().skip(1) {
                if s.flags & SHF_ALLOC == 0 {
                    continue;
                }
                if s.flags & SHF_EXECINSTR != 0 || s.flags & SHF_WRITE == 0 {
                    text += s.size;
                } else if s.kind != SHT_NOBITS {
                    dat += s.size;
                } else {
                    bss += s.size;
                }
            }
            bss += common;
            if !*header_done {
                let _ = out.write_all(
                    b"   text\t   data\t    bss\t    dec\t    hex\tfilename\n",
                );
                *header_done = true;
            }
            let _ = out.write_all(berkeley_row((text, dat, bss), o, path).as_bytes());
        }
        Format::SysV => {
            let mut rows: Vec<(Vec<u8>, u64, u64)> = Vec::new();
            let mut total = 0u64;
            for s in elf.sections.iter().skip(1) {
                if s.flags & SHF_ALLOC == 0 {
                    continue;
                }
                rows.push((s.name.clone(), s.size, s.addr));
                total += s.size;
            }
            if o.common && common > 0 {
                rows.push((b"*COM*".to_vec(), common, 0));
                total += common;
            }
            let fmt = |v: u64| match o.radix {
                8 => format!("{v:#o}"),
                16 => format!("{v:#x}"),
                _ => v.to_string(),
            };
            let mut text_out = format!("{}  :\n", io::lossy(path)).into_bytes();
            let name_w = rows
                .iter()
                .map(|r| r.0.len())
                .chain([5, 7])
                .max()
                .unwrap_or(7)
                + 1;
            let head = format!("{:<name_w$}{:>6}{:>10}\n", "section", "size", "addr");
            text_out.extend_from_slice(head.as_bytes());
            for (n, size, addr) in &rows {
                let mut line = n.clone();
                line.resize(name_w.max(n.len()), b' ');
                line.extend_from_slice(format!("{:>6}{:>10}\n", fmt(*size), fmt(*addr)).as_bytes());
                text_out.extend_from_slice(&line);
            }
            text_out.extend_from_slice(
                format!("{:<name_w$}{:>6}\n\n\n", "Total", fmt(total)).as_bytes(),
            );
            let _ = out.write_all(&text_out);
        }
    }
    Ok((text, dat, bss))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn berkeley_row_decimal() {
        let o = Opts {
            format: Format::Berkeley,
            radix: 10,
            totals: false,
            common: false,
        };
        assert_eq!(
            berkeley_row((1, 2, 3), &o, b"f"),
            "      1\t      2\t      3\t      6\t      6\tf\n"
        );
    }
}
