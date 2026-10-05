//! `funzip` do Info-ZIP UnZip 6.0 (filtro): descompacta o primeiro membro de um zip ou gzip, vindo
//! do arquivo dado ou da entrada padrão, para a saída padrão. Entende armazenado e deflate, e a
//! cifra tradicional (`-senha`).

use std::io::{Cursor, Read};

use sysabi::sys;
use sysabi::Fd;

use crate::sysutil::{self, Output};

fn err(code: i32, msg: &str) -> i32 {
    sysutil::eprint(format!("funzip error: {}\n", msg));
    code
}

/// As chaves da cifra tradicional.
struct Keys([u32; 3]);

fn crc_step(c: u32, b: u8) -> u32 {
    let mut c = c ^ u32::from(b);
    for _ in 0..8 {
        c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
    }
    c
}

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

    fn decode(&mut self, c: &mut u8) {
        let t = (self.0[2] & 0xffff) | 2;
        *c ^= ((t.wrapping_mul(t ^ 1) >> 8) & 0xff) as u8;
        self.update(*c);
    }
}

fn le16(d: &[u8], p: usize) -> usize {
    d[p] as usize | (d[p + 1] as usize) << 8
}

fn le32(d: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]])
}

/// Inflate cru: devolve o que saiu, quanto da entrada foi consumido e se houve erro.
fn inflate(data: &[u8]) -> (Vec<u8>, usize, bool) {
    let mut cur = Cursor::new(data);
    let mut out = Vec::new();
    let bad = {
        let mut dec = flate2::bufread::DeflateDecoder::new(&mut cur);
        dec.read_to_end(&mut out).is_err()
    };
    (out, cur.position() as usize, bad)
}

fn emit(data: &[u8]) {
    let mut o = Output::stdout();
    o.write(data);
    let _ = o.finish();
}

pub fn main(args: &[Vec<u8>]) -> i32 {
    let mut rest = &args[1.min(args.len())..];
    let mut key: Option<Vec<u8>> = None;
    if let Some(a) = rest.first()
        && a.first() == Some(&b'-')
    {
        key = Some(a[1..].to_vec());
        rest = &rest[1..];
    }
    let data = if let Some(f) = rest.first() {
        match sysutil::read_path(f) {
            Ok(d) => d,
            Err(_) => return err(2, "cannot find input file"),
        }
    } else {
        if sys::current().isatty(Fd::STDIN) {
            sysutil::eprint(
                "fUnZip (filter UnZip), version 3.95 of 20 January 2008, by Info-ZIP.\nUsage: funzip [-password] [input[.zip|.gz]]\n",
            );
            return 3;
        }
        sys::read_to_end(Fd::STDIN).unwrap_or_default()
    };

    if data.len() >= 30 && &data[..4] == b"PK\x03\x04" {
        zip_member(&data, key)
    } else if data.len() >= 10 && data[0] == 0x1f && data[1] == 0x8b {
        gzip_member(&data)
    } else {
        err(3, "input not a zip or gzip file")
    }
}

fn zip_member(data: &[u8], key: Option<Vec<u8>>) -> i32 {
    let flg = le16(data, 6);
    let how = le16(data, 8);
    let crc = le32(data, 14);
    let csize = le32(data, 18) as usize;
    let start = 30 + le16(data, 26) + le16(data, 28);
    if start > data.len() {
        return err(2, "unexpected end of file");
    }
    if how != 0 && how != 8 {
        return err(3, "first entry not deflated or stored -- use unzip");
    }
    let mut body: Vec<u8> = data[start..].to_vec();
    let mut limit = if flg & 8 != 0 && csize == 0 { body.len() } else { csize.min(body.len()) };
    if flg & 1 != 0 {
        let Some(pw) = key.filter(|k| !k.is_empty()) else {
            return err(3, "need password");
        };
        if body.len() < 12 {
            return err(2, "unexpected end of file");
        }
        let mut k = Keys::new(&pw);
        for c in body.iter_mut() {
            k.decode(c);
        }
        let check = if flg & 8 != 0 { data[11] } else { (crc >> 24) as u8 };
        if body[11] != check {
            return err(3, "incorrect password");
        }
        body.drain(..12);
        limit = limit.saturating_sub(12).min(body.len());
    }
    if how == 0 {
        emit(&body[..limit]);
        return 0;
    }
    let (out, _used, bad) = inflate(&body);
    emit(&out);
    if bad {
        return err(3, "invalid compressed data--format violated");
    }
    if flg & 8 == 0 && crc32fast::hash(&out) != crc {
        return err(1, "invalid compressed data--crc error");
    }
    0
}

fn gzip_member(data: &[u8]) -> i32 {
    if data[2] != 8 {
        return err(3, "unknown compression method");
    }
    let fl = data[3];
    let mut p = 10;
    let need = |p: usize| -> bool { p <= data.len() };
    if fl & 4 != 0 {
        if !need(p + 2) {
            return err(2, "unexpected end of file");
        }
        p += 2 + le16(data, p);
    }
    for bit in [8u8, 16] {
        if fl & bit != 0 {
            while p < data.len() && data[p] != 0 {
                p += 1;
            }
            p += 1;
        }
    }
    if fl & 2 != 0 {
        p += 2;
    }
    if !need(p) {
        return err(2, "unexpected end of file");
    }
    let (out, used, bad) = inflate(&data[p..]);
    emit(&out);
    if bad {
        return err(3, "invalid compressed data--format violated");
    }
    let t = p + used;
    if t + 8 > data.len() {
        return err(2, "unexpected end of file");
    }
    if le32(data, t) != crc32fast::hash(&out) {
        return err(1, "invalid compressed data--crc error");
    }
    if le32(data, t + 4) != out.len() as u32 {
        return err(1, "invalid compressed data--length error");
    }
    0
}
