//! `gzip` 1.13 (e, por cima dele, os scripts `gunzip` e `zcat` do Debian).
//!
//! Escrito a partir do manual do gzip, da RFC 1952 e do comportamento observado no oráculo: opções,
//! nomes de saída, sufixos conhecidos, cabeçalho (nome, mtime, XFL, SO 3), membros concatenados,
//! sobras no fim, `-l` (que descomprime tudo pra validar, como o gzip), `-v`, avisos com código 2,
//! erros com 1, e os erros que abortam a execução inteira (fim inesperado e dado deflate inválido).

use std::io::Write;

use sysabi::{Errno, Fd, Stat, TimeSpec};

use super::common::{self, Input, Sink};
use crate::codec::{self, GzipHeader, GzipHeaderInfo};
use crate::gailly::gzip::GzDeflate;
use crate::getopt::{Getopt, HasArg, Item, LongOpt};
use crate::sysutil::Output;

const RSYNCABLE: u32 = 0x100;
const SYNCHRONOUS: u32 = 0x101;
const PRESUME_TTY: u32 = 0x102;

/// Tabela de opções longas na ordem do gzip 1.13 (a ordem aparece nas mensagens de ambiguidade).
const LONGS: &[LongOpt] = &[
    LongOpt::new("ascii", HasArg::No, b'a' as u32),
    LongOpt::new("to-stdout", HasArg::No, b'c' as u32),
    LongOpt::new("stdout", HasArg::No, b'c' as u32),
    LongOpt::new("decompress", HasArg::No, b'd' as u32),
    LongOpt::new("uncompress", HasArg::No, b'd' as u32),
    LongOpt::new("force", HasArg::No, b'f' as u32),
    LongOpt::new("help", HasArg::No, b'h' as u32),
    LongOpt::new("keep", HasArg::No, b'k' as u32),
    LongOpt::new("list", HasArg::No, b'l' as u32),
    LongOpt::new("license", HasArg::No, b'L' as u32),
    LongOpt::new("no-name", HasArg::No, b'n' as u32),
    LongOpt::new("name", HasArg::No, b'N' as u32),
    LongOpt::new("-presume-input-tty", HasArg::No, PRESUME_TTY),
    LongOpt::new("quiet", HasArg::No, b'q' as u32),
    LongOpt::new("silent", HasArg::No, b'q' as u32),
    LongOpt::new("synchronous", HasArg::No, SYNCHRONOUS),
    LongOpt::new("recursive", HasArg::No, b'r' as u32),
    LongOpt::new("suffix", HasArg::Required, b'S' as u32),
    LongOpt::new("test", HasArg::No, b't' as u32),
    LongOpt::new("verbose", HasArg::No, b'v' as u32),
    LongOpt::new("version", HasArg::No, b'V' as u32),
    LongOpt::new("fast", HasArg::No, b'1' as u32),
    LongOpt::new("best", HasArg::No, b'9' as u32),
    LongOpt::new("lzw", HasArg::No, b'Z' as u32),
    LongOpt::new("bits", HasArg::Required, b'b' as u32),
    LongOpt::new("rsyncable", HasArg::No, RSYNCABLE),
];

const SHORTS: &str = "-123456789ab:cdfhHklLmMnNqrS:tvVZ";

const HELP: &str = "Usage: gzip [OPTION]... [FILE]...
Compress or uncompress FILEs (by default, compress FILES in-place).

Mandatory arguments to long options are mandatory for short options too.

  -c, --stdout      write on standard output, keep original files unchanged
  -d, --decompress  decompress
  -f, --force       force overwrite of output file and compress links
  -h, --help        give this help
  -k, --keep        keep (don't delete) input files
  -l, --list        list compressed file contents
  -L, --license     display software license
  -n, --no-name     do not save or restore the original name and timestamp
  -N, --name        save or restore the original name and timestamp
  -q, --quiet       suppress all warnings
  -r, --recursive   operate recursively on directories
      --rsyncable   make rsync-friendly archive
  -S, --suffix=SUF  use suffix SUF on compressed files
      --synchronous synchronous output (safer if system crashes, but slower)
  -t, --test        test compressed file integrity
  -v, --verbose     verbose mode
  -V, --version     display version number
  -1, --fast        compress faster
  -9, --best        compress better

With no FILE, or when FILE is -, read standard input.

Report bugs to <bug-gzip@gnu.org>.
";

const VERSION: &str = "gzip 1.13
Copyright (C) 2023 Free Software Foundation, Inc.
Copyright (C) 1993 Jean-loup Gailly.
This is free software.  You may redistribute copies of it under the terms of
the GNU General Public License <https://www.gnu.org/licenses/gpl.html>.
There is NO WARRANTY, to the extent permitted by law.

Written by Jean-loup Gailly.
";

const LICENSE: &str = "gzip 1.13
Copyright (C) 2023 Free Software Foundation, Inc.
Copyright (C) 1993 Jean-loup Gailly.
This is free software.  You may redistribute copies of it under the terms of
the GNU General Public License <https://www.gnu.org/licenses/gpl.html>.
There is NO WARRANTY, to the extent permitted by law.
";

const OK: i32 = 0;
const ERROR: i32 = 1;
const WARNING: i32 = 2;

/// Como terminou a descompressão de uma entrada.
enum Status {
    Ok,
    /// Erro que só encerra este arquivo (mensagem já formatada).
    Bad(String),
    /// Erro que aborta a execução inteira (fim inesperado, deflate inválido).
    Fatal(String),
    /// Erro de escrita na saída.
    WriteErr(Errno),
}

struct Unzipped {
    status: Status,
    /// Havia zeros ou lixo depois do último membro.
    trailing: bool,
    /// Os oito bytes do rodapé do último membro.
    trailer: [u8; 8],
    /// Bytes descomprimidos do último membro (o gzip conta assim na razão do `-v`).
    last_out: u64,
    first: Option<GzipHeaderInfo>,
}

/// Saída pedida por `exit` no meio do processamento (erro fatal).
struct Abort;

struct Gzip {
    prog: String,
    level: u32,
    to_stdout: bool,
    decompress: bool,
    force: u32,
    keep: bool,
    list: bool,
    /// `None` = padrão do modo (comprimindo guarda nome e data; descomprimindo não restaura).
    no_name: Option<bool>,
    no_time: Option<bool>,
    quiet: bool,
    verbose: i32,
    recursive: bool,
    rsync: bool,
    /// O estado do deflate entre os arquivos (a janela do C é global e os bytes velhos influenciam
    /// as correspondências no fim do arquivo seguinte).
    deflate: Option<Box<GzDeflate>>,
    suffix: Vec<u8>,
    test: bool,
    presume_tty: bool,
    exit: i32,
    file_count: usize,
    // Estado do -l.
    out: Output,
    list_first: bool,
    list_any: bool,
    total_in: u64,
    total_out: u64,
    header_bytes: u64,
    tz: Option<jiff::tz::TimeZone>,
}

impl Gzip {
    fn new() -> Gzip {
        Gzip {
            prog: "gzip".into(),
            level: 6,
            to_stdout: false,
            decompress: false,
            force: 0,
            keep: false,
            list: false,
            no_name: None,
            no_time: None,
            quiet: false,
            verbose: 0,
            recursive: false,
            rsync: false,
            deflate: None,
            suffix: b".gz".to_vec(),
            test: false,
            presume_tty: false,
            exit: OK,
            file_count: 0,
            out: Output::stdout(),
            list_first: true,
            list_any: false,
            total_in: 0,
            total_out: 0,
            header_bytes: 0,
            tz: None,
        }
    }

    fn no_name(&self) -> bool {
        self.no_name.unwrap_or(self.decompress)
    }

    fn no_time(&self) -> bool {
        self.no_time.unwrap_or(self.decompress)
    }

    fn error(&mut self, msg: impl AsRef<str>) {
        common::eprint(format!("{}: {}\n", self.prog, msg.as_ref()));
        self.exit = ERROR;
    }

    fn warn(&mut self, msg: impl AsRef<str>) {
        if !self.quiet {
            common::eprint(format!("{}: {}\n", self.prog, msg.as_ref()));
        }
        if self.exit == OK {
            self.exit = WARNING;
        }
    }

    /// Aviso de dados (o gzip começa a linha com `\n`).
    fn warn_nl(&mut self, name: &str, what: &str) {
        if !self.quiet {
            common::eprint(format!("\n{}: {name}: {what}\n", self.prog));
        }
        if self.exit == OK {
            self.exit = WARNING;
        }
    }

    fn try_help(&self) -> i32 {
        common::eprint(format!("Try `{} --help' for more information.\n", self.prog));
        ERROR
    }

    fn known_suffixes(&self) -> Vec<Vec<u8>> {
        let mut v = vec![self.suffix.clone()];
        for s in [&b".gz"[..], b".z", b".taz", b".tgz", b"-gz", b"-z", b"_z"] {
            v.push(s.to_vec());
        }
        v
    }

    /// Sufixo conhecido no fim do nome.
    fn has_suffix(&self, name: &[u8]) -> Option<Vec<u8>> {
        let base = common::base_name(name);
        self.known_suffixes().into_iter().find(|s| !s.is_empty() && base.len() > s.len() && base.ends_with(s))
    }

    fn run(&mut self, args: &[Vec<u8>]) -> i32 {
        self.prog = String::from_utf8_lossy(common::base_name(args.first().map(Vec::as_slice).unwrap_or(b"gzip")))
            .into_owned();
        let mut files: Vec<Vec<u8>> = Vec::new();
        for item in Getopt::from_env(args, SHORTS, LONGS) {
            let o = match item {
                Ok(Item::Operand(op)) => {
                    files.push(op);
                    continue;
                }
                Ok(Item::Opt(o)) => o,
                Err(e) => {
                    common::eprint(e.message_bytes(&self.prog));
                    return self.try_help();
                }
            };
            match o.id {
                c @ 0x31..=0x39 => self.level = c - 0x30,
                0x61 => common::eprint(format!("{}: option --ascii ignored on this system\n", self.prog)),
                0x62 => {
                    if common::parse_u64(&o.arg.unwrap_or_default()).is_none() {
                        common::eprint(format!("{}: -b operand is not an integer\n", self.prog));
                        return self.try_help();
                    }
                }
                0x63 => self.to_stdout = true,
                0x64 => self.decompress = true,
                0x66 => self.force += 1,
                0x68 | 0x48 => return out_text(HELP),
                0x6b => self.keep = true,
                0x6c => {
                    self.list = true;
                    self.decompress = true;
                    self.to_stdout = true;
                }
                0x4c => return out_text(LICENSE),
                0x6d => self.no_time = Some(true),
                0x4d => self.no_time = Some(false),
                0x6e => {
                    self.no_name = Some(true);
                    self.no_time = Some(true);
                }
                0x4e => {
                    self.no_name = Some(false);
                    self.no_time = Some(false);
                }
                0x71 => {
                    self.quiet = true;
                    self.verbose = 0;
                }
                0x72 => self.recursive = true,
                RSYNCABLE => self.rsync = true,
                0x53 => {
                    let s = o.arg.unwrap_or_default();
                    if s.is_empty() || s.len() > 30 {
                        let msg = if s.is_empty() { "invalid suffix ''".to_string() } else { "suffix too long".to_string() };
                        common::eprint(format!("{}: {msg}\n", self.prog));
                        return ERROR;
                    }
                    self.suffix = s;
                }
                0x74 => {
                    self.test = true;
                    self.decompress = true;
                    self.to_stdout = true;
                }
                0x76 => {
                    self.verbose += 1;
                    self.quiet = false;
                }
                0x56 => return out_text(VERSION),
                0x5a => {
                    common::eprint(format!("{}: -Z not supported in this version\n", self.prog));
                    return self.try_help();
                }
                PRESUME_TTY => self.presume_tty = true,
                _ => {}
            }
        }
        self.file_count = files.len();
        let r = if files.is_empty() {
            self.treat_stdin()
        } else {
            files.iter().try_for_each(|f| if f == b"-" { self.treat_stdin() } else { self.treat_file(f, true) })
        };
        if r.is_err() {
            self.out.flush();
            return ERROR;
        }
        if self.list && !self.quiet && self.file_count > 1 && self.list_any {
            self.list_totals();
        }
        if self.out.finish().is_err() && self.exit == OK {
            self.exit = ERROR;
        }
        self.exit
    }

    fn treat_stdin(&mut self) -> Result<(), Abort> {
        let tty_check = if self.decompress { common::isatty(Fd::STDIN) } else { common::isatty(Fd::STDOUT) };
        if self.force == 0 && !self.list && (self.presume_tty || tty_check) {
            let (what, verb) = if self.decompress { ("read from", "decompression") } else { ("written to", "compression") };
            common::eprint(format!(
                "{}: compressed data not {what} a terminal. Use -f to force {verb}.\nFor help, type: {} -h\n",
                self.prog, self.prog
            ));
            self.exit = ERROR;
            return Ok(());
        }
        let st = common::fstat(Fd::STDIN).ok();
        let regular = st.as_ref().is_some_and(|s| s.file_type() == sysabi::FileType::Regular);
        let mut input = Input::new(Fd::STDIN);
        if self.list {
            let mtime = st.as_ref().map(|s| s.mtime.sec).unwrap_or(0);
            return self.list_input(&mut input, "stdin", b"stdout", mtime);
        }
        let mut sink = if self.test { Sink::null() } else { Sink::fd(Fd::STDOUT) };
        if self.decompress {
            let prefix = if self.test && self.verbose > 0 { Some(String::new()) } else { None };
            let u = self.unzip(&mut input, &mut sink, "stdin", prefix);
            match u.status {
                Status::Ok => {
                    if self.verbose > 0 && self.test {
                        common::eprint(" OK\n");
                    }
                }
                Status::Bad(msg) => common::eprint(msg),
                Status::Fatal(msg) => {
                    common::eprint(msg);
                    return Err(Abort);
                }
                Status::WriteErr(e) => self.error(format!("stdout: {}", e.message())),
            }
        } else {
            let mtime = match (&st, self.no_time()) {
                (Some(s), false) if regular => s.mtime.sec.clamp(0, u32::MAX as i64) as u32,
                _ => 0,
            };
            let header = GzipHeader { mtime, name: None };
            match self.zip(&mut input, &mut sink, &header) {
                Ok(hlen) => {
                    if self.verbose > 0 {
                        let r = ratio(input.consumed as i64 - (sink.written as i64 - hlen as i64), input.consumed);
                        common::eprint(format!("{r}\n"));
                    }
                }
                Err(e) => self.error(format!("stdout: {}", e.message())),
            }
        }
        Ok(())
    }

    /// Comprime `input` em `sink` com o cabeçalho dado. Devolve o tamanho de cabeçalho mais rodapé.
    fn zip(&mut self, input: &mut Input, sink: &mut Sink, header: &GzipHeader) -> Result<u64, Errno> {
        let hlen = codec::gzip_header_bytes(header, self.level).len() as u64 + 8;
        let state = self.deflate.take().unwrap_or_default();
        let mut enc = codec::Encoder::gzip_with_state(self.level, self.rsync, header, state, &mut *sink)
            .map_err(|e| Errno::from_io(&e))?;
        loop {
            let chunk = input.fill();
            if chunk.is_empty() {
                break;
            }
            let n = chunk.len();
            enc.write_all(chunk).map_err(|e| Errno::from_io(&e))?;
            input.consume(n);
        }
        if let Some(e) = input.error {
            return Err(e);
        }
        let (_, state) = enc.finish_gzip().map_err(|e| Errno::from_io(&e))?;
        self.deflate = Some(state);
        Ok(hlen)
    }

    /// Descomprime os membros gzip de `input` em `sink`, com as mensagens e a contabilidade do gzip.
    /// `prefix`, quando há, é o "nome:\t" do `-v`, impresso depois que o primeiro cabeçalho é lido.
    fn unzip(&mut self, input: &mut Input, sink: &mut Sink, name: &str, prefix: Option<String>) -> Unzipped {
        let mut u = Unzipped { status: Status::Ok, trailing: false, trailer: [0; 8], last_out: 0, first: None };
        let mut members = 0usize;
        let mut chunk = vec![0u8; common::CHUNK];
        let mut prefix = prefix;
        loop {
            sysabi::sys::checkpoint();
            if members > 0 {
                let peek = input.ensure(2);
                if peek.is_empty() {
                    return u;
                }
                self.header_bytes = 0;
                if peek.len() >= 2 && (peek[0] != 0x1f || peek[1] != 0x8b) {
                    let (zeros, _) = input.drain_check_zeros();
                    u.trailing = true;
                    if zeros {
                        if self.verbose > 0 {
                            self.warn_nl(name, "decompression OK, trailing zero bytes ignored");
                        }
                    } else {
                        self.warn_nl(name, "decompression OK, trailing garbage ignored");
                    }
                    return u;
                }
            }
            // Cabeçalho: pode ter nome e comentário longos; lê até achar o fim dele.
            let mut want = 10usize;
            let parsed = loop {
                let avail = input.ensure(want);
                if avail.len() < 2 {
                    break Err(codec::DecodeError::Truncated);
                }
                match codec::gzip_header(avail) {
                    Err(codec::DecodeError::Truncated) if avail.len() >= want => want = avail.len() + 4096,
                    r => break r,
                }
            };
            let h = match parsed {
                Ok(h) => h,
                Err(codec::DecodeError::NotFormat) => {
                    u.status = Status::Bad(self.bad_nl(name, "not in gzip format"));
                    return u;
                }
                Err(codec::DecodeError::Truncated) => {
                    u.status = Status::Fatal(self.bad_nl(name, "unexpected end of file"));
                    return u;
                }
                Err(codec::DecodeError::Corrupt(m)) => {
                    let method = m.trim_start_matches("unknown method ").to_string();
                    self.exit = ERROR;
                    u.status = Status::Bad(format!("{}: {name}: unknown method {method} -- not supported\n", self.prog));
                    return u;
                }
                Err(_) => {
                    u.status = Status::Fatal(self.bad_nl(name, "invalid compressed data--format violated"));
                    return u;
                }
            };
            if h.flags & 0x20 != 0 {
                self.exit = ERROR;
                u.status = Status::Bad(format!("{}: {name} is encrypted -- not supported\n", self.prog));
                return u;
            }
            if h.flags & 0xc0 != 0 {
                self.exit = ERROR;
                u.status = Status::Bad(format!("{}: {name} has flags 0x{:x} -- not supported\n", self.prog, h.flags));
                return u;
            }
            if h.flags & 0x02 != 0 {
                let raw = input.ensure(h.len).to_vec();
                let stored = u16::from_le_bytes([raw[h.len - 2], raw[h.len - 1]]);
                let computed = (crc32fast::hash(&raw[..h.len - 2]) & 0xffff) as u16;
                if stored != computed {
                    self.exit = ERROR;
                    u.status = Status::Bad(format!(
                        "{}: {name}: header checksum 0x{stored:04x} != computed checksum 0x{computed:04x}\n",
                        self.prog
                    ));
                    return u;
                }
            }
            input.consume(h.len);
            if members == 0 {
                self.header_bytes = h.len as u64 + 8;
                u.first = Some(h.clone());
                if let Some(p) = prefix.take()
                    && !p.is_empty()
                {
                    common::eprint(p);
                }
            }
            let mut inf = flate2::Decompress::new(false);
            let mut crc = crc32fast::Hasher::new();
            let mut size: u64 = 0;
            loop {
                sysabi::sys::checkpoint();
                let avail = input.fill();
                if avail.is_empty() {
                    if let Some(e) = input.error {
                        self.exit = ERROR;
                        u.status = Status::Bad(format!("{}: {name}: {}\n", self.prog, e.message()));
                    } else {
                        u.status = Status::Fatal(self.bad_nl(name, "unexpected end of file"));
                    }
                    return u;
                }
                let (in0, out0) = (inf.total_in(), inf.total_out());
                let st = inf.decompress(avail, &mut chunk, flate2::FlushDecompress::None);
                let consumed = (inf.total_in() - in0) as usize;
                let produced = (inf.total_out() - out0) as usize;
                input.consume(consumed);
                if produced > 0 {
                    crc.update(&chunk[..produced]);
                    size += produced as u64;
                    if let Err(e) = sink.write_all(&chunk[..produced]) {
                        u.status = Status::WriteErr(Errno::from_io(&e));
                        return u;
                    }
                }
                match st {
                    Ok(flate2::Status::StreamEnd) => break,
                    Ok(_) => {}
                    Err(_) => {
                        u.status = Status::Fatal(self.bad_nl(name, "invalid compressed data--format violated"));
                        return u;
                    }
                }
            }
            let t = input.ensure(8).to_vec();
            if t.len() < 8 {
                u.status = Status::Fatal(self.bad_nl(name, "unexpected end of file"));
                return u;
            }
            input.consume(8);
            u.trailer.copy_from_slice(&t[..8]);
            let stored_crc = u32::from_le_bytes([t[0], t[1], t[2], t[3]]);
            let stored_len = u32::from_le_bytes([t[4], t[5], t[6], t[7]]);
            u.last_out = size;
            if stored_crc != crc.finalize() {
                u.status = Status::Bad(self.bad_nl(name, "invalid compressed data--crc error"));
                return u;
            }
            if stored_len != size as u32 {
                u.status = Status::Bad(self.bad_nl(name, "invalid compressed data--length error"));
                return u;
            }
            members += 1;
        }
    }

    /// Mensagem de erro de dados (com `\n` antes) e código 1; devolve o texto pra imprimir.
    fn bad_nl(&mut self, name: &str, what: &str) -> String {
        self.exit = ERROR;
        format!("\n{}: {name}: {what}\n", self.prog)
    }

    /// Resolve o caminho de entrada (com os sufixos, na descompressão) e o `stat` dele.
    fn locate(&mut self, path: &[u8]) -> Option<(Vec<u8>, Stat)> {
        match common::lstat(path) {
            Ok(s) => Some((path.to_vec(), s)),
            Err(Errno::ENOENT) if self.decompress && self.has_suffix(path).is_none() => {
                for s in self.known_suffixes() {
                    let cand = common::cat(&[path, &s]);
                    if let Ok(st) = common::lstat(&cand) {
                        return Some((cand, st));
                    }
                }
                let first = common::cat(&[path, &self.suffix]);
                self.error(format!("{}: {}", common::show(&first), Errno::ENOENT.message()));
                None
            }
            Err(e) => {
                self.error(format!("{}: {}", common::show(path), e.message()));
                None
            }
        }
    }

    fn treat_file(&mut self, arg: &[u8], top: bool) -> Result<(), Abort> {
        let follow = self.to_stdout || self.force > 0;
        let Some((path, st)) = self.locate(arg) else { return Ok(()) };
        let shown = common::show(&path);
        let st = if st.file_type() == sysabi::FileType::Symlink {
            if !follow {
                self.error(format!("{shown}: {}", Errno::ELOOP.message()));
                return Ok(());
            }
            match common::stat(&path) {
                Ok(s) => s,
                Err(e) => {
                    self.error(format!("{shown}: {}", e.message()));
                    return Ok(());
                }
            }
        } else {
            st
        };
        if st.file_type() == sysabi::FileType::Directory {
            if self.recursive {
                return self.treat_dir(&path);
            }
            self.warn(format!("{shown} is a directory -- ignored"));
            return Ok(());
        }
        if !self.to_stdout {
            if st.file_type() != sysabi::FileType::Regular {
                self.warn(format!("{shown} is not a directory or a regular file - ignored"));
                return Ok(());
            }
            if st.mode & common::S_ISUID != 0 {
                self.warn(format!("{shown} is set-user-ID on execution - ignored"));
                return Ok(());
            }
            if st.mode & common::S_ISGID != 0 {
                self.warn(format!("{shown} is set-group-ID on execution - ignored"));
                return Ok(());
            }
            if self.force == 0 {
                if st.mode & common::S_ISVTX != 0 {
                    self.warn(format!("{shown} has the sticky bit set - file ignored"));
                    return Ok(());
                }
                if st.nlink > 1 {
                    let others = st.nlink - 1;
                    self.warn(format!("{shown} has {others} other link{} -- file ignored", if others == 1 { "" } else { "s" }));
                    return Ok(());
                }
            }
        }
        // Nome de saída.
        let out_name: Option<Vec<u8>> = if self.to_stdout {
            None
        } else if self.decompress {
            match self.has_suffix(&path) {
                Some(s) => {
                    let mut n = path[..path.len() - s.len()].to_vec();
                    if s == b".tgz" || s == b".taz" {
                        n.extend_from_slice(b".tar");
                    }
                    Some(n)
                }
                None => {
                    if !(self.recursive && !top) {
                        self.warn(format!("{shown}: unknown suffix -- ignored"));
                    }
                    return Ok(());
                }
            }
        } else {
            if let Some(s) = self.has_suffix(&path) {
                if !(self.recursive && !top) && !self.quiet {
                    common::eprint(format!("{}: {shown} already has {} suffix -- unchanged\n", self.prog, common::show(&s)));
                }
                return Ok(());
            }
            Some(common::cat(&[&path, &self.suffix]))
        };
        let fd = match common::open_input(&path, !follow) {
            Ok(fd) => fd,
            Err(e) => {
                self.error(format!("{shown}: {}", e.message()));
                return Ok(());
            }
        };
        let mut input = Input::new(fd);
        if self.list {
            let name = match self.has_suffix(&path) {
                Some(s) => path[..path.len() - s.len()].to_vec(),
                None => path.clone(),
            };
            let r = self.list_input(&mut input, &shown, &name, st.mtime.sec);
            common::close(fd);
            return r;
        }
        let prefix = (self.verbose > 0).then(|| format!("{shown}:\t"));
        let verb = if self.keep { "created" } else { "replaced with" };
        let Some(mut out) = out_name else {
            // Pra stdout (ou -t).
            let mut sink = if self.test { Sink::null() } else { Sink::fd(Fd::STDOUT) };
            self.header_bytes = 0;
            if self.decompress {
                let u = self.unzip(&mut input, &mut sink, &shown, prefix);
                common::close(fd);
                match u.status {
                    Status::Ok => {
                        if self.verbose > 0 {
                            if self.test {
                                common::eprint(" OK\n");
                            } else {
                                let r = ratio(u.last_out as i64 - (input.consumed as i64 - self.header_bytes as i64), u.last_out);
                                common::eprint(format!("{r} -- {verb} stdout\n"));
                            }
                        }
                    }
                    Status::Bad(m) => common::eprint(m),
                    Status::Fatal(m) => {
                        common::eprint(m);
                        return Err(Abort);
                    }
                    Status::WriteErr(e) => self.error(format!("stdout: {}", e.message())),
                }
            } else {
                if let Some(p) = prefix {
                    common::eprint(p);
                }
                let header = self.header_for(&path, &st);
                let r = self.zip(&mut input, &mut sink, &header);
                common::close(fd);
                match r {
                    Ok(hlen) => {
                        if self.verbose > 0 {
                            let r = ratio(input.consumed as i64 - (sink.written as i64 - hlen as i64), input.consumed);
                            common::eprint(format!("{r} -- {verb} stdout\n"));
                        }
                    }
                    Err(e) => self.error(format!("stdout: {}", e.message())),
                }
            }
            return Ok(());
        };
        // -N na descompressão: o nome guardado no cabeçalho (só o último componente).
        if self.decompress && !self.no_name() {
            let peek = input.ensure(4096).to_vec();
            if let Ok(h) = codec::gzip_header(&peek)
                && let Some(n) = &h.name
            {
                let base = common::base_name(n);
                if !base.is_empty() && base != b"." && base != b".." {
                    let dir = match path.iter().rposition(|&b| b == b'/') {
                        Some(i) => path[..=i].to_vec(),
                        None => Vec::new(),
                    };
                    out = common::cat(&[&dir, base]);
                }
            }
        }
        let out_shown = common::show(&out);
        let Some(ofd) = self.create_output(&out, &out_shown) else {
            common::close(fd);
            return Ok(());
        };
        let mut sink = Sink::fd(ofd);
        if !self.decompress
            && let Some(p) = &prefix
        {
            common::eprint(p);
        }
        let (result, last_out, first) = if self.decompress {
            let u = self.unzip(&mut input, &mut sink, &shown, prefix);
            let r = match u.status {
                Status::Ok => Ok(None),
                Status::Bad(m) => Err((m, false)),
                Status::Fatal(m) => Err((m, true)),
                Status::WriteErr(e) => Err((format!("{}: {out_shown}: {}\n", self.prog, e.message()), false)),
            };
            (r, u.last_out, u.first)
        } else {
            let header = self.header_for(&path, &st);
            let r = match self.zip(&mut input, &mut sink, &header) {
                Ok(h) => Ok(Some(h)),
                Err(e) => Err((format!("{}: {out_shown}: {}\n", self.prog, e.message()), false)),
            };
            (r, 0, None)
        };
        common::close(fd);
        match result {
            Ok(hlen) => {
                let mtime = match &first {
                    Some(h) if !self.no_time() && h.mtime != 0 => TimeSpec { sec: h.mtime as i64, nsec: 0 },
                    _ => st.mtime,
                };
                common::copy_attrs(ofd, &out, &st, mtime);
                common::close(ofd);
                if !self.keep {
                    let _ = common::unlink(&path);
                }
                if self.verbose > 0 {
                    let r = if self.decompress {
                        ratio(last_out as i64 - (input.consumed as i64 - self.header_bytes as i64), last_out)
                    } else {
                        let h = hlen.unwrap_or(0);
                        ratio(input.consumed as i64 - (sink.written as i64 - h as i64), input.consumed)
                    };
                    common::eprint(format!("{r} -- {verb} {out_shown}\n"));
                }
                Ok(())
            }
            Err((msg, fatal)) => {
                common::close(ofd);
                let _ = common::unlink(&out);
                common::eprint(msg);
                self.exit = ERROR;
                if fatal { Err(Abort) } else { Ok(()) }
            }
        }
    }

    fn header_for(&self, path: &[u8], st: &Stat) -> GzipHeader {
        let mtime = if self.no_time() { 0 } else { st.mtime.sec.clamp(0, u32::MAX as i64) as u32 };
        let name = if self.no_name() { None } else { Some(common::base_name(path).to_vec()) };
        GzipHeader { mtime, name }
    }

    /// Cria o arquivo de saída, tratando o arquivo que já existe como o gzip.
    fn create_output(&mut self, out: &[u8], shown: &str) -> Option<Fd> {
        loop {
            match common::create_exclusive(out) {
                Ok(fd) => return Some(fd),
                Err(Errno::EEXIST) => {
                    if self.force > 0 {
                        if let Err(e) = common::unlink(out) {
                            self.error(format!("{shown}: {}", e.message()));
                            return None;
                        }
                        continue;
                    }
                    if common::isatty(Fd::STDIN) {
                        common::eprint(format!("{}: {shown} already exists; do you wish to overwrite (y or n)? ", self.prog));
                        if read_yes() {
                            if let Err(e) = common::unlink(out) {
                                self.error(format!("{shown}: {}", e.message()));
                                return None;
                            }
                            continue;
                        }
                        common::eprint("\tnot overwritten\n");
                    } else {
                        common::eprint(format!("{}: {shown} already exists;\tnot overwritten\n", self.prog));
                    }
                    if self.exit == OK {
                        self.exit = WARNING;
                    }
                    return None;
                }
                Err(e) => {
                    self.error(format!("{shown}: {}", e.message()));
                    return None;
                }
            }
        }
    }

    fn treat_dir(&mut self, dir: &[u8]) -> Result<(), Abort> {
        let names = match common::read_dir(dir) {
            Ok(n) => n,
            Err(e) => {
                self.error(format!("{}: {}", common::show(dir), e.message()));
                return Ok(());
            }
        };
        for n in names {
            let p = common::join(dir, &n);
            self.treat_file(&p, false)?;
        }
        Ok(())
    }

    fn tz(&mut self) -> jiff::tz::TimeZone {
        if self.tz.is_none() {
            self.tz = Some(crate::tz::local());
        }
        self.tz.clone().unwrap_or(jiff::tz::TimeZone::UTC)
    }

    /// `-l`: descomprime tudo pra validar (como o gzip) e lista a partir do rodapé do último membro.
    fn list_input(&mut self, input: &mut Input, shown: &str, name: &[u8], file_mtime: i64) -> Result<(), Abort> {
        self.header_bytes = 0;
        let mut sink = Sink::null();
        let u = self.unzip(input, &mut sink, shown, None);
        match u.status {
            Status::Ok => {}
            Status::Bad(m) => {
                common::eprint(m);
                return Ok(());
            }
            Status::Fatal(m) => {
                common::eprint(m);
                return Err(Abort);
            }
            Status::WriteErr(_) => return Ok(()),
        }
        if u.trailing {
            return Ok(());
        }
        let Some(h) = u.first else { return Ok(()) };
        let t = u.trailer;
        let crc = u32::from_le_bytes([t[0], t[1], t[2], t[3]]);
        let isize = u32::from_le_bytes([t[4], t[5], t[6], t[7]]) as u64;
        self.list_line(&h, crc, isize, input.consumed, name, file_mtime);
        Ok(())
    }

    fn list_line(&mut self, h: &GzipHeaderInfo, crc: u32, isize: u64, comp: u64, name: &[u8], file_mtime: i64) {
        let mut line = String::new();
        if self.list_first && !self.quiet {
            if self.verbose > 0 {
                line.push_str("method  crc     date  time  ");
            }
            line.push_str("         compressed        uncompressed  ratio uncompressed_name\n");
        }
        self.list_first = false;
        self.list_any = true;
        let shown_name = match (&h.name, self.no_name()) {
            (Some(n), false) => common::show(n),
            _ => common::show(name),
        };
        if self.verbose > 0 {
            let t = if !self.no_time() && h.mtime != 0 { h.mtime as i64 } else { file_mtime };
            let tz = self.tz();
            let (_, mo, d, hh, mi, _) = crate::tz::civil(t, &tz);
            line.push_str(&format!("defla {crc:08x} {} {d:2} {hh:02}:{mi:02} ", month(mo)));
        }
        let r = ratio(isize as i64 - (comp as i64 - self.header_bytes as i64), isize);
        line.push_str(&format!("{comp:>19} {isize:>19} {r} {shown_name}\n"));
        self.total_in += comp;
        self.total_out += isize;
        self.out.write_str(&line);
    }

    fn list_totals(&mut self) {
        let mut line = String::new();
        if self.verbose > 0 {
            line.push_str("                            ");
        }
        let r = ratio(self.total_out as i64 - (self.total_in as i64 - self.header_bytes as i64), self.total_out);
        line.push_str(&format!("{:>19} {:>19} {r} (totals)\n", self.total_in, self.total_out));
        self.out.write_str(&line);
    }
}

fn month(m: u32) -> &'static str {
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(m as usize).saturating_sub(1) % 12]
}

/// Razão como o gzip mostra: `%5.1f%%`, zero quando não há base.
pub fn ratio(num: i64, den: u64) -> String {
    let r = if den == 0 { 0.0 } else { 100.0 * num as f64 / den as f64 };
    format!("{r:5.1}%")
}

fn out_text(s: &str) -> i32 {
    match sysabi::sys::write_all(Fd::STDOUT, s.as_bytes()) {
        Ok(()) => OK,
        Err(_) => ERROR,
    }
}

/// Lê uma resposta de sim ou não do stdin (primeiro caractere `y` ou `Y`).
pub fn read_yes() -> bool {
    let mut line = Vec::new();
    let mut b = [0u8; 1];
    while let Ok(1) = sysabi::sys::read(Fd::STDIN, &mut b) {
        if b[0] == b'\n' {
            break;
        }
        line.push(b[0]);
    }
    matches!(line.first(), Some(b'y') | Some(b'Y'))
}

/// Entrada do `gzip`.
pub fn main(args: &[Vec<u8>]) -> i32 {
    let mut g = Gzip::new();
    g.run(args)
}
