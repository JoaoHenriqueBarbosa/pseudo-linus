//! `xz` 5.8.1 e os nomes que o Debian liga nele (`unxz`, `xzcat`, `lzma`, `unlzma`, `lzcat`).
//!
//! Escrito a partir do manual do xz, da especificação do formato .xz e do comportamento observado no
//! oráculo. Compressão pelo `lzma-rust2` (presets 0 a 9, `-e`, verificação CRC32/CRC64/SHA-256/nenhuma,
//! `--block-size`); descompressão pelo decodificador por fluxo do `lzma-rust2`, um fluxo .xz por vez,
//! com o enchimento entre fluxos e o "lixo" depois deles tratados aqui, pra reproduzir a distinção do
//! xz entre fim inesperado e dado corrompido. `.lzma` e `.lz` (lzip) também são lidos, como no 5.8.

use std::io::Write;

use sysabi::{Errno, Fd, Stat};

use super::common::{self, Input, Sink};
use super::xzlist::{self, Info, ListError};
use crate::getopt::{Getopt, HasArg, Item, LongOpt};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Compress,
    Decompress,
    Test,
    List,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    Auto,
    Xz,
    Lzma,
    Lzip,
    Raw,
}

const OPT_INFO_MEMORY: u32 = 0x100;
const OPT_NO_SYNC: u32 = 0x101;
const OPT_SINGLE_STREAM: u32 = 0x102;
const OPT_NO_SPARSE: u32 = 0x103;
const OPT_FILES: u32 = 0x104;
const OPT_FILES0: u32 = 0x105;
const OPT_IGNORE_CHECK: u32 = 0x106;
const OPT_BLOCK_SIZE: u32 = 0x107;
const OPT_BLOCK_LIST: u32 = 0x108;
const OPT_MEM_COMPRESS: u32 = 0x109;
const OPT_MEM_DECOMPRESS: u32 = 0x10a;
const OPT_MEM_MT: u32 = 0x10b;
const OPT_NO_ADJUST: u32 = 0x10c;
const OPT_FLUSH_TIMEOUT: u32 = 0x10d;
const OPT_FILTERS: u32 = 0x10e;
const OPT_FILTERS_N: u32 = 0x110; // até 0x118
const OPT_FILTERS_HELP: u32 = 0x120;
const OPT_LZMA1: u32 = 0x121;
const OPT_LZMA2: u32 = 0x122;
const OPT_X86: u32 = 0x123;
const OPT_POWERPC: u32 = 0x124;
const OPT_IA64: u32 = 0x125;
const OPT_ARM: u32 = 0x126;
const OPT_ARMTHUMB: u32 = 0x127;
const OPT_ARM64: u32 = 0x128;
const OPT_SPARC: u32 = 0x129;
const OPT_RISCV: u32 = 0x12a;
const OPT_DELTA: u32 = 0x12b;
const OPT_ROBOT: u32 = 0x12c;

/// Tabela de opções longas na ordem do xz 5.8.1 (a ordem aparece nas mensagens de ambiguidade).
const LONGS: &[LongOpt] = &[
    LongOpt::new("compress", HasArg::No, b'z' as u32),
    LongOpt::new("decompress", HasArg::No, b'd' as u32),
    LongOpt::new("uncompress", HasArg::No, b'd' as u32),
    LongOpt::new("test", HasArg::No, b't' as u32),
    LongOpt::new("list", HasArg::No, b'l' as u32),
    LongOpt::new("keep", HasArg::No, b'k' as u32),
    LongOpt::new("force", HasArg::No, b'f' as u32),
    LongOpt::new("stdout", HasArg::No, b'c' as u32),
    LongOpt::new("to-stdout", HasArg::No, b'c' as u32),
    LongOpt::new("no-sync", HasArg::No, OPT_NO_SYNC),
    LongOpt::new("single-stream", HasArg::No, OPT_SINGLE_STREAM),
    LongOpt::new("no-sparse", HasArg::No, OPT_NO_SPARSE),
    LongOpt::new("suffix", HasArg::Required, b'S' as u32),
    LongOpt::new("files", HasArg::Optional, OPT_FILES),
    LongOpt::new("files0", HasArg::Optional, OPT_FILES0),
    LongOpt::new("format", HasArg::Required, b'F' as u32),
    LongOpt::new("check", HasArg::Required, b'C' as u32),
    LongOpt::new("ignore-check", HasArg::No, OPT_IGNORE_CHECK),
    LongOpt::new("block-size", HasArg::Required, OPT_BLOCK_SIZE),
    LongOpt::new("block-list", HasArg::Required, OPT_BLOCK_LIST),
    LongOpt::new("memlimit-compress", HasArg::Required, OPT_MEM_COMPRESS),
    LongOpt::new("memlimit-decompress", HasArg::Required, OPT_MEM_DECOMPRESS),
    LongOpt::new("memlimit-mt-decompress", HasArg::Required, OPT_MEM_MT),
    LongOpt::new("memlimit", HasArg::Required, b'M' as u32),
    LongOpt::new("memory", HasArg::Required, b'M' as u32),
    LongOpt::new("no-adjust", HasArg::No, OPT_NO_ADJUST),
    LongOpt::new("threads", HasArg::Required, b'T' as u32),
    LongOpt::new("flush-timeout", HasArg::Required, OPT_FLUSH_TIMEOUT),
    LongOpt::new("extreme", HasArg::No, b'e' as u32),
    LongOpt::new("fast", HasArg::No, b'0' as u32),
    LongOpt::new("best", HasArg::No, b'9' as u32),
    LongOpt::new("filters", HasArg::Required, OPT_FILTERS),
    LongOpt::new("filters1", HasArg::Required, OPT_FILTERS_N + 1),
    LongOpt::new("filters2", HasArg::Required, OPT_FILTERS_N + 2),
    LongOpt::new("filters3", HasArg::Required, OPT_FILTERS_N + 3),
    LongOpt::new("filters4", HasArg::Required, OPT_FILTERS_N + 4),
    LongOpt::new("filters5", HasArg::Required, OPT_FILTERS_N + 5),
    LongOpt::new("filters6", HasArg::Required, OPT_FILTERS_N + 6),
    LongOpt::new("filters7", HasArg::Required, OPT_FILTERS_N + 7),
    LongOpt::new("filters8", HasArg::Required, OPT_FILTERS_N + 8),
    LongOpt::new("filters9", HasArg::Required, OPT_FILTERS_N + 9),
    LongOpt::new("filters-help", HasArg::No, OPT_FILTERS_HELP),
    LongOpt::new("lzma1", HasArg::Optional, OPT_LZMA1),
    LongOpt::new("lzma2", HasArg::Optional, OPT_LZMA2),
    LongOpt::new("x86", HasArg::Optional, OPT_X86),
    LongOpt::new("powerpc", HasArg::Optional, OPT_POWERPC),
    LongOpt::new("ia64", HasArg::Optional, OPT_IA64),
    LongOpt::new("arm", HasArg::Optional, OPT_ARM),
    LongOpt::new("armthumb", HasArg::Optional, OPT_ARMTHUMB),
    LongOpt::new("arm64", HasArg::Optional, OPT_ARM64),
    LongOpt::new("sparc", HasArg::Optional, OPT_SPARC),
    LongOpt::new("riscv", HasArg::Optional, OPT_RISCV),
    LongOpt::new("delta", HasArg::Optional, OPT_DELTA),
    LongOpt::new("quiet", HasArg::No, b'q' as u32),
    LongOpt::new("verbose", HasArg::No, b'v' as u32),
    LongOpt::new("no-warn", HasArg::No, b'Q' as u32),
    LongOpt::new("robot", HasArg::No, OPT_ROBOT),
    LongOpt::new("info-memory", HasArg::No, OPT_INFO_MEMORY),
    LongOpt::new("help", HasArg::No, b'h' as u32),
    LongOpt::new("long-help", HasArg::No, b'H' as u32),
    LongOpt::new("version", HasArg::No, b'V' as u32),
];

const SHORTS: &str = "cC:defF:hHlkM:qQS:tT:vVz0123456789";

const SHORT_HELP: &str = "Compress or decompress FILEs in the .xz format.

Mandatory arguments to long options are mandatory for short options too.

  -z, --compress      force compression
  -d, --decompress    force decompression
  -t, --test          test compressed file integrity
  -l, --list          list information about .xz files
  -k, --keep          keep (don't delete) input files
  -f, --force         force overwrite of output file and (de)compress links
  -c, --stdout        write to standard output and don't delete input files
  -0 ... -9           compression preset; default is 6; take compressor *and*
                      decompressor memory usage into account before using 7-9!
  -e, --extreme       try to improve compression ratio by using more CPU time;
                      does not affect decompressor memory requirements
  -T, --threads=NUM   use at most NUM threads; the default is 0 which uses as
                      many threads as there are processor cores
  -q, --quiet         suppress warnings; specify twice to suppress errors too
  -v, --verbose       be verbose; specify twice for even more verbose
  -h, --help          display this short help and exit
  -H, --long-help     display the long help (lists also the advanced options)
  -V, --version       display the version number and exit

With no FILE, or when FILE is -, read standard input.

Report bugs to <xz@tukaani.org> (in English or Finnish).
XZ Utils home page: <https://tukaani.org/xz/>
";

const LONG_HELP: &str = "Compress or decompress FILEs in the .xz format.

Mandatory arguments to long options are mandatory for short options too.

 Operation mode:

  -z, --compress      force compression
  -d, --decompress    force decompression
  -t, --test          test compressed file integrity
  -l, --list          list information about .xz files

 Operation modifiers:

  -k, --keep          keep (don't delete) input files
  -f, --force         force overwrite of output file and (de)compress links
  -c, --stdout        write to standard output and don't delete input files
      --no-sync       don't synchronize the output file to the storage device
                      before removing the input file
      --single-stream decompress only the first stream, and silently ignore
                      possible remaining input data
      --no-sparse     do not create sparse files when decompressing
  -S, --suffix=.SUF   use the suffix '.SUF' on compressed files
      --files[=FILE]  read filenames to process from FILE; if FILE is omitted,
                      filenames are read from the standard input; filenames
                      must be terminated with the newline character
      --files0[=FILE] like --files but use the null character as terminator

 Basic file format and compression options:

  -F, --format=FORMAT file format to encode or decode; possible values are
                      'auto' (default), 'xz', 'lzma', 'lzip', and 'raw'
  -C, --check=NAME    integrity check type: 'none' (use with caution), 'crc32',
                      'crc64' (default), or 'sha256'
      --ignore-check  don't verify the integrity check when decompressing
  -0 ... -9           compression preset; default is 6; take compressor *and*
                      decompressor memory usage into account before using 7-9!
  -e, --extreme       try to improve compression ratio by using more CPU time;
                      does not affect decompressor memory requirements
  -T, --threads=NUM   use at most NUM threads; the default is 0 which uses as
                      many threads as there are processor cores
      --block-size=SIZE
                      start a new .xz block after every SIZE bytes of input;
                      use this to set the block size for threaded compression
      --block-list=BLOCKS
                      start a new .xz block after the given comma-separated
                      intervals of uncompressed data; optionally, specify a
                      filter chain number (0-9) followed by a ':' before the
                      uncompressed data size
      --flush-timeout=NUM
                      when compressing, if more than NUM milliseconds has
                      passed since the previous flush and reading more input
                      would block, all pending data is flushed out
      --memlimit-compress=LIMIT
      --memlimit-decompress=LIMIT
      --memlimit-mt-decompress=LIMIT
  -M, --memlimit=LIMIT
                      set memory usage limit for compression, decompression,
                      threaded decompression, or all of these; LIMIT is in
                      bytes, % of RAM, or 0 for defaults
      --no-adjust     if compression settings exceed the memory usage limit,
                      give an error instead of adjusting the settings downwards

 Custom filter chain for compression (an alternative to using presets):

  --filters=FILTERS   set the filter chain using the liblzma filter string
                      syntax; use --filters-help for more information
  --filters1=FILTERS ... --filters9=FILTERS
                      set additional filter chains using the liblzma filter
                      string syntax to use with --block-list
  --filters-help      display more information about the liblzma filter string
                      syntax and exit

  --lzma1[=OPTS]
  --lzma2[=OPTS]      LZMA1 or LZMA2; OPTS is a comma-separated list of zero or
                      more of the following options (valid values; default):
                        preset=PRE  reset options to a preset (0-9[e])
                        dict=NUM    dictionary size (4KiB - 1536MiB; 8MiB)
                        lc=NUM      number of literal context bits (0-4; 3)
                        lp=NUM      number of literal position bits (0-4; 0)
                        pb=NUM      number of position bits (0-4; 2)
                        mode=MODE   compression mode (fast, normal; normal)
                        nice=NUM    nice length of a match (2-273; 64)
                        mf=NAME     match finder (hc3, hc4, bt2, bt3, bt4; bt4)
                        depth=NUM   maximum search depth; 0=automatic (default)

  --x86[=OPTS]        x86 BCJ filter (32-bit and 64-bit)
  --arm[=OPTS]        ARM BCJ filter
  --armthumb[=OPTS]   ARM-Thumb BCJ filter
  --arm64[=OPTS]      ARM64 BCJ filter
  --powerpc[=OPTS]    PowerPC BCJ filter (big endian only)
  --ia64[=OPTS]       IA-64 (Itanium) BCJ filter
  --sparc[=OPTS]      SPARC BCJ filter
  --riscv[=OPTS]      RISC-V BCJ filter
                      Valid OPTS for all BCJ filters:
                        start=NUM   start offset for conversions (default=0)

  --delta[=OPTS]      Delta filter; valid OPTS (valid values; default):
                        dist=NUM    distance between bytes being subtracted
                                    from each other (1-256; 1)

 Other options:

  -q, --quiet         suppress warnings; specify twice to suppress errors too
  -v, --verbose       be verbose; specify twice for even more verbose
  -Q, --no-warn       make warnings not affect the exit status
      --robot         use machine-parsable messages (useful for scripts)

      --info-memory   display the total amount of RAM and the currently active
                      memory usage limits, and exit
  -h, --help          display the short help (lists only the basic options)
  -H, --long-help     display this long help and exit
  -V, --version       display the version number and exit

With no FILE, or when FILE is -, read standard input.

Report bugs to <xz@tukaani.org> (in English or Finnish).
XZ Utils home page: <https://tukaani.org/xz/>
";

const OK: i32 = 0;
const ERROR: i32 = 1;
const WARNING: i32 = 2;

/// Erro que encerra o programa na hora (mensagem já impressa).
struct Abort;

/// Pré-filtro BCJ ou delta pedido na linha de comando.
#[derive(Clone, Copy)]
struct PreFilter {
    kind: lzma_rust2::FilterType,
    prop: u32,
}

struct Xz {
    prog: String,
    mode: Mode,
    format: Format,
    check: lzma_rust2::CheckType,
    preset: u32,
    extreme: bool,
    keep: bool,
    force: bool,
    stdout: bool,
    single_stream: bool,
    suffix: Option<Vec<u8>>,
    quiet: u32,
    verbose: u32,
    no_warn: bool,
    robot: bool,
    block_size: Option<u64>,
    threads: Option<u64>,
    filters: Vec<PreFilter>,
    exit: i32,
    // Totais do -l.
    list_files: u64,
    list_total_comp: u64,
    list_checks: u32,
    list_padding: u64,
    list_streams: u64,
    list_blocks: u64,
    list_uncomp: u64,
    out: crate::sysutil::Output,
}

/// Como terminou uma decodificação.
enum Decoded {
    Ok,
    Err(&'static str),
    Write(Errno),
    Read(Errno),
}

impl Xz {
    fn error(&mut self, msg: impl AsRef<str>) {
        if self.quiet < 2 {
            common::eprint(format!("{}: {}\n", self.prog, msg.as_ref()));
        }
        self.exit = ERROR;
    }

    fn warn(&mut self, msg: impl AsRef<str>) {
        if self.quiet == 0 {
            common::eprint(format!("{}: {}\n", self.prog, msg.as_ref()));
        }
        if !self.no_warn && self.exit != ERROR {
            self.exit = WARNING;
        }
    }

    fn usage_line(&self) -> String {
        format!("Usage: {} [OPTION]... [FILE]...\n", self.prog)
    }

    fn suffix(&self) -> Vec<u8> {
        match &self.suffix {
            Some(s) => s.clone(),
            None => match self.format {
                Format::Lzma => b".lzma".to_vec(),
                Format::Lzip => b".lz".to_vec(),
                _ => b".xz".to_vec(),
            },
        }
    }

    fn run(&mut self, args: &[Vec<u8>]) -> i32 {
        self.prog = common::show(args.first().map(Vec::as_slice).unwrap_or(b"xz"));
        let base = common::show(common::base_name(args.first().map(Vec::as_slice).unwrap_or(b"xz")));
        // O nome escolhe o modo e o formato padrão, como o xz faz.
        if base.contains("lz") && !base.contains("xz") {
            self.format = Format::Lzma;
        }
        if base.starts_with("un") {
            self.mode = Mode::Decompress;
        }
        if base.ends_with("cat") {
            self.mode = Mode::Decompress;
            self.stdout = true;
        }
        let mut files: Vec<Vec<u8>> = Vec::new();
        let mut files_from: Vec<(Option<Vec<u8>>, u8)> = Vec::new();
        for item in Getopt::from_env(args, SHORTS, LONGS) {
            let o = match item {
                Ok(Item::Operand(op)) => {
                    files.push(op);
                    continue;
                }
                Ok(Item::Opt(o)) => o,
                Err(e) => {
                    common::eprint(e.message_bytes(&self.prog));
                    common::eprint(format!("{}: Try '{} --help' for more information.\n", self.prog, self.prog));
                    return ERROR;
                }
            };
            let arg = o.arg.clone().unwrap_or_default();
            match o.id {
                c @ 0x30..=0x39 => self.preset = c - 0x30,
                0x7a => self.mode = Mode::Compress,
                0x64 => self.mode = Mode::Decompress,
                0x74 => self.mode = Mode::Test,
                0x6c => self.mode = Mode::List,
                0x6b => self.keep = true,
                0x66 => self.force = true,
                0x63 => self.stdout = true,
                0x65 => self.extreme = true,
                0x71 => self.quiet = (self.quiet + 1).min(2),
                0x76 => self.verbose += 1,
                0x51 => self.no_warn = true,
                0x53 => {
                    if arg.is_empty() || arg.contains(&b'/') {
                        common::eprint(format!("{}: {}: Invalid filename suffix\n", self.prog, common::show(&arg)));
                        return ERROR;
                    }
                    self.suffix = Some(arg);
                }
                0x46 => {
                    self.format = match arg.as_slice() {
                        b"auto" => Format::Auto,
                        b"xz" => Format::Xz,
                        b"lzma" | b"alone" => Format::Lzma,
                        b"lzip" => Format::Lzip,
                        b"raw" => Format::Raw,
                        _ => {
                            common::eprint(format!("{}: {}: Unknown file format type\n", self.prog, common::show(&arg)));
                            return ERROR;
                        }
                    }
                }
                0x43 => {
                    self.check = match arg.as_slice() {
                        b"none" => lzma_rust2::CheckType::None,
                        b"crc32" => lzma_rust2::CheckType::Crc32,
                        b"crc64" => lzma_rust2::CheckType::Crc64,
                        b"sha256" => lzma_rust2::CheckType::Sha256,
                        _ => {
                            common::eprint(format!(
                                "{}: {}: Unsupported integrity check type\n",
                                self.prog,
                                common::show(&arg)
                            ));
                            return ERROR;
                        }
                    }
                }
                0x54 => match parse_size(&arg) {
                    Some(n) => self.threads = Some(n),
                    None => {
                        common::eprint(format!("{}: {}: Invalid argument to --threads\n", self.prog, common::show(&arg)));
                        return ERROR;
                    }
                },
                0x4d | OPT_MEM_COMPRESS | OPT_MEM_DECOMPRESS | OPT_MEM_MT | OPT_FLUSH_TIMEOUT => {}
                OPT_BLOCK_SIZE => match parse_size(&arg) {
                    Some(n) if n > 0 => self.block_size = Some(n),
                    _ => {
                        common::eprint(format!("{}: {}: Invalid argument to --block-size\n", self.prog, common::show(&arg)));
                        return ERROR;
                    }
                },
                OPT_SINGLE_STREAM => self.single_stream = true,
                OPT_NO_SYNC | OPT_NO_SPARSE | OPT_IGNORE_CHECK | OPT_NO_ADJUST | OPT_BLOCK_LIST => {}
                OPT_FILES => files_from.push((o.arg.clone(), b'\n')),
                OPT_FILES0 => files_from.push((o.arg.clone(), 0)),
                OPT_ROBOT => self.robot = true,
                OPT_X86 | OPT_POWERPC | OPT_IA64 | OPT_ARM | OPT_ARMTHUMB | OPT_ARM64 | OPT_SPARC | OPT_RISCV => {
                    let kind = match o.id {
                        OPT_X86 => lzma_rust2::FilterType::BcjX86,
                        OPT_POWERPC => lzma_rust2::FilterType::BcjPpc,
                        OPT_IA64 => lzma_rust2::FilterType::BcjIa64,
                        OPT_ARM => lzma_rust2::FilterType::BcjArm,
                        OPT_ARMTHUMB => lzma_rust2::FilterType::BcjArmThumb,
                        OPT_ARM64 => lzma_rust2::FilterType::BcjArm64,
                        OPT_SPARC => lzma_rust2::FilterType::BcjSparc,
                        _ => lzma_rust2::FilterType::BcjRiscv,
                    };
                    let start = opt_value(&arg, b"start").unwrap_or(0) as u32;
                    self.filters.push(PreFilter { kind, prop: start });
                }
                OPT_DELTA => {
                    let dist = opt_value(&arg, b"dist").unwrap_or(1).clamp(1, 256) as u32;
                    self.filters.push(PreFilter { kind: lzma_rust2::FilterType::Delta, prop: dist });
                }
                OPT_LZMA1 | OPT_LZMA2 => {
                    if let Some(p) = opt_value(&arg, b"preset") {
                        self.preset = p.min(9) as u32;
                    }
                }
                OPT_FILTERS | OPT_FILTERS_HELP => {}
                x if (OPT_FILTERS_N..OPT_FILTERS_N + 10).contains(&x) => {}
                0x68 => return self.print(&format!("{}{SHORT_HELP}", self.usage_line())),
                0x48 => return self.print(&format!("{}{LONG_HELP}", self.usage_line())),
                0x56 => {
                    if self.robot {
                        return self.print("XZ_VERSION=50080012\nLIBLZMA_VERSION=50080012\n");
                    }
                    return self.print("xz (XZ Utils) 5.8.1\nliblzma 5.8.1\n");
                }
                OPT_INFO_MEMORY => {}
                _ => {}
            }
        }
        // Nomes vindos de --files/--files0.
        for (src, sep) in files_from {
            let data = match &src {
                None => common::read_all(Fd::STDIN),
                Some(p) => crate::sysutil::read_path(p),
            };
            match data {
                Ok(d) => files.extend(d.split(|&b| b == sep).filter(|s| !s.is_empty()).map(<[u8]>::to_vec)),
                Err(e) => {
                    let n = src.map(|s| common::show(&s)).unwrap_or_else(|| "(stdin)".into());
                    self.error(format!("{n}: {}", e.message()));
                    return ERROR;
                }
            }
        }
        if self.mode == Mode::Compress && self.format == Format::Lzip {
            common::eprint(format!("{}: Compression of lzip files (.lz) is not supported\n", self.prog));
            return ERROR;
        }
        if self.mode == Mode::List {
            if files.is_empty() || files.iter().any(|f| f == b"-") {
                common::eprint(format!("{}: --list does not support reading from standard input\n", self.prog));
                return ERROR;
            }
            if !matches!(self.format, Format::Auto | Format::Xz) {
                common::eprint(format!("{}: --list works only on .xz files (--format=xz or --format=auto)\n", self.prog));
                return ERROR;
            }
            let n = files.len() as u64;
            for (i, f) in files.iter().enumerate() {
                self.list_file(f, i as u64 + 1, n);
            }
            self.list_totals(n);
            self.out.flush();
            return self.exit;
        }
        if files.is_empty() {
            files.push(b"-".to_vec());
        }
        for f in &files {
            if self.process(f).is_err() {
                return self.exit;
            }
        }
        self.exit
    }

    fn print(&self, s: &str) -> i32 {
        match sysabi::sys::write_all(Fd::STDOUT, s.as_bytes()) {
            Ok(()) => OK,
            Err(_) => ERROR,
        }
    }

    fn process(&mut self, name: &[u8]) -> Result<(), Abort> {
        let stdin = name == b"-";
        let shown = if stdin { "(stdin)".to_string() } else { common::show(name) };
        let to_stdout = self.stdout || stdin || self.mode == Mode::Test;
        // Terminal: nada de dado comprimido no terminal.
        if self.mode == Mode::Compress && to_stdout && !self.force && common::isatty(Fd::STDOUT) {
            self.error("Compressed data cannot be written to a terminal");
            return Err(Abort);
        }
        if self.mode != Mode::Compress && stdin && !self.force && common::isatty(Fd::STDIN) {
            self.error("Compressed data cannot be read from a terminal");
            return Err(Abort);
        }
        // Compressão: arquivo que já tem o sufixo.
        if self.mode == Mode::Compress && !stdin && !self.stdout {
            let base = common::base_name(name);
            let mut sfx = vec![self.suffix()];
            if self.suffix.is_none() {
                match self.format {
                    Format::Lzma => sfx.push(b".tlz".to_vec()),
                    _ => sfx.push(b".txz".to_vec()),
                }
            }
            if let Some(s) = sfx.iter().find(|s| base.len() > s.len() && base.ends_with(s)) {
                self.warn(format!("{shown}: File already has '{}' suffix, skipping", common::show(s)));
                return Ok(());
            }
        }
        let (fd, st) = if stdin {
            (Fd::STDIN, common::fstat(Fd::STDIN).ok())
        } else {
            match self.open_src(name, &shown, to_stdout) {
                Some(x) => (x.0, Some(x.1)),
                None => return Ok(()),
            }
        };
        let mut input = Input::new(fd);
        // Descompressão: o formato vem do cabeçalho, antes de decidir o nome de saída.
        let detected = if self.mode == Mode::Compress { None } else { Some(self.detect(&mut input)) };
        if let Some(None) = detected {
            if !stdin {
                common::close(fd);
            }
            if let Some(e) = input.error {
                self.error(format!("{shown}: {}", e.message()));
            } else {
                self.error(format!("{shown}: File format not recognized"));
            }
            return Ok(());
        }
        let dest: Option<Vec<u8>> = if to_stdout {
            None
        } else if self.mode == Mode::Compress {
            Some(common::cat(&[name, &self.suffix()]))
        } else {
            match self.dest_name(name) {
                Some(d) => Some(d),
                None => {
                    common::close(fd);
                    self.warn(format!("{shown}: Filename has an unknown suffix, skipping"));
                    return Ok(());
                }
            }
        };
        let ofd = match &dest {
            Some(d) => match self.create_dest(d) {
                Some(f) => f,
                None => {
                    common::close(fd);
                    return Ok(());
                }
            },
            None => Fd::STDOUT,
        };
        let mut sink = if self.mode == Mode::Test { Sink::null() } else { Sink::fd(ofd) };
        let result = match self.mode {
            Mode::Compress => self.encode(&mut input, &mut sink).map_err(|e| (e.message().to_string(), true)),
            _ => match self.decode(detected.flatten().unwrap_or(Format::Xz), &mut input, &mut sink) {
                Decoded::Ok => Ok(()),
                Decoded::Err(m) => Err((m.to_string(), false)),
                Decoded::Write(e) => Err((e.message(), true)),
                Decoded::Read(e) => Err((e.message(), false)),
            },
        };
        if !stdin {
            common::close(fd);
        }
        match result {
            Ok(()) => {
                if let (Some(d), Some(st)) = (&dest, &st) {
                    common::copy_attrs(ofd, d, st, st.mtime);
                    common::close(ofd);
                    if !self.keep {
                        let _ = common::unlink(name);
                    }
                }
                if self.verbose > 0 {
                    let (comp, uncomp) = if self.mode == Mode::Compress {
                        (sink.written, input.consumed)
                    } else {
                        (input.consumed, sink.written)
                    };
                    common::eprint(format!("{shown}: {} / {} {}\n", nice(comp), nice(uncomp), ratio_sep(comp, uncomp)));
                }
            }
            Err((msg, write_side)) => {
                let who = match (&dest, write_side) {
                    (Some(d), true) => common::show(d),
                    (None, true) => "(stdout)".into(),
                    _ => shown.clone(),
                };
                self.error(format!("{who}: {msg}"));
                if let Some(d) = &dest {
                    common::close(ofd);
                    let _ = common::unlink(d);
                }
            }
        }
        Ok(())
    }

    /// Abre a entrada com as conferências do xz. `None` quando já reportou.
    fn open_src(&mut self, name: &[u8], shown: &str, to_stdout: bool) -> Option<(Fd, Stat)> {
        let lst = match common::lstat(name) {
            Ok(s) => s,
            Err(e) => {
                self.error(format!("{shown}: {}", e.message()));
                return None;
            }
        };
        let follow = self.force || to_stdout;
        if lst.file_type() == sysabi::FileType::Symlink && !follow {
            self.warn(format!("{shown}: Is a symbolic link, skipping"));
            return None;
        }
        let st = match common::stat(name) {
            Ok(s) => s,
            Err(e) => {
                self.error(format!("{shown}: {}", e.message()));
                return None;
            }
        };
        if st.file_type() == sysabi::FileType::Directory {
            self.warn(format!("{shown}: Is a directory, skipping"));
            return None;
        }
        if !to_stdout {
            if st.file_type() != sysabi::FileType::Regular {
                self.warn(format!("{shown}: Not a regular file, skipping"));
                return None;
            }
            if !self.force {
                if st.mode & (common::S_ISUID | common::S_ISGID) != 0 {
                    self.warn(format!("{shown}: File has setuid or setgid bit set, skipping"));
                    return None;
                }
                if st.mode & common::S_ISVTX != 0 {
                    self.warn(format!("{shown}: File has sticky bit set, skipping"));
                    return None;
                }
                if st.nlink > 1 {
                    self.warn(format!("{shown}: Input file has more than one hard link, skipping"));
                    return None;
                }
            }
        }
        match common::open_input(name, false) {
            Ok(fd) => Some((fd, st)),
            Err(e) => {
                self.error(format!("{shown}: {}", e.message()));
                None
            }
        }
    }

    fn create_dest(&mut self, dest: &[u8]) -> Option<Fd> {
        let shown = common::show(dest);
        loop {
            match common::create_exclusive(dest) {
                Ok(fd) => return Some(fd),
                Err(Errno::EEXIST) if self.force => {
                    if let Err(e) = common::unlink(dest) {
                        self.error(format!("{shown}: Cannot remove: {}", e.message()));
                        return None;
                    }
                }
                Err(e) => {
                    self.error(format!("{shown}: {}", e.message()));
                    return None;
                }
            }
        }
    }

    /// Nome de saída da descompressão, pelos sufixos conhecidos.
    fn dest_name(&self, name: &[u8]) -> Option<Vec<u8>> {
        let base_len = common::base_name(name).len();
        let mut table: Vec<(Vec<u8>, &[u8])> = Vec::new();
        if let Some(s) = &self.suffix {
            table.push((s.clone(), b""));
        }
        match self.format {
            Format::Lzma => {
                table.push((b".lzma".to_vec(), b""));
                table.push((b".tlz".to_vec(), b".tar"));
            }
            Format::Lzip => table.push((b".lz".to_vec(), b"")),
            _ => {
                table.push((b".xz".to_vec(), b""));
                table.push((b".txz".to_vec(), b".tar"));
                table.push((b".lzma".to_vec(), b""));
                table.push((b".tlz".to_vec(), b".tar"));
                table.push((b".lz".to_vec(), b""));
            }
        }
        for (s, r) in table {
            if base_len > s.len() && name.ends_with(&s) {
                return Some(common::cat(&[&name[..name.len() - s.len()], r]));
            }
        }
        None
    }

    /// Reconhece o formato pelo começo da entrada. `None` = não reconhecido.
    fn detect(&self, input: &mut Input) -> Option<Format> {
        let head = input.ensure(13).to_vec();
        let xz = head.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0]);
        let lzip = head.starts_with(b"LZIP");
        let lzma = looks_like_lzma(&head);
        match self.format {
            Format::Auto => {
                if xz {
                    Some(Format::Xz)
                } else if lzip {
                    Some(Format::Lzip)
                } else if lzma {
                    Some(Format::Lzma)
                } else {
                    None
                }
            }
            Format::Xz => xz.then_some(Format::Xz),
            Format::Lzip => lzip.then_some(Format::Lzip),
            Format::Lzma => (head.len() >= 13 && head[0] < 225).then_some(Format::Lzma),
            Format::Raw => Some(Format::Raw),
        }
    }

    fn encode(&self, input: &mut Input, sink: &mut Sink) -> Result<(), Errno> {
        let io = |e: std::io::Error| Errno::from_io(&e);
        let mut lz = lzma_rust2::LzmaOptions::with_preset(self.preset);
        if self.extreme {
            // Ajuste do modo extremo dos presets do liblzma.
            lz.mode = lzma_rust2::EncodeMode::Normal;
            lz.mf = lzma_rust2::MfType::Bt4;
            if self.preset == 3 || self.preset == 5 {
                lz.nice_len = 192;
                lz.depth_limit = 0;
            } else {
                lz.nice_len = 273;
                lz.depth_limit = 512;
            }
        }
        let feed = |w: &mut dyn Write, input: &mut Input| -> Result<(), Errno> {
            loop {
                let chunk = input.fill();
                if chunk.is_empty() {
                    break;
                }
                let n = chunk.len();
                w.write_all(chunk).map_err(io)?;
                input.consume(n);
            }
            match input.error {
                Some(e) => Err(e),
                None => Ok(()),
            }
        };
        match self.format {
            Format::Lzma => {
                let mut w = lzma_rust2::LzmaWriter::new_use_header(&mut *sink, &lz, None).map_err(io)?;
                feed(&mut w, input)?;
                w.finish().map_err(io)?;
            }
            _ if self.threads == Some(1) => {
                // `-T1`: o formato de uma thread, sem tamanhos no cabeçalho do bloco.
                let mut opts = lzma_rust2::XzOptions { lzma_options: lz, ..lzma_rust2::XzOptions::default() };
                opts.set_check_sum_type(self.check);
                opts.set_block_size(self.block_size.and_then(std::num::NonZeroU64::new));
                for f in self.filters.iter().rev() {
                    opts.prepend_pre_filter(f.kind, f.prop);
                }
                let mut w = lzma_rust2::XzWriter::new(&mut *sink, opts).map_err(io)?;
                feed(&mut w, input)?;
                w.finish().map_err(io)?;
            }
            _ => {
                // Padrão do xz 5.8 (`-T0`): blocos inteiros com os tamanhos no cabeçalho.
                let filter = self.filters.last().map(|f| (f.kind, f.prop));
                let mut w = super::xzenc::XzBlockEncoder::new(&mut *sink, lz, self.check, filter, self.block_size);
                feed(&mut w, input)?;
                w.finish().map_err(io)?;
            }
        }
        Ok(())
    }

    /// Decodifica a entrada inteira no formato já reconhecido.
    fn decode(&self, fmt: Format, input: &mut Input, sink: &mut Sink) -> Decoded {
        match fmt {
            Format::Xz => self.decode_xz(input, sink),
            Format::Lzma => decode_simple(input, sink, lzma_rust2::LzmaStream::new_mem_limit(u32::MAX, None)),
            Format::Lzip => decode_simple(input, sink, lzma_rust2::LzipStream::new()),
            _ => Decoded::Err("Unsupported options"),
        }
    }

    fn decode_xz(&self, input: &mut Input, sink: &mut Sink) -> Decoded {
        let mut out = vec![0u8; common::CHUNK];
        let mut first = true;
        loop {
            if !first {
                // Enchimento entre fluxos: zeros em grupos de 4.
                let mut zeros = 0u64;
                loop {
                    if input.fill().is_empty() {
                        if let Some(e) = input.error {
                            return Decoded::Read(e);
                        }
                        return if zeros.is_multiple_of(4) { Decoded::Ok } else { Decoded::Err("Compressed data is corrupt") };
                    }
                    let a = input.available();
                    let (n, len) = (a.iter().take_while(|&&b| b == 0).count(), a.len());
                    zeros += n as u64;
                    input.consume(n);
                    if n < len {
                        break;
                    }
                }
                if !zeros.is_multiple_of(4) {
                    return Decoded::Err("Compressed data is corrupt");
                }
                if self.single_stream {
                    return Decoded::Ok;
                }
                let head = input.ensure(12);
                if head.len() < 12 {
                    return Decoded::Err("Unexpected end of input");
                }
                if !head.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0]) {
                    return Decoded::Err("Compressed data is corrupt");
                }
            }
            let mut st = lzma_rust2::XzStream::new(false);
            loop {
                sysabi::sys::checkpoint();
                let empty = input.fill().is_empty();
                let action = if empty { lzma_rust2::Action::Finish } else { lzma_rust2::Action::Run };
                if empty && let Some(e) = input.error {
                    return Decoded::Read(e);
                }
                let r = st.process(input.available(), &mut out, action);
                match r {
                    Ok(res) => {
                        input.consume(res.bytes_consumed);
                        if res.bytes_produced > 0
                            && let Err(e) = sink.write_all(&out[..res.bytes_produced])
                        {
                            return Decoded::Write(Errno::from_io(&e));
                        }
                        if res.status == lzma_rust2::Status::StreamEnd {
                            break;
                        }
                        if action == lzma_rust2::Action::Finish && res.bytes_produced == 0 {
                            return Decoded::Err("Unexpected end of input");
                        }
                    }
                    Err(e) => return Decoded::Err(classify_xz(&e)),
                }
            }
            first = false;
            if self.single_stream {
                return Decoded::Ok;
            }
        }
    }

    /// `xz -l` de um arquivo.
    fn list_file(&mut self, name: &[u8], index: u64, count: u64) {
        let shown = common::show(name);
        let st = match common::stat(name) {
            Ok(s) => s,
            Err(e) => {
                self.error(format!("{shown}: {}", e.message()));
                return;
            }
        };
        if st.file_type() == sysabi::FileType::Directory {
            self.warn(format!("{shown}: Is a directory, skipping"));
            return;
        }
        if st.file_type() != sysabi::FileType::Regular {
            self.warn(format!("{shown}: Not a regular file, skipping"));
            return;
        }
        let data = match crate::sysutil::read_path(name) {
            Ok(d) => d,
            Err(e) => {
                self.error(format!("{shown}: {}", e.message()));
                return;
            }
        };
        let info = match xzlist::parse(&data) {
            Ok(i) => i,
            Err(ListError::TooSmall) => {
                self.error(format!("{shown}: Too small to be a valid .xz file"));
                return;
            }
            Err(ListError::NotFormat) => {
                self.error(format!("{shown}: File format not recognized"));
                return;
            }
            Err(ListError::Corrupt) => {
                self.error(format!("{shown}: Compressed data is corrupt"));
                return;
            }
        };
        self.list_files += 1;
        self.list_streams += info.streams.len() as u64;
        self.list_blocks += info.block_count();
        self.list_total_comp += info.file_size;
        self.list_uncomp += info.uncomp_size();
        self.list_checks |= info.checks();
        self.list_padding += info.padding();
        if self.robot {
            self.robot_file(&shown, &info);
        } else if self.verbose > 0 {
            self.verbose_file(&shown, &info, index, count);
        } else {
            if self.list_files == 1 {
                self.out.write_str("Strms  Blocks   Compressed Uncompressed  Ratio  Check   Filename\n");
            }
            let line = format!(
                "{:>5} {:>7}  {:>11}  {:>11}  {:>5}  {:<7} {shown}\n",
                info.streams.len(),
                info.block_count(),
                nice(info.file_size),
                nice(info.uncomp_size()),
                list_ratio(info.file_size, info.uncomp_size()),
                xzlist::checks_names(info.checks()),
            );
            self.out.write_str(&line);
        }
    }

    fn verbose_file(&mut self, shown: &str, info: &Info, index: u64, count: u64) {
        let mut s = String::new();
        if self.list_files > 1 {
            s.push('\n');
        }
        s.push_str(&format!("{shown} ({index}/{count})\n"));
        s.push_str(&format!("  Streams:           {}\n", info.streams.len()));
        s.push_str(&format!("  Blocks:            {}\n", info.block_count()));
        s.push_str(&format!("  Compressed size:   {}\n", nice_both(info.file_size)));
        s.push_str(&format!("  Uncompressed size: {}\n", nice_both(info.uncomp_size())));
        s.push_str(&format!("  Ratio:             {}\n", list_ratio(info.file_size, info.uncomp_size())));
        s.push_str(&format!("  Check:             {}\n", xzlist::checks_names(info.checks())));
        s.push_str(&format!("  Stream Padding:    {}\n", nice(info.padding())));
        s.push_str("  Streams:\n    Stream    Blocks      CompOffset    UncompOffset        CompSize      UncompSize  Ratio  Check      Padding\n");
        for st in &info.streams {
            s.push_str(&format!(
                "    {:>6} {:>9} {:>15} {:>15} {:>15} {:>15}  {:>5}  {:<10} {:>7}\n",
                st.number,
                st.blocks.len(),
                st.comp_offset,
                st.uncomp_offset,
                st.comp_size,
                st.uncomp_size,
                list_ratio(st.comp_size, st.uncomp_size),
                xzlist::check_name(st.check),
                st.padding
            ));
        }
        s.push_str("  Blocks:\n    Stream     Block      CompOffset    UncompOffset       TotalSize      UncompSize  Ratio  Check\n");
        for st in &info.streams {
            for b in &st.blocks {
                s.push_str(&format!(
                    "    {:>6} {:>9} {:>15} {:>15} {:>15} {:>15}  {:>5}  {}\n",
                    st.number,
                    b.number_in_stream,
                    b.comp_offset,
                    b.uncomp_offset,
                    b.total_size,
                    b.uncomp_size,
                    list_ratio(b.total_size, b.uncomp_size),
                    xzlist::check_name(st.check)
                ));
            }
        }
        self.out.write_str(&s);
    }

    fn robot_file(&mut self, shown: &str, info: &Info) {
        let mut s = format!("name\t{shown}\n");
        s.push_str(&format!(
            "file\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            info.streams.len(),
            info.block_count(),
            info.file_size,
            info.uncomp_size(),
            list_ratio(info.file_size, info.uncomp_size()),
            xzlist::checks_names(info.checks()),
            info.padding()
        ));
        if self.verbose > 0 {
            for st in &info.streams {
                s.push_str(&format!(
                    "stream\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                    st.number,
                    st.blocks.len(),
                    st.comp_offset,
                    st.uncomp_offset,
                    st.comp_size,
                    st.uncomp_size,
                    list_ratio(st.comp_size, st.uncomp_size),
                    xzlist::check_name(st.check),
                    st.padding
                ));
            }
            for st in &info.streams {
                for b in &st.blocks {
                    s.push_str(&format!(
                        "block\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                        st.number,
                        b.number_in_stream,
                        b.number_in_file,
                        b.comp_offset,
                        b.uncomp_offset,
                        b.total_size,
                        b.uncomp_size,
                        list_ratio(b.total_size, b.uncomp_size),
                        xzlist::check_name(st.check)
                    ));
                }
            }
        }
        self.out.write_str(&s);
    }

    fn list_totals(&mut self, count: u64) {
        if self.list_files == 0 {
            return;
        }
        let checks = xzlist::checks_names(self.list_checks);
        let ratio = list_ratio(self.list_total_comp, self.list_uncomp);
        if self.robot {
            let s = format!(
                "totals\t{}\t{}\t{}\t{}\t{ratio}\t{checks}\t{}\t{}\n",
                self.list_streams, self.list_blocks, self.list_total_comp, self.list_uncomp, self.list_padding, self.list_files
            );
            self.out.write_str(&s);
            return;
        }
        if count < 2 {
            return;
        }
        if self.verbose > 0 {
            let s = format!(
                "\nTotals:\n  Number of files:   {}\n  Streams:           {}\n  Blocks:            {}\n  Compressed size:   {}\n  Uncompressed size: {}\n  Ratio:             {ratio}\n  Check:             {}\n  Stream Padding:    {}\n",
                self.list_files,
                self.list_streams,
                self.list_blocks,
                nice_both(self.list_total_comp),
                nice_both(self.list_uncomp),
                // Nos totais do -lv as verificações vêm separadas por vírgula e espaço.
                checks.replace(',', ", "),
                nice(self.list_padding)
            );
            self.out.write_str(&s);
        } else {
            let s = format!(
                "-------------------------------------------------------------------------------\n{:>5} {:>7}  {:>11}  {:>11}  {:>5}  {:<7} {} file{}\n",
                self.list_streams,
                self.list_blocks,
                nice(self.list_total_comp),
                nice(self.list_uncomp),
                ratio,
                checks,
                self.list_files,
                if self.list_files == 1 { "" } else { "s" }
            );
            self.out.write_str(&s);
        }
    }
}

/// Decodificador por fluxo do `lzma-rust2` (lzma ou lzip) até o fim.
trait SimpleStream {
    fn step(&mut self, input: &[u8], out: &mut [u8], action: lzma_rust2::Action) -> lzma_rust2::Result<lzma_rust2::StreamResult>;
}

impl SimpleStream for lzma_rust2::LzmaStream {
    fn step(&mut self, input: &[u8], out: &mut [u8], action: lzma_rust2::Action) -> lzma_rust2::Result<lzma_rust2::StreamResult> {
        self.process(input, out, action)
    }
}

impl SimpleStream for lzma_rust2::LzipStream {
    fn step(&mut self, input: &[u8], out: &mut [u8], action: lzma_rust2::Action) -> lzma_rust2::Result<lzma_rust2::StreamResult> {
        self.process(input, out, action)
    }
}

fn decode_simple(input: &mut Input, sink: &mut Sink, mut st: impl SimpleStream) -> Decoded {
    let mut out = vec![0u8; common::CHUNK];
    loop {
        sysabi::sys::checkpoint();
        let empty = input.fill().is_empty();
        if empty && let Some(e) = input.error {
            return Decoded::Read(e);
        }
        let action = if empty { lzma_rust2::Action::Finish } else { lzma_rust2::Action::Run };
        match st.step(input.available(), &mut out, action) {
            Ok(res) => {
                input.consume(res.bytes_consumed);
                if res.bytes_produced > 0
                    && let Err(e) = sink.write_all(&out[..res.bytes_produced])
                {
                    return Decoded::Write(Errno::from_io(&e));
                }
                if res.status == lzma_rust2::Status::StreamEnd {
                    return Decoded::Ok;
                }
                if action == lzma_rust2::Action::Finish && res.bytes_produced == 0 {
                    return Decoded::Err("Unexpected end of input");
                }
            }
            Err(e) => return Decoded::Err(classify_xz(&e)),
        }
    }
}

/// Mensagem do xz pra um erro do decodificador.
fn classify_xz(e: &std::io::Error) -> &'static str {
    if e.kind() == std::io::ErrorKind::UnexpectedEof {
        return "Unexpected end of input";
    }
    let m = e.to_string().to_lowercase();
    if m.contains("unexpected end") || m.contains("truncated") || m.contains("incomplete") {
        "Unexpected end of input"
    } else if m.contains("memory") {
        "Memory usage limit reached"
    } else if m.contains("unsupported") || m.contains("no lzma2 filter") {
        "Unsupported options"
    } else {
        "Compressed data is corrupt"
    }
}

/// Cabeçalho .lzma plausível, com as regras do detector do xz no modo automático: byte de
/// propriedades válido, dicionário 2^n ou 2^n + 2^(n-1), tamanho desconhecido ou menor que 2^38.
fn looks_like_lzma(h: &[u8]) -> bool {
    if h.len() < 13 || h[0] >= 225 {
        return false;
    }
    let dict = u32::from_le_bytes([h[1], h[2], h[3], h[4]]);
    let mut d = dict.wrapping_sub(1);
    d |= d >> 2;
    d |= d >> 3;
    d |= d >> 4;
    d |= d >> 8;
    d |= d >> 16;
    d = d.wrapping_add(1);
    if d != dict {
        return false;
    }
    let size = u64::from_le_bytes([h[5], h[6], h[7], h[8], h[9], h[10], h[11], h[12]]);
    size == u64::MAX || size < (1 << 38)
}

/// Número com unidade como o xz mostra: bytes até 9999, depois KiB, MiB... com uma casa.
pub fn nice(v: u64) -> String {
    if v < 10000 {
        return format!("{v} B");
    }
    let mut d = v as f64 / 1024.0;
    let mut unit = "KiB";
    for u in ["MiB", "GiB", "TiB"] {
        if d < 10000.0 {
            break;
        }
        d /= 1024.0;
        unit = u;
    }
    format!("{d:.1} {unit}")
}

/// Como `nice`, com o valor exato em bytes entre parênteses quando a unidade não é byte.
fn nice_both(v: u64) -> String {
    if v < 10000 { nice(v) } else { format!("{} ({v} B)", nice(v)) }
}

/// Razão do `-l`: comprimido sobre descomprimido, `---` sem base.
fn list_ratio(comp: u64, uncomp: u64) -> String {
    if uncomp == 0 {
        return "---".into();
    }
    let r = comp as f64 / uncomp as f64;
    if r > 9.999 { "> 9.999".into() } else { format!("{r:.3}") }
}

/// Razão do `-v`: `= x.xxx`, ou `> 9.999` quando passa disso ou não há base.
fn ratio_sep(comp: u64, uncomp: u64) -> String {
    if uncomp == 0 {
        return "> 9.999".into();
    }
    let r = comp as f64 / uncomp as f64;
    if r > 9.999 { "> 9.999".into() } else { format!("= {r:.3}") }
}

/// Tamanho com sufixo (`k`, `KiB`, `M`, `MiB`, `G`, `GiB`), como as opções do xz aceitam.
fn parse_size(s: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(s).ok()?;
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    let n: u64 = digits.parse().ok()?;
    let mult: u64 = match &text[digits.len()..] {
        "" => 1,
        "k" | "K" | "KiB" | "kiB" | "Ki" => 1024,
        "M" | "MiB" | "Mi" => 1 << 20,
        "G" | "GiB" | "Gi" => 1 << 30,
        _ => return None,
    };
    n.checked_mul(mult)
}

/// Valor numérico `nome=N` numa lista de opções de filtro (`dist=4,start=0`).
fn opt_value(opts: &[u8], name: &[u8]) -> Option<u64> {
    for part in opts.split(|&b| b == b',') {
        if let Some(v) = part.strip_prefix(name).and_then(|r| r.strip_prefix(b"=")) {
            return parse_size(v);
        }
    }
    None
}

/// Entrada do `xz` e dos nomes ligados a ele (o modo vem do argv[0]).
pub fn main(args: &[Vec<u8>]) -> i32 {
    let mut x = Xz {
        prog: String::new(),
        mode: Mode::Compress,
        format: Format::Auto,
        check: lzma_rust2::CheckType::Crc64,
        preset: 6,
        extreme: false,
        keep: false,
        force: false,
        stdout: false,
        single_stream: false,
        suffix: None,
        quiet: 0,
        verbose: 0,
        no_warn: false,
        robot: false,
        block_size: None,
        threads: None,
        filters: Vec::new(),
        exit: OK,
        list_files: 0,
        list_total_comp: 0,
        list_checks: 0,
        list_padding: 0,
        list_streams: 0,
        list_blocks: 0,
        list_uncomp: 0,
        out: crate::sysutil::Output::stdout(),
    };
    x.run(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_like_xz() {
        assert_eq!(nice(5476), "5476 B");
        assert_eq!(nice(100_000), "97.7 KiB");
        assert_eq!(nice(2_688_895), "2625.9 KiB");
        assert_eq!(nice(30_000_000), "28.6 MiB");
        assert_eq!(list_ratio(32, 0), "---");
        assert_eq!(ratio_sep(76, 12), "= 6.333");
        assert_eq!(ratio_sep(32, 0), "> 9.999");
    }
}
