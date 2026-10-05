//! `toe` do ncurses 6.5.20250216 (`toe.c`): a tabela das entradas do banco terminfo.
//!
//! As opções `-u` e `-U` leem as dependências (`use=`) de um arquivo-fonte terminfo; aqui a leitura
//! é um analisador mínimo do formato (nomes e `use=`), sem os avisos do compilador `tic`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, FileType, sys};

use super::terminfo::{TermType, Str, db_dirs, name_match, read_file_entry};
use super::{VERSION, first_name, rootname};
use crate::util::io;
use crate::util::{Getopt, GetoptError};

struct TermData {
    db_index: usize,
    checksum: u64,
    term_name: Vec<u8>,
    description: Vec<u8>,
}

fn is_dotname(name: &[u8]) -> bool {
    name == b"." || name == b".."
}

fn term_description(names: &[u8]) -> Vec<u8> {
    match names.iter().rposition(|b| *b == b'|') {
        Some(p) if p + 1 < names.len() => names[p + 1..].to_vec(),
        _ => b"(No description)".to_vec(),
    }
}

fn string_sum(value: &Str) -> u64 {
    match value {
        Str::Cancelled => !0u64,
        Str::Absent => 0,
        Str::Val(v) => v.iter().fold(0u64, |a, b| a.wrapping_add(u64::from(*b))),
    }
}

fn checksum_of(tp: &TermType) -> u64 {
    let mut result = tp.names.iter().fold(0u64, |a, b| a.wrapping_add(u64::from(*b)));
    for b in &tp.bools {
        result = result.wrapping_add(i64::from(*b) as u64);
    }
    for n in &tp.nums {
        result = result.wrapping_add(i64::from(*n) as u64);
    }
    for s in &tp.strs {
        result = result.wrapping_add(string_sum(s));
    }
    result
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Hook {
    Desc,
    Sort,
}

/// Aplica o gancho a cada entrada dos diretórios terminfo dados (`typelist`).
fn typelist(dirs: &[Vec<u8>], verbosity: bool, hook: Hook, progname: &str) -> i32 {
    let mut o = io::stdout();
    let mut collected: Vec<TermData> = Vec::new();
    for (i, dir) in dirs.iter().enumerate() {
        let is_dir = sys::stat(dir).is_ok_and(|s| s.file_type() == FileType::Directory);
        if !is_dir {
            continue;
        }
        let entries = match sys::read_dir(dir) {
            Ok(e) => e,
            Err(_) => {
                let _ = io::flush_stdout();
                io::eprint(format!("{progname}: can't open terminfo directory {}\n", io::lossy(dir)));
                continue;
            }
        };
        if verbosity {
            let _ = write!(o, "#\n#{}:\n#\n", io::lossy(dir));
        }
        for sub in entries {
            if is_dotname(&sub.name) {
                continue;
            }
            let mut cwd = dir.clone();
            cwd.push(b'/');
            cwd.extend_from_slice(&sub.name);
            cwd.push(b'/');
            let Some(s) = sys::try_current() else { continue };
            if s.chdir(&cwd).is_err() {
                continue;
            }
            let inner = match sys::read_dir(b".") {
                Ok(e) => e,
                Err(e) => {
                    io::eprint(format!("{}: {}\n", io::lossy(&cwd), e.message()));
                    continue;
                }
            };
            for entry in inner {
                let name2 = entry.name;
                if is_dotname(&name2) || !sys::stat(&name2).is_ok_and(|s| s.file_type() == FileType::Regular) {
                    continue;
                }
                let Some(lterm) = read_file_entry(&name2, true) else {
                    let _ = io::flush_stdout();
                    io::eprint(format!("{progname}: couldn't open terminfo file {}.\n", io::lossy(&name2)));
                    continue;
                };
                // Só visita pelo nome primário.
                let cn = first_name(&lterm.names);
                if cn == name2.as_slice() {
                    match hook {
                        Hook::Desc => {
                            let line = format!("{:<10}\t", io::lossy(cn));
                            let _ = o.write_all(line.as_bytes());
                            let _ = o.write_all(&term_description(&lterm.names));
                            let _ = o.write_all(b"\n");
                        }
                        Hook::Sort => collected.push(TermData {
                            db_index: i,
                            checksum: if dirs.len() > 1 { checksum_of(&lterm) } else { 0 },
                            term_name: cn.to_vec(),
                            description: term_description(&lterm.names),
                        }),
                    }
                }
            }
        }
    }
    if hook == Hook::Sort {
        show_termdata_full(&mut collected, dirs);
    }
    0
}

/// `show_termdata` com a descrição (`ptr_termdata[nk].description`).
fn show_termdata_full(data: &mut Vec<TermData>, dirs: &[Vec<u8>]) {
    let mut o = io::stdout();
    if data.is_empty() {
        return;
    }
    let eargc = dirs.len();
    if eargc > 1 {
        for (j, d) in dirs.iter().enumerate() {
            for _ in 0..=j {
                let _ = o.write_all(b"--");
            }
            let _ = o.write_all(b"> ");
            let _ = o.write_all(d);
            let _ = o.write_all(b"\n");
        }
    }
    if data.len() > 1 {
        data.sort_by(|a, b| a.term_name.cmp(&b.term_name).then(a.db_index.cmp(&b.db_index)));
    }
    let mut n = 0usize;
    while n < data.len() {
        let mut nk: i64 = -1;
        if eargc > 1 {
            let mut check: u64 = 0;
            let mut k = 0usize;
            loop {
                let mark = if check == 0 || check != data[n].checksum { b'*' } else { b'+' };
                while k < data[n].db_index {
                    let _ = o.write_all(b"--");
                    k += 1;
                }
                let _ = o.write_all(&[mark, b'-']);
                check = data[n].checksum;
                if mark == b'*' && nk < 0 {
                    nk = n as i64;
                }
                k += 1;
                if n + 1 >= data.len() || data[n].term_name != data[n + 1].term_name {
                    break;
                }
                n += 1;
            }
            while k < eargc {
                let _ = o.write_all(b"--");
                k += 1;
            }
            let _ = o.write_all(b":\t");
        }
        if nk < 0 {
            nk = n as i64;
        }
        let name = &data[n].term_name;
        let _ = o.write_all(name);
        for _ in name.len()..10 {
            let _ = o.write_all(b" ");
        }
        let _ = o.write_all(b"\t");
        let _ = o.write_all(&data[nk as usize].description);
        let _ = o.write_all(b"\n");
        n += 1;
    }
}

/// Uma entrada do arquivo-fonte: os nomes e os `use=`.
struct SrcEntry {
    names: Vec<u8>,
    uses: Vec<Vec<u8>>,
}

/// Leitura mínima de um arquivo-fonte terminfo.
fn read_source(data: &[u8]) -> Vec<SrcEntry> {
    let mut entries: Vec<SrcEntry> = Vec::new();
    let mut body: Vec<u8> = Vec::new();
    let mut have = false;
    let flush = |entries: &mut Vec<SrcEntry>, body: &mut Vec<u8>, have: &mut bool| {
        if *have {
            // nomes até a primeira vírgula; depois os campos
            let mut fields: Vec<Vec<u8>> = Vec::new();
            let mut cur: Vec<u8> = Vec::new();
            let mut i = 0;
            while i < body.len() {
                if body[i] == b'\\' && i + 1 < body.len() {
                    cur.push(body[i]);
                    cur.push(body[i + 1]);
                    i += 2;
                    continue;
                }
                if body[i] == b',' {
                    fields.push(std::mem::take(&mut cur));
                } else {
                    cur.push(body[i]);
                }
                i += 1;
            }
            if !fields.is_empty() {
                let names = fields[0].clone();
                let mut uses = Vec::new();
                for f in &fields[1..] {
                    let t: Vec<u8> = f.iter().copied().skip_while(|b| super::c_isspace(*b)).collect();
                    if let Some(u) = t.strip_prefix(b"use=") {
                        uses.push(u.to_vec());
                    }
                }
                entries.push(SrcEntry { names, uses });
            }
        }
        body.clear();
        *have = false;
    };
    for line in data.split(|b| *b == b'\n') {
        if line.first() == Some(&b'#') {
            continue;
        }
        if line.is_empty() || super::c_isspace(line[0]) {
            if have {
                body.push(b' ');
                body.extend_from_slice(line);
            }
        } else {
            flush(&mut entries, &mut body, &mut have);
            have = true;
            body.extend_from_slice(line);
        }
    }
    flush(&mut entries, &mut body, &mut have);
    entries
}

fn usage(progname: &str) -> ! {
    io::eprint(format!("usage: {progname} [-ahsuUV] [-v n] [file...]\n"));
    sys::exit(1)
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let progname = io::lossy(rootname(&argv[0]));
    let mut all_dirs = false;
    let mut direct = false;
    let mut invert = false;
    let mut header = false;
    let mut report_file: Option<Vec<u8>> = None;
    let mut hook = Hook::Desc;
    let mut g = Getopt::from_env(&argv[1..], "0123456789ahsu:vU:V", &[]);
    while let Some(r) = g.next_opt() {
        match r {
            Ok(o) => match o.short().unwrap_or('?') {
                c if c.is_ascii_digit() => {}
                'a' => all_dirs = true,
                'h' => header = true,
                's' => hook = Hook::Sort,
                'u' => {
                    direct = true;
                    report_file = o.arg.clone();
                }
                'v' => {}
                'U' => {
                    invert = true;
                    report_file = o.arg.clone();
                }
                'V' => {
                    let _ = writeln!(io::stdout(), "{VERSION}");
                    return 0;
                }
                _ => usage(&progname),
            },
            Err(e) => bad_option(&e, &argv0, &progname),
        }
    }
    let operands = g.operands();
    let mut source: Vec<SrcEntry> = Vec::new();
    if let Some(rf) = &report_file {
        match sys::read_file(rf) {
            Ok(d) => source = read_source(&d),
            Err(_) => {
                let _ = io::flush_stdout();
                io::eprint(format!("{progname}: can't open {}\n", io::lossy(rf)));
                return 1;
            }
        }
    }
    if direct {
        let mut o = io::stdout();
        for qp in &source {
            if !qp.uses.is_empty() {
                let _ = o.write_all(first_name(&qp.names));
                let _ = o.write_all(b":");
                for u in &qp.uses {
                    let _ = o.write_all(b" ");
                    let _ = o.write_all(u);
                }
                let _ = o.write_all(b"\n");
            }
        }
        return 0;
    }
    if invert {
        let mut o = io::stdout();
        for qp in &source {
            let mut matchcount = 0;
            for rp in &source {
                if rp.uses.is_empty() {
                    continue;
                }
                for u in &rp.uses {
                    if name_match(&qp.names, u) {
                        if matchcount == 0 {
                            let _ = o.write_all(first_name(&qp.names));
                            let _ = o.write_all(b":");
                        }
                        matchcount += 1;
                        let _ = o.write_all(b" ");
                        let _ = o.write_all(first_name(&rp.names));
                    }
                }
            }
            if matchcount > 0 {
                let _ = o.write_all(b"\n");
            }
        }
        return 0;
    }
    if !operands.is_empty() {
        return typelist(&operands, header, hook, &progname);
    }
    let dirs: Vec<Vec<u8>> = db_dirs(None).into_iter().filter(|d| !(d.starts_with(b"b64:") || d.starts_with(b"hex:"))).collect();
    if all_dirs {
        typelist(&dirs, header, hook, &progname)
    } else {
        let first: Vec<Vec<u8>> = dirs.into_iter().take(1).collect();
        typelist(&first, header, hook, &progname)
    }
}

fn bad_option(e: &GetoptError, argv0: &str, progname: &str) -> ! {
    io::eprint(format!("{}\n", e.message(argv0)));
    usage(progname)
}
