//! `fallocate` do util-linux 2.41 (pacote util-linux do Debian 13): reserva ou libera espaço de um
//! arquivo.
//!
//! Porte do `sys-utils/fallocate.c`. Sem modo, abre com `O_CREAT` e chama `fallocate(2)` com modo 0;
//! `-n`, `-p`, `-c`, `-i` e `-z` viram os bits `FALLOC_FL_*`. `-d` percorre as áreas de dados
//! (`SEEK_DATA`/`SEEK_HOLE`), lê bloco a bloco e fura com `PUNCH_HOLE` os trechos zerados. `-x` usa o
//! `posix_fallocate(3)` da glibc, que cai na escrita de um zero por bloco quando o sistema de arquivos
//! não suporta o syscall (e cujo erro o original nunca detecta, porque compara o retorno com `< 0`).
//! A 2.41 ainda não tem `-w/--write-zeroes` (veio na 2.42, junto do `FALLOC_FL_WRITE_ZEROES`).

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, FallocFlags, Fd, FileType, OFlags, Whence, sys};
use ul_common::fsutil::size_to_human_string;

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("keep-size", HasArg::No, b'n' as i32),
    LongOpt::new("punch-hole", HasArg::No, b'p' as i32),
    LongOpt::new("collapse-range", HasArg::No, b'c' as i32),
    LongOpt::new("dig-holes", HasArg::No, b'd' as i32),
    LongOpt::new("insert-range", HasArg::No, b'i' as i32),
    LongOpt::new("zero-range", HasArg::No, b'z' as i32),
    LongOpt::new("offset", HasArg::Required, b'o' as i32),
    LongOpt::new("length", HasArg::Required, b'l' as i32),
    LongOpt::new("posix", HasArg::No, b'x' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
];

/// O `excl[]` do original (linhas e colunas em ordem ASCII), pro `err_exclusive_options`.
const EXCL: &[&[u8]] = &[b"cdipxz", b"cinx"];

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [options] <filename>

Preallocate space to, or deallocate space from a file.

Options:
 -c, --collapse-range remove a range from the file
 -d, --dig-holes      detect zeroes and replace with holes
 -i, --insert-range   insert a hole at range, shifting existing data
 -l, --length <num>   length for range operations, in bytes
 -n, --keep-size      maintain the apparent size of the file
 -o, --offset <num>   offset for range operations, in bytes
 -p, --punch-hole     replace a range with a hole (implies -n)
 -z, --zero-range     zero and ensure allocation of a range
 -x, --posix          use posix_fallocate(3) instead of fallocate(2)
 -v, --verbose        verbose mode

 -h, --help           display this help
 -V, --version        display version

Arguments:
 Values for <num> may be followed by a suffix: KiB, MiB,
 GiB, TiB, PiB, EiB, ZiB, or YiB (where the \"iB\" is optional).

For more details see fallocate(1).
"
    )
}

/// Falha fatal: o texto já formatado vai pro stderr e o código de saída é 1 (`EXIT_FAILURE`).
struct Fatal;

/// `cvtnum`: `strtosize`, com -1 pra qualquer erro; acima de `i64::MAX` vira negativo (o `loff_t`).
fn cvtnum(s: &[u8]) -> i64 {
    match ul::parse_size(s) {
        Ok(x) => x as i64,
        Err(_) => -1,
    }
}

/// `xfallocate`: `fallocate(2)`, com a mensagem especial de EOPNOTSUPP sob `KEEP_SIZE`.
fn xfallocate(short: &str, fd: Fd, mode: FallocFlags, offset: i64, len: i64) -> Result<(), Fatal> {
    if let Err(e) = sys::fallocate(fd, mode, offset, len) {
        if mode.contains(FallocFlags::KEEP_SIZE) && e == Errno::EOPNOTSUPP {
            ul::warnx(short, "fallocate failed: keep size mode is unsupported");
        } else {
            ul::warn(short, "fallocate failed", e);
        }
        return Err(Fatal);
    }
    Ok(())
}

/// `posix_fallocate(3)` da glibc 2.41: o syscall com modo 0 e, se o sistema de arquivos não o
/// suporta, a emulação que escreve um byte zero em cada bloco ainda não alocado. Devolve o errno
/// (o original ignora: compara o retorno, que é positivo, com `< 0`).
fn posix_fallocate(fd: Fd, offset: i64, len: i64) -> Result<(), Errno> {
    match sys::fallocate(fd, FallocFlags::empty(), offset, len) {
        Err(Errno::EOPNOTSUPP) => {}
        other => return other,
    }
    let s = sys::current();
    if offset < 0 || len < 0 {
        return Err(Errno::EINVAL);
    }
    if offset.checked_add(len).is_none() {
        return Err(Errno::EFBIG);
    }
    let flags = s.get_status_flags(fd)?;
    if !flags.writable() {
        return Err(Errno::EBADF);
    }
    let st = s.fstat(fd)?;
    if st.file_type() != FileType::Regular {
        return Err(Errno::ENODEV);
    }
    if len == 0 {
        if (st.size as i64) < offset {
            s.ftruncate(fd, offset as u64)?;
        }
        return Ok(());
    }
    let mut increment = st.blksize as i64;
    if increment == 0 {
        increment = 512;
    } else if increment > 4096 {
        increment = 4096;
    }
    let mut len = len;
    let mut off = offset + (len - 1) % increment;
    while len > 0 {
        len -= increment;
        if off < st.size as i64 {
            let mut c = [0u8; 1];
            let n = s.pread(fd, &mut c, off as u64)?;
            if n == 1 && c[0] != 0 {
                off += increment;
                continue;
            }
        }
        if s.pwrite(fd, b"\0", off as u64)? != 1 {
            return Err(Errno::EIO);
        }
        off += increment;
    }
    Ok(())
}

/// Bloco só de zeros (o `is_nul` do original).
fn is_nul(buf: &[u8]) -> bool {
    buf.iter().all(|b| *b == 0)
}

/// `dig_holes`: fura os blocos zerados das áreas de dados de `[file_off, file_off + len)` (com `len`
/// 0, até o fim do arquivo).
fn dig_holes(
    short: &str,
    filename: &[u8],
    verbose: bool,
    fd: Fd,
    file_off: i64,
    len: i64,
) -> Result<(), Fatal> {
    let s = sys::current();
    let name = io::lossy(filename);
    let file_end = if len != 0 { file_off + len } else { 0 };
    let mut file_off = file_off;
    let mut hole_start: i64 = 0;
    let mut hole_sz: i64 = 0;
    let mut ct: u64 = 0;
    let punch = FallocFlags::PUNCH_HOLE | FallocFlags::KEEP_SIZE;

    let st = match s.fstat(fd) {
        Ok(st) => st,
        Err(e) => {
            ul::warn(short, format!("stat of {name} failed"), e);
            return Err(Fatal);
        }
    };
    let bufsz = st.blksize.max(1) as usize;
    if let Err(e) = s.lseek(fd, file_off, Whence::Set) {
        ul::warn(short, format!("seek on {name} failed"), e);
        return Err(Fatal);
    }
    let mut buf = vec![0u8; bufsz];

    while file_end == 0 || file_off < file_end {
        // a próxima área de dados (pula os buracos)
        // ENXIO (sem dados depois de file_off) encerra; qualquer outro erro também (o `off < 0`).
        let Ok(off) = s.lseek(fd, file_off, Whence::Data) else {
            break;
        };
        let off = off as i64;
        if file_end != 0 && off >= file_end {
            break;
        }
        let mut end = match s.lseek(fd, off, Whence::Hole) {
            Ok(e) => e as i64,
            Err(_) => break,
        };
        if file_end != 0 && end > file_end {
            end = file_end;
        }

        let mut off = off;
        while off < end {
            let mut rsz = match s.pread(fd, &mut buf, off as u64) {
                Ok(n) => n as i64,
                Err(e) => {
                    ul::warn(short, format!("{name}: read failed"), e);
                    return Err(Fatal);
                }
            };
            if end != 0 && rsz > 0 && off > end - rsz {
                rsz = end - off;
            }
            if rsz <= 0 {
                break;
            }
            if is_nul(&buf[..rsz as usize]) {
                if hole_sz == 0 {
                    hole_start = off;
                }
                hole_sz += rsz;
            } else if hole_sz != 0 {
                xfallocate(short, fd, punch, hole_start, hole_sz)?;
                ct += hole_sz as u64;
                hole_sz = 0;
                hole_start = 0;
            }
            off += rsz;
        }
        if hole_sz != 0 {
            let mut alloc_sz = hole_sz;
            if off >= end {
                alloc_sz += st.blksize as i64; // chega à fronteira do bloco
            }
            xfallocate(short, fd, punch, hole_start, alloc_sz)?;
            ct += hole_sz as u64;
            hole_sz = 0;
            hole_start = 0;
        }
        file_off = off;
    }

    if verbose {
        let mut line = filename.to_vec();
        line.extend_from_slice(
            format!(
                ": {} ({ct} bytes) converted to sparse holes.\n",
                size_to_human_string(ct, false, false)
            )
            .as_bytes(),
        );
        let _ = io::stdout().write_all(&line);
    }
    Ok(())
}

/// `err_exclusive_options`: a mensagem e a saída 1 quando duas opções da mesma linha se encontram.
fn check_exclusive(short: &str, c: i32, status: &mut [i32; 2]) -> Result<(), Fatal> {
    for (row, opts) in EXCL.iter().enumerate() {
        for &op in opts.iter() {
            if i32::from(op) > c {
                break;
            }
            if i32::from(op) != c {
                continue;
            }
            if status[row] == 0 {
                status[row] = c;
            } else if status[row] != c {
                let mut msg = format!("{short}: mutually exclusive arguments:");
                for &o in opts.iter() {
                    if let Some(l) = LONGS.iter().find(|l| l.id == i32::from(o)) {
                        msg.push_str(&format!(" --{}", l.name));
                    }
                }
                msg.push('\n');
                io::eprint(msg);
                return Err(Fatal);
            }
            break;
        }
    }
    Ok(())
}

fn run(args: &[OsString]) -> i32 {
    real_main(args).unwrap_or(1)
}

fn real_main(args: &[OsString]) -> Result<i32, Fatal> {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut mode = FallocFlags::empty();
    let mut dig = false;
    let mut posix = false;
    let mut verbose = false;
    let mut length: i64 = -2;
    let mut offset: i64 = 0;
    let mut excl_status = [0i32; 2];

    let mut g = Getopt::from_env(&argv[1..], "hvVncpdizxl:o:", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return Err(Fatal);
            }
        };
        check_exclusive(&short, o.id, &mut excl_status)?;
        match o.short() {
            Some('c') => mode |= FallocFlags::COLLAPSE_RANGE,
            Some('d') => dig = true,
            Some('i') => mode |= FallocFlags::INSERT_RANGE,
            Some('l') => length = cvtnum(o.arg.as_deref().unwrap_or_default()),
            Some('n') => mode |= FallocFlags::KEEP_SIZE,
            Some('o') => offset = cvtnum(o.arg.as_deref().unwrap_or_default()),
            Some('p') => mode |= FallocFlags::PUNCH_HOLE | FallocFlags::KEEP_SIZE,
            Some('z') => mode |= FallocFlags::ZERO_RANGE,
            Some('x') => posix = true,
            Some('v') => verbose = true,
            Some('h') => {
                let _ = io::stdout().write_all(usage(&short).as_bytes());
                return Ok(0);
            }
            Some('V') => {
                ul::print_version(&short);
                return Ok(0);
            }
            _ => {
                ul::errtryhelp(&short);
                return Err(Fatal);
            }
        }
    }

    let operands = g.operands();
    if operands.is_empty() {
        ul::warnx(&short, "no filename specified");
        return Err(Fatal);
    }
    if operands.len() != 1 {
        ul::warnx(&short, "unexpected number of arguments");
        return Err(Fatal);
    }
    let filename = &operands[0];

    if dig {
        // com --dig-holes o padrão é analisar o arquivo inteiro
        if length == -2 {
            length = 0;
        }
        if length < 0 {
            ul::warnx(&short, "invalid length value specified");
            return Err(Fatal);
        }
    } else {
        // exigir a faixa (--length --offset) é mais seguro
        if length == -2 {
            ul::warnx(&short, "no length argument specified");
            return Err(Fatal);
        }
        if length <= 0 {
            ul::warnx(&short, "invalid length value specified");
            return Err(Fatal);
        }
    }
    if offset < 0 {
        ul::warnx(&short, "invalid offset value specified");
        return Err(Fatal);
    }

    // O_CREAT só faz sentido na alocação comum, sem modo.
    let mut flags = OFlags::RDWR;
    if !dig && mode.is_empty() {
        flags |= OFlags::CREAT;
    }
    let fd = match sys::open(filename, flags, 0o666) {
        Ok(fd) => fd,
        Err(e) => {
            ul::warn(&short, format!("cannot open {}", io::lossy(filename)), e);
            return Err(Fatal);
        }
    };

    let result = if dig {
        dig_holes(&short, filename, verbose, fd, offset, length)
    } else {
        let r = if posix {
            let _ = posix_fallocate(fd, offset, length);
            Ok(())
        } else {
            xfallocate(&short, fd, mode, offset, length)
        };
        if r.is_ok() && verbose {
            let what = if mode.contains(FallocFlags::PUNCH_HOLE) {
                "hole created"
            } else if mode.contains(FallocFlags::COLLAPSE_RANGE) {
                "removed"
            } else if mode.contains(FallocFlags::INSERT_RANGE) {
                "inserted"
            } else if mode.contains(FallocFlags::ZERO_RANGE) {
                "zeroed"
            } else {
                "allocated"
            };
            let mut line = filename.clone();
            line.extend_from_slice(
                format!(
                    ": {} ({} bytes) {what}.\n",
                    size_to_human_string(length as u64, false, false),
                    length as u64
                )
                .as_bytes(),
            );
            let _ = io::stdout().write_all(&line);
        }
        r
    };
    result?;

    if let Err(e) = sys::close(fd) {
        ul::warn(&short, format!("write failed: {}", io::lossy(filename)), e);
        return Err(Fatal);
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cvtnum_values() {
        assert_eq!(cvtnum(b"1KiB"), 1024);
        assert_eq!(cvtnum(b"1MB"), 1_000_000);
        assert_eq!(cvtnum(b"x"), -1);
        assert_eq!(cvtnum(b"-5"), -1);
    }
}
