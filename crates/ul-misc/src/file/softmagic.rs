// Porte para Rust do softmagic.c do file 5.46.
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

//! O motor das regras (`softmagic.c`): percorre o banco, lê os valores no buffer (com offsets
//! indiretos, `use`/`name`, `indirect`), compara e imprime as descrições com as continuações.
//! Os retornos seguem o C: -1 erro, 0 não casou, 1 (ou mais) casou.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::apprentice::*;
use super::cfmt::{self, Arg};
use super::cutil::{at, cstr, is_lower, is_space, is_upper, strtoull};
use super::magic::{
    self, MAGIC_APPLE, MAGIC_CONTINUE, MAGIC_EXTENSION, MAGIC_MIME_TYPE, MAGIC_NODESC, MagicList,
    MagicSet,
};
use super::regex::Regex;

/// O `struct buffer`: o começo do arquivo e, sob demanda, o fim dele (pros offsets negativos).
pub struct Buffer<'a> {
    pub fbuf: &'a [u8],
    pub st_mode: u32,
    pub st_size: u64,
    /// Pra ler o fim do arquivo (`buffer_fill`); `None` num buffer derivado.
    pub fd: Option<sysabi::Fd>,
    ebuf: RefCell<Option<Option<Rc<Vec<u8>>>>>,
}

impl<'a> Buffer<'a> {
    pub fn new(fbuf: &'a [u8], st_mode: u32, st_size: u64, fd: Option<sysabi::Fd>) -> Buffer<'a> {
        Buffer {
            fbuf,
            st_mode,
            st_size,
            fd,
            ebuf: RefCell::new(None),
        }
    }

    /// `buffer_fill()`: os últimos `min(st_size, flen)` bytes; `None` se não dá (não regular).
    fn fill(&self) -> Option<Rc<Vec<u8>>> {
        if let Some(v) = self.ebuf.borrow().as_ref() {
            return v.clone();
        }
        let r = self.fill_now();
        *self.ebuf.borrow_mut() = Some(r.clone());
        r
    }

    fn fill_now(&self) -> Option<Rc<Vec<u8>>> {
        if self.st_mode & 0o170000 != 0o100000 {
            return None;
        }
        let elen = (self.st_size as usize).min(self.fbuf.len());
        if elen == 0 {
            return Some(Rc::new(Vec::new()));
        }
        let eoff = self.st_size - elen as u64;
        if eoff == 0 {
            return Some(Rc::new(self.fbuf[..elen].to_vec()));
        }
        let fd = self.fd?;
        let sys = sysabi::sys::try_current()?;
        let mut buf = vec![0u8; elen];
        let mut got = 0;
        while got < elen {
            match sys.pread(fd, &mut buf[got..], eoff + got as u64) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(sysabi::Errno::EINTR) => {}
                Err(_) => return None,
            }
        }
        Some(Rc::new(buf))
    }
}

/// O `bb` do `match()`: o começo do arquivo ou o fim lido pelo `buffer_fill`.
#[derive(Clone)]
enum View {
    Front,
    End(Rc<Vec<u8>>),
}

impl View {
    fn bytes<'b>(&'b self, b: &'b Buffer<'_>) -> &'b [u8] {
        match self {
            View::Front => b.fbuf,
            View::End(v) => v,
        }
    }
}

/// Estado que o C passa por ponteiro entre as chamadas recursivas.
pub struct State {
    pub printed_something: bool,
    pub need_separator: bool,
    pub firstline: bool,
}

// ---- o valor lido (`union VALUETYPE`) ----

fn vb(ms: &MagicSet) -> u8 {
    ms.ms_value[0]
}
fn vh(ms: &MagicSet) -> u16 {
    u16::from_le_bytes([ms.ms_value[0], ms.ms_value[1]])
}
fn vl(ms: &MagicSet) -> u32 {
    u32::from_le_bytes(ms.ms_value[..4].try_into().unwrap_or([0; 4]))
}
fn vq(ms: &MagicSet) -> u64 {
    u64::from_le_bytes(ms.ms_value[..8].try_into().unwrap_or([0; 8]))
}
fn set_vb(ms: &mut MagicSet, v: u8) {
    ms.ms_value[0] = v;
}
fn set_vh(ms: &mut MagicSet, v: u16) {
    ms.ms_value[..2].copy_from_slice(&v.to_le_bytes());
}
fn set_vl(ms: &mut MagicSet, v: u32) {
    ms.ms_value[..4].copy_from_slice(&v.to_le_bytes());
}
fn set_vq(ms: &mut MagicSet, v: u64) {
    ms.ms_value[..8].copy_from_slice(&v.to_le_bytes());
}
fn be16(p: &[u8]) -> u16 {
    (u16::from(at(p, 0)) << 8) | u16::from(at(p, 1))
}
fn le16(p: &[u8]) -> u16 {
    (u16::from(at(p, 1)) << 8) | u16::from(at(p, 0))
}
fn be32(p: &[u8]) -> u32 {
    (u32::from(at(p, 0)) << 24)
        | (u32::from(at(p, 1)) << 16)
        | (u32::from(at(p, 2)) << 8)
        | u32::from(at(p, 3))
}
fn le32(p: &[u8]) -> u32 {
    (u32::from(at(p, 3)) << 24)
        | (u32::from(at(p, 2)) << 16)
        | (u32::from(at(p, 1)) << 8)
        | u32::from(at(p, 0))
}
fn me32(p: &[u8]) -> u32 {
    (u32::from(at(p, 1)) << 24)
        | (u32::from(at(p, 0)) << 16)
        | (u32::from(at(p, 3)) << 8)
        | u32::from(at(p, 2))
}
fn be64(p: &[u8]) -> u64 {
    (0..8).fold(0u64, |acc, i| (acc << 8) | u64::from(at(p, i)))
}
fn le64(p: &[u8]) -> u64 {
    (0..8)
        .rev()
        .fold(0u64, |acc, i| (acc << 8) | u64::from(at(p, i)))
}

/// `SEXT(sgn, bits, v)`.
fn sext(sgn: bool, bits: u32, v: u64) -> i64 {
    match (sgn, bits) {
        (true, 8) => i64::from(v as i8),
        (true, 16) => i64::from(v as i16),
        (true, 32) => i64::from(v as i32),
        (true, _) => v as i64,
        (false, 8) => i64::from(v as u8),
        (false, 16) => i64::from(v as u16),
        (false, 32) => i64::from(v as u32),
        (false, _) => v as i64,
    }
}

/// `OFFSET_OOB(n, o, i)`.
fn oob(n: usize, o: i64, i: usize) -> bool {
    (n as u64) < u64::from(o as u32) || (i as u64) > (n as u64).wrapping_sub(o as u64)
}

// ---- entrada ----

/// `file_softmagic()`.
pub fn file_softmagic(ms: &mut MagicSet, b: &Buffer<'_>, mode: u16, text: bool) -> i32 {
    let mut ic = 0u16;
    let mut nc = 0u16;
    let mut st = State {
        printed_something: false,
        need_separator: false,
        firstline: true,
    };
    let mut rv = 0;
    let dbs: Vec<Arc<magic::Db>> = ms.mlist.clone();
    for db in &dbs {
        let mut returnval = 0;
        let mut found = 0;
        let ret = do_match(
            ms,
            db.list(0),
            b,
            0,
            mode,
            text,
            false,
            &mut ic,
            &mut nc,
            &mut st,
            &mut returnval,
            &mut found,
        );
        match ret {
            -1 => return ret,
            0 => continue,
            _ => {
                if ms.flags & MAGIC_CONTINUE == 0 {
                    return ret;
                }
                rv = ret;
            }
        }
    }
    rv
}

fn skip_subtests(magic: &[Magic], magindex: &mut usize) {
    while *magindex + 1 < magic.len() && magic[*magindex + 1].cont_level != 0 {
        *magindex += 1;
    }
}

/// `match()`.
#[allow(clippy::too_many_arguments)]
pub fn do_match(
    ms: &mut MagicSet,
    list: MagicList<'_>,
    b: &Buffer<'_>,
    offset: usize,
    mode: u16,
    text: bool,
    flip: bool,
    ic: &mut u16,
    nc: &mut u16,
    st: &mut State,
    returnval: &mut i32,
    found_match: &mut i32,
) -> i32 {
    let magic = list.magic;
    let n = magic.len();
    let print = ms.flags & MAGIC_NODESC == 0;
    let mut cont_level: usize = 0;
    let mut bb = View::Front;
    ms.check_mem(cont_level);
    let mut magindex = 0usize;
    const FLT: u32 = STRING_BINTEST | STRING_TEXTTEST;
    while magindex < n {
        sysabi::sys::checkpoint();
        let flushed: bool = 'body: {
            let m = &magic[magindex];
            if m.typ != FILE_NAME
                && ((is_string_type(m.typ)
                    && ((text && (m.str_flags() & FLT) == STRING_BINTEST)
                        || (!text && (m.str_flags() & FLT) == STRING_TEXTTEST)))
                    || (m.flag & mode) != mode)
            {
                break 'body true;
            }
            match msetoffset(ms, m, b, offset, cont_level) {
                Err(()) => break 'body true,
                Ok(Some(v)) => bb = v,
                Ok(None) => {}
            }
            ms.line = m.lineno;
            let s_owned = bb.clone();
            let s = s_owned.bytes(b);
            let flush;
            match mget(
                ms,
                list,
                magindex,
                b,
                s,
                offset,
                cont_level,
                mode,
                text,
                flip,
                ic,
                nc,
                st,
                returnval,
                found_match,
            ) {
                -1 => return -1,
                0 => flush = m.reln != b'!',
                _ => {
                    if m.typ == FILE_INDIRECT {
                        *found_match = 1;
                        *returnval = 1;
                    }
                    match magiccheck(ms, list, magindex, s) {
                        -1 => return -1,
                        0 => flush = true,
                        _ => flush = false,
                    }
                }
            }
            if flush {
                break 'body true;
            }
            let e = handle_annotation(ms, m, st.firstline);
            if e != 0 {
                *found_match = 1;
                st.need_separator = true;
                st.printed_something = true;
                *returnval = 1;
                st.firstline = false;
                return e;
            }
            if m.desc[0] != 0 {
                *found_match = 1;
                if print {
                    *returnval = 1;
                    st.need_separator = true;
                    st.printed_something = true;
                    if print_sep(ms, st.firstline).is_err() || mprint(ms, m, s).is_err() {
                        return -1;
                    }
                }
            }
            match moffset(ms, m, s.len(), offset) {
                Some(o) => ms.li[cont_level].off = o,
                None => break 'body true,
            }
            cont_level += 1;
            ms.check_mem(cont_level);
            while magindex + 1 < n && magic[magindex + 1].cont_level != 0 {
                magindex += 1;
                let m = &magic[magindex];
                ms.line = m.lineno;
                let lvl = usize::from(m.cont_level);
                if cont_level < lvl {
                    continue;
                }
                if cont_level > lvl {
                    cont_level = lvl;
                }
                match msetoffset(ms, m, b, offset, cont_level) {
                    Err(()) => break 'body true,
                    Ok(Some(v)) => bb = v,
                    Ok(None) => {}
                }
                if m.flag & OFFADD != 0 {
                    if cont_level == 0 {
                        return 0;
                    }
                    ms.offset = ms.offset.wrapping_add(ms.li[cont_level - 1].off);
                }
                if (m.cond == COND_ELSE || m.cond == COND_ELIF) && ms.li[cont_level].last_match {
                    continue;
                }
                let s_owned = bb.clone();
                let s = s_owned.bytes(b);
                let flush = match mget(
                    ms,
                    list,
                    magindex,
                    b,
                    s,
                    offset,
                    cont_level,
                    mode,
                    text,
                    flip,
                    ic,
                    nc,
                    st,
                    returnval,
                    found_match,
                ) {
                    -1 => return -1,
                    0 => {
                        if m.reln != b'!' {
                            continue;
                        }
                        true
                    }
                    _ => {
                        if m.typ == FILE_INDIRECT {
                            *found_match = 1;
                            *returnval = 1;
                        }
                        false
                    }
                };
                let r = if flush {
                    1
                } else {
                    magiccheck(ms, list, magindex, s)
                };
                match r {
                    -1 => return -1,
                    0 => ms.li[cont_level].last_match = false,
                    _ => {
                        ms.li[cont_level].last_match = true;
                        if m.typ == FILE_CLEAR {
                            ms.li[cont_level].got_match = false;
                        } else if ms.li[cont_level].got_match {
                            if m.typ == FILE_DEFAULT {
                                continue;
                            }
                        } else {
                            ms.li[cont_level].got_match = true;
                        }
                        let e = handle_annotation(ms, m, st.firstline);
                        if e != 0 {
                            *found_match = 1;
                            st.need_separator = true;
                            st.printed_something = true;
                            *returnval = 1;
                            return e;
                        }
                        if m.desc[0] != 0 {
                            *found_match = 1;
                        }
                        if print && m.desc[0] != 0 {
                            *returnval = 1;
                            if !st.printed_something {
                                st.printed_something = true;
                                if print_sep(ms, st.firstline).is_err() {
                                    return -1;
                                }
                            }
                            if st.need_separator && m.flag & NOSPACE == 0 && ms.print(b" ").is_err()
                            {
                                return -1;
                            }
                            if mprint(ms, m, s).is_err() {
                                return -1;
                            }
                            st.need_separator = true;
                        }
                        match moffset(ms, m, s.len(), offset) {
                            Some(o) => ms.li[cont_level].off = o,
                            None => cont_level = cont_level.wrapping_sub(1),
                        }
                        cont_level = cont_level.wrapping_add(1);
                        ms.check_mem(cont_level);
                    }
                }
            }
            if st.printed_something {
                st.firstline = false;
            }
            if *found_match != 0 {
                if ms.flags & MAGIC_CONTINUE == 0 {
                    return *returnval;
                }
                st.printed_something = false;
                st.firstline = false;
            }
            cont_level = 0;
            false
        };
        if flushed {
            skip_subtests(magic, &mut magindex);
            cont_level = 0;
        }
        magindex += 1;
    }
    *returnval
}

// ---- impressão ----

/// `check_fmt()`: o formato pede `%s`?
fn check_fmt(desc: &[u8]) -> bool {
    // "%[-0-9\.]*s"
    let mut i = 0;
    while i < desc.len() {
        if desc[i] == b'%' {
            let mut j = i + 1;
            while j < desc.len() && (desc[j] == b'-' || desc[j].is_ascii_digit() || desc[j] == b'.')
            {
                j += 1;
            }
            if j < desc.len() && desc[j] == b's' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// `varexpand()`: `${x?sim:não}` pelo bit de execução do arquivo.
fn varexpand(ms: &MagicSet, s: &[u8], len: usize) -> Option<Vec<u8>> {
    let s = cstr(s);
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = find(&s[i..], b"${").map(|p| p + i) {
        out.extend_from_slice(&s[i..p]);
        let ptr = p + 2;
        if ptr >= s.len() || at(s, ptr + 1) != b'?' {
            return None;
        }
        let t = ptr + 2;
        let et = t + s[t..].iter().position(|&c| c == b':')?;
        let e = et + 1;
        let ee = e + s[e..].iter().position(|&c| c == b'}')?;
        match s[ptr] {
            b'x' => {
                if ms.mode & 0o111 != 0 {
                    out.extend_from_slice(&s[t..et]);
                } else {
                    out.extend_from_slice(&s[e..ee]);
                }
            }
            _ => return None,
        }
        i = ee + 1;
    }
    out.extend_from_slice(&s[i..]);
    if out.len() >= len {
        return None;
    }
    Some(out)
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    if n.is_empty() {
        return Some(0);
    }
    h.windows(n.len()).position(|w| w == n)
}

fn mprint(ms: &mut MagicSet, m: &Magic, s: &[u8]) -> Result<(), magic::Fail> {
    let desc_owned = varexpand(ms, &m.desc, 512).unwrap_or_else(|| m.desc_bytes().to_vec());
    let desc = desc_owned.as_slice();
    let raw = ms.flags & magic::MAGIC_RAW != 0;
    // PRINTER: inteiro de 8/16/32 (promovido a int) ou 64 bits.
    let printer = |ms: &mut MagicSet, value: u64, bits: u32| -> Result<(), magic::Fail> {
        let v = signextend(m, value).unwrap_or(value);
        let unsigned = m.flag & UNSIGNED != 0;
        if check_fmt(desc) {
            let text = match (bits, unsigned) {
                (8, true) => (v as u8).to_string(),
                (8, false) => (v as i8).to_string(),
                (16, true) => (v as u16).to_string(),
                (16, false) => (v as i16).to_string(),
                (32, true) => (v as u32).to_string(),
                (32, false) => (v as i32).to_string(),
                (_, true) => v.to_string(),
                (_, false) => (v as i64).to_string(),
            };
            ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(text.as_bytes()))
        } else {
            let (def, arg): (&[u8], Arg<'_>) = match (bits, unsigned) {
                (8, true) => (b"%u", Arg::Int(u32::from(v as u8))),
                (8, false) => (b"%d", Arg::Int(i32::from(v as i8) as u32)),
                (16, true) => (b"%u", Arg::Int(u32::from(v as u16))),
                (16, false) => (b"%d", Arg::Int(i32::from(v as i16) as u32)),
                (32, true) => (b"%u", Arg::Int(v as u32)),
                (32, false) => (b"%d", Arg::Int(v as u32)),
                (_, true) => (b"%llu", Arg::Long(v)),
                (_, false) => (b"%lld", Arg::Long(v)),
            };
            ms.printf(cfmt::fmtcheck(desc, def), arg)
        }
    };
    match m.typ {
        FILE_BYTE => printer(ms, u64::from(vb(ms)), 8),
        FILE_SHORT | FILE_BESHORT | FILE_LESHORT => printer(ms, u64::from(vh(ms)), 16),
        FILE_LONG | FILE_BELONG | FILE_LELONG | FILE_MELONG => printer(ms, u64::from(vl(ms)), 32),
        FILE_QUAD | FILE_BEQUAD | FILE_LEQUAD | FILE_OFFSET => printer(ms, vq(ms), 64),
        FILE_STRING | FILE_PSTRING | FILE_BESTRING16 | FILE_LESTRING16 => {
            if m.reln == b'=' || m.reln == b'!' {
                let p = magic::printable(raw, 512, &m.value);
                ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(&p))
            } else {
                let mut sv = ms.ms_value.to_vec();
                if m.value[0] == 0 {
                    let cut = sv
                        .iter()
                        .position(|&c| c == b'\r' || c == b'\n' || c == 0)
                        .unwrap_or(sv.len());
                    sv.truncate(cut);
                }
                let str_: &[u8] = if m.str_flags() & STRING_TRIM != 0 {
                    magic::strtrim(&sv)
                } else {
                    cstr(&sv)
                };
                let p = magic::printable(raw, 512, str_);
                ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(&p))?;
                if m.typ == FILE_PSTRING && pstring_length_size(m).is_none() {
                    return Err(magic::Fail);
                }
                Ok(())
            }
        }
        FILE_DATE | FILE_BEDATE | FILE_LEDATE | FILE_MEDATE => {
            let t = magic::fmtdatetime(u64::from(vl(ms)), false, false, &ms.tz);
            ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(t.as_bytes()))
        }
        FILE_LDATE | FILE_BELDATE | FILE_LELDATE | FILE_MELDATE => {
            let t = magic::fmtdatetime(u64::from(vl(ms)), true, false, &ms.tz);
            ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(t.as_bytes()))
        }
        FILE_QDATE | FILE_BEQDATE | FILE_LEQDATE => {
            let t = magic::fmtdatetime(vq(ms), false, false, &ms.tz);
            ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(t.as_bytes()))
        }
        FILE_QLDATE | FILE_BEQLDATE | FILE_LEQLDATE => {
            let t = magic::fmtdatetime(vq(ms), true, false, &ms.tz);
            ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(t.as_bytes()))
        }
        FILE_QWDATE | FILE_BEQWDATE | FILE_LEQWDATE => {
            let t = magic::fmtdatetime(vq(ms), false, true, &ms.tz);
            ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(t.as_bytes()))
        }
        FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT => {
            let vf = f32::from_le_bytes(ms.ms_value[..4].try_into().unwrap_or([0; 4]));
            if check_fmt(desc) {
                let t = cfmt::format(b"%g", Arg::Double(f64::from(vf)));
                ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(&t))
            } else {
                ms.printf(cfmt::fmtcheck(desc, b"%g"), Arg::Double(f64::from(vf)))
            }
        }
        FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => {
            let vd = f64::from_le_bytes(ms.ms_value[..8].try_into().unwrap_or([0; 8]));
            if check_fmt(desc) {
                let t = cfmt::format(b"%g", Arg::Double(vd));
                ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(&t))
            } else {
                ms.printf(cfmt::fmtcheck(desc, b"%g"), Arg::Double(vd))
            }
        }
        FILE_SEARCH | FILE_REGEX => {
            let start = ms.search.s.unwrap_or(0).min(s.len());
            let end = (start + ms.search.rm_len).min(s.len());
            let cp = cstr(&s[start..end]).to_vec();
            let scp: &[u8] = if m.str_flags() & STRING_TRIM != 0 {
                magic::strtrim(&cp)
            } else {
                &cp
            };
            let p = magic::printable(raw, 512, scp);
            ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(&p))
        }
        FILE_DEFAULT | FILE_CLEAR => ms.print(m.desc_bytes()),
        FILE_INDIRECT | FILE_USE | FILE_NAME => Ok(()),
        FILE_DER => {
            let p = magic::printable(raw, 512, &ms.ms_value.clone());
            ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(&p))
        }
        FILE_GUID => {
            let g = ms.ms_value;
            let t = format!(
                "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
                u32::from_le_bytes([g[0], g[1], g[2], g[3]]),
                u16::from_le_bytes([g[4], g[5]]),
                u16::from_le_bytes([g[6], g[7]]),
                g[8],
                g[9],
                g[10],
                g[11],
                g[12],
                g[13],
                g[14],
                g[15]
            );
            ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(t.as_bytes()))
        }
        FILE_MSDOSDATE | FILE_BEMSDOSDATE | FILE_LEMSDOSDATE => {
            let t = magic::fmtdate(vh(ms));
            ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(t.as_bytes()))
        }
        FILE_MSDOSTIME | FILE_BEMSDOSTIME | FILE_LEMSDOSTIME => {
            let t = magic::fmttime(vh(ms));
            ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(t.as_bytes()))
        }
        FILE_OCTAL => {
            let us = cstr(&m.value);
            let c = strtoull(us, 8);
            let rest = &us[c.used.min(us.len())..];
            let t = if !rest.is_empty() && !is_space(rest[0]) {
                format!("*Invalid number {}*", String::from_utf8_lossy(us))
            } else {
                c.value.to_string()
            };
            ms.printf(cfmt::fmtcheck(desc, b"%s"), Arg::Str(t.as_bytes()))
        }
        t => {
            ms.magerror(&format!("invalid m->type ({t}) in mprint()"));
            Err(magic::Fail)
        }
    }
}

fn print_sep(ms: &mut MagicSet, firstline: bool) -> Result<(), magic::Fail> {
    if firstline {
        return Ok(());
    }
    ms.separator()
}

fn handle_annotation(ms: &mut MagicSet, m: &Magic, firstline: bool) -> i32 {
    if ms.flags & MAGIC_APPLE != 0 && m.apple[0] != 0 {
        if print_sep(ms, firstline).is_err() || ms.print(m.apple_bytes()).is_err() {
            return -1;
        }
        return 1;
    }
    if ms.flags & MAGIC_EXTENSION != 0 && m.ext[0] != 0 {
        if print_sep(ms, firstline).is_err() || ms.print(m.ext_bytes()).is_err() {
            return -1;
        }
        return 1;
    }
    if ms.flags & MAGIC_MIME_TYPE != 0 && m.mimetype[0] != 0 {
        if print_sep(ms, firstline).is_err() {
            return -1;
        }
        let p = varexpand(ms, &m.mimetype, 1024).unwrap_or_else(|| m.mime_bytes().to_vec());
        if ms.print(&p).is_err() {
            return -1;
        }
        return 1;
    }
    0
}

// ---- offsets ----

/// `moffset()`: onde o próximo `&` relativo começa. `None` é o -1/0 do C.
fn moffset(ms: &mut MagicSet, m: &Magic, nbytes: usize, offset: usize) -> Option<i32> {
    let off = ms.offset;
    let o: i32 = match m.typ {
        FILE_BYTE => off.wrapping_add(1),
        FILE_SHORT | FILE_BESHORT | FILE_LESHORT | FILE_MSDOSDATE | FILE_LEMSDOSDATE
        | FILE_BEMSDOSDATE | FILE_MSDOSTIME | FILE_LEMSDOSTIME | FILE_BEMSDOSTIME => {
            off.wrapping_add(2)
        }
        FILE_LONG | FILE_BELONG | FILE_LELONG | FILE_MELONG => off.wrapping_add(4),
        FILE_QUAD | FILE_BEQUAD | FILE_LEQUAD => off.wrapping_add(8),
        FILE_STRING | FILE_PSTRING | FILE_BESTRING16 | FILE_LESTRING16 | FILE_OCTAL => {
            if m.reln == b'=' || m.reln == b'!' {
                off.wrapping_add(i32::from(m.vallen))
            } else {
                if m.value[0] == 0 {
                    let cut = ms
                        .ms_value
                        .iter()
                        .position(|&c| c == b'\r' || c == b'\n' || c == 0)
                        .unwrap_or(128);
                    if cut < 128 {
                        ms.ms_value[cut] = 0;
                    }
                }
                let len = cstr(&ms.ms_value).len() as u32;
                let mut o = (off as u32).wrapping_add(len);
                if m.typ == FILE_PSTRING {
                    let l = pstring_length_size(m)?;
                    o = o.wrapping_add(l as u32);
                }
                o as i32
            }
        }
        FILE_DATE | FILE_BEDATE | FILE_LEDATE | FILE_MEDATE | FILE_LDATE | FILE_BELDATE
        | FILE_LELDATE | FILE_MELDATE => off.wrapping_add(4),
        FILE_QDATE | FILE_BEQDATE | FILE_LEQDATE | FILE_QLDATE | FILE_BEQLDATE | FILE_LEQLDATE => {
            off.wrapping_add(8)
        }
        FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT => off.wrapping_add(4),
        FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => off.wrapping_add(8),
        FILE_REGEX => {
            if m.str_flags() & REGEX_OFFSET_START != 0 {
                (ms.search.offset as i64 - offset as i64) as i32
            } else {
                (ms.search.offset as i64 + ms.search.rm_len as i64 - offset as i64) as i32
            }
        }
        FILE_SEARCH => {
            if m.str_flags() & REGEX_OFFSET_START != 0 {
                (ms.search.offset as i64 - offset as i64) as i32
            } else {
                (ms.search.offset as i64 + i64::from(m.vallen) - offset as i64) as i32
            }
        }
        FILE_CLEAR | FILE_DEFAULT | FILE_INDIRECT | FILE_OFFSET | FILE_USE => off,
        FILE_DER => return Some(0),
        FILE_GUID => off.wrapping_add(16),
        _ => 0,
    };
    // `CAST(size_t, o) > nbytes`: negativo vira enorme.
    if o as isize as usize > nbytes {
        return None;
    }
    Some(o)
}

/// `msetoffset()`: `Ok(Some(view))` quando o `bb` muda, `Ok(None)` quando fica o anterior.
fn msetoffset(
    ms: &mut MagicSet,
    m: &Magic,
    b: &Buffer<'_>,
    o: usize,
    cont_level: usize,
) -> Result<Option<View>, ()> {
    if m.flag & OFFNEGATIVE != 0 && !(cont_level > 0 && m.flag & (OFFADD | INDIROFFADD) != 0) {
        let e = b.fill().ok_or(())?;
        if o != 0 {
            ms.magerror(&format!("non zero offset {o} at level {cont_level}"));
            return Err(());
        }
        let moff = m.offset as u32 as usize;
        if moff > e.len() {
            return Err(());
        }
        let v = (e.len() - moff) as i32;
        ms.offset = v;
        ms.eoffset = v;
        return Ok(Some(View::End(e)));
    }
    let offset = if m.flag & OFFNEGATIVE != 0 {
        m.offset.wrapping_neg()
    } else {
        m.offset
    };
    if m.flag & OFFNEGATIVE != 0 || m.flag & OFFPOSITIVE != 0 || cont_level == 0 {
        ms.offset = offset;
        ms.eoffset = 0;
        return Ok(Some(View::Front));
    }
    ms.offset = ms.eoffset.wrapping_add(offset);
    Ok(None)
}

// ---- conversão ----

fn cvt_flip(t: u8, flip: bool) -> u8 {
    if !flip {
        return t;
    }
    match t {
        FILE_BESHORT => FILE_LESHORT,
        FILE_BELONG => FILE_LELONG,
        FILE_BEDATE => FILE_LEDATE,
        FILE_BELDATE => FILE_LELDATE,
        FILE_BEQUAD => FILE_LEQUAD,
        FILE_BEQDATE => FILE_LEQDATE,
        FILE_BEQLDATE => FILE_LEQLDATE,
        FILE_BEQWDATE => FILE_LEQWDATE,
        FILE_LESHORT => FILE_BESHORT,
        FILE_LELONG => FILE_BELONG,
        FILE_LEDATE => FILE_BEDATE,
        FILE_LELDATE => FILE_BELDATE,
        FILE_LEQUAD => FILE_BEQUAD,
        FILE_LEQDATE => FILE_BEQDATE,
        FILE_LEQLDATE => FILE_BEQLDATE,
        FILE_LEQWDATE => FILE_BEQWDATE,
        FILE_BEFLOAT => FILE_LEFLOAT,
        FILE_LEFLOAT => FILE_BEFLOAT,
        FILE_BEDOUBLE => FILE_LEDOUBLE,
        FILE_LEDOUBLE => FILE_BEDOUBLE,
        t => t,
    }
}

/// `DO_CVT`: a máscara (`&`, `|`, `+`...) no inteiro de `bits` bits. `None` é divisão por zero.
fn do_cvt(m: &Magic, v: u64, bits: u32) -> Option<u64> {
    let mask_all = if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let mut v = v & mask_all;
    let k = m.num_mask() & mask_all;
    if m.num_mask() != 0 {
        v = match m.mask_op & FILE_OPS_MASK {
            FILE_OPAND => v & k,
            FILE_OPOR => v | k,
            FILE_OPXOR => v ^ k,
            FILE_OPADD => v.wrapping_add(k),
            FILE_OPMINUS => v.wrapping_sub(k),
            FILE_OPMULTIPLY => v.wrapping_mul(k),
            FILE_OPDIVIDE => {
                if k == 0 {
                    return None;
                }
                v / k
            }
            FILE_OPMODULO => {
                if k == 0 {
                    return None;
                }
                v % k
            }
            _ => v,
        } & mask_all;
    }
    if m.mask_op & FILE_OPINVERSE != 0 {
        v = !v & mask_all;
    }
    Some(v)
}

/// `DO_CVT2` pros floats.
fn do_cvt_float(m: &Magic, v: f64, single: bool) -> Option<f64> {
    if m.num_mask() == 0 {
        return Some(v);
    }
    let k = if single {
        f64::from(m.num_mask() as f32)
    } else {
        m.num_mask() as f64
    };
    let r = match m.mask_op & FILE_OPS_MASK {
        FILE_OPADD => v + k,
        FILE_OPMINUS => v - k,
        FILE_OPMULTIPLY => v * k,
        FILE_OPDIVIDE => {
            if k == 0.0 {
                return None;
            }
            v / k
        }
        _ => v,
    };
    Some(if single { f64::from(r as f32) } else { r })
}

/// `mconvert()`: ordem dos bytes e máscara no valor lido. 0 = falhou.
fn mconvert(ms: &mut MagicSet, m: &Magic, flip: bool) -> i32 {
    let zerodiv = |ms: &mut MagicSet| {
        ms.magerror("zerodivide in mconvert()");
        0
    };
    macro_rules! cvt {
        ($get:expr, $bits:expr, $set:ident, $t:ty) => {{
            match do_cvt(m, u64::from($get), $bits) {
                Some(v) => {
                    $set(ms, v as $t);
                    1
                }
                None => zerodiv(ms),
            }
        }};
    }
    match cvt_flip(m.typ, flip) {
        FILE_BYTE => cvt!(vb(ms), 8, set_vb, u8),
        FILE_SHORT | FILE_MSDOSDATE | FILE_LEMSDOSDATE | FILE_BEMSDOSDATE | FILE_MSDOSTIME
        | FILE_LEMSDOSTIME | FILE_BEMSDOSTIME => {
            cvt!(vh(ms), 16, set_vh, u16)
        }
        FILE_LONG | FILE_DATE | FILE_LDATE => cvt!(vl(ms), 32, set_vl, u32),
        FILE_QUAD | FILE_QDATE | FILE_QLDATE | FILE_QWDATE | FILE_OFFSET => {
            cvt!(vq(ms), 64, set_vq, u64)
        }
        FILE_STRING | FILE_BESTRING16 | FILE_LESTRING16 | FILE_OCTAL => {
            ms.ms_value[127] = 0;
            1
        }
        FILE_PSTRING => {
            let Some(sz) = pstring_length_size(m) else {
                return 0;
            };
            let Some(len) = pstring_get_length(m, &ms.ms_value) else {
                return 0;
            };
            let maxlen = 128 - sz;
            let len = (len as usize).min(maxlen);
            let src: Vec<u8> = ms.ms_value[sz..sz + len.min(128 - sz)].to_vec();
            ms.ms_value[..src.len()].copy_from_slice(&src);
            if src.len() < 128 {
                ms.ms_value[src.len()] = 0;
            }
            1
        }
        FILE_BESHORT => {
            let v = be16(&ms.ms_value);
            cvt!(v, 16, set_vh, u16)
        }
        FILE_BELONG | FILE_BEDATE | FILE_BELDATE => {
            let v = be32(&ms.ms_value);
            cvt!(v, 32, set_vl, u32)
        }
        FILE_BEQUAD | FILE_BEQDATE | FILE_BEQLDATE | FILE_BEQWDATE => {
            let v = be64(&ms.ms_value);
            cvt!(v, 64, set_vq, u64)
        }
        FILE_LESHORT => {
            let v = le16(&ms.ms_value);
            cvt!(v, 16, set_vh, u16)
        }
        FILE_LELONG | FILE_LEDATE | FILE_LELDATE => {
            let v = le32(&ms.ms_value);
            cvt!(v, 32, set_vl, u32)
        }
        FILE_LEQUAD | FILE_LEQDATE | FILE_LEQLDATE | FILE_LEQWDATE => {
            let v = le64(&ms.ms_value);
            cvt!(v, 64, set_vq, u64)
        }
        FILE_MELONG | FILE_MEDATE | FILE_MELDATE => {
            let v = me32(&ms.ms_value);
            cvt!(v, 32, set_vl, u32)
        }
        t @ (FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT) => {
            let bits = match t {
                FILE_BEFLOAT => be32(&ms.ms_value),
                FILE_LEFLOAT => le32(&ms.ms_value),
                _ => vl(ms),
            };
            match do_cvt_float(m, f64::from(f32::from_bits(bits)), true) {
                Some(f) => {
                    set_vl(ms, (f as f32).to_bits());
                    1
                }
                None => zerodiv(ms),
            }
        }
        t @ (FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE) => {
            let bits = match t {
                FILE_BEDOUBLE => be64(&ms.ms_value),
                FILE_LEDOUBLE => le64(&ms.ms_value),
                _ => vq(ms),
            };
            match do_cvt_float(m, f64::from_bits(bits), false) {
                Some(f) => {
                    set_vq(ms, f.to_bits());
                    1
                }
                None => zerodiv(ms),
            }
        }
        FILE_REGEX | FILE_SEARCH | FILE_DEFAULT | FILE_CLEAR | FILE_NAME | FILE_USE | FILE_DER
        | FILE_GUID => 1,
        t => {
            ms.magerror(&format!("invalid type {t} in mconvert()"));
            0
        }
    }
}

// ---- leitura ----

/// `mcopy()`: copia o valor (ou aponta a janela de busca) a partir de `offset`.
fn mcopy(ms: &mut MagicSet, typ: u8, indir: bool, s: &[u8], offset: u32, nbytes: usize, m: &Magic) {
    let mut size = 128usize;
    let mut offset = offset as usize;
    if !indir {
        match typ {
            FILE_DER | FILE_SEARCH => {
                if offset > nbytes {
                    offset = nbytes;
                }
                ms.search.s = Some(offset);
                ms.search.s_len = nbytes - offset;
                ms.search.offset = offset;
                return;
            }
            FILE_REGEX => {
                if nbytes < offset {
                    ms.search.s_len = 0;
                    ms.search.s = None;
                    return;
                }
                let (linecnt, mut bytecnt) = if m.str_flags() & REGEX_LINE_COUNT != 0 {
                    let l = m.str_range() as usize;
                    (l, l * 80)
                } else {
                    (0, m.str_range() as usize)
                };
                if bytecnt == 0 || bytecnt > nbytes - offset {
                    bytecnt = nbytes - offset;
                }
                if bytecnt > usize::from(ms.params.regex_max) {
                    bytecnt = usize::from(ms.params.regex_max);
                }
                let buf = offset;
                let end = offset + bytecnt;
                let mut last = end;
                let mut lines = linecnt;
                let mut bpos = buf;
                while lines > 0 && bpos < end {
                    let nl = s[bpos..end.min(s.len())]
                        .iter()
                        .position(|&c| c == b'\n')
                        .map(|p| p + bpos);
                    let found = match nl {
                        Some(p) => Some(p),
                        None => s[bpos..end.min(s.len())]
                            .iter()
                            .position(|&c| c == b'\r')
                            .map(|p| p + bpos),
                    };
                    let Some(mut p) = found else { break };
                    if p + 1 < end && at(s, p) == b'\r' && at(s, p + 1) == b'\n' {
                        p += 1;
                    }
                    if p + 1 < end && at(s, p) == b'\n' {
                        p += 1;
                    }
                    last = p;
                    lines -= 1;
                    bpos = p + 1;
                }
                if lines > 0 {
                    last = end;
                }
                ms.search.s = Some(buf);
                ms.search.s_len = last - buf;
                ms.search.offset = offset;
                ms.search.rm_len = 0;
                return;
            }
            FILE_BESTRING16 | FILE_LESTRING16 => {
                if offset >= nbytes {
                    // `break`: cai na cópia genérica abaixo.
                } else {
                    let mut src = offset + usize::from(typ == FILE_BESTRING16);
                    let esrc = nbytes;
                    let mut dst = 0usize;
                    let mut out = [0u8; 128];
                    while src < esrc {
                        if dst < 127 {
                            out[dst] = at(s, src);
                        } else {
                            break;
                        }
                        if out[dst] == 0 {
                            let nonzero = if typ == FILE_BESTRING16 {
                                at(s, src - 1) != 0
                            } else {
                                src + 1 < esrc && at(s, src + 1) != 0
                            };
                            if nonzero {
                                out[dst] = b' ';
                            }
                        }
                        src += 2;
                        dst += 1;
                    }
                    out[127] = 0;
                    if dst < 128 {
                        out[dst] = 0;
                    }
                    ms.ms_value = out;
                    return;
                }
            }
            FILE_STRING | FILE_PSTRING => {
                let r = m.str_range() as usize;
                if r != 0 && r < 128 {
                    size = r;
                }
            }
            _ => {}
        }
    }
    if typ == FILE_OFFSET {
        ms.ms_value = [0; 128];
        set_vq(ms, offset as u64);
        return;
    }
    if offset >= nbytes {
        ms.ms_value = [0; 128];
        return;
    }
    let n = (nbytes - offset).min(size);
    let mut out = [0u8; 128];
    for (i, slot) in out.iter_mut().enumerate().take(n) {
        *slot = at(s, offset + i);
    }
    ms.ms_value = out;
}

/// `do_ops()`: o operador do offset indireto. `None` é o "1" (estouro) do C.
fn do_ops(m: &Magic, lhs: i64, off: i64) -> Option<u32> {
    const UINT_MAX: i64 = u32::MAX as i64;
    const INT_MIN: i64 = i32::MIN as i64;
    if lhs >= UINT_MAX || lhs <= INT_MIN || off >= UINT_MAX || off <= INT_MIN {
        return None;
    }
    let mut offset = if off != 0 {
        match m.in_op & FILE_OPS_MASK {
            FILE_OPAND => lhs & off,
            FILE_OPOR => lhs | off,
            FILE_OPXOR => lhs ^ off,
            FILE_OPADD => lhs + off,
            FILE_OPMINUS => lhs - off,
            FILE_OPMULTIPLY => lhs.wrapping_mul(off),
            FILE_OPDIVIDE => lhs / off,
            FILE_OPMODULO => lhs % off,
            _ => lhs,
        }
    } else {
        lhs
    };
    if m.in_op & FILE_OPINVERSE != 0 {
        offset = !offset;
    }
    if offset >= UINT_MAX {
        return None;
    }
    Some(offset as u32)
}

/// `file_magicfind()`: a regra `name` no segundo conjunto.
fn magicfind<'a>(dbs: &'a [Arc<magic::Db>], name: &[u8]) -> Option<MagicList<'a>> {
    for db in dbs {
        let list = db.list(1);
        for (i, m) in list.magic.iter().enumerate() {
            if m.typ != FILE_NAME || cstr(&m.value) != name {
                continue;
            }
            let mut j = i + 1;
            while j < list.magic.len() && list.magic[j].cont_level != 0 {
                j += 1;
            }
            return Some(list.sub(i, j));
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn mget(
    ms: &mut MagicSet,
    list: MagicList<'_>,
    magindex: usize,
    b: &Buffer<'_>,
    s: &[u8],
    o: usize,
    cont_level: usize,
    mode: u16,
    text: bool,
    flip: bool,
    ic: &mut u16,
    nc: &mut u16,
    st: &mut State,
    returnval: &mut i32,
    found_match: &mut i32,
) -> i32 {
    let m = &list.magic[magindex];
    let nbytes = s.len();
    let mut offset = ms.offset as u32;
    if *ic >= ms.params.indir_max {
        ms.file_error(0, &format!("indirect count ({}) exceeded", *ic));
        return -1;
    }
    if *nc >= ms.params.name_max {
        ms.file_error(0, &format!("name use count ({}) exceeded", *nc));
        return -1;
    }
    mcopy(
        ms,
        m.typ,
        m.flag & INDIR != 0,
        s,
        offset.wrapping_add(o as u32),
        nbytes,
        m,
    );

    if m.flag & INDIR != 0 {
        let mut off: i64 = i64::from(m.in_offset);
        let sgn = m.in_op & FILE_OPSIGNED != 0;
        if m.in_op & FILE_OPINDIRECT != 0 {
            let base = i64::from(offset) + off;
            let hb = |k: usize| at(s, (base as usize).wrapping_add(k));
            let hbs: Vec<u8> = (0..8).map(hb).collect();
            let need = |n: usize| oob(nbytes, base, n);
            off = match cvt_flip(m.in_type, flip) {
                FILE_BYTE => {
                    if need(1) {
                        return 0;
                    }
                    sext(sgn, 8, u64::from(hbs[0]))
                }
                FILE_SHORT => {
                    if need(2) {
                        return 0;
                    }
                    sext(sgn, 16, u64::from(le16(&hbs)))
                }
                FILE_BESHORT => {
                    if need(2) {
                        return 0;
                    }
                    sext(sgn, 16, u64::from(be16(&hbs)))
                }
                FILE_LESHORT => {
                    if need(2) {
                        return 0;
                    }
                    sext(sgn, 16, u64::from(le16(&hbs)))
                }
                FILE_LONG => {
                    if need(4) {
                        return 0;
                    }
                    sext(sgn, 32, u64::from(le32(&hbs)))
                }
                FILE_BELONG | FILE_BEID3 => {
                    if need(4) {
                        return 0;
                    }
                    sext(sgn, 32, u64::from(be32(&hbs)))
                }
                FILE_LEID3 | FILE_LELONG => {
                    if need(4) {
                        return 0;
                    }
                    sext(sgn, 32, u64::from(le32(&hbs)))
                }
                FILE_MELONG => {
                    if need(4) {
                        return 0;
                    }
                    sext(sgn, 32, u64::from(me32(&hbs)))
                }
                FILE_BEQUAD => {
                    if need(8) {
                        return 0;
                    }
                    sext(sgn, 64, be64(&hbs))
                }
                FILE_LEQUAD => {
                    if need(8) {
                        return 0;
                    }
                    sext(sgn, 64, le64(&hbs))
                }
                FILE_OCTAL => {
                    if oob(nbytes, i64::from(offset), usize::from(m.vallen)) {
                        return 0;
                    }
                    sext(sgn, 64, strtoull(cstr(&ms.ms_value), 8).value)
                }
                _ => return 0,
            };
        }
        let in_type = cvt_flip(m.in_type, flip);
        let pv = ms.ms_value;
        let lhs: i64 = match in_type {
            FILE_BYTE => {
                if oob(nbytes, i64::from(offset), 1) {
                    return 0;
                }
                sext(sgn, 8, u64::from(pv[0]))
            }
            FILE_BESHORT => {
                if oob(nbytes, i64::from(offset), 2) {
                    return 0;
                }
                sext(sgn, 16, u64::from(be16(&pv)))
            }
            FILE_LESHORT | FILE_SHORT => {
                if oob(nbytes, i64::from(offset), 2) {
                    return 0;
                }
                sext(sgn, 16, u64::from(le16(&pv)))
            }
            FILE_BELONG | FILE_BEID3 => {
                if oob(nbytes, i64::from(offset), 4) {
                    return 0;
                }
                let mut l = be32(&pv);
                if in_type == FILE_BEID3 {
                    l = cvt_id3(l);
                }
                sext(sgn, 32, u64::from(l))
            }
            FILE_LELONG | FILE_LEID3 => {
                if oob(nbytes, i64::from(offset), 4) {
                    return 0;
                }
                let mut l = le32(&pv);
                if in_type == FILE_LEID3 {
                    l = cvt_id3(l);
                }
                sext(sgn, 32, u64::from(l))
            }
            FILE_MELONG => {
                if oob(nbytes, i64::from(offset), 4) {
                    return 0;
                }
                sext(sgn, 32, u64::from(me32(&pv)))
            }
            FILE_LONG => {
                if oob(nbytes, i64::from(offset), 4) {
                    return 0;
                }
                sext(sgn, 32, u64::from(le32(&pv)))
            }
            FILE_LEQUAD => {
                if oob(nbytes, i64::from(offset), 8) {
                    return 0;
                }
                sext(sgn, 64, le64(&pv))
            }
            FILE_BEQUAD => {
                if oob(nbytes, i64::from(offset), 8) {
                    return 0;
                }
                sext(sgn, 64, be64(&pv))
            }
            FILE_OCTAL => {
                if oob(nbytes, i64::from(offset), usize::from(m.vallen)) {
                    return 0;
                }
                sext(sgn, 64, strtoull(cstr(&pv), 8).value)
            }
            _ => return 0,
        };
        match do_ops(m, lhs, off) {
            Some(v) => offset = v,
            None => return 0,
        }
        if m.flag & INDIROFFADD != 0 {
            if cont_level == 0 {
                return 0;
            }
            offset = offset.wrapping_add(ms.li[cont_level - 1].off as u32);
            if offset == 0 {
                return 0;
            }
        }
        mcopy(ms, m.typ, false, s, offset, nbytes, m);
        ms.offset = offset as i32;
    }

    let need = |n: usize| oob(nbytes, i64::from(offset), n);
    match m.typ {
        FILE_BYTE => {
            if need(1) {
                return 0;
            }
        }
        FILE_SHORT | FILE_BESHORT | FILE_LESHORT => {
            if need(2) {
                return 0;
            }
        }
        FILE_LONG | FILE_BELONG | FILE_LELONG | FILE_MELONG | FILE_DATE | FILE_BEDATE
        | FILE_LEDATE | FILE_MEDATE | FILE_LDATE | FILE_BELDATE | FILE_LELDATE | FILE_MELDATE
        | FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT => {
            if need(4) {
                return 0;
            }
        }
        FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => {
            if need(8) {
                return 0;
            }
        }
        FILE_GUID => {
            if need(16) {
                return 0;
            }
        }
        FILE_STRING | FILE_PSTRING | FILE_SEARCH | FILE_OCTAL => {
            if need(usize::from(m.vallen)) {
                return 0;
            }
        }
        FILE_REGEX => {
            if nbytes < offset as usize {
                return 0;
            }
        }
        FILE_INDIRECT => {
            if m.str_flags() & INDIRECT_RELATIVE != 0 {
                offset = offset.wrapping_add(o as u32);
            }
            if offset == 0 || nbytes < offset as usize {
                return 0;
            }
            let Some(pb) = ms.push_buffer() else {
                return -1;
            };
            *ic += 1;
            let sub = &s[offset as usize..];
            let bb = Buffer::new(sub, b.st_mode, sub.len() as u64, None);
            *bb.ebuf.borrow_mut() = Some(Some(Rc::new(Vec::new())));
            let mut rv = -1;
            let dbs: Vec<Arc<magic::Db>> = ms.mlist.clone();
            for db in &dbs {
                let mut r2 = 0;
                let mut f2 = 0;
                rv = do_match(
                    ms,
                    db.list(0),
                    &bb,
                    0,
                    BINTEST,
                    text,
                    false,
                    ic,
                    nc,
                    st,
                    &mut r2,
                    &mut f2,
                );
                if rv != 0 {
                    break;
                }
            }
            let rbuf = ms.pop_buffer(pb);
            if rbuf.is_none() && ms.had_err {
                return -1;
            }
            if rv == 1 {
                if ms.flags & MAGIC_NODESC == 0
                    && ms
                        .printf(cfmt::fmtcheck(m.desc_bytes(), b"%u"), Arg::Int(offset))
                        .is_err()
                {
                    return -1;
                }
                if let Some(r) = &rbuf
                    && ms.print(cstr(r)).is_err()
                {
                    return -1;
                }
            }
            return rv;
        }
        FILE_USE => {
            if nbytes < offset as usize {
                return 0;
            }
            let mut name = cstr(&m.value);
            let mut flip = flip;
            if name.first() == Some(&b'^') {
                name = &name[1..];
                flip = !flip;
            }
            let dbs: Vec<Arc<magic::Db>> = ms.mlist.clone();
            let Some(ml) = magicfind(&dbs, name) else {
                ms.file_error(
                    0,
                    &format!("cannot find entry `{}'", String::from_utf8_lossy(name)),
                );
                return -1;
            };
            let saved = ms.li.clone();
            let oneed = st.need_separator;
            if m.flag & NOSPACE != 0 {
                st.need_separator = false;
            }
            let mut nfound = 0;
            *nc += 1;
            let eoffset = ms.eoffset;
            let rv = do_match(
                ms,
                ml,
                b,
                offset as usize + o,
                mode,
                text,
                flip,
                ic,
                nc,
                st,
                returnval,
                &mut nfound,
            );
            ms.ms_value = [0; 128];
            set_vq(ms, nfound as u64);
            *nc -= 1;
            *found_match |= nfound;
            ms.li = saved;
            if rv != 1 {
                st.need_separator = oneed;
            }
            ms.offset = offset as i32;
            ms.eoffset = eoffset;
            return i32::from(rv != 0 || *found_match != 0);
        }
        FILE_NAME => {
            if ms.flags & MAGIC_NODESC != 0 {
                return 1;
            }
            if ms.print(m.desc_bytes()).is_err() {
                return -1;
            }
            return 1;
        }
        _ => {}
    }
    if mconvert(ms, m, flip) == 0 {
        return 0;
    }
    1
}

fn cvt_id3(v: u32) -> u32 {
    (v & 0x7f) | (((v >> 8) & 0x7f) << 7) | (((v >> 16) & 0x7f) << 14) | (((v >> 24) & 0x7f) << 21)
}

// ---- comparação ----

/// `file_strncmp()`: `a` é o valor da regra, `b` o dado.
fn file_strncmp(a: &[u8], b: &[u8], len: usize, maxlen: usize, flags: u32) -> u64 {
    let ws = flags & (STRING_COMPACT_WHITESPACE | STRING_COMPACT_OPTIONAL_WHITESPACE);
    let eb = if ws != 0 { maxlen } else { len };
    let mut v: u64 = 0;
    let (mut ai, mut bi) = (0usize, 0usize);
    let mut len = len + 1;
    if flags == 0 {
        while {
            len -= 1;
            len > 0
        } {
            v = u64::from(at(b, bi)).wrapping_sub(u64::from(at(a, ai)));
            bi += 1;
            ai += 1;
            if v != 0 {
                break;
            }
        }
        return v;
    }
    while {
        len -= 1;
        len > 0
    } {
        if bi >= eb {
            v = 1;
            break;
        }
        let ac = at(a, ai);
        if flags & STRING_IGNORE_LOWERCASE != 0 && is_lower(ac) {
            v = u64::from(at(b, bi).to_ascii_lowercase()).wrapping_sub(u64::from(ac));
            bi += 1;
            ai += 1;
            if v != 0 {
                break;
            }
        } else if flags & STRING_IGNORE_UPPERCASE != 0 && is_upper(ac) {
            v = u64::from(at(b, bi).to_ascii_uppercase()).wrapping_sub(u64::from(ac));
            bi += 1;
            ai += 1;
            if v != 0 {
                break;
            }
        } else if flags & STRING_COMPACT_WHITESPACE != 0 && is_space(ac) {
            ai += 1;
            if is_space(at(b, bi)) {
                bi += 1;
                if !is_space(at(a, ai)) {
                    while bi < eb && is_space(at(b, bi)) {
                        bi += 1;
                    }
                }
            } else {
                v = 1;
                break;
            }
        } else if flags & STRING_COMPACT_OPTIONAL_WHITESPACE != 0 && is_space(ac) {
            ai += 1;
            while bi < eb && is_space(at(b, bi)) {
                bi += 1;
            }
        } else {
            v = u64::from(at(b, bi)).wrapping_sub(u64::from(ac));
            bi += 1;
            ai += 1;
            if v != 0 {
                break;
            }
        }
    }
    if len == 0 && v == 0 && flags & STRING_FULL_WORD != 0 {
        let c = at(b, bi);
        if c != 0 && !is_space(c) {
            v = 1;
        }
    }
    v
}

fn regex_for(ms: &mut MagicSet, list: MagicList<'_>, i: usize) -> Option<Arc<Regex>> {
    let m = &list.magic[i];
    list.rx[i]
        .get_or_init(|| {
            let icase = m.str_flags() & STRING_IGNORE_CASE != 0;
            Regex::compile(cstr(&m.value), icase, true)
                .ok()
                .map(Arc::new)
        })
        .clone()
        .or_else(|| {
            ms.magerror("regex error");
            None
        })
}

/// `magiccheck()`.
fn magiccheck(ms: &mut MagicSet, list: MagicList<'_>, i: usize, s: &[u8]) -> i32 {
    let m = &list.magic[i];
    let mut l = m.value_q();
    let mut v: u64;
    match m.typ {
        FILE_BYTE => v = u64::from(vb(ms)),
        FILE_SHORT | FILE_BESHORT | FILE_LESHORT | FILE_MSDOSDATE | FILE_LEMSDOSDATE
        | FILE_BEMSDOSDATE | FILE_MSDOSTIME | FILE_LEMSDOSTIME | FILE_BEMSDOSTIME => {
            v = u64::from(vh(ms))
        }
        FILE_LONG | FILE_BELONG | FILE_LELONG | FILE_MELONG | FILE_DATE | FILE_BEDATE
        | FILE_LEDATE | FILE_MEDATE | FILE_LDATE | FILE_BELDATE | FILE_LELDATE | FILE_MELDATE => {
            v = u64::from(vl(ms))
        }
        FILE_QUAD | FILE_LEQUAD | FILE_BEQUAD | FILE_QDATE | FILE_BEQDATE | FILE_LEQDATE
        | FILE_QLDATE | FILE_BEQLDATE | FILE_LEQLDATE | FILE_QWDATE | FILE_BEQWDATE
        | FILE_LEQWDATE | FILE_OFFSET => v = vq(ms),
        FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT | FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => {
            let (fl, fv) = if matches!(m.typ, FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT) {
                (f64::from(m.value_f()), f64::from(f32::from_bits(vl(ms))))
            } else {
                (m.value_d(), f64::from_bits(vq(ms)))
            };
            let unordered = fl.is_nan() || fv.is_nan();
            return match m.reln {
                b'x' => 1,
                b'!' => i32::from(unordered || fv != fl),
                b'=' => i32::from(!unordered && fv == fl),
                b'>' => i32::from(fv > fl),
                b'<' => i32::from(fv < fl),
                r => {
                    ms.magerror(&format!(
                        "cannot happen with float: invalid relation `{}'",
                        r as char
                    ));
                    -1
                }
            };
        }
        FILE_DEFAULT | FILE_CLEAR => {
            l = 0;
            v = 0;
        }
        FILE_STRING | FILE_PSTRING | FILE_OCTAL => {
            l = 0;
            let p = ms.ms_value;
            v = file_strncmp(&m.value, &p, usize::from(m.vallen), 128, m.str_flags());
        }
        FILE_BESTRING16 | FILE_LESTRING16 => {
            l = 0;
            let p = ms.ms_value;
            v = file_strncmp(&m.value, &p, usize::from(m.vallen), 128, 0);
        }
        FILE_SEARCH => {
            let Some(sbase) = ms.search.s else { return 0 };
            let slen = usize::from(m.vallen).min(m.value.len());
            l = 0;
            v = 0;
            let window = &s[sbase.min(s.len())..];
            let s_len = ms.search.s_len;
            if slen > 0 && m.str_flags() == 0 {
                let r = m.str_range() as usize;
                let mut idx = r + slen;
                if r == 0 || s_len < idx {
                    idx = s_len;
                }
                let hay = &window[..idx.min(window.len())];
                match find(hay, &m.value[..slen]) {
                    None => v = 1,
                    Some(found) => {
                        ms.search.offset += found;
                        ms.search.rm_len = s_len - found;
                    }
                }
            } else {
                let mut idx = 0usize;
                loop {
                    if m.str_range() != 0 && idx >= m.str_range() as usize {
                        break;
                    }
                    if slen + idx > s_len {
                        v = 1;
                        break;
                    }
                    v = file_strncmp(
                        &m.value,
                        &window[idx.min(window.len())..],
                        slen,
                        s_len - idx,
                        m.str_flags(),
                    );
                    if v == 0 {
                        ms.search.offset += idx;
                        ms.search.rm_len = s_len - idx;
                        break;
                    }
                    idx += 1;
                }
            }
        }
        FILE_REGEX => {
            let Some(sbase) = ms.search.s else { return 0 };
            let Some(rx) = regex_for(ms, list, i) else {
                return -1;
            };
            l = 0;
            let slen = ms.search.s_len;
            let window = &s[sbase.min(s.len())..(sbase + slen).min(s.len())];
            // A cópia perde o último byte (vira o NUL) e a busca para no primeiro NUL.
            let hay = if slen != 0 {
                cstr(&window[..window.len().saturating_sub(1)])
            } else {
                &[][..]
            };
            match rx.find(hay) {
                Some((so, eo)) => {
                    ms.search.s = Some(sbase + so);
                    ms.search.offset += so;
                    ms.search.rm_len = eo - so;
                    v = 0;
                }
                None => v = 1,
            }
        }
        FILE_USE => return i32::from(vq(ms) != 0),
        FILE_NAME | FILE_INDIRECT => return 1,
        FILE_DER => return 0,
        FILE_GUID => {
            l = 0;
            let n = m.value[..16] != ms.ms_value[..16];
            v = u64::from(n);
        }
        t => {
            ms.magerror(&format!("invalid type {t} in magiccheck()"));
            return -1;
        }
    }
    let v = signextend(m, v).unwrap_or(v);
    match m.reln {
        b'x' => 1,
        b'!' => i32::from(v != l),
        b'=' => i32::from(v == l),
        b'>' => {
            if m.flag & UNSIGNED != 0 {
                i32::from(v > l)
            } else {
                i32::from((v as i64) > (l as i64))
            }
        }
        b'<' => {
            if m.flag & UNSIGNED != 0 {
                i32::from(v < l)
            } else {
                i32::from((v as i64) < (l as i64))
            }
        }
        b'&' => i32::from(v & l == l),
        b'^' => i32::from(v & l != l),
        r => {
            ms.magerror(&format!("cannot happen: invalid relation `{}'", r as char));
            -1
        }
    }
}
