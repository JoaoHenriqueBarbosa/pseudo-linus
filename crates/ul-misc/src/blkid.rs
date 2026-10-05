//! `blkid` do util-linux 2.41: identifica sistemas de arquivos e áreas de swap por rótulo, UUID e tipo.
//!
//! O sandbox não tem dispositivos de bloco nem cache do blkid, então sem alvo a listagem é vazia e a
//! saída é 2, como no oráculo em container. Com alvos explícitos (inclusive arquivos regulares) o
//! módulo sonda os superblocos de ext2/3/4, vfat, xfs e swap. Códigos de saída: 0 achou, 2 nada
//! encontrado, 4 erro de uso, 8 erro de ambiente.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, OFlags, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("cache-file", HasArg::Required, b'c' as i32),
    LongOpt::new("no-encoding", HasArg::No, b'd' as i32),
    LongOpt::new("garbage-collect", HasArg::No, b'g' as i32),
    LongOpt::new("output", HasArg::Required, b'o' as i32),
    LongOpt::new("list-filesystems", HasArg::No, b'k' as i32),
    LongOpt::new("match-tag", HasArg::Required, b's' as i32),
    LongOpt::new("match-token", HasArg::Required, b't' as i32),
    LongOpt::new("list-one", HasArg::No, b'l' as i32),
    LongOpt::new("label", HasArg::Required, b'L' as i32),
    LongOpt::new("uuid", HasArg::Required, b'U' as i32),
    LongOpt::new("probe", HasArg::No, b'p' as i32),
    LongOpt::new("info", HasArg::No, b'i' as i32),
    LongOpt::new("hint", HasArg::Required, b'H' as i32),
    LongOpt::new("size", HasArg::Required, b'S' as i32),
    LongOpt::new("offset", HasArg::Required, b'O' as i32),
    LongOpt::new("usages", HasArg::Required, b'u' as i32),
    LongOpt::new("match-types", HasArg::Required, b'n' as i32),
    LongOpt::new("no-part-details", HasArg::No, b'D' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 blkid --label <label> | --uuid <uuid>

 blkid [--cache-file <file>] [-ghlLv] [--output <format>] [--match-tag <tag>]\x20
       [--match-token <token>] [<dev> ...]

 blkid -p [--match-tag <tag>] [--offset <offset>] [--size <size>]\x20
       [--output <format>] <dev> ...

 blkid -i [--match-tag <tag>] [--output <format>] <dev> ...

Options:
 -c, --cache-file <file>    read from <file> instead of reading from the default
                              cache file (-c /dev/null means no cache)
 -d, --no-encoding          don't encode non-printing characters
 -g, --garbage-collect      garbage collect the blkid cache
 -o, --output <format>      output format; can be one of:
                              value, device, export, json or full; (default: full)
 -k, --list-filesystems     list all known filesystems/RAIDs and exit
 -s, --match-tag <tag>      show specified tag(s) (default show all tags)
 -t, --match-token <token>  find device with a specific token (NAME=value pair)
 -l, --list-one             look up only first device with token specified by -t
 -L, --label <label>        convert LABEL to device name
 -U, --uuid <uuid>          convert UUID to device name

Low-level probing options:
 -p, --probe                low-level superblocks probing (bypass cache)
 -i, --info                 gather information about I/O limits
 -H, --hint <value>         set hint for probing function
 -S, --size <size>          override device size
 -O, --offset <offset>      probe at the given offset
 -u, --usages <list>        filter by \"usage\" (e.g. -u filesystem,raid)
 -n, --match-types <list>   filter by filesystem type (e.g. -n vfat,ext3)
 -D, --no-part-details      don't print info from partition table

 -h, --help                 display this help
 -V, --version              display version

Arguments:
 Values for <size> and <offset> may be followed by a suffix: KiB, MiB,
 GiB, TiB, PiB, EiB, ZiB, or YiB (where the \"iB\" is optional).

 <dev> specify device(s) to probe (default: all devices)

For more details see blkid(8).
";

/// Assinatura de superbloco reconhecida.
pub struct Signature {
    pub ty: &'static str,
    pub usage: &'static str,
    pub label: Option<Vec<u8>>,
    pub uuid: Option<String>,
    /// Deslocamento e conteúdo do número mágico (para o wipefs).
    pub magic_offset: usize,
    pub magic: &'static [u8],
}

/// Quantos bytes iniciais bastam para sondar todos os formatos suportados.
pub const PROBE_LEN: usize = 8192;

fn fmt_uuid(b: &[u8]) -> Option<String> {
    if b.iter().all(|&x| x == 0) {
        return None;
    }
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Some(format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32]))
}

fn cstr(b: &[u8]) -> Option<Vec<u8>> {
    let end = b.iter().position(|&x| x == 0).unwrap_or(b.len());
    if end == 0 { None } else { Some(b[..end].to_vec()) }
}

fn le32(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

/// Sonda os primeiros bytes de um dispositivo ou arquivo.
pub fn probe(d: &[u8]) -> Option<Signature> {
    // swap
    if d.len() >= 4096 {
        let m = &d[4086..4096];
        if m == b"SWAPSPACE2" || m == b"SWAP-SPACE" {
            let v2 = m == b"SWAPSPACE2";
            return Some(Signature {
                ty: "swap",
                usage: "other",
                label: if v2 && d.len() >= 1068 { cstr(&d[1052..1068]) } else { None },
                uuid: if v2 && d.len() >= 1052 { fmt_uuid(&d[1036..1052]) } else { None },
                magic_offset: 4086,
                magic: if v2 { b"SWAPSPACE2" } else { b"SWAP-SPACE" },
            });
        }
    }
    // xfs
    if d.len() >= 120 && &d[0..4] == b"XFSB" {
        return Some(Signature {
            ty: "xfs",
            usage: "filesystem",
            label: cstr(&d[108..120]),
            uuid: fmt_uuid(&d[32..48]),
            magic_offset: 0,
            magic: b"XFSB",
        });
    }
    // ext2/3/4
    if d.len() >= 1024 + 264 && d[1080] == 0x53 && d[1081] == 0xEF {
        let sb = &d[1024..];
        let compat = le32(sb, 0x5c);
        let incompat = le32(sb, 0x60);
        let ro = le32(sb, 0x64);
        let ty = if incompat & 0x0004 != 0 {
            "jbd"
        } else if incompat & (0x0040 | 0x0080 | 0x0200 | 0x0400) != 0 || ro & (0x0008 | 0x0010 | 0x0020 | 0x0040 | 0x2000) != 0 {
            "ext4"
        } else if compat & 0x0004 != 0 {
            "ext3"
        } else {
            "ext2"
        };
        return Some(Signature {
            ty,
            usage: "filesystem",
            label: cstr(&sb[120..136]),
            uuid: fmt_uuid(&sb[104..120]),
            magic_offset: 0x438,
            magic: &[0x53, 0xEF],
        });
    }
    // vfat
    if d.len() >= 512 && d[510] == 0x55 && d[511] == 0xAA {
        let fat32 = &d[82..90] == b"FAT32   ";
        let old = &d[54..62] == b"FAT16   " || &d[54..62] == b"FAT12   " || &d[54..59] == b"FAT  ";
        if fat32 || old {
            let (serial, lab, off, magic): (u32, &[u8], usize, &'static [u8]) = if fat32 {
                (le32(d, 67), &d[71..82], 0x52, b"FAT32   ")
            } else if &d[54..62] == b"FAT12   " {
                (le32(d, 39), &d[43..54], 0x36, b"FAT12   ")
            } else {
                (le32(d, 39), &d[43..54], 0x36, b"FAT16   ")
            };
            let mut l = lab.to_vec();
            while l.last() == Some(&b' ') {
                l.pop();
            }
            let label = if l.is_empty() || l == b"NO NAME" { None } else { Some(l) };
            return Some(Signature {
                ty: "vfat",
                usage: "filesystem",
                label,
                uuid: Some(format!("{:04X}-{:04X}", serial >> 16, serial & 0xffff)),
                magic_offset: off,
                magic,
            });
        }
    }
    None
}

/// Lê até `PROBE_LEN` bytes do caminho. `Err` traz o errno.
pub fn read_head(path: &[u8]) -> Result<Vec<u8>, sysabi::Errno> {
    let fd = sys::open(path, OFlags::RDONLY, 0)?;
    let mut buf = vec![0u8; PROBE_LEN];
    let mut got = 0;
    while got < PROBE_LEN {
        match sys::read(fd, &mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(sysabi::Errno::EINTR) => {}
            Err(e) => {
                let _ = sys::close(fd);
                return Err(e);
            }
        }
    }
    let _ = sys::close(fd);
    buf.truncate(got);
    Ok(buf)
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn quote(v: &[u8]) -> String {
    let mut s = String::new();
    for &c in v {
        match c {
            b'"' => s.push_str("\\\""),
            b'\\' => s.push_str("\\\\"),
            _ => s.push(c as char),
        }
    }
    s
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut output = String::from("full");
    let mut tags: Vec<String> = Vec::new();
    let mut token: Option<(String, String)> = None;
    let mut want: Option<(&'static str, String)> = None;
    let mut list_one = false;
    let mut probe_mode = false;
    let mut list_fs = false;

    let mut g = Getopt::from_env(&argv[1..], "c:dgko:s:t:lL:U:pimH:S:O:u:n:VhD", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        let arg = o.arg.clone().map(|a| io::lossy(&a)).unwrap_or_default();
        match o.short() {
            Some('c') | Some('d') | Some('g') | Some('H') | Some('i') | Some('m') | Some('u')
            | Some('n') | Some('D') => {}
            Some('o') => {
                if !matches!(arg.as_str(), "value" | "device" | "export" | "full" | "udev") {
                    ul::warnx(&short, format!("unsupported output format {arg}"));
                    return 4;
                }
                output = arg;
            }
            Some('k') => list_fs = true,
            Some('s') => tags.push(arg),
            Some('t') => match arg.split_once('=') {
                Some((k, v)) if !k.is_empty() => token = Some((k.to_string(), v.to_string())),
                _ => {
                    ul::warnx(&short, format!("-t needs NAME=value pair"));
                    return 4;
                }
            },
            Some('l') => list_one = true,
            Some('L') => want = Some(("LABEL", arg)),
            Some('U') => want = Some(("UUID", arg)),
            Some('p') => probe_mode = true,
            Some('S') | Some('O') => {
                if arg.parse::<u64>().is_err() {
                    ul::warnx(&short, format!("invalid size argument: '{arg}'"));
                    return 4;
                }
            }
            Some('V') => {
                let mut out = io::stdout();
                let _ = out.write_all(
                    format!("{short} from util-linux 2.41.5  (libblkid 2.41.5, 16-Jun-2026)\n").as_bytes(),
                );
                return 0;
            }
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }

    if list_fs {
        let mut out = io::stdout();
        for n in ["ext2", "ext3", "ext4", "swap", "vfat", "xfs"] {
            let _ = out.write_all(format!("{n}\n").as_bytes());
        }
        return 0;
    }

    let devs = g.operands();
    if probe_mode && devs.is_empty() {
        ul::warnx(&short, "no device specified".to_string());
        return 4;
    }

    let mut found = false;
    let mut out = io::stdout();
    // Sem alvo, a lista de dispositivos do sistema é vazia no sandbox.
    for d in &devs {
        let Ok(head) = read_head(d) else { continue };
        let Some(sig) = probe(&head) else { continue };
        if let Some((k, v)) = want.as_ref().map(|(k, v)| (k.to_string(), v.clone())).or(token.clone()) {
            let have = match k.as_str() {
                "TYPE" => Some(sig.ty.as_bytes().to_vec()),
                "LABEL" => sig.label.clone(),
                "UUID" => sig.uuid.clone().map(String::into_bytes),
                _ => None,
            };
            if have.as_deref() != Some(v.as_bytes()) {
                continue;
            }
        }
        found = true;
        let name = io::lossy(d);
        let mut kv: Vec<(&str, Vec<u8>)> = Vec::new();
        if let Some(l) = &sig.label {
            kv.push(("LABEL", l.clone()));
        }
        if let Some(u) = &sig.uuid {
            kv.push(("UUID", u.clone().into_bytes()));
        }
        kv.push(("TYPE", sig.ty.as_bytes().to_vec()));
        if !tags.is_empty() {
            kv.retain(|(k, _)| tags.iter().any(|t| t == k));
        }
        if want.is_some() {
            let _ = out.write_all(format!("{name}\n").as_bytes());
            break;
        }
        let text = match output.as_str() {
            "value" => kv.iter().map(|(_, v)| format!("{}\n", io::lossy(v))).collect::<String>(),
            "device" => format!("{name}\n"),
            "export" => {
                let mut s = format!("DEVNAME={name}\n");
                for (k, v) in &kv {
                    s.push_str(&format!("{k}={}\n", io::lossy(v)));
                }
                s.push('\n');
                s
            }
            _ => {
                let mut s = format!("{name}:");
                for (k, v) in &kv {
                    s.push_str(&format!(" {k}=\"{}\"", quote(v)));
                }
                s.push('\n');
                s
            }
        };
        let _ = out.write_all(text.as_bytes());
        if list_one {
            break;
        }
    }
    let _ = out.flush();
    if found { 0 } else { 2 }
}
