//! `readelf` do GNU binutils 2.44 (Debian 13) sobre ELF64 x86_64 little-endian.
//!
//! Escrito a partir do comportamento observado e da man page (o código do binutils é GPL e não
//! foi consultado). Implementa `-h`, `-l`, `-S`, `-e`, `-s`, `--dyn-syms`, `-d`, `-W`, `-T`,
//! `-a` (como `-h -l -S -s -d`) e `-v`/`--version`, `-H`/`--help`.
//!
//! Divergências conhecidas: `-r`, `-n`, `-V`, `-u`, `-A`, `-I`, `-g`, `-t`, `-x`, `-p`, `-R`,
//! `-w` são aceitos mas não produzem saída; só ELF64 little-endian; arquivos `.a` não são abertos.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, FileType, sys};
use ul_common::ctype::{cstr, cstr_at};

use crate::strings::expand_response_files;
use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

use super::elf::{Versions, cstr_lossy, rd16, rd32, rd64, segment_type_name, slice_at};

const SHORTOPTS: &str = "ahlSegtsnrudVAcDLCvHWTzIx:p:R:j:";

const ID_SEGMENTS: i32 = 256;
const ID_SECTIONS: i32 = 257;
const ID_SYMBOLS: i32 = 258;
const ID_DYN_SYMS: i32 = 259;

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("all", HasArg::No, 'a' as i32),
    LongOpt::new("file-header", HasArg::No, 'h' as i32),
    LongOpt::new("program-headers", HasArg::No, 'l' as i32),
    LongOpt::new("segments", HasArg::No, ID_SEGMENTS),
    LongOpt::new("section-headers", HasArg::No, 'S' as i32),
    LongOpt::new("sections", HasArg::No, ID_SECTIONS),
    LongOpt::new("headers", HasArg::No, 'e' as i32),
    LongOpt::new("section-groups", HasArg::No, 'g' as i32),
    LongOpt::new("section-details", HasArg::No, 't' as i32),
    LongOpt::new("syms", HasArg::No, 's' as i32),
    LongOpt::new("symbols", HasArg::No, ID_SYMBOLS),
    LongOpt::new("dyn-syms", HasArg::No, ID_DYN_SYMS),
    LongOpt::new("notes", HasArg::No, 'n' as i32),
    LongOpt::new("relocs", HasArg::No, 'r' as i32),
    LongOpt::new("unwind", HasArg::No, 'u' as i32),
    LongOpt::new("dynamic", HasArg::No, 'd' as i32),
    LongOpt::new("version-info", HasArg::No, 'V' as i32),
    LongOpt::new("arch-specific", HasArg::No, 'A' as i32),
    LongOpt::new("archive-index", HasArg::No, 'c' as i32),
    LongOpt::new("use-dynamic", HasArg::No, 'D' as i32),
    LongOpt::new("demangle", HasArg::No, 'C' as i32),
    LongOpt::new("histogram", HasArg::No, 'I' as i32),
    LongOpt::new("wide", HasArg::No, 'W' as i32),
    LongOpt::new("silent-truncation", HasArg::No, 'T' as i32),
    LongOpt::new("hex-dump", HasArg::Required, 'x' as i32),
    LongOpt::new("string-dump", HasArg::Required, 'p' as i32),
    LongOpt::new("relocated-dump", HasArg::Required, 'R' as i32),
    LongOpt::new("display-section", HasArg::Required, 'j' as i32),
    LongOpt::new("decompress", HasArg::No, 'z' as i32),
    LongOpt::new("help", HasArg::No, 'H' as i32),
    LongOpt::new("version", HasArg::No, 'v' as i32),
];

/// Corpo do usage, uma linha por item (preserva espaços iniciais e finais byte a byte).
const USAGE_LINES: &[&str] = &[
    " Display information about the contents of ELF format files",
    " Options are:",
    "  -a --all               Equivalent to: -h -l -S -s -r -d -V -A -I",
    "  -h --file-header       Display the ELF file header",
    "  -l --program-headers   Display the program headers",
    "     --segments          An alias for --program-headers",
    "  -S --section-headers   Display the sections' header",
    "     --sections          An alias for --section-headers",
    "  -g --section-groups    Display the section groups",
    "  -t --section-details   Display the section details",
    "  -e --headers           Equivalent to: -h -l -S",
    "  -s --syms              Display the symbol table",
    "     --symbols           An alias for --syms",
    "     --dyn-syms          Display the dynamic symbol table",
    "     --lto-syms          Display LTO symbol tables",
    "     --sym-base=[0|8|10|16] ",
    "                         Force base for symbol sizes.  The options are ",
    "                         mixed (the default), octal, decimal, hexadecimal.",
    "  -C --demangle[=STYLE]  Decode mangled/processed symbol names",
    "                           STYLE can be \"none\", \"auto\", \"gnu-v3\", \"java\",",
    "                           \"gnat\", \"dlang\", \"rust\"",
    "     --no-demangle       Do not demangle low-level symbol names.  (default)",
    "     --recurse-limit     Enable a demangling recursion limit.  (default)",
    "     --no-recurse-limit  Disable a demangling recursion limit",
    "     -U[dlexhi] --unicode=[default|locale|escape|hex|highlight|invalid]",
    "                         Display unicode characters as determined by the current locale",
    "                          (default), escape sequences, \"<hex sequences>\", highlighted",
    "                          escape sequences, or treat them as invalid and display as",
    "                          \"{hex sequences}\"",
    "     -X --extra-sym-info Display extra information when showing symbols",
    "     --no-extra-sym-info Do not display extra information when showing symbols (default)",
    "  -n --notes             Display the contents of note sections (if present)",
    "  -r --relocs            Display the relocations (if present)",
    "  -u --unwind            Display the unwind info (if present)",
    "  -d --dynamic           Display the dynamic section (if present)",
    "  -V --version-info      Display the version sections (if present)",
    "  -A --arch-specific     Display architecture specific information (if any)",
    "  -c --archive-index     Display the symbol/file index in an archive",
    "  -D --use-dynamic       Use the dynamic section info when displaying symbols",
    "  -L --lint|--enable-checks",
    "                         Display warning messages for possible problems",
    "  -x --hex-dump=<number|name>",
    "                         Dump the contents of section <number|name> as bytes",
    "  -p --string-dump=<number|name>",
    "                         Dump the contents of section <number|name> as strings",
    "  -R --relocated-dump=<number|name>",
    "                         Dump the relocated contents of section <number|name>",
    "  -z --decompress        Decompress section before dumping it",
    "",
    "  -j --display-section=<name|number>",
    "\t\t         Display the contents of the indicated section.  Can be repeated",
    "  -w --debug-dump[a/=abbrev, A/=addr, r/=aranges, c/=cu_index, L/=decodedline,",
    "                  f/=frames, F/=frames-interp, g/=gdb_index, i/=info, o/=loc,",
    "                  m/=macro, p/=pubnames, t/=pubtypes, R/=Ranges, l/=rawline,",
    "                  s/=str, O/=str-offsets, u/=trace_abbrev, T/=trace_aranges,",
    "                  U/=trace_info]",
    "                         Display the contents of DWARF debug sections",
    "  -wk --debug-dump=links Display the contents of sections that link to separate",
    "                          debuginfo files",
    "  -P --process-links     Display the contents of non-debug sections in separate",
    "                          debuginfo files.  (Implies -wK)",
    "  -wK --debug-dump=follow-links",
    "                         Follow links to separate debug info files (default)",
    "  -wN --debug-dump=no-follow-links",
    "                         Do not follow links to separate debug info files",
    "  --dwarf-depth=N        Do not display DIEs at depth N or greater",
    "  --dwarf-start=N        Display DIEs starting at offset N",
    "  --ctf=<number|name>    Display CTF info from section <number|name>",
    "  --ctf-parent=<name>    Use CTF archive member <name> as the CTF parent",
    "  --ctf-symbols=<number|name>",
    "                         Use section <number|name> as the CTF external symtab",
    "  --ctf-strings=<number|name>",
    "                         Use section <number|name> as the CTF external strtab",
    "  --sframe[=NAME]        Display SFrame info from section NAME, (default '.sframe')",
    "  -I --histogram         Display histogram of bucket list lengths",
    "  -W --wide              Allow output width to exceed 80 characters",
    "  -T --silent-truncation If a symbol name is truncated, do not add [...] suffix",
    "  @<file>                Read options from <file>",
    "  -H --help              Display this information",
    "  -v --version           Display the version number of readelf",
];

const SHF_ALLOC: u64 = 2;
const SHF_TLS: u64 = 0x400;

const SHT_NOBITS: u32 = 8;
const SHT_SYMTAB: u32 = 2;
const SHT_DYNAMIC: u32 = 6;
const SHT_NOTE: u32 = 7;
const SHT_DYNSYM: u32 = 11;

const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PT_NOTE: u32 = 4;
const PT_PHDR: u32 = 6;
const PT_TLS: u32 = 7;

struct Shdr {
    name: Vec<u8>,
    kind: u32,
    flags: u64,
    addr: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    align: u64,
    entsize: u64,
}

struct Phdr {
    kind: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    paddr: u64,
    filesz: u64,
    memsz: u64,
    align: u64,
}

struct Elf<'a> {
    d: &'a [u8],
    etype: u16,
    machine: u16,
    version: u32,
    entry: u64,
    phoff: u64,
    shoff: u64,
    flags: u32,
    ehsize: u16,
    phentsize: u16,
    phnum: usize,
    shentsize: u16,
    shnum_raw: u16,
    shstrndx_raw: u16,
    shnum: usize,
    shstrndx: usize,
    sh: Vec<Shdr>,
    ph: Vec<Phdr>,
}

impl<'a> Elf<'a> {
    fn parse(d: &'a [u8]) -> Result<Elf<'a>, String> {
        if d.len() < 4 || &d[..4] != b"\x7fELF" {
            return Err("Not an ELF file - it has the wrong magic bytes at the start".to_string());
        }
        if d.len() < 64 {
            return Err("Failed to read file header".to_string());
        }
        if d[4] != 2 || d[5] != 1 {
            return Err("Only ELF64 little-endian files are supported".to_string());
        }
        let bad = || "Failed to read file header".to_string();
        let mut e = Elf {
            d,
            etype: rd16(d, 16).ok_or_else(bad)?,
            machine: rd16(d, 18).ok_or_else(bad)?,
            version: rd32(d, 20).ok_or_else(bad)?,
            entry: rd64(d, 24).ok_or_else(bad)?,
            phoff: rd64(d, 32).ok_or_else(bad)?,
            shoff: rd64(d, 40).ok_or_else(bad)?,
            flags: rd32(d, 48).ok_or_else(bad)?,
            ehsize: rd16(d, 52).ok_or_else(bad)?,
            phentsize: rd16(d, 54).ok_or_else(bad)?,
            phnum: usize::from(rd16(d, 56).ok_or_else(bad)?),
            shentsize: rd16(d, 58).ok_or_else(bad)?,
            shnum_raw: rd16(d, 60).ok_or_else(bad)?,
            shstrndx_raw: rd16(d, 62).ok_or_else(bad)?,
            shnum: 0,
            shstrndx: 0,
            sh: Vec::new(),
            ph: Vec::new(),
        };
        let shoff = usize::try_from(e.shoff).unwrap_or(usize::MAX);
        if shoff != 0 && e.shentsize == 64 {
            let mut shnum = usize::from(e.shnum_raw);
            if shnum == 0 {
                shnum = rd64(d, shoff.saturating_add(32)).unwrap_or(0) as usize;
            }
            e.shstrndx = usize::from(e.shstrndx_raw);
            if e.shstrndx_raw == 0xffff {
                e.shstrndx = rd32(d, shoff.saturating_add(40)).unwrap_or(0) as usize;
            }
            e.shnum = shnum;
            for i in 0..shnum {
                let b = shoff.saturating_add(i.saturating_mul(64));
                let Some(name_off) = rd32(d, b) else { break };
                let (Some(kind), Some(flags), Some(addr), Some(offset), Some(size)) = (
                    rd32(d, b + 4),
                    rd64(d, b + 8),
                    rd64(d, b + 16),
                    rd64(d, b + 24),
                    rd64(d, b + 32),
                ) else {
                    break;
                };
                e.sh.push(Shdr {
                    name: name_off.to_le_bytes().to_vec(),
                    kind,
                    flags,
                    addr,
                    offset,
                    size,
                    link: rd32(d, b + 40).unwrap_or(0),
                    info: rd32(d, b + 44).unwrap_or(0),
                    align: rd64(d, b + 48).unwrap_or(0),
                    entsize: rd64(d, b + 56).unwrap_or(0),
                });
            }
            let strtab = e.section_data(e.shstrndx).to_vec();
            for s in &mut e.sh {
                let off = u32::from_le_bytes([s.name[0], s.name[1], s.name[2], s.name[3]]);
                s.name = cstr_at(&strtab, off as usize).to_vec();
            }
        }
        if e.phnum == 0xffff {
            e.phnum = e.sh.first().map_or(0, |s| s.info as usize);
        }
        let phoff = usize::try_from(e.phoff).unwrap_or(usize::MAX);
        if phoff != 0 && e.phentsize == 56 {
            for i in 0..e.phnum {
                let b = phoff.saturating_add(i.saturating_mul(56));
                let Some(kind) = rd32(d, b) else { break };
                let (Some(flags), Some(offset), Some(vaddr), Some(paddr)) = (
                    rd32(d, b + 4),
                    rd64(d, b + 8),
                    rd64(d, b + 16),
                    rd64(d, b + 24),
                ) else {
                    break;
                };
                e.ph.push(Phdr {
                    kind,
                    flags,
                    offset,
                    vaddr,
                    paddr,
                    filesz: rd64(d, b + 32).unwrap_or(0),
                    memsz: rd64(d, b + 40).unwrap_or(0),
                    align: rd64(d, b + 48).unwrap_or(0),
                });
            }
        }
        Ok(e)
    }

    fn section_data(&self, i: usize) -> &'a [u8] {
        match self.sh.get(i) {
            Some(s) if s.kind != SHT_NOBITS => slice_at(self.d, s.offset, s.size),
            _ => &[],
        }
    }

    /// As versões dos símbolos dinâmicos, lidas das seções `.gnu.version*`.
    fn versions(&self) -> Versions {
        Versions::load(self.sh.iter().enumerate().map(|(i, s)| (s.kind, s.info, self.section_data(i), self.section_data(s.link as usize))))
    }

    fn is_pie(&self) -> bool {
        let Some(dynsec) = self.sh.iter().position(|s| s.kind == SHT_DYNAMIC) else {
            return false;
        };
        for ent in self.section_data(dynsec).chunks_exact(16) {
            let tag = rd64(ent, 0).unwrap_or(0);
            let val = rd64(ent, 8).unwrap_or(0);
            if tag == 0x6fff_fffb && val & 0x0800_0000 != 0 {
                return true;
            }
            if tag == 0 {
                break;
            }
        }
        false
    }

    fn type_text(&self) -> String {
        match self.etype {
            0 => "NONE (None)".to_string(),
            1 => "REL (Relocatable file)".to_string(),
            2 => "EXEC (Executable file)".to_string(),
            3 => {
                if self.is_pie() {
                    "DYN (Position-Independent Executable file)".to_string()
                } else {
                    "DYN (Shared object file)".to_string()
                }
            }
            4 => "CORE (Core file)".to_string(),
            t => format!("<unknown>: {t:x}"),
        }
    }
}

fn machine_text(m: u16) -> String {
    match m {
        0 => "None".to_string(),
        3 => "Intel 80386".to_string(),
        8 => "MIPS R3000".to_string(),
        20 => "PowerPC".to_string(),
        21 => "PowerPC64".to_string(),
        40 => "ARM".to_string(),
        62 => "Advanced Micro Devices X86-64".to_string(),
        183 => "AArch64".to_string(),
        243 => "RISC-V".to_string(),
        _ => format!("<unknown>: 0x{m:x}"),
    }
}

fn osabi_text(o: u8) -> String {
    match o {
        0 => "UNIX - System V".to_string(),
        1 => "UNIX - HP-UX".to_string(),
        2 => "UNIX - NetBSD".to_string(),
        3 => "UNIX - GNU".to_string(),
        6 => "UNIX - Solaris".to_string(),
        9 => "UNIX - FreeBSD".to_string(),
        _ => format!("<unknown: {o:x}>"),
    }
}

fn label(l: &str, v: &str) -> String {
    format!("  {l:<35}{v}\n")
}

fn print_header(e: &Elf<'_>, out: &mut String) {
    out.push_str("ELF Header:\n  Magic:   ");
    for b in &e.d[..16] {
        out.push_str(&format!("{b:02x} "));
    }
    out.push('\n');
    out.push_str(&label("Class:", "ELF64"));
    out.push_str(&label("Data:", "2's complement, little endian"));
    let ver = e.d[6];
    out.push_str(&label(
        "Version:",
        &if ver == 1 {
            "1 (current)".to_string()
        } else {
            format!("{ver}")
        },
    ));
    out.push_str(&label("OS/ABI:", &osabi_text(e.d[7])));
    out.push_str(&label("ABI Version:", &e.d[8].to_string()));
    out.push_str(&label("Type:", &e.type_text()));
    out.push_str(&label("Machine:", &machine_text(e.machine)));
    out.push_str(&label("Version:", &format!("0x{:x}", e.version)));
    out.push_str(&label("Entry point address:", &format!("0x{:x}", e.entry)));
    out.push_str(&label(
        "Start of program headers:",
        &format!("{} (bytes into file)", e.phoff),
    ));
    out.push_str(&label(
        "Start of section headers:",
        &format!("{} (bytes into file)", e.shoff),
    ));
    out.push_str(&label("Flags:", &format!("0x{:x}", e.flags)));
    out.push_str(&label(
        "Size of this header:",
        &format!("{} (bytes)", e.ehsize),
    ));
    out.push_str(&label(
        "Size of program headers:",
        &format!("{} (bytes)", e.phentsize),
    ));
    let phn = rd16(e.d, 56).unwrap_or(0);
    let phn_txt = if phn == 0xffff {
        format!("{} ({})", phn, e.phnum)
    } else {
        phn.to_string()
    };
    out.push_str(&label("Number of program headers:", &phn_txt));
    out.push_str(&label(
        "Size of section headers:",
        &format!("{} (bytes)", e.shentsize),
    ));
    let shn_txt = if e.shnum_raw == 0 && e.shnum != 0 {
        format!("0 ({})", e.shnum)
    } else {
        e.shnum_raw.to_string()
    };
    out.push_str(&label("Number of section headers:", &shn_txt));
    let idx_txt = if e.shstrndx_raw == 0xffff {
        format!("65535 ({})", e.shstrndx)
    } else {
        e.shstrndx_raw.to_string()
    };
    out.push_str(&label("Section header string table index:", &idx_txt));
}

fn sec_type_text(k: u32) -> String {
    match k {
        0 => "NULL".to_string(),
        1 => "PROGBITS".to_string(),
        2 => "SYMTAB".to_string(),
        3 => "STRTAB".to_string(),
        4 => "RELA".to_string(),
        5 => "HASH".to_string(),
        6 => "DYNAMIC".to_string(),
        7 => "NOTE".to_string(),
        8 => "NOBITS".to_string(),
        9 => "REL".to_string(),
        10 => "SHLIB".to_string(),
        11 => "DYNSYM".to_string(),
        14 => "INIT_ARRAY".to_string(),
        15 => "FINI_ARRAY".to_string(),
        16 => "PREINIT_ARRAY".to_string(),
        17 => "GROUP".to_string(),
        18 => "SYMTAB SECTION INDICES".to_string(),
        19 => "RELR".to_string(),
        0x6fff_fff5 => "GNU_ATTRIBUTES".to_string(),
        0x6fff_fff6 => "GNU_HASH".to_string(),
        0x6fff_fff7 => "GNU_LIBLIST".to_string(),
        0x6fff_fffd => "VERDEF".to_string(),
        0x6fff_fffe => "VERNEED".to_string(),
        0x6fff_ffff => "VERSYM".to_string(),
        0x7000_0001 => "X86_64_UNWIND".to_string(),
        k => os_proc_type(k).unwrap_or_else(|| format!("{k:08x}: <unknown>")),
    }
}

/// Tipo de seção ou segmento nas faixas do sistema operacional e do processador.
fn os_proc_type(k: u32) -> Option<String> {
    match k {
        0x6000_0000..0x7000_0000 => Some(format!("LOOS+{:x}", k - 0x6000_0000)),
        0x7000_0000..0x8000_0000 => Some(format!("LOPROC+{:x}", k - 0x7000_0000)),
        _ => None,
    }
}

fn flags_text(f: u64) -> String {
    let mut s = String::new();
    for bit in 0..64 {
        let b = 1u64 << bit;
        if f & b == 0 {
            continue;
        }
        let c = match b {
            0x1 => 'W',
            0x2 => 'A',
            0x4 => 'X',
            0x10 => 'M',
            0x20 => 'S',
            0x40 => 'I',
            0x80 => 'L',
            0x100 => 'O',
            0x200 => 'G',
            0x400 => 'T',
            0x800 => 'C',
            0x8000_0000 => 'E',
            0x1000_0000 => 'l',
            0x0400_0000 => 'D',
            _ if b & 0x0ff0_0000 != 0 => 'o',
            _ if b & 0xf000_0000 != 0 => 'p',
            _ => 'x',
        };
        s.push(c);
    }
    s
}

/// Trunca como o readelf em modo estreito: sobra `width` colunas e o corte leva `[...]`.
fn clip(name: &str, width: usize, silent: bool) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= width {
        return name.to_string();
    }
    if silent {
        chars[..width].iter().collect()
    } else {
        let keep: String = chars[..width.saturating_sub(5)].iter().collect();
        format!("{keep}[...]")
    }
}

fn print_sections(e: &Elf<'_>, wide: bool, silent: bool, summary: bool, out: &mut String) {
    if e.sh.is_empty() {
        out.push_str("\nThere are no sections in this file.\n");
        return;
    }
    if summary {
        if e.sh.len() == 1 {
            out.push_str(&format!(
                "There is 1 section header, starting at offset 0x{:x}:\n",
                e.shoff
            ));
        } else {
            out.push_str(&format!(
                "There are {} section headers, starting at offset 0x{:x}:\n",
                e.sh.len(),
                e.shoff
            ));
        }
    }
    out.push_str("\nSection Headers:\n");
    if wide {
        out.push_str(
            "  [Nr] Name              Type            Address          Off    Size   ES Flg Lk Inf Al\n",
        );
    } else {
        out.push_str("  [Nr] Name              Type             Address           Offset\n");
        out.push_str("       Size              EntSize          Flags  Link  Info  Align\n");
    }
    for (i, s) in e.sh.iter().enumerate() {
        let name = String::from_utf8_lossy(&s.name).into_owned();
        let ty = sec_type_text(s.kind);
        let fl = flags_text(s.flags);
        if wide {
            out.push_str(&format!(
                "  [{i:>2}] {name:<17} {ty:<15} {:016x} {:06x} {:06x} {:02x} {fl:>3} {:>2} {:>3} {:>2}\n",
                s.addr, s.offset, s.size, s.entsize, s.link, s.info, s.align
            ));
        } else {
            let name = clip(&name, 17, silent);
            out.push_str(&format!(
                "  [{i:>2}] {name:<17} {ty:<16} {:016x}  {:08x}\n",
                s.addr, s.offset
            ));
            out.push_str(&format!(
                "       {:016x}  {:016x} {fl:>3} {:>7} {:>5} {:>5}\n",
                s.size, s.entsize, s.link, s.info, s.align
            ));
        }
    }
    out.push_str("Key to Flags:\n");
    out.push_str("  W (write), A (alloc), X (execute), M (merge), S (strings), I (info),\n");
    out.push_str("  L (link order), O (extra OS processing required), G (group), T (TLS),\n");
    out.push_str("  C (compressed), x (unknown), o (OS specific), E (exclude),\n");
    out.push_str("  D (mbind), l (large), p (processor specific)\n");
}

fn seg_type_text(k: u32) -> String {
    match segment_type_name(k) {
        Some(n) => n.to_string(),
        None => os_proc_type(k).unwrap_or_else(|| format!("<unknown>: {k:x}")),
    }
}

fn in_segment(s: &Shdr, p: &Phdr) -> bool {
    if s.size == 0 {
        return false;
    }
    let tls = s.flags & SHF_TLS != 0;
    match p.kind {
        PT_PHDR => return false,
        PT_TLS if !tls => return false,
        PT_NOTE if s.kind != SHT_NOTE => return false,
        PT_DYNAMIC if s.kind != SHT_DYNAMIC => return false,
        _ => {}
    }
    if tls && s.kind == SHT_NOBITS && p.kind != PT_TLS {
        return false;
    }
    if s.flags & SHF_ALLOC == 0 {
        return false;
    }
    let (Some(end), Some(pend)) = (
        s.addr.checked_add(s.size),
        p.vaddr.checked_add(p.memsz),
    ) else {
        return false;
    };
    if s.addr < p.vaddr || end > pend {
        return false;
    }
    if s.kind != SHT_NOBITS {
        let (Some(soff), Some(poff)) = (
            s.offset.checked_add(s.size),
            p.offset.checked_add(p.filesz),
        ) else {
            return false;
        };
        if s.offset < p.offset || soff > poff {
            return false;
        }
    }
    true
}

fn print_segments(e: &Elf<'_>, wide: bool, summary: bool, out: &mut String) {
    if e.ph.is_empty() {
        out.push_str("\nThere are no program headers in this file.\n");
        return;
    }
    if summary {
        out.push_str(&format!("\nElf file type is {}\n", e.type_text()));
        out.push_str(&format!("Entry point 0x{:x}\n", e.entry));
        if e.ph.len() == 1 {
            out.push_str(&format!(
                "There is 1 program header, starting at offset {}\n",
                e.phoff
            ));
        } else {
            out.push_str(&format!(
                "There are {} program headers, starting at offset {}\n",
                e.ph.len(),
                e.phoff
            ));
        }
    }
    out.push_str("\nProgram Headers:\n");
    if wide {
        out.push_str(
            "  Type           Offset   VirtAddr           PhysAddr           FileSiz  MemSiz   Flg Align\n",
        );
    } else {
        out.push_str("  Type           Offset             VirtAddr           PhysAddr\n");
        out.push_str("                 FileSiz            MemSiz              Flags  Align\n");
    }
    for p in &e.ph {
        let fl: String = [(4u32, 'R'), (2, 'W'), (1, 'E')]
            .iter()
            .map(|&(b, c)| if p.flags & b != 0 { c } else { ' ' })
            .collect();
        let align = if p.align == 0 {
            "0".to_string()
        } else {
            format!("0x{:x}", p.align)
        };
        let ty = seg_type_text(p.kind);
        if wide {
            out.push_str(&format!(
                "  {ty:<14} 0x{:06x} 0x{:016x} 0x{:016x} 0x{:06x} 0x{:06x} {fl} {align}\n",
                p.offset, p.vaddr, p.paddr, p.filesz, p.memsz
            ));
        } else {
            out.push_str(&format!(
                "  {ty:<14} 0x{:016x} 0x{:016x} 0x{:016x}\n",
                p.offset, p.vaddr, p.paddr
            ));
            out.push_str(&format!(
                "                 0x{:016x} 0x{:016x}  {fl}    {align}\n",
                p.filesz, p.memsz
            ));
        }
        if p.kind == PT_INTERP {
            let interp = usize::try_from(p.offset)
                .ok()
                .zip(usize::try_from(p.filesz).ok())
                .and_then(|(o, n)| e.d.get(o..o.checked_add(n)?))
                .map(|b| cstr_lossy(b, 0))
                .unwrap_or_default();
            out.push_str(&format!(
                "      [Requesting program interpreter: {interp}]\n"
            ));
        }
    }
    if e.sh.is_empty() {
        return;
    }
    out.push_str("\n Section to Segment mapping:\n  Segment Sections...\n");
    for (i, p) in e.ph.iter().enumerate() {
        out.push_str(&format!("   {i:02}     "));
        for s in e.sh.iter().skip(1) {
            if in_segment(s, p) {
                out.push_str(&String::from_utf8_lossy(&s.name));
                out.push(' ');
            }
        }
        out.push('\n');
    }
}

fn dyn_tag_name(tag: u64) -> Option<&'static str> {
    Some(match tag {
        0 => "NULL",
        1 => "NEEDED",
        2 => "PLTRELSZ",
        3 => "PLTGOT",
        4 => "HASH",
        5 => "STRTAB",
        6 => "SYMTAB",
        7 => "RELA",
        8 => "RELASZ",
        9 => "RELAENT",
        10 => "STRSZ",
        11 => "SYMENT",
        12 => "INIT",
        13 => "FINI",
        14 => "SONAME",
        15 => "RPATH",
        16 => "SYMBOLIC",
        17 => "REL",
        18 => "RELSZ",
        19 => "RELENT",
        20 => "PLTREL",
        21 => "DEBUG",
        22 => "TEXTREL",
        23 => "JMPREL",
        24 => "BIND_NOW",
        25 => "INIT_ARRAY",
        26 => "FINI_ARRAY",
        27 => "INIT_ARRAYSZ",
        28 => "FINI_ARRAYSZ",
        29 => "RUNPATH",
        30 => "FLAGS",
        32 => "PREINIT_ARRAY",
        33 => "PREINIT_ARRAYSZ",
        34 => "SYMTAB_SHNDX",
        35 => "RELRSZ",
        36 => "RELR",
        37 => "RELRENT",
        0x6fff_fef5 => "GNU_HASH",
        0x6fff_fff0 => "VERSYM",
        0x6fff_fff9 => "RELACOUNT",
        0x6fff_fffa => "RELCOUNT",
        0x6fff_fffb => "FLAGS_1",
        0x6fff_fffc => "VERDEF",
        0x6fff_fffd => "VERDEFNUM",
        0x6fff_fffe => "VERNEED",
        0x6fff_ffff => "VERNEEDNUM",
        0x7fff_fffd => "AUXILIARY",
        0x7fff_ffff => "FILTER",
        _ => return None,
    })
}

const DF_FLAGS: &[(u64, &str)] = &[
    (1, "ORIGIN"),
    (2, "SYMBOLIC"),
    (4, "TEXTREL"),
    (8, "BIND_NOW"),
    (0x10, "STATIC_TLS"),
];

const DF_1_FLAGS: &[(u64, &str)] = &[
    (0x1, "NOW"),
    (0x2, "GLOBAL"),
    (0x4, "GROUP"),
    (0x8, "NODELETE"),
    (0x10, "LOADFLTR"),
    (0x20, "INITFIRST"),
    (0x40, "NOOPEN"),
    (0x80, "ORIGIN"),
    (0x100, "DIRECT"),
    (0x200, "TRANS"),
    (0x400, "INTERPOSE"),
    (0x800, "NODEFLIB"),
    (0x1000, "NODUMP"),
    (0x2000, "CONFALT"),
    (0x4000, "ENDFILTEE"),
    (0x8000, "DISPRELDNE"),
    (0x10000, "DISPRELPND"),
    (0x20000, "NODIRECT"),
    (0x40000, "IGNMULDEF"),
    (0x80000, "NOKSYMS"),
    (0x100000, "NOHDR"),
    (0x200000, "EDITED"),
    (0x400000, "NORELOC"),
    (0x800000, "SYMINTPOSE"),
    (0x1000000, "GLOBAUDIT"),
    (0x2000000, "SINGLETON"),
    (0x4000000, "STUB"),
    (0x8000000, "PIE"),
];

fn flag_names(v: u64, table: &[(u64, &str)]) -> String {
    let mut names: Vec<String> = Vec::new();
    let mut rest = v;
    for (bit, n) in table {
        if v & bit != 0 {
            names.push((*n).to_string());
            rest &= !bit;
        }
    }
    if rest != 0 {
        names.push(format!("unknown: 0x{rest:x}"));
    }
    names.join(" ")
}

fn print_dynamic(e: &Elf<'_>, out: &mut String) {
    let Some(di) = e.sh.iter().position(|s| s.kind == SHT_DYNAMIC) else {
        out.push_str("\nThere is no dynamic section in this file.\n");
        return;
    };
    let sec = &e.sh[di];
    let data = e.section_data(di);
    let strtab = e.section_data(sec.link as usize);
    let n = data.len() / 16;
    if n == 1 {
        out.push_str(&format!(
            "\nDynamic section at offset 0x{:x} contains 1 entry:\n",
            sec.offset
        ));
    } else {
        out.push_str(&format!(
            "\nDynamic section at offset 0x{:x} contains {n} entries:\n",
            sec.offset
        ));
    }
    out.push_str("  Tag        Type                         Name/Value\n");
    for ent in data.chunks_exact(16) {
        let tag = rd64(ent, 0).unwrap_or(0);
        let val = rd64(ent, 8).unwrap_or(0);
        let tname = match dyn_tag_name(tag) {
            Some(n) => format!("({n})"),
            None => format!("(0x{tag:x})"),
        };
        out.push_str(&format!(" 0x{tag:016x} {tname:<20} "));
        let s = |v: u64| cstr_lossy(strtab, v as usize);
        let text = match tag {
            1 => format!("Shared library: [{}]", s(val)),
            14 => format!("Library soname: [{}]", s(val)),
            15 => format!("Library rpath: [{}]", s(val)),
            29 => format!("Library runpath: [{}]", s(val)),
            0x7fff_fffd => format!("Auxiliary library: [{}]", s(val)),
            0x7fff_ffff => format!("Filter library: [{}]", s(val)),
            2 | 8 | 9 | 10 | 11 | 18 | 19 | 27 | 28 | 33 | 35 | 37 => format!("{val} (bytes)"),
            0x6fff_fff9 | 0x6fff_fffa | 0x6fff_fffd | 0x6fff_ffff => val.to_string(),
            20 => (if val == 7 { "RELA" } else { "REL" }).to_string(),
            30 => flag_names(val, DF_FLAGS),
            0x6fff_fffb => format!("Flags: {}", flag_names(val, DF_1_FLAGS)),
            16 | 22 | 24 => String::new(),
            _ => format!("0x{val:x}"),
        };
        out.push_str(&text);
        out.push('\n');
        if tag == 0 {
            break;
        }
    }
}

fn ndx_text(shndx: u16) -> String {
    match shndx {
        0 => "UND".to_string(),
        0xfff1 => "ABS".to_string(),
        0xfff2 => "COM".to_string(),
        0xffff => "XINDEX".to_string(),
        n if (0xff00..0xff20).contains(&n) => format!("PRC[0x{n:04x}]"),
        n if (0xff20..0xff3f).contains(&n) => format!("OS [0x{n:04x}]"),
        n if n >= 0xff00 => format!("RSV[0x{n:04x}]"),
        n => n.to_string(),
    }
}

fn sym_type_text(t: u8) -> String {
    match t {
        0 => "NOTYPE".to_string(),
        1 => "OBJECT".to_string(),
        2 => "FUNC".to_string(),
        3 => "SECTION".to_string(),
        4 => "FILE".to_string(),
        5 => "COMMON".to_string(),
        6 => "TLS".to_string(),
        10 => "IFUNC".to_string(),
        t => format!("<unknown>: {t}"),
    }
}

fn sym_bind_text(b: u8) -> String {
    match b {
        0 => "LOCAL".to_string(),
        1 => "GLOBAL".to_string(),
        2 => "WEAK".to_string(),
        10 => "UNIQUE".to_string(),
        b => format!("<unknown>: {b}"),
    }
}

fn print_symbols(e: &Elf<'_>, want_dyn: bool, want_sym: bool, wide: bool, silent: bool, out: &mut String) {
    let versions = e.versions();
    for (i, s) in e.sh.iter().enumerate() {
        let is_dyn = s.kind == SHT_DYNSYM;
        if !((is_dyn && want_dyn) || (s.kind == SHT_SYMTAB && want_sym)) {
            continue;
        }
        let data = e.section_data(i);
        let strtab = e.section_data(s.link as usize);
        let count = data.len() / 24;
        let sname = String::from_utf8_lossy(&s.name);
        if count == 1 {
            out.push_str(&format!(
                "\nSymbol table '{sname}' contains 1 entry:\n"
            ));
        } else {
            out.push_str(&format!(
                "\nSymbol table '{sname}' contains {count} entries:\n"
            ));
        }
        out.push_str("   Num:    Value          Size Type    Bind   Vis      Ndx Name\n");
        for (n, ent) in data.chunks_exact(24).enumerate() {
            let name_off = rd32(ent, 0).unwrap_or(0);
            let info = ent[4];
            let other = ent[5];
            let shndx = rd16(ent, 6).unwrap_or(0);
            let value = rd64(ent, 8).unwrap_or(0);
            let size = rd64(ent, 16).unwrap_or(0);
            let mut name = cstr_lossy(strtab, name_off as usize);
            if name.is_empty() && info & 0xf == 3 && shndx < 0xff00 {
                if let Some(sec) = e.sh.get(usize::from(shndx)) {
                    name = String::from_utf8_lossy(&sec.name).into_owned();
                }
            }
            let mut ver = String::new();
            if is_dyn && let Some((idx, vn, need, hidden)) = versions.of(n) {
                ver = if need {
                    format!("@{vn} ({idx})")
                } else if hidden {
                    format!("@{vn}")
                } else {
                    format!("@@{vn}")
                };
            }
            // No modo estreito o nome divide 21 colunas com o sufixo de versão.
            let name = if wide {
                name
            } else {
                clip(&name, 21usize.saturating_sub(ver.chars().count()), silent)
            };
            let vis = match other & 3 {
                0 => "DEFAULT",
                1 => "INTERNAL",
                2 => "HIDDEN",
                _ => "PROTECTED",
            };
            out.push_str(&format!(
                "{n:>6}: {value:016x} {size:>5} {:<7} {:<6} {vis:<7} {:>4} {name}{ver}\n",
                sym_type_text(info & 0xf),
                sym_bind_text(info >> 4),
                ndx_text(shndx)
            ));
        }
    }
}

#[derive(Default)]
struct Opts {
    header: bool,
    segments: bool,
    sections: bool,
    syms: bool,
    dyn_syms: bool,
    dynamic: bool,
    notes: bool,
    wide: bool,
    silent: bool,
}

/// Nome e descrição de um tipo de nota do dono `GNU` (`get_gnu_elf_note_type`).
fn gnu_note_type(t: u32) -> String {
    match t {
        1 => "NT_GNU_ABI_TAG (ABI version tag)".into(),
        2 => "NT_GNU_HWCAP (DSO-supplied software HWCAP info)".into(),
        3 => "NT_GNU_BUILD_ID (unique build ID bitstring)".into(),
        4 => "NT_GNU_GOLD_VERSION (gold version)".into(),
        5 => "NT_GNU_PROPERTY_TYPE_0".into(),
        0x100 => "OPEN".into(),
        0x101 => "func".into(),
        _ => format!("Unknown note type: (0x{t:08x})"),
    }
}

/// Bits do `GNU_PROPERTY_X86_ISA_1_*` (`decode_x86_isa`).
fn x86_isa(mut bits: u32) -> String {
    if bits == 0 {
        return "<None>".into();
    }
    let names = [(1, "x86-64-baseline"), (2, "x86-64-v2"), (4, "x86-64-v3"), (8, "x86-64-v4")];
    let mut v = Vec::new();
    for (b, n) in names {
        if bits & b != 0 {
            v.push(n.to_string());
            bits &= !b;
        }
    }
    if bits != 0 {
        v.push(format!("<unknown: {bits:x}>"));
    }
    v.join(", ")
}

/// Bits do `GNU_PROPERTY_X86_FEATURE_1_AND` (`decode_x86_feature_1`).
fn x86_feature_1(mut bits: u32) -> String {
    if bits == 0 {
        return "<None>".into();
    }
    let names = [(1, "IBT"), (2, "SHSTK"), (4, "LAM_U48"), (8, "LAM_U57")];
    let mut v = Vec::new();
    for (b, n) in names {
        if bits & b != 0 {
            v.push(n.to_string());
            bits &= !b;
        }
    }
    if bits != 0 {
        v.push(format!("<unknown: {bits:x}>"));
    }
    v.join(", ")
}

/// `print_gnu_property_note` para x86-64: as propriedades de um `NT_GNU_PROPERTY_TYPE_0`.
fn gnu_properties(desc: &[u8]) -> String {
    let mut parts = Vec::new();
    let mut o = 0;
    while o + 8 <= desc.len() {
        let ptype = u32::from_le_bytes([desc[o], desc[o + 1], desc[o + 2], desc[o + 3]]);
        let datasz = u32::from_le_bytes([desc[o + 4], desc[o + 5], desc[o + 6], desc[o + 7]]) as usize;
        o += 8;
        if o + datasz > desc.len() {
            parts.push(format!("<corrupt type (0x{ptype:x}) datasz: 0x{datasz:x}>"));
            break;
        }
        let d = &desc[o..o + datasz];
        let word = (datasz == 4).then(|| u32::from_le_bytes([d[0], d[1], d[2], d[3]]));
        let text = match (ptype, word) {
            (0xc000_8002, Some(w)) => format!("x86 ISA needed: {}", x86_isa(w)),
            (0xc001_0002, Some(w)) => format!("x86 ISA used: {}", x86_isa(w)),
            (0xc000_0002, Some(w)) => format!("x86 feature: {}", x86_feature_1(w)),
            (0xc000_0001, Some(_)) => "no copy on protected".into(),
            (1, _) => "stack size: ...".into(),
            (2, _) => "no copy on protected".into(),
            _ => format!("<unknown type (0x{ptype:x}) datasz: 0x{datasz:x}>"),
        };
        parts.push(text);
        // Cada propriedade é alinhada a 8 bytes num ELF de 64 bits.
        o += (datasz + 7) & !7;
    }
    format!("      Properties: {}\n", parts.join(", "))
}

/// `process_notes`: as notas de cada seção `SHT_NOTE`, na ordem da tabela de seções.
fn print_notes(e: &Elf<'_>, wide: bool, out: &mut String) {
    for s in e.sh.iter().filter(|s| s.kind == SHT_NOTE) {
        let start = s.offset as usize;
        let Some(data) = e.d.get(start..start + s.size as usize) else { continue };
        out.push_str(&format!("\nDisplaying notes found in: {}\n", String::from_utf8_lossy(&s.name)));
        out.push_str("  Owner                Data size \tDescription\n");
        let align = if s.align >= 8 { 8 } else { 4 };
        let mut o = 0;
        while o + 12 <= data.len() {
            let namesz = rd32(data, o).unwrap_or(0) as usize;
            let descsz = rd32(data, o + 4).unwrap_or(0) as usize;
            let ntype = rd32(data, o + 8).unwrap_or(0);
            let noff = o + 12;
            let doff = (noff + namesz + align - 1) & !(align - 1);
            let next = (doff + descsz + align - 1) & !(align - 1);
            if doff + descsz > data.len() {
                break;
            }
            let name = cstr(&data[noff..noff + namesz]).to_vec();
            let desc = &data[doff..doff + descsz];
            let owner = String::from_utf8_lossy(&name).into_owned();
            let gnu = name == b"GNU";
            let tdesc = if gnu { gnu_note_type(ntype) } else { format!("Unknown note type: (0x{ntype:08x})") };
            // Com `-W` o detalhe da nota segue na mesma linha, depois de um tab.
            let detailed = gnu && matches!(ntype, 3 | 4 | 5) || gnu && ntype == 1 && descsz >= 16;
            let sep = if wide && detailed { '\t' } else { '\n' };
            out.push_str(&format!("  {owner:<20} 0x{descsz:08x}\t{tdesc}{sep}"));
            if gnu {
                match ntype {
                    3 => {
                        let hex: String = desc.iter().map(|b| format!("{b:02x}")).collect();
                        out.push_str(&format!("    Build ID: {hex}\n"));
                    }
                    1 if descsz >= 16 => {
                        let w = |i: usize| rd32(desc, i * 4).unwrap_or(0);
                        let os = match w(0) {
                            0 => "Linux",
                            1 => "Hurd",
                            2 => "Solaris",
                            3 => "FreeBSD",
                            4 => "NetBSD",
                            5 => "Syllable",
                            _ => "Unknown",
                        };
                        out.push_str(&format!("    OS: {os}, ABI: {}.{}.{}\n", w(1), w(2), w(3)));
                    }
                    4 => {
                        out.push_str(&format!("    Version: {}\n", String::from_utf8_lossy(cstr(desc))));
                    }
                    5 => out.push_str(&gnu_properties(desc)),
                    _ => {}
                }
            }
            if next <= o {
                break;
            }
            o = next;
        }
    }
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(prog: &str, to_stdout: bool) -> i32 {
    let mut text = format!("Usage: {prog} <option(s)> elf-file(s)\n");
    text.push_str(&USAGE_LINES.join("\n"));
    text.push('\n');
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
    let mut o = Opts::default();
    let mut acted = false;
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(x) => x,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&prog)));
                return usage(&prog, false);
            }
        };
        match opt.id {
            ID_SEGMENTS => {
                o.segments = true;
                acted = true;
            }
            ID_SECTIONS => {
                o.sections = true;
                acted = true;
            }
            ID_SYMBOLS => {
                o.syms = true;
                o.dyn_syms = true;
                acted = true;
            }
            ID_DYN_SYMS => {
                o.dyn_syms = true;
                acted = true;
            }
            id => match u8::try_from(id).unwrap_or(0) {
                b'a' => {
                    o.header = true;
                    o.segments = true;
                    o.sections = true;
                    o.syms = true;
                    o.dyn_syms = true;
                    o.dynamic = true;
                    o.notes = true;
                    acted = true;
                }
                b'h' => {
                    o.header = true;
                    acted = true;
                }
                b'l' => {
                    o.segments = true;
                    acted = true;
                }
                b'S' => {
                    o.sections = true;
                    acted = true;
                }
                b'e' => {
                    o.header = true;
                    o.segments = true;
                    o.sections = true;
                    acted = true;
                }
                b's' => {
                    o.syms = true;
                    o.dyn_syms = true;
                    acted = true;
                }
                b'd' => {
                    o.dynamic = true;
                    acted = true;
                }
                b'W' => o.wide = true,
                b'T' => o.silent = true,
                b'n' => {
                    o.notes = true;
                    acted = true;
                }
                b'g' | b't' | b'r' | b'u' | b'V' | b'A' | b'c' | b'I' | b'x' | b'p'
                | b'R' | b'j' => acted = true,
                b'H' => return usage(&prog, true),
                b'v' => {
                    super::ar::print_version("readelf");
                    return 0;
                }
                _ => {}
            },
        }
    }
    let files = g.operands();
    if !acted {
        return usage(&prog, false);
    }
    if files.is_empty() {
        return usage(&prog, false);
    }
    let show_name = files.len() > 1;
    let mut status = 0;
    for f in &files {
        if process(&prog, f, &o, show_name).is_err() {
            status = 1;
        }
    }
    status
}

fn process(prog: &str, path: &[u8], o: &Opts, show_name: bool) -> Result<(), ()> {
    let say = |text: String| io::eprint(format!("{prog}: Error: {text}\n"));
    let shown = io::lossy(path);
    match sys::stat(path) {
        Err(Errno::ENOENT) => {
            say(format!("'{shown}': No such file"));
            return Err(());
        }
        Err(_) => {
            say(format!("Input file '{shown}' is not readable."));
            return Err(());
        }
        Ok(st) if st.file_type() == FileType::Directory => {
            say(format!("'{shown}' is not an ordinary file"));
            return Err(());
        }
        Ok(_) => {}
    }
    let data = match io::read_path(path) {
        Ok(d) => d,
        Err(_) => {
            say(format!("Input file '{shown}' is not readable."));
            return Err(());
        }
    };
    if data.len() < 16 {
        say(format!("{shown}: Failed to read file's magic number"));
        return Err(());
    }
    let elf = match Elf::parse(&data) {
        Ok(e) => e,
        Err(m) => {
            say(m);
            return Err(());
        }
    };
    let mut out = String::new();
    if show_name {
        out.push_str(&format!("\nFile: {shown}\n"));
    }
    if o.header {
        print_header(&elf, &mut out);
    }
    if o.sections {
        print_sections(&elf, o.wide, o.silent, !o.header, &mut out);
    }
    if o.segments {
        print_segments(&elf, o.wide, !o.header, &mut out);
    }
    if o.dynamic {
        print_dynamic(&elf, &mut out);
    }
    if o.syms || o.dyn_syms {
        print_symbols(&elf, o.dyn_syms, o.syms, o.wide, o.silent, &mut out);
    }
    if o.notes {
        print_notes(&elf, o.wide, &mut out);
    }
    let mut so = io::stdout();
    let _ = so.write_all(out.as_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_marks_truncation() {
        assert_eq!(clip(".note.gnu.property", 17, false), ".note.gnu.pr[...]");
        assert_eq!(clip(".text", 17, false), ".text");
        assert_eq!(clip(".note.gnu.property", 17, true), ".note.gnu.propert");
    }

    #[test]
    fn flags() {
        assert_eq!(flags_text(0x6), "AX");
        assert_eq!(flags_text(0x3), "WA");
        assert_eq!(flags_text(0x30), "MS");
    }
}
