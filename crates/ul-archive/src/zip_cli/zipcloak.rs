//! `zipcloak` do Info-ZIP 3.0: cifra (ou, com `-d`, decifra) as entradas de um zip com a cifra
//! tradicional do PKZIP. A senha vem do terminal de controle (`/dev/tty`), nunca dos argumentos.

use super::ztools::{self, Archive, Entry, ZE_FORM, ZE_PARMS};
use crate::sysutil::{self, Output};

const HELP: &str = "\nZipCloak 3.0 (July 5th 2008)\n\
Usage:  zipcloak [-dq] [-b path] zipfile\n\
\x20 the default action is to encrypt all unencrypted entries in the zip file\n\
\n\
\x20 -d  --decrypt      decrypt encrypted entries (copy if given wrong password)\n\
\x20 -b  --temp-path    use \"path\" for the temporary zip file\n\
\x20 -O  --output-file  write output to new zip file\n\
\x20 -q  --quiet        quiet operation, suppress some informational messages\n\
\x20 -h  --help         show this help\n\
\x20 -v  --version      show version info\n\
\x20 -L  --license      show software license\n";

const VERSION: &str = "Copyright (c) 1990-2008 Info-ZIP - Type 'zipcloak \"-L\"' for software license.\n\
\n\
This is ZipCloak 3.0 (July 5th 2008), by Info-ZIP.\n\
Currently maintained by E. Gordon.  Please send bug reports to\n\
the authors using the web page at www.info-zip.org; see README for details.\n\
\n\
Latest sources and executables are at ftp://ftp.info-zip.org/pub/infozip,\n\
as of above date; see http://www.info-zip.org/ for other sites.\n\
\n\
Compiled with gcc 14.2.0 for Unix (Linux ELF).\n\
\n\
ZipCloak special compilation options:\n\
\t[encryption, version 2.91 of 05 Jan 2007]\n";

/// Tamanho do cabeçalho de criptografia que precede os dados de cada entrada.
const HEAD: usize = 12;

/// `ZE_ZIPNULL` do `ziperr.h`: zip ausente ou vazio.
const ZE_ZIPNULL: i32 = 13;

/// O `ziperr` do zipcloak escreve na saída padrão, não no erro padrão.
fn fail(code: i32, h: &str) -> i32 {
    put_stdout(&format!("zipcloak error: {} ({})\n", ztools::error_text(code), h));
    code
}

fn put_stdout(s: &str) {
    let mut o = Output::stdout();
    o.write_str(s);
    let _ = o.finish();
}

fn crc_step(c: u32, b: u8) -> u32 {
    let mut c = c ^ u32::from(b);
    for _ in 0..8 {
        c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
    }
    c
}

/// As três chaves da cifra tradicional.
struct Keys([u32; 3]);

impl Keys {
    fn new(pw: &[u8]) -> Keys {
        let mut k = Keys([305_419_896, 591_751_049, 878_082_192]);
        for &c in pw {
            k.update(c);
        }
        k
    }

    fn update(&mut self, c: u8) {
        self.0[0] = crc_step(self.0[0], c);
        self.0[1] = self.0[1].wrapping_add(self.0[0] & 0xff).wrapping_mul(134_775_813).wrapping_add(1);
        self.0[2] = crc_step(self.0[2], (self.0[1] >> 24) as u8);
    }

    fn stream_byte(&self) -> u8 {
        let t = (self.0[2] & 0xffff) | 2;
        ((t.wrapping_mul(t ^ 1) >> 8) & 0xff) as u8
    }

    fn encode(&mut self, c: u8) -> u8 {
        let t = self.stream_byte();
        self.update(c);
        t ^ c
    }

    fn decode(&mut self, c: u8) -> u8 {
        let p = c ^ self.stream_byte();
        self.update(p);
        p
    }
}

fn le16(d: &[u8], p: usize) -> usize {
    d[p] as usize | (d[p + 1] as usize) << 8
}

fn le32(d: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]])
}

/// Lê uma linha do terminal de controle, escrevendo o aviso antes. `None` sem terminal.
fn prompt(msg: &str) -> Option<Vec<u8>> {
    let fd = sysabi::sys::open(b"/dev/tty", sysabi::OFlags::RDWR, 0).ok()?;
    let _ = sysabi::sys::write_all(fd, msg.as_bytes());
    let mut line = Vec::new();
    let mut b = [0u8; 1];
    loop {
        match sysabi::sys::current().read(fd, &mut b) {
            Ok(1) if b[0] != b'\n' => line.push(b[0]),
            _ => break,
        }
    }
    let _ = sysabi::sys::write_all(fd, b"\n");
    let _ = sysabi::sys::close(fd);
    Some(line)
}

pub fn main(args: &[Vec<u8>]) -> i32 {
    let mut decrypt = false;
    let mut quiet = false;
    let mut output: Option<Vec<u8>> = None;
    let mut zipfile: Option<Vec<u8>> = None;
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if a.starts_with(b"--") && a.len() > 2 {
            match &a[2..] {
                b"help" => {
                    put_stdout(HELP);
                    return 0;
                }
                b"version" => {
                    put_stdout(VERSION);
                    return 0;
                }
                b"decrypt" => decrypt = true,
                b"quiet" => quiet = true,
                b"temp-path" | b"output-file" => {
                    let is_out = &a[2..] == b"output-file";
                    i += 1;
                    if i >= args.len() {
                        return fail(ZE_PARMS, &format!("option '{}' requires a value", String::from_utf8_lossy(a)));
                    }
                    if is_out {
                        output = Some(args[i].clone());
                    }
                }
                _ => return fail(ZE_PARMS, &format!("long option '{}' not supported", String::from_utf8_lossy(&a[2..]))),
            }
        } else if a.len() > 1 && a[0] == b'-' {
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
                    b'd' => decrypt = true,
                    b'q' => quiet = true,
                    b'b' | b'O' => {
                        let is_out = a[k] == b'O';
                        // O argumento vem colado ou no próximo parâmetro. O diretório do temporário
                        // é só consumido: a escrita aqui é direta.
                        let value = if k + 1 < a.len() {
                            let v = a[k + 1..].to_vec();
                            k = a.len();
                            v
                        } else {
                            i += 1;
                            if i >= args.len() {
                                return fail(ZE_PARMS, &format!("option '{}' requires a value", a[k] as char));
                            }
                            args[i].clone()
                        };
                        if is_out {
                            output = Some(value);
                        }
                        if k >= a.len() {
                            break;
                        }
                    }
                    c => return fail(ZE_PARMS, &format!("short option '{}' not supported", c as char)),
                }
                k += 1;
            }
        } else if zipfile.is_none() {
            zipfile = Some(a.clone());
        } else {
            return fail(ZE_PARMS, "Use option -h for help.");
        }
        i += 1;
    }
    let Some(zipfile) = zipfile else {
        put_stdout(HELP);
        return 0;
    };
    let name = String::from_utf8_lossy(&zipfile).into_owned();
    let data = match sysutil::read_path(&zipfile) {
        Ok(d) if !d.is_empty() => d,
        _ => return fail(ZE_ZIPNULL, &name),
    };
    let arc = match ztools::parse(&data) {
        Ok(a) => a,
        Err(c) => {
            let tail = &data[data.len().saturating_sub(65_557)..];
            if !tail.windows(4).any(|w| w == b"PK\x05\x06") {
                put_stdout(
                    "zipcloak warning: missing end signature--probably not a zip file (did you\n\
                     zipcloak warning: remember to use binary mode when you transferred it?)\n\
                     zipcloak warning: (if you are trying to read a damaged archive try -F)\n",
                );
            }
            return fail(c, &name);
        }
    };
    let target = output.unwrap_or_else(|| zipfile.clone());
    run(&target, &data, arc, decrypt, quiet)
}

/// A senha para a operação: na cifragem, pedida duas vezes e conferida.
fn ask_password(decrypt: bool) -> Result<Vec<u8>, i32> {
    // Sem terminal de controle, o original recusa antes de pedir a senha.
    let no_tty = "stderr is not a tty (you may never see this message!)";
    let Some(pw) = prompt("Enter password: ") else {
        return Err(fail(ZE_PARMS, no_tty));
    };
    if pw.is_empty() {
        return Err(fail(ZE_PARMS, "zero length password not allowed"));
    }
    if !decrypt {
        let Some(again) = prompt("Verify password: ") else {
            return Err(fail(ZE_PARMS, no_tty));
        };
        if again != pw {
            return Err(fail(ZE_PARMS, "password verification failed"));
        }
    }
    Ok(pw)
}

/// Cabeçalho local de `e` já com os dados e o descritor, devolvido como bytes crus do arquivo.
struct Raw<'a> {
    local: &'a [u8],
    data_start: usize,
}

fn split_local<'a>(data: &'a [u8], e: &Entry) -> Result<Raw<'a>, i32> {
    let len = ztools::local_len(data, e)?;
    let o = e.off as usize;
    let data_start = 30 + le16(data, o + 26) + le16(data, o + 28);
    Ok(Raw { local: &data[o..o + len], data_start })
}

fn run(zipfile: &[u8], data: &[u8], arc: Archive, decrypt: bool, quiet: bool) -> i32 {
    let wanted = |e: &Entry| (e.flg & 1 != 0) == decrypt;
    let password = if arc.entries.iter().any(wanted) {
        match ask_password(decrypt) {
            Ok(p) => Some(p),
            Err(code) => return code,
        }
    } else {
        None
    };

    let mut out = Vec::new();
    let first = arc.entries.iter().map(|e| e.off as usize).min().unwrap_or(0);
    out.extend_from_slice(&data[..first.min(data.len())]);
    let mut cds = Vec::new();
    for e in &arc.entries {
        let off = out.len() as u64;
        let name = String::from_utf8_lossy(&e.name).into_owned();
        let Ok(raw) = split_local(data, e) else {
            return fail(ZE_FORM, &String::from_utf8_lossy(zipfile));
        };
        let mut cen = e.cen.clone();
        if !wanted(e) {
            if !quiet {
                put_stdout(&format!("skipping: {}  {}\n", name, if decrypt { "not encrypted" } else { "already encrypted" }));
            }
            out.extend_from_slice(raw.local);
            cds.push(ztools::central(e, off, &e.name, &e.comment));
            continue;
        }
        let pw = password.as_deref().unwrap_or_default();
        let csize = e.csize as usize;
        let body = &raw.local[raw.data_start..raw.data_start + csize.min(raw.local.len() - raw.data_start)];
        let tail = &raw.local[raw.data_start + body.len()..];
        let mut hdr = raw.local[..raw.data_start].to_vec();
        let mut new_body = Vec::with_capacity(body.len() + HEAD);
        let new_csize: u32;
        if decrypt {
            if body.len() < HEAD {
                return fail(ZE_FORM, &String::from_utf8_lossy(zipfile));
            }
            let mut k = Keys::new(pw);
            let plain: Vec<u8> = body.iter().map(|&c| k.decode(c)).collect();
            let check = if e.flg & 8 != 0 { (le16(&e.cen, 12) >> 8) as u8 } else { (le32(&e.cen, 16) >> 24) as u8 };
            if plain[HEAD - 1] != check {
                return fail(ZE_PARMS, "incorrect password");
            }
            new_body.extend_from_slice(&plain[HEAD..]);
            new_csize = (csize - HEAD) as u32;
            if !quiet {
                put_stdout(&format!("decrypting: {}\n", name));
            }
        } else {
            let crc = le32(&e.cen, 16);
            // Os dez primeiros bytes do cabeçalho são ruído; o original usa `rand()` semeado pelo relógio.
            let mut seed = crc32fast::hash(&[pw, &e.name].concat());
            let mut k = Keys::new(pw);
            for _ in 0..HEAD - 2 {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                new_body.push(k.encode((seed >> 16) as u8));
            }
            let check = if e.flg & 8 != 0 { (le16(&e.cen, 12) >> 8) as u8 } else { (crc >> 24) as u8 };
            new_body.push(k.encode((crc >> 16) as u8));
            new_body.push(k.encode(check));
            for &c in body {
                new_body.push(k.encode(c));
            }
            new_csize = (csize + HEAD) as u32;
            if !quiet {
                put_stdout(&format!("encrypting: {}\n", name));
            }
        }
        // Cabeçalho local: bit de cifra e tamanho comprimido (zerado quando o descritor o carrega).
        let flg = le16(&hdr, 6) as u16;
        let nflg = if decrypt { flg & !1 } else { flg | 1 };
        hdr[6..8].copy_from_slice(&nflg.to_le_bytes());
        if e.flg & 8 == 0 || le32(&hdr, 18) != 0 {
            hdr[18..22].copy_from_slice(&new_csize.to_le_bytes());
        }
        let cflg = le16(&cen, 8) as u16;
        let ncflg = if decrypt { cflg & !1 } else { cflg | 1 };
        cen[8..10].copy_from_slice(&ncflg.to_le_bytes());
        cen[20..24].copy_from_slice(&new_csize.to_le_bytes());
        let mut tail = tail.to_vec();
        if e.flg & 8 != 0 && tail.len() >= 12 {
            // Descritor de dados: o campo do tamanho comprimido fica depois do crc.
            let p = if tail.len() == 16 { 8 } else { 4 };
            tail[p..p + 4].copy_from_slice(&new_csize.to_le_bytes());
        }
        out.extend_from_slice(&hdr);
        out.extend_from_slice(&new_body);
        out.extend_from_slice(&tail);
        let ne = Entry { cen, flg: nflg, csize: u64::from(new_csize), ..e.clone() };
        cds.push(ztools::central(&ne, off, &e.name, &e.comment));
    }
    let cd_off = out.len();
    let mut size = 0;
    for c in &cds {
        size += c.len();
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&ztools::eocd(arc.entries.len(), size, cd_off, &arc.comment));
    if sysutil::write_file(zipfile, &out, 0o666).is_err() {
        return fail(ztools::ZE_WRITE, &String::from_utf8_lossy(zipfile));
    }
    0
}
