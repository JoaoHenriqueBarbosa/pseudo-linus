//! `utmpdump` do util-linux 2.41: despeja arquivos utmp/wtmp binários em texto e, com `-r`, faz o
//! caminho inverso.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, OFlags};

use crate::last::{UTMP_SIZE, Utmp, decode};
use crate::util::io;
use crate::util::time;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("follow", HasArg::No, b'f' as i32),
    LongOpt::new("reverse", HasArg::No, b'r' as i32),
    LongOpt::new("output", HasArg::Required, b'o' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

const USAGE: &str = "
Usage:
 utmpdump [options] [filename]

Dump UTMP and WTMP files in raw format.

Options:
 -f, --follow         output appended data as the file grows
 -r, --reverse        write back dumped data into utmp file
 -o, --output <file>  write to file instead of standard output
 -h, --help           display this help
 -V, --version        display version

For more details see utmpdump(1).
";

fn trunc_pad(b: &[u8], width: usize, max: usize) -> String {
    let s = io::lossy(&b[..b.len().min(max)]);
    format!("{s:<width$}")
}

fn addr_text(u: &Utmp) -> String {
    let a = u.addr;
    if a[1] == 0 && a[2] == 0 && a[3] == 0 {
        let b = a[0].to_le_bytes();
        return format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]);
    }
    let mut parts = Vec::new();
    for w in a {
        let b = w.to_le_bytes();
        parts.push(format!("{:x}", u16::from_be_bytes([b[0], b[1]])));
        parts.push(format!("{:x}", u16::from_be_bytes([b[2], b[3]])));
    }
    parts.join(":")
}

fn dump_record(rec: &[u8]) -> String {
    let u = decode(rec);
    let dt = time::civil(i64::from(u.sec), &jiff::tz::TimeZone::UTC);
    let stamp = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02},{:06}+00:00",
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
        u.usec
    );
    format!(
        "[{}] [{:05}] [{:<4}] [{}] [{}] [{}] [{:<15}] [{}]\n",
        u.kind,
        u.pid,
        io::lossy(&u.id[..u.id.len().min(4)]),
        trunc_pad(&u.user, 8, 32),
        trunc_pad(&u.line, 12, 32),
        trunc_pad(&u.host, 20, 256),
        addr_text(&u),
        stamp
    )
}

fn put(dst: &mut [u8], s: &[u8]) {
    let n = s.len().min(dst.len());
    dst[..n].copy_from_slice(&s[..n]);
}

fn bracket_fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(s) = rest.find('[') {
        let Some(e) = rest[s..].find(']') else { break };
        out.push(rest[s + 1..s + e].trim_end().to_string());
        rest = &rest[s + e + 1..];
    }
    out
}

fn parse_addr(s: &str) -> [i32; 4] {
    let mut a = [0i32; 4];
    let v4: Vec<u8> = s.split('.').filter_map(|p| p.trim().parse().ok()).collect();
    if v4.len() == 4 {
        a[0] = i32::from_le_bytes([v4[0], v4[1], v4[2], v4[3]]);
    }
    a
}

fn parse_stamp(s: &str) -> (i32, i32) {
    let (main, frac) = s.split_once(',').unwrap_or((s, "0"));
    let usec: i32 = frac
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    let digits: Vec<i64> = main
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .collect();
    if digits.len() < 6 {
        return (0, usec);
    }
    let dt = jiff::civil::DateTime::new(
        digits[0] as i16,
        digits[1] as i8,
        digits[2] as i8,
        digits[3] as i8,
        digits[4] as i8,
        digits[5] as i8,
        0,
    );
    let sec = dt
        .ok()
        .and_then(|d| d.to_zoned(jiff::tz::TimeZone::UTC).ok())
        .map_or(0, |z| z.timestamp().as_second() as i32);
    (sec, usec)
}

fn undump(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for line in text.lines() {
        let f = bracket_fields(line);
        if f.len() < 8 {
            continue;
        }
        let mut rec = [0u8; UTMP_SIZE];
        let kind: i16 = f[0].trim().parse().unwrap_or(0);
        rec[0..2].copy_from_slice(&kind.to_le_bytes());
        let pid: i32 = f[1].trim().parse().unwrap_or(0);
        rec[4..8].copy_from_slice(&pid.to_le_bytes());
        put(&mut rec[40..44], f[2].as_bytes());
        put(&mut rec[44..76], f[3].as_bytes());
        put(&mut rec[8..40], f[4].as_bytes());
        put(&mut rec[76..332], f[5].as_bytes());
        let a = parse_addr(&f[6]);
        for (i, w) in a.iter().enumerate() {
            rec[348 + i * 4..352 + i * 4].copy_from_slice(&w.to_le_bytes());
        }
        let (sec, usec) = parse_stamp(&f[7]);
        rec[340..344].copy_from_slice(&sec.to_le_bytes());
        rec[344..348].copy_from_slice(&usec.to_le_bytes());
        out.extend_from_slice(&rec);
    }
    out
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut reverse = false;
    let mut follow = false;
    let mut output: Option<Vec<u8>> = None;

    let mut g = Getopt::from_env(&argv[1..], "fro:hV", LONGS);
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
            Some('f') => follow = true,
            Some('r') => reverse = true,
            Some('o') => output = o.arg.clone(),
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
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }
    let _ = follow;
    let ops = g.operands();
    if ops.len() > 1 {
        ul::warnx(&short, "unexpected argument");
        ul::errtryhelp(&short);
        return 1;
    }

    let data = match ops.first() {
        Some(p) => match io::read_path(p) {
            Ok(d) => d,
            Err(e) => {
                ul::warn(&short, format!("cannot open {}", io::lossy(p)), e);
                return 1;
            }
        },
        None => io::read_stdin().unwrap_or_default(),
    };

    let result: Vec<u8> = if reverse {
        undump(&String::from_utf8_lossy(&data))
    } else {
        let mut s = String::new();
        for rec in data.chunks_exact(UTMP_SIZE) {
            s.push_str(&dump_record(rec));
        }
        s.into_bytes()
    };

    match output {
        Some(path) => {
            let w = io::File::open_with(&path, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, 0o666)
                .and_then(|f| sysabi::sys::write_all(f.fd(), &result));
            if let Err(e) = w {
                ul::warn(&short, format!("cannot open {}", io::lossy(&path)), e);
                return 1;
            }
        }
        None => {
            let mut out = io::stdout();
            let _ = out.write_all(&result);
        }
    }
    let _ = Errno::ENOENT;
    0
}
