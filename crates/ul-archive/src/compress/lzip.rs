//! `lzip` 1.25. Escrito a partir do manual do lzip, da especificação do formato .lz (membros com
//! cabeçalho de 6 bytes, fluxo LZMA com marcador de fim e rodapé de 20 bytes com CRC32, tamanho dos
//! dados e tamanho do membro) e do comportamento observado no oráculo. O enquadramento dos membros e
//! o tratamento do que sobra depois deles são nossos; o LZMA é do `lzma-rust2`.

use std::io::Write;

use sysabi::{Errno, Fd, Stat};

use super::common::{self, Input, Sink};
use crate::getopt::{Error as OptError, Getopt, HasArg, Item, LongOpt};

const OPT_LOOSE: u32 = 0x100;

const LONGS: &[LongOpt] = &[
    LongOpt::new("trailing-error", HasArg::No, b'a' as u32),
    LongOpt::new("member-size", HasArg::Required, b'b' as u32),
    LongOpt::new("stdout", HasArg::No, b'c' as u32),
    LongOpt::new("decompress", HasArg::No, b'd' as u32),
    LongOpt::new("fast", HasArg::No, b'0' as u32),
    LongOpt::new("best", HasArg::No, b'9' as u32),
    LongOpt::new("force", HasArg::No, b'f' as u32),
    LongOpt::new("recompress", HasArg::No, b'F' as u32),
    LongOpt::new("help", HasArg::No, b'h' as u32),
    LongOpt::new("keep", HasArg::No, b'k' as u32),
    LongOpt::new("list", HasArg::No, b'l' as u32),
    LongOpt::new("loose-trailing", HasArg::No, OPT_LOOSE),
    LongOpt::new("match-length", HasArg::Required, b'm' as u32),
    LongOpt::new("dictionary-size", HasArg::Required, b's' as u32),
    LongOpt::new("volume-size", HasArg::Required, b'S' as u32),
    LongOpt::new("output", HasArg::Required, b'o' as u32),
    LongOpt::new("quiet", HasArg::No, b'q' as u32),
    LongOpt::new("test", HasArg::No, b't' as u32),
    LongOpt::new("verbose", HasArg::No, b'v' as u32),
    LongOpt::new("version", HasArg::No, b'V' as u32),
];

const SHORTS: &str = "0123456789ab:cdfFhklm:o:qs:S:tvV";

const HELP: &str = "Lzip is a lossless data compressor with a user interface similar to the one
of gzip or bzip2. Lzip uses a simplified form of LZMA (Lempel-Ziv-Markov
chain-Algorithm) designed to achieve complete interoperability between
implementations. The maximum dictionary size is 512 MiB so that any lzip
file can be decompressed on 32-bit machines. Lzip provides accurate and
robust 3-factor integrity checking. 'lzip -0' compresses about as fast as
gzip, while 'lzip -9' compresses most files more than bzip2. Decompression
speed is intermediate between gzip and bzip2. Lzip provides better data
recovery capabilities than gzip and bzip2. Lzip has been designed, written,
and tested with great care to replace gzip and bzip2 as general-purpose
compressed format for Unix-like systems.

Usage: lzip [options] [files]

Options:
  -h, --help                     display this help and exit
  -V, --version                  output version information and exit
  -a, --trailing-error           exit with error status if trailing data
  -b, --member-size=<bytes>      set member size limit of multimember files
  -c, --stdout                   write to standard output, keep input files
  -d, --decompress               decompress, test compressed file integrity
  -f, --force                    overwrite existing output files
  -F, --recompress               force re-compression of compressed files
  -k, --keep                     keep (don't delete) input files
  -l, --list                     print (un)compressed file sizes
  -m, --match-length=<bytes>     set match length limit in bytes [36]
  -o, --output=<file>            write to <file>, keep input files
  -q, --quiet                    suppress all messages
  -s, --dictionary-size=<bytes>  set dictionary size limit in bytes [8 MiB]
  -S, --volume-size=<bytes>      set volume size limit in bytes
  -t, --test                     test compressed file integrity
  -v, --verbose                  be verbose (a 2nd -v gives more)
  -0 .. -9                       set compression level [default 6]
      --fast                     alias for -0
      --best                     alias for -9
      --loose-trailing           allow trailing data seeming corrupt header

If no file names are given, or if a file is '-', lzip compresses or
decompresses from standard input to standard output.
Numbers may be followed by a multiplier: k = kB = 10^3 = 1000,
Ki = KiB = 2^10 = 1024, M = 10^6, Mi = 2^20, G = 10^9, Gi = 2^30, etc...
Dictionary sizes 12 to 29 are interpreted as powers of two, meaning 2^12 to
2^29 bytes.

The bidimensional parameter space of LZMA can't be mapped to a linear scale
optimal for all files. If your files are large, very repetitive, etc, you
may need to use the options --dictionary-size and --match-length directly
to achieve optimal performance.

To extract all the files from archive 'foo.tar.lz', use the commands
'tar -xf foo.tar.lz' or 'lzip -cd foo.tar.lz | tar -xf -'.

Exit status: 0 for a normal exit, 1 for environmental problems
(file not found, invalid command-line options, I/O errors, etc), 2 to
indicate a corrupt or invalid input file, 3 for an internal consistency
error (e.g., bug) which caused lzip to panic.

The ideas embodied in lzip are due to (at least) the following people:
Abraham Lempel and Jacob Ziv (for the LZ algorithm), Andrei Markov (for the
definition of Markov chains), G.N.N. Martin (for the definition of range
encoding), Igor Pavlov (for putting all the above together in LZMA), and
Julian Seward (for bzip2's CLI).

Report bugs to lzip-bug@nongnu.org
Lzip home page: http://www.nongnu.org/lzip/lzip.html
";

const VERSION: &str = "lzip 1.25
Copyright (C) 2025 Antonio Diaz Diaz.
License GPLv2+: GNU GPL version 2 or later <http://gnu.org/licenses/gpl.html>
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.
";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Compress,
    Decompress,
    Test,
    List,
}

struct Lzip {
    prog: String,
    mode: Mode,
    level: u32,
    stdout: bool,
    force: bool,
    keep: bool,
    quiet: bool,
    verbose: u32,
    trailing_error: bool,
    loose: bool,
    output: Option<Vec<u8>>,
    dict: Option<u32>,
    exit: i32,
    longest: usize,
    failed_tests: u32,
    out: crate::sysutil::Output,
    // Totais do -l.
    list_count: u32,
    list_in: u64,
    list_out: u64,
    list_header: bool,
}

/// Como terminou a decodificação de um arquivo.
enum Outcome {
    Ok,
    /// Erro de dados: mensagem (sem o nome), código de saída e talvez o dump do que sobrou.
    Bad(String, Option<Vec<u8>>),
    Write(Errno),
    Read(Errno),
}

const MAGIC: &[u8; 4] = b"LZIP";

/// Tamanho do dicionário codificado no byte 5 do cabeçalho.
fn header_dict(b: u8) -> Option<u32> {
    let base = 1u32.checked_shl((b & 0x1f) as u32)?;
    let frac = (b >> 5) as u32;
    let d = base - (base / 16) * frac;
    if !(4096..=(512 << 20)).contains(&d) { None } else { Some(d) }
}

/// O menor tamanho de dicionário representável no cabeçalho que cabe `size` (`2^n - k·2^n/16`).
fn valid_dict_size(size: u32) -> u32 {
    let n = 32 - (size - 1).leading_zeros();
    let base = 1u32 << n;
    (0..8).rev().map(|k| base - (base / 16) * k).find(|&d| d >= size).unwrap_or(base)
}

fn trailing_dump(t: &[u8]) -> String {
    let hex: Vec<String> = t.iter().map(|b| format!("{b:02X}")).collect();
    let text: String = t.iter().map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' }).collect();
    format!("trailing data = {} '{text}'", hex.join(" "))
}

impl Lzip {
    fn error(&mut self, msg: impl AsRef<str>, code: i32) {
        if !self.quiet {
            common::eprint(format!("{}: {}\n", self.prog, msg.as_ref()));
        }
        self.exit = self.exit.max(code);
    }

    fn pad(&self, name: &str) -> String {
        " ".repeat(self.longest.saturating_sub(name.chars().count()))
    }

    fn run(&mut self, args: &[Vec<u8>]) -> i32 {
        self.prog = common::show(common::base_name(args.first().map(Vec::as_slice).unwrap_or(b"lzip")));
        let mut files: Vec<Vec<u8>> = Vec::new();
        for item in Getopt::from_env(args, SHORTS, LONGS) {
            let o = match item {
                Ok(Item::Operand(op)) => {
                    files.push(op);
                    continue;
                }
                Ok(Item::Opt(o)) => o,
                Err(e) => {
                    let msg = match &e {
                        // O analisador do lzip não lista as possibilidades.
                        OptError::Ambiguous { given, .. } => {
                            format!("{}: option '--{}' is ambiguous\n", self.prog, common::show(given))
                        }
                        other => other.message(&self.prog),
                    };
                    common::eprint(format!("{msg}Try '{} --help' for more information.\n", self.prog));
                    return 1;
                }
            };
            let arg = o.arg.clone().unwrap_or_default();
            match o.id {
                c @ 0x30..=0x39 => self.level = c - 0x30,
                0x61 => self.trailing_error = true,
                0x62 | 0x6d | 0x53 => {
                    if parse_num(&arg).is_none() {
                        common::eprint(format!("{}: Bad or missing numerical argument in option '{}'.\n", self.prog, opt_text(&o)));
                        return 1;
                    }
                }
                0x73 => match parse_num(&arg) {
                    Some(n) => {
                        let n = if (12..=29).contains(&n) { 1u64 << n } else { n };
                        self.dict = Some(n.clamp(4096, 512 << 20) as u32);
                    }
                    None => {
                        common::eprint(format!("{}: Bad or missing numerical argument in option '{}'.\n", self.prog, opt_text(&o)));
                        return 1;
                    }
                },
                0x63 => self.stdout = true,
                0x64 => self.mode = Mode::Decompress,
                0x66 => self.force = true,
                0x46 => {}
                0x68 => return print(HELP),
                0x6b => self.keep = true,
                0x6c => self.mode = Mode::List,
                0x6f => self.output = Some(arg),
                0x71 => self.quiet = true,
                0x74 => self.mode = Mode::Test,
                0x76 => {
                    if self.verbose < 4 {
                        self.verbose += 1;
                    }
                }
                0x56 => return print(VERSION),
                OPT_LOOSE => self.loose = true,
                _ => {}
            }
        }
        if files.is_empty() {
            files.push(b"-".to_vec());
        }
        for f in &files {
            let n = if f == b"-" { 7 } else { String::from_utf8_lossy(f).chars().count() };
            self.longest = self.longest.max(n);
        }
        for f in &files {
            self.process(f);
        }
        if self.mode == Mode::List && self.list_count > 1 {
            let mut s = String::new();
            if self.verbose > 0 {
                s.push_str(&" ".repeat(21));
            }
            s.push_str(&format!("{:>14} {:>14} {}  (totals)\n", self.list_in, self.list_out, saved(self.list_out, self.list_in)));
            self.out.write_str(&s);
        }
        self.out.flush();
        if self.mode == Mode::Test && self.verbose > 0 && self.failed_tests > 0 {
            common::eprint(format!(
                "{}: warning: {} {} failed the test.\n",
                self.prog,
                self.failed_tests,
                if self.failed_tests == 1 { "file" } else { "files" }
            ));
        }
        self.exit
    }

    fn process(&mut self, name: &[u8]) {
        let stdin = name == b"-";
        let shown = if stdin { "(stdin)".to_string() } else { common::show(name) };
        // Abre a entrada.
        let (fd, st): (Fd, Option<Stat>) = if stdin {
            (Fd::STDIN, None)
        } else {
            let st = match common::stat(name) {
                Ok(s) => s,
                Err(e) => {
                    self.error(format!("{shown}: Can't open input file: {}", e.message()), 1);
                    return;
                }
            };
            if st.file_type() != sysabi::FileType::Regular {
                self.error(format!("{shown}: Input file is not a regular file."), 1);
                return;
            }
            match common::open_input(name, false) {
                Ok(fd) => (fd, Some(st)),
                Err(e) => {
                    self.error(format!("{shown}: Can't open input file: {}", e.message()), 1);
                    return;
                }
            }
        };
        if self.mode == Mode::List {
            let data = common::read_all(fd).unwrap_or_default();
            if !stdin {
                common::close(fd);
            }
            self.list(&data, &shown);
            return;
        }
        if self.mode == Mode::Compress && !stdin && !self.stdout && self.output.is_none() {
            let base = common::base_name(name);
            for s in [&b".lz"[..], b".tlz"] {
                if base.len() > s.len() && base.ends_with(s) {
                    common::close(fd);
                    self.error(format!("{shown}: Input file already has '{}' suffix, ignored.", common::show(s)), 1);
                    return;
                }
            }
        }
        // Destino.
        let dest: Option<Vec<u8>> = if self.mode == Mode::Test || self.stdout || (stdin && self.output.is_none()) {
            None
        } else if let Some(o) = &self.output {
            if o.as_slice() == b"-" { None } else { Some(o.clone()) }
        } else if self.mode == Mode::Compress {
            Some(common::cat(&[name, b".lz"]))
        } else {
            let base = common::base_name(name);
            let mut d = None;
            for (s, r) in [(&b".lz"[..], &b""[..]), (b".tlz", b".tar")] {
                if base.len() > s.len() && base.ends_with(s) {
                    d = Some(common::cat(&[&name[..name.len() - s.len()], r]));
                }
            }
            Some(d.unwrap_or_else(|| common::cat(&[name, b".out"])))
        };
        let ofd = match &dest {
            Some(d) => {
                if common::lstat(d).is_ok() {
                    if !self.force {
                        common::close(fd);
                        self.error(format!("{}: Output file already exists, skipping.", common::show(d)), 1);
                        return;
                    }
                    let _ = common::unlink(d);
                }
                match common::create_exclusive(d) {
                    Ok(f) => f,
                    Err(e) => {
                        common::close(fd);
                        self.error(format!("{}: Can't create output file: {}", common::show(d), e.message()), 1);
                        return;
                    }
                }
            }
            None => Fd::STDOUT,
        };
        let mut input = Input::new(fd);
        let mut sink = if self.mode == Mode::Test { Sink::null() } else { Sink::fd(ofd) };
        let prefix = format!("  {shown}: {}", self.pad(&shown));
        let result = if self.mode == Mode::Compress {
            if self.verbose > 0 {
                common::eprint(&prefix);
            }
            match self.compress(&mut input, &mut sink) {
                Ok(()) => {
                    if self.verbose > 0 {
                        let (i, o) = (input.consumed, sink.written);
                        if i == 0 {
                            common::eprint(" no data compressed.\n");
                        } else {
                            common::eprint(format!(
                                "{:6.3}:1, {:6.2}% ratio, {:6.2}% saved, {i} in, {o} out.\n",
                                i as f64 / o as f64,
                                100.0 * o as f64 / i as f64,
                                100.0 - 100.0 * o as f64 / i as f64
                            ));
                        }
                    }
                    Outcome::Ok
                }
                Err(e) => Outcome::Write(e),
            }
        } else {
            if self.verbose > 0 {
                common::eprint(&prefix);
            }
            let r = self.decompress(&mut input, &mut sink);
            if matches!(r, Outcome::Ok) && self.verbose > 0 {
                if self.verbose > 1 {
                    let (c, u) = (input.consumed, sink.written);
                    if u > 0 && c > 0 {
                        common::eprint(format!(
                            "{:6.3}:1, {:6.2}% ratio, {:6.2}% saved. ",
                            u as f64 / c as f64,
                            100.0 * c as f64 / u as f64,
                            100.0 - 100.0 * c as f64 / u as f64
                        ));
                    }
                }
                common::eprint(if self.mode == Mode::Test { "ok\n" } else { "done\n" });
            }
            r
        };
        if !stdin {
            common::close(fd);
        }
        match result {
            Outcome::Ok => {
                if let (Some(d), Some(st)) = (&dest, &st) {
                    common::copy_attrs(ofd, d, st, st.mtime);
                    common::close(ofd);
                    if !self.keep && self.output.is_none() {
                        let _ = common::unlink(name);
                    }
                } else if dest.is_some() {
                    common::close(ofd);
                }
            }
            Outcome::Bad(msg, dump) => {
                if !self.quiet {
                    if self.verbose > 0 {
                        common::eprint(format!("{msg}\n"));
                    } else {
                        common::eprint(format!("{prefix}{msg}\n"));
                    }
                    if let Some(t) = dump {
                        common::eprint(format!("{}\n", trailing_dump(&t)));
                    }
                }
                self.exit = self.exit.max(2);
                if self.mode == Mode::Test {
                    self.failed_tests += 1;
                }
                self.cleanup(&dest, ofd);
            }
            Outcome::Write(e) | Outcome::Read(e) => {
                self.error(format!("{shown}: {}", e.message()), 1);
                self.cleanup(&dest, ofd);
            }
        }
    }

    fn cleanup(&mut self, dest: &Option<Vec<u8>>, ofd: Fd) {
        if let Some(d) = dest {
            common::close(ofd);
            if !self.quiet {
                common::eprint(format!("{}: {}: Deleting output file, if it exists.\n", self.prog, common::show(d)));
            }
            let _ = common::unlink(d);
        }
    }

    fn compress(&self, input: &mut Input, sink: &mut Sink) -> Result<(), Errno> {
        let io = |e: std::io::Error| Errno::from_io(&e);
        let mut opts = lzma_rust2::LzipOptions::with_preset(self.level);
        if let Some(d) = self.dict {
            opts.lzma_options.dict_size = d;
        }
        // Entrada regular menor que o limite: o lzip reduz o dicionário ao tamanho dela (mínimo
        // 4 KiB), arredondado pra cima até um tamanho que o cabeçalho representa.
        if let Ok(st) = common::fstat(input.fd)
            && st.file_type() == sysabi::FileType::Regular
            && st.size > 0
            && st.size < u64::from(opts.lzma_options.dict_size)
        {
            opts.lzma_options.dict_size = valid_dict_size(st.size.max(4096) as u32);
        }
        let mut w = lzma_rust2::LzipWriter::new(&mut *sink, opts);
        loop {
            let chunk = input.fill();
            if chunk.is_empty() {
                break;
            }
            let n = chunk.len();
            w.write_all(chunk).map_err(io)?;
            input.consume(n);
        }
        if let Some(e) = input.error {
            return Err(e);
        }
        w.finish().map_err(io)?;
        Ok(())
    }

    /// Decodifica membros até o fim, com as mensagens do lzip.
    fn decompress(&self, input: &mut Input, sink: &mut Sink) -> Outcome {
        let mut out = vec![0u8; common::CHUNK];
        let mut members = 0u32;
        // Bytes já lidos que voltam pra frente da entrada (o decodificador LZMA lê um pouco além).
        let mut carry: Vec<u8> = Vec::new();
        let mut pos: u64 = 0;
        loop {
            // Cabeçalho (ou o fim).
            let mut head = std::mem::take(&mut carry);
            while head.len() < 6 {
                let a = input.fill();
                if a.is_empty() {
                    break;
                }
                let take = (6 - head.len()).min(a.len());
                head.extend_from_slice(&a[..take]);
                input.consume(take);
            }
            if let Some(e) = input.error {
                return Outcome::Read(e);
            }
            if members > 0 {
                if head.is_empty() {
                    return Outcome::Ok;
                }
                let is_prefix = head.len() < 4 && MAGIC.starts_with(&head);
                let full_magic = head.len() >= 4 && &head[..4] == MAGIC;
                if is_prefix || (full_magic && head.len() < 6) {
                    let mut rest = head.clone();
                    input.drain_check_zeros();
                    rest.truncate(64);
                    return Outcome::Bad("Truncated header in multimember file.".into(), Some(rest));
                }
                if !full_magic {
                    let mut rest = head.clone();
                    while rest.len() < 64 {
                        let a = input.fill();
                        if a.is_empty() {
                            break;
                        }
                        let take = (64 - rest.len()).min(a.len());
                        rest.extend_from_slice(&a[..take]);
                        input.consume(take);
                    }
                    input.drain_check_zeros();
                    if self.trailing_error {
                        return Outcome::Bad("Trailing data not allowed.".into(), Some(rest));
                    }
                    if self.verbose > 3 {
                        common::eprint(format!("{}\n", trailing_dump(&rest)));
                    }
                    return Outcome::Ok;
                }
            } else {
                if head.len() < 4 || &head[..4] != MAGIC {
                    if head.len() < 4 && !head.is_empty() && MAGIC.starts_with(&head) {
                        return Outcome::Bad(format!("File ends unexpectedly at pos {}", head.len()), None);
                    }
                    if head.is_empty() {
                        return Outcome::Bad("File ends unexpectedly at pos 0".into(), None);
                    }
                    return Outcome::Bad("Bad magic number (file not in lzip format).".into(), None);
                }
                if head.len() < 6 {
                    return Outcome::Bad(format!("File ends unexpectedly at pos {}", head.len()), None);
                }
            }
            if head[4] != 1 {
                return Outcome::Bad(format!("Version {} member format not supported.", head[4]), None);
            }
            let Some(dict) = header_dict(head[5]) else {
                return Outcome::Bad("Invalid dictionary size in member header.".into(), None);
            };
            let member_start = pos;
            pos += 6;
            let mut lz = match lzma_rust2::LzmaStream::new(u64::MAX, 3, 0, 2, dict, None) {
                Ok(s) => s,
                Err(_) => return Outcome::Bad("Invalid dictionary size in member header.".into(), None),
            };
            let mut crc = crc32fast::Hasher::new();
            let mut size: u64 = 0;
            let mut fed: u64 = 0;
            loop {
                sysabi::sys::checkpoint();
                let empty = input.fill().is_empty();
                if empty && let Some(e) = input.error {
                    return Outcome::Read(e);
                }
                let action = if empty { lzma_rust2::Action::Finish } else { lzma_rust2::Action::Run };
                let r = lz.process(input.available(), &mut out, action);
                match r {
                    Ok(res) => {
                        input.consume(res.bytes_consumed);
                        fed += res.bytes_consumed as u64;
                        if res.bytes_produced > 0 {
                            crc.update(&out[..res.bytes_produced]);
                            size += res.bytes_produced as u64;
                            if let Err(e) = sink.write_all(&out[..res.bytes_produced]) {
                                return Outcome::Write(Errno::from_io(&e));
                            }
                        }
                        if res.status == lzma_rust2::Status::StreamEnd {
                            break;
                        }
                        if empty && res.bytes_produced == 0 {
                            return Outcome::Bad(format!("File ends unexpectedly at pos {}", pos + fed), None);
                        }
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::UnexpectedEof {
                            return Outcome::Bad(format!("File ends unexpectedly at pos {}", pos + fed), None);
                        }
                        return Outcome::Bad(format!("Decoder error at pos {}", pos + fed), None);
                    }
                }
            }
            let unused = lz.unused_input().to_vec();
            let data_len = fed - unused.len() as u64;
            pos += data_len;
            // Rodapé.
            let mut trailer = unused;
            while trailer.len() < 20 {
                let a = input.fill();
                if a.is_empty() {
                    break;
                }
                let take = (20 - trailer.len()).min(a.len());
                trailer.extend_from_slice(&a[..take]);
                input.consume(take);
            }
            if trailer.len() < 20 {
                return Outcome::Bad(format!("File ends unexpectedly at pos {}", pos + trailer.len() as u64), None);
            }
            carry = trailer.split_off(20);
            pos += 20;
            let stored_crc = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
            let stored_size = u64::from_le_bytes(trailer[4..12].try_into().unwrap_or([0; 8]));
            let stored_member = u64::from_le_bytes(trailer[12..20].try_into().unwrap_or([0; 8]));
            let computed = crc.finalize();
            if stored_crc != computed {
                return Outcome::Bad(format!("CRC mismatch; stored {stored_crc:08X}, computed {computed:08X}"), None);
            }
            if stored_size != size {
                return Outcome::Bad(format!("Data size mismatch; stored {stored_size} (0x{stored_size:X}), computed {size} (0x{size:X})"), None);
            }
            let member = pos - member_start;
            if stored_member != member {
                return Outcome::Bad(format!("Member size mismatch; stored {stored_member} (0x{stored_member:X}), computed {member} (0x{member:X})"), None);
            }
            members += 1;
        }
    }

    /// `lzip -l` de um arquivo inteiro na memória.
    fn list(&mut self, data: &[u8], shown: &str) {
        let Some((members, trail)) = index(data) else {
            if data.len() < 4 || &data[..4] != MAGIC {
                self.error(format!("{shown}: Bad magic number (file not in lzip format)."), 2);
            } else {
                self.error(format!("{shown}: Can't create file index."), 2);
            }
            return;
        };
        let uncomp: u64 = members.iter().map(|m| m.1).sum();
        let comp = data.len() as u64;
        let dict = members.iter().map(|m| m.0).max().unwrap_or(0);
        let mut s = String::new();
        if !self.list_header {
            self.list_header = true;
            if self.verbose > 0 {
                s.push_str("   dict   memb  trail   uncompressed     compressed   saved  name\n");
            } else {
                s.push_str("  uncompressed     compressed   saved  name\n");
            }
        }
        if self.verbose > 0 {
            s.push_str(&format!("{:>8}{:>6}{:>7}", format_ds(dict), members.len(), trail));
        }
        s.push_str(&format!("{:>14} {:>14} {}  {shown}\n", uncomp, comp, saved(comp, uncomp)));
        self.out.write_str(&s);
        self.list_count += 1;
        self.list_in += uncomp;
        self.list_out += comp;
    }
}

/// Índice dos membros, do fim pro começo: (dicionário, tamanho dos dados) de cada membro e quantos
/// bytes sobram depois do último.
fn index(data: &[u8]) -> Option<(Vec<(u32, u64)>, u64)> {
    if data.len() < 36 || &data[..4] != MAGIC {
        return None;
    }
    let valid_end = |end: usize| -> Option<Vec<(u32, u64)>> {
        let mut members = Vec::new();
        let mut e = end;
        while e > 0 {
            if e < 26 {
                return None;
            }
            let t = &data[e - 20..e];
            let member = u64::from_le_bytes(t[12..20].try_into().ok()?);
            let dsize = u64::from_le_bytes(t[4..12].try_into().ok()?);
            if member < 26 || member > e as u64 {
                return None;
            }
            let start = e - member as usize;
            let h = &data[start..start + 6];
            if &h[..4] != MAGIC || h[4] != 1 {
                return None;
            }
            members.push((header_dict(h[5])?, dsize));
            e = start;
        }
        members.reverse();
        Some(members)
    };
    let mut end = data.len();
    loop {
        if let Some(m) = valid_end(end) {
            return Some((m, (data.len() - end) as u64));
        }
        if end <= 36 {
            return None;
        }
        end -= 1;
    }
}

/// Dicionário como o lzip mostra: "N KiB" ou "N MiB".
fn format_ds(d: u32) -> String {
    if d % (1 << 20) == 0 { format!("{} MiB", d >> 20) } else { format!("{} KiB", d >> 10) }
}

/// Coluna "saved" do `-l`.
fn saved(comp: u64, uncomp: u64) -> String {
    if uncomp == 0 {
        return "  -INF%".into();
    }
    format!("{:6.2}%", 100.0 - 100.0 * comp as f64 / uncomp as f64)
}

/// Número com os multiplicadores do lzip (k, Ki, M, Mi...).
fn parse_num(s: &[u8]) -> Option<u64> {
    let t = std::str::from_utf8(s).ok()?;
    let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
    let n: u64 = digits.parse().ok()?;
    let rest = &t[digits.len()..];
    let mult: u64 = match rest {
        "" => 1,
        "k" | "kB" => 1000,
        "Ki" | "KiB" => 1024,
        "M" | "MB" => 1_000_000,
        "Mi" | "MiB" => 1 << 20,
        "G" | "GB" => 1_000_000_000,
        "Gi" | "GiB" => 1 << 30,
        _ => return None,
    };
    n.checked_mul(mult)
}

fn opt_text(o: &crate::getopt::Opt) -> String {
    match o.long {
        Some(l) => format!("--{l}"),
        None => format!("-{}", char::from_u32(o.id).unwrap_or('?')),
    }
}

fn print(s: &str) -> i32 {
    match sysabi::sys::write_all(Fd::STDOUT, s.as_bytes()) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

/// Entrada do `lzip`.
pub fn main(args: &[Vec<u8>]) -> i32 {
    let mut l = Lzip {
        prog: String::new(),
        mode: Mode::Compress,
        level: 6,
        stdout: false,
        force: false,
        keep: false,
        quiet: false,
        verbose: 0,
        trailing_error: false,
        loose: false,
        output: None,
        dict: None,
        exit: 0,
        longest: 0,
        failed_tests: 0,
        out: crate::sysutil::Output::stdout(),
        list_count: 0,
        list_in: 0,
        list_out: 0,
        list_header: false,
    };
    l.run(args)
}
