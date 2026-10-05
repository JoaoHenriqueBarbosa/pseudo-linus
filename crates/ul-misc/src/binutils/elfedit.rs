//! `elfedit` do GNU binutils 2.44 (Debian 13): altera in loco o tipo, a máquina e o OSABI do
//! cabeçalho de ELF64 little-endian.
//!
//! Escrito a partir do comportamento observado e da man page (o código do binutils é GPL e não
//! foi consultado). As opções `--input-*` só deixam o arquivo passar se o valor atual casar.
//!
//! Divergências conhecidas: `--enable-x86-feature` e `--disable-x86-feature` são aceitas sem
//! efeito (não editam `.note.gnu.property`); só ELF64 little-endian é editado; os textos de erro
//! para valores desconhecidos foram escritos de memória e precisam de conferência com o oráculo.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, FileType, OFlags, sys};

use crate::strings::expand_response_files;
use crate::util::io::{self, File};
use crate::util::{Getopt, HasArg, LongOpt};

const SHORTOPTS: &str = "hv";

const ID_INPUT_MACH: i32 = 256;
const ID_OUTPUT_MACH: i32 = 257;
const ID_INPUT_TYPE: i32 = 258;
const ID_OUTPUT_TYPE: i32 = 259;
const ID_INPUT_OSABI: i32 = 260;
const ID_OUTPUT_OSABI: i32 = 261;
const ID_ENABLE_X86: i32 = 262;
const ID_DISABLE_X86: i32 = 263;

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("input-mach", HasArg::Required, ID_INPUT_MACH),
    LongOpt::new("output-mach", HasArg::Required, ID_OUTPUT_MACH),
    LongOpt::new("input-type", HasArg::Required, ID_INPUT_TYPE),
    LongOpt::new("output-type", HasArg::Required, ID_OUTPUT_TYPE),
    LongOpt::new("input-osabi", HasArg::Required, ID_INPUT_OSABI),
    LongOpt::new("output-osabi", HasArg::Required, ID_OUTPUT_OSABI),
    LongOpt::new("enable-x86-feature", HasArg::Required, ID_ENABLE_X86),
    LongOpt::new("disable-x86-feature", HasArg::Required, ID_DISABLE_X86),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("version", HasArg::No, 'v' as i32),
];

const USAGE_BODY: &str = " Update the ELF header of ELF files\n\
\x20 The options are:\n\
\x20 --input-mach <machine>      Set input machine type to <machine>\n\
\x20 --output-mach <machine>     Set output machine type to <machine>\n\
\x20 --input-type <type>         Set input file type to <type>\n\
\x20 --output-type <type>        Set output file type to <type>\n\
\x20 --input-osabi <osabi>       Set input OSABI to <osabi>\n\
\x20 --output-osabi <osabi>      Set output OSABI to <osabi>\n\
\x20 --enable-x86-feature <feature>\n\
\x20                             Enable x86 feature <feature>\n\
\x20 --disable-x86-feature <feature>\n\
\x20                             Disable x86 feature <feature>\n\
\x20 -h --help                   Display this information\n\
\x20 -v --version                Display the version number of elfedit\n";

#[derive(Default)]
struct Opts {
    input_mach: Option<u16>,
    output_mach: Option<u16>,
    input_type: Option<u16>,
    output_type: Option<u16>,
    input_osabi: Option<u8>,
    output_osabi: Option<u8>,
    x86_features: bool,
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(prog: &str, to_stdout: bool) -> i32 {
    let mut text = format!("Usage: {prog} <option(s)> elffile(s)\n");
    text.push_str(USAGE_BODY);
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

fn parse_number(s: &[u8]) -> Option<u64> {
    let t = std::str::from_utf8(s).ok()?;
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).ok()
    } else {
        t.parse().ok()
    }
}

fn machine_value(s: &[u8]) -> Option<u16> {
    match s.to_ascii_lowercase().as_slice() {
        b"i386" => Some(3),
        b"iamcu" => Some(6),
        b"l1om" => Some(180),
        b"k1om" => Some(181),
        b"x86_64" | b"x86-64" => Some(62),
        _ => None,
    }
}

fn type_value(s: &[u8]) -> Option<u16> {
    match s.to_ascii_lowercase().as_slice() {
        b"none" => Some(0),
        b"rel" => Some(1),
        b"exec" => Some(2),
        b"dyn" => Some(3),
        _ => None,
    }
}

fn osabi_value(s: &[u8]) -> Option<u8> {
    let table: &[(&str, u8)] = &[
        ("none", 0),
        ("sysv", 0),
        ("hpux", 1),
        ("netbsd", 2),
        ("gnu", 3),
        ("linux", 3),
        ("solaris", 6),
        ("aix", 7),
        ("irix", 8),
        ("freebsd", 9),
        ("tru64", 10),
        ("modesto", 11),
        ("openbsd", 12),
        ("openvms", 13),
        ("nsk", 14),
        ("aros", 15),
        ("fenixos", 16),
        ("cloudabi", 17),
        ("arm_aeabi", 64),
        ("arm", 97),
        ("standalone", 255),
    ];
    let lower = s.to_ascii_lowercase();
    for (n, v) in table {
        if n.as_bytes() == lower.as_slice() {
            return Some(*v);
        }
    }
    parse_number(s).and_then(|v| u8::try_from(v).ok())
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
    let mut o = Opts::default();
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(x) => x,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&prog)));
                return usage(&prog, false);
            }
        };
        let arg = opt.arg.clone().unwrap_or_default();
        let bad = |what: &str| -> i32 {
            io::eprint(format!(
                "{prog}: Error: Unknown {what}: {}\n",
                io::lossy(&arg)
            ));
            usage(&prog, false)
        };
        match opt.id {
            ID_INPUT_MACH => match machine_value(&arg) {
                Some(v) => o.input_mach = Some(v),
                None => return bad("machine type"),
            },
            ID_OUTPUT_MACH => match machine_value(&arg) {
                Some(v) => o.output_mach = Some(v),
                None => return bad("machine type"),
            },
            ID_INPUT_TYPE => match type_value(&arg) {
                Some(v) => o.input_type = Some(v),
                None => return bad("ELF file type"),
            },
            ID_OUTPUT_TYPE => match type_value(&arg) {
                Some(v) => o.output_type = Some(v),
                None => return bad("ELF file type"),
            },
            ID_INPUT_OSABI => match osabi_value(&arg) {
                Some(v) => o.input_osabi = Some(v),
                None => return bad("OSABI"),
            },
            ID_OUTPUT_OSABI => match osabi_value(&arg) {
                Some(v) => o.output_osabi = Some(v),
                None => return bad("OSABI"),
            },
            ID_ENABLE_X86 | ID_DISABLE_X86 => o.x86_features = true,
            id => match u8::try_from(id).unwrap_or(0) {
                b'h' => return usage(&prog, true),
                b'v' => {
                    super::ar::print_version("elfedit");
                    return 0;
                }
                _ => {}
            },
        }
    }
    let files = g.operands();
    let nothing = o.input_mach.is_none()
        && o.output_mach.is_none()
        && o.input_type.is_none()
        && o.output_type.is_none()
        && o.input_osabi.is_none()
        && o.output_osabi.is_none()
        && !o.x86_features;
    if files.is_empty() || nothing {
        return usage(&prog, false);
    }
    let mut status = 0;
    for f in &files {
        if process(&prog, f, &o).is_err() {
            status = 1;
        }
    }
    status
}

fn process(prog: &str, path: &[u8], o: &Opts) -> Result<(), ()> {
    let say = |parts: &[&[u8]]| {
        let mut m = format!("{prog}: Error: ").into_bytes();
        for p in parts {
            m.extend_from_slice(p);
        }
        m.push(b'\n');
        io::eprint(m);
    };
    let st = match sys::stat(path) {
        Err(Errno::ENOENT) => {
            say(&[b"'", path, b"': No such file"]);
            return Err(());
        }
        Err(e) => {
            say(&[path, b": ", e.message().as_bytes()]);
            return Err(());
        }
        Ok(st) if st.file_type() == FileType::Directory => {
            say(&[b"'", path, b"' is not an ordinary file"]);
            return Err(());
        }
        Ok(st) => st,
    };
    let mut data = match io::read_path(path) {
        Ok(d) => d,
        Err(e) => {
            say(&[path, b": ", e.message().as_bytes()]);
            return Err(());
        }
    };
    if data.len() < 64 {
        say(&[path, b": Failed to read ELF header"]);
        return Err(());
    }
    if &data[..4] != b"\x7fELF" {
        say(&[
            path,
            b": not an ELF file - it has the wrong magic bytes at the start",
        ]);
        return Err(());
    }
    if data[4] != 2 || data[5] != 1 {
        say(&[path, b": Unsupported ELF class or data encoding"]);
        return Err(());
    }
    let cur_type = u16::from_le_bytes([data[16], data[17]]);
    let cur_mach = u16::from_le_bytes([data[18], data[19]]);
    let cur_osabi = data[7];
    if o.input_type.is_some_and(|v| v != cur_type)
        || o.input_mach.is_some_and(|v| v != cur_mach)
        || o.input_osabi.is_some_and(|v| v != cur_osabi)
    {
        return Ok(());
    }
    if let Some(v) = o.output_type {
        data[16..18].copy_from_slice(&v.to_le_bytes());
    }
    if let Some(v) = o.output_mach {
        data[18..20].copy_from_slice(&v.to_le_bytes());
    }
    if let Some(v) = o.output_osabi {
        data[7] = v;
    }
    let write = || -> Result<(), Errno> {
        let mut f = File::open_with(path, OFlags::WRONLY, st.mode & 0o777)?;
        f.write_all(&data).map_err(|e| io::io_errno(&e))
    };
    if let Err(e) = write() {
        say(&[path, b": ", e.message().as_bytes()]);
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(type_value(b"DYN"), Some(3));
        assert_eq!(machine_value(b"x86_64"), Some(62));
        assert_eq!(osabi_value(b"Linux"), Some(3));
        assert_eq!(osabi_value(b"0x40"), Some(64));
        assert_eq!(osabi_value(b"zzz"), None);
    }
}
