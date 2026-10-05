// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (words) wipesync genpattern randpasses randint genmax randnum randmax

//! Porte pseudo-linus: `shred` escrito sobre o uucore portado e o sysio, seguindo o shred.c do GNU
//! coreutils 9.7 (escolha e ordem dos passes, mensagens do `-v`, renomeações do `--remove`).
//!
//! A aleatoriedade segue o GNU: sem `--random-source` os bytes vêm do `getrandom(2)` do
//! pseudo-kernel (`sysio::random`); com `--random-source=ARQ` os bytes são lidos do arquivo, tanto
//! para os passes aleatórios quanto para o sorteio da ordem dos padrões (o `randint` do gnulib), de
//! modo que a mesma fonte dá a mesma ordem de passes que o GNU.

use std::ffi::{OsStr, OsString};
use std::io::{ErrorKind, Read as _, Seek as _, SeekFrom, Write as _};
use std::mem::ManuallyDrop;
use std::os::unix::ffi::OsStrExt;

use clap::builder::ValueParser;
use clap::{Arg, ArgAction, Command};
use sysio::fd::FromRawFd as _;
use sysio::fs::{self, File, OpenOptions};
use sysio::io::{BufReader, IsTerminal as _};
use sysio::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
use uucore::display::{Quotable, locale_quote};
use uucore::error::{FromIo, UResult, USimpleError, UUsageError};
use uucore::{format_usage, show, show_error, translate};

mod options {
    pub const FORCE: &str = "force";
    pub const ITERATIONS: &str = "iterations";
    pub const RANDOM_SOURCE: &str = "random-source";
    pub const SIZE: &str = "size";
    pub const U: &str = "u";
    pub const REMOVE: &str = "remove";
    pub const VERBOSE: &str = "verbose";
    pub const EXACT: &str = "exact";
    pub const ZERO: &str = "zero";
    pub const FILE: &str = "file";
}

/// Tamanho do bloco de saída de um passe de padrão fixo: múltiplo de 3 (o padrão tem 3 bytes) e de
/// 512 (o bit invertido de cada setor), como o `PERIODIC_OUTPUT_SIZE` do GNU.
const PERIODIC_OUTPUT_SIZE: usize = 60 * 1024;
/// Tamanho do bloco de saída de um passe aleatório (`NONPERIODIC_OUTPUT_SIZE` do GNU).
const NONPERIODIC_OUTPUT_SIZE: usize = 64 * 1024;
const SECTOR_SIZE: usize = 512;

// Números de errno do Linux que o GNU trata à parte.
const EBADF: i32 = 9;
const EINVAL: i32 = 22;
const ENOSPC: i32 = 28;

/// Os padrões do GNU: número positivo é um bloco com aquela quantidade de padrões; negativo é a
/// quantidade de passes aleatórios; zero volta ao começo.
const PATTERNS: &[i32] = &[
    -2, // 2 passes aleatórios
    2, 0x000, 0xFFF, // 1 bit
    2, 0x555, 0xAAA, // 2 bits
    -1, // 1 passe aleatório
    6, 0x249, 0x492, 0x6DB, 0x924, 0xB6D, 0xDB6, // 3 bits
    12, 0x111, 0x222, 0x333, 0x444, 0x666, 0x777, 0x888, 0x999, 0xBBB, 0xCCC, 0xDDD, 0xEEE, // 4 bits
    -1, // 1 passe aleatório
    // Os mesmos com o primeiro bit de cada setor invertido.
    8, 0x1000, 0x1249, 0x1492, 0x16DB, 0x1924, 0x1B6D, 0x1DB6, 0x1FFF, //
    14, 0x1111, 0x1222, 0x1333, 0x1444, 0x1555, 0x1666, 0x1777, 0x1888, 0x1999, 0x1AAA, 0x1BBB, 0x1CCC,
    0x1DDD, 0x1EEE, //
    -1, // 1 passe aleatório
    0,
];

/// Os caracteres dos nomes que o `--remove=wipe` experimenta, na ordem do GNU.
const NAMESET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum RemoveMethod {
    None,
    Unlink,
    Wipe,
    WipeSync,
}

struct Flags {
    force: bool,
    iterations: usize,
    size: Option<u64>,
    remove: RemoveMethod,
    verbose: bool,
    exact: bool,
    zero: bool,
}

/// A fonte de bytes aleatórios (o `randread` do gnulib) com o estado do `randint` por cima.
struct RandomSource {
    file: Option<(BufReader<File>, String)>,
    randnum: u64,
    randmax: u64,
}

impl RandomSource {
    /// Enche `buf`. Fim de arquivo ou erro de leitura na fonte encerram o programa, como no GNU.
    fn read(&mut self, buf: &mut [u8]) -> UResult<()> {
        match &mut self.file {
            None => {
                sysio::random::fill_bytes(buf);
                Ok(())
            }
            Some((reader, name)) => match reader.read_exact(buf) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                    Err(USimpleError::new(1, format!("{}: end of file", locale_quote(name.as_str()))))
                }
                Err(e) => {
                    let ctx = format!("{}: read error", locale_quote(name.as_str()));
                    Err(e.map_err_context(move || ctx))
                }
            },
        }
    }

    /// `randint_genmax`: um número uniforme em `0..=genmax`, aproveitando a sobra de cada leitura.
    fn genmax(&mut self, genmax: u64) -> UResult<u64> {
        let choices = genmax.wrapping_add(1);
        loop {
            if self.randmax < genmax {
                let mut needed = 0;
                let mut rmax = self.randmax;
                loop {
                    rmax = (rmax << 8) + 0xFF;
                    needed += 1;
                    if rmax >= genmax {
                        break;
                    }
                }
                let mut buf = [0u8; 8];
                self.read(&mut buf[..needed])?;
                let mut i = 0;
                loop {
                    self.randnum = (self.randnum << 8) + u64::from(buf[i]);
                    self.randmax = (self.randmax << 8) + 0xFF;
                    i += 1;
                    if self.randmax >= genmax {
                        break;
                    }
                }
            }

            if self.randmax == genmax {
                let n = self.randnum;
                self.randnum = 0;
                self.randmax = 0;
                return Ok(n);
            }

            let excess = self.randmax - genmax;
            let unusable = excess % choices;
            let last_usable = self.randmax - unusable;
            let reduced = self.randnum % choices;
            if self.randnum <= last_usable {
                self.randnum /= choices;
                self.randmax = excess / choices;
                return Ok(reduced);
            }
            self.randnum -= last_usable + 1;
            self.randmax = unusable - 1;
        }
    }

    /// `randint_choose`: um número uniforme em `0..n`.
    fn choose(&mut self, n: usize) -> UResult<usize> {
        Ok(self.genmax(n as u64 - 1)? as usize)
    }
}

/// `genpattern` do GNU: a lista dos `num` passes, `-1` para aleatório e o padrão de 12 (ou 13)
/// bits para os fixos.
fn genpattern(num: usize, rng: &mut RandomSource) -> UResult<Vec<i32>> {
    let mut dest = vec![0i32; num];
    if num == 0 {
        return Ok(dest);
    }

    // Etapa 1: escolher os passes.
    let mut p = 0usize;
    let mut randpasses = 0usize;
    let mut d = 0usize;
    let mut n = num;
    loop {
        let mut k = PATTERNS[p];
        p += 1;
        if k == 0 {
            p = 0;
        } else if k < 0 {
            let k = k.unsigned_abs() as usize;
            if k >= n {
                randpasses += n;
                break;
            }
            randpasses += k;
            n -= k;
        } else if (k as usize) <= n {
            let k = k as usize;
            dest[d..d + k].copy_from_slice(&PATTERNS[p..p + k]);
            p += k;
            d += k;
            n -= k;
        } else if n < 2 || 3 * n < k as usize {
            randpasses += n;
            break;
        } else {
            // Completa com `n` dos `k` padrões do bloco.
            loop {
                if n == k as usize || rng.choose(k as usize)? < n {
                    dest[d] = PATTERNS[p];
                    d += 1;
                    n -= 1;
                }
                p += 1;
                k -= 1;
                if n == 0 {
                    break;
                }
            }
            break;
        }
    }

    // Etapa 2: espalhar os passes aleatórios entre os fixos (DDA de Bresenham) e embaralhar os fixos.
    let mut top = num - randpasses;
    let randpasses = randpasses - 1;
    let mut accum = randpasses;
    for n in 0..num {
        if accum <= randpasses {
            accum += num - 1;
            dest[top] = dest[n];
            top += 1;
            dest[n] = -1;
        } else {
            let swap = n + rng.choose(top - n)?;
            dest.swap(n, swap);
        }
        accum -= randpasses;
    }
    Ok(dest)
}

/// `fillpattern` do GNU: replica os 12 bits do padrão no buffer e, nos padrões com o bit 0x1000,
/// inverte o primeiro bit de cada setor.
fn fill_pattern(kind: i32, buf: &mut [u8]) {
    let mut bits = (kind & 0xFFF) as u32;
    bits |= bits << 12;
    let r = [((bits >> 4) & 0xFF) as u8, ((bits >> 8) & 0xFF) as u8, (bits & 0xFF) as u8];
    for (i, b) in buf.iter_mut().enumerate() {
        *b = r[i % 3];
    }
    if kind & 0x1000 != 0 {
        for i in (0..buf.len()).step_by(SECTOR_SIZE) {
            buf[i] ^= 0x80;
        }
    }
}

#[uucore::main]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let matches = uucore::clap_localization::handle_clap_result(uu_app(), args)?;

    let iterations = match matches.get_one::<String>(options::ITERATIONS) {
        Some(text) => parse_iterations(text)?,
        None => 3,
    };
    let size = match matches.get_one::<String>(options::SIZE) {
        Some(text) => Some(uucore::parser::parse_size::parse_size_u64(text).map_err(|_| {
            USimpleError::new(1, format!("invalid file size: {}", locale_quote(text.as_str())))
        })?),
        None => None,
    };

    // `-u` e `--remove` valem na ordem em que aparecem: o último ganha.
    let mut remove = RemoveMethod::None;
    // O `-u` é contador: o clap lhe dá o valor padrão 0 (com índice), então só vale se veio da
    // linha de comando, senão todo arquivo seria removido.
    let u_index = (matches.get_count(options::U) > 0)
        .then(|| matches.indices_of(options::U).and_then(Iterator::last))
        .flatten();
    let remove_index = matches.indices_of(options::REMOVE).and_then(Iterator::last);
    if let Some(how) = matches.get_many::<String>(options::REMOVE).and_then(Iterator::last) {
        remove = parse_remove(how)?;
    }
    if u_index.is_some() && (remove_index.is_none() || u_index > remove_index) {
        remove = RemoveMethod::WipeSync;
    }

    let flags = Flags {
        force: matches.get_flag(options::FORCE),
        iterations,
        size,
        remove,
        verbose: matches.get_flag(options::VERBOSE),
        exact: matches.get_flag(options::EXACT),
        zero: matches.get_flag(options::ZERO),
    };

    let files: Vec<OsString> = matches
        .get_many::<OsString>(options::FILE)
        .map(|v| v.cloned().collect())
        .unwrap_or_default();
    if files.is_empty() {
        return Err(UUsageError::new(1, "missing file operand"));
    }

    let file = match matches.get_one::<String>(options::RANDOM_SOURCE) {
        Some(name) => {
            let f = File::open(name).map_err_context(|| name.maybe_quote().to_string())?;
            Some((BufReader::new(f), name.clone()))
        }
        None => None,
    };
    let mut rng = RandomSource { file, randnum: 0, randmax: 0 };

    for name in &files {
        let qname = name.maybe_quote().to_string();
        if name.as_bytes() == b"-" {
            // O fd 1 é do processo: o `File` não pode fechá-lo no fim.
            let out = ManuallyDrop::new(File::from_raw_fd(1));
            wipe_fd(&out, &qname, &mut rng, &flags)?;
        } else {
            wipe_file(name, &qname, &mut rng, &flags)?;
        }
    }
    Ok(())
}

/// `-n N`: dígitos decimais, como o `xnumtoumax` do GNU.
fn parse_iterations(text: &str) -> UResult<usize> {
    let digits = text.trim_start().strip_prefix('+').unwrap_or(text.trim_start());
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(USimpleError::new(1, format!("invalid number of passes: {}", locale_quote(text))));
    }
    let limit = (usize::MAX / size_of::<i32>()).min(u64::MAX as usize) as u64;
    match digits.parse::<u64>() {
        Ok(n) if n <= limit => Ok(n as usize),
        _ => Err(USimpleError::new(
            1,
            format!("invalid number of passes: {}: Value too large for defined data type", locale_quote(text)),
        )),
    }
}

/// `--remove=HOW` com as regras do `argmatch` (prefixo único vale, ambíguo e inválido são erro).
fn parse_remove(how: &str) -> UResult<RemoveMethod> {
    const ARGS: [(&str, RemoveMethod); 3] =
        [("unlink", RemoveMethod::Unlink), ("wipe", RemoveMethod::Wipe), ("wipesync", RemoveMethod::WipeSync)];
    if let Some((_, m)) = ARGS.iter().find(|(name, _)| *name == how) {
        return Ok(*m);
    }
    let candidates: Vec<RemoveMethod> =
        ARGS.iter().filter(|(name, _)| !how.is_empty() && name.starts_with(how)).map(|(_, m)| *m).collect();
    if let Some(first) = candidates.first()
        && candidates.iter().all(|m| m == first)
    {
        return Ok(*first);
    }
    let problem = if candidates.is_empty() { "invalid" } else { "ambiguous" };
    let mut msg = format!("{problem} argument {} for {}\nValid arguments are:", locale_quote(how), locale_quote("--remove"));
    for (name, _) in ARGS {
        msg.push_str("\n  - ");
        msg.push_str(&locale_quote(name));
    }
    Err(UUsageError::new(1, msg))
}

/// `wipefile` do GNU: abre para escrita (com `-f`, liberando a escrita se preciso), sobrescreve e,
/// se pedido, remove.
fn wipe_file(name: &OsStr, qname: &str, rng: &mut RandomSource, flags: &Flags) -> UResult<()> {
    let open = || OpenOptions::new().write(true).open(name);
    let mut file = open();
    if let Err(e) = &file
        && e.kind() == ErrorKind::PermissionDenied
        && flags.force
        && fs::set_permissions(name, fs::Permissions::from_mode(0o200)).is_ok()
    {
        file = open();
    }
    let file = match file {
        Ok(f) => f,
        Err(e) => {
            let ctx = format!("{qname}: failed to open for writing");
            show!(e.map_err_context(move || ctx));
            return Ok(());
        }
    };

    let ok = wipe_fd(&file, qname, rng, flags)?;
    drop(file);
    if ok && flags.remove != RemoveMethod::None {
        wipe_name(name, qname, flags);
    }
    Ok(())
}

/// `do_wipefd` do GNU. Devolve se deu tudo certo; um `Err` é erro fatal (fonte aleatória).
fn wipe_fd(file: &File, qname: &str, rng: &mut RandomSource, flags: &Flags) -> UResult<bool> {
    let meta = match file.metadata() {
        Ok(m) => m,
        Err(e) => {
            let ctx = format!("{qname}: fstat failed");
            show!(e.map_err_context(move || ctx));
            return Ok(false);
        }
    };
    let ft = meta.file_type();
    if (ft.is_char_device() && file.is_terminal()) || ft.is_fifo() || ft.is_socket() {
        show_error!("{qname}: invalid file type");
        uucore::error::set_exit_code(1);
        return Ok(false);
    }

    let blksize = meta.blksize().max(1);
    let mut i_size: u64 = 0;
    let mut size: Option<u64> = match flags.size {
        Some(s) => {
            if ft.is_file() && meta.len() < blksize.min(s) {
                i_size = meta.len();
            }
            Some(s)
        }
        None if ft.is_file() => {
            let mut s = meta.len();
            if !flags.exact {
                // Arredonda para o próximo bloco, para limpar a sobra do último.
                let remainder = s % blksize;
                if s != 0 && s < blksize {
                    i_size = s;
                }
                if remainder != 0 {
                    s = s.saturating_add(blksize - remainder);
                }
            }
            Some(s)
        }
        None => {
            let mut f = file;
            match f.seek(SeekFrom::End(0)) {
                Ok(s) if s > 0 => Some(s),
                _ => None,
            }
        }
    };

    let passes = genpattern(flags.iterations, rng)?;
    let total = flags.iterations + usize::from(flags.zero);
    let mut ok = true;
    for (i, kind) in passes.iter().copied().chain(flags.zero.then_some(0)).enumerate() {
        match do_pass(file, qname, &mut size, kind, rng, i + 1, total, flags.verbose)? {
            PassResult::Ok => {}
            PassResult::Failed => ok = false,
            PassResult::Fatal => return Ok(false),
        }
    }

    if flags.remove != RemoveMethod::None {
        if let Err(e) = file.set_len(0)
            && (ft.is_file() || meta.file_type().is_dir())
        {
            let ctx = format!("{qname}: error truncating");
            show!(e.map_err_context(move || ctx));
            return Ok(false);
        }
    } else if i_size != 0 && ft.is_file() {
        // Arquivo menor que um bloco: o bloco inteiro foi sobrescrito, e o tamanho original volta.
        let _ = file.set_len(i_size);
    }
    Ok(ok)
}

enum PassResult {
    Ok,
    Failed,
    Fatal,
}

/// `dopass` do GNU: um passe do começo ao fim do arquivo, e o `fsync` no fim.
#[expect(clippy::too_many_arguments)]
fn do_pass(
    file: &File,
    qname: &str,
    size: &mut Option<u64>,
    kind: i32,
    rng: &mut RandomSource,
    pass: usize,
    total: usize,
    verbose: bool,
) -> UResult<PassResult> {
    let mut f = file;
    if let Err(e) = f.seek(SeekFrom::Start(0)) {
        let ctx = format!("{qname}: cannot rewind");
        show!(e.map_err_context(move || ctx));
        return Ok(PassResult::Fatal);
    }

    let random = kind < 0;
    let mut buf = vec![0u8; if random { NONPERIODIC_OUTPUT_SIZE } else { PERIODIC_OUTPUT_SIZE }];
    let pass_name = if random {
        "random".to_string()
    } else {
        fill_pattern(kind, &mut buf);
        format!("{:02x}{:02x}{:02x}", buf[0], buf[1], buf[2])
    };
    if verbose {
        show_error!("{qname}: pass {pass}/{total} ({pass_name})...");
    }

    let mut offset: u64 = 0;
    loop {
        let lim = match *size {
            Some(s) if offset >= s => break,
            Some(s) => (s - offset).min(buf.len() as u64) as usize,
            None => buf.len(),
        };
        if random {
            rng.read(&mut buf[..lim])?;
        }
        let mut written = 0;
        while written < lim {
            match f.write(&buf[written..lim]) {
                Ok(0) => break,
                Ok(n) => written += n,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => {
                    // Tamanho desconhecido: o fim do dispositivo é o fim do passe.
                    if size.is_none() && e.raw_os_error() == Some(ENOSPC) {
                        break;
                    }
                    let ctx = format!("{qname}: error writing at offset {}", offset + written as u64);
                    show!(e.map_err_context(move || ctx));
                    return Ok(PassResult::Fatal);
                }
            }
        }
        offset += written as u64;
        if written < lim {
            if size.is_none() {
                *size = Some(offset);
                break;
            }
            show_error!("{qname}: error writing at offset {offset}");
            uucore::error::set_exit_code(1);
            return Ok(PassResult::Fatal);
        }
    }
    if size.is_none() {
        *size = Some(offset);
    }

    if let Err(e) = file.sync_data()
        && e.raw_os_error() != Some(EINVAL)
        && e.raw_os_error() != Some(EBADF)
    {
        let ctx = format!("{qname}: fdatasync failed");
        show!(e.map_err_context(move || ctx));
        return Ok(PassResult::Failed);
    }
    Ok(PassResult::Ok)
}

/// `incname` do GNU: o próximo nome do mesmo tamanho na ordem do `NAMESET`; `false` quando acabou.
fn inc_name(name: &mut [u8]) -> bool {
    for i in (0..name.len()).rev() {
        let pos = NAMESET.iter().position(|&c| c == name[i]).unwrap_or(NAMESET.len() - 1);
        if pos + 1 < NAMESET.len() {
            name[i] = NAMESET[pos + 1];
            return true;
        }
        name[i] = NAMESET[0];
    }
    false
}

/// `wipename` do GNU: renomeia o arquivo para nomes cada vez mais curtos (com `wipe`) e remove.
fn wipe_name(name: &OsStr, qname: &str, flags: &Flags) {
    let mut oldname = name.as_bytes().to_vec();
    let trimmed_len = oldname.iter().rposition(|&c| c != b'/').map_or(0, |i| i + 1);
    let base_start = oldname[..trimmed_len].iter().rposition(|&c| c == b'/').map_or(0, |i| i + 1);
    let dir: Vec<u8> = if base_start == 0 { b".".to_vec() } else { oldname[..base_start].to_vec() };

    let dir_fd = (flags.remove == RemoveMethod::WipeSync)
        .then(|| OpenOptions::new().read(true).open(OsStr::from_bytes(&dir)).ok())
        .flatten();

    if flags.verbose {
        show_error!("{qname}: removing");
    }

    let mut ok = true;
    if flags.remove != RemoveMethod::Unlink {
        let mut first = true;
        let base_len = trimmed_len - base_start;
        for len in (1..=base_len).rev() {
            let mut base = vec![NAMESET[0]; len];
            let renamed = loop {
                let mut newname = oldname[..base_start].to_vec();
                newname.extend_from_slice(&base);
                let new_path = OsStr::from_bytes(&newname);
                // `renameat2(RENAME_NOREPLACE)`: um nome que já existe passa para o próximo.
                if fs::symlink_metadata(new_path).is_ok() {
                    if inc_name(&mut base) {
                        continue;
                    }
                    break None;
                }
                match fs::rename(OsStr::from_bytes(&oldname), new_path) {
                    Ok(()) => break Some(newname),
                    Err(e) if e.kind() == ErrorKind::AlreadyExists && inc_name(&mut base) => {}
                    Err(_) => break None,
                }
            };
            if let Some(newname) = renamed {
                if let Some(d) = &dir_fd
                    && let Err(e) = d.sync_all()
                    && e.raw_os_error() != Some(EINVAL)
                    && e.raw_os_error() != Some(EBADF)
                {
                    let ctx = format!("{}: fsync failed", OsStr::from_bytes(&dir).maybe_quote());
                    show!(e.map_err_context(move || ctx));
                    ok = false;
                }
                if flags.verbose {
                    let old = if first { qname.to_string() } else { String::from_utf8_lossy(&oldname).into_owned() };
                    show_error!("{old}: renamed to {}", String::from_utf8_lossy(&newname));
                    first = false;
                }
                oldname = newname;
            }
        }
    }

    match fs::remove_file(OsStr::from_bytes(&oldname)) {
        Err(e) => {
            let ctx = format!("{qname}: failed to remove");
            show!(e.map_err_context(move || ctx));
            ok = false;
        }
        Ok(()) => {
            if flags.verbose {
                show_error!("{qname}: removed");
            }
        }
    }
    if let Some(d) = &dir_fd
        && let Err(e) = d.sync_all()
        && e.raw_os_error() != Some(EINVAL)
        && e.raw_os_error() != Some(EBADF)
    {
        let ctx = format!("{}: fsync failed", OsStr::from_bytes(&dir).maybe_quote());
        show!(e.map_err_context(move || ctx));
        ok = false;
    }
    if !ok {
        uucore::error::set_exit_code(1);
    }
}

pub fn uu_app() -> Command {
    Command::new("shred")
        .version(uucore::crate_version!())
        .help_template(uucore::localized_help_template("shred"))
        .about(translate!("shred-about"))
        .override_usage(format_usage(&translate!("shred-usage")))
        .after_help(translate!("shred-after-help"))
        .infer_long_args(true)
        .arg(
            Arg::new(options::FORCE)
                .short('f')
                .long(options::FORCE)
                .help(translate!("shred-help-force"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::ITERATIONS)
                .short('n')
                .long(options::ITERATIONS)
                .value_name("N")
                .help(translate!("shred-help-iterations"))
                .allow_hyphen_values(true),
        )
        .arg(
            Arg::new(options::RANDOM_SOURCE)
                .long(options::RANDOM_SOURCE)
                .value_name("FILE")
                .help(translate!("shred-help-random-source"))
                .value_hint(clap::ValueHint::FilePath),
        )
        .arg(
            Arg::new(options::SIZE)
                .short('s')
                .long(options::SIZE)
                .value_name("N")
                .help(translate!("shred-help-size"))
                .allow_hyphen_values(true),
        )
        .arg(
            Arg::new(options::U)
                .short('u')
                .help(translate!("shred-help-u"))
                .action(ArgAction::Count),
        )
        .arg(
            Arg::new(options::REMOVE)
                .long(options::REMOVE)
                .value_name("HOW")
                .help(translate!("shred-help-remove"))
                .num_args(0..=1)
                .require_equals(true)
                .default_missing_value("wipesync")
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new(options::VERBOSE)
                .short('v')
                .long(options::VERBOSE)
                .help(translate!("shred-help-verbose"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::EXACT)
                .short('x')
                .long(options::EXACT)
                .help(translate!("shred-help-exact"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::ZERO)
                .short('z')
                .long(options::ZERO)
                .help(translate!("shred-help-zero"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(options::FILE)
                .action(ArgAction::Append)
                .value_parser(ValueParser::os_string())
                .value_hint(clap::ValueHint::FilePath),
        )
}
