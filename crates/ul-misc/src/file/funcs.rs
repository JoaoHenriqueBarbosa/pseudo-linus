// Porte para Rust do funcs.c (file_buffer), ascmagic.c, is_json.c, is_csv.c, is_tar.c e da parte
// embutida do compress.c do file 5.46.
//
// Copyright (c) Ian F. Darwin 1986-1995.
// Software written by Ian F. Darwin and others;
// maintained 1995-present by Christos Zoulas and others.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
// 1. Redistributions of source code must retain the above copyright
//    notice immediately at the beginning of the file, without modification,
//    this list of conditions, and the following disclaimer.
// 2. Redistributions in binary form must reproduce the above copyright
//    notice, this list of conditions and the following disclaimer in the
//    documentation and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
// ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
// ARE DISCLAIMED. IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE FOR
// ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
// OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
// HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
// LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
// OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
// SUCH DAMAGE.

//! `file_buffer()`: a sequência de testes sobre o conteúdo (compressão, tar, JSON, CSV, regras,
//! texto) e o resultado padrão.

use super::apprentice::{BINTEST, TEXTTEST};
use super::encoding::file_encoding;
use super::magic::*;
use super::softmagic::{Buffer, file_softmagic};

/// O `file_default()`.
fn file_default(ms: &mut MagicSet, nb: usize) -> i32 {
    if ms.flags & MAGIC_MIME != 0 {
        if ms.flags & MAGIC_MIME_TYPE != 0 {
            let t: &[u8] = if nb != 0 {
                b"application/octet-stream"
            } else {
                b"application/x-empty"
            };
            if ms.print(t).is_err() {
                return -1;
            }
        }
        return 1;
    }
    if ms.flags & MAGIC_APPLE != 0 {
        return if ms.print(b"UNKNUNKN").is_err() {
            -1
        } else {
            1
        };
    }
    if ms.flags & MAGIC_EXTENSION != 0 {
        return if ms.print(b"???").is_err() { -1 } else { 1 };
    }
    0
}

/// `checkdone()`: `true` encerra; com `-k` imprime o separador e segue.
fn checkdone(ms: &mut MagicSet, rv: &mut i32) -> bool {
    if ms.flags & MAGIC_CONTINUE == 0 {
        return true;
    }
    if ms.separator().is_err() {
        *rv = -1;
    }
    false
}

/// `file_buffer()`.
pub fn file_buffer(ms: &mut MagicSet, b: &Buffer<'_>, inname: Option<&[u8]>) -> i32 {
    let nb = b.fbuf.len();
    ms.mode = b.st_mode;
    let mut m = 0;
    let mut rv = 0;
    let mut looks_text = false;
    let mut code: Option<&'static str> = None;
    let mut code_mime = "binary";
    let mut def: &[u8] = b"data";

    enum Next {
        Simple,
        Done,
        DoneEncoding,
    }
    let next = 'tests: {
        if nb == 0 {
            def = b"empty";
            break 'tests Next::Simple;
        } else if nb == 1 {
            def = b"very short file (no magic)";
            break 'tests Next::Simple;
        }
        if ms.flags & MAGIC_NO_CHECK_ENCODING == 0 {
            let e = file_encoding(b.fbuf, ms.params.encoding_max);
            looks_text = e.text;
            code = Some(e.code);
            code_mime = e.code_mime;
        }
        if ms.flags & MAGIC_NO_CHECK_COMPRESS == 0 {
            m = file_zmagic(ms, b, inname);
            if m != 0 {
                break 'tests Next::DoneEncoding;
            }
        }
        if ms.flags & MAGIC_NO_CHECK_TAR == 0 {
            m = file_is_tar(ms, b.fbuf);
            if m != 0 && checkdone(ms, &mut rv) {
                break 'tests Next::Done;
            }
        }
        if ms.flags & MAGIC_NO_CHECK_JSON == 0 {
            m = file_is_json(ms, b.fbuf);
            if m != 0 && checkdone(ms, &mut rv) {
                break 'tests Next::Done;
            }
        }
        if ms.flags & MAGIC_NO_CHECK_CSV == 0 {
            m = file_is_csv(ms, b.fbuf, looks_text, code);
            if m != 0 && checkdone(ms, &mut rv) {
                break 'tests Next::Done;
            }
        }
        // O leitor de ELF roda antes das regras, num buffer à parte; o texto dele entra depois da
        // descrição que as regras derem (`#ifdef BUILTIN_ELF`).
        let mut rbuf: Option<Vec<u8>> = None;
        if ms.flags & MAGIC_NO_CHECK_ELF == 0 && nb > 5 && b.fd.is_some() {
            let Some(pb) = ms.push_buffer() else { return -1 };
            rv = super::readelf::file_tryelf(ms, b);
            rbuf = ms.pop_buffer(pb);
            if rv == -1 {
                rbuf = None;
            }
        }
        if ms.flags & MAGIC_NO_CHECK_SOFT == 0 {
            m = file_softmagic(ms, b, BINTEST, looks_text);
            if m != 0 {
                if m == 1 && let Some(r) = rbuf.as_deref() && ms.print(r).is_err() {
                    break 'tests Next::Done;
                }
                if checkdone(ms, &mut rv) {
                    break 'tests Next::Done;
                }
            }
        }
        if ms.flags & MAGIC_NO_CHECK_TEXT == 0 {
            m = file_ascmagic(ms, b, looks_text);
            if m != 0 {
                break 'tests Next::Done;
            }
        }
        Next::Simple
    };
    match next {
        Next::DoneEncoding => {
            return if rv != 0 { rv } else { m };
        }
        Next::Simple => {
            if m == 0 {
                m = 1;
                rv = file_default(ms, nb);
                if rv == 0 && ms.print(def).is_err() {
                    rv = -1;
                }
            }
        }
        Next::Done => {}
    }
    ms.trim_separator();
    if ms.flags & MAGIC_MIME_ENCODING != 0 {
        if ms.flags & MAGIC_MIME_TYPE != 0 && ms.print(b"; charset=").is_err() {
            rv = -1;
        }
        if ms.print(code_mime.as_bytes()).is_err() {
            rv = -1;
        }
    }
    if rv != 0 { rv } else { m }
}

// ---- texto ----

const MAXLINELEN: usize = 300;

fn trim_nuls(buf: &[u8]) -> usize {
    let mut n = buf.len();
    while n > 1 && buf[n - 1] == 0 {
        n -= 1;
    }
    n
}

/// `encode_utf8()`; `None` num ponto de código inválido.
fn encode_utf8(ubuf: &[u32]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(ubuf.len());
    for &c in ubuf {
        if c <= 0x7f {
            out.push(c as u8);
            continue;
        }
        let (lead, n): (u32, u32) = if c <= 0x7ff {
            (0xc0, 1)
        } else if c <= 0xffff {
            (0xe0, 2)
        } else if c <= 0x1f_ffff {
            (0xf0, 3)
        } else if c <= 0x3ff_ffff {
            (0xf8, 4)
        } else if c <= 0x7fff_ffff {
            (0xfc, 5)
        } else {
            return None;
        };
        out.push(((c >> (6 * n)) + lead) as u8);
        for k in (0..n).rev() {
            out.push((((c >> (6 * k)) & 0x3f) + 0x80) as u8);
        }
    }
    Some(out)
}

/// `file_ascmagic()`.
fn file_ascmagic(ms: &mut MagicSet, b: &Buffer<'_>, text: bool) -> i32 {
    let mut flen = trim_nuls(b.fbuf);
    if flen & 1 != 0 && b.fbuf.len() & 1 == 0 {
        flen += 1;
    }
    let buf = &b.fbuf[..flen.min(b.fbuf.len())];
    let e = file_encoding(buf, ms.params.encoding_max);
    if !e.text {
        return 0;
    }
    let bb = Buffer::new(buf, b.st_mode, b.st_size, b.fd);
    file_ascmagic_with_encoding(ms, &bb, &e.ubuf, e.code, e.kind, text)
}

fn file_ascmagic_with_encoding(
    ms: &mut MagicSet,
    b: &Buffer<'_>,
    ubuf: &[u32],
    code: &str,
    typ: &str,
    text: bool,
) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    let nbytes = trim_nuls(b.fbuf);
    if nbytes <= 1 {
        return 0;
    }
    let mut need_separator = false;
    let ulen = ubuf.len();
    if ulen > 0 && ms.flags & MAGIC_NO_CHECK_SOFT == 0 {
        let Some(utf8) = encode_utf8(ubuf) else {
            return 0;
        };
        let bb = Buffer::new(&utf8, b.st_mode, b.st_size, b.fd);
        let rv = file_softmagic(ms, &bb, TEXTTEST, text);
        if rv == 0 {
            if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
                return 0;
            }
        } else {
            need_separator = true;
            if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
                return i32::from(rv != -1);
            }
        }
    }
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    let mut seen_cr = false;
    let (mut n_crlf, mut n_lf, mut n_cr, mut n_nel) = (0usize, 0usize, 0usize, 0usize);
    let mut last_line_end: usize = usize::MAX;
    let mut has_long_lines = 0usize;
    let mut has_escapes = false;
    let mut has_backspace = false;
    for (i, &c) in ubuf.iter().enumerate() {
        if c == u32::from(b'\n') {
            if seen_cr {
                n_crlf += 1;
            } else {
                n_lf += 1;
            }
            last_line_end = i;
        } else if seen_cr {
            n_cr += 1;
        }
        seen_cr = c == u32::from(b'\r');
        if seen_cr {
            last_line_end = i;
        }
        if c == 0x85 {
            n_nel += 1;
            last_line_end = i;
        }
        if i > last_line_end.wrapping_add(MAXLINELEN) {
            let ll = i.wrapping_sub(last_line_end);
            if ll > has_long_lines {
                has_long_lines = ll;
            }
        }
        if c == 0x1b {
            has_escapes = true;
        }
        if c == 0x08 {
            has_backspace = true;
        }
    }
    if typ == "binary" {
        return 0;
    }
    let len = ms.printedlen();
    let mut executable = false;
    let res: Result<(), Fail> = (|| {
        if mime != 0 {
            if mime & MAGIC_MIME_TYPE != 0 {
                if len != 0 {
                    if ms.flags & MAGIC_CONTINUE == 0 {
                        return Ok(());
                    }
                    if need_separator {
                        ms.separator()?;
                    }
                }
                ms.print(b"text/plain")?;
            }
            return Ok(());
        }
        if len != 0 && ms.replace(b" text$", b", ")? == 0 {
            match ms.replace(b" text executable$", b", ")? {
                0 => ms.print(b", ")?,
                _ => executable = true,
            }
        }
        ms.print(code.as_bytes())?;
        ms.print(format!(" {typ}").as_bytes())?;
        if executable {
            ms.print(b" executable")?;
        }
        if has_long_lines != 0 {
            ms.print(format!(", with very long lines ({has_long_lines})").as_bytes())?;
        }
        if (n_crlf == 0 && n_cr == 0 && n_nel == 0 && n_lf == 0)
            || (n_crlf != 0 || n_cr != 0 || n_nel != 0)
        {
            ms.print(b", with")?;
            if n_crlf == 0 && n_cr == 0 && n_nel == 0 && n_lf == 0 {
                ms.print(b" no")?;
            } else {
                if n_crlf != 0 {
                    ms.print(b" CRLF")?;
                    if n_cr != 0 || n_lf != 0 || n_nel != 0 {
                        ms.print(b",")?;
                    }
                }
                if n_cr != 0 {
                    ms.print(b" CR")?;
                    if n_lf != 0 || n_nel != 0 {
                        ms.print(b",")?;
                    }
                }
                if n_lf != 0 {
                    ms.print(b" LF")?;
                    if n_nel != 0 {
                        ms.print(b",")?;
                    }
                }
                if n_nel != 0 {
                    ms.print(b" NEL")?;
                }
            }
            ms.print(b" line terminators")?;
        }
        if has_escapes {
            ms.print(b", with escape sequences")?;
        }
        if has_backspace {
            ms.print(b", with overstriking")?;
        }
        Ok(())
    })();
    if res.is_err() { -1 } else { 1 }
}

// ---- JSON ----

const JSON_ARRAY: usize = 0;
const JSON_CONSTANT: usize = 1;
const JSON_NUMBER: usize = 2;
const JSON_OBJECT: usize = 3;
const JSON_STRING: usize = 4;
const JSON_ARRAYN: usize = 5;

fn json_isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\n' | b'\r' | b'\t')
}

fn json_skip_space(b: &[u8], mut uc: usize) -> usize {
    while uc < b.len() && json_isspace(b[uc]) {
        uc += 1;
    }
    uc
}

fn json_parse_string(b: &[u8], ucp: &mut usize) -> bool {
    let ue = b.len();
    let mut uc = *ucp;
    while uc < ue {
        let c = b[uc];
        uc += 1;
        match c {
            0 => break,
            b'\\' => {
                if uc == ue {
                    break;
                }
                let e = b[uc];
                uc += 1;
                match e {
                    0 => break,
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => continue,
                    b'u' => {
                        if ue - uc < 4 {
                            uc = ue;
                            break;
                        }
                        let mut ok = true;
                        for _ in 0..4 {
                            let x = b[uc];
                            uc += 1;
                            if !x.is_ascii_hexdigit() {
                                ok = false;
                                break;
                            }
                        }
                        if !ok {
                            break;
                        }
                        continue;
                    }
                    _ => break,
                }
            }
            b'"' => {
                *ucp = uc;
                return true;
            }
            _ => continue,
        }
    }
    *ucp = uc;
    false
}

fn json_parse_array(b: &[u8], ucp: &mut usize, st: &mut [usize; 6], lvl: usize) -> bool {
    let ue = b.len();
    let mut uc = *ucp;
    while uc < ue {
        uc = json_skip_space(b, uc);
        if uc == ue {
            break;
        }
        if b[uc] == b']' {
            st[JSON_ARRAYN] += 1;
            *ucp = uc + 1;
            return true;
        }
        if !json_parse(b, &mut uc, st, lvl + 1) {
            break;
        }
        if uc == ue {
            break;
        }
        match b[uc] {
            b',' => {
                uc += 1;
                continue;
            }
            b']' => {
                st[JSON_ARRAYN] += 1;
                *ucp = uc + 1;
                return true;
            }
            _ => break,
        }
    }
    *ucp = uc;
    false
}

fn json_parse_object(b: &[u8], ucp: &mut usize, st: &mut [usize; 6], lvl: usize) -> bool {
    let ue = b.len();
    let mut uc = *ucp;
    while uc < ue {
        uc = json_skip_space(b, uc);
        if uc == ue {
            break;
        }
        if b[uc] == b'}' {
            *ucp = uc + 1;
            return true;
        }
        let c = b[uc];
        uc += 1;
        if c != b'"' {
            break;
        }
        if !json_parse_string(b, &mut uc) {
            break;
        }
        uc = json_skip_space(b, uc);
        if uc == ue {
            break;
        }
        let c = b[uc];
        uc += 1;
        if c != b':' {
            break;
        }
        if !json_parse(b, &mut uc, st, lvl + 1) {
            break;
        }
        if uc == ue {
            break;
        }
        let c = b[uc];
        uc += 1;
        match c {
            b',' => continue,
            b'}' => {
                *ucp = uc;
                return true;
            }
            _ => {
                *ucp = uc - 1;
                return false;
            }
        }
    }
    *ucp = uc;
    false
}

fn json_parse_number(b: &[u8], ucp: &mut usize) -> bool {
    let ue = b.len();
    let mut uc = *ucp;
    let mut got = false;
    if uc == ue {
        return false;
    }
    if b[uc] == b'-' {
        uc += 1;
    }
    'out: {
        while uc < ue && b[uc].is_ascii_digit() {
            got = true;
            uc += 1;
        }
        if uc == ue {
            break 'out;
        }
        if b[uc] == b'.' {
            uc += 1;
        }
        while uc < ue && b[uc].is_ascii_digit() {
            got = true;
            uc += 1;
        }
        if uc == ue {
            break 'out;
        }
        if got && (b[uc] == b'e' || b[uc] == b'E') {
            uc += 1;
            got = false;
            if uc == ue {
                break 'out;
            }
            if b[uc] == b'+' || b[uc] == b'-' {
                uc += 1;
            }
            while uc < ue && b[uc].is_ascii_digit() {
                got = true;
                uc += 1;
            }
        }
    }
    *ucp = uc;
    got
}

fn json_parse_const(b: &[u8], ucp: &mut usize, s: &[u8]) -> bool {
    // `str` com o NUL: len = strlen + 1; o primeiro caractere já foi consumido.
    let ue = b.len();
    let mut uc = *ucp;
    let mut len = s.len() + 1;
    len -= 1;
    *ucp = (*ucp + len - 1).min(ue);
    let mut si = 0;
    while uc < ue && {
        len -= 1;
        len > 0
    } {
        si += 1;
        let c = b[uc];
        uc += 1;
        if c != s.get(si).copied().unwrap_or(0) {
            return false;
        }
    }
    true
}

fn json_parse(b: &[u8], ucp: &mut usize, st: &mut [usize; 6], lvl: usize) -> bool {
    json_parse_top(b, ucp, st, lvl) != 0
}

/// `json_parse()` com o retorno inteiro do nível 0 (1 = JSON, 2 = vários por linha).
fn json_parse_top(b: &[u8], ucp: &mut usize, st: &mut [usize; 6], lvl: usize) -> i32 {
    let ue = b.len();
    let ouc = json_skip_space(b, *ucp);
    let mut uc = ouc;
    let mut rv = false;
    if uc != ue {
        if lvl > 500 {
            return 0;
        }
        let c = b[uc];
        uc += 1;
        let t;
        match c {
            b'"' => {
                rv = json_parse_string(b, &mut uc);
                t = JSON_STRING;
            }
            b'[' => {
                rv = json_parse_array(b, &mut uc, st, lvl + 1);
                t = JSON_ARRAY;
            }
            b'{' => {
                rv = json_parse_object(b, &mut uc, st, lvl + 1);
                t = JSON_OBJECT;
            }
            b't' => {
                rv = json_parse_const(b, &mut uc, b"true");
                t = JSON_CONSTANT;
            }
            b'f' => {
                rv = json_parse_const(b, &mut uc, b"false");
                t = JSON_CONSTANT;
            }
            b'n' => {
                rv = json_parse_const(b, &mut uc, b"null");
                t = JSON_CONSTANT;
            }
            _ => {
                uc -= 1;
                rv = json_parse_number(b, &mut uc);
                t = JSON_NUMBER;
            }
        }
        if rv {
            st[t] += 1;
        }
        uc = json_skip_space(b, uc);
    }
    *ucp = uc;
    if lvl == 0 {
        if !rv {
            return 0;
        }
        if uc == ue {
            return if st[JSON_ARRAYN] != 0 || st[JSON_OBJECT] != 0 {
                1
            } else {
                0
            };
        }
        let mut uc2 = uc;
        if b[ouc] == b[uc] && json_parse_top(b, &mut uc2, st, 1) != 0 {
            return if st[JSON_ARRAYN] != 0 || st[JSON_OBJECT] != 0 {
                2
            } else {
                0
            };
        }
        return 0;
    }
    i32::from(rv)
}

fn file_is_json(ms: &mut MagicSet, buf: &[u8]) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    let mut st = [0usize; 6];
    let mut uc = 0;
    let jt = json_parse_top(buf, &mut uc, &mut st, 0);
    if jt == 0 {
        return 0;
    }
    if mime == MAGIC_MIME_ENCODING {
        return 1;
    }
    let r = if mime != 0 {
        ms.print(if jt == 1 {
            b"application/json".as_slice()
        } else {
            b"application/x-ndjson"
        })
    } else {
        ms.print(if jt == 1 {
            b"JSON text data".as_slice()
        } else {
            b"New Line Delimited JSON text data"
        })
    };
    if r.is_err() { -1 } else { 1 }
}

// ---- CSV ----

fn eatquote(b: &[u8], mut uc: usize) -> usize {
    let mut quote = false;
    while uc < b.len() {
        let c = b[uc];
        uc += 1;
        if c != b'"' {
            if quote {
                return uc - 1;
            }
            continue;
        }
        if quote {
            quote = false;
            continue;
        }
        quote = true;
    }
    b.len()
}

fn csv_parse(b: &[u8]) -> bool {
    const CSV_LINES: usize = 10;
    let (mut nf, mut tf, mut nl) = (0usize, 0usize, 0usize);
    let mut uc = 0;
    while uc < b.len() {
        let c = b[uc];
        uc += 1;
        match c {
            b'"' => uc = eatquote(b, uc),
            b',' => nf += 1,
            b'\n' => {
                nl += 1;
                if nl == CSV_LINES {
                    return tf > 1 && tf == nf;
                }
                if tf == 0 {
                    if nf == 0 {
                        return false;
                    }
                    tf = nf;
                } else if tf != nf {
                    return false;
                }
                nf = 0;
            }
            _ => {}
        }
    }
    tf > 1 && nl >= 2
}

fn file_is_csv(ms: &mut MagicSet, buf: &[u8], looks_text: bool, code: Option<&str>) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    if !looks_text || ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 || !csv_parse(buf) {
        return 0;
    }
    if mime == MAGIC_MIME_ENCODING {
        return 1;
    }
    let r = if mime != 0 {
        ms.print(b"text/csv")
    } else {
        let c = code.unwrap_or("");
        let sp = if code.is_some() { " " } else { "" };
        ms.print(format!("CSV {c}{sp}text").as_bytes())
    };
    if r.is_err() { -1 } else { 1 }
}

// ---- tar ----

fn from_oct(w: &[u8]) -> i64 {
    let mut digs = w.len();
    if digs == 0 {
        return -1;
    }
    let mut i = 0;
    while i < w.len() && super::cutil::is_space(w[i]) {
        i += 1;
        if digs == 0 {
            return -1;
        }
        digs -= 1;
    }
    let mut value: i64 = 0;
    while digs > 0 && i < w.len() && (b'0'..=b'7').contains(&w[i]) {
        value = (value << 3) | i64::from(w[i] - b'0');
        i += 1;
        digs -= 1;
    }
    if digs > 0 && i < w.len() && w[i] != 0 && !super::cutil::is_space(w[i]) {
        return -1;
    }
    value
}

fn is_tar(buf: &[u8]) -> i32 {
    if buf.len() < 512 {
        return 0;
    }
    let h = &buf[..512];
    let name = &h[..100];
    const GPKG: &[u8] = b"/gpkg-1\0";
    if let Some(nul) = name.iter().position(|&c| c == 0)
        && nul + 1 >= GPKG.len()
        && &h[nul + 1 - GPKG.len()..nul + 1] == GPKG
    {
        return 0;
    }
    let chk = &h[148..156];
    let recsum = from_oct(chk);
    let mut sum: i64 = h.iter().map(|&c| i64::from(c)).sum();
    for &c in chk {
        sum -= i64::from(c);
    }
    sum += i64::from(b' ') * 8;
    if sum != recsum {
        return 0;
    }
    let magic = &h[257..263];
    if magic == b"ustar " {
        return 3;
    }
    if magic == b"ustar\0" {
        return 2;
    }
    1
}

fn file_is_tar(ms: &mut MagicSet, buf: &[u8]) -> i32 {
    const TARTYPE: [&str; 3] = [
        "tar archive",
        "POSIX tar archive",
        "POSIX tar archive (GNU)",
    ];
    let mime = ms.flags & MAGIC_MIME;
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    let tar = is_tar(buf);
    if !(1..=3).contains(&tar) {
        return 0;
    }
    if mime == MAGIC_MIME_ENCODING {
        return 1;
    }
    let t = if mime != 0 {
        "application/x-tar"
    } else {
        TARTYPE[(tar - 1) as usize]
    };
    if ms.print(t.as_bytes()).is_err() {
        -1
    } else {
        1
    }
}

// ---- compressão (-z) ----

enum Uncompressed {
    Ok(Vec<u8>),
    Err(String),
}

/// `uncompressgzipped()` + `uncompresszlib()` (inflate cru, até `bytes_max`).
fn uncompress_gzip(old: &[u8], bytes_max: usize) -> Uncompressed {
    const FHCRC: u8 = 1 << 1;
    const FEXTRA: u8 = 1 << 2;
    const FNAME: u8 = 1 << 3;
    const FCOMMENT: u8 = 1 << 4;
    let n = old.len();
    if n < 4 {
        return Uncompressed::Err("File too short".to_string());
    }
    let flg = old[3];
    let mut ds = 10usize;
    if flg & FEXTRA != 0 {
        if ds + 1 >= n {
            return Uncompressed::Err("File too short".to_string());
        }
        ds += 2 + usize::from(old[ds]) + usize::from(old[ds + 1]) * 256;
    }
    if flg & FNAME != 0 {
        while ds < n && old[ds] != 0 {
            ds += 1;
        }
        ds += 1;
    }
    if flg & FCOMMENT != 0 {
        while ds < n && old[ds] != 0 {
            ds += 1;
        }
        ds += 1;
    }
    if flg & FHCRC != 0 {
        ds += 2;
    }
    if ds >= n {
        return Uncompressed::Err("File too short".to_string());
    }
    match miniz_oxide::inflate::decompress_to_vec_with_limit(&old[ds..], bytes_max) {
        Ok(v) => Uncompressed::Ok(v),
        // Saída truncada no limite, ou um fluxo corrompido depois de já ter produzido dados: o
        // zlib com Z_SYNC_FLUSH entrega o que saiu.
        Err(e)
            if matches!(e.status, miniz_oxide::inflate::TINFLStatus::HasMoreOutput)
                || !e.output.is_empty() =>
        {
            Uncompressed::Ok(e.output)
        }
        Err(_) => Uncompressed::Err("invalid block type".to_string()),
    }
}

/// `uncompresszlib()` com cabeçalho zlib (o método 14).
fn uncompress_zlib(old: &[u8], bytes_max: usize) -> Uncompressed {
    match miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(old, bytes_max) {
        Ok(v) => Uncompressed::Ok(v),
        Err(e) if !e.output.is_empty() => Uncompressed::Ok(e.output),
        Err(_) => Uncompressed::Err("incorrect header check".to_string()),
    }
}

/// `filter_error()`, com a esquisitice do C: a mensagem lida do stderr é cortada no tamanho do
/// que tinha saído no stdout.
fn filter_error(stderr: &[u8], stdout: &[u8]) -> String {
    let n = stdout.len();
    let mut ubuf = stderr.to_vec();
    if n > ubuf.len() {
        ubuf.extend_from_slice(&stdout[ubuf.len()..n]);
    }
    ubuf.truncate(n);
    let buf = super::cutil::cstr(&ubuf);
    let start = buf
        .iter()
        .position(|&c| !super::cutil::is_space(c))
        .unwrap_or(buf.len());
    let mut s = &buf[start..];
    if let Some(p) = s.iter().position(|&c| c == b'\n') {
        s = &s[..p];
    }
    if let Some(p) = s.iter().position(|&c| c == b';') {
        s = &s[..p];
    }
    if let Some(p) = s.iter().rposition(|&c| c == b':') {
        s = &s[p + 1..];
        while let Some((&c, rest)) = s.split_first() {
            if !super::cutil::is_space(c) {
                break;
            }
            s = rest;
        }
    }
    let mut out = s.to_vec();
    if let Some(c) = out.first_mut()
        && c.is_ascii_lowercase()
    {
        *c = c.to_ascii_uppercase();
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// O caminho com `fork` do `uncompressbuf()`: o descompressor do sistema lendo o buffer no stdin.
fn uncompress_external(argv: &[&str], old: &[u8], bytes_max: usize) -> Uncompressed {
    use std::io::{Read, Write};
    use sysio::process::{Command, Stdio};
    let _ = crate::util::io::flush_stdout();
    let mut cmd = Command::new(argv[0]);
    cmd.args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let Ok(mut child) = cmd.spawn() else {
        return Uncompressed::Ok(Vec::new());
    };
    let stdin = child.stdin.take();
    let data = old.to_vec();
    let writer = sysio::thread::spawn(move || {
        if let Some(mut s) = stdin {
            let _ = s.write_all(&data);
        }
    });
    let mut out = Vec::new();
    if let Some(o) = child.stdout.take() {
        let _ = o.take(bytes_max as u64).read_to_end(&mut out);
    }
    if out.len() == bytes_max {
        let _ = child.kill();
        let _ = child.wait();
        let _ = writer.join();
        return Uncompressed::Ok(out);
    }
    let mut err = Vec::new();
    if let Some(e) = child.stderr.take() {
        let _ = e.take(bytes_max as u64).read_to_end(&mut err);
    }
    let _ = child.wait();
    let _ = writer.join();
    if !err.is_empty() {
        return Uncompressed::Err(filter_error(&err, &out));
    }
    Uncompressed::Ok(out)
}

#[derive(Clone, Copy)]
enum Method {
    Gzip,
    Zlib,
    External(&'static [&'static str]),
}

/// Uma linha da tabela `compr`: o teste da assinatura, o tamanho mínimo do buffer, como
/// descomprimir e o nome do método nas mensagens.
type Compr = (fn(&[u8]) -> bool, usize, Method, &'static str);

/// A tabela `compr`: assinatura (ou teste), tamanho mínimo e como descomprimir.
fn compr_table() -> [Compr; 15] {
    const GZIP: &[&str] = &["gzip", "-cd"];
    const UNCOMPRESS: &[&str] = &["uncompress", "-c"];
    const BZIP2: &[&str] = &["bzip2", "-cd"];
    const LZIP: &[&str] = &["lzip", "-cd"];
    const XZ: &[&str] = &["xz", "-cd"];
    const LRZIP: &[&str] = &["lrzip", "-qdf", "-"];
    const LZ4: &[&str] = &["lz4", "-cd"];
    const ZSTD: &[&str] = &["zstd", "-cd"];
    fn zlibcmp(b: &[u8]) -> bool {
        if (b[0] & 0xf) != 8 || (b[0] & 0x80) != 0 {
            return false;
        }
        let x = u16::from(b[1]) | (u16::from(b[0]) << 8);
        x % 31 == 0
    }
    fn lzmacmp(b: &[u8]) -> bool {
        b[0] == 0x5d && b[1] == 0 && b[2] == 0 && (b[12] == 0 || b[12] == 0xff)
    }
    [
        (
            |b| b.starts_with(b"\x1f\x9d"),
            2,
            Method::External(GZIP),
            "gzip",
        ),
        (
            |b| b.starts_with(b"\x1f\x9d"),
            2,
            Method::External(UNCOMPRESS),
            "uncompress",
        ),
        (|b| b.starts_with(b"\x1f\x8b"), 2, Method::Gzip, "zlib"),
        (
            |b| b.starts_with(b"\x1f\x9e"),
            2,
            Method::External(GZIP),
            "gzip",
        ),
        (
            |b| b.starts_with(b"\x1f\xa0"),
            2,
            Method::External(GZIP),
            "gzip",
        ),
        (
            |b| b.starts_with(b"\x1f\x1e"),
            2,
            Method::External(GZIP),
            "gzip",
        ),
        (
            |b| b.starts_with(b"PK\x03\x04"),
            4,
            Method::External(GZIP),
            "gzip",
        ),
        (
            |b| b.starts_with(b"BZh"),
            3,
            Method::External(BZIP2),
            "bzip2",
        ),
        (
            |b| b.starts_with(b"LZIP"),
            4,
            Method::External(LZIP),
            "lzip",
        ),
        (
            |b| b.starts_with(b"\xfd7zXZ\x00"),
            6,
            Method::External(XZ),
            "xz",
        ),
        (
            |b| b.starts_with(b"LRZI"),
            4,
            Method::External(LRZIP),
            "lrzip",
        ),
        (
            |b| b.starts_with(b"\x04\"M\x18"),
            4,
            Method::External(LZ4),
            "lz4",
        ),
        (
            |b| b.starts_with(b"\x28\xb5\x2f\xfd"),
            4,
            Method::External(ZSTD),
            "zstd",
        ),
        (lzmacmp, 13, Method::External(XZ), "xz"),
        (zlibcmp, 2, Method::Zlib, "zlib"),
    ]
}

/// `file_zmagic()`: o gzip e o zlib têm descompressor embutido; os outros formatos passam pelo
/// descompressor do sistema, como o file do Debian faz.
fn file_zmagic(ms: &mut MagicSet, b: &Buffer<'_>, name: Option<&[u8]>) -> i32 {
    if ms.flags & MAGIC_COMPRESS == 0 {
        return 0;
    }
    let mime = ms.flags & MAGIC_MIME;
    let buf = b.fbuf;
    let mut rv = 0;
    // Como o C: SIGPIPE ignorado enquanto um descompressor que morreu cedo pode deixar a escrita
    // no stdin dele sem leitor.
    let mut saved_pipe: Option<sysabi::SigDisposition> = None;
    for (test, maglen, method, mname) in compr_table() {
        if buf.len() < maglen || !test(buf) {
            continue;
        }
        if saved_pipe.is_none()
            && let Some(s) = sysabi::sys::try_current()
        {
            saved_pipe = s
                .sigaction(sysabi::Signal::SIGPIPE, sysabi::SigDisposition::Ignore)
                .ok();
        }
        let res = match method {
            Method::Gzip => uncompress_gzip(buf, ms.params.bytes_max),
            Method::Zlib => uncompress_zlib(buf, ms.params.bytes_max),
            Method::External(argv) => uncompress_external(argv, buf, ms.params.bytes_max),
        };
        ms.flags &= !MAGIC_COMPRESS;
        let r: Result<bool, ()> = (|| {
            let prv = match &res {
                Uncompressed::Err(msg) => {
                    if mime == 0 {
                        if ms
                            .print(format!("ERROR:[{mname}: {msg}]").as_bytes())
                            .is_err()
                        {
                            -1
                        } else {
                            0
                        }
                    } else {
                        let m: String = msg
                            .chars()
                            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                            .collect();
                        if ms
                            .print(
                                format!("application/x-decompression-error-{mname}-{m}").as_bytes(),
                            )
                            .is_err()
                        {
                            -1
                        } else {
                            0
                        }
                    }
                }
                Uncompressed::Ok(data) => {
                    let nb = Buffer::new(data, 0, 0, None);
                    file_buffer(ms, &nb, name)
                }
            };
            if prv == -1 {
                return Err(());
            }
            rv = 1;
            if ms.flags & MAGIC_COMPRESS_TRANSP != 0 {
                return Ok(true);
            }
            if mime != MAGIC_MIME && mime != 0 {
                return Ok(true);
            }
            ms.print(if mime != 0 {
                b" compressed-encoding=".as_slice()
            } else {
                b" ("
            })
            .map_err(|_| ())?;
            let pb = ms.push_buffer().ok_or(())?;
            let ob = Buffer::new(buf, 0, 0, None);
            if file_buffer(ms, &ob, None) == -1 {
                let _ = ms.pop_buffer(pb);
                return Err(());
            }
            if let Some(rbuf) = ms.pop_buffer(pb) {
                ms.print(super::cutil::cstr(&rbuf)).map_err(|_| ())?;
            }
            if mime == 0 {
                ms.print(b")").map_err(|_| ())?;
            }
            Ok(false)
        })();
        match r {
            Err(()) => rv = -1,
            Ok(true) => break,
            Ok(false) => {}
        }
    }
    if let Some(d) = saved_pipe
        && d != sysabi::SigDisposition::Ignore
        && let Some(s) = sysabi::sys::try_current()
    {
        let _ = s.sigaction(sysabi::Signal::SIGPIPE, d);
    }
    ms.flags |= MAGIC_COMPRESS;
    rv
}
