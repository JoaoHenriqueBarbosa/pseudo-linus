//! `objcopy` do GNU binutils 2.44 (Debian 13) para ELF64 x86_64 little-endian.
//!
//! Escrito a partir do comportamento observado e da man page (o código do binutils é GPL e não
//! foi consultado). Implementado: cópia de ELF, `-S`/`--strip-all`, `-g`/`--strip-debug`,
//! `--strip-unneeded`, `--strip-dwo`, `-R`/`--remove-section`, `-j`/`--only-section`,
//! `-O binary`, `--add-section`, `--rename-section`, `--set-section-flags`,
//! `--dump-section`, `--set-start` e `--strip-section-headers`. A remoção reaproveita o motor do
//! `strip` (mesmas regras de layout).
//!
//! Estratégia das edições de cabeçalho (`--add-section`, `--rename-section`,
//! `--set-section-flags`): o conteúdo das seções existentes e dos segmentos fica onde está; os
//! dados novos, a `.shstrtab` reconstruída (nomes em sequência, sem fusão de sufixos) e a nova
//! tabela de cabeçalhos de seção são anexados ao fim do arquivo.
//!
//! Divergências conhecidas: o texto do `--help` e várias mensagens de erro foram escritos de
//! memória e precisam de conferência com o oráculo; as demais opções de símbolos
//! (`-N`, `-K`, `-L`, `-G`, `-W`, `--redefine-sym`, `--prefix-*`, `--add-symbol`...), de endereços
//! e de formato (`srec`, `ihex`, `elf32-*`) são aceitas sem efeito; só existem os alvos `binary` e
//! `elf64-x86-64`; o `-O binary` monta a imagem pelos endereços das seções alocadas com conteúdo
//! (o original usa os endereços de carga dos segmentos); arquivos `.a` não são abertos.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, FileType, OFlags, sys};

use crate::strings::{TARGETS, expand_response_files};
use crate::util::io::{self, File};
use crate::util::{Getopt, HasArg, LongOpt};

use super::elf::{is_elf64_le, rd32, rd64};
use super::strip::{Mode, strip_bytes};

const SHORTOPTS: &str = "I:O:B:F:K:N:R:L:G:W:j:b:i:pDUSgxXwMvVhH";

const ID_ADD_SECTION: i32 = 256;
const ID_RENAME_SECTION: i32 = 257;
const ID_SET_FLAGS: i32 = 258;
const ID_DUMP_SECTION: i32 = 259;
const ID_STRIP_UNNEEDED: i32 = 260;
const ID_STRIP_DWO: i32 = 261;
const ID_STRIP_HEADERS: i32 = 262;
const ID_SET_START: i32 = 263;
const IGN: i32 = 999;

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("input-target", HasArg::Required, 'I' as i32),
    LongOpt::new("output-target", HasArg::Required, 'O' as i32),
    LongOpt::new("binary-architecture", HasArg::Required, 'B' as i32),
    LongOpt::new("target", HasArg::Required, 'F' as i32),
    LongOpt::new("debugging", HasArg::No, IGN),
    LongOpt::new("preserve-dates", HasArg::No, 'p' as i32),
    LongOpt::new("enable-deterministic-archives", HasArg::No, 'D' as i32),
    LongOpt::new("disable-deterministic-archives", HasArg::No, 'U' as i32),
    LongOpt::new("only-section", HasArg::Required, 'j' as i32),
    LongOpt::new("add-gnu-debuglink", HasArg::Required, IGN),
    LongOpt::new("remove-section", HasArg::Required, 'R' as i32),
    LongOpt::new("remove-relocations", HasArg::Required, IGN),
    LongOpt::new("strip-section-headers", HasArg::No, ID_STRIP_HEADERS),
    LongOpt::new("strip-all", HasArg::No, 'S' as i32),
    LongOpt::new("strip-debug", HasArg::No, 'g' as i32),
    LongOpt::new("strip-dwo", HasArg::No, ID_STRIP_DWO),
    LongOpt::new("strip-unneeded", HasArg::No, ID_STRIP_UNNEEDED),
    LongOpt::new("strip-symbol", HasArg::Required, 'N' as i32),
    LongOpt::new("strip-unneeded-symbol", HasArg::Required, IGN),
    LongOpt::new("only-keep-debug", HasArg::No, IGN),
    LongOpt::new("extract-dwo", HasArg::No, IGN),
    LongOpt::new("extract-symbol", HasArg::No, IGN),
    LongOpt::new("keep-section", HasArg::Required, IGN),
    LongOpt::new("keep-symbol", HasArg::Required, 'K' as i32),
    LongOpt::new("keep-file-symbols", HasArg::No, IGN),
    LongOpt::new("localize-hidden", HasArg::No, IGN),
    LongOpt::new("localize-symbol", HasArg::Required, 'L' as i32),
    LongOpt::new("globalize-symbol", HasArg::Required, IGN),
    LongOpt::new("keep-global-symbol", HasArg::Required, 'G' as i32),
    LongOpt::new("weaken-symbol", HasArg::Required, 'W' as i32),
    LongOpt::new("weaken", HasArg::No, IGN),
    LongOpt::new("wildcard", HasArg::No, 'w' as i32),
    LongOpt::new("discard-all", HasArg::No, 'x' as i32),
    LongOpt::new("discard-locals", HasArg::No, 'X' as i32),
    LongOpt::new("interleave", HasArg::Optional, 'i' as i32),
    LongOpt::new("interleave-width", HasArg::Required, IGN),
    LongOpt::new("byte", HasArg::Required, 'b' as i32),
    LongOpt::new("gap-fill", HasArg::Required, IGN),
    LongOpt::new("pad-to", HasArg::Required, IGN),
    LongOpt::new("set-start", HasArg::Required, ID_SET_START),
    LongOpt::new("change-start", HasArg::Required, IGN),
    LongOpt::new("adjust-start", HasArg::Required, IGN),
    LongOpt::new("change-addresses", HasArg::Required, IGN),
    LongOpt::new("adjust-vma", HasArg::Required, IGN),
    LongOpt::new("change-section-address", HasArg::Required, IGN),
    LongOpt::new("adjust-section-vma", HasArg::Required, IGN),
    LongOpt::new("change-section-lma", HasArg::Required, IGN),
    LongOpt::new("change-section-vma", HasArg::Required, IGN),
    LongOpt::new("change-warnings", HasArg::No, IGN),
    LongOpt::new("no-change-warnings", HasArg::No, IGN),
    LongOpt::new("adjust-warnings", HasArg::No, IGN),
    LongOpt::new("no-adjust-warnings", HasArg::No, IGN),
    LongOpt::new("set-section-flags", HasArg::Required, ID_SET_FLAGS),
    LongOpt::new("set-section-alignment", HasArg::Required, IGN),
    LongOpt::new("add-section", HasArg::Required, ID_ADD_SECTION),
    LongOpt::new("update-section", HasArg::Required, IGN),
    LongOpt::new("dump-section", HasArg::Required, ID_DUMP_SECTION),
    LongOpt::new("rename-section", HasArg::Required, ID_RENAME_SECTION),
    LongOpt::new("long-section-names", HasArg::Required, IGN),
    LongOpt::new("change-leading-char", HasArg::No, IGN),
    LongOpt::new("remove-leading-char", HasArg::No, IGN),
    LongOpt::new("reverse-bytes", HasArg::Required, IGN),
    LongOpt::new("redefine-sym", HasArg::Required, IGN),
    LongOpt::new("redefine-syms", HasArg::Required, IGN),
    LongOpt::new("srec-len", HasArg::Required, IGN),
    LongOpt::new("srec-forceS3", HasArg::No, IGN),
    LongOpt::new("strip-symbols", HasArg::Required, IGN),
    LongOpt::new("strip-unneeded-symbols", HasArg::Required, IGN),
    LongOpt::new("keep-symbols", HasArg::Required, IGN),
    LongOpt::new("localize-symbols", HasArg::Required, IGN),
    LongOpt::new("globalize-symbols", HasArg::Required, IGN),
    LongOpt::new("keep-global-symbols", HasArg::Required, IGN),
    LongOpt::new("weaken-symbols", HasArg::Required, IGN),
    LongOpt::new("add-symbol", HasArg::Required, IGN),
    LongOpt::new("alt-machine-code", HasArg::Required, IGN),
    LongOpt::new("writable-text", HasArg::No, IGN),
    LongOpt::new("readonly-text", HasArg::No, IGN),
    LongOpt::new("pure", HasArg::No, IGN),
    LongOpt::new("impure", HasArg::No, IGN),
    LongOpt::new("prefix-symbols", HasArg::Required, IGN),
    LongOpt::new("prefix-sections", HasArg::Required, IGN),
    LongOpt::new("prefix-alloc-sections", HasArg::Required, IGN),
    LongOpt::new("file-alignment", HasArg::Required, IGN),
    LongOpt::new("heap", HasArg::Required, IGN),
    LongOpt::new("image-base", HasArg::Required, IGN),
    LongOpt::new("section-alignment", HasArg::Required, IGN),
    LongOpt::new("stack", HasArg::Required, IGN),
    LongOpt::new("subsystem", HasArg::Required, IGN),
    LongOpt::new("compress-debug-sections", HasArg::Optional, IGN),
    LongOpt::new("decompress-debug-sections", HasArg::No, IGN),
    LongOpt::new("elf-stt-common", HasArg::Optional, IGN),
    LongOpt::new("verilog-data-width", HasArg::Required, IGN),
    LongOpt::new("merge-notes", HasArg::No, 'M' as i32),
    LongOpt::new("no-merge-notes", HasArg::No, IGN),
    LongOpt::new("verbose", HasArg::No, 'v' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("info", HasArg::No, IGN),
];

const USAGE_LINES: &[&str] = &[
    " Copies a binary file, possibly transforming it in the process",
    " The options are:",
    "  -I --input-target <bfdname>      Assume input file is in format <bfdname>",
    "  -O --output-target <bfdname>     Create an output file in format <bfdname>",
    "  -B --binary-architecture <arch>  Set output arch, when input is arch-less",
    "  -F --target <bfdname>            Set both input and output format to <bfdname>",
    "     --debugging                   Convert debugging information, if possible",
    "  -p --preserve-dates              Copy modified/access timestamps to the output",
    "  -D --enable-deterministic-archives",
    "                                   Produce deterministic output when stripping archives (default)",
    "  -U --disable-deterministic-archives",
    "                                   Disable -D behavior",
    "  -j --only-section <name>         Only copy section <name> into the output",
    "     --add-gnu-debuglink=<file>    Add section .gnu_debuglink linking to <file>",
    "  -R --remove-section <name>       Remove section <name> from the output",
    "     --remove-relocations <name>   Remove relocations from section <name>",
    "     --strip-section-headers       Strip section headers from the output",
    "  -S --strip-all                   Remove all symbol and relocation information",
    "  -g --strip-debug                 Remove all debugging symbols & sections",
    "     --strip-dwo                   Remove all DWO sections",
    "     --strip-unneeded              Remove all symbols not needed by relocations",
    "  -N --strip-symbol <name>         Do not copy symbol <name>",
    "     --strip-unneeded-symbol <name>",
    "                                   Do not copy symbol <name> unless needed by",
    "                                     relocations",
    "     --only-keep-debug             Strip everything but the debug information",
    "     --extract-dwo                 Copy only DWO sections",
    "     --extract-symbol              Remove section contents but keep symbols",
    "     --keep-section <name>         Do not strip section <name>",
    "  -K --keep-symbol <name>          Do not strip symbol <name>",
    "     --keep-section-symbols        Do not strip section symbols",
    "     --keep-file-symbols           Do not strip file symbol(s)",
    "     --localize-hidden             Turn all ELF hidden symbols into locals",
    "  -L --localize-symbol <name>      Force symbol <name> to be marked as a local",
    "     --globalize-symbol <name>     Force symbol <name> to be marked as a global",
    "  -G --keep-global-symbol <name>   Localize all symbols except <name>",
    "  -W --weaken-symbol <name>        Force symbol <name> to be marked as a weak",
    "     --weaken                      Force all global symbols to be marked as weak",
    "  -w --wildcard                    Permit wildcard in symbol comparison",
    "  -x --discard-all                 Remove all non-global symbols",
    "  -X --discard-locals              Remove any compiler-generated symbols",
    "  -i --interleave [<number>]       Only copy N out of every <abs> bytes",
    "     --interleave-width <number>   Set N for --interleave",
    "  -b --byte <num>                  Select byte <num> in every interleaved block",
    "     --gap-fill <val>              Fill gaps between sections with <val>",
    "     --pad-to <addr>               Pad the last section up to address <addr>",
    "     --set-start <addr>            Set the start address to <addr>",
    "    {--change-start|--adjust-start} <incr>",
    "                                   Add <incr> to the start address",
    "    {--change-addresses|--adjust-vma} <incr>",
    "                                   Add <incr> to LMA, VMA and start addresses",
    "    {--change-section-address|--adjust-section-vma} <name>{=|+|-}<val>",
    "                                   Change LMA and VMA of section <name> by <val>",
    "     --change-section-lma <name>{=|+|-}<val>",
    "                                   Change the LMA of section <name> by <val>",
    "     --change-section-vma <name>{=|+|-}<val>",
    "                                   Change the VMA of section <name> by <val>",
    "    {--[no-]change-warnings|--[no-]adjust-warnings}",
    "                                   Warn if a named section does not exist",
    "     --set-section-flags <name>=<flags>",
    "                                   Set section <name>'s properties to <flags>",
    "     --set-section-alignment <name>=<align>",
    "                                   Set section <name> alignment to <align> bytes",
    "     --add-section <name>=<file>   Add section <name> found in <file> to output",
    "     --update-section <name>=<file>",
    "                                   Update contents of section <name> with",
    "                                   contents found in <file>",
    "     --dump-section <name>=<file>  Copy the contents of section <name> into <file>",
    "     --rename-section <old>=<new>[,<flags>] Rename section <old> to <new>",
    "     --long-section-names {enable|disable|keep}",
    "                                   Handle long section names in Coff objects.",
    "     --change-leading-char         Force output format's leading character style",
    "     --remove-leading-char         Remove leading character from global symbols",
    "     --reverse-bytes=<num>         Reverse <num> bytes at a time, in output sections with content",
    "     --redefine-sym <old>=<new>    Redefine symbol name <old> to <new>",
    "     --redefine-syms <file>        --redefine-sym for all symbol pairs ",
    "                                     listed in <file>",
    "     --srec-len <number>           Restrict the length of generated Srecords",
    "     --srec-forceS3                Restrict the type of generated Srecords to S3",
    "     --strip-symbols <file>        -N for all symbols listed in <file>",
    "     --strip-unneeded-symbols <file>",
    "                                   --strip-unneeded-symbol for all symbols listed",
    "                                     in <file>",
    "     --keep-symbols <file>         -K for all symbols listed in <file>",
    "     --localize-symbols <file>     -L for all symbols listed in <file>",
    "     --globalize-symbols <file>    --globalize-symbol for all in <file>",
    "     --keep-global-symbols <file>  -G for all symbols listed in <file>",
    "     --weaken-symbols <file>       -W for all symbols listed in <file>",
    "     --add-symbol <name>=[<section>:]<value>[,<flags>]  Add a symbol",
    "     --alt-machine-code <index>    Use the target's <index>'th alternative machine",
    "     --writable-text               Mark the output text as writable",
    "     --readonly-text               Make the output text write protected",
    "     --pure                        Mark the output file as demand paged",
    "     --impure                      Mark the output file as impure",
    "     --prefix-symbols <prefix>     Add <prefix> to start of every symbol name",
    "     --prefix-sections <prefix>    Add <prefix> to start of every section name",
    "     --prefix-alloc-sections <prefix>",
    "                                   Add <prefix> to start of every allocatable",
    "                                     section name",
    "     --file-alignment <num>        Set PE file alignment to <num>",
    "     --heap <reserve>[,<commit>]   Set PE reserve/commit heap to <reserve>/",
    "                                   <commit>",
    "     --image-base <address>        Set PE image base to <address>",
    "     --section-alignment <num>     Set PE section alignment to <num>",
    "     --stack <reserve>[,<commit>]  Set PE reserve/commit stack to <reserve>/",
    "                                   <commit>",
    "     --subsystem <name>[:<version>]",
    "                                   Set PE subsystem to <name> [& <version>]",
    "     --compress-debug-sections[={none|zlib|zlib-gnu|zlib-gabi|zstd}]",
    "                                   Compress DWARF debug sections",
    "     --decompress-debug-sections   Decompress DWARF debug sections using zlib",
    "     --elf-stt-common=[yes|no]     Generate ELF common symbols with STT_COMMON",
    "                                     type",
    "     --verilog-data-width <number> Specifies data width, in bytes, for verilog output",
    "  -M  --merge-notes                Remove redundant entries in note sections (default)",
    "      --no-merge-notes             Do not attempt to remove redundant notes",
    "  -v --verbose                     List all object files modified",
    "  @<file>                          Read options from <file>",
    "  -V --version                     Show this program's version",
    "  -h --help                        Show this output",
    "     --info                        List object formats & architectures supported",
];

const SHT_PROGBITS: u32 = 1;
const SHT_NOBITS: u32 = 8;
const SHF_WRITE: u64 = 1;
const SHF_ALLOC: u64 = 2;
const SHF_EXECINSTR: u64 = 4;

#[derive(Default)]
struct Config {
    strip_all: bool,
    strip_debug: bool,
    strip_unneeded: bool,
    strip_dwo: bool,
    strip_headers: bool,
    remove: Vec<Vec<u8>>,
    only: Vec<Vec<u8>>,
    /// Nome da seção e caminho do arquivo com o conteúdo.
    adds: Vec<(Vec<u8>, Vec<u8>)>,
    /// Nome antigo, nome novo e flags opcionais.
    renames: Vec<(Vec<u8>, Vec<u8>, Option<u64>)>,
    set_flags: Vec<(Vec<u8>, u64)>,
    /// Nome da seção e caminho do arquivo de destino.
    dumps: Vec<(Vec<u8>, Vec<u8>)>,
    set_start: Option<u64>,
    output_target: Option<Vec<u8>>,
}

struct Shdr {
    name: Vec<u8>,
    raw: [u8; 64],
}

impl Shdr {
    fn u32_at(&self, at: usize) -> u32 {
        let mut b = [0u8; 4];
        b.copy_from_slice(&self.raw[at..at + 4]);
        u32::from_le_bytes(b)
    }

    fn u64_at(&self, at: usize) -> u64 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&self.raw[at..at + 8]);
        u64::from_le_bytes(b)
    }

    fn kind(&self) -> u32 {
        self.u32_at(4)
    }

    fn flags(&self) -> u64 {
        self.u64_at(8)
    }

    fn addr(&self) -> u64 {
        self.u64_at(16)
    }

    fn offset(&self) -> u64 {
        self.u64_at(24)
    }

    fn size(&self) -> u64 {
        self.u64_at(32)
    }
}

struct Table {
    shstrndx: usize,
    hdrs: Vec<Shdr>,
}

fn read_table(d: &[u8]) -> Option<Table> {
    if !is_elf64_le(d) {
        return None;
    }
    let shoff = usize::try_from(rd64(d, 40)?).ok()?;
    if shoff == 0 {
        return None;
    }
    let mut shnum = usize::from(u16::from_le_bytes([d[60], d[61]]));
    let mut shstrndx = usize::from(u16::from_le_bytes([d[62], d[63]]));
    if shnum == 0 {
        shnum = usize::try_from(rd64(d, shoff.checked_add(32)?)?).ok()?;
    }
    if shstrndx == 0xffff {
        shstrndx = rd32(d, shoff.checked_add(40)?)? as usize;
    }
    let end = shoff.checked_add(shnum.checked_mul(64)?)?;
    if end > d.len() {
        return None;
    }
    let mut hdrs = Vec::with_capacity(shnum);
    for i in 0..shnum {
        let b = shoff + i * 64;
        let mut raw = [0u8; 64];
        raw.copy_from_slice(&d[b..b + 64]);
        hdrs.push(Shdr {
            name: Vec::new(),
            raw,
        });
    }
    let strtab: Vec<u8> = hdrs
        .get(shstrndx)
        .map(|h| section_bytes(d, h).to_vec())
        .unwrap_or_default();
    for h in hdrs.iter_mut() {
        let off = h.u32_at(0) as usize;
        let tail = strtab.get(off..).unwrap_or(&[]);
        let n = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
        h.name = tail[..n].to_vec();
    }
    Some(Table { shstrndx, hdrs })
}

/// Conteúdo de uma seção no arquivo (vazio para NOBITS ou intervalo fora do arquivo).
fn section_bytes<'a>(d: &'a [u8], h: &Shdr) -> &'a [u8] {
    if h.kind() == SHT_NOBITS {
        return &[];
    }
    let (Ok(o), Ok(n)) = (usize::try_from(h.offset()), usize::try_from(h.size())) else {
        return &[];
    };
    o.checked_add(n).and_then(|e| d.get(o..e)).unwrap_or(&[])
}

/// Converte a lista de flags do `--set-section-flags` em `sh_flags`.
fn parse_flags(list: &[u8]) -> u64 {
    let mut f = 0u64;
    let mut readonly = false;
    for w in list.split(|&b| b == b',') {
        match w {
            b"alloc" => f |= SHF_ALLOC,
            b"code" => f |= SHF_EXECINSTR,
            b"merge" => f |= 0x10,
            b"strings" => f |= 0x20,
            b"exclude" => f |= 0x8000_0000,
            b"readonly" => readonly = true,
            _ => {}
        }
    }
    if f & SHF_ALLOC != 0 && !readonly {
        f |= SHF_WRITE;
    }
    f
}

fn split_eq(a: &[u8]) -> Option<(&[u8], &[u8])> {
    let p = a.iter().position(|&b| b == b'=')?;
    Some((&a[..p], &a[p + 1..]))
}

fn parse_number(s: &[u8]) -> u64 {
    let t = String::from_utf8_lossy(s);
    let t = t.trim();
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).unwrap_or(0)
    } else {
        t.parse().unwrap_or(0)
    }
}

fn align_up(v: usize, a: usize) -> usize {
    v.div_ceil(a) * a
}

/// Imagem plana (`-O binary`): seções alocadas com conteúdo, posicionadas pelo endereço.
fn to_binary(d: &[u8]) -> Vec<u8> {
    let Some(t) = read_table(d) else {
        return Vec::new();
    };
    let mut secs: Vec<(u64, &[u8])> = t
        .hdrs
        .iter()
        .filter(|h| h.flags() & SHF_ALLOC != 0 && h.kind() != SHT_NOBITS && h.size() > 0)
        .map(|h| (h.addr(), section_bytes(d, h)))
        .collect();
    secs.sort_by_key(|s| s.0);
    let Some(base) = secs.first().map(|s| s.0) else {
        return Vec::new();
    };
    let mut out: Vec<u8> = Vec::new();
    for (addr, bytes) in secs {
        let Ok(pos) = usize::try_from(addr - base) else {
            continue;
        };
        if pos > (1 << 31) {
            continue;
        }
        let end = pos + bytes.len();
        if end > out.len() {
            out.resize(end, 0);
        }
        out[pos..end].copy_from_slice(bytes);
    }
    out
}

/// `--add-section`, `--set-section-flags` e `--rename-section`: anexa tudo ao fim do arquivo.
fn apply_edits(d: Vec<u8>, added: &[(Vec<u8>, Vec<u8>)], c: &Config) -> Vec<u8> {
    let mut out = d;
    if let Some(v) = c.set_start {
        out[24..32].copy_from_slice(&v.to_le_bytes());
    }
    if added.is_empty() && c.set_flags.is_empty() && c.renames.is_empty() {
        return out;
    }
    let Some(mut t) = read_table(&out) else {
        return out;
    };
    for (name, bytes) in added {
        let off = out.len();
        out.extend_from_slice(bytes);
        let mut raw = [0u8; 64];
        raw[4..8].copy_from_slice(&SHT_PROGBITS.to_le_bytes());
        raw[24..32].copy_from_slice(&(off as u64).to_le_bytes());
        raw[32..40].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
        raw[48..56].copy_from_slice(&1u64.to_le_bytes());
        t.hdrs.push(Shdr {
            name: name.clone(),
            raw,
        });
    }
    for (name, flags) in &c.set_flags {
        if let Some(h) = t.hdrs.iter_mut().skip(1).find(|h| &h.name == name) {
            h.raw[8..16].copy_from_slice(&flags.to_le_bytes());
        }
    }
    for (old, new, flags) in &c.renames {
        if let Some(h) = t.hdrs.iter_mut().skip(1).find(|h| &h.name == old) {
            h.name = new.clone();
            if let Some(f) = flags {
                h.raw[8..16].copy_from_slice(&f.to_le_bytes());
            }
        }
    }
    let mut tab: Vec<u8> = vec![0];
    for h in t.hdrs.iter_mut() {
        let off = if h.name.is_empty() {
            0
        } else {
            let o = tab.len();
            tab.extend_from_slice(&h.name);
            tab.push(0);
            o
        };
        h.raw[0..4].copy_from_slice(&(off as u32).to_le_bytes());
    }
    let shstr_off = out.len();
    out.extend_from_slice(&tab);
    if let Some(h) = t.hdrs.get_mut(t.shstrndx) {
        h.raw[24..32].copy_from_slice(&(shstr_off as u64).to_le_bytes());
        h.raw[32..40].copy_from_slice(&(tab.len() as u64).to_le_bytes());
    }
    let shoff = align_up(out.len(), 8);
    out.resize(shoff, 0);
    for h in &t.hdrs {
        out.extend_from_slice(&h.raw);
    }
    out[40..48].copy_from_slice(&(shoff as u64).to_le_bytes());
    out[60..62].copy_from_slice(&(t.hdrs.len() as u16).to_le_bytes());
    out
}

/// Aplica as opções ao ELF lido e devolve os bytes de saída ou a linha de erro pronta.
fn convert(prog: &str, in_name: &[u8], d: &[u8], c: &Config) -> Result<Vec<u8>, String> {
    let not_recognized = || format!("{prog}: {}: file format not recognized\n", io::lossy(in_name));
    if !is_elf64_le(d) {
        return Err(not_recognized());
    }
    let mut remove = c.remove.clone();
    if let Some(t) = read_table(d) {
        for h in t.hdrs.iter().skip(1) {
            if c.strip_dwo && h.name.ends_with(b".dwo") {
                remove.push(h.name.clone());
            }
            if !c.only.is_empty() && !h.name.is_empty() && !c.only.contains(&h.name) {
                remove.push(h.name.clone());
            }
        }
    }
    let mode = if c.strip_all || c.strip_unneeded {
        Mode::All
    } else if c.strip_debug {
        Mode::Debug
    } else {
        Mode::Keep
    };
    let mut data = if mode != Mode::Keep || !remove.is_empty() {
        strip_bytes(d, mode, &remove).ok_or_else(not_recognized)?
    } else {
        d.to_vec()
    };
    for (name, path) in &c.dumps {
        let Some(t) = read_table(&data) else {
            continue;
        };
        let Some(h) = t.hdrs.iter().find(|h| &h.name == name) else {
            io::eprint(format!(
                "{prog}: can't dump section '{}' - it does not exist\n",
                io::lossy(name)
            ));
            return Err(String::new());
        };
        let bytes = section_bytes(&data, h);
        let r = File::open_with(path, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, 0o666)
            .and_then(|mut f| f.write_all(bytes).map_err(|e| sysabi::Errno::from_io(&e)));
        if let Err(e) = r {
            return Err(format!(
                "{prog}: {}: {}\n",
                io::lossy(path),
                e.message()
            ));
        }
    }
    let mut added = Vec::new();
    for (name, path) in &c.adds {
        match io::read_path(path) {
            Ok(b) => added.push((name.clone(), b)),
            Err(e) => {
                return Err(format!(
                    "{prog}: can't add section '{}': {}\n",
                    io::lossy(name),
                    e.message()
                ));
            }
        }
    }
    data = apply_edits(data, &added, c);
    if c.output_target.as_deref() == Some(b"binary".as_slice()) {
        return Ok(to_binary(&data));
    }
    if c.strip_headers && is_elf64_le(&data) {
        data[40..48].copy_from_slice(&0u64.to_le_bytes());
        data[60..64].copy_from_slice(&[0, 0, 0, 0]);
    }
    Ok(data)
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(prog: &str, to_stdout: bool) -> i32 {
    let mut text = format!("Usage: {prog} [option(s)] in-file [out-file]\n");
    text.push_str(&USAGE_LINES.join("\n"));
    text.push('\n');
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
    let mut c = Config::default();
    let bad_format = |what: &str| -> i32 {
        io::eprint(format!("{prog}: bad format for {what}\n"));
        1
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
            IGN => {}
            ID_ADD_SECTION => match split_eq(&arg) {
                Some((n, f)) => c.adds.push((n.to_vec(), f.to_vec())),
                None => return bad_format("--add-section"),
            },
            ID_DUMP_SECTION => match split_eq(&arg) {
                Some((n, f)) => c.dumps.push((n.to_vec(), f.to_vec())),
                None => return bad_format("--dump-section"),
            },
            ID_SET_FLAGS => match split_eq(&arg) {
                Some((n, f)) => c.set_flags.push((n.to_vec(), parse_flags(f))),
                None => return bad_format("--set-section-flags"),
            },
            ID_RENAME_SECTION => match split_eq(&arg) {
                Some((old, rest)) => {
                    let (new, flags) = match rest.iter().position(|&b| b == b',') {
                        Some(p) => (&rest[..p], Some(parse_flags(&rest[p + 1..]))),
                        None => (rest, None),
                    };
                    c.renames.push((old.to_vec(), new.to_vec(), flags));
                }
                None => return bad_format("--rename-section"),
            },
            ID_STRIP_UNNEEDED => c.strip_unneeded = true,
            ID_STRIP_DWO => c.strip_dwo = true,
            ID_STRIP_HEADERS => c.strip_headers = true,
            ID_SET_START => c.set_start = Some(parse_number(&arg)),
            id => match u8::try_from(id).unwrap_or(0) {
                b'S' => c.strip_all = true,
                b'g' => c.strip_debug = true,
                b'R' => c.remove.push(arg),
                b'j' => c.only.push(arg),
                b'O' | b'F' => c.output_target = Some(arg),
                b'h' | b'H' => return usage(&prog, true),
                b'V' => {
                    super::ar::print_version("objcopy");
                    return 0;
                }
                _ => {}
            },
        }
    }
    let files = g.operands();
    if files.is_empty() || files.len() > 2 {
        return usage(&prog, false);
    }
    let infile = files[0].clone();
    let outfile = files.get(1).cloned().unwrap_or_else(|| infile.clone());
    let say = |parts: &[&[u8]]| {
        let mut m = format!("{prog}: ").into_bytes();
        for p in parts {
            m.extend_from_slice(p);
        }
        m.push(b'\n');
        io::eprint(m);
    };
    let st = match sys::stat(&infile) {
        Err(Errno::ENOENT) => {
            say(&[b"'", &infile, b"': No such file"]);
            return 1;
        }
        Err(e) => {
            say(&[&infile, b": ", e.message().as_bytes()]);
            return 1;
        }
        Ok(st) if st.file_type() == FileType::Directory => {
            say(&[b"Warning: '", &infile, b"' is a directory"]);
            return 1;
        }
        Ok(st) => st,
    };
    let data = match io::read_path(&infile) {
        Ok(d) => d,
        Err(e) => {
            say(&[&infile, b": ", e.message().as_bytes()]);
            return 1;
        }
    };
    if let Some(t) = &c.output_target {
        let ok = matches!(t.as_slice(), b"binary" | b"elf64-x86-64");
        if is_elf64_le(&data) && !ok {
            say(&[&outfile, b": invalid bfd target"]);
            return 1;
        }
    }
    let result = match convert(&prog, &infile, &data, &c) {
        Ok(r) => r,
        Err(m) => {
            io::eprint(m);
            return 1;
        }
    };
    let write = || -> Result<(), Errno> {
        let mut f = File::open_with(
            &outfile,
            OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC,
            st.mode & 0o777,
        )?;
        f.write_all(&result).map_err(|e| sysabi::Errno::from_io(&e))
    };
    if let Err(e) = write() {
        say(&[&outfile, b": ", e.message().as_bytes()]);
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_mapping() {
        assert_eq!(parse_flags(b"alloc,load,readonly,code"), SHF_ALLOC | SHF_EXECINSTR);
        assert_eq!(parse_flags(b"alloc,data"), SHF_ALLOC | SHF_WRITE);
        assert_eq!(parse_flags(b"contents,readonly"), 0);
    }

    #[test]
    fn eq_split() {
        assert_eq!(split_eq(b"a=b=c"), Some((&b"a"[..], &b"b=c"[..])));
        assert_eq!(split_eq(b"abc"), None);
    }

    #[test]
    fn numbers() {
        assert_eq!(parse_number(b"0x10"), 16);
        assert_eq!(parse_number(b"7"), 7);
    }
}
