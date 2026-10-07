//! `objdump` do GNU binutils 2.44 (Debian 13) sobre ELF64 little-endian, sem disassembler.
//!
//! Escrito a partir do comportamento observado e da man page (o código do binutils é GPL e não
//! foi consultado). Implementa `-a`, `-f`, `-p` (Program Header, Dynamic Section, Version
//! definitions e Version References), `-h`, `-x`, `-t`, `-T`, `-r`, `-R`, `-s`, `-j`, `-g`,
//! `-w`, `-i`, `-v`/`-V`, `-H`.
//!
//! Divergências conhecidas:
//! - `-d`, `-D` e `-S` emitem só os cabeçalhos `Disassembly of section` e os rótulos de símbolo;
//!   as instruções NÃO são inventadas (ver `TODO(disasm)` em [`disassemble`]).
//! - `-W`/`--dwarf`, `-G`, `-e` e `-g` são aceitos e não produzem saída além do cabeçalho.
//! - `-i` lista só os nomes de alvo (sem a matriz de arquitetura) e a ajuda não traz a lista de
//!   arquiteturas nem as opções do disassembler x86.
//! - Só ELF64 little-endian; arquivos `.a` não são abertos.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, FileType, sys};
use ul_common::ctype::cstr_at;

use crate::strings::expand_response_files;
use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

use super::elf::{SHT_VERDEF, SHT_VERNEED, Versions, cstr_lossy, is_elf64_le, rd16, rd32, rd64, segment_type_name, slice_at, walk_chain};

const SHORTOPTS: &str = "pP:ib:m:M:VvCdDlfFaHhrRtTxsSj:wzZgeGWLI:E:";

const ID_IGNORE_FLAG: i32 = 300;
const ID_IGNORE_ARG: i32 = 301;

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("archive-headers", HasArg::No, 'a' as i32),
    LongOpt::new("file-headers", HasArg::No, 'f' as i32),
    LongOpt::new("private-headers", HasArg::No, 'p' as i32),
    LongOpt::new("private", HasArg::Required, 'P' as i32),
    LongOpt::new("section-headers", HasArg::No, 'h' as i32),
    LongOpt::new("headers", HasArg::No, 'h' as i32),
    LongOpt::new("all-headers", HasArg::No, 'x' as i32),
    LongOpt::new("disassemble", HasArg::No, 'd' as i32),
    LongOpt::new("disassemble-all", HasArg::No, 'D' as i32),
    LongOpt::new("source", HasArg::No, 'S' as i32),
    LongOpt::new("full-contents", HasArg::No, 's' as i32),
    LongOpt::new("decompress", HasArg::No, 'Z' as i32),
    LongOpt::new("debugging", HasArg::No, 'g' as i32),
    LongOpt::new("debugging-tags", HasArg::No, 'e' as i32),
    LongOpt::new("stabs", HasArg::No, 'G' as i32),
    LongOpt::new("dwarf", HasArg::No, 'W' as i32),
    LongOpt::new("process-links", HasArg::No, 'L' as i32),
    LongOpt::new("syms", HasArg::No, 't' as i32),
    LongOpt::new("dynamic-syms", HasArg::No, 'T' as i32),
    LongOpt::new("reloc", HasArg::No, 'r' as i32),
    LongOpt::new("dynamic-reloc", HasArg::No, 'R' as i32),
    LongOpt::new("version", HasArg::No, 'v' as i32),
    LongOpt::new("info", HasArg::No, 'i' as i32),
    LongOpt::new("help", HasArg::No, 'H' as i32),
    LongOpt::new("wide", HasArg::No, 'w' as i32),
    LongOpt::new("section", HasArg::Required, 'j' as i32),
    LongOpt::new("target", HasArg::Required, 'b' as i32),
    LongOpt::new("architecture", HasArg::Required, 'm' as i32),
    LongOpt::new("disassembler-options", HasArg::Required, 'M' as i32),
    LongOpt::new("demangle", HasArg::No, 'C' as i32),
    LongOpt::new("line-numbers", HasArg::No, 'l' as i32),
    LongOpt::new("file-offsets", HasArg::No, 'F' as i32),
    LongOpt::new("disassemble-zeroes", HasArg::No, 'z' as i32),
    LongOpt::new("start-address", HasArg::Required, ID_IGNORE_ARG),
    LongOpt::new("stop-address", HasArg::Required, ID_IGNORE_ARG),
    LongOpt::new("adjust-vma", HasArg::Required, ID_IGNORE_ARG),
    LongOpt::new("prefix", HasArg::Required, ID_IGNORE_ARG),
    LongOpt::new("prefix-strip", HasArg::Required, ID_IGNORE_ARG),
    LongOpt::new("no-show-raw-insn", HasArg::No, ID_IGNORE_FLAG),
    LongOpt::new("show-raw-insn", HasArg::No, ID_IGNORE_FLAG),
    LongOpt::new("prefix-addresses", HasArg::No, ID_IGNORE_FLAG),
    LongOpt::new("special-syms", HasArg::No, ID_IGNORE_FLAG),
    LongOpt::new("no-addresses", HasArg::No, ID_IGNORE_FLAG),
    LongOpt::new("visualize-jumps", HasArg::No, ID_IGNORE_FLAG),
];

/// Corpo do usage, uma linha por item.
const USAGE_LINES: &[&str] = &[
    " Display information from object <file(s)>.",
    " At least one of the following switches must be given:",
    "  -a, --archive-headers    Display archive header information",
    "  -f, --file-headers       Display the contents of the overall file header",
    "  -p, --private-headers    Display object format specific file header contents",
    "  -P, --private=OPT,OPT... Display object format specific contents",
    "  -h, --[section-]headers  Display the contents of the section headers",
    "  -x, --all-headers        Display the contents of all headers",
    "  -d, --disassemble        Display assembler contents of executable sections",
    "  -D, --disassemble-all    Display assembler contents of all sections",
    "      --disassemble=<sym>  Display assembler contents from <sym>",
    "  -S, --source             Intermix source code with disassembly",
    "      --source-comment[=<txt>] Prefix lines of source code with <txt>",
    "  -s, --full-contents      Display the full contents of all sections requested",
    "  -Z, --decompress         Decompress section(s) before displaying their contents",
    "  -g, --debugging          Display debug information in object file",
    "  -e, --debugging-tags     Display debug information using ctags style",
    "  -G, --stabs              Display (in raw form) any STABS info in the file",
    "  -W, --dwarf[a/=abbrev, A/=addr, r/=aranges, c/=cu_index, L/=decodedline,",
    "              f/=frames, F/=frames-interp, g/=gdb_index, i/=info, o/=loc,",
    "              m/=macro, p/=pubnames, t/=pubtypes, R/=Ranges, l/=rawline,",
    "              s/=str, O/=str-offsets, u/=trace_abbrev, T/=trace_aranges,",
    "              U/=trace_info]",
    "                           Display DWARF info in the file",
    "  -L, --process-links      Display the contents of non-debug sections in",
    "                            separate debuginfo files.  (Implies -WK)",
    "  -t, --syms               Display the contents of the symbol table(s)",
    "  -T, --dynamic-syms       Display the contents of the dynamic symbol table",
    "  -r, --reloc              Display the relocation entries in the file",
    "  -R, --dynamic-reloc      Display the dynamic relocation entries in the file",
    "  @<file>                  Read options from <file>",
    "  -v, --version            Display this program's version number",
    "  -i, --info               List object formats and architectures supported",
    "  -H, --help               Display this information",
];

const SHF_WRITE: u64 = 1;
const SHF_ALLOC: u64 = 2;
const SHF_EXECINSTR: u64 = 4;
const SHF_TLS: u64 = 0x400;

const SHT_SYMTAB: u32 = 2;
const SHT_RELA: u32 = 4;
const SHT_DYNAMIC: u32 = 6;
const SHT_NOBITS: u32 = 8;
const SHT_REL: u32 = 9;
const SHT_DYNSYM: u32 = 11;
const SHT_GROUP: u32 = 17;
const SHT_SYMTAB_SHNDX: u32 = 18;

const STT_SECTION: u8 = 3;
const STT_FILE: u8 = 4;

const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;

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

struct Sym {
    name: Vec<u8>,
    info: u8,
    other: u8,
    shndx: u16,
    value: u64,
    size: u64,
}

struct Rel {
    offset: u64,
    sym: usize,
    kind: u32,
    addend: Option<i64>,
}

struct Elf<'a> {
    d: &'a [u8],
    etype: u16,
    machine: u16,
    entry: u64,
    shstrndx: usize,
    sh: Vec<Shdr>,
    ph: Vec<Phdr>,
}

impl<'a> Elf<'a> {
    /// Interpreta cabeçalho, tabela de seções e program headers. `None` se não for ELF64 LE íntegro.
    fn parse(d: &'a [u8]) -> Option<Elf<'a>> {
        if !is_elf64_le(d) {
            return None;
        }
        let etype = rd16(d, 16)?;
        let machine = rd16(d, 18)?;
        let entry = rd64(d, 24)?;
        let phoff = usize::try_from(rd64(d, 32)?).ok()?;
        let shoff = usize::try_from(rd64(d, 40)?).ok()?;
        let phentsize = rd16(d, 54)?;
        let phnum_raw = rd16(d, 56)?;
        let shentsize = rd16(d, 58)?;
        let shnum_raw = rd16(d, 60)?;
        let shstrndx_raw = rd16(d, 62)?;

        let mut sh: Vec<Shdr> = Vec::new();
        let mut shstrndx = 0usize;
        if shoff != 0 {
            if shentsize != 64 {
                return None;
            }
            let mut shnum = usize::from(shnum_raw);
            if shnum == 0 {
                shnum = usize::try_from(rd64(d, shoff.checked_add(32)?)?).ok()?;
            }
            shstrndx = usize::from(shstrndx_raw);
            if shstrndx_raw == 0xffff {
                shstrndx = rd32(d, shoff.checked_add(40)?)? as usize;
            }
            let end = shoff.checked_add(shnum.checked_mul(64)?)?;
            if end > d.len() {
                return None;
            }
            let mut name_offs: Vec<usize> = Vec::with_capacity(shnum);
            for i in 0..shnum {
                let b = shoff + i * 64;
                name_offs.push(rd32(d, b)? as usize);
                sh.push(Shdr {
                    name: Vec::new(),
                    kind: rd32(d, b + 4)?,
                    flags: rd64(d, b + 8)?,
                    addr: rd64(d, b + 16)?,
                    offset: rd64(d, b + 24)?,
                    size: rd64(d, b + 32)?,
                    link: rd32(d, b + 40)?,
                    info: rd32(d, b + 44)?,
                    align: rd64(d, b + 48)?,
                });
            }
            let strtab: Vec<u8> = sh
                .get(shstrndx)
                .map(|s| slice_at(d, s.offset, s.size).to_vec())
                .unwrap_or_default();
            for (s, off) in sh.iter_mut().zip(name_offs) {
                s.name = cstr_at(&strtab, off).to_vec();
            }
        }

        let mut ph: Vec<Phdr> = Vec::new();
        let mut phnum = usize::from(phnum_raw);
        if phnum == 0xffff {
            phnum = sh.first().map_or(0, |s| s.info as usize);
        }
        if phoff != 0 && phentsize == 56 {
            for i in 0..phnum {
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
                ph.push(Phdr {
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
        Some(Elf {
            d,
            etype,
            machine,
            entry,
            shstrndx,
            sh,
            ph,
        })
    }

    fn data(&self, i: usize) -> &'a [u8] {
        match self.sh.get(i) {
            Some(s) if s.kind != SHT_NOBITS => slice_at(self.d, s.offset, s.size),
            _ => &[],
        }
    }

    /// As versões dos símbolos dinâmicos, lidas das seções `.gnu.version*`.
    fn versions(&self) -> Versions {
        Versions::load(self.sh.iter().enumerate().map(|(i, s)| (s.kind, s.info, self.data(i), self.data(s.link as usize))))
    }

    fn symtab_index(&self) -> Option<usize> {
        self.sh.iter().position(|s| s.kind == SHT_SYMTAB)
    }

    fn dynsym_index(&self) -> Option<usize> {
        self.sh.iter().position(|s| s.kind == SHT_DYNSYM)
    }

    /// Seção de relocação absorvida pelo bfd: pertence a uma seção e usa a `.symtab`.
    fn absorbed_reloc(&self, s: &Shdr) -> bool {
        matches!(s.kind, SHT_REL | SHT_RELA)
            && self.symtab_index().is_some_and(|st| s.link as usize == st)
            && s.info != 0
            && (s.info as usize) < self.sh.len()
    }

    /// Índices ELF das seções que o objdump enumera (as seções do bfd), na ordem do arquivo.
    fn listed(&self) -> Vec<usize> {
        let symtab = self.symtab_index();
        let symtab_str = symtab.map(|i| self.sh[i].link as usize);
        let mut v = Vec::new();
        for (i, s) in self.sh.iter().enumerate() {
            if i == 0
                || s.kind == SHT_SYMTAB
                || s.kind == SHT_SYMTAB_SHNDX
                || Some(i) == symtab_str
                || i == self.shstrndx
                || self.absorbed_reloc(s)
            {
                continue;
            }
            v.push(i);
        }
        v
    }

    fn has_relocs(&self, target: usize) -> bool {
        self.sh
            .iter()
            .any(|r| self.absorbed_reloc(r) && r.info as usize == target)
    }

    fn is_debug_name(name: &[u8]) -> bool {
        name.starts_with(b".debug")
            || name.starts_with(b".zdebug")
            || name.starts_with(b".gnu.linkonce.wi.")
            || name.starts_with(b".line")
            || name.starts_with(b".stab")
    }

    fn is_code(s: &Shdr) -> bool {
        s.flags & SHF_EXECINSTR != 0
    }

    /// Nomes dos flags de seção, na ordem em que o objdump os imprime.
    fn section_flag_names(&self, i: usize) -> Vec<&'static str> {
        let s = &self.sh[i];
        let mut f: Vec<&'static str> = Vec::new();
        let nobits = s.kind == SHT_NOBITS;
        let alloc = s.flags & SHF_ALLOC != 0;
        if !nobits {
            f.push("CONTENTS");
        }
        if alloc {
            f.push("ALLOC");
        }
        if alloc && !nobits {
            f.push("LOAD");
        }
        if self.has_relocs(i) {
            f.push("RELOC");
        }
        if s.flags & SHF_WRITE == 0 {
            f.push("READONLY");
        }
        if Self::is_code(s) {
            f.push("CODE");
        } else if alloc && !nobits {
            f.push("DATA");
        } else if alloc && s.flags & SHF_TLS != 0 {
            // .tbss: só ALLOC e THREAD_LOCAL.
        }
        if s.kind == SHT_GROUP {
            f.push("EXCLUDE");
        }
        if Self::is_debug_name(&s.name) {
            f.push("DEBUGGING");
        }
        if s.flags & SHF_TLS != 0 {
            f.push("THREAD_LOCAL");
        }
        f
    }

    fn read_syms(&self, idx: usize) -> Vec<Sym> {
        let Some(s) = self.sh.get(idx) else {
            return Vec::new();
        };
        let strtab = self.data(s.link as usize);
        self.data(idx)
            .chunks_exact(24)
            .map(|ent| Sym {
                name: cstr_at(strtab, rd32(ent, 0).unwrap_or(0) as usize).to_vec(),
                info: ent[4],
                other: ent[5],
                shndx: rd16(ent, 6).unwrap_or(0),
                value: rd64(ent, 8).unwrap_or(0),
                size: rd64(ent, 16).unwrap_or(0),
            })
            .collect()
    }

    fn read_rels(&self, idx: usize) -> Vec<Rel> {
        let data = self.data(idx);
        let mut v = Vec::new();
        match self.sh[idx].kind {
            SHT_RELA => {
                for ent in data.chunks_exact(24) {
                    let info = rd64(ent, 8).unwrap_or(0);
                    v.push(Rel {
                        offset: rd64(ent, 0).unwrap_or(0),
                        sym: (info >> 32) as usize,
                        kind: (info & 0xffff_ffff) as u32,
                        addend: Some(rd64(ent, 16).unwrap_or(0) as i64),
                    });
                }
            }
            _ => {
                for ent in data.chunks_exact(16) {
                    let info = rd64(ent, 8).unwrap_or(0);
                    v.push(Rel {
                        offset: rd64(ent, 0).unwrap_or(0),
                        sym: (info >> 32) as usize,
                        kind: (info & 0xffff_ffff) as u32,
                        addend: None,
                    });
                }
            }
        }
        v
    }

    fn format_name(&self) -> &'static str {
        match self.machine {
            62 => "elf64-x86-64",
            183 => "elf64-littleaarch64",
            243 => "elf64-littleriscv",
            _ => "elf64-little",
        }
    }

    fn arch_name(&self) -> &'static str {
        match self.machine {
            62 => "i386:x86-64",
            183 => "aarch64",
            243 => "riscv:rv64",
            _ => "UNKNOWN!",
        }
    }

    /// Flags do bfd para o arquivo (`HAS_RELOC`, `EXEC_P`, ...).
    fn file_flags(&self) -> u32 {
        let mut f = 0u32;
        if self.sh.iter().any(|s| self.absorbed_reloc(s)) {
            f |= 0x1;
        }
        if self.etype == ET_EXEC {
            f |= 0x2;
        }
        let has_syms = [self.symtab_index(), self.dynsym_index()]
            .iter()
            .flatten()
            .any(|&i| self.sh[i].size > 0);
        if has_syms {
            f |= 0x10;
        }
        let dynamic = self.etype == ET_DYN
            || (self.etype == ET_EXEC && self.sh.iter().any(|s| s.kind == SHT_DYNAMIC));
        if dynamic {
            f |= 0x40;
        }
        if (f & 0x2 != 0 || dynamic) && !self.ph.is_empty() {
            f |= 0x100;
        }
        f
    }
}

const FILE_FLAG_NAMES: &[(u32, &str)] = &[
    (0x1, "HAS_RELOC"),
    (0x2, "EXEC_P"),
    (0x4, "HAS_LINENO"),
    (0x8, "HAS_DEBUG"),
    (0x10, "HAS_SYMS"),
    (0x20, "HAS_LOCALS"),
    (0x40, "DYNAMIC"),
    (0x80, "WP_TEXT"),
    (0x100, "D_PAGED"),
];

#[derive(Default)]
struct Opts {
    archive: bool,
    file_hdr: bool,
    private: bool,
    sec_hdr: bool,
    syms: bool,
    dyn_syms: bool,
    reloc: bool,
    dyn_reloc: bool,
    contents: bool,
    debugging: bool,
    disasm: bool,
    disasm_all: bool,
    wide: bool,
    only: Vec<Vec<u8>>,
}

impl Opts {
    fn selected(&self, name: &[u8]) -> bool {
        self.only.is_empty() || self.only.iter().any(|n| n.as_slice() == name)
    }
}

/// Garante uma linha em branco antes do próximo bloco.
fn ensure_blank(out: &mut String) {
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(prog: &str, to_stdout: bool, status: i32) -> i32 {
    let mut text = format!("Usage: {prog} <option(s)> <file(s)>\n");
    text.push_str(&USAGE_LINES.join("\n"));
    text.push('\n');
    text.push_str(&format!(
        "{prog}: supported targets: {}\n",
        crate::strings::TARGETS.join(" ")
    ));
    if to_stdout {
        text.push_str("Report bugs to <https://sourceware.org/bugzilla/>\n");
        let mut out = io::stdout();
        let _ = out.write_all(text.as_bytes());
        0
    } else {
        io::eprint(text);
        status
    }
}

/// `-i`: só a lista de alvos (sem a matriz de arquiteturas).
fn print_info() {
    let mut text = String::from("BFD header file version (GNU Binutils for Debian) 2.44\n");
    for t in crate::strings::TARGETS {
        let endian = if t.contains("big") { "big" } else { "little" };
        text.push_str(&format!("{t}\n (header {endian} endian, data {endian} endian)\n"));
    }
    let mut out = io::stdout();
    let _ = out.write_all(text.as_bytes());
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
    let mut seen = false;
    let mut info = false;
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(x) => x,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&prog)));
                return usage(&prog, false, 1);
            }
        };
        match opt.id {
            ID_IGNORE_FLAG | ID_IGNORE_ARG => {}
            id => match u8::try_from(id).unwrap_or(0) {
                b'a' => {
                    o.archive = true;
                    seen = true;
                }
                b'f' => {
                    o.file_hdr = true;
                    seen = true;
                }
                b'p' => {
                    o.private = true;
                    seen = true;
                }
                b'h' => {
                    o.sec_hdr = true;
                    seen = true;
                }
                b'x' => {
                    o.archive = true;
                    o.file_hdr = true;
                    o.private = true;
                    o.sec_hdr = true;
                    o.syms = true;
                    o.reloc = true;
                    seen = true;
                }
                b't' => {
                    o.syms = true;
                    seen = true;
                }
                b'T' => {
                    o.dyn_syms = true;
                    seen = true;
                }
                b'r' => {
                    o.reloc = true;
                    seen = true;
                }
                b'R' => {
                    o.dyn_reloc = true;
                    seen = true;
                }
                b's' => {
                    o.contents = true;
                    seen = true;
                }
                b'd' | b'S' => {
                    o.disasm = true;
                    seen = true;
                }
                b'D' => {
                    o.disasm = true;
                    o.disasm_all = true;
                    seen = true;
                }
                b'g' | b'e' | b'G' | b'W' | b'P' | b'L' => {
                    o.debugging = true;
                    seen = true;
                }
                b'i' => {
                    info = true;
                    seen = true;
                }
                b'w' => o.wide = true,
                b'j' => o.only.push(opt.arg.clone().unwrap_or_default()),
                b'v' | b'V' => {
                    super::ar::print_version("objdump");
                    return 0;
                }
                b'H' => return usage(&prog, true, 0),
                _ => {}
            },
        }
    }
    if info {
        print_info();
        return 0;
    }
    if !seen {
        return usage(&prog, false, 2);
    }
    let mut files = g.operands();
    if files.is_empty() {
        files.push(b"a.out".to_vec());
    }
    let mut found = vec![false; o.only.len()];
    let mut status = 0;
    for f in &files {
        if process(&prog, f, &o, &mut found).is_err() {
            status = 1;
        }
    }
    for (name, ok) in o.only.iter().zip(&found) {
        if !ok {
            io::eprint(format!(
                "{prog}: section '{}' mentioned in a -j option, but not found in any input file\n",
                String::from_utf8_lossy(name)
            ));
            status = 1;
        }
    }
    status
}

fn process(prog: &str, path: &[u8], o: &Opts, found: &mut [bool]) -> Result<(), ()> {
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
    let shown = io::lossy(path);
    for (n, flag) in o.only.iter().zip(found.iter_mut()) {
        if elf.listed().iter().any(|&i| elf.sh[i].name == *n) {
            *flag = true;
        }
    }

    let mut out = format!("\n{shown}:     file format {}\n", elf.format_name());
    if o.archive {
        out.push_str(&format!("{shown}\n"));
    }
    if o.file_hdr {
        print_file_header(&elf, &mut out);
    }
    if o.private {
        print_private(&elf, &mut out);
    }
    if o.sec_hdr {
        print_section_headers(&elf, o, &mut out);
    }
    if o.syms {
        print_symbols(&elf, false, &mut out);
    }
    if o.dyn_syms {
        if elf.dynsym_index().is_none() {
            say(&[path, b": not a dynamic object"]);
        }
        print_symbols(&elf, true, &mut out);
    }
    if o.reloc && !o.disasm {
        print_relocs(&elf, o, &mut out);
    }
    if o.dyn_reloc && !o.disasm {
        if elf.dynsym_index().is_none() {
            say(&[path, b": not a dynamic object"]);
        }
        print_dynamic_relocs(&elf, &mut out);
    }
    if o.contents {
        dump_contents(&elf, o, &mut out);
    }
    if o.disasm {
        disassemble(&elf, o, &mut out);
    }
    let _ = o.debugging;
    let mut so = io::stdout();
    let _ = so.write_all(out.as_bytes());
    Ok(())
}

fn print_file_header(e: &Elf<'_>, out: &mut String) {
    let flags = e.file_flags();
    let names: Vec<&str> = FILE_FLAG_NAMES
        .iter()
        .filter(|(b, _)| flags & b != 0)
        .map(|(_, n)| *n)
        .collect();
    out.push_str(&format!(
        "architecture: {}, flags 0x{flags:08x}:\n{}\nstart address 0x{:016x}\n\n",
        e.arch_name(),
        names.join(", "),
        e.entry
    ));
}

fn print_section_headers(e: &Elf<'_>, o: &Opts, out: &mut String) {
    ensure_blank(out);
    let list = e.listed();
    let width = if o.wide {
        list.iter()
            .map(|&i| e.sh[i].name.len())
            .max()
            .unwrap_or(0)
            .max(13)
    } else {
        13
    };
    out.push_str("Sections:\n");
    out.push_str(&format!(
        "Idx {:<width$} Size      VMA               LMA               File off  Algn\n",
        "Name"
    ));
    for (n, &i) in list.iter().enumerate() {
        let s = &e.sh[i];
        if !o.selected(&s.name) {
            continue;
        }
        let name = String::from_utf8_lossy(&s.name);
        out.push_str(&format!(
            "{n:>3} {name:<width$} {:08x}  {:016x}  {:016x}  {:08x}  2**{}\n",
            s.size,
            s.addr,
            s.addr,
            s.offset,
            s.align.checked_ilog2().unwrap_or(0)
        ));
        out.push_str(&" ".repeat(width + 5));
        out.push_str(&e.section_flag_names(i).join(", "));
        out.push('\n');
    }
}

fn dyn_tag_name(tag: u64) -> Option<&'static str> {
    Some(match tag {
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
        0x6fff_fdf5 => "GNU_PRELINKED",
        0x6fff_fef5 => "GNU_HASH",
        0x6fff_fdf6 => "GNU_CONFLICT",
        0x6fff_fdf7 => "GNU_LIBLIST",
        0x6fff_fdf8 => "CHECKSUM",
        0x6fff_fdf9 => "PLTPADSZ",
        0x6fff_fdfa => "MOVEENT",
        0x6fff_fdfb => "MOVESZ",
        0x6fff_fdfc => "FEATURE",
        0x6fff_fdfd => "POSFLAG_1",
        0x6fff_fdfe => "SYMINSZ",
        0x6fff_fdff => "SYMINENT",
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

/// `-p`: Program Header, Dynamic Section, Version definitions e Version References.
fn print_private(e: &Elf<'_>, out: &mut String) {
    if !e.ph.is_empty() {
        ensure_blank(out);
        out.push_str("Program Header:\n");
        for p in &e.ph {
            let fl: String = [(4u32, 'r'), (2, 'w'), (1, 'x')]
                .iter()
                .map(|&(b, c)| if p.flags & b != 0 { c } else { '-' })
                .collect();
            out.push_str(&format!(
                "{:>8} off    0x{:016x} vaddr 0x{:016x} paddr 0x{:016x} align 2**{}\n",
                segment_type_name(p.kind).map_or_else(
                    || format!("0x{:08x}", p.kind),
                    |n| n.trim_start_matches("GNU_").to_string()
                ),
                p.offset,
                p.vaddr,
                p.paddr,
                p.align.checked_ilog2().unwrap_or(0)
            ));
            out.push_str(&format!(
                "         filesz 0x{:016x} memsz 0x{:016x} flags {fl}\n",
                p.filesz, p.memsz
            ));
        }
    }
    if let Some(di) = e.sh.iter().position(|s| s.kind == SHT_DYNAMIC) {
        ensure_blank(out);
        out.push_str("Dynamic Section:\n");
        let strtab = e.data(e.sh[di].link as usize);
        for ent in e.data(di).chunks_exact(16) {
            let tag = rd64(ent, 0).unwrap_or(0);
            let val = rd64(ent, 8).unwrap_or(0);
            if tag == 0 {
                break;
            }
            let name = match dyn_tag_name(tag) {
                Some(n) => n.to_string(),
                None => format!("0x{tag:x}"),
            };
            let text = match tag {
                1 | 14 | 15 | 29 | 0x7fff_fffd | 0x7fff_ffff => {
                    cstr_lossy(strtab, val as usize)
                }
                _ => format!("0x{val:016x}"),
            };
            out.push_str(&format!("  {name:<20} {text}\n"));
        }
    }
    print_version_defs(e, out);
    print_version_refs(e, out);
}

/// A seção de versões do tipo `kind`, depois do título: (conteúdo, tabela de strings, registros).
fn version_section<'a>(e: &Elf<'a>, kind: u32, title: &str, out: &mut String) -> Option<(&'a [u8], &'a [u8], usize)> {
    let si = e.sh.iter().position(|s| s.kind == kind)?;
    let s = &e.sh[si];
    ensure_blank(out);
    out.push_str(title);
    Some((e.data(si), e.data(s.link as usize), s.info as usize))
}

fn print_version_defs(e: &Elf<'_>, out: &mut String) {
    let Some((data, strtab, count)) = version_section(e, SHT_VERDEF, "Version definitions:\n", out) else {
        return;
    };
    walk_chain(data, 0, count, 16, |off| {
        let (flags, ndx, cnt, hash, aux) =
            (rd16(data, off + 2)?, rd16(data, off + 4)?, rd16(data, off + 6)?, rd32(data, off + 8)?, rd32(data, off + 12)?);
        let mut line = format!("{ndx} 0x{flags:02x} 0x{hash:08x} ");
        let mut first = true;
        walk_chain(data, off + aux as usize, usize::from(cnt), 4, |a| {
            let name = rd32(data, a)?;
            if !std::mem::take(&mut first) {
                line.push_str("\n\t");
            }
            line.push_str(&cstr_lossy(strtab, name as usize));
            Some(())
        });
        out.push_str(&line);
        out.push('\n');
        Some(())
    });
}

fn print_version_refs(e: &Elf<'_>, out: &mut String) {
    let Some((data, strtab, count)) = version_section(e, SHT_VERNEED, "Version References:\n", out) else {
        return;
    };
    walk_chain(data, 0, count, 12, |off| {
        let (cnt, file, aux) = (rd16(data, off + 2)?, rd32(data, off + 4)?, rd32(data, off + 8)?);
        out.push_str(&format!("  required from {}:\n", cstr_lossy(strtab, file as usize)));
        walk_chain(data, off + aux as usize, usize::from(cnt), 12, |a| {
            let (hash, flags, other, name) = (rd32(data, a)?, rd16(data, a + 4)?, rd16(data, a + 6)?, rd32(data, a + 8)?);
            out.push_str(&format!("    0x{hash:08x} 0x{flags:02x} {other:02} {}\n", cstr_lossy(strtab, name as usize)));
            Some(())
        });
        Some(())
    });
}

fn section_label(e: &Elf<'_>, shndx: u16) -> String {
    match shndx {
        0 => "*UND*".to_string(),
        0xfff1 => "*ABS*".to_string(),
        0xfff2 => "*COM*".to_string(),
        n => match e.sh.get(usize::from(n)) {
            Some(s) if n < 0xff00 => String::from_utf8_lossy(&s.name).into_owned(),
            _ => "*UND*".to_string(),
        },
    }
}

/// Sete caracteres de flags da linha de símbolo.
fn sym_flags(s: &Sym, dynamic: bool) -> String {
    let bind = s.info >> 4;
    let typ = s.info & 0xf;
    let undefined = s.shndx == 0;
    let c0 = match bind {
        0 => 'l',
        1 if !undefined => 'g',
        10 => 'u',
        _ => ' ',
    };
    let c1 = if bind == 2 { 'w' } else { ' ' };
    let c4 = if typ == 10 { 'i' } else { ' ' };
    let c5 = if typ == STT_SECTION || typ == STT_FILE {
        'd'
    } else if dynamic {
        'D'
    } else {
        ' '
    };
    let c6 = match typ {
        STT_FILE => 'f',
        2 | 10 => 'F',
        1 => 'O',
        _ => ' ',
    };
    [c0, c1, ' ', ' ', c4, c5, c6].iter().collect()
}

fn print_symbols(e: &Elf<'_>, dynamic: bool, out: &mut String) {
    ensure_blank(out);
    out.push_str(if dynamic {
        "DYNAMIC SYMBOL TABLE:\n"
    } else {
        "SYMBOL TABLE:\n"
    });
    let idx = if dynamic {
        e.dynsym_index()
    } else {
        e.symtab_index()
    };
    let Some(idx) = idx else {
        out.push_str("no symbols\n\n");
        return;
    };
    let versions = if dynamic { Some(e.versions()) } else { None };
    for (n, s) in e.read_syms(idx).iter().enumerate().skip(1) {
        let typ = s.info & 0xf;
        let name = if typ == STT_SECTION {
            section_label(e, s.shndx)
        } else {
            String::from_utf8_lossy(&s.name).into_owned()
        };
        let mut field = String::new();
        if let Some(v) = &versions {
            if let Some((_, vn, need, hidden)) = v.of(n) {
                field = if need || hidden { format!("({vn})") } else { vn.to_string() };
            } else if v.has_verdef && v.versym.get(n).is_some_and(|&x| x & 0x7fff == 1) {
                field = "Base".to_string();
            }
        }
        if field.is_empty() {
            field = match s.other & 3 {
                1 => ".internal".to_string(),
                2 => ".hidden".to_string(),
                3 => ".protected".to_string(),
                _ => String::new(),
            };
        }
        out.push_str(&format!(
            "{:016x} {} {}\t{:016x} {field:<12} {name}\n",
            s.value,
            sym_flags(s, dynamic),
            section_label(e, s.shndx),
            s.size
        ));
    }
    out.push('\n');
}

fn reloc_type_name(k: u32) -> String {
    let n = match k {
        0 => "R_X86_64_NONE",
        1 => "R_X86_64_64",
        2 => "R_X86_64_PC32",
        3 => "R_X86_64_GOT32",
        4 => "R_X86_64_PLT32",
        5 => "R_X86_64_COPY",
        6 => "R_X86_64_GLOB_DAT",
        7 => "R_X86_64_JUMP_SLOT",
        8 => "R_X86_64_RELATIVE",
        9 => "R_X86_64_GOTPCREL",
        10 => "R_X86_64_32",
        11 => "R_X86_64_32S",
        12 => "R_X86_64_16",
        13 => "R_X86_64_PC16",
        14 => "R_X86_64_8",
        15 => "R_X86_64_PC8",
        16 => "R_X86_64_DTPMOD64",
        17 => "R_X86_64_DTPOFF64",
        18 => "R_X86_64_TPOFF64",
        19 => "R_X86_64_TLSGD",
        20 => "R_X86_64_TLSLD",
        21 => "R_X86_64_DTPOFF32",
        22 => "R_X86_64_GOTTPOFF",
        23 => "R_X86_64_TPOFF32",
        24 => "R_X86_64_PC64",
        25 => "R_X86_64_GOTOFF64",
        26 => "R_X86_64_GOTPC32",
        27 => "R_X86_64_GOT64",
        28 => "R_X86_64_GOTPCREL64",
        29 => "R_X86_64_GOTPC64",
        30 => "R_X86_64_GOTPLT64",
        31 => "R_X86_64_PLTOFF64",
        32 => "R_X86_64_SIZE32",
        33 => "R_X86_64_SIZE64",
        34 => "R_X86_64_GOTPC32_TLSDESC",
        35 => "R_X86_64_TLSDESC_CALL",
        36 => "R_X86_64_TLSDESC",
        37 => "R_X86_64_IRELATIVE",
        38 => "R_X86_64_RELATIVE64",
        41 => "R_X86_64_GOTPCRELX",
        42 => "R_X86_64_REX_GOTPCRELX",
        _ => return format!("R_X86_64_{k}"),
    };
    n.to_string()
}

const RELOC_HEADER: &str = "OFFSET           TYPE              VALUE \n";

/// Texto da coluna VALUE: símbolo (ou seção, ou `*ABS*`) mais o addend.
fn reloc_value(
    e: &Elf<'_>,
    table: Option<usize>,
    r: &Rel,
    versions: Option<&Versions>,
) -> String {
    let mut name = "*ABS*".to_string();
    if r.sym != 0 {
        if let Some(t) = table {
            let syms = e.read_syms(t);
            if let Some(s) = syms.get(r.sym) {
                if s.info & 0xf == STT_SECTION {
                    name = section_label(e, s.shndx);
                } else {
                    name = String::from_utf8_lossy(&s.name).into_owned();
                    if let Some(v) = versions {
                        if let Some((_, vn, need, hidden)) = v.of(r.sym) {
                            name.push_str(if need || hidden { "@" } else { "@@" });
                            name.push_str(vn);
                        }
                    }
                }
            }
        }
    }
    match r.addend {
        Some(a) if a > 0 => format!("{name}+0x{:016x}", a as u64),
        Some(a) if a < 0 => format!("{name}-0x{:016x}", a.wrapping_neg() as u64),
        _ => name,
    }
}

fn print_reloc_line(
    e: &Elf<'_>,
    table: Option<usize>,
    r: &Rel,
    versions: Option<&Versions>,
    out: &mut String,
) {
    out.push_str(&format!(
        "{:016x} {:<17} {}\n",
        r.offset,
        reloc_type_name(r.kind),
        reloc_value(e, table, r, versions)
    ));
}

/// `-r`: relocações de cada seção que as possui (arquivos relocáveis).
fn print_relocs(e: &Elf<'_>, o: &Opts, out: &mut String) {
    for &i in &e.listed() {
        let s = &e.sh[i];
        if !o.selected(&s.name) || !e.has_relocs(i) {
            continue;
        }
        ensure_blank(out);
        out.push_str(&format!(
            "RELOCATION RECORDS FOR [{}]:\n",
            String::from_utf8_lossy(&s.name)
        ));
        let mut count = 0usize;
        let mut body = String::new();
        for (ri, r) in e.sh.iter().enumerate() {
            if e.absorbed_reloc(r) && r.info as usize == i {
                for rel in e.read_rels(ri) {
                    print_reloc_line(e, Some(r.link as usize), &rel, None, &mut body);
                    count += 1;
                }
            }
        }
        if count == 0 {
            out.push_str("(none)\n\n");
        } else {
            out.push_str(RELOC_HEADER);
            out.push_str(&body);
            out.push('\n');
        }
    }
}

/// `-R`: relocações dinâmicas (seções REL/RELA alocadas que não pertencem a `.symtab`).
fn print_dynamic_relocs(e: &Elf<'_>, out: &mut String) {
    ensure_blank(out);
    out.push_str("DYNAMIC RELOCATION RECORDS");
    let versions = e.versions();
    let mut body = String::new();
    let mut count = 0usize;
    for (ri, r) in e.sh.iter().enumerate() {
        if !matches!(r.kind, SHT_REL | SHT_RELA) || r.flags & SHF_ALLOC == 0 || e.absorbed_reloc(r)
        {
            continue;
        }
        let table = if r.link != 0 {
            Some(r.link as usize)
        } else {
            None
        };
        for rel in e.read_rels(ri) {
            print_reloc_line(e, table, &rel, Some(&versions), &mut body);
            count += 1;
        }
    }
    if count == 0 {
        out.push_str(" (none)\n\n");
    } else {
        out.push('\n');
        out.push_str(RELOC_HEADER);
        out.push_str(&body);
        out.push('\n');
    }
}

/// `-s`: hexdump das seções com conteúdo, endereços pelo VMA.
fn dump_contents(e: &Elf<'_>, o: &Opts, out: &mut String) {
    ensure_blank(out);
    for &i in &e.listed() {
        let s = &e.sh[i];
        if !o.selected(&s.name) || s.kind == SHT_NOBITS || s.size == 0 {
            continue;
        }
        let d = e.data(i);
        if d.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "Contents of section {}:\n",
            String::from_utf8_lossy(&s.name)
        ));
        let end = s.addr.saturating_add(d.len() as u64);
        let digits = format!("{end:x}").len().max(4);
        for (line, chunk) in d.chunks(16).enumerate() {
            out.push_str(&format!(
                " {:0digits$x} ",
                s.addr.wrapping_add((line * 16) as u64)
            ));
            for j in 0..16 {
                match chunk.get(j) {
                    Some(b) => out.push_str(&format!("{b:02x}")),
                    None => out.push_str("  "),
                }
                if j % 4 == 3 {
                    out.push(' ');
                }
            }
            out.push(' ');
            for j in 0..16 {
                match chunk.get(j) {
                    Some(&b) if (0x20..0x7f).contains(&b) => out.push(b as char),
                    Some(_) => out.push('.'),
                    None => out.push(' '),
                }
            }
            out.push('\n');
        }
    }
}

/// `-d`, `-D`, `-S`: cabeçalhos de seção e rótulos de símbolo.
///
/// TODO(disasm): a decodificação x86-64 virá em outro módulo. Enquanto ela não existir NÃO são
/// emitidas linhas de instrução (nem `(bad)`): cada rótulo fica sem corpo, para não inventar
/// saída. Quando o módulo existir, é aqui que ele recebe `e.data(i)` e o VMA do rótulo.
fn disassemble(e: &Elf<'_>, o: &Opts, out: &mut String) {
    let tab = e.symtab_index().or_else(|| e.dynsym_index());
    let syms = tab.map(|t| e.read_syms(t)).unwrap_or_default();
    for &i in &e.listed() {
        let s = &e.sh[i];
        if !o.selected(&s.name) || s.kind == SHT_NOBITS || s.size == 0 {
            continue;
        }
        if !o.disasm_all && !Elf::is_code(s) {
            continue;
        }
        if o.disasm_all && s.flags & SHF_ALLOC == 0 {
            continue;
        }
        let secname = String::from_utf8_lossy(&s.name).into_owned();
        out.push_str(&format!("\nDisassembly of section {secname}:\n"));
        let mut labels: Vec<(u64, &Sym)> = syms
            .iter()
            .filter(|y| {
                usize::from(y.shndx) == i
                    && y.shndx < 0xff00
                    && !y.name.is_empty()
                    && !matches!(y.info & 0xf, STT_SECTION | STT_FILE)
                    && y.value >= s.addr
                    && y.value < s.addr.saturating_add(s.size)
            })
            .map(|y| (y.value, y))
            .collect();
        labels.sort_by_key(|(v, y)| (*v, u8::from(y.info >> 4 == 0)));
        labels.dedup_by_key(|(v, _)| *v);
        if labels.first().is_none_or(|(v, _)| *v != s.addr) {
            out.push_str(&format!("\n{:016x} <{secname}>:\n", s.addr));
        }
        for (v, y) in labels {
            out.push_str(&format!(
                "\n{v:016x} <{}>:\n",
                String::from_utf8_lossy(&y.name)
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_string_for_global_function() {
        let s = Sym {
            name: b"f".to_vec(),
            info: (1 << 4) | 2,
            other: 0,
            shndx: 1,
            value: 0,
            size: 0,
        };
        assert_eq!(sym_flags(&s, false), "g     F");
        assert_eq!(sym_flags(&s, true), "g    DF");
    }

    #[test]
    fn blank_line_is_not_doubled() {
        let mut s = String::from("a\n");
        ensure_blank(&mut s);
        ensure_blank(&mut s);
        assert_eq!(s, "a\n\n");
    }

    #[test]
    fn reloc_type_names() {
        assert_eq!(reloc_type_name(8), "R_X86_64_RELATIVE");
        assert_eq!(reloc_type_name(99), "R_X86_64_99");
    }
}
