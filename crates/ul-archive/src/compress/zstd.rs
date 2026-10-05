//! `zstd` 1.5.7 do Debian 13 e os links `unzstd`, `zstdcat` e `zstdmt`: porte do `programs/zstdcli.c`
//! e do `programs/fileio.c`.
//!
//! A descompressão usa o decodificador próprio ([`super::zstd_dec`]), que reproduz a saída parcial e os
//! erros do C, inclusive o fatiamento de leitura em pedaços de `ZSTD_DStreamInSize()` e de saída em
//! `ZSTD_DStreamOutSize()`. A compressão usa o codificador do `structured-zstd`: os frames são válidos e
//! interoperáveis, mas os bytes não são os do zstd 1.5.7 (pendência registrada). `--format=gzip`, `xz`,
//! `lzma` e `lz4` usam os codificadores do crate e do `lz4_flex`; na descompressão esses formatos são
//! lidos como o C lê (zlib, liblzma e lz4frame).
//!
//! Fora do porte: o indicador de progresso que se atualiza por tempo (só as limpezas de linha do
//! `--progress` saem) e o `--sparse`, que no C só muda a ocupação em disco, não o conteúdo.

use std::io::{self, Read, Write};

use sysabi::sys::{self, SysResult};
use sysabi::{AtFlags, Errno, Fd, FileType, Mode, OFlags, SetTime, Stat, TimeSpec};

use super::common;
use super::zstd_dec::{self, DStream, Dict};
use super::zstd_gen;
use crate::codec;
use crate::sysutil::Output;

const STDIN_MARK: &[u8] = b"/*stdin*\\";
const STDOUT_MARK: &[u8] = b"/*stdout*\\";
const NUL_MARK: &[u8] = b"/dev/null";

const CLEVEL_DEFAULT: i32 = 3;
const CLEVEL_MAX: i32 = 19;
const MAX_CLEVEL: i32 = 22;
const MIN_CLEVEL: i32 = -(1 << 17);
const DEFAULT_MAX_WINDOW_LOG: u32 = 27;
const DEFAULT_MAX_DICT_SIZE: u32 = 110 << 10;
const DEFAULT_DICT_CLEVEL: i32 = 3;
const DEFAULT_SELECTIVITY: u32 = 9;
const DICTSIZE_MAX: u64 = 32 << 20;
const DEFAULT_FILE_PERMISSIONS: Mode = 0o666;
const TEMPORARY_FILE_PERMISSIONS: Mode = 0o600;
const CSTREAM_IN_SIZE: usize = 1 << 17;
const LZ4_MAGIC: u32 = 0x184D_2204;
const WINDOWLOG_MAX: u32 = 31;
const FILESIZE_UNKNOWN: u64 = u64::MAX;
const MAX_FILE_OF_FILE_NAMES_SIZE: u64 = 50 << 20;

const SUFFIX_LIST: [&[u8]; 10] =
    [b".zst", b".tzst", b".zstd", b".gz", b".tgz", b".lzma", b".xz", b".txz", b".lz4", b".tlz4"];
const SUFFIX_LIST_STR: &str = ".zst/.tzst/.gz/.tgz/.lzma/.xz/.txz/.lz4/.tlz4";

/// Extensões que o `--exclude-compressed` pula.
const COMPRESSED_EXTENSIONS: &[&str] = &[
    ".zst", ".tzst", ".gz", ".tgz", ".lzma", ".xz", ".txz", ".lz4", ".tlz4", ".7z", ".aa3", ".aac", ".aar",
    ".ace", ".alac", ".ape", ".apk", ".apng", ".arc", ".archive", ".arj", ".ark", ".asf", ".avi", ".avif",
    ".ba", ".br", ".bz2", ".cab", ".cdx", ".chm", ".cr2", ".divx", ".dmg", ".dng", ".docm", ".docx", ".dotm",
    ".dotx", ".dsft", ".ear", ".eftx", ".emz", ".eot", ".epub", ".f4v", ".flac", ".flv", ".gho", ".gif",
    ".gifv", ".gnp", ".iso", ".jar", ".jpeg", ".jpg", ".jxl", ".lz", ".lzh", ".m4a", ".m4v", ".mkv", ".mov",
    ".mp2", ".mp3", ".mp4", ".mpa", ".mpc", ".mpe", ".mpeg", ".mpg", ".mpl", ".mpv", ".msi", ".odp", ".ods",
    ".odt", ".ogg", ".ogv", ".otp", ".ots", ".ott", ".pea", ".png", ".pptx", ".qt", ".rar", ".s7z", ".sfx",
    ".sit", ".sitx", ".sqx", ".svgz", ".swf", ".tbz2", ".tib", ".tlz", ".vob", ".war", ".webm", ".webp",
    ".wma", ".wmv", ".woff", ".woff2", ".wvl", ".xlsx", ".xpi", ".xps", ".zip", ".zipx", ".zoo", ".zpaq",
];

/// Junta pedaços (texto e bytes) numa mensagem.
macro_rules! msg {
    ($($p:expr),* $(,)?) => {{
        let mut v: Vec<u8> = Vec::new();
        $( v.extend_from_slice(AsRef::<[u8]>::as_ref(&$p)); )*
        v
    }};
}

/// Saída do programa com o código de `exit` (a mensagem já foi impressa).
struct Exit(i32);

type R<T> = Result<T, Exit>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CType {
    Zstd,
    Gzip,
    Xz,
    Lzma,
    Lz4,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Compress,
    Decompress,
    Test,
    Bench,
    Train,
    List,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Progress {
    Auto,
    Never,
    Always,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DictKind {
    Cover,
    FastCover,
    Legacy,
}

/// `FIO_prefs_t`.
struct Prefs {
    ctype: CType,
    overwrite: bool,
    dict_id_flag: bool,
    checksum_flag: u8,
    remove_src: bool,
    mem_limit: u32,
    test_mode: bool,
    exclude_compressed: bool,
    allow_block_devices: bool,
    pass_through: i8,
    content_size: bool,
    stream_src_size: usize,
    patch_from: bool,
    /// `windowLog` explícito do `--long`/`--zstd=wlog=` (0 = o do nível).
    window_log: u32,
    /// `--long`.
    ldm: bool,
    /// `sparseFileSupport`: 0 desligado, 1 padrão, 2 `--sparse`. Só muda as mensagens do `-vv`.
    sparse: u8,
    /// `nbWorkers`, pras mensagens do `-vvv`.
    nb_workers: i64,
}

/// `FIO_ctx_t`.
#[derive(Default)]
struct Fctx {
    nb_files_total: usize,
    has_stdin: bool,
    has_stdout: bool,
    curr: usize,
    processed: usize,
    total_in: u64,
    total_out: u64,
}

/// Parâmetros de compressão do `--zstd=` e do `--long` (`ZSTD_compressionParameters`).
#[derive(Default, Clone, Copy)]
struct CParams {
    window_log: u32,
    chain_log: u32,
    hash_log: u32,
    search_log: u32,
    min_match: u32,
    target_length: u32,
    strategy: u32,
    /// `overlapLog=` do `--zstd=` (`g_overlapLog`).
    overlap_log: Option<u32>,
}

/// O pool de leitura do `fileio_asyncio.c`: pedaços de `job` bytes, com a coalescência do que sobrou.
struct ReadPool {
    fd: Option<Fd>,
    job: usize,
    buf: Vec<u8>,
    pos: usize,
    eof: bool,
}

impl ReadPool {
    fn new(job: usize) -> ReadPool {
        ReadPool { fd: None, job, buf: Vec::new(), pos: 0, eof: false }
    }

    fn set_file(&mut self, fd: Option<Fd>) {
        self.fd = fd;
        self.buf.clear();
        self.pos = 0;
        self.eof = false;
    }

    fn loaded(&self) -> &[u8] {
        &self.buf[self.pos..]
    }

    fn consume(&mut self, n: usize) {
        self.pos += n.min(self.buf.len() - self.pos);
    }

    /// Lê um pedaço inteiro (ou até o fim do arquivo).
    fn read_job(&mut self) -> SysResult<Vec<u8>> {
        let mut out = vec![0u8; self.job];
        let mut got = 0;
        if self.eof {
            return Ok(Vec::new());
        }
        let Some(fd) = self.fd else { return Ok(Vec::new()) };
        while got < self.job {
            sys::checkpoint();
            match sys::read(fd, &mut out[got..]) {
                Ok(0) => {
                    self.eof = true;
                    break;
                }
                Ok(n) => got += n,
                Err(Errno::EINTR) => {}
                Err(e) => return Err(e),
            }
        }
        out.truncate(got);
        Ok(out)
    }

    /// `AIO_ReadPool_fillBuffer`: garante `min(n, job)` bytes carregados; devolve quanto leu.
    fn fill(&mut self, n: usize) -> SysResult<usize> {
        let n = n.min(self.job);
        if self.buf.len() - self.pos >= n {
            return Ok(0);
        }
        let job = self.read_job()?;
        self.buf.drain(..self.pos);
        self.pos = 0;
        self.buf.extend_from_slice(&job);
        Ok(job.len())
    }
}

/// Destino da saída: o stdout do programa, um arquivo, ou nada (modo de teste).
enum Dst {
    Closed,
    Stdout,
    File { fd: Fd, out: Output },
    Test,
}

/// `Write` sobre o destino, contando os bytes e guardando o primeiro erro.
struct Sink<'a> {
    dst: &'a mut Dst,
    stdout: &'a mut Output,
    written: u64,
    error: Option<Errno>,
}

impl Sink<'_> {
    fn put(&mut self, data: &[u8]) -> io::Result<()> {
        if self.error.is_some() {
            return Err(io::Error::other("write error"));
        }
        let out = match self.dst {
            Dst::Stdout => &mut *self.stdout,
            Dst::File { out, .. } => out,
            Dst::Closed | Dst::Test => {
                self.written += data.len() as u64;
                return Ok(());
            }
        };
        out.write(data);
        if let Some(e) = out.error() {
            self.error = Some(e);
            return Err(io::Error::other("write error"));
        }
        self.written += data.len() as u64;
        Ok(())
    }
}

impl Write for Sink<'_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.put(data)?;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// `UTIL_HumanReadableSize_t`.
struct Hrs {
    value: f64,
    suffix: &'static str,
    precision: usize,
}

impl Hrs {
    fn show(&self) -> String {
        format!("{:.*}{}", self.precision, self.value, self.suffix)
    }

    /// `%W.*f%S` com largura do número e do sufixo.
    fn padded(&self, width: usize, suffix_width: usize) -> String {
        format!("{:>w$.p$}{:>sw$}", self.value, self.suffix, w = width, p = self.precision, sw = suffix_width)
    }
}

/// `%-Ns` sobre bytes.
fn ljust(s: &[u8], width: usize) -> Vec<u8> {
    let mut v = s.to_vec();
    while v.len() < width {
        v.push(b' ');
    }
    v
}

/// `readU32FromCharChecked`: dígitos com sufixo K/M (e `i`, `B` opcionais). `None` no estouro.
fn read_u32(s: &[u8], pos: &mut usize) -> Option<u32> {
    let mut result: u32 = 0;
    while let Some(&c) = s.get(*pos) {
        if !c.is_ascii_digit() {
            break;
        }
        let last = result;
        if result > u32::MAX / 10 {
            return None;
        }
        result = result.wrapping_mul(10).wrapping_add(u32::from(c - b'0'));
        if result < last {
            return None;
        }
        *pos += 1;
    }
    if let Some(&c @ (b'K' | b'M')) = s.get(*pos) {
        let max_k = u32::MAX >> 10;
        if result > max_k {
            return None;
        }
        result <<= 10;
        if c == b'M' {
            if result > max_k {
                return None;
            }
            result <<= 10;
        }
        *pos += 1;
        if s.get(*pos) == Some(&b'i') {
            *pos += 1;
        }
        if s.get(*pos) == Some(&b'B') {
            *pos += 1;
        }
    }
    Some(result)
}

/// `readSizeTFromCharChecked`.
fn read_size(s: &[u8], pos: &mut usize) -> Option<u64> {
    let mut result: u64 = 0;
    while let Some(&c) = s.get(*pos) {
        if !c.is_ascii_digit() {
            break;
        }
        let last = result;
        if result > u64::MAX / 10 {
            return None;
        }
        result = result.wrapping_mul(10).wrapping_add(u64::from(c - b'0'));
        if result < last {
            return None;
        }
        *pos += 1;
    }
    if let Some(&c @ (b'K' | b'M')) = s.get(*pos) {
        let max_k = u64::MAX >> 10;
        if result > max_k {
            return None;
        }
        result <<= 10;
        if c == b'M' {
            if result > max_k {
                return None;
            }
            result <<= 10;
        }
        *pos += 1;
        if s.get(*pos) == Some(&b'i') {
            *pos += 1;
        }
        if s.get(*pos) == Some(&b'B') {
            *pos += 1;
        }
    }
    Some(result)
}

/// `exeNameMatch`: o nome sem extensão é `test`.
fn exe_name_match(name: &[u8], test: &str) -> bool {
    name.starts_with(test.as_bytes()) && matches!(name.get(test.len()), None | Some(b'.'))
}

fn stat_path(path: &[u8]) -> SysResult<Stat> {
    common::stat(path)
}

fn is_regular(path: &[u8]) -> bool {
    stat_path(path).is_ok_and(|st| st.file_type() == FileType::Regular)
}

fn is_directory(path: &[u8]) -> bool {
    stat_path(path).is_ok_and(|st| st.file_type() == FileType::Directory)
}

fn is_link(path: &[u8]) -> bool {
    common::lstat(path).is_ok_and(|st| st.file_type() == FileType::Symlink)
}

fn is_fifo(path: &[u8]) -> bool {
    stat_path(path).is_ok_and(|st| st.file_type() == FileType::Fifo)
}

/// `UTIL_getFileSize`: tamanho de arquivo regular, ou desconhecido.
fn file_size(path: &[u8]) -> u64 {
    match stat_path(path) {
        Ok(st) => file_size_stat(&st),
        Err(_) => FILESIZE_UNKNOWN,
    }
}

fn file_size_stat(st: &Stat) -> u64 {
    if st.file_type() == FileType::Regular { st.size } else { FILESIZE_UNKNOWN }
}

fn same_file(a: &Stat, b: &Stat) -> bool {
    a.dev == b.dev && a.ino == b.ino
}

fn isatty(fd: Fd) -> bool {
    common::isatty(fd)
}

/// O programa: estado do `zstdcli.c` e do `fileio.c`.
struct Zstd {
    level: i32,
    util_level: i32,
    progress: Progress,
    out: Output,
    prefs: Prefs,
    fctx: Fctx,
    rp: ReadPool,
    dst: Dst,
    dctx: Option<DStream>,
    fake_stdin_console: bool,
    fake_stdout_console: bool,
    fake_stderr_console: bool,
}

impl Zstd {
    fn disp(&self, l: i32, m: impl AsRef<[u8]>) {
        if self.level >= l {
            common::eprint(m);
        }
    }

    /// `EXM_THROW`.
    fn throw(&self, code: i32, m: impl AsRef<[u8]>) -> Exit {
        self.disp(1, msg!("zstd: error ", code.to_string(), " : ", m, " \n"));
        Exit(code)
    }

    fn should_display_summary(&self) -> bool {
        self.level >= 2 || self.progress == Progress::Always
    }

    fn display_summary(&self, m: impl AsRef<[u8]>) {
        if self.should_display_summary() {
            self.disp(1, m);
        }
    }

    /// `DISPLAY_PROGRESS("\r%79s\r", "")`.
    fn clear_progress(&self) {
        if self.progress != Progress::Never && self.should_display_summary() {
            self.disp(1, format!("\r{:79}\r", ""));
        }
    }

    fn console_stdin(&self) -> bool {
        self.fake_stdin_console || isatty(Fd::STDIN)
    }

    fn console_stdout(&self) -> bool {
        self.fake_stdout_console || isatty(Fd::STDOUT)
    }

    fn console_stderr(&self) -> bool {
        self.fake_stderr_console || isatty(Fd::STDERR)
    }

    /// `UTIL_makeHumanReadableSize`.
    fn hrs(&self, size: u64) -> Hrs {
        if self.util_level > 3 {
            if size >= 1 << 53 {
                return Hrs { value: size as f64 / (1u64 << 20) as f64, suffix: " MiB", precision: 2 };
            }
            return Hrs { value: size as f64, suffix: " B", precision: 0 };
        }
        let (value, suffix) = if size >= 1 << 60 {
            (size as f64 / (1u64 << 60) as f64, " EiB")
        } else if size >= 1 << 50 {
            (size as f64 / (1u64 << 50) as f64, " PiB")
        } else if size >= 1 << 40 {
            (size as f64 / (1u64 << 40) as f64, " TiB")
        } else if size >= 1 << 30 {
            (size as f64 / (1u64 << 30) as f64, " GiB")
        } else if size >= 1 << 20 {
            (size as f64 / (1u64 << 20) as f64, " MiB")
        } else if size >= 1 << 10 {
            (size as f64 / (1u64 << 10) as f64, " KiB")
        } else {
            (size as f64, " B")
        };
        let precision = if value >= 100.0 || value as u64 == size {
            0
        } else if value >= 10.0 {
            1
        } else if value > 1.0 {
            2
        } else {
            3
        };
        Hrs { value, suffix, precision }
    }

    fn fill(&mut self, n: usize) -> R<usize> {
        match self.rp.fill(n) {
            Ok(n) => Ok(n),
            Err(_) => Err(self.throw(37, "Read error")),
        }
    }

    /// Escreve no destino atual; erro de escrita encerra como o `AIO_WritePool`.
    fn write_out(&mut self, data: &[u8]) -> R<()> {
        if data.is_empty() {
            return Ok(());
        }
        let mut sink = Sink { dst: &mut self.dst, stdout: &mut self.out, written: 0, error: None };
        let _ = sink.put(data);
        if let Some(e) = sink.error {
            return Err(self.throw(70, msg!("Write error : cannot write block : ", e.message())));
        }
        Ok(())
    }

    /// `AIO_WritePool_closeFile`: devolve o erro do `fclose`.
    fn close_dst(&mut self) -> Option<Errno> {
        match std::mem::replace(&mut self.dst, Dst::Closed) {
            Dst::Stdout => self.out.finish().err(),
            Dst::File { fd, mut out } => {
                let r = out.finish().err();
                let c = sys::close(fd).err();
                r.or(c)
            }
            Dst::Closed | Dst::Test => None,
        }
    }

    /// `UTIL_requireUserConfirmation`: `true` quando a resposta não é aceita.
    fn require_confirmation(&self, prompt: &str, abort: &str, letters: &[u8]) -> bool {
        if self.fctx.has_stdin {
            common::eprint("stdin is an input - not proceeding.\n");
            return true;
        }
        common::eprint(prompt);
        let getchar = || -> Option<u8> {
            let mut b = [0u8; 1];
            match sys::read(Fd::STDIN, &mut b) {
                Ok(1) => Some(b[0]),
                _ => None,
            }
        };
        let mut ch = getchar();
        let mut result = false;
        let accepted = matches!(ch, Some(c) if c == 0 || letters.contains(&c));
        if !accepted {
            common::eprint(format!("{abort} \n"));
            result = true;
        }
        while let Some(c) = ch {
            if c == b'\n' {
                break;
            }
            ch = getchar();
        }
        result
    }

    /// `FIO_removeFile`: `Ok` quando removeu ou recusou com aviso; `Err` com o errno do `remove`.
    fn remove_file(&self, path: &[u8]) -> Result<(), Errno> {
        let Ok(st) = stat_path(path) else {
            self.disp(2, msg!("zstd: Failed to stat ", path, " while trying to remove it\n"));
            return Ok(());
        };
        if st.file_type() != FileType::Regular {
            self.disp(2, msg!("zstd: Refusing to remove non-regular file ", path, "\n"));
            return Ok(());
        }
        common::unlink(path)
    }

    /// `FIO_openSrcFile`.
    fn open_src(&self, name: &[u8], allow_block: bool) -> Option<(Fd, Option<Stat>)> {
        if name == STDIN_MARK {
            self.disp(4, "Using stdin for input \n");
            return Some((Fd::STDIN, None));
        }
        let st = match stat_path(name) {
            Ok(st) => st,
            Err(e) => {
                self.disp(1, msg!("zstd: can't stat ", name, " : ", e.message(), " -- ignored \n"));
                return None;
            }
        };
        let ft = st.file_type();
        if ft != FileType::Regular && ft != FileType::Fifo && !(allow_block && ft == FileType::BlockDevice) {
            self.disp(1, msg!("zstd: ", name, " is not a regular file -- ignored \n"));
            return None;
        }
        match common::open_input(name, false) {
            Ok(fd) => Some((fd, Some(st))),
            Err(e) => {
                self.disp(1, msg!("zstd: ", name, ": ", e.message(), " \n"));
                None
            }
        }
    }

    /// `FIO_openDstFile`: abre o destino em `self.dst`. `false` quando não abriu.
    fn open_dst(&mut self, src: Option<&[u8]>, dst: &[u8], mode: Mode) -> R<bool> {
        if self.prefs.test_mode {
            return Ok(false);
        }
        if dst == STDOUT_MARK {
            self.disp(4, "Using stdout for output \n");
            if self.prefs.sparse == 1 {
                self.prefs.sparse = 0;
                self.disp(4, "Sparse File Support is automatically disabled on stdout ; try --sparse \n");
            }
            self.dst = Dst::Stdout;
            return Ok(true);
        }
        if let Some(src) = src
            && let (Ok(a), Ok(b)) = (stat_path(src), stat_path(dst))
            && same_file(&a, &b)
        {
            self.disp(1, "zstd: Refusing to open an output file which will overwrite the input file \n");
            return Ok(false);
        }
        let dst_is_reg = is_regular(dst);
        if self.prefs.sparse == 1 && !dst_is_reg {
            self.prefs.sparse = 0;
            self.disp(4, "Sparse File Support is disabled when output is not a file \n");
        }
        if dst_is_reg {
            if dst == NUL_MARK {
                return Err(self.throw(40, msg!(dst, " is unexpectedly categorized as a regular file")));
            }
            if !self.prefs.overwrite {
                if self.level <= 1 {
                    self.disp(1, msg!("zstd: ", dst, " already exists; not overwritten  \n"));
                    return Ok(false);
                }
                common::eprint(msg!("zstd: ", dst, " already exists; "));
                if self.require_confirmation("overwrite (y/n) ? ", "Not overwritten  \n", b"yY") {
                    return Ok(false);
                }
            }
            let _ = self.remove_file(dst);
        }
        match sys::open(dst, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC, mode) {
            Ok(fd) => {
                self.dst = Dst::File { fd, out: Output::new(fd) };
                Ok(true)
            }
            Err(e) => {
                self.disp(1, msg!("zstd: ", dst, ": ", e.message(), "\n"));
                Ok(false)
            }
        }
    }

    /// `UTIL_setFDStat`: grupo, permissões e dono do original.
    fn set_fd_stat(&self, fd: Fd, path: &[u8], st: &Stat) {
        let cur = sys::current();
        match cur.fstat(fd) {
            Ok(c) if c.file_type() == FileType::Regular => {}
            _ => return,
        }
        let _ = cur.fchownat(Fd::CWD, path, None, Some(st.gid), AtFlags::empty());
        let _ = cur.fchmod(fd, st.mode & 0o777);
        let _ = cur.fchownat(Fd::CWD, path, Some(st.uid), None, AtFlags::empty());
    }

    /// `UTIL_utime`: acesso agora, modificação do original.
    fn utime(&self, path: &[u8], st: &Stat) {
        let _ = sys::current().utimensat(Fd::CWD, path, SetTime::Now, SetTime::At(st.mtime), AtFlags::empty());
    }

    /// `FIO_multiFilesConcatWarning`: `true` quando deve abortar.
    fn multi_files_concat_warning(&mut self, out_name: Option<&[u8]>, cutoff: i32) -> R<bool> {
        if self.fctx.has_stdout && self.prefs.remove_src {
            return Err(self.throw(
                43,
                "It's not allowed to remove input files when processed output is piped to stdout. This scenario is not supposed to be possible. This is a programming error. File an issue for it to be fixed.",
            ));
        }
        if self.prefs.test_mode {
            if self.prefs.remove_src {
                return Err(self.throw(
                    43,
                    "Test mode shall not remove input files! This scenario is not supposed to be possible. This is a programming error. File an issue for it to be fixed.",
                ));
            }
            return Ok(false);
        }
        if self.fctx.nb_files_total == 1 {
            return Ok(false);
        }
        let Some(out_name) = out_name else { return Ok(false) };
        if self.fctx.has_stdout {
            self.disp(2, "zstd: WARNING: all input files will be processed and concatenated into stdout. \n");
        } else {
            self.disp(
                2,
                msg!(
                    "zstd: WARNING: all input files will be processed and concatenated into a single output file: ",
                    out_name,
                    " \n"
                ),
            );
        }
        self.disp(2, "The concatenated output CANNOT regenerate original file names nor directory structure. \n");
        if self.prefs.remove_src {
            self.disp(2, "Since it's a destructive operation, input files will not be removed. \n");
            self.prefs.remove_src = false;
        }
        if self.fctx.has_stdout || self.prefs.overwrite {
            return Ok(false);
        }
        if self.level <= cutoff {
            self.disp(1, "Concatenating multiple processed inputs into a single output loses file metadata. \n");
            self.disp(1, "Aborting. \n");
            return Ok(true);
        }
        Ok(self.require_confirmation("Proceed? (y/n): ", "Aborting...", b"yY"))
    }

    /// `FIO_checkFilenameCollisions`.
    fn check_filename_collisions(&self, names: &[Vec<u8>]) {
        let mut sorted: Vec<&[u8]> = names.iter().map(|n| common::base_name(n)).collect();
        sorted.sort();
        for w in sorted.windows(2) {
            if w[0] == w[1] {
                self.disp(2, msg!("WARNING: Two files have same filename: ", w[0], "\n"));
            }
        }
    }

    /// Carrega o dicionário (`FIO_initDict` com o `FIO_getDictFileStat`).
    fn load_dict(&self, name: Option<&[u8]>) -> R<Option<(Vec<u8>, Stat)>> {
        let Some(name) = name else { return Ok(None) };
        let st = match stat_path(name) {
            Ok(st) => st,
            Err(e) => {
                return Err(self.throw(31, msg!("Stat failed on dictionary file ", name, ": ", e.message())));
            }
        };
        if st.file_type() != FileType::Regular {
            return Err(self.throw(32, msg!("Dictionary ", name, " must be a regular file.")));
        }
        self.disp(4, msg!("Loading ", name, " as dictionary \n"));
        let fd = match common::open_input(name, false) {
            Ok(fd) => fd,
            Err(e) => return Err(self.throw(33, msg!("Couldn't open dictionary ", name, ": ", e.message()))),
        };
        let max = if self.prefs.patch_from { u64::from(self.prefs.mem_limit) } else { DICTSIZE_MAX };
        if st.size > max {
            common::close(fd);
            return Err(self.throw(
                34,
                msg!("Dictionary file ", name, " is too large (> ", (max as u32).to_string(), " bytes)"),
            ));
        }
        let data = common::read_all(fd);
        common::close(fd);
        match data {
            Ok(d) if d.len() as u64 == st.size => Ok(Some((d, st))),
            Ok(_) => Err(self.throw(35, msg!("Error reading dictionary file ", name, " : Success"))),
            Err(e) => Err(self.throw(35, msg!("Error reading dictionary file ", name, " : ", e.message()))),
        }
    }

}

// -------------------------------------------------------------------------------------------------
// Codificadores (compressão de um arquivo num frame).

/// Por que a codificação de um frame parou.
enum EncError {
    /// Erro de leitura da entrada (`Read error`, código 37).
    Read,
    /// Erro de escrita no destino (o errno ficou no `Sink`).
    Write,
    /// Erro da biblioteca zstd, já com o nome do `ZSTD_getErrorName`.
    Zstd(&'static str),
    /// `deflateInit2` recusou o nível.
    GzipInit(i32),
    /// Erro do codificador lzma/xz.
    Lzma,
    /// Erro do codificador lz4, com o nome do `LZ4F_getErrorName`.
    Lz4(String),
}

/// O que o frame grava, montado das preferências.
struct Job<'a> {
    level: i32,
    checksum: bool,
    content_size: bool,
    dict_id: bool,
    /// Tamanho prometido (`ZSTD_CCtx_setPledgedSrcSize`), quando conhecido.
    pledged: Option<u64>,
    dict: Option<&'a [u8]>,
    /// `windowLog` explícito (`--long`, `--zstd=wlog=`), 0 quando é o do nível.
    window_log: u32,
    /// Tamanho do arquivo de entrada (`UTIL_getFileSize`), ou `FILESIZE_UNKNOWN`.
    file_size: u64,
    /// Nível de mensagens, pros traços do `-vvvv`.
    trace: i32,
}

/// Lê a entrada em pedaços de `ZSTD_CStreamInSize()` e entrega ao codificador.
fn pump(enc: &mut dyn Write, rp: &mut ReadPool, read: &mut u64) -> Result<(), EncError> {
    loop {
        sys::checkpoint();
        if rp.fill(CSTREAM_IN_SIZE).is_err() {
            return Err(EncError::Read);
        }
        let n = rp.loaded().len();
        if n == 0 {
            return Ok(());
        }
        let chunk = rp.loaded().to_vec();
        rp.consume(n);
        *read += n as u64;
        enc.write_all(&chunk).map_err(|_| EncError::Write)?;
    }
}

/// Regrava o cabeçalho de frame do codificador do crate no formato do `ZSTD_writeFrameHeader`
/// quando o C gravaria segmento único (janela do nível, ajustada ao tamanho, cobrindo o conteúdo).
/// Com segmento único a janela passa a ser o tamanho do conteúdo, que cobre qualquer offset do
/// frame (e o dicionário continua referenciável, como nos frames que o próprio C grava), então a
/// troca não muda a validade dos blocos. Fora desse caso (tamanho desconhecido, janela menor que o
/// conteúdo) o cabeçalho do crate passa intacto: declarar uma janela menor que a usada pelo
/// codificador geraria frames inválidos.
struct HeaderFix<W: Write> {
    inner: W,
    /// O cabeçalho do C, enquanto ainda não foi trocado.
    replacement: Option<Vec<u8>>,
    pending: Vec<u8>,
    /// Bytes entregues ao escritor de baixo.
    written: u64,
}

impl<W: Write> HeaderFix<W> {
    fn new(inner: W) -> HeaderFix<W> {
        HeaderFix { inner, replacement: None, pending: Vec::new(), written: 0 }
    }

    /// Prepara a troca pro frame de `size` bytes; antes do primeiro byte escrito.
    fn arm(&mut self, job: &Job, size: u64) {
        // Com dicionário a janela do C só cresce (`ZSTD_dictAndWindowLog`), então a condição sem ele
        // já implica o segmento único do C.
        let dict_id = match job.dict {
            Some(d) if job.dict_id => Dict::parse(d).map(|d| d.id()).unwrap_or(0),
            _ => 0,
        };
        self.replacement = if job.content_size {
            // `ZSTD_overrideCParams` e depois o encolhimento do `ZSTD_adjustCParams_internal`.
            let over = CParams { window_log: job.window_log, ..CParams::default() };
            let wlog = get_cparams_with(job.level, size, 0, &over).window_log;
            (1u64 << wlog >= size).then(|| {
                let fcs_code = u8::from(size >= 256) + u8::from(size >= 65536 + 256) + u8::from(size >= 0xFFFF_FFFF);
                let did_code = u8::from(dict_id > 0) + u8::from(dict_id >= 256) + u8::from(dict_id >= 65536);
                let mut h = zstd_dec::MAGIC.to_le_bytes().to_vec();
                h.push(did_code | (u8::from(job.checksum) << 2) | (1 << 5) | (fcs_code << 6));
                match did_code {
                    0 => {}
                    1 => h.push(dict_id as u8),
                    2 => h.extend_from_slice(&(dict_id as u16).to_le_bytes()),
                    _ => h.extend_from_slice(&dict_id.to_le_bytes()),
                }
                match fcs_code {
                    0 => h.push(size as u8),
                    1 => h.extend_from_slice(&((size - 256) as u16).to_le_bytes()),
                    2 => h.extend_from_slice(&(size as u32).to_le_bytes()),
                    _ => h.extend_from_slice(&size.to_le_bytes()),
                }
                h
            })
        } else {
            None
        };
    }
}

impl<W: Write> Write for HeaderFix<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let Some(rep) = &self.replacement else {
            let n = self.inner.write(data)?;
            self.written += n as u64;
            return Ok(n);
        };
        self.pending.extend_from_slice(data);
        let size = match zstd_dec::get_frame_header(&mut zstd_dec::FrameHeader::default(), &self.pending) {
            Ok(0) => zstd_dec::frame_header_size(&self.pending).unwrap_or(self.pending.len()),
            Ok(_) => return Ok(data.len()),
            // Não é um cabeçalho que reconheço: entrega como veio.
            Err(_) => 0,
        };
        let mut out = if size > 0 { rep.clone() } else { Vec::new() };
        out.extend_from_slice(&self.pending[size..]);
        self.inner.write_all(&out)?;
        self.written += out.len() as u64;
        self.pending.clear();
        self.replacement = None;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// O formato zstd, pelo `structured-zstd`.
fn encode_zstd(job: &Job, sink: &mut Sink, rp: &mut ReadPool, read: &mut u64) -> Result<(), EncError> {
    use structured_zstd::encoding::{CompressionLevel, StreamingEncoder};
    let mut fixed = HeaderFix::new(&mut *sink);
    if let Some(size) = job.pledged {
        fixed.arm(job, size);
    }
    let mut enc = StreamingEncoder::new(fixed, CompressionLevel::from_level(job.level));
    // Os erros de configuração são de parâmetro; o C só os veria como `ZSTD_compressStream2` falhando.
    let size_wrong = |_| EncError::Zstd("Src size is incorrect");
    enc.set_content_checksum(job.checksum).map_err(size_wrong)?;
    enc.set_content_size_flag(job.content_size).map_err(size_wrong)?;
    enc.set_dictionary_id_flag(job.dict_id).map_err(size_wrong)?;
    if let Some(size) = job.pledged {
        enc.set_pledged_content_size(size).map_err(size_wrong)?;
    }
    if let Some(d) = job.dict {
        // `ZSTD_createCDict` devolve NULL num dicionário inválido, e o C reporta falta de memória.
        enc.set_dictionary_from_bytes(d).map_err(|_| EncError::Zstd("Allocation error : not enough memory"))?;
    }
    // O laço do `FIO_compressZstdFrame`: lê pedaços até o fim ou até o tamanho do arquivo, e o
    // último pedaço vai com `ZSTD_e_end`.
    let mut first = true;
    loop {
        sys::checkpoint();
        let Ok(in_size) = rp.fill(CSTREAM_IN_SIZE) else { return Err(EncError::Read) };
        if job.trace >= 6 {
            common::eprint(format!("fread {in_size} bytes from source \n"));
        }
        *read += in_size as u64;
        let chunk = rp.loaded().to_vec();
        rp.consume(chunk.len());
        let end = chunk.is_empty() || *read == job.file_size;
        // Entrada inteira na primeira chamada com `ZSTD_e_end`: o `ZSTD_compressStream2` passa a
        // conhecer o tamanho e grava o FCS (o caso do stdin pequeno).
        if first && end && job.pledged.is_none() {
            let size = chunk.len() as u64;
            enc.set_pledged_content_size(size).map_err(size_wrong)?;
            enc.get_mut().arm(job, size);
        }
        first = false;
        let before = enc.get_ref().written;
        enc.write_all(&chunk).map_err(|_| EncError::Write)?;
        if end {
            let r = enc.finish();
            let (ok, produced) = match &r {
                Ok(w) => (true, w.written - before),
                Err(_) => (false, 0),
            };
            drop(r);
            if job.trace >= 6 {
                common::eprint(format!(
                    "ZSTD_compress_generic(end:2) => input pos({0})<=({0})size ; output generated {produced} bytes \n",
                    chunk.len()
                ));
            }
            return if ok {
                Ok(())
            } else if sink_failed(sink) {
                Err(EncError::Write)
            } else {
                Err(EncError::Zstd("Src size is incorrect"))
            };
        }
        if job.trace >= 6 {
            common::eprint(format!(
                "ZSTD_compress_generic(end:0) => input pos({0})<=({0})size ; output generated {1} bytes \n",
                chunk.len(),
                enc.get_ref().written - before
            ));
        }
    }
}

fn sink_failed(sink: &Sink) -> bool {
    sink.error.is_some()
}

/// O formato gzip, com o cabeçalho que o `deflateInit2(..., 15 + 16, ...)` do zlib grava.
fn encode_gzip(job: &Job, sink: &mut Sink, rp: &mut ReadPool, read: &mut u64) -> Result<(), EncError> {
    let level = match job.level {
        l if l > 9 => 9,
        -1 => 6,
        l if l < -1 => return Err(EncError::GzipInit(-2)),
        l => l,
    };
    let xfl = if level == 9 {
        2
    } else if level < 2 {
        4
    } else {
        0
    };
    sink.put(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, xfl, 3]).map_err(|_| EncError::Write)?;
    let mut d = codec::RawDeflate::new(level as u32, &mut *sink);
    pump(&mut d, rp, read)?;
    let (w, crc, size) = d.finish().map_err(|_| EncError::Write)?;
    w.put(&crc.to_le_bytes()).map_err(|_| EncError::Write)?;
    w.put(&(size as u32).to_le_bytes()).map_err(|_| EncError::Write)
}

/// Os formatos xz (`lzma_easy_encoder` com CRC64) e lzma (`lzma_alone_encoder`).
fn encode_lzma(job: &Job, xz: bool, sink: &mut Sink, rp: &mut ReadPool, read: &mut u64) -> Result<(), EncError> {
    let level = job.level.clamp(0, 9) as u32;
    let fmt = if xz { codec::Format::Xz } else { codec::Format::Lzma };
    let mut enc =
        codec::Encoder::new(fmt, level, &codec::GzipHeader::default(), &mut *sink).map_err(|_| EncError::Lzma)?;
    pump(&mut enc, rp, read)?;
    let r = enc.finish().map(|_| ());
    match r {
        Ok(()) => Ok(()),
        Err(_) if sink_failed(sink) => Err(EncError::Write),
        Err(_) => Err(EncError::Lzma),
    }
}

/// O formato lz4: blocos ligados de 64 KiB, checksum de conteúdo e tamanho quando conhecido.
fn encode_lz4(job: &Job, sink: &mut Sink, rp: &mut ReadPool, read: &mut u64) -> Result<(), EncError> {
    use lz4_flex::frame::{BlockMode, BlockSize, FrameEncoder, FrameInfo};
    let info = FrameInfo::new()
        .block_size(BlockSize::Max64KB)
        .block_mode(BlockMode::Linked)
        .content_checksum(job.checksum)
        .content_size(job.pledged.filter(|&s| s > 0));
    let mut enc = FrameEncoder::with_frame_info(info, &mut *sink);
    pump(&mut enc, rp, read)?;
    let r = enc.finish().map(|_| ());
    match r {
        Ok(()) => Ok(()),
        Err(_) if sink_failed(sink) => Err(EncError::Write),
        Err(e) => Err(EncError::Lz4(lz4_error_name(&e))),
    }
}

/// Nome do `LZ4F_getErrorName` pro erro equivalente do `lz4_flex`.
fn lz4_error_name(e: &lz4_flex::frame::Error) -> String {
    use lz4_flex::frame::Error as E;
    match e {
        E::WrongMagicNumber => "ERROR_frameType_unknown",
        E::ContentChecksumError => "ERROR_contentChecksum_invalid",
        E::BlockChecksumError => "ERROR_blockChecksum_invalid",
        E::HeaderChecksumError => "ERROR_headerChecksum_invalid",
        E::ContentLengthError { .. } => "ERROR_frameSize_wrong",
        E::UnsupportedBlocksize(_) | E::BlockTooBig => "ERROR_maxBlockSize_invalid",
        E::ReservedBitsSet => "ERROR_reservedFlag_set",
        E::UnsupportedVersion(_) => "ERROR_headerVersion_wrong",
        E::DictionaryNotSupported => "ERROR_parameter_unsupported",
        _ => "ERROR_decompressionFailed",
    }
    .to_string()
}

// -------------------------------------------------------------------------------------------------
// Nomes de saída.

/// `FIO_createFilename_fromOutDir`: o último componente de `path` dentro de `out_dir`.
fn filename_in_dir(path: &[u8], out_dir: &[u8]) -> Vec<u8> {
    let mut v = out_dir.to_vec();
    if out_dir.last() != Some(&b'/') {
        v.push(b'/');
    }
    v.extend_from_slice(common::base_name(path));
    v
}

/// `trimPath`: tira um `./` e depois uma `/` do começo.
fn trim_path(p: &[u8]) -> &[u8] {
    let p = p.strip_prefix(b"./").unwrap_or(p);
    p.strip_prefix(b"/").unwrap_or(p)
}

/// `mallocAndJoin2Dir`.
fn join_dirs(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut v = a.to_vec();
    if a.last() != Some(&b'/') {
        v.push(b'/');
    }
    v.extend_from_slice(b);
    v
}

/// `convertPathnameToDirName`: o diretório do caminho, ou `.`.
fn dir_name(p: &[u8]) -> Vec<u8> {
    let mut p = p.to_vec();
    while p.len() > 1 && p.last() == Some(&b'/') {
        p.pop();
    }
    match p.iter().rposition(|&b| b == b'/') {
        Some(i) => p[..i].to_vec(),
        None => b".".to_vec(),
    }
}

/// `UTIL_createMirroredDestDirName`.
fn mirrored_dir_name(src: &[u8], root: &[u8]) -> Option<Vec<u8>> {
    if src.windows(2).any(|w| w == b"..") {
        return None;
    }
    Some(dir_name(&join_dirs(root, trim_path(src))))
}

/// `UTIL_mirrorSourceFilesDirectories`: cria a raiz e, sob ela, os diretórios das entradas com o
/// modo dos originais.
fn mirror_source_dirs(names: &[Vec<u8>], root: &[u8]) {
    let cur = sys::current();
    let valid: Vec<&Vec<u8>> = names.iter().filter(|n| !n.windows(2).any(|w| w == b"..")).collect();
    if valid.is_empty() {
        return;
    }
    let _ = cur.mkdirat(Fd::CWD, root, 0o755);
    for n in valid {
        let dir = dir_name(n);
        let dir = trim_path(dir.strip_prefix(b"./").unwrap_or(&dir)).to_vec();
        if dir == b"." {
            continue;
        }
        // Cada prefixo do caminho, na ordem, como o `mirrorSrcDirRecursive`.
        let mut ends: Vec<usize> = dir.iter().enumerate().filter(|&(i, &b)| b == b'/' && i > 0).map(|(i, _)| i).collect();
        ends.push(dir.len());
        for e in ends {
            let part = &dir[..e];
            let mode = stat_path(part).map(|st| st.mode & 0o7777).unwrap_or(0o755);
            let _ = cur.mkdirat(Fd::CWD, &join_dirs(root, trim_path(part)), mode);
        }
    }
}

/// `UTIL_isCompressedFile` com a lista do `--exclude-compressed`.
fn is_compressed_name(name: &[u8]) -> bool {
    let ext = match name.iter().rposition(|&b| b == b'.') {
        Some(0) | None => return false,
        Some(i) => &name[i..],
    };
    COMPRESSED_EXTENSIONS.iter().any(|e| e.as_bytes() == ext)
}

/// `FIO_determineCompressedName`.
fn compressed_name(src: &[u8], out_dir: Option<&[u8]>, suffix: &[u8]) -> Vec<u8> {
    if src == STDIN_MARK {
        return STDOUT_MARK.to_vec();
    }
    let mut v = match out_dir {
        Some(d) => filename_in_dir(src, d),
        None => src.to_vec(),
    };
    v.extend_from_slice(suffix);
    v
}

// -------------------------------------------------------------------------------------------------
// Compressão de arquivos (`FIO_compressFilename*`, `FIO_compressMultipleFilenames`).

/// `cRess_t`: o dicionário carregado e a identidade do arquivo dele.
struct CRess {
    dict: Option<Vec<u8>>,
    dict_stat: Option<Stat>,
}

/// Para onde vão os resultados de vários arquivos: `-o`, `--output-dir-flat` ou
/// `--output-dir-mirror` (ou ao lado de cada entrada, quando nenhum).
struct Dest<'a> {
    mirror: Option<&'a [u8]>,
    out_dir: Option<&'a [u8]>,
    out_name: Option<&'a [u8]>,
}

impl Zstd {
    fn should_display_file_summary(&self) -> bool {
        self.fctx.nb_files_total <= 1 || self.level >= 3
    }

    /// `FIO_createCResources`, na parte observável: o dicionário.
    fn create_cress(&self, dict_name: Option<&[u8]>) -> R<CRess> {
        self.disp(6, "FIO_createCResources \n");
        let r = match self.load_dict(dict_name)? {
            Some((d, st)) => CRess { dict: Some(d), dict_stat: Some(st) },
            None => CRess { dict: None, dict_stat: None },
        };
        self.disp(5, format!("set nb workers = {} \n", self.prefs.nb_workers as u32));
        Ok(r)
    }

    /// `FIO_compressFilename_internal`: um frame do formato escolhido mais o resumo.
    fn compress_internal(&mut self, ress: &CRess, dst: &[u8], src: &[u8], level: i32) -> R<()> {
        let start = sys::current().clock_gettime(sysabi::Clock::Monotonic).ok();
        let cpu_start = sys::current().clock_gettime(sysabi::Clock::ProcessCpuTime).ok();
        let size = file_size(src);
        self.disp(5, msg!(src, format!(": {size} bytes \n")));
        let known = (size != FILESIZE_UNKNOWN).then_some(size);
        let ctype = self.prefs.ctype;
        let pledged = match ctype {
            CType::Zstd => known.or((self.prefs.stream_src_size > 0).then_some(self.prefs.stream_src_size as u64)),
            _ => known,
        };
        let job = Job {
            level,
            checksum: self.prefs.checksum_flag != 0,
            content_size: self.prefs.content_size,
            dict_id: self.prefs.dict_id_flag,
            pledged,
            dict: ress.dict.as_deref(),
            window_log: self.prefs.window_log,
            file_size: size,
            trace: self.level,
        };
        if ctype == CType::Zstd {
            self.disp(6, "compression using zstd format \n");
            if self.level >= 4 {
                let wlog = if self.prefs.window_log != 0 {
                    self.prefs.window_log
                } else if self.prefs.ldm {
                    DEFAULT_MAX_WINDOW_LOG
                } else {
                    get_cparams(level, size, 0).window_log
                };
                let need = (1u64 << wlog).min(pledged.unwrap_or(u64::MAX)).max(1);
                self.disp(4, format!("Decompression will require {} of memory\n", self.hrs(need).show()));
            }
        }
        let mut read = 0u64;
        let (result, written, errno) = {
            let Zstd { rp, dst: d, out, .. } = &mut *self;
            let mut sink = Sink { dst: d, stdout: out, written: 0, error: None };
            let r = match ctype {
                CType::Zstd => encode_zstd(&job, &mut sink, rp, &mut read),
                CType::Gzip => encode_gzip(&job, &mut sink, rp, &mut read),
                CType::Xz => encode_lzma(&job, true, &mut sink, rp, &mut read),
                CType::Lzma => encode_lzma(&job, false, &mut sink, rp, &mut read),
                CType::Lz4 => encode_lz4(&job, &mut sink, rp, &mut read),
            };
            (r, sink.written, sink.error)
        };
        match result {
            Ok(()) => {}
            Err(EncError::Read) => return Err(self.throw(37, "Read error")),
            Err(EncError::Write) => {
                let e = errno.unwrap_or(Errno::EIO);
                return Err(self.throw(70, msg!("Write error : cannot write block : ", e.message())));
            }
            Err(EncError::Zstd(name)) => return Err(self.throw(11, name)),
            Err(EncError::GzipInit(code)) => {
                return Err(self.throw(71, msg!("zstd: ", src, ": deflateInit2 error ", code.to_string(), " \n")));
            }
            Err(EncError::Lzma) => return Err(self.throw(84, msg!("zstd: ", src, ": lzma_code encoding error 11"))),
            Err(EncError::Lz4(name)) => {
                return Err(self.throw(35, msg!("zstd: ", src, ": lz4 compression failed : ", name)));
            }
        }
        if ctype == CType::Zstd && size != FILESIZE_UNKNOWN && read != size {
            return Err(self.throw(
                27,
                format!("Read error : Incomplete read : {read} / {size} B"),
            ));
        }
        self.fctx.total_in += read;
        self.fctx.total_out += written;
        self.clear_progress();
        if self.should_display_file_summary() {
            let (hi, ho) = (self.hrs(read), self.hrs(written));
            let line = if read == 0 {
                msg!(ljust(src, 20), " :  (", hi.padded(6, 0), " => ", ho.padded(6, 0), ", ", dst, ") \n")
            } else {
                let pct = written as f64 / read as f64 * 100.0;
                msg!(
                    ljust(src, 20),
                    format!(" :{pct:6.2}%   ("),
                    hi.padded(6, 0),
                    " => ",
                    ho.padded(6, 0),
                    ", ",
                    dst,
                    ") \n"
                )
            };
            self.display_summary(line);
        }
        if self.level >= 4 {
            let now = sys::current().clock_gettime(sysabi::Clock::Monotonic).ok();
            let cpu = sys::current().clock_gettime(sysabi::Clock::ProcessCpuTime).ok();
            let secs = |a: Option<TimeSpec>, b: Option<TimeSpec>| match (a, b) {
                (Some(a), Some(b)) => (b.sec - a.sec) as f64 + (b.nsec - a.nsec) as f64 / 1e9,
                _ => 0.0,
            };
            let t = secs(start, now);
            let load = if t > 0.0 { secs(cpu_start, cpu) / t * 100.0 } else { 0.0 };
            self.disp(4, msg!(ljust(src, 20), format!(" : Completed in {t:.2} sec  (cpu load : {load:.0}%)\n")));
        }
        Ok(())
    }

    /// `FIO_compressFilename_dstFile`: abre o destino (se ainda não está aberto), comprime e passa
    /// dono, modo e datas.
    fn compress_dst_file(&mut self, ress: &CRess, dst: &[u8], src: &[u8], src_stat: Option<&Stat>, level: i32) -> R<i32> {
        let mut close = false;
        let mut transfer = false;
        if matches!(self.dst, Dst::Closed) {
            let mut perm = DEFAULT_FILE_PERMISSIONS;
            if src != STDIN_MARK && dst != STDOUT_MARK && src_stat.is_some_and(|s| s.file_type() == FileType::Regular) {
                transfer = true;
                perm = TEMPORARY_FILE_PERMISSIONS;
            }
            close = true;
            self.disp(6, msg!("FIO_compressFilename_dstFile: opening dst: ", dst, " \n"));
            if !self.open_dst(Some(src), dst, perm)? {
                return Ok(1);
            }
        }
        self.compress_internal(ress, dst, src, level)?;
        let mut result = 0;
        if close {
            if transfer
                && let (Dst::File { fd, .. }, Some(st)) = (&self.dst, src_stat)
            {
                self.set_fd_stat(*fd, dst, st);
            }
            self.disp(6, msg!("FIO_compressFilename_dstFile: closing dst: ", dst, " \n"));
            if let Some(e) = self.close_dst() {
                self.disp(1, msg!("zstd: ", dst, ": ", e.message(), " \n"));
                result = 1;
            }
            if transfer && let Some(st) = src_stat {
                self.utime(dst, st);
            }
            if result != 0 && dst != STDOUT_MARK {
                let _ = self.remove_file(dst);
            }
        }
        Ok(result)
    }

    /// `FIO_compressFilename_srcFile`.
    fn compress_src_file(&mut self, ress: &CRess, dst: &[u8], src: &[u8], level: i32) -> R<i32> {
        self.disp(6, msg!("FIO_compressFilename_srcFile: ", src, " \n"));
        if src != STDIN_MARK
            && let Ok(st) = stat_path(src)
        {
            if st.file_type() == FileType::Directory {
                self.disp(1, msg!("zstd: ", src, " is a directory -- ignored \n"));
                return Ok(1);
            }
            if let Some(ds) = &ress.dict_stat
                && same_file(&st, ds)
            {
                self.disp(1, msg!("zstd: cannot use ", src, " as an input file and dictionary \n"));
                return Ok(1);
            }
        }
        if self.prefs.exclude_compressed && is_compressed_name(src) {
            self.disp(4, msg!("File is already compressed : ", src, " \n"));
            return Ok(0);
        }
        let Some((fd, st)) = self.open_src(src, self.prefs.allow_block_devices) else { return Ok(1) };
        self.rp.set_file(Some(fd));
        let result = self.compress_dst_file(ress, dst, src, st.as_ref(), level);
        self.rp.set_file(None);
        if fd != Fd::STDIN {
            common::close(fd);
        }
        let result = result?;
        if self.prefs.remove_src
            && result == 0
            && src != STDIN_MARK
            && let Err(e) = self.remove_file(src)
        {
            return Err(self.throw(1, msg!("zstd: ", src, ": ", e.message())));
        }
        Ok(result)
    }

    /// `FIO_compressFilename`: um arquivo com destino dado.
    fn compress_filename(&mut self, dst: &[u8], src: &[u8], dict: Option<&[u8]>, level: i32) -> R<i32> {
        let ress = self.create_cress(dict)?;
        self.compress_src_file(&ress, dst, src, level)
    }

    /// `FIO_compressMultipleFilenames`.
    fn compress_multiple(
        &mut self,
        names: &[Vec<u8>],
        dest: &Dest,
        suffix: &[u8],
        dict: Option<&[u8]>,
        level: i32,
    ) -> R<i32> {
        let (mirror, out_dir, out_name) = (dest.mirror, dest.out_dir, dest.out_name);
        let ress = self.create_cress(dict)?;
        let mut error = 0;
        if let Some(out_name) = out_name {
            if self.multi_files_concat_warning(Some(out_name), 1)? {
                return Ok(1);
            }
            if !self.open_dst(None, out_name, DEFAULT_FILE_PERMISSIONS)? {
                error = 1;
            } else {
                while self.fctx.curr < self.fctx.nb_files_total {
                    let src = names[self.fctx.curr].clone();
                    let status = self.compress_src_file(&ress, out_name, &src, level)?;
                    if status == 0 {
                        self.fctx.processed += 1;
                    }
                    error |= status;
                    self.fctx.curr += 1;
                }
                if let Some(e) = self.close_dst() {
                    return Err(self.throw(
                        29,
                        msg!("Write error (", e.message(), ") : cannot properly close ", out_name),
                    ));
                }
            }
        } else {
            if let Some(root) = mirror {
                mirror_source_dirs(names, root);
            }
            while self.fctx.curr < self.fctx.nb_files_total {
                let src = names[self.fctx.curr].clone();
                let dst = match mirror {
                    Some(root) => match mirrored_dir_name(&src, root) {
                        Some(d) => compressed_name(&src, Some(&d), suffix),
                        None => {
                            self.disp(
                                2,
                                msg!("zstd: --output-dir-mirror cannot compress '", src, "' into '", root, "' \n"),
                            );
                            error = 1;
                            self.fctx.curr += 1;
                            continue;
                        }
                    },
                    None => compressed_name(&src, out_dir, suffix),
                };
                let status = self.compress_src_file(&ress, &dst, &src, level)?;
                if status == 0 {
                    self.fctx.processed += 1;
                }
                error |= status;
                self.fctx.curr += 1;
            }
            if out_dir.is_some() {
                self.check_filename_collisions(names);
            }
        }
        if self.fctx.processed >= 1 && self.fctx.nb_files_total > 1 {
            let (hi, ho) = (self.hrs(self.fctx.total_in), self.hrs(self.fctx.total_out));
            self.clear_progress();
            let n = self.fctx.processed;
            let line = if self.fctx.total_in == 0 {
                format!("{n:3} files compressed : ({} => {})\n", hi.padded(6, 4), ho.padded(6, 4))
            } else {
                let pct = self.fctx.total_out as f64 / self.fctx.total_in as f64 * 100.0;
                format!("{n:3} files compressed : {pct:.2}% ({} => {})\n", hi.padded(6, 4), ho.padded(6, 4))
            };
            self.display_summary(line);
        }
        Ok(error)
    }
}

// -------------------------------------------------------------------------------------------------
// Descompressão de frames (`FIO_decompress*Frame`, `FIO_decompressFrames`).

/// Frame que falhou (`FIO_ERROR_FRAME_DECODING`); a mensagem já saiu.
struct FrameError;

impl Zstd {
    /// `FIO_zstdErrorHelp`: ajuda pra janela grande demais.
    fn zstd_error_help(&self, err: zstd_dec::Error, src: &[u8]) {
        if err != zstd_dec::Error::WindowTooLarge {
            return;
        }
        let mut fh = zstd_dec::FrameHeader::default();
        if zstd_dec::get_frame_header(&mut fh, self.rp.loaded()) == Ok(0) {
            let w = fh.window_size;
            let log = (63 - w.leading_zeros()) + u32::from(w & (w - 1) != 0);
            self.disp(
                1,
                msg!(src, format!(" : Window size larger than maximum : {w} > {} \n", self.prefs.mem_limit)),
            );
            if log <= WINDOWLOG_MAX {
                let mb = (w >> 20) + u64::from(w & ((1 << 20) - 1) != 0);
                self.disp(1, msg!(src, format!(" : Use --long={log} or --memory={mb}MB \n")));
                return;
            }
        }
        self.disp(1, msg!(src, format!(" : Window log larger than ZSTD_WINDOWLOG_MAX={WINDOWLOG_MAX}; not supported \n")));
    }

    /// `FIO_decompressZstdFrame`: um frame zstd, gravando cada pedaço que o decodificador entrega.
    fn decompress_zstd_frame(&mut self, src: &[u8]) -> R<Result<u64, FrameError>> {
        let mut frame_size = 0u64;
        let mut dctx = self.dctx.take().unwrap_or_else(|| DStream::new(u64::from(self.prefs.mem_limit)));
        dctx.reset_session();
        let r = (|| {
            self.fill(zstd_dec::FRAMEHEADERSIZE_MAX)?;
            loop {
                let mut out = Vec::new();
                let mut pos = 0usize;
                let hint = dctx.decompress_stream(&mut out, zstd_dec::DSTREAM_OUT_SIZE, self.rp.loaded(), &mut pos);
                let hint = match hint {
                    Ok(h) => h,
                    Err(e) => {
                        self.disp(1, msg!(src, " : Decoding error (36) : ", e.name(), " \n"));
                        self.zstd_error_help(e, src);
                        return Ok(Err(FrameError));
                    }
                };
                self.write_out(&out)?;
                frame_size += out.len() as u64;
                self.rp.consume(pos);
                if hint == 0 {
                    return Ok(Ok(frame_size));
                }
                let to_decode = hint.min(zstd_dec::DSTREAM_IN_SIZE);
                if self.rp.loaded().len() < to_decode && self.fill(to_decode)? == 0 {
                    self.disp(1, msg!(src, " : Read error (39) : premature end \n"));
                    return Ok(Err(FrameError));
                }
            }
        })();
        self.dctx = Some(dctx);
        r
    }

    /// `FIO_decompressGzFrame`: um membro gzip, com o `inflate` do zlib.
    fn decompress_gz_frame(&mut self, src: &[u8]) -> R<Result<u64, FrameError>> {
        use flate2::{Decompress, FlushDecompress, Status};
        let mut z = Decompress::new_gzip(15);
        let mut out_size = 0u64;
        let mut out = vec![0u8; zstd_dec::DSTREAM_OUT_SIZE];
        let mut finish = false;
        let mut error = false;
        loop {
            sys::checkpoint();
            if self.rp.loaded().is_empty() {
                self.rp.consume(0);
                self.fill(zstd_dec::DSTREAM_IN_SIZE)?;
                if self.rp.loaded().is_empty() {
                    finish = true;
                }
            }
            let before_in = z.total_in();
            let before_out = z.total_out();
            let flush = if finish { FlushDecompress::Finish } else { FlushDecompress::None };
            let r = z.decompress(self.rp.loaded(), &mut out, flush);
            let used = (z.total_in() - before_in) as usize;
            let made = (z.total_out() - before_out) as usize;
            self.rp.consume(used);
            let status = match r {
                Ok(s) => s,
                Err(_) => {
                    self.disp(1, msg!("zstd: ", src, ": inflate error -3 \n"));
                    error = true;
                    break;
                }
            };
            if made > 0 {
                self.write_out(&out[..made])?;
                out_size += made as u64;
            }
            match status {
                Status::StreamEnd => break,
                Status::BufError if finish || (used == 0 && made == 0 && self.rp.loaded().is_empty() && finish) => {
                    self.disp(1, msg!("zstd: ", src, ": premature gz end \n"));
                    error = true;
                    break;
                }
                _ => {
                    if finish && used == 0 && made == 0 {
                        self.disp(1, msg!("zstd: ", src, ": premature gz end \n"));
                        error = true;
                        break;
                    }
                }
            }
        }
        Ok(if error { Err(FrameError) } else { Ok(out_size) })
    }

    /// `FIO_decompressLzmaFrame`: um fluxo xz ou lzma. O decodificador do crate lê o fluxo inteiro
    /// de uma vez; o que sobra depois dele volta pro pool.
    fn decompress_lzma_frame(&mut self, src: &[u8], plain_lzma: bool) -> R<Result<u64, FrameError>> {
        // O fluxo pode estar no meio do pool; junta tudo que resta do arquivo.
        let mut data = self.rp.loaded().to_vec();
        self.rp.consume(data.len());
        loop {
            self.fill(zstd_dec::DSTREAM_IN_SIZE)?;
            let more = self.rp.loaded().to_vec();
            if more.is_empty() {
                break;
            }
            self.rp.consume(more.len());
            data.extend_from_slice(&more);
        }
        let mut cursor = io::Cursor::new(&data[..]);
        let fmt = if plain_lzma { codec::Format::Lzma } else { codec::Format::Xz };
        let mut out = vec![0u8; zstd_dec::DSTREAM_OUT_SIZE];
        let mut total = 0u64;
        let mut err = None;
        match codec::decoder(fmt, &mut cursor) {
            Err(e) => err = Some(e),
            Ok(mut dec) => loop {
                sys::checkpoint();
                match dec.read(&mut out) {
                    Ok(0) => break,
                    Ok(n) => {
                        total += n as u64;
                        let chunk = out[..n].to_vec();
                        self.write_out(&chunk)?;
                    }
                    Err(e) => {
                        err = Some(e);
                        break;
                    }
                }
            },
        }
        // O que sobrou depois do fluxo é o próximo frame; o leitor do xz não consome além do fim.
        let used = cursor.position() as usize;
        self.rp.buf = data[used.min(data.len())..].to_vec();
        self.rp.pos = 0;
        if let Some(e) = err {
            if codec::classify_io(&e) == codec::DecodeError::Truncated {
                self.disp(1, msg!("zstd: ", src, ": premature lzma end \n"));
            } else {
                let code = if codec::classify_io(&e) == codec::DecodeError::NotFormat { 7 } else { 9 };
                self.disp(1, msg!("zstd: ", src, format!(": lzma_code decoding error {code} \n")));
            }
            return Ok(Err(FrameError));
        }
        Ok(Ok(total))
    }

    /// `FIO_decompressLz4Frame`.
    fn decompress_lz4_frame(&mut self, src: &[u8]) -> R<Result<u64, FrameError>> {
        let mut data = self.rp.loaded().to_vec();
        self.rp.consume(data.len());
        loop {
            self.fill(zstd_dec::DSTREAM_IN_SIZE)?;
            let more = self.rp.loaded().to_vec();
            if more.is_empty() {
                break;
            }
            self.rp.consume(more.len());
            data.extend_from_slice(&more);
        }
        let mut cursor = io::Cursor::new(&data[..]);
        let mut dec = lz4_flex::frame::FrameDecoder::new(&mut cursor);
        let mut out = vec![0u8; 64 << 10];
        let mut total = 0u64;
        let mut failure = None;
        loop {
            sys::checkpoint();
            match dec.read(&mut out) {
                Ok(0) => break,
                Ok(n) => {
                    total += n as u64;
                    let chunk = out[..n].to_vec();
                    self.write_out(&chunk)?;
                }
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
        }
        drop(dec);
        let used = cursor.position() as usize;
        self.rp.buf = data[used.min(data.len())..].to_vec();
        self.rp.pos = 0;
        if let Some(e) = failure {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                self.disp(1, msg!("zstd: ", src, ": unfinished lz4 stream \n"));
            } else {
                let name = e
                    .get_ref()
                    .and_then(|i| i.downcast_ref::<lz4_flex::frame::Error>())
                    .map(lz4_error_name)
                    .unwrap_or_else(|| "ERROR_decompressionFailed".to_string());
                self.disp(1, msg!("zstd: ", src, ": lz4 decompression error : ", name, " \n"));
            }
            return Ok(Err(FrameError));
        }
        Ok(Ok(total))
    }

    /// `FIO_passThrough`: copia a entrada como está.
    fn pass_through(&mut self) -> R<i32> {
        let block = 64 << 10;
        self.fill(block)?;
        while !self.rp.loaded().is_empty() {
            let n = block.min(self.rp.loaded().len());
            let chunk = self.rp.loaded()[..n].to_vec();
            self.write_out(&chunk)?;
            self.rp.consume(n);
            self.fill(block)?;
        }
        Ok(0)
    }

    /// `FIO_decompressFrames`: decodifica os frames do arquivo, de qualquer formato suportado.
    fn decompress_frames(&mut self, dst: &[u8], src: &[u8]) -> R<i32> {
        let mut read_something = false;
        let mut filesize = 0u64;
        let pass = match self.prefs.pass_through {
            -1 => self.prefs.overwrite && dst == STDOUT_MARK,
            p => p == 1,
        };
        loop {
            self.fill(4)?;
            let buf = self.rp.loaded();
            if buf.is_empty() {
                if !read_something {
                    self.disp(1, msg!("zstd: ", src, ": unexpected end of file \n"));
                    return Ok(1);
                }
                break;
            }
            read_something = true;
            if buf.len() < 4 {
                if pass {
                    return self.pass_through();
                }
                self.disp(1, msg!("zstd: ", src, ": unknown header \n"));
                return Ok(1);
            }
            let magic = zstd_dec::le32(buf);
            let frame = if magic == zstd_dec::MAGIC
                || magic & zstd_dec::MAGIC_SKIPPABLE_MASK == zstd_dec::MAGIC_SKIPPABLE_START
            {
                self.decompress_zstd_frame(src)?
            } else if buf[0] == 31 && buf[1] == 139 {
                self.decompress_gz_frame(src)?
            } else if (buf[0] == 0xFD && buf[1] == 0x37) || (buf[0] == 0x5D && buf[1] == 0) {
                let plain = buf[0] != 0xFD;
                self.decompress_lzma_frame(src, plain)?
            } else if magic == LZ4_MAGIC {
                self.decompress_lz4_frame(src)?
            } else if pass {
                return self.pass_through();
            } else {
                self.disp(1, msg!("zstd: ", src, ": unsupported format \n"));
                return Ok(1);
            };
            match frame {
                Ok(n) => filesize += n,
                Err(FrameError) => return Ok(1),
            }
        }
        self.fctx.total_out += filesize;
        self.clear_progress();
        if self.should_display_file_summary() {
            self.display_summary(msg!(ljust(src, 20), format!(": {filesize} bytes \n")));
        }
        Ok(0)
    }
}

// -------------------------------------------------------------------------------------------------
// Descompressão de arquivos (`FIO_decompressFilename`, `FIO_decompressMultipleFilenames`).

/// `FIO_determineDstName`: tira o sufixo conhecido (`.tzst`, `.tgz`... viram `.tar`).
fn decompressed_name(src: &[u8], out_dir: Option<&[u8]>) -> Result<Vec<u8>, ()> {
    if src == STDIN_MARK {
        return Ok(STDOUT_MARK.to_vec());
    }
    let Some(dot) = src.iter().rposition(|&b| b == b'.') else { return Err(()) };
    let suffix = &src[dot..];
    let Some(matched) = SUFFIX_LIST.iter().find(|s| **s == suffix) else { return Err(()) };
    if src.len() <= suffix.len() {
        return Err(());
    }
    let base = match out_dir {
        Some(d) => filename_in_dir(src, d),
        None => src.to_vec(),
    };
    let mut v = base[..base.len() - suffix.len()].to_vec();
    if matched[1] == b't' {
        v.extend_from_slice(b".tar");
    }
    Ok(v)
}

impl Zstd {
    /// `FIO_determineDstName` com a mensagem de sufixo desconhecido.
    fn determine_dst_name(&self, src: &[u8], out_dir: Option<&[u8]>) -> Option<Vec<u8>> {
        let r = decompressed_name(src, out_dir).ok();
        if r.is_none() {
            self.disp(
                1,
                msg!(
                    "zstd: ",
                    src,
                    ": unknown suffix (",
                    SUFFIX_LIST_STR,
                    " expected). Can't derive the output file name. Specify it with -o dstFileName. Ignoring.\n"
                ),
            );
        }
        r
    }

    /// `FIO_createDResources`: o contexto com o limite de janela, o checksum e o dicionário.
    fn create_dress(&mut self, dict_name: Option<&[u8]>) -> R<()> {
        let mut d = DStream::new(u64::from(self.prefs.mem_limit));
        d.set_ignore_checksum(self.prefs.checksum_flag == 0);
        if let Some((bytes, _)) = self.load_dict(dict_name)? {
            let dict = if self.prefs.patch_from {
                Dict::raw(&bytes)
            } else {
                match Dict::parse(&bytes) {
                    Ok(d) => d,
                    Err(e) => return Err(self.throw(11, e.name())),
                }
            };
            d.set_dict(Some(dict));
        }
        self.dctx = Some(d);
        Ok(())
    }

    /// `FIO_decompressDstFile`.
    fn decompress_dst_file(&mut self, dst: &[u8], src: &[u8], src_stat: Option<&Stat>) -> R<i32> {
        let mut release = false;
        let mut transfer = false;
        if matches!(self.dst, Dst::Closed) && !self.prefs.test_mode {
            let mut perm = DEFAULT_FILE_PERMISSIONS;
            if src != STDIN_MARK && dst != STDOUT_MARK && src_stat.is_some_and(|s| s.file_type() == FileType::Regular) {
                transfer = true;
                perm = TEMPORARY_FILE_PERMISSIONS;
            }
            release = true;
            if !self.open_dst(Some(src), dst, perm)? {
                return Ok(1);
            }
        }
        if self.prefs.test_mode && matches!(self.dst, Dst::Closed) {
            self.dst = Dst::Test;
        }
        let mut result = self.decompress_frames(dst, src)?;
        if self.prefs.test_mode && matches!(self.dst, Dst::Test) {
            self.dst = Dst::Closed;
        }
        if release {
            if transfer
                && let (Dst::File { fd, .. }, Some(st)) = (&self.dst, src_stat)
            {
                self.set_fd_stat(*fd, dst, st);
            }
            if let Some(e) = self.close_dst() {
                self.disp(1, msg!("zstd: ", dst, ": ", e.message(), " \n"));
                result = 1;
            }
            if transfer && let Some(st) = src_stat {
                self.utime(dst, st);
            }
            if result != 0 && dst != STDOUT_MARK {
                let _ = self.remove_file(dst);
            }
        }
        Ok(result)
    }

    /// `FIO_decompressSrcFile`.
    fn decompress_src_file(&mut self, dst: &[u8], src: &[u8]) -> R<i32> {
        if is_directory(src) {
            self.disp(1, msg!("zstd: ", src, " is a directory -- ignored \n"));
            return Ok(1);
        }
        let Some((fd, st)) = self.open_src(src, self.prefs.allow_block_devices) else { return Ok(1) };
        self.rp.set_file(Some(fd));
        let result = self.decompress_dst_file(dst, src, st.as_ref());
        self.rp.set_file(None);
        if fd != Fd::STDIN {
            common::close(fd);
        }
        let result = result?;
        if self.prefs.remove_src
            && result == 0
            && src != STDIN_MARK
            && let Err(e) = self.remove_file(src)
        {
            self.disp(1, msg!("zstd: ", src, ": ", e.message(), " \n"));
            return Ok(1);
        }
        Ok(result)
    }

    /// `FIO_decompressFilename`.
    fn decompress_filename(&mut self, dst: &[u8], src: &[u8], dict: Option<&[u8]>) -> R<i32> {
        self.create_dress(dict)?;
        self.decompress_src_file(dst, src)
    }

    /// `FIO_decompressMultipleFilenames`.
    fn decompress_multiple(&mut self, names: &[Vec<u8>], dest: &Dest, dict: Option<&[u8]>) -> R<i32> {
        let (mirror, out_dir, out_name) = (dest.mirror, dest.out_dir, dest.out_name);
        self.create_dress(dict)?;
        let mut error = 0;
        if let Some(out_name) = out_name {
            if self.multi_files_concat_warning(Some(out_name), 1)? {
                return Ok(1);
            }
            if !self.prefs.test_mode && !self.open_dst(None, out_name, DEFAULT_FILE_PERMISSIONS)? {
                return Err(self.throw(19, msg!("cannot open ", out_name)));
            }
            while self.fctx.curr < self.fctx.nb_files_total {
                let src = names[self.fctx.curr].clone();
                let status = self.decompress_src_file(out_name, &src)?;
                if status == 0 {
                    self.fctx.processed += 1;
                }
                error |= status;
                self.fctx.curr += 1;
            }
            if !self.prefs.test_mode
                && let Some(e) = self.close_dst()
            {
                return Err(self.throw(
                    72,
                    msg!("Write error : ", e.message(), " : cannot properly close output file"),
                ));
            }
        } else {
            if let Some(root) = mirror {
                mirror_source_dirs(names, root);
            }
            while self.fctx.curr < self.fctx.nb_files_total {
                let src = names[self.fctx.curr].clone();
                let dst = match mirror {
                    Some(root) => match mirrored_dir_name(&src, root) {
                        Some(d) => self.determine_dst_name(&src, Some(&d)),
                        None => {
                            self.disp(
                                2,
                                msg!("zstd: --output-dir-mirror cannot decompress '", src, "' into '", root, "'\n"),
                            );
                            None
                        }
                    },
                    None => self.determine_dst_name(&src, out_dir),
                };
                let Some(dst) = dst else {
                    error = 1;
                    self.fctx.curr += 1;
                    continue;
                };
                let status = self.decompress_src_file(&dst, &src)?;
                if status == 0 {
                    self.fctx.processed += 1;
                }
                error |= status;
                self.fctx.curr += 1;
            }
            if out_dir.is_some() {
                self.check_filename_collisions(names);
            }
        }
        if self.fctx.processed >= 1 && self.fctx.nb_files_total > 1 {
            self.clear_progress();
            let line = format!("{} files decompressed : {:6} bytes total \n", self.fctx.processed, self.fctx.total_out);
            self.display_summary(line);
        }
        Ok(error)
    }
}

// -------------------------------------------------------------------------------------------------
// `--list` (`FIO_listMultipleFiles`).

/// `fileInfo_t`.
#[derive(Default, Clone)]
struct FileInfo {
    decompressed: u64,
    compressed: u64,
    window: u64,
    frames: u32,
    skippable: u32,
    decomp_unavailable: bool,
    uses_check: bool,
    checksum: [u8; 4],
    nb_files: u32,
    dict_id: u32,
}

/// `InfoError`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InfoError {
    Success,
    Frame,
    NotZstd,
    File,
    Truncated,
}

/// O arquivo aberto como `FILE*`: posição que o `fseek` pode levar além do fim.
struct SeekFile {
    data: Vec<u8>,
    pos: u64,
    eof: bool,
}

impl SeekFile {
    fn fread(&mut self, n: usize) -> Vec<u8> {
        let start = self.pos.min(self.data.len() as u64) as usize;
        let end = (start + n).min(self.data.len());
        let v = self.data[start..end].to_vec();
        if v.len() < n {
            self.eof = true;
        }
        self.pos = (start + v.len()) as u64;
        if self.pos < self.data.len() as u64 {
            self.eof = false;
        }
        v
    }

    /// `fseek(SEEK_CUR)`: falha só se a posição ficasse negativa; limpa o fim de arquivo.
    fn seek_cur(&mut self, off: i64) -> bool {
        let p = self.pos as i64 + off;
        if p < 0 {
            return false;
        }
        self.pos = p as u64;
        self.eof = false;
        true
    }
}

impl Zstd {
    /// `ERROR_IF` do `--list`: mensagem no stderr com o ` \n` final.
    fn info_error(&self, m: impl AsRef<[u8]>, e: InfoError) -> InfoError {
        self.disp(1, msg!(m, " \n"));
        e
    }

    /// `FIO_analyzeFrames`.
    fn analyze_frames(&self, info: &mut FileInfo, f: &mut SeekFile) -> InfoError {
        loop {
            sys::checkpoint();
            let header = f.fread(zstd_dec::FRAMEHEADERSIZE_MAX);
            let n = header.len();
            if n < 6 {
                if f.eof && n == 0 && info.compressed > 0 && info.compressed != FILESIZE_UNKNOWN {
                    if f.pos != info.compressed {
                        return self.info_error(
                            format!(
                                "Error: seeked to position {}, which is beyond file size of {}\n",
                                f.pos, info.compressed
                            ),
                            InfoError::Truncated,
                        );
                    }
                    return InfoError::Success;
                }
                if f.eof {
                    return self.info_error("Error: reached end of file with incomplete frame", InfoError::NotZstd);
                }
                return self.info_error("Error: did not reach end of file but ran out of frames", InfoError::Frame);
            }
            let magic = zstd_dec::le32(&header);
            if magic == zstd_dec::MAGIC {
                let mut fh = zstd_dec::FrameHeader::default();
                let r = zstd_dec::get_frame_header(&mut fh, &header);
                // `ZSTD_getFrameContentSize`: erro ou desconhecido deixam o total indisponível.
                match r {
                    Ok(0) if fh.content_size != zstd_dec::CONTENTSIZE_UNKNOWN => info.decompressed += fh.content_size,
                    _ => info.decomp_unavailable = true,
                }
                if r != Ok(0) {
                    return self.info_error("Error: could not decode frame header", InfoError::Frame);
                }
                if info.dict_id != 0 && info.dict_id != fh.dict_id {
                    common::eprint(
                        "WARNING: File contains multiple frames with different dictionary IDs. Showing dictID 0 instead",
                    );
                    info.dict_id = 0;
                } else {
                    info.dict_id = fh.dict_id;
                }
                info.window = fh.window_size;
                if !f.seek_cur(fh.header_size as i64 - n as i64) {
                    return self.info_error("Error: could not move to end of frame header", InfoError::Frame);
                }
                loop {
                    let bh = f.fread(3);
                    if bh.len() != 3 {
                        return self.info_error("Error while reading block header", InfoError::Frame);
                    }
                    let h = u32::from(bh[0]) | u32::from(bh[1]) << 8 | u32::from(bh[2]) << 16;
                    let btype = (h >> 1) & 3;
                    if btype == 3 {
                        return self.info_error("Error: unsupported block type", InfoError::Frame);
                    }
                    let size = if btype == 1 { 1 } else { i64::from(h >> 3) };
                    if !f.seek_cur(size) {
                        return self.info_error("Error: could not skip to end of block", InfoError::Frame);
                    }
                    if h & 1 == 1 {
                        break;
                    }
                }
                if header[4] & 4 != 0 {
                    info.uses_check = true;
                    let c = f.fread(4);
                    if c.len() != 4 {
                        return self.info_error("Error: could not read checksum", InfoError::Frame);
                    }
                    info.checksum.copy_from_slice(&c);
                }
                info.frames += 1;
            } else if magic & zstd_dec::MAGIC_SKIPPABLE_MASK == zstd_dec::MAGIC_SKIPPABLE_START {
                let size = i64::from(zstd_dec::le32(&header[4..]));
                if !f.seek_cur(8 + size - n as i64) {
                    return self.info_error("Error: could not find end of skippable frame", InfoError::Frame);
                }
                info.skippable += 1;
            } else {
                return InfoError::NotZstd;
            }
        }
    }

    /// `getFileInfo`.
    fn get_file_info(&self, info: &mut FileInfo, name: &[u8]) -> InfoError {
        if !is_regular(name) {
            return self.info_error(msg!("Error : ", name, " is not a file"), InfoError::File);
        }
        let Some((fd, st)) = self.open_src(name, false) else {
            return self.info_error(msg!("Error: could not open source file ", name), InfoError::File);
        };
        info.compressed = st.as_ref().map_or(FILESIZE_UNKNOWN, file_size_stat);
        let data = common::read_all(fd).unwrap_or_default();
        common::close(fd);
        let mut f = SeekFile { data, pos: 0, eof: false };
        let status = self.analyze_frames(info, &mut f);
        info.nb_files = 1;
        status
    }

    /// `displayInfo`.
    fn display_info(&mut self, name: &[u8], info: &FileInfo, level: i32) {
        let (wh, ch, dh) = (self.hrs(info.window), self.hrs(info.compressed), self.hrs(info.decompressed));
        let ratio = if info.compressed == 0 { 0.0 } else { info.decompressed as f64 / info.compressed as f64 };
        let check = if info.uses_check { "XXH64" } else { "None" };
        let mut o = Vec::new();
        if level <= 2 {
            let frames = info.frames + info.skippable;
            if !info.decomp_unavailable {
                o.extend(msg!(
                    format!(
                        "{frames:6}  {:5}  {}  {}  {ratio:5.3}  {check:>5}  ",
                        info.skippable,
                        ch.padded(6, 4),
                        dh.padded(8, 4)
                    ),
                    name,
                    "\n"
                ));
            } else {
                o.extend(msg!(
                    format!("{frames:6}  {:5}  {}                       {check:>5}  ", info.skippable, ch.padded(6, 4)),
                    name,
                    "\n"
                ));
            }
        } else {
            o.extend(msg!(name, " \n"));
            o.extend(format!("# Zstandard Frames: {}\n", info.frames).bytes());
            if info.skippable > 0 {
                o.extend(format!("# Skippable Frames: {}\n", info.skippable).bytes());
            }
            o.extend(format!("DictID: {}\n", info.dict_id).bytes());
            o.extend(format!("Window Size: {} ({} B)\n", wh.show(), info.window).bytes());
            o.extend(format!("Compressed Size: {} ({} B)\n", ch.show(), info.compressed).bytes());
            if !info.decomp_unavailable {
                o.extend(format!("Decompressed Size: {} ({} B)\n", dh.show(), info.decompressed).bytes());
                o.extend(format!("Ratio: {ratio:.4}\n").bytes());
            }
            if info.uses_check && info.frames == 1 {
                let c = info.checksum;
                o.extend(format!("Check: {check} {:02x}{:02x}{:02x}{:02x}\n", c[3], c[2], c[1], c[0]).bytes());
            } else {
                o.extend(format!("Check: {check}\n").bytes());
            }
            o.extend(b"\n");
        }
        self.out.write(&o);
    }

    /// `FIO_listFile`.
    fn list_file(&mut self, total: &mut FileInfo, name: &[u8], level: i32) -> i32 {
        let mut info = FileInfo::default();
        let error = self.get_file_info(&mut info, name);
        match error {
            InfoError::Frame => self.disp(1, msg!("Error while parsing \"", name, "\" \n")),
            InfoError::NotZstd => {
                self.out.write(&msg!("File \"", name, "\" not compressed by zstd \n"));
                if level > 2 {
                    self.out.write(b"\n");
                }
                return 1;
            }
            InfoError::File => {
                if level > 2 {
                    self.out.write(b"\n");
                }
                return 1;
            }
            InfoError::Truncated => {
                self.out.write(&msg!("File \"", name, "\" is truncated \n"));
                if level > 2 {
                    self.out.write(b"\n");
                }
                return 1;
            }
            InfoError::Success => {}
        }
        self.display_info(name, &info, level);
        total.frames += info.frames;
        total.skippable += info.skippable;
        total.compressed += info.compressed;
        total.decompressed += info.decompressed;
        total.decomp_unavailable |= info.decomp_unavailable;
        total.uses_check &= info.uses_check;
        total.nb_files += info.nb_files;
        i32::from(error == InfoError::Frame)
    }

    /// `FIO_listMultipleFiles`.
    fn list_multiple(&mut self, names: &[Vec<u8>]) -> i32 {
        let level = self.level;
        if names.iter().any(|n| n == STDIN_MARK) {
            self.disp(1, "zstd: --list does not support reading from standard input \n");
            return 1;
        }
        if names.is_empty() {
            if !self.console_stdin() {
                self.disp(1, "zstd: --list does not support reading from standard input \n");
            }
            self.disp(1, "No files given \n");
            return 1;
        }
        if level <= 2 {
            self.out.write(b"Frames  Skips  Compressed  Uncompressed  Ratio  Check  Filename\n");
        }
        let mut total = FileInfo { uses_check: true, ..FileInfo::default() };
        let mut error = 0;
        for n in names {
            error |= self.list_file(&mut total, n, level);
        }
        if names.len() > 1 && level <= 2 {
            let (ch, dh) = (self.hrs(total.compressed), self.hrs(total.decompressed));
            let ratio = if total.compressed == 0 { 0.0 } else { total.decompressed as f64 / total.compressed as f64 };
            let check = if total.uses_check { "XXH64" } else { "" };
            let frames = total.skippable + total.frames;
            let mut o = b"----------------------------------------------------------------- \n".to_vec();
            if total.decomp_unavailable {
                o.extend(
                    format!(
                        "{frames:6}  {:5}  {}                       {check:>5}  {} files\n",
                        total.skippable,
                        ch.padded(6, 4),
                        total.nb_files
                    )
                    .bytes(),
                );
            } else {
                o.extend(
                    format!(
                        "{frames:6}  {:5}  {}  {}  {ratio:5.3}  {check:>5}  {} files\n",
                        total.skippable,
                        ch.padded(6, 4),
                        dh.padded(8, 4),
                        total.nb_files
                    )
                    .bytes(),
                );
            }
            self.out.write(&o);
        }
        error
    }
}

// -------------------------------------------------------------------------------------------------
// `--train` (`programs/dibio.c`), com os treinadores do `structured-zstd`.

const SAMPLESIZE_MAX: u64 = 128 << 10;
const MAX_SAMPLES_SIZE: u64 = 2 << 30;

/// Parâmetros do treino vindos da linha de comando.
struct TrainParams {
    kind: DictKind,
    /// `--train-cover`/`--train-fastcover` com `k`, `d`, ... (zero pede busca).
    k: u32,
    d: u32,
    f: u32,
    steps: u32,
    split: Option<f64>,
    accel: u32,
    shrink: Option<u32>,
    selectivity: u32,
    dict_id: u32,
    level: i32,
    max_dict_size: u32,
    chunk_size: usize,
    mem_limit: u32,
}

/// `DiB_rand` e `DiB_shuffle`: a mesma permutação do C, com a semente fixa.
fn dib_shuffle(names: &mut [Vec<u8>]) {
    let mut seed: u32 = 0xFD2F_B528;
    let mut rand = || {
        let mut r = seed.wrapping_mul(2_654_435_761);
        r ^= 2_246_822_519;
        r = r.rotate_left(13);
        seed = r;
        r >> 5
    };
    for i in (1..names.len()).rev() {
        let j = rand() as usize % (i + 1);
        names.swap(i, j);
    }
}

/// Tamanho de um arquivo regular, ou -1 (`DiB_getFileSize`).
fn dib_file_size(name: &[u8]) -> i64 {
    match file_size(name) {
        FILESIZE_UNKNOWN => -1,
        s => s as i64,
    }
}

impl Zstd {
    /// `EXM_THROW` do `dibio.c`, que imprime sem o `zstd: ` e sem depender do nível.
    fn dib_throw(&self, code: i32, m: impl AsRef<[u8]>) -> Exit {
        common::eprint(msg!("Error ", code.to_string(), " : ", m, "\n"));
        Exit(code)
    }

    /// `DiB_trainFromFiles`.
    fn train_from_files(&self, out_name: &[u8], names: &[Vec<u8>], p: &TrainParams) -> R<i32> {
        use structured_zstd::dictionary as zd;
        let max_dict = p.max_dict_size as usize;
        let mut names = names.to_vec();
        self.disp(3, "Shuffling input files\n");
        dib_shuffle(&mut names);
        // `DiB_fileStats`.
        let (mut total, mut nb_samples, mut too_large) = (0i64, 0usize, false);
        for n in &names {
            let size = dib_file_size(n);
            if size == 0 {
                self.disp(3, msg!("Sample file '", n, "' has zero size, skipping...\n"));
                continue;
            }
            if p.chunk_size > 0 {
                nb_samples += ((size + p.chunk_size as i64 - 1) / p.chunk_size as i64).max(0) as usize;
                total += size;
            } else {
                if size > SAMPLESIZE_MAX as i64 {
                    too_large |= size > 2 * SAMPLESIZE_MAX as i64;
                    self.disp(
                        3,
                        msg!("Sample file '", n, format!("' is too large, limiting to {} KB\n", SAMPLESIZE_MAX >> 10)),
                    );
                }
                nb_samples += 1;
                total += size.min(SAMPLESIZE_MAX as i64);
            }
        }
        self.disp(4, format!("Found training data {} files, {} KB, {} samples\n", names.len(), total >> 10, nb_samples));
        let mut loaded_size = total.clamp(0, MAX_SAMPLES_SIZE as i64) as u64;
        if p.mem_limit != 0 {
            self.disp(
                2,
                format!(
                    "!  Warning : setting manual memory limit for dictionary training data at {} MB \n",
                    p.mem_limit >> 20
                ),
            );
            loaded_size = loaded_size.min(u64::from(p.mem_limit));
        }
        if too_large {
            self.disp(2, "!  Warning : some sample(s) are very large \n");
            self.disp(2, "!  Note that dictionary is only useful for small samples. \n");
            self.disp(
                2,
                format!("!  As a consequence, only the first {SAMPLESIZE_MAX} bytes of each sample are loaded \n"),
            );
        }
        if nb_samples < 5 {
            self.disp(2, "!  Warning : nb of samples too low for proper processing ! \n");
            self.disp(2, "!  Please provide _one file per sample_. \n");
            self.disp(
                2,
                "!  Alternatively, split files into fixed-size blocks representative of samples, with -B# \n",
            );
            return Err(self.dib_throw(14, "nb of samples too low"));
        }
        if total < i64::from(p.max_dict_size) * 8 {
            self.disp(2, "!  Warning : data size of samples too small for target dictionary size \n");
            self.disp(2, "!  Samples should be about 100x larger than target dictionary size \n");
        }
        if (loaded_size as i64) < total {
            self.disp(
                1,
                format!(
                    "Training samples set too large ({} MB); training on {} MB only...\n",
                    total >> 20,
                    loaded_size >> 20
                ),
            );
        }
        // `DiB_loadFiles`.
        let mut samples = Vec::new();
        let mut sizes = Vec::new();
        for n in &names {
            if sizes.len() >= nb_samples {
                break;
            }
            let size = dib_file_size(n);
            if size <= 0 {
                continue;
            }
            let fd = match common::open_input(n, false) {
                Ok(fd) => fd,
                Err(e) => return Err(self.dib_throw(10, msg!("zstd: dictBuilder: ", n, " ", e.message(), " "))),
            };
            let data = common::read_all(fd);
            common::close(fd);
            let Ok(data) = data else { return Err(self.dib_throw(11, msg!("Pb reading ", n))) };
            let size = size as usize;
            let first = if p.chunk_size > 0 { size.min(p.chunk_size) } else { size.min(SAMPLESIZE_MAX as usize) };
            if samples.len() + first > loaded_size as usize {
                break;
            }
            if data.len() < first {
                return Err(self.dib_throw(11, msg!("Pb reading ", n)));
            }
            samples.extend_from_slice(&data[..first]);
            sizes.push(first);
            if p.chunk_size > 0 {
                let mut done = first;
                while done < size && sizes.len() < nb_samples {
                    let chunk = (size - done).min(p.chunk_size);
                    if samples.len() + chunk > loaded_size as usize || data.len() < done + chunk {
                        break;
                    }
                    samples.extend_from_slice(&data[done..done + chunk]);
                    sizes.push(chunk);
                    done += chunk;
                }
            }
        }
        self.disp(2, format!("\r{:79}\r", ""));
        self.disp(4, format!("Loaded {} KB total training data, {} nb samples \n", samples.len() >> 10, sizes.len()));
        let finalize = zd::FinalizeOptions { dict_id: (p.dict_id != 0).then_some(p.dict_id), level: p.level };
        let cover = |split_default: f64| zd::CoverOptions {
            k: p.k,
            d: p.d,
            steps: p.steps,
            split_point: p.split.unwrap_or(split_default),
            shrink: p.shrink,
        };
        let optimize = p.k == 0 || p.d == 0;
        let result: io::Result<Vec<u8>> = match p.kind {
            DictKind::Legacy => {
                let mut out = Vec::new();
                zd::create_legacy_dict_from_slice(&samples, &sizes, &mut out, max_dict, p.selectivity, finalize)
                    .map(|()| out)
            }
            DictKind::Cover if optimize => {
                zd::optimize_cover_dict(&samples, &sizes, max_dict, &cover(1.0), finalize).map(|(d, o)| {
                    self.disp(
                        2,
                        format!(
                            "k={}\nd={}\nsteps={}\nsplit={}\n",
                            o.k,
                            o.d,
                            o.steps,
                            (o.split_point * 100.0) as u32
                        ),
                    );
                    d
                })
            }
            DictKind::Cover => zd::train_cover_dict(&samples, &sizes, max_dict, &cover(1.0), finalize),
            DictKind::FastCover => {
                let opts = zd::FastCoverOptions { cover: cover(0.75), f: p.f, accel: p.accel };
                if optimize {
                    zd::optimize_fastcover_dict(&samples, &sizes, max_dict, &opts, finalize).map(|(d, o)| {
                        self.disp(
                            2,
                            format!(
                                "k={}\nd={}\nf={}\nsteps={}\nsplit={}\naccel={}\n",
                                o.cover.k,
                                o.cover.d,
                                o.f,
                                o.cover.steps,
                                (o.cover.split_point * 100.0) as u32,
                                o.accel
                            ),
                        );
                        d
                    })
                } else {
                    zd::train_fastcover_dict(&samples, &sizes, max_dict, &opts, finalize)
                }
            }
        };
        let dict = match result {
            Ok(d) => d,
            Err(e) => {
                let name = if e.to_string().contains("too small") {
                    "Destination buffer is too small"
                } else {
                    "Error (generic)"
                };
                self.disp(1, format!("dictionary training failed : {name} \n"));
                return Ok(1);
            }
        };
        self.disp(2, msg!(format!("Save dictionary of size {} into file ", dict.len()), out_name, " \n"));
        let fd = match common::create_truncate(out_name, 0o666) {
            Ok(fd) => fd,
            Err(_) => return Err(self.dib_throw(3, msg!("cannot open ", out_name, " "))),
        };
        if sys::write_all(fd, &dict).is_err() {
            common::close(fd);
            return Err(self.dib_throw(4, msg!(out_name, " : write error")));
        }
        if sys::close(fd).is_err() {
            return Err(self.dib_throw(5, msg!(out_name, " : flush error")));
        }
        Ok(0)
    }
}

// -------------------------------------------------------------------------------------------------
// Ajuda e versão (`usage`, `usageAdvanced`, `printVersion`).

const WELCOME: &str = "*** Zstandard CLI (64-bit) v1.5.7, by Yann Collet ***\n";

const USAGE_OPTIONS: &str = "Options:
  -o OUTPUT                     Write output to a single file, OUTPUT.
  -k, --keep                    Preserve INPUT file(s). [Default]\x20
  --rm                          Remove INPUT file(s) after successful (de)compression.
";

const USAGE_REST: &str = "
  -#                            Desired compression level, where `#` is a number between 1 and 19;
                                lower numbers provide faster compression, higher numbers yield
                                better compression ratios. [Default: 3]

  -d, --decompress              Perform decompression.
  -D DICT                       Use DICT as the dictionary for compression or decompression.

  -f, --force                   Disable input and output checks. Allows overwriting existing files,
                                receiving input from the console, printing output to STDOUT, and
                                operating on links, block devices, etc. Unrecognized formats will be
                                passed-through through as-is.

  -h                            Display short usage and exit.
  -H, --help                    Display full help and exit.
  -V, --version                 Display the program version and exit.

";

const ADVANCED_1: &str = "Advanced options:
  -c, --stdout                  Write to STDOUT (even if it is a console) and keep the INPUT file(s).

  -v, --verbose                 Enable verbose output; pass multiple times to increase verbosity.
  -q, --quiet                   Suppress warnings; pass twice to suppress errors.
  --trace LOG                   Log tracing information to LOG.

  --[no-]progress               Forcibly show/hide the progress counter. NOTE: Any (de)compressed
                                output to terminal will mix with progress counter text.

  -r                            Operate recursively on directories.
  --filelist LIST               Read a list of files to operate on from LIST.
  --output-dir-flat DIR         Store processed files in DIR.
  --output-dir-mirror DIR       Store processed files in DIR, respecting original directory structure.
  --[no-]asyncio                Use asynchronous IO. [Default: Enabled]

  --[no-]check                  Add XXH64 integrity checksums during compression. [Default: Add, Validate]
                                If `-d` is present, ignore/validate checksums during decompression.

  --                            Treat remaining arguments after `--` as files.

Advanced compression options:
  --ultra                       Enable levels beyond 19, up to 22; requires more memory.
  --fast[=#]                    Use to very fast compression levels. [Default: 1]
";

const ADVANCED_2: &str = "  --adapt                       Dynamically adapt compression level to I/O conditions.
  --long[=#]                    Enable long distance matching with window log #. [Default: 27]
  --patch-from=REF              Use REF as the reference point for Zstandard's diff engine.\x20

  -T#                           Spawn # compression threads. [Default: 1; pass 0 for core count.]
  --single-thread               Share a single thread for I/O and compression (slightly different than `-T1`).
  --auto-threads={physical|logical}
                                Use physical/logical cores when using `-T0`. [Default: Physical]

  -B#                           Set job size to #. [Default: 0 (automatic)]
  --rsyncable                   Compress using a rsync-friendly method (`-B` sets block size).\x20

  --exclude-compressed          Only compress files that are not already compressed.

  --stream-size=#               Specify size of streaming input from STDIN.
  --size-hint=#                 Optimize compression parameters for streaming input of approximately size #.
  --target-compressed-block-size=#
                                Generate compressed blocks of approximately # size.

  --no-dictID                   Don't write `dictID` into the header (dictionary compression only).
  --[no-]compress-literals      Force (un)compressed literals.
  --[no-]row-match-finder       Explicitly enable/disable the fast, row-based matchfinder for
                                the 'greedy', 'lazy', and 'lazy2' strategies.

  --format=zstd                 Compress files to the `.zst` format. [Default]
  --[no-]mmap-dict              Memory-map dictionary file rather than mallocing and loading all at once
  --format=gzip                 Compress files to the `.gz` format.
  --format=xz                   Compress files to the `.xz` format.
  --format=lzma                 Compress files to the `.lzma` format.
  --format=lz4                 Compress files to the `.lz4` format.

Advanced decompression options:
  -l                            Print information about Zstandard-compressed files.
  --test                        Test compressed file integrity.
  -M#                           Set the memory usage limit to # megabytes.
  --[no-]sparse                 Enable sparse mode. [Default: Enabled for files, disabled for STDOUT.]
";

const ADVANCED_3: &str = "
Dictionary builder:
  --train                       Create a dictionary from a training set of files.

  --train-cover[=k=#,d=#,steps=#,split=#,shrink[=#]]
                                Use the cover algorithm (with optional arguments).
  --train-fastcover[=k=#,d=#,f=#,steps=#,split=#,accel=#,shrink[=#]]
                                Use the fast cover algorithm (with optional arguments).

  --train-legacy[=s=#]          Use the legacy algorithm with selectivity #. [Default: 9]
  -o NAME                       Use NAME as dictionary name. [Default: dictionary]
  --maxdict=#                   Limit dictionary to specified size #. [Default: 112640]
  --dictID=#                    Force dictionary ID to #. [Default: Random]

Benchmark options:
  -b#                           Perform benchmarking with compression level #. [Default: 3]
  -e#                           Test all compression levels up to #; starting level is `-b#`. [Default: 1]
  -i#                           Set the minimum evaluation to time # seconds. [Default: 3]
  -B#                           Cut file into independent chunks of size #. [Default: No chunking]
  -S                            Output one benchmark result per input file. [Default: Consolidated result]
  -D dictionary                 Benchmark using dictionary\x20
  --priority=rt                 Set process priority to real-time.
";

/// `usage`: o texto curto, com o nome do programa.
fn usage_text(prog: &[u8]) -> Vec<u8> {
    let mut v = msg!(
        "Compress or decompress the INPUT file(s); reads from STDIN if INPUT is `-` or not provided.\n\n",
        "Usage: ",
        prog,
        " [OPTIONS...] [INPUT... | -] [-o OUTPUT]\n\n",
        USAGE_OPTIONS
    );
    if exe_name_match(prog, "gzip") {
        v.extend_from_slice(b"  -n, --no-name                 Do not store original filename when compressing.\n\n");
    }
    v.extend_from_slice(USAGE_REST.as_bytes());
    v
}

/// `usageAdvanced`.
fn usage_advanced_text(prog: &[u8]) -> Vec<u8> {
    let mut v = msg!(WELCOME, "\n", usage_text(prog), ADVANCED_1);
    if exe_name_match(prog, "gzip") {
        v.extend_from_slice(b"  --best                        Compatibility alias for `-9`.\n");
    }
    v.extend_from_slice(ADVANCED_2.as_bytes());
    let pass = if ["zstdcat", "zcat", "gzcat"].iter().any(|n| exe_name_match(prog, n)) { "Enabled" } else { "Disabled" };
    v.extend(
        format!("  --[no-]pass-through           Pass through uncompressed files as-is. [Default: {pass}]\n").bytes(),
    );
    v.extend_from_slice(ADVANCED_3.as_bytes());
    v
}

/// `printVersion`.
fn version_text(level: i32) -> String {
    if level < 2 {
        return "1.5.7\n".to_string();
    }
    let mut s = WELCOME.to_string();
    if level >= 3 {
        s.push_str("*** supports: zstd, zstd legacy v0.5+, gzip, lz4, lzma, xz \n");
        if level >= 4 {
            s.push_str("zlib version 1.3.1\nlz4 version 1.10.0\nlzma version 5.8.1\n");
            s.push_str("_POSIX_C_SOURCE defined: 200809L\n_POSIX_VERSION defined: 200809L \n");
            s.push_str("PLATFORM_POSIX_VERSION defined: 200809L\n");
        }
    }
    s
}

// -------------------------------------------------------------------------------------------------
// Linha de comando (o `main` do `zstdcli.c`).

/// Parâmetros de um treinador cover/fastcover (`ZDICT_cover_params_t`, `ZDICT_fastCover_params_t`).
#[derive(Clone, Copy)]
struct CoverArgs {
    k: u32,
    d: u32,
    f: u32,
    steps: u32,
    split: f64,
    accel: u32,
    shrink: Option<u32>,
}

impl CoverArgs {
    const ZERO: CoverArgs = CoverArgs { k: 0, d: 0, f: 0, steps: 0, split: 0.0, accel: 0, shrink: None };
}

/// As variáveis locais do `main`.
struct Cli {
    prog: Vec<u8>,
    follow_links: bool,
    force_stdin: bool,
    force_stdout: bool,
    ldm: bool,
    pause: bool,
    adapt: bool,
    adapt_min: i32,
    adapt_max: i32,
    rsyncable: bool,
    next_are_files: bool,
    single_thread: bool,
    logical_cores: bool,
    show_default_cparams: bool,
    ultra: bool,
    nb_workers: i64,
    block_size: u64,
    op: Op,
    cparams: CParams,
    clevel: i32,
    clevel_last: i32,
    recursive: bool,
    mem_limit: u32,
    filenames: Vec<Vec<u8>>,
    file_lists: Vec<Vec<u8>>,
    out_name: Option<Vec<u8>>,
    out_dir: Option<Vec<u8>>,
    out_mirror: Option<Vec<u8>>,
    dict_name: Option<Vec<u8>>,
    patch_from: Option<Vec<u8>>,
    suffix: &'static [u8],
    max_dict_size: u32,
    dict_id: u32,
    dict_clevel: i32,
    dict_select: u32,
    dict_kind: DictKind,
    cover: CoverArgs,
    fast_cover: CoverArgs,
    bench_seconds: u32,
    /// `ZSTD_ParamSwitch_e` do `--[no-]row-match-finder` (0 auto, 1 ligado, 2 desligado).
    row_match: u8,
    /// O mesmo pro `--[no-]compress-literals`.
    literals: u8,
    size_hint: u64,
    target_cblock: u64,
    /// `-P#`: compressibilidade do dado sintético (negativo = lorem ipsum).
    compressibility: f64,
    /// `-p#`: parâmetro extra que o `-b -q` imprime.
    additional_param: i32,
    /// `-S`: um resultado por arquivo.
    separate_files: bool,
    /// `-d` junto do `-b`: mede só a descompressão.
    decode_only: bool,
    /// `--priority=rt`.
    real_time: bool,
}

/// Erro de número no meio da linha de comando (`errorOut`).
const OVERFLOW_U32: &str = "error: numeric value overflows 32-bit unsigned int";
const OVERFLOW_I32: &str = "error: numeric value overflows 32-bit int";
const OVERFLOW_SIZE: &str = "error: numeric value overflows size_t";
const ONLY_NUMERIC: &str = "error: only numeric values with optional suffixes K, KB, KiB, M, MB, MiB are allowed";

/// `longCommandWArg`: `arg` começa com `prefix`; devolve o resto.
fn long_arg<'a>(arg: &'a [u8], prefix: &str) -> Option<&'a [u8]> {
    arg.strip_prefix(prefix.as_bytes())
}

impl Zstd {
    /// `errorOut`.
    fn error_out(&self, m: &str) -> Exit {
        self.disp(1, format!("{m} \n"));
        Exit(1)
    }

    /// `badUsage`.
    fn bad_usage(&self, prog: &[u8], param: &[u8]) -> Exit {
        self.disp(1, msg!("Incorrect parameter: ", param, " \n"));
        if self.level >= 2 {
            common::eprint(usage_text(prog));
        }
        Exit(1)
    }

    fn u32_of(&self, s: &[u8], pos: &mut usize) -> R<u32> {
        read_u32(s, pos).ok_or_else(|| self.error_out(OVERFLOW_U32))
    }

    /// `readIntFromChar`.
    fn i32_of(&self, s: &[u8], pos: &mut usize) -> R<i32> {
        let neg = s.get(*pos) == Some(&b'-');
        if neg {
            *pos += 1;
        }
        let v = read_u32(s, pos).ok_or_else(|| self.error_out(OVERFLOW_I32))? as i32;
        Ok(if neg { v.wrapping_neg() } else { v })
    }

    /// `NEXT_FIELD`: o valor depois de `=` no próprio argumento, ou o argumento seguinte. Devolve o
    /// valor e se ele veio do próprio argumento (que então foi consumido inteiro).
    fn next_field(&self, args: &[Vec<u8>], idx: &mut usize, rest: &[u8]) -> R<(Vec<u8>, bool)> {
        if let Some(v) = rest.strip_prefix(b"=") {
            return Ok((v.to_vec(), true));
        }
        *idx += 1;
        let Some(next) = args.get(*idx) else {
            self.disp(1, "error: missing command argument \n");
            return Err(Exit(1));
        };
        if next.first() == Some(&b'-') {
            self.disp(1, "error: command cannot be separated from its argument by another command \n");
            return Err(Exit(1));
        }
        Ok((next.clone(), false))
    }

    /// `NEXT_UINT32`.
    fn next_u32(&self, args: &[Vec<u8>], idx: &mut usize, rest: &[u8]) -> R<u32> {
        let (v, _) = self.next_field(args, idx, rest)?;
        let mut p = 0;
        let n = self.u32_of(&v, &mut p)?;
        if p != v.len() {
            return Err(self.error_out(ONLY_NUMERIC));
        }
        Ok(n)
    }

    /// `NEXT_TSIZE`.
    fn next_size(&self, args: &[Vec<u8>], idx: &mut usize, rest: &[u8]) -> R<u64> {
        let (v, _) = self.next_field(args, idx, rest)?;
        let mut p = 0;
        let n = read_size(&v, &mut p).ok_or_else(|| self.error_out(OVERFLOW_SIZE))?;
        if p != v.len() {
            return Err(self.error_out(ONLY_NUMERIC));
        }
        Ok(n)
    }

    /// `parseCoverParameters` e `parseFastCoverParameters`: `None` quando malformado.
    fn parse_cover(&self, s: &[u8], fast: bool) -> R<Option<CoverArgs>> {
        let mut p = CoverArgs::ZERO;
        let mut i = 0;
        loop {
            let rest = &s[i..];
            let keys: &[(&str, u8)] = &[("k=", b'k'), ("d=", b'd'), ("f=", b'f'), ("steps=", b's'), ("accel=", b'a')];
            let mut matched = false;
            for &(key, which) in keys {
                if (which == b'f' || which == b'a') && !fast {
                    continue;
                }
                if rest.starts_with(key.as_bytes()) {
                    i += key.len();
                    let v = self.u32_of(s, &mut i)?;
                    match which {
                        b'k' => p.k = v,
                        b'd' => p.d = v,
                        b'f' => p.f = v,
                        b's' => p.steps = v,
                        _ => p.accel = v,
                    }
                    matched = true;
                    break;
                }
            }
            if !matched {
                if rest.starts_with(b"split=") {
                    i += 6;
                    p.split = f64::from(self.u32_of(s, &mut i)?) / 100.0;
                } else if rest.starts_with(b"shrink") {
                    i += 6;
                    p.shrink = Some(1);
                    if s.get(i) == Some(&b'=') {
                        i += 1;
                        p.shrink = Some(self.u32_of(s, &mut i)?);
                    }
                } else {
                    return Ok(None);
                }
            }
            if s.get(i) == Some(&b',') {
                i += 1;
                continue;
            }
            break;
        }
        Ok((i == s.len()).then_some(p))
    }

    /// `parseLegacyParameters`.
    fn parse_legacy(&self, s: &[u8]) -> R<Option<u32>> {
        let rest = long_arg(s, "s=").or_else(|| long_arg(s, "selectivity="));
        let Some(rest) = rest else { return Ok(None) };
        let mut i = 0;
        let v = self.u32_of(rest, &mut i)?;
        Ok((i == rest.len()).then_some(v))
    }

    /// `parseAdaptParameters`.
    fn parse_adapt(&self, s: &[u8], min: &mut i32, max: &mut i32) -> R<bool> {
        let mut i = 0;
        loop {
            let rest = &s[i..];
            if rest.starts_with(b"min=") {
                i += 4;
                *min = self.i32_of(s, &mut i)?;
            } else if rest.starts_with(b"max=") {
                i += 4;
                *max = self.i32_of(s, &mut i)?;
            } else {
                self.disp(4, "invalid compression parameter \n");
                return Ok(false);
            }
            if s.get(i) == Some(&b',') {
                i += 1;
                continue;
            }
            break;
        }
        if i != s.len() {
            return Ok(false);
        }
        if *min > *max {
            self.disp(4, "incoherent adaptation limits \n");
            return Ok(false);
        }
        Ok(true)
    }

    /// `parseCompressionParameters`; os parâmetros de LDM e overlap só são aceitos (o codificador do
    /// crate não tem esses ajustes).
    fn parse_cparams(&self, s: &[u8], cp: &mut CParams) -> R<bool> {
        let mut i = 0;
        loop {
            let rest = &s[i..];
            let fields: &[(&str, &str, u8)] = &[
                ("windowLog=", "wlog=", b'w'),
                ("chainLog=", "clog=", b'c'),
                ("hashLog=", "hlog=", b'h'),
                ("searchLog=", "slog=", b's'),
                ("minMatch=", "mml=", b'm'),
                ("targetLength=", "tlen=", b't'),
                ("strategy=", "strat=", b'S'),
                ("overlapLog=", "ovlog=", b'O'),
                ("ldmHashLog=", "lhlog=", b'o'),
                ("ldmMinMatch=", "lmml=", b'o'),
                ("ldmBucketSizeLog=", "lblog=", b'o'),
                ("ldmHashRateLog=", "lhrlog=", b'o'),
            ];
            let hit = fields.iter().find_map(|&(a, b, w)| {
                if rest.starts_with(a.as_bytes()) {
                    Some((a.len(), w))
                } else if rest.starts_with(b.as_bytes()) {
                    Some((b.len(), w))
                } else {
                    None
                }
            });
            let Some((len, which)) = hit else {
                self.disp(4, "invalid compression parameter \n");
                return Ok(false);
            };
            i += len;
            let v = self.u32_of(s, &mut i)?;
            match which {
                b'w' => cp.window_log = v,
                b'c' => cp.chain_log = v,
                b'h' => cp.hash_log = v,
                b's' => cp.search_log = v,
                b'm' => cp.min_match = v,
                b't' => cp.target_length = v,
                b'S' => cp.strategy = v,
                b'O' => cp.overlap_log = Some(v),
                _ => {}
            }
            if s.get(i) == Some(&b',') {
                i += 1;
                continue;
            }
            break;
        }
        Ok(i == s.len())
    }

    /// `init_cLevel`: o nível do `ZSTD_CLEVEL`.
    fn init_clevel(&self) -> i32 {
        let Some(env) = common::getenv("ZSTD_CLEVEL") else { return CLEVEL_DEFAULT };
        let mut i = 0;
        let mut sign = 1i32;
        match env.first() {
            Some(b'-') => {
                sign = -1;
                i = 1;
            }
            Some(b'+') => i = 1,
            _ => {}
        }
        if env.get(i).is_some_and(u8::is_ascii_digit) {
            match read_u32(&env, &mut i) {
                None => {
                    self.disp(
                        2,
                        msg!("Ignore environment variable setting ZSTD_CLEVEL=", env, ": numeric value too large \n"),
                    );
                    return CLEVEL_DEFAULT;
                }
                Some(v) if i == env.len() => return sign.wrapping_mul(v as i32),
                Some(_) => {}
            }
        }
        self.disp(2, msg!("Ignore environment variable setting ZSTD_CLEVEL=", env, ": not a valid integer value \n"));
        CLEVEL_DEFAULT
    }

    /// `default_nbThreads`: o `ZSTD_NBTHREADS`, ou `ZSTDCLI_NBTHREADS_DEFAULT`
    /// (`MAX(1, MIN(4, núcleos lógicos / 4))`).
    fn default_nb_threads(&self) -> i64 {
        let cores = sys::current().sched_getaffinity().len() as i64;
        let fallback = (cores / 4).clamp(1, 4);
        let Some(env) = common::getenv("ZSTD_NBTHREADS") else { return fallback };
        if env.first().is_some_and(u8::is_ascii_digit) {
            let mut i = 0;
            match read_u32(&env, &mut i) {
                None => {
                    self.disp(
                        2,
                        msg!("Ignore environment variable setting ZSTD_NBTHREADS=", env, ": numeric value too large \n"),
                    );
                    return fallback;
                }
                Some(v) if i == env.len() => return i64::from(v),
                Some(_) => {}
            }
        }
        self.disp(
            2,
            msg!("Ignore environment variable setting ZSTD_NBTHREADS=", env, ": not a valid unsigned value \n"),
        );
        fallback
    }

    /// Lê a linha de comando. `Err(Exit(n))` quando o programa termina ali (ajuda, versão, erro).
    fn parse_args(&mut self, args: &[Vec<u8>]) -> R<Cli> {
        let prog = common::base_name(args.first().map_or(&b"zstd"[..], |a| a)).to_vec();
        let clevel = self.init_clevel();
        let mut c = Cli {
            prog: prog.clone(),
            follow_links: false,
            force_stdin: false,
            force_stdout: false,
            ldm: false,
            pause: false,
            adapt: false,
            adapt_min: MIN_CLEVEL,
            adapt_max: MAX_CLEVEL,
            rsyncable: false,
            next_are_files: false,
            single_thread: false,
            logical_cores: false,
            show_default_cparams: false,
            ultra: false,
            nb_workers: -1,
            block_size: 0,
            op: Op::Compress,
            cparams: CParams::default(),
            clevel,
            clevel_last: MIN_CLEVEL - 1,
            recursive: false,
            mem_limit: 0,
            filenames: Vec::new(),
            file_lists: Vec::new(),
            out_name: None,
            out_dir: None,
            out_mirror: None,
            dict_name: None,
            patch_from: None,
            suffix: b".zst",
            max_dict_size: DEFAULT_MAX_DICT_SIZE,
            dict_id: 0,
            dict_clevel: DEFAULT_DICT_CLEVEL,
            dict_select: DEFAULT_SELECTIVITY,
            dict_kind: DictKind::FastCover,
            cover: CoverArgs { k: 0, d: 8, f: 0, steps: 4, split: 1.0, accel: 0, shrink: None },
            fast_cover: CoverArgs { k: 0, d: 8, f: 20, steps: 4, split: 0.75, accel: 1, shrink: None },
            bench_seconds: 3,
            row_match: 0,
            literals: 0,
            size_hint: 0,
            target_cblock: 0,
            compressibility: -1.0,
            additional_param: 0,
            separate_files: false,
            decode_only: false,
            real_time: false,
        };
        let is = |n: &str| exe_name_match(&prog, n);
        // Os nomes com que o binário muda de comportamento.
        if is("zstdmt") {
            c.nb_workers = 0;
            c.single_thread = false;
        }
        if is("unzstd") {
            c.op = Op::Decompress;
        }
        if is("zstdcat") || is("zcat") || is("gzcat") {
            c.op = Op::Decompress;
            self.prefs.overwrite = true;
            c.force_stdout = true;
            c.follow_links = true;
            self.prefs.pass_through = 1;
            c.out_name = Some(STDOUT_MARK.to_vec());
            self.level = 1;
        }
        if is("gzip") {
            c.suffix = b".gz";
            self.prefs.ctype = CType::Gzip;
            self.prefs.remove_src = true;
            c.clevel = 6;
            c.dict_clevel = 6;
        }
        if is("gunzip") {
            c.op = Op::Decompress;
            self.prefs.remove_src = true;
        }
        if is("lzma") {
            c.suffix = b".lzma";
            self.prefs.ctype = CType::Lzma;
            self.prefs.remove_src = true;
        }
        if is("unlzma") {
            c.op = Op::Decompress;
            self.prefs.ctype = CType::Lzma;
            self.prefs.remove_src = true;
        }
        if is("xz") {
            c.suffix = b".xz";
            self.prefs.ctype = CType::Xz;
            self.prefs.remove_src = true;
        }
        if is("unxz") {
            c.op = Op::Decompress;
            self.prefs.ctype = CType::Xz;
            self.prefs.remove_src = true;
        }
        if is("lz4") {
            c.suffix = b".lz4";
            self.prefs.ctype = CType::Lz4;
        }
        if is("unlz4") {
            c.op = Op::Decompress;
            self.prefs.ctype = CType::Lz4;
        }

        let mut idx = 1;
        while idx < args.len() {
            let arg = args[idx].clone();
            if c.next_are_files {
                c.filenames.push(arg);
                idx += 1;
                continue;
            }
            if arg == b"-" {
                c.filenames.push(STDIN_MARK.to_vec());
                idx += 1;
                continue;
            }
            if arg.first() != Some(&b'-') {
                c.filenames.push(arg);
                idx += 1;
                continue;
            }
            if arg.get(1) == Some(&b'-') {
                self.parse_long(args, &mut idx, &arg, &mut c)?;
                idx += 1;
                continue;
            }
            self.parse_short(args, &mut idx, &arg, &mut c)?;
            idx += 1;
        }
        Ok(c)
    }

    /// Uma opção longa (`--...`).
    fn parse_long(&mut self, args: &[Vec<u8>], idx: &mut usize, arg: &[u8], c: &mut Cli) -> R<()> {
        let a = arg;
        let prog = c.prog.clone();
        let exact = |s: &str| a == s.as_bytes();
        let p = &mut self.prefs;
        match a {
            b"--" => c.next_are_files = true,
            b"--list" => c.op = Op::List,
            b"--compress" => c.op = Op::Compress,
            b"--decompress" | b"--uncompress" => c.op = Op::Decompress,
            b"--force" => {
                p.overwrite = true;
                c.force_stdin = true;
                c.force_stdout = true;
                c.follow_links = true;
                p.allow_block_devices = true;
            }
            b"--version" => {
                self.out.write_str(&version_text(self.level));
                return Err(Exit(0));
            }
            b"--help" => {
                self.out.write(&usage_advanced_text(&prog));
                return Err(Exit(0));
            }
            b"--verbose" => self.level += 1,
            b"--quiet" => self.level -= 1,
            b"--stdout" => {
                c.force_stdout = true;
                c.out_name = Some(STDOUT_MARK.to_vec());
            }
            b"--ultra" => c.ultra = true,
            b"--check" => p.checksum_flag = 2,
            b"--no-check" => p.checksum_flag = 0,
            b"--sparse" => p.sparse = 2,
            b"--no-sparse" => p.sparse = 0,
            b"--asyncio" | b"--no-asyncio" | b"--trace-file-stat" | b"--mmap-dict" | b"--no-mmap-dict" => {}
            b"--priority=rt" => c.real_time = true,
            b"--no-row-match-finder" => c.row_match = 2,
            b"--row-match-finder" => c.row_match = 1,
            b"--compress-literals" => c.literals = 1,
            b"--no-compress-literals" => c.literals = 2,
            b"--pass-through" => p.pass_through = 1,
            b"--no-pass-through" => p.pass_through = 0,
            b"--test" => c.op = Op::Test,
            b"--train" => {
                c.op = Op::Train;
                c.out_name.get_or_insert_with(|| b"dictionary".to_vec());
            }
            b"--no-dictID" => p.dict_id_flag = false,
            b"--keep" => p.remove_src = false,
            b"--rm" => p.remove_src = true,
            b"--show-default-cparams" => c.show_default_cparams = true,
            b"--content-size" => p.content_size = true,
            b"--no-content-size" => p.content_size = false,
            b"--adapt" => c.adapt = true,
            b"--single-thread" => {
                c.nb_workers = 0;
                c.single_thread = true;
            }
            b"--format=zstd" => {
                c.suffix = b".zst";
                p.ctype = CType::Zstd;
            }
            b"--format=gzip" => {
                c.suffix = b".gz";
                p.ctype = CType::Gzip;
            }
            b"--format=lzma" => {
                c.suffix = b".lzma";
                p.ctype = CType::Lzma;
            }
            b"--format=xz" => {
                c.suffix = b".xz";
                p.ctype = CType::Xz;
            }
            b"--format=lz4" => {
                c.suffix = b".lz4";
                p.ctype = CType::Lz4;
            }
            b"--rsyncable" => c.rsyncable = true,
            b"--no-progress" => self.progress = Progress::Never,
            b"--progress" => self.progress = Progress::Always,
            b"--exclude-compressed" => p.exclude_compressed = true,
            b"--fake-stdin-is-console" => self.fake_stdin_console = true,
            b"--fake-stdout-is-console" => self.fake_stdout_console = true,
            b"--fake-stderr-is-console" => self.fake_stderr_console = true,
            b"--max" => {
                c.ultra = true;
                c.ldm = true;
                c.cparams = CParams {
                    window_log: WINDOWLOG_MAX,
                    chain_log: 30,
                    hash_log: 30,
                    search_log: 30,
                    min_match: 3,
                    target_length: 1 << 17,
                    strategy: 9,
                    overlap_log: Some(9),
                };
            }
            _ if exact("--best") && exe_name_match(&prog, "gzip") => {
                c.clevel = 9;
                c.dict_clevel = 9;
            }
            _ if exact("--no-name") && exe_name_match(&prog, "gzip") => {}
            _ => return self.parse_long_with_arg(args, idx, a, c),
        }
        Ok(())
    }

    /// As opções longas com argumento, na ordem de prefixos do C.
    fn parse_long_with_arg(&mut self, args: &[Vec<u8>], idx: &mut usize, a: &[u8], c: &mut Cli) -> R<()> {
        let prog = c.prog.clone();
        if let Some(r) = long_arg(a, "--adapt=") {
            c.adapt = true;
            let (mut min, mut max) = (c.adapt_min, c.adapt_max);
            let ok = self.parse_adapt(r, &mut min, &mut max)?;
            c.adapt_min = min;
            c.adapt_max = max;
            if !ok {
                return Err(self.bad_usage(&prog, a));
            }
            return Ok(());
        }
        for (name, kind) in [("--train-cover", DictKind::Cover), ("--train-fastcover", DictKind::FastCover)] {
            let Some(r) = long_arg(a, name) else { continue };
            c.op = Op::Train;
            c.out_name.get_or_insert_with(|| b"dictionary".to_vec());
            c.dict_kind = kind;
            let fast = kind == DictKind::FastCover;
            let params = if r.is_empty() {
                Some(CoverArgs::ZERO)
            } else if let Some(r) = r.strip_prefix(b"=") {
                self.parse_cover(r, fast)?
            } else {
                None
            };
            let Some(params) = params else { return Err(self.bad_usage(&prog, a)) };
            if fast {
                c.fast_cover = params;
            } else {
                c.cover = params;
            }
            return Ok(());
        }
        if let Some(r) = long_arg(a, "--train-legacy") {
            c.op = Op::Train;
            c.out_name.get_or_insert_with(|| b"dictionary".to_vec());
            c.dict_kind = DictKind::Legacy;
            if r.is_empty() {
                return Ok(());
            }
            let parsed = match r.strip_prefix(b"=") {
                Some(r) => self.parse_legacy(r)?,
                None => None,
            };
            match parsed {
                Some(s) => c.dict_select = s,
                None => return Err(self.bad_usage(&prog, a)),
            }
            return Ok(());
        }
        if let Some(r) = long_arg(a, "--threads") {
            c.nb_workers = i64::from(self.next_u32(args, idx, r)?);
        } else if let Some(r) = long_arg(a, "--memlimit") {
            c.mem_limit = self.next_u32(args, idx, r)?;
        } else if let Some(r) = long_arg(a, "--memory") {
            c.mem_limit = self.next_u32(args, idx, r)?;
        } else if let Some(r) = long_arg(a, "--block-size") {
            c.block_size = self.next_size(args, idx, r)?;
        } else if let Some(r) = long_arg(a, "--maxdict") {
            c.max_dict_size = self.next_u32(args, idx, r)?;
        } else if let Some(r) = long_arg(a, "--dictID") {
            c.dict_id = self.next_u32(args, idx, r)?;
        } else if let Some(r) = long_arg(a, "--zstd=") {
            if !self.parse_cparams(r, &mut c.cparams)? {
                return Err(self.bad_usage(&prog, a));
            }
            self.prefs.ctype = CType::Zstd;
        } else if let Some(r) = long_arg(a, "--stream-size") {
            self.prefs.stream_src_size = self.next_size(args, idx, r)? as usize;
        } else if let Some(r) = long_arg(a, "--target-compressed-block-size") {
            c.target_cblock = self.next_size(args, idx, r)?;
        } else if let Some(r) = long_arg(a, "--size-hint") {
            c.size_hint = self.next_size(args, idx, r)?;
        } else if let Some(r) = long_arg(a, "--output-dir-flat") {
            let (v, _) = self.next_field(args, idx, r)?;
            if v.is_empty() {
                self.disp(1, "error: output dir cannot be empty string (did you mean to pass '.' instead?)\n");
                return Err(Exit(1));
            }
            c.out_dir = Some(v);
        } else if let Some(r) = long_arg(a, "--auto-threads") {
            let (v, _) = self.next_field(args, idx, r)?;
            if v == b"logical" {
                c.logical_cores = true;
            }
        } else if let Some(r) = long_arg(a, "--output-dir-mirror") {
            let (v, _) = self.next_field(args, idx, r)?;
            if v.is_empty() {
                self.disp(1, "error: output dir cannot be empty string (did you mean to pass '.' instead?)\n");
                return Err(Exit(1));
            }
            c.out_mirror = Some(v);
        } else if let Some(r) = long_arg(a, "--trace") {
            // O rastro do C vai pra um arquivo de diagnóstico interno; aqui só consome o argumento.
            self.next_field(args, idx, r)?;
        } else if let Some(r) = long_arg(a, "--patch-from") {
            let (v, _) = self.next_field(args, idx, r)?;
            c.patch_from = Some(v);
            c.ultra = true;
        } else if let Some(r) = long_arg(a, "--long") {
            c.ldm = true;
            c.ultra = true;
            let wlog = if let Some(r) = r.strip_prefix(b"=") {
                let mut i = 0;
                self.u32_of(r, &mut i)?
            } else if !r.is_empty() {
                return Err(self.bad_usage(&prog, a));
            } else {
                DEFAULT_MAX_WINDOW_LOG
            };
            if c.cparams.window_log == 0 {
                c.cparams.window_log = wlog;
            }
        } else if let Some(r) = long_arg(a, "--fast") {
            if let Some(r) = r.strip_prefix(b"=") {
                let max_fast = (-MIN_CLEVEL) as u32;
                let mut i = 0;
                let lvl = self.u32_of(r, &mut i)?.min(max_fast);
                if lvl == 0 {
                    return Err(self.bad_usage(&prog, a));
                }
                c.clevel = -(lvl as i32);
                c.dict_clevel = c.clevel;
            } else if !r.is_empty() {
                return Err(self.bad_usage(&prog, a));
            } else {
                c.clevel = -1;
            }
        } else if let Some(r) = long_arg(a, "--filelist") {
            let (v, _) = self.next_field(args, idx, r)?;
            c.file_lists.push(v);
        } else {
            return Err(self.bad_usage(&prog, a));
        }
        Ok(())
    }

    /// Um grupo de opções curtas (`-dcf19`...).
    fn parse_short(&mut self, args: &[Vec<u8>], idx: &mut usize, arg: &[u8], c: &mut Cli) -> R<()> {
        let prog = c.prog.clone();
        let mut i = 1;
        while let Some(&ch) = arg.get(i) {
            if ch.is_ascii_digit() {
                c.clevel = self.u32_of(arg, &mut i)? as i32;
                c.dict_clevel = c.clevel;
                continue;
            }
            i += 1;
            match ch {
                b'V' => {
                    self.out.write_str(&version_text(self.level));
                    return Err(Exit(0));
                }
                b'H' => {
                    self.out.write(&usage_advanced_text(&prog));
                    return Err(Exit(0));
                }
                b'h' => {
                    self.out.write(&usage_text(&prog));
                    return Err(Exit(0));
                }
                b'z' => c.op = Op::Compress,
                b'd' => {
                    c.decode_only = true;
                    if c.op != Op::Bench {
                        c.op = Op::Decompress;
                    }
                }
                b'c' => {
                    c.force_stdout = true;
                    c.out_name = Some(STDOUT_MARK.to_vec());
                }
                b'o' | b'D' => {
                    // `NEXT_FIELD` sem `=` pega o argumento seguinte e continua lendo este como opções.
                    let (v, whole) = self.next_field(args, idx, &arg[i..])?;
                    if whole {
                        i = arg.len();
                    }
                    if ch == b'o' {
                        c.out_name = Some(v);
                    } else {
                        c.dict_name = Some(v);
                    }
                }
                b'n' => {}
                b'f' => {
                    self.prefs.overwrite = true;
                    c.force_stdin = true;
                    c.force_stdout = true;
                    c.follow_links = true;
                    self.prefs.allow_block_devices = true;
                }
                b'v' => self.level += 1,
                b'q' => self.level -= 1,
                b'k' => self.prefs.remove_src = false,
                b'C' => self.prefs.checksum_flag = 2,
                b't' => c.op = Op::Test,
                b'M' => c.mem_limit = self.u32_of(arg, &mut i)?,
                b'l' => c.op = Op::List,
                b'r' => c.recursive = true,
                b'b' => c.op = Op::Bench,
                b'e' => c.clevel_last = self.u32_of(arg, &mut i)? as i32,
                b'i' => c.bench_seconds = self.u32_of(arg, &mut i)?,
                b'B' => c.block_size = u64::from(self.u32_of(arg, &mut i)?),
                b'S' => c.separate_files = true,
                b'T' => c.nb_workers = i64::from(self.u32_of(arg, &mut i)?),
                b's' => c.dict_select = self.u32_of(arg, &mut i)?,
                b'p' => {
                    if arg.get(i).is_some_and(u8::is_ascii_digit) {
                        c.additional_param = self.u32_of(arg, &mut i)? as i32;
                    } else {
                        c.pause = true;
                    }
                }
                b'P' => c.compressibility = f64::from(self.u32_of(arg, &mut i)?) / 100.0,
                _ => return Err(self.bad_usage(&prog, &[b'-', ch])),
            }
        }
        Ok(())
    }
}

// -------------------------------------------------------------------------------------------------
// Listas de arquivos (`UTIL_createFileNamesTable_fromFileName`, `UTIL_expandFNT`).

/// `readLinesFromFile`: uma entrada por linha, sem o `\n`; linha vazia conta.
fn read_file_list(name: &[u8]) -> Option<Vec<Vec<u8>>> {
    let st = stat_path(name).ok()?;
    if st.file_type() != FileType::Regular || st.size > MAX_FILE_OF_FILE_NAMES_SIZE {
        return None;
    }
    let fd = common::open_input(name, false).ok()?;
    let data = common::read_all(fd);
    common::close(fd);
    let data = data.ok()?;
    let mut names = Vec::new();
    let mut rest = &data[..];
    while !rest.is_empty() {
        let (line, next) = match rest.iter().position(|&b| b == b'\n') {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, &rest[rest.len()..]),
        };
        // O `fgets` para no primeiro NUL: o nome acaba ali.
        let line = line.split(|&b| b == 0).next().unwrap_or_default();
        names.push(line.to_vec());
        rest = next;
    }
    (!names.is_empty()).then_some(names)
}

impl Zstd {
    /// `UTIL_prepareFileList`: os arquivos sob `dir`, recursivamente.
    fn prepare_file_list(&self, dir: &[u8], follow_links: bool, out: &mut Vec<Vec<u8>>) {
        let entries = match common::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                if self.util_level >= 1 {
                    common::eprint(msg!("Cannot open directory '", dir, "': ", e.message(), "\n"));
                }
                return;
            }
        };
        for name in entries {
            let mut path = dir.to_vec();
            path.push(b'/');
            path.extend_from_slice(&name);
            if !follow_links && is_link(&path) {
                if self.util_level >= 2 {
                    common::eprint(msg!("Warning : ", path, " is a symbolic link, ignoring\n"));
                }
                continue;
            }
            if is_directory(&path) {
                self.prepare_file_list(&path, follow_links, out);
            } else {
                out.push(path);
            }
        }
    }

    /// `UTIL_expandFNT`.
    fn expand_file_names(&self, names: &[Vec<u8>], follow_links: bool) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for n in names {
            if is_directory(n) {
                self.prepare_file_list(n, follow_links, &mut out);
            } else {
                out.push(n.clone());
            }
        }
        out
    }

    /// O `main` depois da análise das opções.
    fn run(&mut self, mut c: Cli) -> R<i32> {
        self.disp(3, WELCOME);
        if c.op == Op::Decompress && c.nb_workers > 1 {
            self.disp(2, "Warning : decompression does not support multi-threading\n");
        }
        if c.nb_workers == 0 && !c.single_thread {
            let n = sys::current().sched_getaffinity().len().max(1) as i64;
            c.nb_workers = n;
            let kind = if c.logical_cores { "logical" } else { "physical" };
            self.disp(3, format!("Note: {n} {kind} core(s) detected \n"));
        }
        if c.nb_workers == -1 {
            c.nb_workers = if c.op == Op::Decompress { 1 } else { self.default_nb_threads() };
        }
        if c.op != Op::Bench {
            self.disp(4, format!("Compressing with {} worker threads \n", c.nb_workers));
        }
        self.util_level = self.level;

        if !c.follow_links {
            let total = c.filenames.len();
            let mut kept = Vec::new();
            for n in std::mem::take(&mut c.filenames) {
                if is_link(&n) && !is_fifo(&n) {
                    self.disp(2, msg!("Warning : ", n, " is a symbolic link, ignoring \n"));
                } else {
                    kept.push(n);
                }
            }
            if kept.is_empty() && total > 0 {
                return Ok(1);
            }
            c.filenames = kept;
        }
        for list in c.file_lists.clone() {
            match read_file_list(&list) {
                Some(names) => c.filenames.extend(names),
                None => {
                    self.disp(1, msg!("zstd: error reading ", list, " \n"));
                    return Ok(1);
                }
            }
        }
        let nb_input_names = c.filenames.len();
        if c.recursive {
            c.filenames = self.expand_file_names(&c.filenames, c.follow_links);
        }

        if c.op == Op::List {
            let names = c.filenames.clone();
            return Ok(self.list_multiple(&names));
        }
        if c.op == Op::Bench {
            return self.bench(&c);
        }
        if c.op == Op::Train {
            let (kind, args) = match c.dict_kind {
                DictKind::Cover => (DictKind::Cover, c.cover),
                DictKind::FastCover => (DictKind::FastCover, c.fast_cover),
                DictKind::Legacy => (DictKind::Legacy, CoverArgs::ZERO),
            };
            let params = TrainParams {
                kind,
                k: args.k,
                d: args.d,
                f: args.f,
                steps: args.steps,
                split: (args.split > 0.0).then_some(args.split),
                accel: args.accel,
                shrink: args.shrink,
                selectivity: c.dict_select,
                dict_id: c.dict_id,
                level: c.dict_clevel,
                max_dict_size: c.max_dict_size,
                chunk_size: c.block_size as usize,
                mem_limit: c.mem_limit,
            };
            let out = c.out_name.clone().unwrap_or_else(|| b"dictionary".to_vec());
            return self.train_from_files(&out, &c.filenames, &params);
        }
        if c.op == Op::Test {
            self.prefs.test_mode = true;
            c.out_name = Some(NUL_MARK.to_vec());
            self.prefs.remove_src = false;
        }
        if c.filenames.is_empty() {
            if nb_input_names > 0 {
                self.disp(1, "please provide correct input file(s) or non-empty directories -- ignored \n");
                return Ok(0);
            }
            c.filenames.push(STDIN_MARK.to_vec());
        }
        if c.filenames.len() == 1 && c.filenames[0] == STDIN_MARK && c.out_name.is_none() {
            c.out_name = Some(STDOUT_MARK.to_vec());
        }
        let has_stdin_name = c.filenames.iter().any(|n| n == STDIN_MARK);
        if !c.force_stdin && has_stdin_name && self.console_stdin() {
            self.disp(1, "stdin is a console, aborting\n");
            return Ok(1);
        }
        if c.out_name.as_deref().is_none_or(|o| o == STDOUT_MARK)
            && self.console_stdout()
            && has_stdin_name
            && !c.force_stdout
            && c.op != Op::Decompress
        {
            self.disp(1, "stdout is a console, aborting\n");
            return Ok(1);
        }
        let max_clevel = if c.ultra { MAX_CLEVEL } else { CLEVEL_MAX };
        if c.clevel > max_clevel {
            self.disp(2, format!("Warning : compression level higher than max, reduced to {max_clevel} \n"));
            c.clevel = max_clevel;
        }
        if c.show_default_cparams && c.op == Op::Decompress {
            self.disp(1, "error : can't use --show-default-cparams in decompression mode \n");
            return Ok(1);
        }
        if c.dict_name.is_some() && c.patch_from.is_some() {
            self.disp(1, "error : can't use -D and --patch-from=# at the same time \n");
            return Ok(1);
        }
        if c.patch_from.is_some() && c.filenames.len() > 1 {
            self.disp(1, "error : can't use --patch-from=# on multiple files \n");
            return Ok(1);
        }
        let has_stdout = c.out_name.as_deref() == Some(STDOUT_MARK);
        if has_stdout && self.level == 2 {
            self.level = 1;
        }
        if !self.console_stderr() && self.progress != Progress::Always {
            self.progress = Progress::Never;
        }
        if has_stdout && self.prefs.remove_src {
            self.disp(3, "Note: src files are not removed when output is stdout \n");
            self.prefs.remove_src = false;
        }
        self.fctx.has_stdout = has_stdout;
        self.fctx.nb_files_total = c.filenames.len();
        self.fctx.has_stdin = has_stdin_name;
        self.prefs.patch_from = c.patch_from.is_some();
        if c.mem_limit == 0 {
            c.mem_limit = if c.cparams.window_log == 0 {
                1 << DEFAULT_MAX_WINDOW_LOG
            } else {
                1u32.wrapping_shl(c.cparams.window_log & 31)
            };
        }
        if c.patch_from.is_some() {
            c.dict_name = c.patch_from.clone();
        }
        self.prefs.mem_limit = c.mem_limit;
        self.prefs.window_log = c.cparams.window_log;
        self.prefs.ldm = c.ldm;
        self.prefs.nb_workers = c.nb_workers;
        let names = c.filenames.clone();
        let dict = c.dict_name.clone();
        let out = c.out_name.clone();
        if c.op == Op::Compress {
            self.prefs.sparse = 0;
            // Os avisos e recusas dos `FIO_set*`, na ordem em que o C os chama.
            if c.block_size != 0 && c.nb_workers == 0 {
                self.disp(2, "Setting block size is useless in single-thread mode \n");
            }
            if c.cparams.overlap_log.is_some_and(|o| o != 0) && c.nb_workers == 0 {
                self.disp(2, "Setting overlapLog is useless in single-thread mode \n");
            }
            if c.adapt && c.nb_workers == 0 {
                return Err(self.throw(1, "Adaptive mode is not compatible with single thread mode \n"));
            }
            if c.rsyncable && c.nb_workers == 0 {
                return Err(self.throw(1, "Rsyncable mode is not compatible with single thread mode \n"));
            }
            if c.adapt_min > c.clevel {
                c.clevel = c.adapt_min;
            }
            if c.adapt_max < c.clevel {
                c.clevel = c.adapt_max;
            }
            if c.show_default_cparams || self.level >= 4 {
                for n in &names {
                    if c.show_default_cparams {
                        self.print_default_cparams(n, dict.as_deref(), c.clevel);
                    }
                    if self.level >= 4 {
                        self.print_actual_cparams(n, dict.as_deref(), c.clevel, &c.cparams);
                    }
                }
            }
            if self.level >= 4 {
                self.display_compression_parameters(&c);
            }
            if names.len() == 1
                && let Some(out) = &out
            {
                return self.compress_filename(out, &names[0], dict.as_deref(), c.clevel);
            }
            let dest = Dest { mirror: c.out_mirror.as_deref(), out_dir: c.out_dir.as_deref(), out_name: out.as_deref() };
            return self.compress_multiple(&names, &dest, c.suffix, dict.as_deref(), c.clevel);
        }
        if names.len() == 1
            && let Some(out) = &out
        {
            return self.decompress_filename(out, &names[0], dict.as_deref());
        }
        let dest = Dest { mirror: c.out_mirror.as_deref(), out_dir: c.out_dir.as_deref(), out_name: out.as_deref() };
        self.decompress_multiple(&names, &dest, dict.as_deref())
    }
}

/// `main` do `zstd`, `unzstd`, `zstdcat` e `zstdmt`.
pub fn main(args: &[Vec<u8>]) -> i32 {
    let mut z = Zstd {
        level: 2,
        util_level: 2,
        progress: Progress::Auto,
        out: Output::stdout(),
        prefs: Prefs {
            ctype: CType::Zstd,
            overwrite: false,
            dict_id_flag: true,
            checksum_flag: 1,
            remove_src: false,
            mem_limit: 0,
            test_mode: false,
            exclude_compressed: false,
            allow_block_devices: false,
            pass_through: -1,
            content_size: true,
            stream_src_size: 0,
            patch_from: false,
            window_log: 0,
            ldm: false,
            sparse: 1,
            nb_workers: 1,
        },
        fctx: Fctx { nb_files_total: 1, ..Fctx::default() },
        rp: ReadPool::new(zstd_dec::DSTREAM_IN_SIZE),
        dst: Dst::Closed,
        dctx: None,
        fake_stdin_console: false,
        fake_stdout_console: false,
        fake_stderr_console: false,
    };
    let code = match z.parse_args(args) {
        Ok(c) => {
            let pause = c.pause;
            let r = z.run(c);
            if pause {
                common::eprint("Press enter to continue... \n");
                let mut b = [0u8; 1];
                let _ = sys::read(Fd::STDIN, &mut b);
            }
            match r {
                Ok(n) => n,
                Err(Exit(n)) => n,
            }
        }
        Err(Exit(n)) => n,
    };
    let _ = z.close_dst();
    let _ = z.out.finish();
    code
}

// -------------------------------------------------------------------------------------------------
// Parâmetros de compressão por nível (`ZSTD_getCParams`, `lib/compress/clevels.h`), pro
// `--show-default-cparams` e pro `-vv`.

/// `ZSTD_defaultCParameters[tabela][nível]`: W, C, H, S, L, TL, estratégia.
#[rustfmt::skip]
const DEFAULT_CPARAMS: [[[u32; 7]; 23]; 4] = [
    [
        [19, 12, 13, 1, 6, 1, 1], [19, 13, 14, 1, 7, 0, 1], [20, 15, 16, 1, 6, 0, 1], [21, 16, 17, 1, 5, 0, 2],
        [21, 18, 18, 1, 5, 0, 2], [21, 18, 19, 3, 5, 2, 3], [21, 18, 19, 3, 5, 4, 4], [21, 19, 20, 4, 5, 8, 4],
        [21, 19, 20, 4, 5, 16, 5], [22, 20, 21, 4, 5, 16, 5], [22, 21, 22, 5, 5, 16, 5], [22, 21, 22, 6, 5, 16, 5],
        [22, 22, 23, 6, 5, 32, 5], [22, 22, 22, 4, 5, 32, 6], [22, 22, 23, 5, 5, 32, 6], [22, 23, 23, 6, 5, 32, 6],
        [22, 22, 22, 5, 5, 48, 7], [23, 23, 22, 5, 4, 64, 7], [23, 23, 22, 6, 3, 64, 8], [23, 24, 22, 7, 3, 256, 9],
        [25, 25, 23, 7, 3, 256, 9], [26, 26, 24, 7, 3, 512, 9], [27, 27, 25, 9, 3, 999, 9],
    ],
    [
        [18, 12, 13, 1, 5, 1, 1], [18, 13, 14, 1, 6, 0, 1], [18, 14, 14, 1, 5, 0, 2], [18, 16, 16, 1, 4, 0, 2],
        [18, 16, 17, 3, 5, 2, 3], [18, 17, 18, 5, 5, 2, 3], [18, 18, 19, 3, 5, 4, 4], [18, 18, 19, 4, 4, 4, 4],
        [18, 18, 19, 4, 4, 8, 5], [18, 18, 19, 5, 4, 8, 5], [18, 18, 19, 6, 4, 8, 5], [18, 18, 19, 5, 4, 12, 6],
        [18, 19, 19, 7, 4, 12, 6], [18, 18, 19, 4, 4, 16, 7], [18, 18, 19, 4, 3, 32, 7], [18, 18, 19, 6, 3, 128, 7],
        [18, 19, 19, 6, 3, 128, 8], [18, 19, 19, 8, 3, 256, 8], [18, 19, 19, 6, 3, 128, 9], [18, 19, 19, 8, 3, 256, 9],
        [18, 19, 19, 10, 3, 512, 9], [18, 19, 19, 12, 3, 512, 9], [18, 19, 19, 13, 3, 999, 9],
    ],
    [
        [17, 12, 12, 1, 5, 1, 1], [17, 12, 13, 1, 6, 0, 1], [17, 13, 15, 1, 5, 0, 1], [17, 15, 16, 2, 5, 0, 2],
        [17, 17, 17, 2, 4, 0, 2], [17, 16, 17, 3, 4, 2, 3], [17, 16, 17, 3, 4, 4, 4], [17, 16, 17, 3, 4, 8, 5],
        [17, 16, 17, 4, 4, 8, 5], [17, 16, 17, 5, 4, 8, 5], [17, 16, 17, 6, 4, 8, 5], [17, 17, 17, 5, 4, 8, 6],
        [17, 18, 17, 7, 4, 12, 6], [17, 18, 17, 3, 4, 12, 7], [17, 18, 17, 4, 3, 32, 7], [17, 18, 17, 6, 3, 256, 7],
        [17, 18, 17, 6, 3, 128, 8], [17, 18, 17, 8, 3, 256, 8], [17, 18, 17, 10, 3, 512, 8], [17, 18, 17, 5, 3, 256, 9],
        [17, 18, 17, 7, 3, 512, 9], [17, 18, 17, 9, 3, 512, 9], [17, 18, 17, 11, 3, 999, 9],
    ],
    [
        [14, 12, 13, 1, 5, 1, 1], [14, 14, 15, 1, 5, 0, 1], [14, 14, 15, 1, 4, 0, 1], [14, 14, 15, 2, 4, 0, 2],
        [14, 14, 14, 4, 4, 2, 3], [14, 14, 14, 3, 4, 4, 4], [14, 14, 14, 4, 4, 8, 5], [14, 14, 14, 6, 4, 8, 5],
        [14, 14, 14, 8, 4, 8, 5], [14, 15, 14, 5, 4, 8, 6], [14, 15, 14, 9, 4, 8, 6], [14, 15, 14, 3, 4, 12, 7],
        [14, 15, 14, 4, 3, 24, 7], [14, 15, 14, 5, 3, 32, 8], [14, 15, 15, 6, 3, 64, 8], [14, 15, 15, 7, 3, 256, 8],
        [14, 15, 15, 5, 3, 48, 9], [14, 15, 15, 6, 3, 128, 9], [14, 15, 15, 7, 3, 256, 9], [14, 15, 15, 8, 3, 256, 9],
        [14, 15, 15, 8, 3, 512, 9], [14, 15, 15, 9, 3, 512, 9], [14, 15, 15, 10, 3, 999, 9],
    ],
];

const STRATEGY_NAMES: [&str; 10] = [
    "", "ZSTD_fast", "ZSTD_dfast", "ZSTD_greedy", "ZSTD_lazy", "ZSTD_lazy2", "ZSTD_btlazy2", "ZSTD_btopt",
    "ZSTD_btultra", "ZSTD_btultra2",
];

fn highbit32(v: u32) -> u32 {
    31 - v.leading_zeros()
}

/// `ZSTD_getCParams(level, srcSize, dictSize)` com o `ZSTD_adjustCParams_internal` do modo
/// desconhecido e o buscador por linhas no automático.
fn get_cparams(level: i32, src_size: u64, dict_size: u64) -> CParams {
    let unknown = src_size == FILESIZE_UNKNOWN;
    let r_size = if unknown && dict_size == 0 {
        FILESIZE_UNKNOWN
    } else {
        src_size.wrapping_add(dict_size).wrapping_add(if unknown && dict_size > 0 { 500 } else { 0 })
    };
    let table = usize::from(r_size <= 256 << 10) + usize::from(r_size <= 128 << 10) + usize::from(r_size <= 16 << 10);
    let row = match level {
        0 => CLEVEL_DEFAULT as usize,
        l if l < 0 => 0,
        l if l > MAX_CLEVEL => MAX_CLEVEL as usize,
        l => l as usize,
    };
    let t = DEFAULT_CPARAMS[table][row];
    let mut cp = CParams {
        window_log: t[0],
        chain_log: t[1],
        hash_log: t[2],
        search_log: t[3],
        min_match: t[4],
        target_length: t[5],
        strategy: t[6],
        overlap_log: None,
    };
    if level < 0 {
        cp.target_length = level.max(MIN_CLEVEL).unsigned_abs();
    }
    adjust_cparams(cp, src_size, dict_size)
}

/// `ZSTD_getCParamsFromCCtxParams`: os do nível, a sobrescrita do `--zstd=`/`--long` e o ajuste de
/// novo ao tamanho.
fn get_cparams_with(level: i32, src_size: u64, dict_size: u64, over: &CParams) -> CParams {
    let mut cp = get_cparams(level, src_size, dict_size);
    let pick = |a: u32, b: u32| if b == 0 { a } else { b };
    cp.window_log = pick(cp.window_log, over.window_log);
    cp.hash_log = pick(cp.hash_log, over.hash_log);
    cp.chain_log = pick(cp.chain_log, over.chain_log);
    cp.search_log = pick(cp.search_log, over.search_log);
    cp.min_match = pick(cp.min_match, over.min_match);
    cp.target_length = pick(cp.target_length, over.target_length);
    cp.strategy = pick(cp.strategy, over.strategy);
    adjust_cparams(cp, src_size, dict_size)
}

/// `ZSTD_adjustCParams_internal` no modo desconhecido, com o buscador por linhas no automático.
fn adjust_cparams(mut cp: CParams, src_size: u64, dict_size: u64) -> CParams {
    let unknown = src_size == FILESIZE_UNKNOWN;
    let max_resize = 1u64 << (WINDOWLOG_MAX - 1);
    if src_size <= max_resize && dict_size <= max_resize {
        let t_size = (src_size + dict_size) as u32;
        let src_log = if t_size < 1 << 6 { 6 } else { highbit32(t_size - 1) + 1 };
        cp.window_log = cp.window_log.min(src_log);
    }
    if !unknown {
        let dict_and_window = if dict_size == 0 {
            cp.window_log
        } else {
            let w = 1u64 << cp.window_log;
            if w >= dict_size + src_size {
                cp.window_log
            } else if dict_size + w >= 1 << WINDOWLOG_MAX {
                WINDOWLOG_MAX
            } else {
                highbit32((dict_size + w) as u32 - 1) + 1
            }
        };
        let cycle = cp.chain_log - u32::from(cp.strategy >= 6);
        if cp.hash_log > dict_and_window + 1 {
            cp.hash_log = dict_and_window + 1;
        }
        if cycle > dict_and_window {
            cp.chain_log -= cycle - dict_and_window;
        }
    }
    cp.window_log = cp.window_log.max(10);
    if (3..=5).contains(&cp.strategy) {
        let row_log = cp.search_log.clamp(4, 6);
        cp.hash_log = cp.hash_log.min(24 + row_log);
    }
    cp
}

impl Zstd {
    fn dict_file_size(dict: Option<&[u8]>) -> u64 {
        dict.map_or(0, file_size)
    }

    /// `printDefaultCParams`.
    fn print_default_cparams(&self, name: &[u8], dict: Option<&[u8]>, level: i32) {
        let size = file_size(name);
        let cp = get_cparams(level, size, Self::dict_file_size(dict));
        let head = if size != FILESIZE_UNKNOWN {
            msg!(name, format!(" ({size} bytes)\n"))
        } else {
            msg!(name, " (src size unknown)\n")
        };
        common::eprint(msg!(
            head,
            format!(
                " - windowLog     : {}\n - chainLog      : {}\n - hashLog       : {}\n - searchLog     : {}\n - minMatch      : {}\n - targetLength  : {}\n - strategy      : {} ({})\n",
                cp.window_log,
                cp.chain_log,
                cp.hash_log,
                cp.search_log,
                cp.min_match,
                cp.target_length,
                STRATEGY_NAMES[cp.strategy as usize],
                cp.strategy
            )
        ));
    }

    /// `printActualCParams`: os do nível com os do `--zstd=` por cima.
    fn print_actual_cparams(&self, name: &[u8], dict: Option<&[u8]>, level: i32, over: &CParams) {
        let mut cp = get_cparams(level, file_size(name), Self::dict_file_size(dict));
        let pick = |a: u32, b: u32| if b == 0 { a } else { b };
        cp.window_log = pick(cp.window_log, over.window_log);
        cp.chain_log = pick(cp.chain_log, over.chain_log);
        cp.hash_log = pick(cp.hash_log, over.hash_log);
        cp.search_log = pick(cp.search_log, over.search_log);
        cp.min_match = pick(cp.min_match, over.min_match);
        cp.target_length = pick(cp.target_length, over.target_length);
        cp.strategy = pick(cp.strategy, over.strategy);
        common::eprint(format!(
            "--zstd=wlog={},clog={},hlog={},slog={},mml={},tlen={},strat={}\n",
            cp.window_log, cp.chain_log, cp.hash_log, cp.search_log, cp.min_match, cp.target_length, cp.strategy
        ));
    }

    /// `FIO_displayCompressionParameters` (a compressão sempre desliga o `--sparse`).
    fn display_compression_parameters(&self, c: &Cli) {
        let format = match self.prefs.ctype {
            CType::Zstd => ".zst",
            CType::Gzip => ".gz",
            CType::Xz => ".xz",
            CType::Lzma => ".lzma",
            CType::Lz4 => ".lz4",
        };
        let mut s = format!("--format={format} --no-sparse");
        if !self.prefs.dict_id_flag {
            s.push_str(" --no-dictID");
        }
        s.push_str([" --no-check", "", " --check"][self.prefs.checksum_flag as usize]);
        s.push_str(&format!(" --block-size={}", c.block_size as i32));
        if c.adapt {
            s.push_str(&format!(" --adapt=min={},max={}", c.adapt_min, c.adapt_max));
        }
        s.push_str(["", " --no-row-match-finder", " --row-match-finder"][c.row_match as usize]);
        if c.rsyncable {
            s.push_str(" --rsyncable");
        }
        if self.prefs.stream_src_size > 0 {
            s.push_str(&format!(" --stream-size={}", self.prefs.stream_src_size as u32));
        }
        if c.size_hint > 0 {
            s.push_str(&format!(" --size-hint={}", c.size_hint.min(i32::MAX as u64)));
        }
        if c.target_cblock > 0 {
            s.push_str(&format!(" --target-compressed-block-size={}", c.target_cblock as u32));
        }
        s.push_str(["", " --compress-literals", " --no-compress-literals"][c.literals as usize]);
        let mem = if self.prefs.mem_limit != 0 { self.prefs.mem_limit } else { 128 << 20 };
        s.push_str(&format!(" --memory={mem} --threads={}", c.nb_workers));
        if self.prefs.exclude_compressed {
            s.push_str(" --exclude-compressed");
        }
        s.push_str(if self.prefs.content_size { " --content-size" } else { " --no-content-size" });
        s.push('\n');
        common::eprint(s);
    }
}

// -------------------------------------------------------------------------------------------------
// Benchmark (`-b`, `programs/benchzstd.c`). As velocidades são medidas de verdade no relógio
// monotônico; os tamanhos comprimidos são os do codificador do `structured-zstd`.

/// Arquivos carregados pro benchmark (dados e tamanho de cada um), ou o código de saída.
type BenchLoad = Result<(Vec<u8>, Vec<usize>), i32>;

/// Os parâmetros do benchmark (`BMK_advancedParams_t`) que mudam a saída.
struct BenchArgs<'a> {
    seconds: u32,
    block_size: usize,
    decode_only: bool,
    additional_param: i32,
    real_time: bool,
    dict: Option<&'a [u8]>,
}

fn now_ns() -> u64 {
    match sys::current().clock_gettime(sysabi::Clock::Monotonic) {
        Ok(t) => t.sec as u64 * 1_000_000_000 + t.nsec as u64,
        Err(_) => 0,
    }
}

/// Uma rodada cronometrada do `BMK_benchTimedFn`: repete `f` por cerca de um segundo (ou até o
/// orçamento que falta) e devolve os nanossegundos por execução.
fn timed_round(budget_ns: u64, f: &mut dyn FnMut() -> bool) -> Option<u64> {
    let start = now_ns();
    let mut runs = 0u64;
    loop {
        sys::checkpoint();
        if !f() {
            return None;
        }
        runs += 1;
        let elapsed = now_ns().saturating_sub(start);
        if elapsed >= budget_ns.min(1_000_000_000) {
            return Some((elapsed / runs).max(1));
        }
    }
}

/// Comprime um bloco num frame independente, com o dicionário e o nível do benchmark.
fn bench_compress_block(block: &[u8], level: i32, dict: Option<&[u8]>) -> Option<Vec<u8>> {
    use structured_zstd::encoding::{CompressionLevel, StreamingEncoder};
    let mut enc = StreamingEncoder::new(Vec::new(), CompressionLevel::from_level(level));
    enc.set_pledged_content_size(block.len() as u64).ok()?;
    if let Some(d) = dict {
        enc.set_dictionary_from_bytes(d).ok()?;
    }
    enc.write_all(block).ok()?;
    enc.finish().ok()
}

impl Zstd {
    /// `OUTPUT`/`OUTPUTLEVEL`: stdout com descarga imediata, como o `fflush(NULL)` do C.
    fn bench_out(&mut self, l: i32, m: impl AsRef<[u8]>) {
        if self.level >= l {
            self.out.write(m.as_ref());
            self.out.flush();
        }
    }

    /// `BMK_benchMemAdvanced`: um nível sobre os blocos carregados.
    fn bench_mem(&mut self, src: &[u8], file_sizes: &[usize], level: i32, name: &str, a: &BenchArgs) -> R<i32> {
        let name = if name.len() > 17 { &name[name.len() - 17..] } else { name };
        let marks = [" |", " /", " =", " \\"];
        // Os blocos: cada arquivo, cortado em pedaços de `-B#` quando pedido.
        let mut blocks: Vec<&[u8]> = Vec::new();
        let mut at = 0;
        for &fs in file_sizes {
            let file = &src[at..at + fs];
            at += fs;
            if a.block_size > 0 && !a.decode_only {
                blocks.extend(file.chunks(a.block_size));
            } else {
                blocks.push(file);
            }
        }
        let dict = a.dict.map(|d| Dict::parse(d).unwrap_or_else(|_| Dict::raw(d)));
        let (mut compressed, src_size, mut c_size, mut ratio): (Vec<Vec<u8>>, usize, usize, f64);
        if a.decode_only {
            let mut total = 0u64;
            for b in &blocks {
                match zstd_dec::find_decompressed_size(b) {
                    Ok(Some(n)) => total += n,
                    Ok(None) => {
                        self.disp(1, "Error 32 : Decompressed size cannot be determined: cannot benchmark \n");
                        return Ok(1);
                    }
                    Err(_) => {
                        self.disp(
                            1,
                            "Error 32 : Error while trying to assess decompressed size: data may be invalid \n",
                        );
                        return Ok(1);
                    }
                }
            }
            compressed = blocks.iter().map(|b| b.to_vec()).collect();
            c_size = src.len();
            src_size = total as usize;
            ratio = src_size as f64 / c_size.max(1) as f64;
        } else {
            compressed = Vec::new();
            src_size = src.len();
            c_size = 0;
            ratio = 0.0;
        }
        let crc_orig = if a.decode_only {
            0
        } else {
            let mut h = zstd_dec::Xxh64::new(0);
            h.update(src);
            h.digest()
        };
        let mut c_speed = 0u64;
        let mut d_speed = 0u64;
        let mut mark = 0;
        self.bench_out(2, format!("\r{:70}\r", ""));
        self.bench_out(2, format!("{:>2}-{:<17.17} :{:10} -> \r", marks[mark], name, src_size));
        let budget = u64::from(a.seconds) * 1_000_000_000;
        let (mut c_spent, mut d_spent) = (0u64, 0u64);
        let (mut c_done, mut d_done) = (a.decode_only, false);
        let mut result = Vec::new();
        while !(c_done && d_done) {
            if !c_done {
                let start = now_ns();
                let mut out: Vec<Vec<u8>> = Vec::new();
                let per_run = timed_round(budget.saturating_sub(c_spent), &mut || {
                    out.clear();
                    for b in &blocks {
                        match bench_compress_block(b, level, a.dict) {
                            Some(z) => out.push(z),
                            None => return false,
                        }
                    }
                    true
                });
                let Some(per_run) = per_run else {
                    self.disp(1, "Error 30 : compression error \n");
                    return Ok(1);
                };
                c_spent += now_ns().saturating_sub(start);
                c_size = out.iter().map(Vec::len).sum();
                compressed = out;
                ratio = src_size as f64 / c_size.max(1) as f64;
                c_speed = c_speed.max((src_size as f64 * 1e9 / per_run as f64) as u64);
                let digits = 1 + usize::from(ratio < 100.0) + usize::from(ratio < 10.0);
                let cprec = if c_speed < 10_000_000 { 2 } else { 1 };
                self.bench_out(
                    2,
                    format!(
                        "{:>2}-{:<17.17} :{:10} ->{:10} (x{:5.*}), {:6.*} MB/s \r",
                        marks[mark],
                        name,
                        src_size,
                        c_size,
                        digits,
                        ratio,
                        cprec,
                        c_speed as f64 / 1e6
                    ),
                );
                c_done = c_spent >= budget;
            }
            if !d_done {
                let start = now_ns();
                let mut out = Vec::new();
                let per_run = timed_round(budget.saturating_sub(d_spent), &mut || {
                    out.clear();
                    for z in &compressed {
                        match zstd_dec::decompress_all(z, dict.as_ref()) {
                            Ok(d) => out.extend_from_slice(&d),
                            Err(_) => return false,
                        }
                    }
                    true
                });
                let Some(per_run) = per_run else {
                    self.disp(1, "Error 30 : decompression error \n");
                    return Ok(1);
                };
                d_spent += now_ns().saturating_sub(start);
                result = out;
                d_speed = d_speed.max((src_size as f64 * 1e9 / per_run as f64) as u64);
                let digits = 1 + usize::from(ratio < 100.0) + usize::from(ratio < 10.0);
                let cprec = if c_speed < 10_000_000 { 2 } else { 1 };
                self.bench_out(
                    2,
                    format!(
                        "{:>2}-{:<17.17} :{:10} ->{:10} (x{:5.*}), {:6.*} MB/s, {:6.1} MB/s\r",
                        marks[mark],
                        name,
                        src_size,
                        c_size,
                        digits,
                        ratio,
                        cprec,
                        c_speed as f64 / 1e6,
                        d_speed as f64 / 1e6
                    ),
                );
                d_done = d_spent >= budget;
            }
            mark = (mark + 1) % marks.len();
        }
        if !a.decode_only {
            let mut h = zstd_dec::Xxh64::new(0);
            h.update(&result);
            let crc = h.digest();
            if crc != crc_orig {
                common::eprint(format!(
                    "!!! WARNING !!! {name:>14} : Invalid Checksum : {:x} != {:x}   \n",
                    crc_orig as u32, crc as u32
                ));
                self.report_bench_mismatch(src, &result, &blocks);
            }
        }
        if self.level == 1 {
            let (cs, ds) = (c_speed as f64 / 1e6, d_speed as f64 / 1e6);
            let line = if a.additional_param != 0 {
                format!(
                    "-{level:<3}{c_size:11} ({ratio:5.3}) {cs:6.2} MB/s {ds:6.1} MB/s  {name} (param={})\n",
                    a.additional_param
                )
            } else {
                format!("-{level:<3}{c_size:11} ({ratio:5.3}) {cs:6.2} MB/s {ds:6.1} MB/s  {name}\n")
            };
            self.bench_out(1, line);
        }
        self.bench_out(2, format!("{level:2}#\n"));
        Ok(0)
    }

    /// O relatório de divergência do `BMK_benchMemAdvanced` (primeira posição diferente).
    fn report_bench_mismatch(&self, src: &[u8], got: &[u8], blocks: &[&[u8]]) {
        let Some(u) = (0..src.len()).find(|&u| got.get(u) != Some(&src[u])) else {
            common::eprint("no difference detected\n");
            return;
        };
        let mut s = format!("Decoding error at pos {u} ");
        let (mut seg, mut bn, mut pos) = (0usize, 0usize, u);
        for (i, b) in blocks.iter().enumerate() {
            if pos < b.len() {
                bn = i;
                break;
            }
            pos -= b.len();
            seg = i + 1;
        }
        s.push_str(&format!("(sample {seg}, block {bn}, pos {pos}) \n"));
        if u > 5 {
            s.push_str("origin: ");
            for n in (1..=5).rev() {
                s.push_str(&format!("{:02X} ", src[u - n]));
            }
            s.push_str(&format!(" :{:02X}:  ", src[u]));
            for n in 1..=3 {
                if let Some(b) = src.get(u + n) {
                    s.push_str(&format!("{b:02X} "));
                }
            }
            s.push_str(" \ndecode: ");
            for n in (1..=5).rev() {
                s.push_str(&format!("{:02X} ", got.get(u - n).copied().unwrap_or(0)));
            }
            s.push_str(&format!(" :{:02X}:  ", got.get(u).copied().unwrap_or(0)));
            for n in 1..=3 {
                if let Some(b) = got.get(u + n) {
                    s.push_str(&format!("{b:02X} "));
                }
            }
            s.push_str(" \n");
        }
        common::eprint(s);
    }

    /// `BMK_benchCLevels`.
    fn bench_levels(&mut self, src: &[u8], sizes: &[usize], start: i32, end: i32, name: &[u8], a: &BenchArgs) -> R<i32> {
        let name = String::from_utf8_lossy(common::base_name(name)).into_owned();
        if end > MAX_CLEVEL {
            self.disp(1, "Invalid Compression Level \n");
            return Ok(15);
        }
        if end < start {
            self.disp(1, "Invalid Compression Level Range \n");
            return Ok(15);
        }
        if a.real_time {
            self.disp(2, "Note : switching to real-time priority \n");
        }
        if self.level == 1 && a.additional_param == 0 {
            self.bench_out(
                1,
                format!(
                    "bench 1.5.7 : input {} bytes, {} seconds, {} KB blocks\n",
                    src.len() as u32,
                    a.seconds,
                    a.block_size >> 10
                ),
            );
        }
        for level in start..=end {
            if self.bench_mem(src, sizes, level, &name, a)? != 0 {
                return Ok(1);
            }
        }
        Ok(0)
    }

    /// `BMK_loadFiles`: os dados e o tamanho de cada arquivo, ou o código de saída.
    fn bench_load(&mut self, names: &[Vec<u8>], cap: u64) -> R<BenchLoad> {
        let mut data = Vec::new();
        let mut sizes = Vec::new();
        for n in names {
            let mut size = file_size(n);
            if is_directory(n) {
                self.disp(2, msg!("Ignoring ", n, " directory...       \n"));
                sizes.push(0);
                continue;
            }
            if size == FILESIZE_UNKNOWN {
                self.disp(2, msg!("Cannot evaluate size of ", n, ", ignoring ... \n"));
                sizes.push(0);
                continue;
            }
            let last = size > cap - data.len() as u64;
            if last {
                size = cap - data.len() as u64;
            }
            let Ok(fd) = common::open_input(n, false) else {
                self.disp(1, msg!("Error 10 : cannot open file ", n, " \n"));
                return Ok(Err(10));
            };
            self.bench_out(2, msg!("Loading ", n, "...       \r"));
            let got = common::read_all(fd);
            common::close(fd);
            match got {
                Ok(d) if d.len() as u64 >= size => {
                    data.extend_from_slice(&d[..size as usize]);
                    sizes.push(size as usize);
                }
                _ => {
                    self.disp(1, msg!("Error 11 : invalid read ", n, " \n"));
                    return Ok(Err(11));
                }
            }
            if last {
                break;
            }
        }
        if data.is_empty() {
            self.disp(1, "Error 12 : no data to bench \n");
            return Ok(Err(12));
        }
        Ok(Ok((data, sizes)))
    }

    /// `BMK_benchFilesAdvanced`.
    fn bench_files(&mut self, names: &[Vec<u8>], dict: Option<&[u8]>, start: i32, end: i32, a: &BenchArgs) -> R<i32> {
        if names.is_empty() {
            self.disp(1, "No Files to Benchmark");
            return Ok(13);
        }
        if end > MAX_CLEVEL {
            self.disp(1, "Invalid Compression Level");
            return Ok(14);
        }
        if names.iter().any(|n| file_size(n) == FILESIZE_UNKNOWN) {
            self.disp(1, "Error loading files");
            return Ok(15);
        }
        let dict_bytes;
        if let Some(d) = dict {
            let size = file_size(d);
            if size == FILESIZE_UNKNOWN {
                let e = stat_path(d).err().unwrap_or(Errno::EINVAL);
                self.disp(1, msg!("error loading ", d, " : ", e.message(), " \n"));
                self.disp(1, "benchmark aborted");
                return Ok(17);
            }
            if size > 64 << 20 {
                self.disp(1, msg!("dictionary file ", d, " too large"));
                return Ok(18);
            }
            match self.bench_load(&[d.to_vec()], size)? {
                Ok((bytes, _)) => dict_bytes = Some(bytes),
                Err(_) => return Ok(1),
            }
        } else {
            dict_bytes = None;
        }
        let total: u64 = names.iter().map(|n| file_size(n)).sum();
        let (data, sizes) = match self.bench_load(names, total)? {
            Ok(v) => v,
            Err(_) => return Ok(1),
        };
        let display = if names.len() > 1 { format!(" {} files", names.len()).into_bytes() } else { names[0].clone() };
        let a2 = BenchArgs { dict: dict_bytes.as_deref(), ..*a };
        self.bench_levels(&data, &sizes, start, end, &display, &a2)
    }

    /// O `-b` do `main`: arquivos, um por vez com `-S`, ou o dado sintético.
    fn bench(&mut self, c: &Cli) -> R<i32> {
        if self.prefs.ctype != CType::Zstd {
            self.disp(1, "benchmark mode is only compatible with zstd format \n");
            return Ok(1);
        }
        let (mut start, mut end) = (c.clevel, c.clevel_last);
        if c.decode_only {
            start = 0;
            end = 0;
        }
        start = start.min(MAX_CLEVEL);
        end = end.min(MAX_CLEVEL);
        if end < start {
            end = start;
        }
        let mut head = String::from("Benchmarking ");
        if c.filenames.len() > 1 {
            head.push_str(&format!("{} files ", c.filenames.len()));
        }
        if end > start {
            head.push_str(&format!("from level {start} to {end} "));
        } else {
            head.push_str(&format!("at level {start} "));
        }
        head.push_str(&format!("using {} threads \n", c.nb_workers));
        self.disp(3, head);
        let a = BenchArgs {
            seconds: c.bench_seconds,
            block_size: c.block_size as usize,
            decode_only: c.decode_only,
            additional_param: c.additional_param,
            real_time: c.real_time,
            dict: None,
        };
        let dict = c.dict_name.as_deref();
        if !c.filenames.is_empty() {
            if c.separate_files {
                let mut r = 0;
                for n in &c.filenames {
                    r = self.bench_files(std::slice::from_ref(n), dict, start, end, &a)?;
                }
                return Ok(r);
            }
            return self.bench_files(&c.filenames, dict, start, end, &a);
        }
        let size = if c.block_size > 0 { c.block_size as usize } else { 10_000_000 };
        let (data, name) = if c.compressibility < 0.0 {
            (zstd_gen::lorem(size, 0), "Lorem ipsum".to_string())
        } else {
            (
                zstd_gen::datagen(size, c.compressibility, 0.0, 0),
                format!("Synthetic {}%", (c.compressibility * 100.0) as u32),
            )
        };
        self.bench_levels(&data, &[size], start, end, name.as_bytes(), &a)
    }
}
