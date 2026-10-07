//! `dpkg-deb` do dpkg 1.22 (Debian 13): constrói (`-b`) e inspeciona (`-c`, `-I`, `-f`, `-W`) pacotes
//! `.deb`, extrai (`-x`, `-X`, `-R`, `-e`) e despeja os tarballs internos (`--ctrl-tarfile`,
//! `--fsys-tarfile`).
//!
//! O `.deb` é um arquivo `ar` com `debian-binary`, `control.tar[.ext]` e `data.tar[.ext]`. O dpkg-deb
//! original chama o `tar` e os compressores como subprocessos; aqui o tar (formato GNU, entradas
//! ordenadas pelo nome) é escrito e lido por este módulo e a compressão vem de [`crate::codec`]
//! (gzip, xz e zstd na construção; gzip, xz, zstd, bzip2 e lzma na leitura).
//!
//! Divergências conhecidas: formato antigo `0.939000` (só leitura de `--deb-format` aceita o valor,
//! sem efeito), `-S` e `--uniform-compression` são validados e ignorados, nomes de usuário e grupo no
//! tar de dados só são resolvidos pra root, e a data das listagens é sempre UTC.

use std::collections::HashMap;
use std::ffi::OsString;

use sysabi::{AtFlags, Clock, Ctx, Errno, Fd, FileType, OFlags, SetTime, Stat, TimeSpec, sys};
use ul_common::fsutil;
use ul_common::time::Civil;

use crate::codec::{self, Format, GzipHeader};
use crate::sysutil::{self, Output};

const PROG: &str = "dpkg-deb";
const VERSION_STR: &str = "1.22.22";

const USAGE: &str = "Usage: dpkg-deb [<option>...] <command>

Commands:
  -b|--build <directory> [<deb>]   Build an archive.
  -c|--contents <deb>              List contents.
  -I|--info <deb> [<cfile>...]     Show info to stdout.
  -W|--show <deb>...               Show information on package(s)
  -f|--field <deb> [<cfield>...]   Show field(s) to stdout.
  -e|--control <deb> [<directory>] Extract control info.
  -x|--extract <deb> <directory>   Extract files.
  -X|--vextract <deb> <directory>  Extract & list files.
  -R|--raw-extract <deb> <directory>
                                   Extract control info and files.
  --ctrl-tarfile <deb>             Output control tarfile.
  --fsys-tarfile <deb>             Output filesystem tarfile.

Options:
  --showformat=<format>            Use alternative format for --show.
  --deb-format=<format>            Select archive format ('2.0' or '0.939000').
  --nocheck                        Suppress control file check (build bad
                                     packages).
  --root-owner-group               Forces the owner and group to root.
  -z#                              Set the compression level when building.
  -Z<type>                         Set the compression type used when building.
                                     Allowed types: gzip, xz, zstd, none.
  -S<strategy>                     Set the compression strategy when building.
                                     Allowed values: none;
                                       extreme (xz);
                                       filtered, huffman, rle, fixed (gzip).
  --uniform-compression            Use the compression params on all members.
  --no-uniform-compression         Use the compression params on data member only.
                                   Implies --uniform-compression by default.

  -?, --help                       Show this help message.
      --version                    Show the version.

Use 'dpkg' to install and remove packages from your system, or
'dselect' or 'aptitude' for user-friendly package management.  Packages
unpacked using 'dpkg-deb --extract' will be incorrectly installed !
";

fn version_text() -> String {
    format!(
        "Debian 'dpkg-deb' package archive backend version {VERSION_STR} (amd64).\n\
This is free software; see the GNU General Public License version 2 or\n\
later for copying conditions. There is NO warranty.\n"
    )
}

thread_local! {
    /// Nome do programa nas mensagens: o `dpkg-split` reaproveita a leitura de `.deb` deste módulo.
    static PROG_NAME: std::cell::Cell<&'static str> = const { std::cell::Cell::new(PROG) };
}

/// Troca o nome do programa usado nas mensagens de erro e de aviso.
pub(crate) fn set_prog(name: &'static str) {
    PROG_NAME.with(|p| p.set(name));
}

fn prog() -> &'static str {
    PROG_NAME.with(|p| p.get())
}

pub(crate) fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

pub(crate) fn die(out: &mut Output, msg: &str) -> i32 {
    out.flush();
    sysutil::eprint(format!("{}: error: {msg}\n", prog()));
    2
}

pub(crate) fn die_errno(out: &mut Output, msg: &str, e: Errno) -> i32 {
    die(out, &format!("{msg}: {}", e.message()))
}

fn warn(out: &mut Output, msg: &str) {
    out.flush();
    sysutil::eprint(format!("{}: warning: {msg}\n", prog()));
}

fn badusage(out: &mut Output, msg: &str) -> i32 {
    out.flush();
    sysutil::eprint(format!(
        "{PROG}: error: {msg}\n\nType {PROG} --help for help about manipulating *.deb files;\nType dpkg --help for help about installing and deinstalling packages.\n"
    ));
    2
}

// ---------------------------------------------------------------------------------------------
// tar
// ---------------------------------------------------------------------------------------------

/// Uma entrada de tar, com o conteúdo na memória.
#[derive(Clone, Debug, Default)]
pub(crate) struct TarEntry {
    name: Vec<u8>,
    mode: u32,
    uid: u64,
    gid: u64,
    uname: Vec<u8>,
    gname: Vec<u8>,
    size: u64,
    mtime: i64,
    kind: u8,
    link: Vec<u8>,
    major: u32,
    minor: u32,
    data: Vec<u8>,
}

fn cstr(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(i) => &b[..i],
        None => b,
    }
}

fn parse_octal(b: &[u8]) -> u64 {
    if b.first().is_some_and(|&c| c & 0x80 != 0) {
        let mut v = u64::from(b[0] & 0x7f);
        for &x in &b[1..] {
            v = (v << 8) | u64::from(x);
        }
        return v;
    }
    let mut v = 0u64;
    let mut started = false;
    for &c in b {
        match c {
            b'0'..=b'7' => {
                started = true;
                v = v.wrapping_mul(8).wrapping_add(u64::from(c - b'0'));
            }
            b' ' if !started => {}
            _ => break,
        }
    }
    v
}

fn put_octal(buf: &mut [u8], v: u64) {
    let n = buf.len();
    let s = format!("{:0w$o}", v, w = n - 1);
    if s.len() > n - 1 {
        // Base 256 (extensão do GNU tar) pros valores que não cabem em octal.
        buf[0] = 0x80;
        let mut x = v;
        for i in (1..n).rev() {
            buf[i] = (x & 0xff) as u8;
            x >>= 8;
        }
    } else {
        buf[..n - 1].copy_from_slice(s.as_bytes());
        buf[n - 1] = 0;
    }
}

fn parse_pax(body: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p < body.len() {
        let rest = &body[p..];
        let Some(sp) = rest.iter().position(|&c| c == b' ') else { break };
        let len: usize = lossy(&rest[..sp]).parse().unwrap_or(0);
        if len == 0 || len > rest.len() {
            break;
        }
        let rec = &rest[sp + 1..len];
        let rec = rec.strip_suffix(b"\n").unwrap_or(rec);
        if let Some(eq) = rec.iter().position(|&c| c == b'=') {
            out.push((rec[..eq].to_vec(), rec[eq + 1..].to_vec()));
        }
        p += len;
    }
    out
}

fn parse_tar(data: &[u8]) -> Result<Vec<TarEntry>, String> {
    let mut pos = 0usize;
    let mut out = Vec::new();
    let mut long_name: Option<Vec<u8>> = None;
    let mut long_link: Option<Vec<u8>> = None;
    let mut pax: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    while pos + 512 <= data.len() {
        let h = &data[pos..pos + 512];
        if h.iter().all(|&b| b == 0) {
            break;
        }
        let stored = parse_octal(&h[148..156]);
        let sum: u64 = h
            .iter()
            .enumerate()
            .map(|(i, &b)| if (148..156).contains(&i) { 32 } else { u64::from(b) })
            .sum();
        if sum != stored {
            return Err("tar: This does not look like a tar archive".to_string());
        }
        let kind = h[156];
        let mut size = parse_octal(&h[124..136]);
        if let Some((_, v)) = pax.iter().find(|(k, _)| k == b"size") {
            size = lossy(v).parse().unwrap_or(size);
        }
        let body_start = pos + 512;
        let body_end = body_start.saturating_add(size as usize);
        if body_end > data.len() {
            return Err("tar: Unexpected EOF in archive".to_string());
        }
        let body = &data[body_start..body_end];
        pos = body_start + (size as usize).div_ceil(512) * 512;
        match kind {
            b'L' => {
                long_name = Some(cstr(body).to_vec());
                continue;
            }
            b'K' => {
                long_link = Some(cstr(body).to_vec());
                continue;
            }
            b'x' => {
                pax = parse_pax(body);
                continue;
            }
            b'g' => continue,
            _ => {}
        }
        let mut name = cstr(&h[0..100]).to_vec();
        if &h[257..263] == b"ustar\0" {
            let prefix = cstr(&h[345..500]);
            if !prefix.is_empty() {
                let mut n = prefix.to_vec();
                n.push(b'/');
                n.extend_from_slice(&name);
                name = n;
            }
        }
        let mut link = cstr(&h[157..257]).to_vec();
        let mut mtime = parse_octal(&h[136..148]) as i64;
        let mut uid = parse_octal(&h[108..116]);
        let mut gid = parse_octal(&h[116..124]);
        if let Some(n) = long_name.take() {
            name = n;
        }
        if let Some(l) = long_link.take() {
            link = l;
        }
        for (k, v) in pax.drain(..) {
            match k.as_slice() {
                b"path" => name = v,
                b"linkpath" => link = v,
                b"mtime" => {
                    mtime = lossy(&v).split('.').next().and_then(|s| s.parse().ok()).unwrap_or(mtime)
                }
                b"uid" => uid = lossy(&v).parse().unwrap_or(uid),
                b"gid" => gid = lossy(&v).parse().unwrap_or(gid),
                _ => {}
            }
        }
        let regular = matches!(kind, b'0' | 0 | b'7');
        out.push(TarEntry {
            name,
            mode: parse_octal(&h[100..108]) as u32,
            uid,
            gid,
            uname: cstr(&h[265..297]).to_vec(),
            gname: cstr(&h[297..329]).to_vec(),
            size,
            mtime,
            kind: if kind == 0 { b'0' } else { kind },
            link,
            major: parse_octal(&h[329..337]) as u32,
            minor: parse_octal(&h[337..345]) as u32,
            data: if regular { body.to_vec() } else { Vec::new() },
        });
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn raw_header(
    name: &[u8],
    mode: u32,
    uid: u64,
    gid: u64,
    size: u64,
    mtime: i64,
    kind: u8,
    link: &[u8],
    uname: &[u8],
    gname: &[u8],
    major: u32,
    minor: u32,
) -> [u8; 512] {
    let mut h = [0u8; 512];
    let n = name.len().min(100);
    h[..n].copy_from_slice(&name[..n]);
    put_octal(&mut h[100..108], u64::from(mode & 0o7777));
    put_octal(&mut h[108..116], uid);
    put_octal(&mut h[116..124], gid);
    put_octal(&mut h[124..136], size);
    put_octal(&mut h[136..148], mtime.max(0) as u64);
    h[148..156].copy_from_slice(b"        ");
    h[156] = kind;
    let l = link.len().min(100);
    h[157..157 + l].copy_from_slice(&link[..l]);
    h[257..265].copy_from_slice(b"ustar  \0");
    let un = uname.len().min(31);
    h[265..265 + un].copy_from_slice(&uname[..un]);
    let gn = gname.len().min(31);
    h[297..297 + gn].copy_from_slice(&gname[..gn]);
    put_octal(&mut h[329..337], u64::from(major));
    put_octal(&mut h[337..345], u64::from(minor));
    let sum: u64 = h.iter().map(|&b| u64::from(b)).sum();
    let cs = format!("{sum:06o}");
    h[148..154].copy_from_slice(cs.as_bytes());
    h[154] = 0;
    h[155] = b' ';
    h
}

fn push_padded(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(data);
    let pad = (512 - data.len() % 512) % 512;
    out.extend(std::iter::repeat_n(0u8, pad));
}

/// Serializa as entradas em tar GNU (nomes longos em `L`/`K`), fechado com dois blocos zero e
/// completado até o múltiplo de 10240 bytes do tar.
fn write_tar(entries: &[TarEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    for e in entries {
        if e.name.len() > 100 {
            let mut n = e.name.clone();
            n.push(0);
            out.extend_from_slice(&raw_header(
                b"././@LongLink", 0, 0, 0, n.len() as u64, 0, b'L', b"", b"root", b"root", 0, 0,
            ));
            push_padded(&mut out, &n);
        }
        if e.link.len() > 100 {
            let mut n = e.link.clone();
            n.push(0);
            out.extend_from_slice(&raw_header(
                b"././@LongLink", 0, 0, 0, n.len() as u64, 0, b'K', b"", b"root", b"root", 0, 0,
            ));
            push_padded(&mut out, &n);
        }
        let size = if matches!(e.kind, b'0') { e.data.len() as u64 } else { 0 };
        out.extend_from_slice(&raw_header(
            &e.name, e.mode, e.uid, e.gid, size, e.mtime, e.kind, &e.link, &e.uname, &e.gname, e.major,
            e.minor,
        ));
        if e.kind == b'0' {
            push_padded(&mut out, &e.data);
        }
    }
    out.extend(std::iter::repeat_n(0u8, 1024));
    let pad = (10240 - out.len() % 10240) % 10240;
    out.extend(std::iter::repeat_n(0u8, pad));
    out
}

fn mode_string(e: &TarEntry) -> String {
    let t = match e.kind {
        b'5' => 'd',
        b'2' => 'l',
        b'1' => 'h',
        b'3' => 'c',
        b'4' => 'b',
        b'6' => 'p',
        _ => '-',
    };
    fsutil::mode_string(t, e.mode)
}

fn escape_name(n: &[u8]) -> String {
    let mut s = String::new();
    for &b in n {
        match b {
            b'\\' => s.push_str("\\\\"),
            b'\n' => s.push_str("\\n"),
            b'\t' => s.push_str("\\t"),
            b'\r' => s.push_str("\\r"),
            0x20..=0x7e | 0x80..=0xff => s.push(b as char),
            _ => s.push_str(&format!("\\{b:03o}")),
        }
    }
    // Bytes altos voltam como UTF-8 quando válidos.
    if n.iter().any(|&b| b >= 0x80) {
        let mut v = Vec::new();
        for &b in n {
            match b {
                b'\\' => v.extend_from_slice(b"\\\\"),
                b'\n' => v.extend_from_slice(b"\\n"),
                b'\t' => v.extend_from_slice(b"\\t"),
                b'\r' => v.extend_from_slice(b"\\r"),
                0x20..=0x7e | 0x80..=0xff => v.push(b),
                _ => v.extend_from_slice(format!("\\{b:03o}").as_bytes()),
            }
        }
        return lossy(&v);
    }
    s
}

/// A linha do `tar -tv` do GNU tar. `ugw` é a largura acumulada da coluna dono/grupo e tamanho.
fn tv_line(e: &TarEntry, ugw: &mut usize) -> String {
    let user = if e.uname.is_empty() { e.uid.to_string() } else { lossy(&e.uname) };
    let group = if e.gname.is_empty() { e.gid.to_string() } else { lossy(&e.gname) };
    let ug = format!("{user}/{group}");
    let size = if matches!(e.kind, b'3' | b'4') {
        format!("{},{}", e.major, e.minor)
    } else if e.kind == b'0' {
        e.size.to_string()
    } else {
        "0".to_string()
    };
    if ug.len() + 1 + size.len() > *ugw {
        *ugw = ug.len() + 1 + size.len();
    }
    let c = Civil::from_secs(e.mtime);
    let mut s = format!(
        "{} {}{:>w$} {:04}-{:02}-{:02} {:02}:{:02} {}",
        mode_string(e),
        ug,
        size,
        c.year,
        c.mon,
        c.mday,
        c.hour,
        c.min,
        escape_name(&e.name),
        w = *ugw - ug.len()
    );
    match e.kind {
        b'2' => s.push_str(&format!(" -> {}", escape_name(&e.link))),
        b'1' => s.push_str(&format!(" link to {}", escape_name(&e.link))),
        _ => {}
    }
    s.push('\n');
    s
}

// ---------------------------------------------------------------------------------------------
// ar e .deb
// ---------------------------------------------------------------------------------------------

struct ArMember {
    name: String,
    data: Vec<u8>,
}

fn parse_ar(bytes: &[u8]) -> Result<Vec<ArMember>, String> {
    if bytes.len() < 8 || &bytes[..8] != b"!<arch>\n" {
        return Err("magic".to_string());
    }
    let mut pos = 8usize;
    let mut out = Vec::new();
    while pos < bytes.len() {
        if pos + 60 > bytes.len() {
            return Err("header".to_string());
        }
        let h = &bytes[pos..pos + 60];
        if &h[58..60] != b"`\n" {
            return Err("header".to_string());
        }
        let size: usize = lossy(&h[48..58]).trim().parse().map_err(|_| "header".to_string())?;
        let start = pos + 60;
        let end = start.checked_add(size).ok_or_else(|| "header".to_string())?;
        if end > bytes.len() {
            return Err("truncated".to_string());
        }
        let mut name = lossy(&h[..16]).trim_end().to_string();
        if name.len() > 1 && name.ends_with('/') {
            name.pop();
        }
        out.push(ArMember { name, data: bytes[start..end].to_vec() });
        pos = end + (size & 1);
    }
    Ok(out)
}

pub(crate) fn ar_member(name: &str, date: i64, data: &[u8]) -> Vec<u8> {
    let mut v = format!("{name:<16}{date:<12}0     0     100644  {:<10}`\n", data.len()).into_bytes();
    v.extend_from_slice(data);
    if data.len() & 1 == 1 {
        v.push(b'\n');
    }
    v
}

pub(crate) struct Deb {
    pub(crate) fsize: u64,
    pub(crate) vmaj: u32,
    pub(crate) vmin: u32,
    pub(crate) ctrl_name: String,
    pub(crate) ctrl_len: u64,
    pub(crate) ctrl: Vec<u8>,
    pub(crate) data_name: String,
    pub(crate) data: Vec<u8>,
}

fn member_format(name: &str) -> Result<Option<Format>, ()> {
    let ext = name.split_once("tar").map(|(_, e)| e).unwrap_or("");
    match ext {
        "" => Ok(None),
        ".gz" => Ok(Some(Format::Gzip)),
        ".xz" => Ok(Some(Format::Xz)),
        ".zst" => Ok(Some(Format::Zstd)),
        ".bz2" => Ok(Some(Format::Bzip2)),
        ".lzma" => Ok(Some(Format::Lzma)),
        _ => Err(()),
    }
}

pub(crate) fn read_deb(out: &mut Output, path: &[u8]) -> Result<Deb, i32> {
    let disp = lossy(path);
    let bytes = match sysutil::read_path(path) {
        Ok(b) => b,
        Err(e) => return Err(die_errno(out, &format!("failed to read archive '{disp}'"), e)),
    };
    let not_deb = format!("file '{disp}' is not a Debian binary archive (try dpkg-split?)");
    if bytes.len() < 8 || &bytes[..8] != b"!<arch>\n" {
        return Err(die(out, &not_deb));
    }
    let members = match parse_ar(&bytes) {
        Ok(m) => m,
        Err(_) => return Err(die(out, &format!("file '{disp}' is corrupt - bad archive header magic"))),
    };
    let mut it = members.into_iter();
    let Some(first) = it.next() else {
        return Err(die(out, &not_deb));
    };
    if first.name != "debian-binary" {
        return Err(die(out, &not_deb));
    }
    let vtxt = lossy(&first.data);
    let (vmaj, vmin) = {
        let line = vtxt.lines().next().unwrap_or("");
        let mut p = line.splitn(2, '.');
        let a: Option<u32> = p.next().and_then(|s| s.trim().parse().ok());
        let b: Option<u32> = p.next().and_then(|s| s.trim().parse().ok());
        match (a, b) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err(die(out, &format!("archive '{disp}' has invalid format version"))),
        }
    };
    if vmaj != 2 {
        return Err(die(
            out,
            &format!("archive format version {vmaj}.{vmin} not understood, get newer dpkg-deb"),
        ));
    }
    let mut ctrl: Option<ArMember> = None;
    let mut data: Option<ArMember> = None;
    for m in it {
        if m.name.starts_with('_') {
            continue;
        }
        if m.name.starts_with("control.tar") && ctrl.is_none() {
            ctrl = Some(m);
        } else if m.name.starts_with("data.tar") && data.is_none() {
            if ctrl.is_none() {
                return Err(die(
                    out,
                    &format!("archive '{disp}' contains data member before control member"),
                ));
            }
            data = Some(m);
            break;
        } else {
            return Err(die(out, &format!("file '{disp}' is corrupt - bad archive part name '{}'", m.name)));
        }
    }
    let (Some(c), Some(d)) = (ctrl, data) else {
        return Err(die(out, &format!("file '{disp}' is corrupt - missing control or data member")));
    };
    Ok(Deb {
        fsize: bytes.len() as u64,
        vmaj,
        vmin,
        ctrl_len: c.data.len() as u64,
        ctrl_name: c.name,
        ctrl: c.data,
        data_name: d.name,
        data: d.data,
    })
}

pub(crate) fn unpack_member(out: &mut Output, disp: &str, name: &str, bytes: &[u8]) -> Result<Vec<u8>, i32> {
    let fmt = match member_format(name) {
        Ok(f) => f,
        Err(()) => {
            return Err(die(
                out,
                &format!("archive '{disp}' uses unknown compression for member '{name}', giving up"),
            ));
        }
    };
    match fmt {
        None => Ok(bytes.to_vec()),
        Some(f) => match codec::decompress(f, bytes) {
            Ok(d) => Ok(d.data),
            Err(_) => Err(die(out, &format!("{} subprocess returned error exit status 1", f.label()))),
        },
    }
}

fn tar_entries(out: &mut Output, tar: &[u8]) -> Result<Vec<TarEntry>, i32> {
    match parse_tar(tar) {
        Ok(e) => Ok(e),
        Err(m) => {
            out.flush();
            sysutil::eprint(format!("{m}\ntar: Exiting with failure status due to previous errors\n"));
            Err(die(out, "tar subprocess returned error exit status 2"))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// campos de controle
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub(crate) struct Field {
    pub(crate) name: String,
    pub(crate) value: String,
}

/// Lê o primeiro parágrafo. `Err((linha, mensagem))` em erro de formato.
pub(crate) fn parse_control(text: &str) -> Result<Vec<Field>, (usize, String)> {
    let mut fields: Vec<Field> = Vec::new();
    let mut lno = 0usize;
    for line in text.split('\n') {
        lno += 1;
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with('#') {
            continue;
        }
        if line.trim().is_empty() {
            if fields.is_empty() {
                continue;
            }
            break;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            match fields.last_mut() {
                Some(f) => {
                    f.value.push('\n');
                    f.value.push_str(line);
                }
                None => return Err((lno, "first line of the control file is a continuation".to_string())),
            }
            continue;
        }
        let Some((n, v)) = line.split_once(':') else {
            return Err((lno, "line with unknown format (not field-colon-value)".to_string()));
        };
        fields.push(Field { name: n.to_string(), value: v.trim().to_string() });
    }
    Ok(fields)
}

pub(crate) fn field<'a>(fields: &'a [Field], name: &str) -> Option<&'a Field> {
    fields.iter().find(|f| f.name.eq_ignore_ascii_case(name))
}

fn show_format(fmt: &str, fields: &[Field]) -> String {
    let mut out = String::new();
    let cs: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        match cs[i] {
            '\\' => {
                i += 1;
                match cs.get(i) {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some(c) => out.push(*c),
                    None => {}
                }
                i += 1;
            }
            '$' if cs.get(i + 1) == Some(&'{') => {
                let mut j = i + 2;
                let mut spec = String::new();
                while j < cs.len() && cs[j] != '}' {
                    spec.push(cs[j]);
                    j += 1;
                }
                i = j + 1;
                let (name, width) = match spec.split_once(';') {
                    Some((n, w)) => (n.to_string(), w.trim().parse::<i64>().unwrap_or(0)),
                    None => (spec.clone(), 0),
                };
                let value = field(fields, &name).map(|f| f.value.clone()).unwrap_or_default();
                let w = width.unsigned_abs() as usize;
                if width < 0 {
                    out.push_str(&format!("{value:<w$}"));
                } else {
                    out.push_str(&format!("{value:>w$}"));
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

pub(crate) fn control_text(out: &mut Output, deb: &Deb, disp: &str) -> Result<(String, Vec<TarEntry>), i32> {
    let tar = unpack_member(out, disp, &deb.ctrl_name, &deb.ctrl)?;
    let entries = tar_entries(out, &tar)?;
    let text = entries
        .iter()
        .find(|e| e.kind == b'0' && strip_dot(&e.name) == b"control")
        .map(|e| lossy(&e.data))
        .unwrap_or_default();
    Ok((text, entries))
}

fn strip_dot(n: &[u8]) -> &[u8] {
    let mut n = n;
    while let Some(r) = n.strip_prefix(b"./") {
        n = r;
    }
    n.strip_suffix(b"/").unwrap_or(n)
}

// ---------------------------------------------------------------------------------------------
// opções
// ---------------------------------------------------------------------------------------------

const ACTIONS: &[(&str, char)] = &[
    ("build", 'b'),
    ("contents", 'c'),
    ("control", 'e'),
    ("info", 'I'),
    ("field", 'f'),
    ("extract", 'x'),
    ("vextract", 'X'),
    ("raw-extract", 'R'),
    ("ctrl-tarfile", '\0'),
    ("fsys-tarfile", '\0'),
    ("show", 'W'),
];

#[derive(Default)]
struct Opts {
    action: Option<(&'static str, char)>,
    showformat: Option<String>,
    nocheck: bool,
    root_owner_group: bool,
    level: Option<String>,
    ctype: Option<String>,
    strategy: Option<String>,
}

fn set_action(out: &mut Output, o: &mut Opts, long: &'static str, short: char) -> Result<(), i32> {
    if let Some((pl, ps)) = o.action {
        let sc = |c: char| if c == '\0' { String::new() } else { c.to_string() };
        return Err(badusage(
            out,
            &format!("conflicting actions -{} (--{}) and -{} (--{})", sc(short), long, sc(ps), pl),
        ));
    }
    o.action = Some((long, short));
    Ok(())
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
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
        if let Some(long) = a.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (long.to_string(), None),
            };
            if let Some(&(l, s)) = ACTIONS.iter().find(|(l, _)| *l == name) {
                if inline.is_some() {
                    return badusage(out, &format!("option --{name} doesn't take a value"));
                }
                if let Err(c) = set_action(out, &mut o, l, s) {
                    return c;
                }
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
                "nocheck" | "root-owner-group" | "uniform-compression" | "no-uniform-compression" => {
                    if inline.is_some() {
                        return badusage(out, &format!("option --{name} doesn't take a value"));
                    }
                    match name.as_str() {
                        "nocheck" => o.nocheck = true,
                        "root-owner-group" => o.root_owner_group = true,
                        _ => {}
                    }
                }
                "showformat" | "deb-format" => {
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
                    if name == "showformat" {
                        o.showformat = Some(v);
                    } else if v != "2.0" && v != "0.939000" {
                        return badusage(out, &format!("invalid deb format version: {v}"));
                    }
                }
                _ => return badusage(out, &format!("unknown option --{name}")),
            }
        } else {
            let mut chars = a.chars();
            chars.next();
            let c = chars.next().unwrap_or('-');
            let rest: String = chars.collect();
            if let Some(&(l, s)) = ACTIONS.iter().find(|(_, s)| *s == c && *s != '\0') {
                if !rest.is_empty() {
                    return badusage(out, &format!("unknown option -{c}"));
                }
                if let Err(code) = set_action(out, &mut o, l, s) {
                    return code;
                }
                continue;
            }
            match c {
                '?' => {
                    out.write_str(USAGE);
                    return 0;
                }
                'z' | 'Z' | 'S' => {
                    let v = if !rest.is_empty() {
                        rest
                    } else if i < args.len() {
                        i += 1;
                        args[i - 1].clone()
                    } else {
                        return badusage(out, &format!("-{c} option takes a value"));
                    };
                    match c {
                        'z' => o.level = Some(v),
                        'Z' => o.ctype = Some(v),
                        _ => o.strategy = Some(v),
                    }
                }
                _ => return badusage(out, &format!("unknown option -{c}")),
            }
        }
    }
    let rest: Vec<String> = args[i..].to_vec();
    let Some((action, _)) = o.action else {
        return badusage(out, "need an action option");
    };
    match action {
        "build" => do_build(out, &o, &rest),
        "contents" => do_contents(out, &rest),
        "info" => do_info(out, &rest),
        "field" => do_field(out, &rest),
        "show" => do_show(out, &o, &rest),
        "control" => do_control(out, &rest),
        "extract" | "vextract" => do_extract(out, action, &rest),
        "raw-extract" => do_raw_extract(out, &rest),
        "ctrl-tarfile" | "fsys-tarfile" => do_tarfile(out, action, &rest),
        _ => 2,
    }
}

fn one_deb<'a>(out: &mut Output, action: &str, rest: &'a [String]) -> Result<&'a str, i32> {
    match rest {
        [] => Err(badusage(out, &format!("--{action} needs a .deb filename argument"))),
        [d] => Ok(d.as_str()),
        _ => Err(badusage(out, &format!("--{action} takes only one argument (.deb filename)"))),
    }
}

// ---------------------------------------------------------------------------------------------
// ações de leitura
// ---------------------------------------------------------------------------------------------

fn do_contents(out: &mut Output, rest: &[String]) -> i32 {
    let path = match one_deb(out, "contents", rest) {
        Ok(p) => p,
        Err(c) => return c,
    };
    let deb = match read_deb(out, path.as_bytes()) {
        Ok(d) => d,
        Err(c) => return c,
    };
    let tar = match unpack_member(out, path, &deb.data_name, &deb.data) {
        Ok(t) => t,
        Err(c) => return c,
    };
    let entries = match tar_entries(out, &tar) {
        Ok(e) => e,
        Err(c) => return c,
    };
    let mut ugw = 19usize;
    for e in &entries {
        out.write_str(&tv_line(e, &mut ugw));
    }
    0
}

fn do_tarfile(out: &mut Output, action: &str, rest: &[String]) -> i32 {
    let path = match one_deb(out, action, rest) {
        Ok(p) => p,
        Err(c) => return c,
    };
    let deb = match read_deb(out, path.as_bytes()) {
        Ok(d) => d,
        Err(c) => return c,
    };
    let (name, bytes) = if action == "ctrl-tarfile" {
        (&deb.ctrl_name, &deb.ctrl)
    } else {
        (&deb.data_name, &deb.data)
    };
    match unpack_member(out, path, name, bytes) {
        Ok(t) => {
            out.write(&t);
            0
        }
        Err(c) => c,
    }
}

fn do_info(out: &mut Output, rest: &[String]) -> i32 {
    let Some(path) = rest.first() else {
        return badusage(out, "--info needs a .deb filename argument");
    };
    let deb = match read_deb(out, path.as_bytes()) {
        Ok(d) => d,
        Err(c) => return c,
    };
    let (_, entries) = match control_text(out, &deb, path) {
        Ok(r) => r,
        Err(c) => return c,
    };
    let regs: Vec<&TarEntry> = {
        let mut v: Vec<&TarEntry> = entries.iter().filter(|e| e.kind == b'0').collect();
        v.sort_by(|a, b| strip_dot(&a.name).cmp(strip_dot(&b.name)));
        v
    };
    if rest.len() > 1 {
        let mut status = 0;
        for want in &rest[1..] {
            match regs.iter().find(|e| strip_dot(&e.name) == want.as_bytes()) {
                Some(e) => out.write(&e.data),
                None => {
                    out.flush();
                    sysutil::eprint(format!(
                        "{PROG}: error: '{path}' contains no control component '{want}'\n"
                    ));
                    status = 2;
                }
            }
        }
        return status;
    }
    out.write_str(&format!(" new Debian package, version {}.{}.\n", deb.vmaj, deb.vmin));
    out.write_str(&format!(" size {} bytes: control archive={} bytes.\n", deb.fsize, deb.ctrl_len));
    for e in &regs {
        let lines = e.data.iter().filter(|&&b| b == b'\n').count();
        let mut interp = String::new();
        if e.data.starts_with(b"#!") {
            let first = e.data.split(|&b| b == b'\n').next().unwrap_or(&[]);
            let txt = lossy(&first[2..]);
            if let Some(tok) = txt.split_whitespace().next() {
                interp = format!("#!{tok}");
            }
        }
        let name = lossy(strip_dot(&e.name));
        out.write_str(&format!(
            "{:7} bytes, {:5} lines   {}  {:<20.127} {:.127}\n",
            e.data.len(),
            lines,
            if e.mode & 0o100 != 0 { '*' } else { ' ' },
            name,
            interp
        ));
    }
    if let Some(c) = regs.iter().find(|e| strip_dot(&e.name) == b"control") {
        let text = lossy(&c.data);
        let mut parts: Vec<&str> = text.split('\n').collect();
        let had_nl = parts.last() == Some(&"");
        if had_nl {
            parts.pop();
        }
        for l in parts {
            out.write_str(&format!(" {l}\n"));
        }
    }
    0
}

fn do_field(out: &mut Output, rest: &[String]) -> i32 {
    let Some(path) = rest.first() else {
        return badusage(out, "--field needs a .deb filename argument");
    };
    let deb = match read_deb(out, path.as_bytes()) {
        Ok(d) => d,
        Err(c) => return c,
    };
    let (text, _) = match control_text(out, &deb, path) {
        Ok(r) => r,
        Err(c) => return c,
    };
    if rest.len() == 1 {
        out.write_str(&text);
        return 0;
    }
    let fields = match parse_control(&text) {
        Ok(f) => f,
        Err((n, m)) => {
            return die(out, &format!("parsing file '{path}' near line {n}:\n {m}"));
        }
    };
    let names = &rest[1..];
    for n in names {
        if let Some(f) = field(&fields, n) {
            if names.len() == 1 {
                out.write_str(&format!("{}\n", f.value));
            } else {
                out.write_str(&format!("{}: {}\n", f.name, f.value));
            }
        }
    }
    0
}

fn do_show(out: &mut Output, o: &Opts, rest: &[String]) -> i32 {
    if rest.is_empty() {
        return badusage(out, "--show needs a .deb filename argument");
    }
    let fmt = o.showformat.clone().unwrap_or_else(|| "${Package}\t${Version}\n".to_string());
    for path in rest {
        let deb = match read_deb(out, path.as_bytes()) {
            Ok(d) => d,
            Err(c) => return c,
        };
        let (text, _) = match control_text(out, &deb, path) {
            Ok(r) => r,
            Err(c) => return c,
        };
        let fields = match parse_control(&text) {
            Ok(f) => f,
            Err((n, m)) => {
                return die(out, &format!("parsing file '{path}' near line {n}:\n {m}"));
            }
        };
        out.write_str(&show_format(&fmt, &fields));
    }
    0
}

// ---------------------------------------------------------------------------------------------
// extração
// ---------------------------------------------------------------------------------------------

fn makedev(major: u32, minor: u32) -> u64 {
    let (ma, mi) = (u64::from(major), u64::from(minor));
    ((ma & 0xfff) << 8) | (mi & 0xff) | ((mi & !0xff) << 12) | ((ma & !0xfff) << 32)
}

fn join_path(dir: &[u8], name: &[u8]) -> Vec<u8> {
    if name.is_empty() {
        return dir.to_vec();
    }
    sysutil::join(dir, name)
}

fn apply_meta(e: &TarEntry, path: &[u8], root: bool, symlink: bool) {
    let s = sys::current();
    if root {
        let _ = s.fchownat(Fd::CWD, path, Some(e.uid as u32), Some(e.gid as u32), AtFlags::SYMLINK_NOFOLLOW);
    }
    if !symlink {
        let _ = s.fchmodat(Fd::CWD, path, e.mode & 0o7777, AtFlags::empty());
    }
    let _ = s.utimensat(
        Fd::CWD,
        path,
        SetTime::Omit,
        SetTime::At(TimeSpec { sec: e.mtime, nsec: 0 }),
        AtFlags::SYMLINK_NOFOLLOW,
    );
}

/// Extrai as entradas em `dir`, como o `tar -x -p` que o dpkg-deb dispara.
fn extract_entries(out: &mut Output, entries: &[TarEntry], dir: &[u8], verbose: bool) -> i32 {
    let s = sys::current();
    let root = s.geteuid() == 0;
    let mut status = 0;
    let mut dirs: Vec<(Vec<u8>, usize)> = Vec::new();
    let tarerr = |out: &mut Output, msg: String| {
        out.flush();
        sysutil::eprint(format!("tar: {msg}\n"));
    };
    for (idx, e) in entries.iter().enumerate() {
        let shown = lossy(&e.name);
        let name = strip_dot(&e.name).to_vec();
        let name: Vec<u8> = {
            let mut n = name.as_slice();
            while let Some(r) = n.strip_prefix(b"/") {
                n = r;
            }
            n.to_vec()
        };
        if name.split(|&b| b == b'/').any(|c| c == b"..") {
            tarerr(out, format!("{shown}: Member name contains '..'"));
            status = 2;
            continue;
        }
        if verbose {
            out.write_str(&format!("{shown}\n"));
        }
        let path = join_path(dir, &name);
        if let Some(i) = path.iter().rposition(|&b| b == b'/') {
            if i > 0 && !name.is_empty() {
                let _ = fsutil::mkdir_p(&path[..i], 0o777);
            }
        }
        match e.kind {
            b'5' => {
                match s.mkdirat(Fd::CWD, &path, 0o777) {
                    Ok(()) => {}
                    Err(Errno::EEXIST) if sys::stat(&path).is_ok_and(|st| st.file_type() == FileType::Directory) => {}
                    Err(er) => {
                        tarerr(out, format!("{shown}: Cannot mkdir: {}", er.message()));
                        status = 2;
                        continue;
                    }
                }
                dirs.push((path, idx));
            }
            b'2' => {
                let _ = s.unlinkat(Fd::CWD, &path, AtFlags::empty());
                match s.symlinkat(&e.link, Fd::CWD, &path) {
                    Ok(()) => apply_meta(e, &path, root, true),
                    Err(er) => {
                        tarerr(out, format!("{shown}: Cannot create symlink to '{}': {}", lossy(&e.link), er.message()));
                        status = 2;
                    }
                }
            }
            b'1' => {
                let target = join_path(dir, strip_dot(&e.link));
                let _ = s.unlinkat(Fd::CWD, &path, AtFlags::empty());
                if let Err(er) = s.linkat(Fd::CWD, &target, Fd::CWD, &path, AtFlags::empty()) {
                    tarerr(out, format!("{shown}: Cannot hard link to '{}': {}", lossy(&e.link), er.message()));
                    status = 2;
                }
            }
            b'0' => {
                let _ = s.unlinkat(Fd::CWD, &path, AtFlags::empty());
                let fd = match sys::open(&path, OFlags::WRONLY | OFlags::CREAT | OFlags::EXCL | OFlags::CLOEXEC, 0o600) {
                    Ok(fd) => fd,
                    Err(er) => {
                        tarerr(out, format!("{shown}: Cannot open: {}", er.message()));
                        status = 2;
                        continue;
                    }
                };
                let w = sys::write_all(fd, &e.data);
                let _ = sys::close(fd);
                if let Err(er) = w {
                    tarerr(out, format!("{shown}: Cannot write: {}", er.message()));
                    status = 2;
                    continue;
                }
                apply_meta(e, &path, root, false);
            }
            b'3' | b'4' | b'6' => {
                let _ = s.unlinkat(Fd::CWD, &path, AtFlags::empty());
                let ty = match e.kind {
                    b'3' => sysabi::mode::S_IFCHR,
                    b'4' => sysabi::mode::S_IFBLK,
                    _ => sysabi::mode::S_IFIFO,
                };
                match s.mknodat(Fd::CWD, &path, ty | (e.mode & 0o7777), makedev(e.major, e.minor)) {
                    Ok(()) => apply_meta(e, &path, root, false),
                    Err(er) => {
                        tarerr(out, format!("{shown}: Cannot mknod: {}", er.message()));
                        status = 2;
                    }
                }
            }
            k => {
                tarerr(out, format!("{shown}: Unknown file type '{}', extracted as normal file", k as char));
                status = 2;
            }
        }
    }
    for (path, idx) in dirs.iter().rev() {
        apply_meta(&entries[*idx], path, root, false);
    }
    if status != 0 {
        out.flush();
        sysutil::eprint("tar: Exiting with failure status due to previous errors\n");
        return die(out, "tar subprocess returned error exit status 2");
    }
    0
}

fn make_target(out: &mut Output, dir: &str) -> Result<(), i32> {
    match sys::current().mkdirat(Fd::CWD, dir.as_bytes(), 0o777) {
        Ok(()) | Err(Errno::EEXIST) => Ok(()),
        Err(e) => Err(die_errno(out, "failed to create directory", e)),
    }
}

fn do_extract(out: &mut Output, action: &str, rest: &[String]) -> i32 {
    let Some(path) = rest.first() else {
        return badusage(out, &format!("--{action} needs a .deb filename argument"));
    };
    let Some(dir) = rest.get(1) else {
        return badusage(
            out,
            &format!("--{action} needs a target directory.\nPerhaps you should be using dpkg --install ?"),
        );
    };
    if rest.len() > 2 {
        return badusage(out, &format!("--{action} takes at most two arguments (.deb and directory)"));
    }
    let deb = match read_deb(out, path.as_bytes()) {
        Ok(d) => d,
        Err(c) => return c,
    };
    if let Err(c) = make_target(out, dir) {
        return c;
    }
    let tar = match unpack_member(out, path, &deb.data_name, &deb.data) {
        Ok(t) => t,
        Err(c) => return c,
    };
    let entries = match tar_entries(out, &tar) {
        Ok(e) => e,
        Err(c) => return c,
    };
    extract_entries(out, &entries, dir.as_bytes(), action == "vextract")
}

fn do_control(out: &mut Output, rest: &[String]) -> i32 {
    let Some(path) = rest.first() else {
        return badusage(out, "--control needs a .deb filename argument");
    };
    if rest.len() > 2 {
        return badusage(out, "--control takes at most two arguments (.deb and directory)");
    }
    let dir = rest.get(1).cloned().unwrap_or_else(|| "DEBIAN".to_string());
    let deb = match read_deb(out, path.as_bytes()) {
        Ok(d) => d,
        Err(c) => return c,
    };
    if let Err(c) = make_target(out, &dir) {
        return c;
    }
    let tar = match unpack_member(out, path, &deb.ctrl_name, &deb.ctrl) {
        Ok(t) => t,
        Err(c) => return c,
    };
    let entries = match tar_entries(out, &tar) {
        Ok(e) => e,
        Err(c) => return c,
    };
    extract_entries(out, &entries, dir.as_bytes(), false)
}

fn do_raw_extract(out: &mut Output, rest: &[String]) -> i32 {
    let Some(path) = rest.first() else {
        return badusage(out, "--raw-extract needs a .deb filename argument");
    };
    let Some(dir) = rest.get(1) else {
        return badusage(
            out,
            "--raw-extract needs a target directory.\nPerhaps you should be using dpkg --install ?",
        );
    };
    if rest.len() > 2 {
        return badusage(out, "--raw-extract takes at most two arguments (.deb and directory)");
    }
    let deb = match read_deb(out, path.as_bytes()) {
        Ok(d) => d,
        Err(c) => return c,
    };
    if let Err(c) = make_target(out, dir) {
        return c;
    }
    let ctrl_dir = format!("{dir}/DEBIAN");
    if let Err(c) = make_target(out, &ctrl_dir) {
        return c;
    }
    let tar = match unpack_member(out, path, &deb.ctrl_name, &deb.ctrl) {
        Ok(t) => t,
        Err(c) => return c,
    };
    let entries = match tar_entries(out, &tar) {
        Ok(e) => e,
        Err(c) => return c,
    };
    let c = extract_entries(out, &entries, ctrl_dir.as_bytes(), false);
    if c != 0 {
        return c;
    }
    let tar = match unpack_member(out, path, &deb.data_name, &deb.data) {
        Ok(t) => t,
        Err(c) => return c,
    };
    let entries = match tar_entries(out, &tar) {
        Ok(e) => e,
        Err(c) => return c,
    };
    extract_entries(out, &entries, dir.as_bytes(), false)
}

// ---------------------------------------------------------------------------------------------
// construção
// ---------------------------------------------------------------------------------------------

/// Campos que o dpkg conhece; os demais (fora de `X[BCS]-`) geram o aviso de campo do usuário.
const KNOWN_FIELDS: &[&str] = &[
    "Package", "Essential", "Status", "Priority", "Section", "Installed-Size", "Origin", "Maintainer",
    "Bugs", "Architecture", "Multi-Arch", "Source", "Version", "Revision", "Config-Version", "Replaces",
    "Provides", "Depends", "Pre-Depends", "Recommends", "Suggests", "Breaks", "Conflicts", "Enhances",
    "Conffiles", "Filename", "Size", "MD5sum", "MSDOS-Filename", "Description", "Triggers-Pending",
    "Triggers-Awaited", "Built-Using", "Homepage", "Protected", "Static-Built-Using",
];

fn is_user_defined_ok(name: &str) -> bool {
    let b = name.as_bytes();
    if b.first().is_none_or(|c| !c.eq_ignore_ascii_case(&b'x')) {
        return false;
    }
    let rest = &b[1..];
    let n = rest.iter().take_while(|c| matches!(c.to_ascii_lowercase(), b'b' | b'c' | b's')).count();
    n > 0 && rest.get(n) == Some(&b'-')
}

struct Walker {
    control: bool,
    root_owner_group: bool,
    sde: Option<i64>,
    seen: HashMap<(u64, u64), Vec<u8>>,
    entries: Vec<TarEntry>,
}

fn major_of(rdev: u64) -> u32 {
    (((rdev >> 8) & 0xfff) | ((rdev >> 32) & !0xfff)) as u32
}

fn minor_of(rdev: u64) -> u32 {
    ((rdev & 0xff) | ((rdev >> 12) & !0xff)) as u32
}

impl Walker {
    fn base_entry(&self, st: &Stat, name: Vec<u8>) -> TarEntry {
        let zero = self.control || self.root_owner_group;
        let (uid, gid) = if zero { (0, 0) } else { (u64::from(st.uid), u64::from(st.gid)) };
        let mut mtime = st.mtime.sec;
        if let Some(sde) = self.sde {
            if mtime > sde {
                mtime = sde;
            }
        }
        let root_name = |id: u64| if id == 0 { b"root".to_vec() } else { Vec::new() };
        TarEntry {
            name,
            mode: st.mode & 0o7777,
            uid,
            gid,
            uname: root_name(uid),
            gname: root_name(gid),
            mtime,
            ..TarEntry::default()
        }
    }

    fn walk(&mut self, fs: &[u8], tarname: &[u8], st: &Stat, skip_debian: bool) -> Result<(), String> {
        let mut dname = tarname.to_vec();
        dname.push(b'/');
        let mut e = self.base_entry(st, dname);
        e.kind = b'5';
        self.entries.push(e);
        let mut names: Vec<Vec<u8>> = match sys::read_dir(fs) {
            Ok(v) => v.into_iter().map(|d| d.name).collect(),
            Err(er) => return Err(format!("unable to read directory '{}': {}", lossy(fs), er.message())),
        };
        names.sort();
        for n in names {
            if skip_debian && n == b"DEBIAN" {
                continue;
            }
            let cfs = sysutil::join(fs, &n);
            let mut ctar = tarname.to_vec();
            ctar.push(b'/');
            ctar.extend_from_slice(&n);
            let cst = match sys::lstat(&cfs) {
                Ok(s) => s,
                Err(er) => return Err(format!("unable to stat '{}': {}", lossy(&cfs), er.message())),
            };
            match cst.file_type() {
                FileType::Directory => self.walk(&cfs, &ctar, &cst, false)?,
                FileType::Symlink => {
                    let mut e = self.base_entry(&cst, ctar);
                    e.kind = b'2';
                    e.mode = 0o777;
                    e.link = sys::current()
                        .readlinkat(Fd::CWD, &cfs)
                        .map_err(|er| format!("unable to read link '{}': {}", lossy(&cfs), er.message()))?;
                    self.entries.push(e);
                }
                FileType::Regular => {
                    let mut e = self.base_entry(&cst, ctar.clone());
                    if cst.nlink > 1 {
                        if let Some(first) = self.seen.get(&(cst.dev, cst.ino)) {
                            e.kind = b'1';
                            e.link = first.clone();
                            self.entries.push(e);
                            continue;
                        }
                        self.seen.insert((cst.dev, cst.ino), ctar);
                    }
                    e.kind = b'0';
                    e.data = sysutil::read_path(&cfs)
                        .map_err(|er| format!("unable to open '{}': {}", lossy(&cfs), er.message()))?;
                    e.size = e.data.len() as u64;
                    self.entries.push(e);
                }
                FileType::CharDevice | FileType::BlockDevice | FileType::Fifo => {
                    let mut e = self.base_entry(&cst, ctar);
                    e.kind = match cst.file_type() {
                        FileType::CharDevice => b'3',
                        FileType::BlockDevice => b'4',
                        _ => b'6',
                    };
                    e.major = major_of(cst.rdev);
                    e.minor = minor_of(cst.rdev);
                    self.entries.push(e);
                }
                FileType::Socket => {}
            }
        }
        Ok(())
    }
}

fn compress_tar(
    out: &mut Output,
    tar: &[u8],
    ctype: &str,
    level: Option<u32>,
) -> Result<(Vec<u8>, &'static str), i32> {
    let (fmt, ext, lv) = match ctype {
        "gzip" => (Format::Gzip, ".gz", level.unwrap_or(9).clamp(1, 9)),
        "xz" => (Format::Xz, ".xz", level.unwrap_or(6)),
        "zstd" => (Format::Zstd, ".zst", level.unwrap_or(9)),
        _ => return Ok((tar.to_vec(), "")),
    };
    match codec::compress(fmt, tar, lv, &GzipHeader::default()) {
        Ok(b) => Ok((b, ext)),
        Err(e) => Err(die(out, &format!("{} subprocess returned error exit status 1: {e}", fmt.label()))),
    }
}

fn do_build(out: &mut Output, o: &Opts, rest: &[String]) -> i32 {
    let Some(dir) = rest.first() else {
        return badusage(out, "--build needs a <directory> argument");
    };
    if rest.len() > 2 {
        return badusage(out, "--build takes at most two arguments (directory and .deb)");
    }
    // Opções de compressão.
    let ctype = o.ctype.clone().unwrap_or_else(|| "xz".to_string());
    if !matches!(ctype.as_str(), "gzip" | "xz" | "zstd" | "none") {
        return die(out, &format!("unknown compression type '{ctype}'!"));
    }
    let level = match &o.level {
        None => None,
        Some(v) => match v.parse::<u32>() {
            Ok(n) => {
                let max = match ctype.as_str() {
                    "gzip" | "xz" => 9,
                    "zstd" => 22,
                    _ => 9,
                };
                if n > max {
                    return die(out, &format!("invalid compression level for {ctype}: {n}"));
                }
                Some(n)
            }
            Err(_) => return die(out, &format!("invalid compression level '{v}'")),
        },
    };
    if let Some(s) = &o.strategy {
        if !matches!(s.as_str(), "none" | "extreme" | "filtered" | "huffman" | "rle" | "fixed") {
            return die(out, &format!("unknown compression strategy '{s}'!"));
        }
    }

    // Destino.
    let trimmed = {
        let mut d = dir.as_str();
        while d.len() > 1 && d.ends_with('/') {
            d = &d[..d.len() - 1];
        }
        d.to_string()
    };
    let mut debar: Option<String> = rest.get(1).cloned();
    let mut subdir = false;
    if let Some(p) = &debar {
        if sys::stat(p.as_bytes()).is_ok_and(|s| s.file_type() == FileType::Directory) {
            subdir = true;
        }
    }
    if debar.is_none() {
        debar = Some(format!("{trimmed}.deb"));
    }
    let mut debar = debar.unwrap_or_default();

    // Arquivo de controle.
    let ctrl_dir = format!("{trimmed}/DEBIAN");
    let ctrl_file = format!("{ctrl_dir}/control");
    let text = match sysutil::read_path(ctrl_file.as_bytes()) {
        Ok(b) => lossy(&b),
        Err(e) => {
            return die_errno(
                out,
                &format!("failed to open package info file '{ctrl_file}' for reading"),
                e,
            );
        }
    };
    let nlines = text.matches('\n').count();
    let fields = match parse_control(&text) {
        Ok(f) => f,
        Err((n, m)) => {
            return die(out, &format!("parsing file '{ctrl_file}' near line {n}:\n {m}"));
        }
    };
    let pkg = field(&fields, "Package").map(|f| f.value.clone());
    let ver = field(&fields, "Version").map(|f| f.value.clone());
    let arch = field(&fields, "Architecture").map(|f| f.value.clone());
    if !o.nocheck {
        let near = |p: &Option<String>| match p {
            Some(p) => format!("parsing file '{ctrl_file}' near line {nlines} package '{p}':\n"),
            None => format!("parsing file '{ctrl_file}' near line {nlines}:\n"),
        };
        if pkg.is_none() {
            return die(out, &format!("{} missing 'Package' field", near(&pkg)));
        }
        if ver.as_deref().is_none_or(str::is_empty) {
            return die(out, &format!("{} missing 'Version' field", near(&pkg)));
        }
        if arch.as_deref().is_none_or(str::is_empty) {
            return die(out, &format!("{} missing 'Architecture' field", near(&pkg)));
        }
        let name = pkg.clone().unwrap_or_default();
        if name.is_empty()
            || !name.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'+' | b'-' | b'.'))
        {
            return die(out, "package name has characters that aren't lowercase alphanums or '-+.'");
        }
        if field(&fields, "Maintainer").is_none() {
            warn(out, &format!("{} missing 'Maintainer' field", near(&pkg)));
        }
        if field(&fields, "Description").is_none() {
            warn(out, &format!("{} missing 'Description' field", near(&pkg)));
        }
        for f in &fields {
            if !KNOWN_FIELDS.iter().any(|k| k.eq_ignore_ascii_case(&f.name)) && !is_user_defined_ok(&f.name) {
                warn(out, &format!("'{ctrl_file}' contains user-defined field '{}'", f.name));
            }
        }
        // Permissões do diretório de controle e dos scripts.
        let dst = match sys::lstat(ctrl_dir.as_bytes()) {
            Ok(s) => s,
            Err(e) => {
                return die_errno(out, &format!("unable to check for existence of '{ctrl_dir}'"), e);
            }
        };
        let perm = dst.mode & 0o7777;
        if !(0o755..=0o775).contains(&perm) {
            return die(
                out,
                &format!("control directory has bad permissions {perm:03o} (must be >=0755 and <=0775)"),
            );
        }
        for script in ["preinst", "postinst", "prerm", "postrm", "config"] {
            let p = format!("{ctrl_dir}/{script}");
            let Ok(st) = sys::lstat(p.as_bytes()) else { continue };
            match st.file_type() {
                FileType::Symlink => {}
                FileType::Regular => {
                    let perm = st.mode & 0o7777;
                    if !(0o555..=0o775).contains(&perm) {
                        return die(
                            out,
                            &format!(
                                "maintainer script '{script}' has bad permissions {perm:03o} (must be >=0555 and <=0775)"
                            ),
                        );
                    }
                }
                _ => {
                    return die(
                        out,
                        &format!("maintainer script '{script}' is not a plain file or symlink (see dpkg-deb(1))"),
                    );
                }
            }
        }
        // conffiles.
        if let Ok(b) = sysutil::read_path(format!("{ctrl_dir}/conffiles").as_bytes()) {
            let ctext = lossy(&b);
            let mut seen: Vec<&str> = Vec::new();
            let lines: Vec<&str> = ctext.split('\n').collect();
            let has_final_nl = ctext.ends_with('\n');
            for (i, raw) in lines.iter().enumerate() {
                if i + 1 == lines.len() && raw.is_empty() {
                    break;
                }
                if i + 1 == lines.len() && !has_final_nl {
                    return die(
                        out,
                        &format!("conffile name '{}' is too long, or missing final newline", raw),
                    );
                }
                let line = raw.trim_end();
                if line.is_empty() {
                    continue;
                }
                let line = line.strip_prefix("remove-on-upgrade ").unwrap_or(line);
                if !line.starts_with('/') {
                    return die(out, &format!("conffile name '{line}' is not an absolute pathname"));
                }
                let full = format!("{trimmed}{line}");
                match sys::lstat(full.as_bytes()) {
                    Err(_) => {
                        return die(out, &format!("conffile '{line}' does not appear in package"));
                    }
                    Ok(st) if st.file_type() == FileType::Directory => {
                        return die(out, &format!("conffile '{line}' is not a plain file"));
                    }
                    Ok(_) => {}
                }
                if seen.contains(&line) {
                    warn(out, &format!("conffile name '{line}' is duplicated"));
                } else {
                    seen.push(line);
                }
            }
        }
    }

    // Nome do arquivo de saída quando o destino é um diretório.
    if subdir {
        let v = ver.clone().unwrap_or_default();
        let v = match v.split_once(':') {
            Some((e, r)) if e.bytes().all(|c| c.is_ascii_digit()) && !e.is_empty() => r.to_string(),
            _ => v,
        };
        let sep = if debar.ends_with('/') { "" } else { "/" };
        debar = format!(
            "{debar}{sep}{}_{}_{}.deb",
            pkg.clone().unwrap_or_default(),
            v,
            arch.clone().unwrap_or_default()
        );
    }
    out.write_str(&format!(
        "dpkg-deb: building package '{}' in '{}'.\n",
        pkg.clone().unwrap_or_default(),
        debar
    ));

    let sde: Option<i64> = sysutil::getenv("SOURCE_DATE_EPOCH").and_then(|v| lossy(&v).trim().parse().ok());
    let now = match sde {
        Some(s) => s,
        None => sys::current().clock_gettime(Clock::Realtime).map(|t| t.sec).unwrap_or(0),
    };

    // Tar de controle.
    let dst = match sys::lstat(ctrl_dir.as_bytes()) {
        Ok(s) => s,
        Err(e) => return die_errno(out, &format!("unable to check for existence of '{ctrl_dir}'"), e),
    };
    let mut cw = Walker {
        control: true,
        root_owner_group: o.root_owner_group,
        sde,
        seen: HashMap::new(),
        entries: Vec::new(),
    };
    if let Err(m) = cw.walk(ctrl_dir.as_bytes(), b".", &dst, false) {
        return die(out, &m);
    }
    // Tar de dados.
    let rst = match sys::lstat(trimmed.as_bytes()) {
        Ok(s) => s,
        Err(e) => return die_errno(out, &format!("unable to check for existence of '{trimmed}'"), e),
    };
    let mut dw = Walker {
        control: false,
        root_owner_group: o.root_owner_group,
        sde,
        seen: HashMap::new(),
        entries: Vec::new(),
    };
    if let Err(m) = dw.walk(trimmed.as_bytes(), b".", &rst, true) {
        return die(out, &m);
    }
    let ctar = write_tar(&cw.entries);
    let dtar = write_tar(&dw.entries);
    let (cz, cext) = match compress_tar(out, &ctar, &ctype, level) {
        Ok(r) => r,
        Err(c) => return c,
    };
    let (dz, dext) = match compress_tar(out, &dtar, &ctype, level) {
        Ok(r) => r,
        Err(c) => return c,
    };
    let mut deb = b"!<arch>\n".to_vec();
    deb.extend(ar_member("debian-binary", now, b"2.0\n"));
    deb.extend(ar_member(&format!("control.tar{cext}"), now, &cz));
    deb.extend(ar_member(&format!("data.tar{dext}"), now, &dz));
    if let Err(e) = sysutil::write_file(debar.as_bytes(), &deb, 0o666) {
        return die_errno(out, &format!("unable to create '{debar}'"), e);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tar_roundtrip() {
        let e = TarEntry {
            name: b"./a/".to_vec(),
            mode: 0o755,
            kind: b'5',
            uname: b"root".to_vec(),
            gname: b"root".to_vec(),
            ..TarEntry::default()
        };
        let long = "x".repeat(150);
        let f = TarEntry {
            name: format!("./a/{long}").into_bytes(),
            mode: 0o644,
            kind: b'0',
            size: 3,
            data: b"abc".to_vec(),
            ..TarEntry::default()
        };
        let bytes = write_tar(&[e, f]);
        assert_eq!(bytes.len() % 10240, 0);
        let back = parse_tar(&bytes).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[1].name.len(), 154);
        assert_eq!(back[1].data, b"abc");
    }

    #[test]
    fn listing_line() {
        let e = TarEntry {
            name: b"./".to_vec(),
            mode: 0o755,
            kind: b'5',
            uname: b"root".to_vec(),
            gname: b"root".to_vec(),
            mtime: 0,
            ..TarEntry::default()
        };
        let mut w = 19;
        assert_eq!(tv_line(&e, &mut w), "drwxr-xr-x root/root         0 1970-01-01 00:00 ./\n");
    }

    #[test]
    fn control_fields() {
        let f = parse_control("Package: foo\nDescription: a\n b\n\nPackage: bar\n").unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(f[1].value, "a\n b");
        assert_eq!(show_format("${Package;5}|${Version}\\n", &f), "  foo|\n");
    }
}
