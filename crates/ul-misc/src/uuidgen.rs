//! `uuidgen` do util-linux 2.41 (libuuid): UUID v4 por padrão (`-r`), v1 com `-t`, e v3/v5 (MD5 e
//! SHA-1 de namespace mais nome) com `-m`/`-s`, `-n` e `-N`. Saída em minúsculas, formato 8-4-4-4-12.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, sys};

use crate::util::io;
use crate::util::md5::Md5;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("random", HasArg::No, b'r' as i32),
    LongOpt::new("time", HasArg::No, b't' as i32),
    LongOpt::new("namespace", HasArg::Required, b'n' as i32),
    LongOpt::new("name", HasArg::Required, b'N' as i32),
    LongOpt::new("md5", HasArg::No, b'm' as i32),
    LongOpt::new("sha1", HasArg::No, b's' as i32),
    LongOpt::new("hex", HasArg::No, b'x' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 uuidgen [options]

Create a new UUID value.

Options:
 -r, --random        generate random-based uuid
 -t, --time          generate time-based uuid
 -n, --namespace <ns>  generate hash-based uuid in this namespace
                     available namespaces: @dns @url @oid @x500
 -N, --name <name>   generate hash-based uuid from this name
 -m, --md5           generate md5 hash
 -s, --sha1          generate sha1 hash
 -x, --hex           interpret name as hex string
 -h, --help          display this help
 -V, --version       display version

For more details see uuidgen(1).
";

#[derive(PartialEq, Clone, Copy)]
enum Kind {
    Default,
    Random,
    Time,
    Md5,
    Sha1,
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn random_bytes(buf: &mut [u8]) {
    let mut filled = 0;
    while filled < buf.len() {
        match sys::current().getrandom(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(Errno::EINTR) => {}
            Err(_) => break,
        }
    }
}

fn parse_uuid(s: &[u8]) -> Option<[u8; 16]> {
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
        let hi = (s[i] as char).to_digit(16)?;
        let lo = (s[i + 1] as char).to_digit(16)?;
        out[n] = (hi * 16 + lo) as u8;
        n += 1;
        i += 2;
    }
    Some(out)
}

fn namespace(arg: &[u8]) -> Option<[u8; 16]> {
    let s = match arg {
        b"@dns" => &b"6ba7b810-9dad-11d1-80b4-00c04fd430c8"[..],
        b"@url" => b"6ba7b811-9dad-11d1-80b4-00c04fd430c8",
        b"@oid" => b"6ba7b812-9dad-11d1-80b4-00c04fd430c8",
        b"@x500" => b"6ba7b814-9dad-11d1-80b4-00c04fd430c8",
        other => other,
    };
    parse_uuid(s)
}

fn fmt_uuid(u: &[u8; 16]) -> String {
    let mut s = String::with_capacity(37);
    for (i, b) in u.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            s.push('-');
        }
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn decode_hex(s: &[u8]) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    s.chunks(2)
        .map(|c| {
            let hi = (c[0] as char).to_digit(16)?;
            let lo = (c[1] as char).to_digit(16)?;
            Some((hi * 16 + lo) as u8)
        })
        .collect()
}

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

fn time_uuid() -> [u8; 16] {
    // 100 ns desde 1582-10-15
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let ticks: u64 = now.as_secs() * 10_000_000
        + (now.subsec_nanos() as u64) / 100
        + 0x01B2_1DD2_1381_4000;
    let mut r = [0u8; 8];
    random_bytes(&mut r);
    let clock_seq = u16::from_be_bytes([r[0], r[1]]) & 0x3fff;
    let mut u = [0u8; 16];
    u[0..4].copy_from_slice(&((ticks & 0xffff_ffff) as u32).to_be_bytes());
    u[4..6].copy_from_slice(&(((ticks >> 32) & 0xffff) as u16).to_be_bytes());
    u[6..8].copy_from_slice(&((((ticks >> 48) & 0x0fff) as u16) | 0x1000).to_be_bytes());
    u[8..10].copy_from_slice(&(clock_seq | 0x8000).to_be_bytes());
    u[10..16].copy_from_slice(&r[2..8]);
    u[10] |= 0x01; // nó aleatório: bit multicast ligado
    u
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut kind = Kind::Default;
    let mut ns: Option<Vec<u8>> = None;
    let mut name: Option<Vec<u8>> = None;
    let mut hex = false;

    let mut g = Getopt::from_env(&argv[1..], "rtn:N:msxVh", LONGS);
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
            Some('r') => kind = Kind::Random,
            Some('t') => kind = Kind::Time,
            Some('m') => kind = Kind::Md5,
            Some('s') => kind = Kind::Sha1,
            Some('x') => hex = true,
            Some('n') => ns = Some(o.arg.clone().unwrap_or_default()),
            Some('N') => name = Some(o.arg.clone().unwrap_or_default()),
            Some('V') => {
                ul::print_version(&short);
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

    let hashed = matches!(kind, Kind::Md5 | Kind::Sha1);
    if (ns.is_some() || name.is_some()) && !hashed {
        ul::warnx(
            &short,
            "--namespace and --name options are allowed only with --md5 or --sha1",
        );
        return 1;
    }
    let uuid = if hashed {
        let (Some(ns), Some(name)) = (ns, name) else {
            ul::warnx(
                &short,
                "--md5 and --sha1 require both --namespace and --name",
            );
            return 1;
        };
        let Some(nsb) = namespace(&ns) else {
            ul::warnx(
                &short,
                format!("unknown namespace: {}", io::lossy(&ns)),
            );
            return 1;
        };
        let name = if hex {
            match decode_hex(&name) {
                Some(v) => v,
                None => {
                    ul::warnx(&short, "invalid hex string for --name");
                    return 1;
                }
            }
        } else {
            name
        };
        let mut input = nsb.to_vec();
        input.extend_from_slice(&name);
        let mut u = [0u8; 16];
        if kind == Kind::Md5 {
            let mut m = Md5::new();
            m.update(&input);
            u.copy_from_slice(&m.finish());
            u[6] = (u[6] & 0x0f) | 0x30;
        } else {
            u.copy_from_slice(&sha1(&input)[..16]);
            u[6] = (u[6] & 0x0f) | 0x50;
        }
        u[8] = (u[8] & 0x3f) | 0x80;
        u
    } else if kind == Kind::Time {
        time_uuid()
    } else {
        let mut u = [0u8; 16];
        random_bytes(&mut u);
        u[6] = (u[6] & 0x0f) | 0x40;
        u[8] = (u[8] & 0x3f) | 0x80;
        u
    };

    let mut out = io::stdout();
    let _ = out.write_all(format!("{}\n", fmt_uuid(&uuid)).as_bytes());
    if out.flush().is_err() {
        return 1;
    }
    0
}
