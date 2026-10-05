//! `zipnote` do Info-ZIP 3.0: mostra e troca os comentários (e os nomes) das entradas de um zip.

use sysabi::sys;
use sysabi::Fd;

use super::ztools::{self, Archive, ZE_FORM, ZE_PARMS};
use crate::sysutil::{self, Output};

const HELP: &str = "Copyright (c) 1990-2008 Info-ZIP - Type 'zipnote \"-L\"' for software license.\n\
\n\
ZipNote 3.0 (July 5th 2008)\n\
Usage:  zipnote [-w] [-q] [-b path] zipfile\n\
\x20 the default action is to write the comments in zipfile to stdout\n\
\x20 -w   write the zipfile comments from stdin\n\
\x20 -b   use \"path\" for the temporary zip file\n\
\x20 -q   quieter operation, suppress some informational messages\n\
\x20 -h   show this help    -v   show version info    -L   show software license\n\
\n\
Example:\n\
\x20    zipnote foo.zip > foo.tmp\n\
\x20    ed foo.tmp\n\
\x20    ... then you edit the comments, save, and exit ...\n\
\x20    zipnote -w foo.zip < foo.tmp\n\
\n\
\x20 \"@ name\" can be followed by an \"@=newname\" line to change the name\n";

const VERSION: &str = "Copyright (c) 1990-2008 Info-ZIP - Type 'zipnote \"-L\"' for software license.\n\
This is ZipNote 3.0 (July 5th 2008), by Info-ZIP.\n\
Currently maintained by E. Gordon.  Please send bug reports to\n\
the authors using the web page at www.info-zip.org; see README for details.\n\
\n\
Latest sources and executables are at ftp://ftp.info-zip.org/pub/infozip,\n\
as of above date; see http://www.info-zip.org/ for other sites.\n\
\n\
Compiled with gcc 14.2.0 for Unix (Linux ELF).\n\
\n\
ZipNote special compilation options:\n\
\t[none]\n";

fn fail(code: i32, h: &str) -> i32 {
    sysutil::eprint(format!("zipnote error: {} ({})\n", ztools::error_text(code), h));
    code
}

fn fail_args() -> i32 {
    fail(ZE_PARMS, "Use option -h for help.")
}

fn put_stdout(s: &str) {
    let mut o = Output::stdout();
    o.write_str(s);
    let _ = o.finish();
}

pub fn main(args: &[Vec<u8>]) -> i32 {
    let mut write = false;
    let mut zipfile: Option<Vec<u8>> = None;
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if a.len() > 1 && a[0] == b'-' {
            let mut k = 1;
            while k < a.len() {
                match a[k] {
                    b'h' => {
                        put_stdout(HELP);
                        return 0;
                    }
                    b'v' => {
                        put_stdout(VERSION);
                        return 0;
                    }
                    b'w' => write = true,
                    b'q' => {}
                    b'b' => {
                        // O diretório do arquivo temporário: a escrita aqui é direta, então só se consome.
                        if k + 1 < a.len() {
                            k = a.len();
                            continue;
                        }
                        i += 1;
                        if i >= args.len() {
                            return fail_args();
                        }
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
        put_stdout(HELP);
        return 0;
    };
    let data = match sysutil::read_path(&zipfile) {
        Ok(d) => d,
        Err(_) => {
            sysutil::eprint("\nzipnote error: Interrupted (aborting)\n");
            return ztools::ZE_ABORT;
        }
    };
    let arc = match ztools::parse(&data) {
        Ok(a) => a,
        Err(c) => return fail(c, &String::from_utf8_lossy(&zipfile)),
    };
    if write {
        write_mode(&zipfile, &data, arc)
    } else {
        list_mode(&arc);
        0
    }
}

/// Escreve um comentário com os fins de linha em `\n` e a quebra final.
fn put_comment(out: &mut Vec<u8>, c: &[u8]) {
    let mut j = 0;
    while j < c.len() {
        if c[j] == b'\r' && c.get(j + 1) == Some(&b'\n') {
            j += 1;
            continue;
        }
        out.push(c[j]);
        j += 1;
    }
    if !c.is_empty() && out.last() != Some(&b'\n') {
        out.push(b'\n');
    }
}

fn list_mode(arc: &Archive) {
    let mut out = Vec::new();
    for e in &arc.entries {
        out.extend_from_slice(b"@ ");
        out.extend_from_slice(&e.name);
        out.push(b'\n');
        put_comment(&mut out, &e.comment);
        out.extend_from_slice(b"@ (comment above this line)\n");
    }
    out.extend_from_slice(b"@ (zip file comment below this line)\n");
    put_comment(&mut out, &arc.comment);
    let mut o = Output::stdout();
    o.write(&out);
    let _ = o.finish();
}

fn strip_nl(mut v: Vec<u8>) -> Vec<u8> {
    if v.last() == Some(&b'\n') {
        v.pop();
        if v.last() == Some(&b'\r') {
            v.pop();
        }
    }
    v
}

fn write_mode(zipfile: &[u8], data: &[u8], arc: Archive) -> i32 {
    let input = sys::read_to_end(Fd::STDIN).unwrap_or_default();
    let n = arc.entries.len();
    let mut comments: Vec<Option<Vec<u8>>> = vec![None; n];
    let mut names: Vec<Option<Vec<u8>>> = vec![None; n];
    let mut zip_comment: Option<Vec<u8>> = None;
    let mut cur: Option<usize> = None;
    let mut buf: Vec<u8> = Vec::new();
    let mut in_zip_comment = false;
    let mut zc: Vec<u8> = Vec::new();
    for line in input.split_inclusive(|&b| b == b'\n') {
        if in_zip_comment {
            zc.extend_from_slice(line);
            continue;
        }
        if line.first() == Some(&b'@') {
            let l = strip_nl(line.to_vec());
            if l == b"@ (comment above this line)" {
                if let Some(i) = cur {
                    comments[i] = Some(strip_nl(std::mem::take(&mut buf)));
                }
                cur = None;
                buf.clear();
            } else if l == b"@ (zip file comment below this line)" {
                in_zip_comment = true;
            } else if l.starts_with(b"@=") {
                if let Some(i) = cur {
                    names[i] = Some(l[2..].to_vec());
                }
            } else if l.starts_with(b"@ ") {
                match arc.entries.iter().position(|e| e.name == l[2..]) {
                    Some(i) => {
                        cur = Some(i);
                        buf.clear();
                    }
                    None => {
                        sysutil::eprint(format!("zipnote warning: name not matched: {}\n", String::from_utf8_lossy(&l[2..])));
                        cur = None;
                    }
                }
            }
            continue;
        }
        if cur.is_some() {
            buf.extend_from_slice(line);
        }
    }
    if in_zip_comment {
        zip_comment = Some(strip_nl(zc));
    }

    let mut out = Vec::new();
    let first = arc.entries.iter().map(|e| e.off as usize).min().unwrap_or(0);
    out.extend_from_slice(&data[..first.min(data.len())]);
    let mut cds = Vec::new();
    for (i, e) in arc.entries.iter().enumerate() {
        let off = out.len() as u64;
        let name = names[i].clone().unwrap_or_else(|| e.name.clone());
        if ztools::copy_local(data, e, Some(&name), &mut out).is_err() {
            return fail(ZE_FORM, &String::from_utf8_lossy(zipfile));
        }
        let mut com = comments[i].clone().unwrap_or_else(|| e.comment.clone());
        com.truncate(0xFFFF);
        cds.push(ztools::central(e, off, &name, &com));
    }
    let cd_off = out.len();
    let mut size = 0;
    for c in &cds {
        size += c.len();
        out.extend_from_slice(c);
    }
    let mut zcom = zip_comment.unwrap_or_else(|| arc.comment.clone());
    zcom.truncate(0xFFFF);
    out.extend_from_slice(&ztools::eocd(n, size, cd_off, &zcom));
    if sysutil::write_file(zipfile, &out, 0o666).is_err() {
        return fail(ztools::ZE_WRITE, &String::from_utf8_lossy(zipfile));
    }
    0
}
