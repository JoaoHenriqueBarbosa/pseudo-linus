//! `mkswap` do util-linux 2.41: prepara um arquivo ou dispositivo como área de swap.
//!
//! Porte do `disk-utils/mkswap.c` para o caso que o sandbox produz: arquivos comuns (não há
//! dispositivos de bloco). Escreve o cabeçalho da página (versão 1, UUID em 0x40c, rótulo em 0x41c e
//! assinatura `SWAPSPACE2` no fim da página) depois de zerar os primeiros 1024 bytes.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, OFlags, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("check", HasArg::No, b'c' as i32),
    LongOpt::new("force", HasArg::No, b'f' as i32),
    LongOpt::new("quiet", HasArg::No, b'q' as i32),
    LongOpt::new("pagesize", HasArg::Required, b'p' as i32),
    LongOpt::new("label", HasArg::Required, b'L' as i32),
    LongOpt::new("swapversion", HasArg::Required, b'v' as i32),
    LongOpt::new("uuid", HasArg::Required, b'U' as i32),
    LongOpt::new("offset", HasArg::Required, b'o' as i32),
    LongOpt::new("endianness", HasArg::Required, b'e' as i32),
    LongOpt::new("lock", HasArg::Optional, 256),
    LongOpt::new("verbose", HasArg::No, 257),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

const USAGE: &str = "
Usage:
 mkswap [options] device [size]

Set up a Linux swap area.

Options:
 -c, --check               check bad blocks before creating the swap area
 -f, --force               allow swap size area be larger than device
 -q, --quiet               suppress output and warning messages
 -p, --pagesize SIZE       specify page size in bytes
 -L, --label LABEL         specify label
 -v, --swapversion NUM     specify swap-space version number
 -U, --uuid UUID           specify the uuid to use
 -e, --endianness=<value>  specify the endianness to use (native, little or big)
 -o, --offset OFFSET       specify the offset in bytes
     --lock[=<mode>]       use exclusive device lock (yes, no or nonblock)
     --verbose             verbose output

 -h, --help                display this help
 -V, --version             display version

For more details see mkswap(8).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// `size_to_human_string(SIZE_SUFFIX_3LETTER | SIZE_SUFFIX_SPACE)`: `4 KiB`, `1020 KiB`, `1.5 MiB`.
fn human(bytes: u64) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut unit = 0;
    let mut div: u64 = 1;
    while unit + 1 < UNITS.len() && bytes / div >= 1024 {
        div *= 1024;
        unit += 1;
    }
    let whole = bytes / div;
    let rem = bytes % div;
    if rem == 0 || unit == 0 {
        return format!("{whole} {}", UNITS[unit]);
    }
    let tenth = rem * 10 / div;
    if tenth == 0 {
        format!("{whole} {}", UNITS[unit])
    } else {
        format!("{whole}.{tenth} {}", UNITS[unit])
    }
}

fn valid_uuid(s: &[u8]) -> Option<[u8; 16]> {
    if s.len() != 36 {
        return None;
    }
    let mut out = [0u8; 16];
    let mut n = 0;
    let mut i = 0;
    while i < 36 {
        if matches!(i, 8 | 13 | 18 | 23) {
            if s[i] != b'-' {
                return None;
            }
            i += 1;
            continue;
        }
        let hi = char::from(s[i]).to_digit(16)?;
        let lo = char::from(*s.get(i + 1)?).to_digit(16)?;
        out[n] = (hi * 16 + lo) as u8;
        n += 1;
        i += 2;
    }
    Some(out)
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut pagesize: u64 = 4096;
    let mut label: Option<Vec<u8>> = None;
    let mut uuid: Option<[u8; 16]> = None;
    let mut quiet = false;
    let mut offset: u64 = 0;
    let mut g = Getopt::from_env(&argv[1..], "cfqp:L:v:U:e:o:Vh", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        match o.short() {
            Some('c') | Some('f') => {}
            Some('q') => quiet = true,
            Some('p') => match ul::strtou64_or_err(o.arg.as_deref().unwrap_or(b""), "parse page size failure") {
                Ok(n) => pagesize = n,
                Err(m) => {
                    ul::warnx(&short, m);
                    return 1;
                }
            },
            Some('o') => match ul::strtosize_or_err(o.arg.as_deref().unwrap_or(b""), "failed to parse offset") {
                Ok(n) => offset = n,
                Err(m) => {
                    ul::warnx(&short, m);
                    return 1;
                }
            },
            Some('L') => label = o.arg.clone(),
            Some('v') => {
                let a = o.arg_str();
                if a != "1" {
                    ul::warnx(&short, format!("swapspace version {a} is not supported"));
                    return 1;
                }
            }
            Some('U') => match valid_uuid(o.arg.as_deref().unwrap_or(b"")) {
                Some(u) => uuid = Some(u),
                None => {
                    ul::warnx(&short, format!("error: parsing UUID failed: {}", o.arg_str()));
                    return 1;
                }
            },
            Some('e') => {
                let a = o.arg_str();
                if !matches!(a.as_str(), "native" | "little" | "big") {
                    ul::warnx(&short, format!("invalid endianness argument: {a}"));
                    return 1;
                }
            }
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            _ => {
                if o.id == 256 || o.id == 257 {
                    continue;
                }
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }

    let ops = g.operands();
    if ops.is_empty() {
        ul::warnx(&short, "error: Nowhere to set up swap on?");
        ul::errtryhelp(&short);
        return 1;
    }
    let dev = ops[0].clone();
    let dev_s = io::lossy(&dev);
    let mut size_arg: Option<u64> = None;
    if ops.len() > 1 {
        match ul::strtou64_or_err(&ops[1], "invalid block count") {
            Ok(n) => size_arg = Some(n * 1024),
            Err(m) => {
                ul::warnx(&short, m);
                return 1;
            }
        }
    }
    if !pagesize.is_power_of_two() || pagesize < 1024 {
        ul::warnx(&short, format!("error: parse page size failure: {pagesize}"));
        return 1;
    }

    let st = match sys::stat(&dev) {
        Ok(s) => s,
        Err(e) => {
            ul::warn(&short, format!("cannot stat {dev_s}"), e);
            return 1;
        }
    };
    let is_blk = st.mode & 0o170000 == 0o060000;
    let is_dir = st.mode & 0o170000 == 0o040000;
    if is_dir {
        ul::warn(&short, format!("cannot open {dev_s}"), Errno::EISDIR);
        return 1;
    }
    let file = match io::File::open_with(&dev, OFlags::RDWR, 0) {
        Ok(f) => f,
        Err(e) => {
            ul::warn(&short, format!("cannot open {dev_s}"), e);
            return 1;
        }
    };
    let dev_size = size_arg.unwrap_or(st.size as u64).saturating_sub(offset);
    let pages = dev_size / pagesize;
    if pages < 10 {
        ul::warnx(
            &short,
            format!("error: swap area needs to be at least {}KiB", 10 * pagesize / 1024),
        );
        return 1;
    }
    if is_blk {
        ul::warn(&short, format!("{dev_s}: failed to write signature page"), Errno::ENOSYS);
        return 1;
    }
    if st.mode & 0o177 != 0 && !quiet {
        ul::warnx(
            &short,
            format!(
                "{dev_s}: insecure permissions {:04o}, fix with: chmod {:04o} {dev_s}",
                st.mode & 0o7777,
                st.mode & 0o7777 & !0o177
            ),
        );
    }

    let uuid = match uuid {
        Some(u) => u,
        None => {
            let mut u = [0u8; 16];
            if let Ok(mut r) = io::File::open(b"/dev/urandom") {
                let _ = r.read_full(&mut u);
            }
            u[6] = (u[6] & 0x0f) | 0x40;
            u[8] = (u[8] & 0x3f) | 0x80;
            u
        }
    };
    let mut page = vec![0u8; pagesize as usize];
    page[0x400..0x404].copy_from_slice(&1u32.to_le_bytes());
    page[0x404..0x408].copy_from_slice(&((pages - 1) as u32).to_le_bytes());
    page[0x40c..0x41c].copy_from_slice(&uuid);
    if let Some(l) = &label {
        let n = l.len().min(15);
        page[0x41c..0x41c + n].copy_from_slice(&l[..n]);
        if l.len() > 15 {
            ul::warnx(&short, format!("label is too long, truncating to {}", io::lossy(&l[..15])));
        }
    }
    let sig = b"SWAPSPACE2";
    let end = pagesize as usize;
    page[end - 10..end].copy_from_slice(sig);

    // Escreve a página no começo do arquivo (deslocamento zero, o único que a escrita sequencial cobre).
    let mut f = file;
    if offset != 0 {
        ul::warn(&short, format!("{dev_s}: failed to write signature page"), Errno::ENOSYS);
        return 1;
    }
    if let Err(e) = f.write_all(&page) {
        ul::warn(&short, format!("{dev_s}: failed to write signature page"), io::io_errno(&e));
        return 1;
    }
    if !quiet {
        let mut out = io::stdout();
        let _ = out.write_all(
            format!(
                "Setting up swapspace version 1, size = {} ({} bytes)\n",
                human((pages - 1) * pagesize),
                (pages - 1) * pagesize
            )
            .as_bytes(),
        );
        let h: String = uuid.iter().map(|b| format!("{b:02x}")).collect();
        let uuid_s = format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32]);
        let line = match &label {
            Some(l) => format!("LABEL={}, UUID={uuid_s}\n", io::lossy(&l[..l.len().min(15)])),
            None => format!("no label, UUID={uuid_s}\n"),
        };
        let _ = out.write_all(line.as_bytes());
    }
    0
}
