//! `dpkg-split` do dpkg 1.22 (Debian 13): parte um `.deb` em pedaços (`--split`), junta os pedaços
//! (`--join`), mostra o cabeçalho de um pedaço (`--info`) e acumula pedaços num depósito
//! (`--auto`, `--listq`, `--discard`).
//!
//! Um pedaço é um arquivo `ar` com o membro `debian-split` (versão `2.1`, pacote, versão, MD5 do
//! arquivo inteiro, tamanho total, tamanho máximo do pedaço, `n/m` e arquitetura) seguido do membro
//! `data.N` com os bytes.
//!
//! Divergências conhecidas: `--msdos` é aceito e ignorado, e a saída do `--listq` é simplificada.

use std::ffi::OsString;

use sysabi::{AtFlags, Clock, Ctx, Fd, sys};

use crate::dpkg_deb::{self, ar_member, control_text, die, field, lossy, parse_control, read_deb};
use crate::sysutil::{self, Output};

const PROG: &str = "dpkg-split";

const USAGE: &str = "Usage: dpkg-split [<option>...] <command>

Commands:
  -s|--split <file> [<prefix>]     Split an archive.
  -j|--join <part>...              Join parts together.
  -I|--info <part>...              Display info about a part.
  -a|--auto -o <complete> <part>   Auto-accumulate parts.
  -l|--listq                       List unmatched pieces.
  -d|--discard [<filename>...]     Discard unmatched pieces.

  -?, --help                       Show this help message.
      --version                    Show the version.

Options:
  --depotdir <directory>           Use <directory> instead of /var/lib/dpkg/parts.
  -S|--partsize <size>             In KiB, for -s (default is 450).
  -o|--output <file>               Filename, for -j (default is
                                     <package>_<version>_<arch>.deb).
  --msdos                          Generate 8.3 filenames.

Exit status:
  0 = ok
  1 = with --auto, file is not a part
  2 = trouble
";

const HEADER_ALLOWANCE: i64 = 1024;

fn version_text() -> String {
    format!(
        "Debian {PROG} version 1.22.22 (amd64).\n\
This is free software; see the GNU General Public License version 2 or\n\
later for copying conditions. There is NO warranty.\n"
    )
}

fn badusage(out: &mut Output, msg: &str) -> i32 {
    out.flush();
    sysutil::eprint(format!(
        "{PROG}: error: {msg}\n\nUse '{PROG} --help' for program usage information.\n"
    ));
    2
}

// ---------------------------------------------------------------------------------------------
// MD5
// ---------------------------------------------------------------------------------------------

fn md5_hex(data: &[u8]) -> String {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
        5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15,
        21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: [u32; 64] = std::array::from_fn(|i| (((i + 1) as f64).sin().abs() * 4_294_967_296.0) as u32);
    let (mut a0, mut b0, mut c0, mut d0) = (0x6745_2301u32, 0xefcd_ab89u32, 0x98ba_dcfeu32, 0x1032_5476u32);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_le_bytes());
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (mut f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut s = String::new();
    for v in [a0, b0, c0, d0] {
        for b in v.to_le_bytes() {
            s.push_str(&format!("{b:02x}"));
        }
    }
    s
}

// ---------------------------------------------------------------------------------------------
// pedaços
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Part {
    version: String,
    package: String,
    pkg_version: String,
    md5: String,
    total: u64,
    maxpart: u64,
    num: u32,
    count: u32,
    arch: String,
    data: Vec<u8>,
    file_size: u64,
}

/// Lê um pedaço. `Ok(None)` quando o arquivo não é um pedaço.
fn read_part(path: &[u8]) -> Result<Option<Part>, String> {
    let bytes = sysutil::read_path(path).map_err(|e| format!("cannot open '{}': {}", lossy(path), e.message()))?;
    if bytes.len() < 8 || &bytes[..8] != b"!<arch>\n" {
        return Ok(None);
    }
    // Primeiro membro: debian-split.
    let mut pos = 8usize;
    let mut members: Vec<(String, Vec<u8>)> = Vec::new();
    while pos + 60 <= bytes.len() && members.len() < 2 {
        let h = &bytes[pos..pos + 60];
        let size: usize = lossy(&h[48..58]).trim().parse().map_err(|_| "bad archive header".to_string())?;
        let start = pos + 60;
        let end = start + size;
        if end > bytes.len() {
            return Err("truncated archive".to_string());
        }
        let mut name = lossy(&h[..16]).trim_end().to_string();
        if name.len() > 1 && name.ends_with('/') {
            name.pop();
        }
        members.push((name, bytes[start..end].to_vec()));
        pos = end + (size & 1);
    }
    if members.len() < 2 || members[0].0 != "debian-split" {
        return Ok(None);
    }
    let text = lossy(&members[0].1);
    let l: Vec<&str> = text.lines().collect();
    if l.len() < 7 {
        return Err("part archive header is corrupt".to_string());
    }
    let (n, c) = l[6].split_once('/').ok_or_else(|| "bad part number".to_string())?;
    let num: u32 = n.parse().map_err(|_| "bad part number".to_string())?;
    let count: u32 = c.parse().map_err(|_| "bad part count".to_string())?;
    Ok(Some(Part {
        version: l[0].to_string(),
        package: l[1].to_string(),
        pkg_version: l[2].to_string(),
        md5: l[3].to_string(),
        total: l[4].parse().map_err(|_| "bad total size".to_string())?,
        maxpart: l[5].parse().map_err(|_| "bad part size".to_string())?,
        num,
        count,
        arch: l.get(7).map(|s| s.to_string()).unwrap_or_default(),
        data: members[1].1.clone(),
        file_size: bytes.len() as u64,
    }))
}

fn now(sde_ok: bool) -> i64 {
    if sde_ok {
        if let Some(v) = sysutil::getenv("SOURCE_DATE_EPOCH").and_then(|v| lossy(&v).trim().parse().ok()) {
            return v;
        }
    }
    sys::current().clock_gettime(Clock::Realtime).map(|t| t.sec).unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn part_bytes(md5: &str, total: u64, maxpart: u64, num: u32, count: u32, pkg: &str, ver: &str, arch: &str, data: &[u8], date: i64) -> Vec<u8> {
    let header = format!("2.1\n{pkg}\n{ver}\n{md5}\n{total}\n{maxpart}\n{num}/{count}\n{arch}\n");
    let mut v = b"!<arch>\n".to_vec();
    v.extend(ar_member("debian-split", date, header.as_bytes()));
    v.extend(ar_member(&format!("data.{num}"), date, data));
    v
}

fn strip_epoch(v: &str) -> &str {
    match v.split_once(':') {
        Some((e, r)) if !e.is_empty() && e.bytes().all(|c| c.is_ascii_digit()) => r,
        _ => v,
    }
}

// ---------------------------------------------------------------------------------------------
// opções
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Opts {
    action: Option<&'static str>,
    output: Option<String>,
    partsize: Option<String>,
    depot: Option<String>,
    npquiet: bool,
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    dpkg_deb::set_prog(PROG);
    let argv = sysutil::args_bytes(args);
    let mut out = Output::stdout();
    let code = run(&mut out, &argv);
    let _ = out.finish();
    code
}

fn run(out: &mut Output, argv: &[Vec<u8>]) -> i32 {
    let args: Vec<String> = argv.iter().skip(1).map(|b| lossy(b)).collect();
    let mut o = Opts::default();
    let mut i = 0usize;
    while i < args.len() {
        let a = args[i].clone();
        if a == "--" {
            i += 1;
            break;
        }
        if !a.starts_with('-') || a == "-" {
            break;
        }
        i += 1;
        // (nome longo, valor inline)
        let (name, inline): (String, Option<String>) = if let Some(l) = a.strip_prefix("--") {
            match l.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (l.to_string(), None),
            }
        } else {
            let c = a.chars().nth(1).unwrap_or('-');
            let rest: String = a.chars().skip(2).collect();
            let long = match c {
                's' => "split",
                'j' => "join",
                'I' => "info",
                'a' => "auto",
                'l' => "listq",
                'd' => "discard",
                'o' => "output",
                'S' => "partsize",
                'Q' => "npquiet",
                '?' => "help",
                _ => return badusage(out, &format!("unknown option -{c}")),
            };
            let takes = matches!(long, "output" | "partsize");
            if !rest.is_empty() && !takes {
                return badusage(out, &format!("unknown option -{c}"));
            }
            (long.to_string(), if rest.is_empty() { None } else { Some(rest) })
        };
        let action_name: Option<&'static str> = match name.as_str() {
            "split" => Some("split"),
            "join" => Some("join"),
            "info" => Some("info"),
            "auto" => Some("auto"),
            "listq" => Some("listq"),
            "discard" => Some("discard"),
            _ => None,
        };
        if let Some(act) = action_name {
            if inline.is_some() {
                return badusage(out, &format!("option --{name} doesn't take a value"));
            }
            if let Some(p) = o.action {
                return badusage(out, &format!("conflicting actions -{} (--{act}) and -{} (--{p})", act_short(act), act_short(p)));
            }
            o.action = Some(act);
            continue;
        }
        match name.as_str() {
            "help" => {
                out.write_str(USAGE);
                return 0;
            }
            "version" => {
                out.write_str(&version_text());
                return 0;
            }
            "msdos" => {}
            "npquiet" => o.npquiet = true,
            "output" | "partsize" | "depotdir" => {
                let v = match inline {
                    Some(v) => v,
                    None => {
                        if i < args.len() {
                            i += 1;
                            args[i - 1].clone()
                        } else {
                            return badusage(out, &format!("--{name} option takes a value"));
                        }
                    }
                };
                match name.as_str() {
                    "output" => o.output = Some(v),
                    "partsize" => o.partsize = Some(v),
                    _ => o.depot = Some(v),
                }
            }
            _ => return badusage(out, &format!("unknown option --{name}")),
        }
    }
    let rest: Vec<String> = args[i..].to_vec();
    let Some(action) = o.action else {
        return badusage(out, "need an action option");
    };
    match action {
        "split" => do_split(out, &o, &rest),
        "join" => do_join(out, &o, &rest),
        "info" => do_info(out, &rest),
        "auto" => do_auto(out, &o, &rest),
        "listq" => do_listq(out, &o, &rest),
        _ => do_discard(out, &o, &rest),
    }
}

fn act_short(a: &str) -> char {
    match a {
        "split" => 's',
        "join" => 'j',
        "info" => 'I',
        "auto" => 'a',
        "listq" => 'l',
        _ => 'd',
    }
}

// ---------------------------------------------------------------------------------------------
// ações
// ---------------------------------------------------------------------------------------------

fn do_split(out: &mut Output, o: &Opts, rest: &[String]) -> i32 {
    let Some(file) = rest.first() else {
        return badusage(out, "--split needs a source filename argument");
    };
    if rest.len() > 2 {
        return badusage(out, "--split takes at most a source filename and destination prefix");
    }
    let kib: i64 = match &o.partsize {
        None => 450,
        Some(v) => match v.parse::<i64>() {
            Ok(n) if n > 0 && n < 1 << 40 => n,
            _ => return badusage(out, "part size is far too large or is not positive"),
        },
    };
    let maxpart = kib * 1024 - HEADER_ALLOWANCE;
    if maxpart <= 0 {
        return badusage(out, "part size is far too large or is not positive");
    }
    let maxpart = maxpart as usize;
    // Pacote, versão e arquitetura do .deb.
    let deb = match read_deb(out, file.as_bytes()) {
        Ok(d) => d,
        Err(c) => return c,
    };
    let (text, _) = match control_text(out, &deb, file) {
        Ok(r) => r,
        Err(c) => return c,
    };
    let fields = match parse_control(&text) {
        Ok(f) => f,
        Err((n, m)) => return die(out, &format!("parsing file '{file}' near line {n}:\n {m}")),
    };
    let get = |n: &str| field(&fields, n).map(|f| f.value.clone()).unwrap_or_default();
    let (pkg, ver, arch) = (get("Package"), get("Version"), get("Architecture"));
    let data = match sysutil::read_path(file.as_bytes()) {
        Ok(d) => d,
        Err(e) => return dpkg_deb::die_errno(out, &format!("cannot open '{file}'"), e),
    };
    let md5 = md5_hex(&data);
    let total = data.len();
    let nparts = total.div_ceil(maxpart).max(1);
    let prefix = match rest.get(1) {
        Some(p) => p.clone(),
        None => {
            let b = lossy(sysutil::basename(file.as_bytes()));
            b.strip_suffix(".deb").map(str::to_string).unwrap_or(b)
        }
    };
    out.write_str(&format!(
        "Splitting package {pkg} into {nparts} part{}: ",
        if nparts == 1 { "" } else { "s" }
    ));
    let date = now(true);
    for p in 1..=nparts {
        let from = (p - 1) * maxpart;
        let to = (from + maxpart).min(total);
        let bytes = part_bytes(&md5, total as u64, maxpart as u64, p as u32, nparts as u32, &pkg, &ver, &arch, &data[from..to], date);
        let name = format!("{prefix}.{p}of{nparts}.deb");
        if let Err(e) = sysutil::write_file(name.as_bytes(), &bytes, 0o666) {
            return dpkg_deb::die_errno(out, &format!("unable to create '{name}'"), e);
        }
        out.write_str(&format!("{p} "));
    }
    out.write_str("done\n");
    0
}

/// Junta os pedaços e devolve o conteúdo e o nome padrão de saída.
fn assemble(out: &mut Output, files: &[String]) -> Result<(Vec<u8>, String), i32> {
    let mut parts: Vec<(String, Part)> = Vec::new();
    for f in files {
        match read_part(f.as_bytes()) {
            Ok(Some(p)) => parts.push((f.clone(), p)),
            Ok(None) => return Err(die(out, &format!("file '{f}' is not part of a multipart archive"))),
            Err(m) => return Err(die(out, &format!("file '{f}' is corrupt - {m}"))),
        }
    }
    let Some((first_name, first)) = parts.first().cloned() else {
        return Err(2);
    };
    let mut slots: Vec<Option<&(String, Part)>> = vec![None; first.count as usize];
    for entry in &parts {
        let (name, p) = entry;
        if p.md5 != first.md5 || p.total != first.total || p.count != first.count || p.maxpart != first.maxpart {
            return Err(die(out, &format!("files '{first_name}' and '{name}' are not parts of the same file")));
        }
        if p.num == 0 || p.num > p.count {
            return Err(die(out, &format!("file '{name}' is corrupt - bad part number")));
        }
        let idx = (p.num - 1) as usize;
        if let Some(prev) = slots[idx] {
            return Err(die(
                out,
                &format!("there are several versions of part {} - at least '{}' and '{name}'", p.num, prev.0),
            ));
        }
        slots[idx] = Some(entry);
    }
    let mut data: Vec<u8> = Vec::new();
    for (i, s) in slots.iter().enumerate() {
        match s {
            Some((_, p)) => data.extend_from_slice(&p.data),
            None => return Err(die(out, &format!("part {} is missing", i + 1))),
        }
    }
    if data.len() as u64 != first.total {
        return Err(die(out, "total size of the parts does not match the original file size"));
    }
    let name = format!("{}_{}_{}.deb", first.package, strip_epoch(&first.pkg_version), first.arch);
    Ok((data, name))
}

fn do_join(out: &mut Output, o: &Opts, rest: &[String]) -> i32 {
    if rest.is_empty() {
        return badusage(out, "--join needs at least one part file argument");
    }
    let (data, default) = match assemble(out, rest) {
        Ok(r) => r,
        Err(c) => return c,
    };
    let name = o.output.clone().unwrap_or(default);
    match sysutil::write_file(name.as_bytes(), &data, 0o666) {
        Ok(()) => 0,
        Err(e) => dpkg_deb::die_errno(out, &format!("unable to create '{name}'"), e),
    }
}

fn do_info(out: &mut Output, rest: &[String]) -> i32 {
    let mut status = 0;
    for f in rest {
        match read_part(f.as_bytes()) {
            Ok(Some(p)) => {
                let offset = u64::from(p.num.saturating_sub(1)) * p.maxpart;
                out.write_str(&format!(
                    "{f}:\n    Part format version:            {}\n    Part of package:                {}\n        ... version:                {}\n        ... architecture:           {}\n        ... MD5 checksum:           {}\n        ... length:                 {} bytes\n        ... split every:            {} bytes\n    Part number:                    {}/{}\n    Part length:                    {} bytes\n    Part offset:                    {} bytes\n    Part file size (used portion):  {} bytes\n\n",
                    p.version, p.package, p.pkg_version, p.arch, p.md5, p.total, p.maxpart, p.num, p.count,
                    p.data.len(), offset, p.file_size
                ));
            }
            Ok(None) => {
                out.write_str(&format!("File '{f}' is not part of a multipart archive.\n"));
                status = 1;
            }
            Err(m) => return die(out, &format!("file '{f}' is corrupt - {m}")),
        }
    }
    status
}

fn depot(o: &Opts) -> String {
    let d = o.depot.clone().unwrap_or_else(|| "/var/lib/dpkg/parts".to_string());
    d.trim_end_matches('/').to_string()
}

/// `(md5, maxpart, num, count)` do nome `md5.maxpart.num.count` do depósito.
fn depot_entries(dir: &str) -> Vec<(String, String, u32, u32)> {
    let mut v = Vec::new();
    if let Ok(es) = sys::read_dir(dir.as_bytes()) {
        for e in es {
            let n = lossy(&e.name);
            let p: Vec<&str> = n.split('.').collect();
            if p.len() == 4 {
                if let (Ok(num), Ok(count)) = (p[2].parse::<u32>(), p[3].parse::<u32>()) {
                    v.push((p[0].to_string(), p[1].to_string(), num, count));
                }
            }
        }
    }
    v.sort();
    v
}

fn ranges(missing: &[u32]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < missing.len() {
        let mut j = i;
        while j + 1 < missing.len() && missing[j + 1] == missing[j] + 1 {
            j += 1;
        }
        if j == i {
            out.push(missing[i].to_string());
        } else {
            out.push(format!("{}-{}", missing[i], missing[j]));
        }
        i = j + 1;
    }
    out.join(",")
}

fn do_auto(out: &mut Output, o: &Opts, rest: &[String]) -> i32 {
    let Some(target) = &o.output else {
        return badusage(out, "--auto requires the use of the --output option");
    };
    let [file] = rest else {
        return badusage(out, "--auto requires exactly one part file argument");
    };
    let part = match read_part(file.as_bytes()) {
        Ok(Some(p)) => p,
        Ok(None) => {
            if !o.npquiet {
                out.write_str(&format!("File '{file}' is not part of a multipart archive.\n"));
            }
            return 1;
        }
        Err(m) => return die(out, &format!("file '{file}' is corrupt - {m}")),
    };
    let dir = depot(o);
    let stored = format!("{dir}/{}.{}.{}.{}", part.md5, part.maxpart, part.num, part.count);
    let bytes = match sysutil::read_path(file.as_bytes()) {
        Ok(b) => b,
        Err(e) => return dpkg_deb::die_errno(out, &format!("cannot open '{file}'"), e),
    };
    if let Err(e) = sysutil::write_file(stored.as_bytes(), &bytes, 0o644) {
        return dpkg_deb::die_errno(out, &format!("unable to create '{stored}'"), e);
    }
    let have: Vec<u32> = depot_entries(&dir)
        .into_iter()
        .filter(|(m, s, _, c)| *m == part.md5 && *s == part.maxpart.to_string() && *c == part.count)
        .map(|(_, _, n, _)| n)
        .collect();
    let missing: Vec<u32> = (1..=part.count).filter(|n| !have.contains(n)).collect();
    if !missing.is_empty() {
        out.write_str(&format!(
            "Part {} of package {} filed (still want {})\n",
            part.num,
            part.package,
            ranges(&missing)
        ));
        return 0;
    }
    let names: Vec<String> = (1..=part.count)
        .map(|n| format!("{dir}/{}.{}.{}.{}", part.md5, part.maxpart, n, part.count))
        .collect();
    out.write_str(&format!(
        "Putting package {} together from {} part{}: ",
        part.package,
        part.count,
        if part.count == 1 { "" } else { "s" }
    ));
    let (data, _) = match assemble(out, &names) {
        Ok(r) => r,
        Err(c) => return c,
    };
    if let Err(e) = sysutil::write_file(target.as_bytes(), &data, 0o666) {
        return dpkg_deb::die_errno(out, &format!("unable to create '{target}'"), e);
    }
    for n in 1..=part.count {
        out.write_str(&format!("{n} "));
    }
    out.write_str("done\n");
    for n in &names {
        let _ = sys::current().unlinkat(Fd::CWD, n.as_bytes(), AtFlags::empty());
    }
    0
}

fn do_listq(out: &mut Output, o: &Opts, rest: &[String]) -> i32 {
    if !rest.is_empty() {
        return badusage(out, "--listq takes no arguments");
    }
    let dir = depot(o);
    let entries = depot_entries(&dir);
    let mut seen: Vec<(String, String, u32)> = Vec::new();
    for (m, s, _, c) in &entries {
        let key = (m.clone(), s.clone(), *c);
        if !seen.contains(&key) {
            seen.push(key);
        }
    }
    for (m, s, c) in seen {
        let have = entries.iter().filter(|(m2, s2, _, c2)| *m2 == m && *s2 == s && *c2 == c).count();
        out.write_str(&format!("{m} {have}/{c}\n"));
    }
    0
}

fn do_discard(out: &mut Output, o: &Opts, rest: &[String]) -> i32 {
    let dir = depot(o);
    let sc = sys::current();
    let names: Vec<String> = if rest.is_empty() {
        match sys::read_dir(dir.as_bytes()) {
            Ok(es) => {
                let mut v: Vec<String> = es.into_iter().map(|e| lossy(&e.name)).collect();
                v.sort();
                v
            }
            Err(_) => Vec::new(),
        }
    } else {
        rest.to_vec()
    };
    for n in names {
        let path = if n.contains('/') { n.clone() } else { format!("{dir}/{n}") };
        if sc.unlinkat(Fd::CWD, path.as_bytes(), AtFlags::empty()).is_ok() {
            out.write_str(&format!("Deleted {path}.\n"));
        }
    }
    let _ = out.flush();
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_known_values() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn ranges_format() {
        assert_eq!(ranges(&[1, 2, 3, 5]), "1-3,5");
    }

    #[test]
    fn part_roundtrip_header() {
        let b = part_bytes("m", 10, 4, 2, 3, "p", "1", "all", b"abcd", 0);
        assert!(b.starts_with(b"!<arch>\ndebian-split"));
    }
}
