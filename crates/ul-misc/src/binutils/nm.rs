//! `nm` do GNU binutils 2.44 (Debian 13) sobre ELF64 x86_64: lista os símbolos de `.symtab`.
//!
//! Escrito a partir do comportamento observado no oráculo e da man page (o código do binutils é
//! GPL e não foi consultado).
//!
//! Opções: `-a`, `-g`, `-u`, `-n`/`-v`, `-p`, `-r`, `-A`, `-S`, `-t {d,o,x}`/`--radix`,
//! `--defined-only`, `-V`/`--version`. Divergências conhecidas: `--help` (o texto não foi
//! capturado), `-D`, `-C`, `-l`, `-s`, `-P`, `-f` e arquivos `.a` não existem; só ELF64
//! little-endian.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, FileType, sys};

use crate::strings::expand_response_files;
use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

use super::elf::{
    Elf, STB_GNU_UNIQUE, STB_LOCAL, STB_WEAK, STT_FILE, STT_GNU_IFUNC, STT_OBJECT, STT_SECTION,
    SHF_ALLOC, SHF_EXECINSTR, SHF_WRITE, SHN_ABS, SHN_COMMON, SHN_LORESERVE, SHN_UNDEF,
    SHT_NOBITS, Sym,
};

const SHORTOPTS: &str = "agunvprASVt:";

const ID_RADIX: i32 = 256;
const ID_DEFINED: i32 = 257;

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("debug-syms", HasArg::No, 'a' as i32),
    LongOpt::new("extern-only", HasArg::No, 'g' as i32),
    LongOpt::new("undefined-only", HasArg::No, 'u' as i32),
    LongOpt::new("numeric-sort", HasArg::No, 'n' as i32),
    LongOpt::new("no-sort", HasArg::No, 'p' as i32),
    LongOpt::new("reverse-sort", HasArg::No, 'r' as i32),
    LongOpt::new("print-file-name", HasArg::No, 'A' as i32),
    LongOpt::new("print-size", HasArg::No, 'S' as i32),
    LongOpt::new("radix", HasArg::Required, ID_RADIX),
    LongOpt::new("defined-only", HasArg::No, ID_DEFINED),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

#[derive(Default)]
struct Opts {
    all: bool,
    extern_only: bool,
    undefined_only: bool,
    defined_only: bool,
    numeric: bool,
    no_sort: bool,
    reverse: bool,
    print_file: bool,
    print_size: bool,
    radix: u8,
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
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
        radix: b'x',
        ..Opts::default()
    };
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(x) => x,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&prog)));
                return 1;
            }
        };
        let arg = opt.arg.clone().unwrap_or_default();
        match opt.id {
            ID_RADIX => match arg.as_slice() {
                [c @ (b'd' | b'o' | b'x')] => o.radix = *c,
                _ => {
                    io::eprint(format!("{prog}: invalid radix\n"));
                    return 1;
                }
            },
            ID_DEFINED => o.defined_only = true,
            id => match u8::try_from(id).unwrap_or(0) {
                b'a' => o.all = true,
                b'g' => o.extern_only = true,
                b'u' => o.undefined_only = true,
                b'n' | b'v' => o.numeric = true,
                b'p' => o.no_sort = true,
                b'r' => o.reverse = true,
                b'A' => o.print_file = true,
                b'S' => o.print_size = true,
                b't' => match arg.as_slice() {
                    [c @ (b'd' | b'o' | b'x')] => o.radix = *c,
                    _ => {
                        io::eprint(format!("{prog}: invalid radix\n"));
                        return 1;
                    }
                },
                b'V' => {
                    super::ar::print_version("nm");
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
    let multiple = files.len() > 1;
    let mut status = 0;
    for f in &files {
        if process(&prog, f, &o, multiple).is_err() {
            status = 1;
        }
    }
    status
}

/// A letra que o `nm` mostra para o símbolo.
fn type_letter(elf: &Elf<'_>, s: &Sym) -> char {
    if s.kind() == STT_FILE {
        return 'f';
    }
    let weak = s.bind() == STB_WEAK;
    if s.shndx == SHN_UNDEF {
        return if weak {
            if s.kind() == STT_OBJECT { 'v' } else { 'w' }
        } else {
            'U'
        };
    }
    if s.shndx == SHN_ABS {
        return if s.bind() == STB_LOCAL { 'a' } else { 'A' };
    }
    if s.shndx == SHN_COMMON {
        return 'C';
    }
    if s.kind() == STT_GNU_IFUNC {
        return 'i';
    }
    if s.bind() == STB_GNU_UNIQUE {
        return 'u';
    }
    if weak {
        return if s.kind() == STT_OBJECT { 'V' } else { 'W' };
    }
    let lower = match elf.sections.get(usize::from(s.shndx)) {
        Some(sec) if sec.flags & SHF_EXECINSTR != 0 => 't',
        Some(sec) if sec.flags & SHF_ALLOC != 0 => {
            if sec.kind == SHT_NOBITS {
                'b'
            } else if sec.flags & SHF_WRITE != 0 {
                'd'
            } else {
                'r'
            }
        }
        Some(sec) if sec.name.starts_with(b".debug") => 'N',
        Some(_) => 'n',
        None => '?',
    };
    if s.bind() == STB_LOCAL || lower == 'N' {
        lower
    } else {
        lower.to_ascii_uppercase()
    }
}

fn fmt_num(v: u64, radix: u8) -> String {
    match radix {
        b'd' => format!("{v:016}"),
        b'o' => format!("{v:016o}"),
        _ => format!("{v:016x}"),
    }
}

fn process(prog: &str, path: &[u8], o: &Opts, multiple: bool) -> Result<(), ()> {
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
            return Err(());
        }
        Err(e) => {
            say(&[path, b": ", e.message().as_bytes()]);
            return Err(());
        }
        Ok(st) if st.file_type() == FileType::Directory => {
            say(&[b"Warning: '", path, b"' is a directory"]);
            return Err(());
        }
        Ok(_) => {}
    }
    let data = match io::read_path(path) {
        Ok(d) => d,
        Err(e) => {
            say(&[path, b": ", e.message().as_bytes()]);
            return Err(());
        }
    };
    let Some(elf) = Elf::parse(&data) else {
        say(&[path, b": file format not recognized"]);
        return Err(());
    };
    let Some(mut syms) = elf.symbols() else {
        say(&[path, b": no symbols"]);
        return Ok(());
    };
    for s in &mut syms {
        if s.kind() == STT_SECTION && s.name.is_empty() {
            if let Some(sec) = elf.sections.get(usize::from(s.shndx)) {
                if s.shndx < SHN_LORESERVE {
                    s.name = sec.name.clone();
                }
            }
        }
    }
    let mut shown: Vec<(char, &Sym)> = Vec::new();
    for s in &syms {
        if !o.all && matches!(s.kind(), STT_SECTION | STT_FILE) {
            continue;
        }
        let letter = type_letter(&elf, s);
        let undefined = s.shndx == SHN_UNDEF;
        if o.extern_only && s.bind() == STB_LOCAL {
            continue;
        }
        if o.undefined_only && !undefined {
            continue;
        }
        if o.defined_only && undefined {
            continue;
        }
        shown.push((letter, s));
    }
    if !o.no_sort {
        if o.numeric {
            shown.sort_by(|a, b| {
                let (ua, ub) = (a.1.shndx == SHN_UNDEF, b.1.shndx == SHN_UNDEF);
                ub.cmp(&ua)
                    .then(a.1.value.cmp(&b.1.value))
                    .then_with(|| a.1.name.cmp(&b.1.name))
            });
            // Indefinidos primeiro, como no original.
            shown.sort_by_key(|x| x.1.shndx != SHN_UNDEF);
        } else {
            shown.sort_by(|a, b| a.1.name.cmp(&b.1.name));
        }
        if o.reverse {
            shown.reverse();
        }
    }
    let mut out = Vec::new();
    if multiple {
        out.extend_from_slice(b"\n");
        out.extend_from_slice(path);
        out.extend_from_slice(b":\n");
    }
    for (letter, s) in shown {
        if o.print_file {
            out.extend_from_slice(path);
            out.push(b':');
        }
        let blank = s.shndx == SHN_UNDEF;
        if blank {
            out.extend_from_slice(b"                ");
        } else {
            out.extend_from_slice(fmt_num(s.value, o.radix).as_bytes());
        }
        if o.print_size && !blank {
            out.push(b' ');
            out.extend_from_slice(fmt_num(s.size, o.radix).as_bytes());
        }
        out.push(b' ');
        out.push(letter as u8);
        out.push(b' ');
        out.extend_from_slice(&s.name);
        out.push(b'\n');
    }
    let mut so = io::stdout();
    let _ = so.write_all(&out);
    Ok(())
}
