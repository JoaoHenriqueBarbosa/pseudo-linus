//! `zipsplit` do Info-ZIP 3.0: parte um zip em vários, cada um com no máximo `size` bytes.

use sysabi::sys;
use sysabi::Fd;

use super::ztools::{self, ZE_BIG, ZE_NONE, ZE_PARMS};
use crate::sysutil::{self, Output};

const HELP: &str = "Copyright (c) 1990-2008 Info-ZIP - Type 'zipsplit \"-L\"' for software license.\n\
\n\
ZipSplit 3.0 (July 5th 2008)\n\
Usage:  zipsplit [-tipqs] [-n size] [-r room] [-b path] zipfile\n\
\x20 -t   report how many files it will take, but don't make them\n\
\x20 -i   make index (zipsplit.idx) and count its size against first zip file\n\
\x20 -n   make zip files no larger than \"size\" (default = 36000)\n\
\x20 -r   leave room for \"room\" bytes on the first disk (default = 0)\n\
\x20 -b   use \"path\" for the output zip files\n\
\x20 -q   quieter operation, suppress some informational messages\n\
\x20 -p   pause between output zip files\n\
\x20 -s   do a sequential split even if it takes more zip files\n\
\x20 -h   show this help    -v   show version info    -L   show software license\n";

const VERSION: &str = "Copyright (c) 1990-2008 Info-ZIP - Type 'zipsplit \"-L\"' for software license.\n\
This is ZipSplit 3.0 (July 5th 2008), by Info-ZIP.\n\
Currently maintained by E. Gordon.  Please send bug reports to\n\
the authors using the web page at www.info-zip.org; see README for details.\n\
\n\
Latest sources and executables are at ftp://ftp.info-zip.org/pub/infozip,\n\
as of above date; see http://www.info-zip.org/ for other sites.\n\
\n\
Compiled with gcc 14.2.0 for Unix (Linux ELF).\n\
\n\
ZipSplit special compilation options:\n\
\t[none]\n";

fn fail(code: i32, h: &str) -> i32 {
    sysutil::eprint(format!("zipsplit error: {} ({})\n", ztools::error_text(code), h));
    code
}

fn fail_args() -> i32 {
    fail(ZE_PARMS, "Use option -h for help.")
}

fn put_stdout(s: &[u8]) {
    let mut o = Output::stdout();
    o.write(s);
    let _ = o.finish();
}

fn parse_num(s: &[u8]) -> Option<usize> {
    let t = std::str::from_utf8(s).ok()?;
    let (digits, mult) = match t.chars().last()? {
        'k' | 'K' => (&t[..t.len() - 1], 1024),
        'm' | 'M' => (&t[..t.len() - 1], 1024 * 1024),
        _ => (t, 1),
    };
    digits.parse::<usize>().ok()?.checked_mul(mult)
}

/// Primeiro encaixe decrescente em `k` caixas; devolve a caixa de cada entrada.
fn first_fit(costs: &[usize], caps: &[usize]) -> Option<Vec<usize>> {
    let mut order: Vec<usize> = (0..costs.len()).collect();
    order.sort_by(|&a, &b| costs[b].cmp(&costs[a]));
    let mut left = caps.to_vec();
    let mut bin = vec![0; costs.len()];
    for i in order {
        let b = left.iter().position(|&l| l >= costs[i])?;
        left[b] -= costs[i];
        bin[i] = b;
    }
    Some(bin)
}

pub fn main(args: &[Vec<u8>]) -> i32 {
    let (mut test, mut index, mut quiet, mut pause, mut seq) = (false, false, false, false, false);
    let mut size: usize = 36000;
    let mut room: usize = 0;
    let mut path: Option<Vec<u8>> = None;
    let mut zipfile: Option<Vec<u8>> = None;
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if a.len() > 1 && a[0] == b'-' {
            let mut k = 1;
            while k < a.len() {
                let c = a[k];
                match c {
                    b'h' => {
                        put_stdout(HELP.as_bytes());
                        return 0;
                    }
                    b'v' => {
                        put_stdout(VERSION.as_bytes());
                        return 0;
                    }
                    b't' => test = true,
                    b'i' => index = true,
                    b'q' => quiet = true,
                    b'p' => pause = true,
                    b's' => seq = true,
                    b'n' | b'r' | b'b' => {
                        let val: Vec<u8> = if k + 1 < a.len() {
                            let v = a[k + 1..].to_vec();
                            k = a.len();
                            v
                        } else {
                            i += 1;
                            match args.get(i) {
                                Some(v) => {
                                    k = a.len();
                                    v.clone()
                                }
                                None => return fail_args(),
                            }
                        };
                        match c {
                            b'n' => match parse_num(&val) {
                                Some(v) => size = v,
                                None => return fail_args(),
                            },
                            b'r' => match parse_num(&val) {
                                Some(v) => room = v,
                                None => return fail_args(),
                            },
                            _ => path = Some(val),
                        }
                        continue;
                    }
                    _ => return fail_args(),
                }
                k += 1;
            }
        } else if zipfile.is_none() {
            zipfile = Some(a.clone());
        } else {
            return fail_args();
        }
        i += 1;
    }
    let Some(zipfile) = zipfile else {
        put_stdout(HELP.as_bytes());
        return 0;
    };
    let zf = String::from_utf8_lossy(&zipfile).into_owned();
    let data = match sysutil::read_path(&zipfile) {
        Ok(d) => d,
        Err(_) => {
            sysutil::eprint("\nzipsplit error: Interrupted (aborting)\n");
            return ztools::ZE_ABORT;
        }
    };
    let arc = match ztools::parse(&data) {
        Ok(a) => a,
        Err(c) => return fail(c, &zf),
    };
    if arc.entries.is_empty() {
        return fail(ZE_NONE, "zip file empty");
    }
    let mut costs = Vec::with_capacity(arc.entries.len());
    for e in &arc.entries {
        match ztools::entry_cost(&data, e) {
            Ok(c) => costs.push(c),
            Err(c) => return fail(c, &zf),
        }
    }
    // O índice (zipsplit.idx) conta contra o primeiro zip: uma linha por entrada e um cabeçalho por zip.
    let idx_room = if index { arc.entries.iter().map(|e| e.name.len() + 4).sum::<usize>() + 64 } else { 0 };
    let first_cap = size.saturating_sub(22 + room + idx_room);
    let other_cap = size.saturating_sub(22);
    for (e, &c) in arc.entries.iter().zip(&costs) {
        if c > other_cap.max(first_cap) {
            return fail(ZE_BIG, &String::from_utf8_lossy(&e.name));
        }
    }
    // Divisão sequencial.
    let mut seq_bin = vec![0usize; costs.len()];
    let mut nseq = 1;
    let mut used = 0;
    for (j, &c) in costs.iter().enumerate() {
        let cap = if nseq == 1 { first_cap } else { other_cap };
        if used + c > cap {
            nseq += 1;
            used = 0;
            if c > other_cap {
                return fail(ZE_BIG, &String::from_utf8_lossy(&arc.entries[j].name));
            }
        }
        used += c;
        seq_bin[j] = nseq - 1;
    }
    let mut bins = seq_bin;
    let mut nzips = nseq;
    if !seq && nseq > 1 {
        let total: usize = costs.iter().sum();
        let mut k = (total / other_cap.max(1)).max(1);
        while k < nseq {
            let caps: Vec<usize> = (0..k).map(|b| if b == 0 { first_cap } else { other_cap }).collect();
            if let Some(b) = first_fit(&costs, &caps) {
                bins = b;
                nzips = k;
                break;
            }
            k += 1;
        }
    }
    // Zips vazios (possível no encaixe) não se criam: renumera as caixas pela ordem.
    let mut present: Vec<usize> = bins.clone();
    present.sort_unstable();
    present.dedup();
    let nzips_real = present.len().min(nzips);
    let remap = |b: usize| present.iter().position(|&x| x == b).unwrap_or(0);
    let mut groups: Vec<Vec<usize>> = vec![Vec::new(); nzips_real];
    for (j, &b) in bins.iter().enumerate() {
        groups[remap(b)].push(j);
    }
    let sizes: Vec<usize> = groups.iter().map(|g| g.iter().map(|&j| costs[j]).sum::<usize>() + 22).collect();
    let sum: usize = sizes.iter().sum();
    let efficiency = (200 * sum / (size.max(1) * nzips_real) + 1) / 2;
    if !quiet || test {
        put_stdout(format!("{} zip files will be made ({}% efficiency)\n", nzips_real, efficiency).as_bytes());
    }
    if test {
        return 0;
    }

    let base: Vec<u8> = {
        let stem = if zipfile.len() >= 4 && zipfile[zipfile.len() - 4..].eq_ignore_ascii_case(b".zip") {
            &zipfile[..zipfile.len() - 4]
        } else {
            &zipfile[..]
        };
        match &path {
            Some(p) => {
                let mut out = p.clone();
                if !out.is_empty() && !out.ends_with(b"/") {
                    out.push(b'/');
                }
                out.extend_from_slice(sysutil::basename(stem));
                out
            }
            None => stem.to_vec(),
        }
    };
    let dir: Vec<u8> = match &path {
        Some(p) => {
            let mut out = p.clone();
            if !out.is_empty() && !out.ends_with(b"/") {
                out.push(b'/');
            }
            out
        }
        None => Vec::new(),
    };
    let mut names: Vec<Vec<u8>> = Vec::new();
    for (n, g) in groups.iter().enumerate() {
        let mut name = base.clone();
        name.extend_from_slice((n + 1).to_string().as_bytes());
        name.extend_from_slice(b".zip");
        let zip = match ztools::build(&data, &arc, g, if n == 0 { &arc.comment } else { &[] }) {
            Ok(z) => z,
            Err(c) => return fail(c, &zf),
        };
        if sysutil::write_file(&name, &zip, 0o666).is_err() {
            return fail(ztools::ZE_WRITE, &String::from_utf8_lossy(&name));
        }
        if !quiet {
            put_stdout(format!("creating: {}\n", String::from_utf8_lossy(&name)).as_bytes());
        }
        names.push(name);
        if pause && n + 1 < groups.len() {
            let mut b = [0u8; 1];
            while let Ok(1) = sys::read(Fd::STDIN, &mut b) {
                if b[0] == b'\n' {
                    break;
                }
            }
        }
    }
    if index {
        let mut idx = format!("Index for {}:\n", zf).into_bytes();
        for (g, name) in groups.iter().zip(&names) {
            idx.extend_from_slice(format!("\n{}:\n", String::from_utf8_lossy(name)).as_bytes());
            for &j in g {
                idx.extend_from_slice(b"  ");
                idx.extend_from_slice(&arc.entries[j].name);
                idx.push(b'\n');
            }
        }
        let mut iname = dir;
        iname.extend_from_slice(b"zipsplit.idx");
        if sysutil::write_file(&iname, &idx, 0o666).is_err() {
            return fail(ztools::ZE_WRITE, "zipsplit.idx");
        }
        if !quiet {
            put_stdout(b"creating: zipsplit.idx\n");
        }
    }
    0
}
