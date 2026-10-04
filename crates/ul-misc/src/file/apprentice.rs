// Porte para Rust do apprentice.c do file 5.46 (com os patches do Debian 5.46-5).
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

//! Leitura das regras de magic (magic(5)): o `apprentice.c` do file 5.46 portado função por
//! função. A entrada é guardada com o mesmo layout do `struct magic` do C (432 bytes no x86-64),
//! o que dá três coisas de graça: o desempate da ordenação (`memcmp` da entrada inteira) sai
//! idêntico, o `-C` grava um `.mgc` que o file de verdade lê, e dá pra comparar o banco compilado
//! aqui com o `magic.mgc` do oráculo byte a byte.

use std::cmp::Ordering;

use super::cutil::{at, cstr, is_alnum, is_alpha, is_digit, is_print, is_space, is_upper, strtod, strtof, strtol, strtoul, strtoull};
use super::encoding::looks_utf8;
use super::regex;

// ---- tipos (FILE_*) ----
pub const FILE_INVALID: u8 = 0;
pub const FILE_BYTE: u8 = 1;
pub const FILE_SHORT: u8 = 2;
pub const FILE_DEFAULT: u8 = 3;
pub const FILE_LONG: u8 = 4;
pub const FILE_STRING: u8 = 5;
pub const FILE_DATE: u8 = 6;
pub const FILE_BESHORT: u8 = 7;
pub const FILE_BELONG: u8 = 8;
pub const FILE_BEDATE: u8 = 9;
pub const FILE_LESHORT: u8 = 10;
pub const FILE_LELONG: u8 = 11;
pub const FILE_LEDATE: u8 = 12;
pub const FILE_PSTRING: u8 = 13;
pub const FILE_LDATE: u8 = 14;
pub const FILE_BELDATE: u8 = 15;
pub const FILE_LELDATE: u8 = 16;
pub const FILE_REGEX: u8 = 17;
pub const FILE_BESTRING16: u8 = 18;
pub const FILE_LESTRING16: u8 = 19;
pub const FILE_SEARCH: u8 = 20;
pub const FILE_MEDATE: u8 = 21;
pub const FILE_MELDATE: u8 = 22;
pub const FILE_MELONG: u8 = 23;
pub const FILE_QUAD: u8 = 24;
pub const FILE_LEQUAD: u8 = 25;
pub const FILE_BEQUAD: u8 = 26;
pub const FILE_QDATE: u8 = 27;
pub const FILE_LEQDATE: u8 = 28;
pub const FILE_BEQDATE: u8 = 29;
pub const FILE_QLDATE: u8 = 30;
pub const FILE_LEQLDATE: u8 = 31;
pub const FILE_BEQLDATE: u8 = 32;
pub const FILE_FLOAT: u8 = 33;
pub const FILE_BEFLOAT: u8 = 34;
pub const FILE_LEFLOAT: u8 = 35;
pub const FILE_DOUBLE: u8 = 36;
pub const FILE_BEDOUBLE: u8 = 37;
pub const FILE_LEDOUBLE: u8 = 38;
pub const FILE_BEID3: u8 = 39;
pub const FILE_LEID3: u8 = 40;
pub const FILE_INDIRECT: u8 = 41;
pub const FILE_QWDATE: u8 = 42;
pub const FILE_LEQWDATE: u8 = 43;
pub const FILE_BEQWDATE: u8 = 44;
pub const FILE_NAME: u8 = 45;
pub const FILE_USE: u8 = 46;
pub const FILE_CLEAR: u8 = 47;
pub const FILE_DER: u8 = 48;
pub const FILE_GUID: u8 = 49;
pub const FILE_OFFSET: u8 = 50;
pub const FILE_BEVARINT: u8 = 51;
pub const FILE_LEVARINT: u8 = 52;
pub const FILE_MSDOSDATE: u8 = 53;
pub const FILE_LEMSDOSDATE: u8 = 54;
pub const FILE_BEMSDOSDATE: u8 = 55;
pub const FILE_MSDOSTIME: u8 = 56;
pub const FILE_LEMSDOSTIME: u8 = 57;
pub const FILE_BEMSDOSTIME: u8 = 58;
pub const FILE_OCTAL: u8 = 59;
pub const FILE_NAMES_SIZE: usize = 60;

// ---- `flag` ----
pub const INDIR: u16 = 0x01;
pub const OFFADD: u16 = 0x02;
pub const INDIROFFADD: u16 = 0x04;
pub const UNSIGNED: u16 = 0x08;
pub const NOSPACE: u16 = 0x10;
pub const BINTEST: u16 = 0x20;
pub const TEXTTEST: u16 = 0x40;
pub const OFFNEGATIVE: u16 = 0x80;
pub const OFFPOSITIVE: u16 = 0x100;

// ---- `str_flags` ----
pub const STRING_COMPACT_WHITESPACE: u32 = 1 << 0;
pub const STRING_COMPACT_OPTIONAL_WHITESPACE: u32 = 1 << 1;
pub const STRING_IGNORE_LOWERCASE: u32 = 1 << 2;
pub const STRING_IGNORE_UPPERCASE: u32 = 1 << 3;
pub const REGEX_OFFSET_START: u32 = 1 << 4;
pub const STRING_TEXTTEST: u32 = 1 << 5;
pub const STRING_BINTEST: u32 = 1 << 6;
pub const PSTRING_1_LE: u32 = 1 << 7;
pub const PSTRING_2_BE: u32 = 1 << 8;
pub const PSTRING_2_LE: u32 = 1 << 9;
pub const PSTRING_4_BE: u32 = 1 << 10;
pub const PSTRING_4_LE: u32 = 1 << 11;
pub const REGEX_LINE_COUNT: u32 = 1 << 11;
pub const PSTRING_LEN: u32 = PSTRING_1_LE | PSTRING_2_LE | PSTRING_2_BE | PSTRING_4_LE | PSTRING_4_BE;
pub const PSTRING_LENGTH_INCLUDES_ITSELF: u32 = 1 << 12;
pub const STRING_TRIM: u32 = 1 << 13;
pub const STRING_FULL_WORD: u32 = 1 << 14;
pub const STRING_IGNORE_CASE: u32 = STRING_IGNORE_LOWERCASE | STRING_IGNORE_UPPERCASE;
pub const STRING_DEFAULT_RANGE: u32 = 100;
pub const INDIRECT_RELATIVE: u32 = 1 << 0;

// ---- operadores de máscara e de indireção ----
pub const FILE_OPAND: u8 = 0;
pub const FILE_OPOR: u8 = 1;
pub const FILE_OPXOR: u8 = 2;
pub const FILE_OPADD: u8 = 3;
pub const FILE_OPMINUS: u8 = 4;
pub const FILE_OPMULTIPLY: u8 = 5;
pub const FILE_OPDIVIDE: u8 = 6;
pub const FILE_OPMODULO: u8 = 7;
pub const FILE_OPS_MASK: u8 = 0x07;
pub const FILE_OPSIGNED: u8 = 0x20;
pub const FILE_OPINVERSE: u8 = 0x40;
pub const FILE_OPINDIRECT: u8 = 0x80;

pub const FILE_FACTOR_OP_NONE: u8 = 0;

pub const COND_NONE: u8 = 0;
pub const COND_IF: u8 = 1;
pub const COND_ELIF: u8 = 2;
pub const COND_ELSE: u8 = 3;

pub const MAXDESC: usize = 64;
pub const MAXMIME: usize = 80;
pub const MAXEXT: usize = 120;
pub const MAXSTRING: usize = 128;
/// Tamanho do `struct magic` no x86-64 (e de cada registro do `.mgc`).
pub const MAGIC_SIZE: usize = 432;
pub const MAGICNO: u32 = 0xF11E_041C;
pub const VERSIONNO: u32 = 20;
pub const FILE_BADSIZE: u64 = u64::MAX;

// Formatos aceitos na descrição de cada tipo (`file_formats`).
const FILE_FMT_NONE: u8 = 0;
const FILE_FMT_NUM: u8 = 1;
const FILE_FMT_STR: u8 = 2;
const FILE_FMT_QUAD: u8 = 3;
const FILE_FMT_FLOAT: u8 = 4;
const FILE_FMT_DOUBLE: u8 = 5;

/// `type_tbl`: nome, tipo e formato, na ordem do C (a busca é por prefixo, a primeira que
/// casa ganha).
const TYPE_TBL: &[(&str, u8, u8)] = &[
    ("invalid", FILE_INVALID, FILE_FMT_NONE),
    ("byte", FILE_BYTE, FILE_FMT_NUM),
    ("short", FILE_SHORT, FILE_FMT_NUM),
    ("default", FILE_DEFAULT, FILE_FMT_NONE),
    ("long", FILE_LONG, FILE_FMT_NUM),
    ("string", FILE_STRING, FILE_FMT_STR),
    ("date", FILE_DATE, FILE_FMT_STR),
    ("beshort", FILE_BESHORT, FILE_FMT_NUM),
    ("belong", FILE_BELONG, FILE_FMT_NUM),
    ("bedate", FILE_BEDATE, FILE_FMT_STR),
    ("leshort", FILE_LESHORT, FILE_FMT_NUM),
    ("lelong", FILE_LELONG, FILE_FMT_NUM),
    ("ledate", FILE_LEDATE, FILE_FMT_STR),
    ("pstring", FILE_PSTRING, FILE_FMT_STR),
    ("ldate", FILE_LDATE, FILE_FMT_STR),
    ("beldate", FILE_BELDATE, FILE_FMT_STR),
    ("leldate", FILE_LELDATE, FILE_FMT_STR),
    ("regex", FILE_REGEX, FILE_FMT_STR),
    ("bestring16", FILE_BESTRING16, FILE_FMT_STR),
    ("lestring16", FILE_LESTRING16, FILE_FMT_STR),
    ("search", FILE_SEARCH, FILE_FMT_STR),
    ("medate", FILE_MEDATE, FILE_FMT_STR),
    ("meldate", FILE_MELDATE, FILE_FMT_STR),
    ("melong", FILE_MELONG, FILE_FMT_NUM),
    ("quad", FILE_QUAD, FILE_FMT_QUAD),
    ("lequad", FILE_LEQUAD, FILE_FMT_QUAD),
    ("bequad", FILE_BEQUAD, FILE_FMT_QUAD),
    ("qdate", FILE_QDATE, FILE_FMT_STR),
    ("leqdate", FILE_LEQDATE, FILE_FMT_STR),
    ("beqdate", FILE_BEQDATE, FILE_FMT_STR),
    ("qldate", FILE_QLDATE, FILE_FMT_STR),
    ("leqldate", FILE_LEQLDATE, FILE_FMT_STR),
    ("beqldate", FILE_BEQLDATE, FILE_FMT_STR),
    ("float", FILE_FLOAT, FILE_FMT_FLOAT),
    ("befloat", FILE_BEFLOAT, FILE_FMT_FLOAT),
    ("lefloat", FILE_LEFLOAT, FILE_FMT_FLOAT),
    ("double", FILE_DOUBLE, FILE_FMT_DOUBLE),
    ("bedouble", FILE_BEDOUBLE, FILE_FMT_DOUBLE),
    ("ledouble", FILE_LEDOUBLE, FILE_FMT_DOUBLE),
    ("leid3", FILE_LEID3, FILE_FMT_NUM),
    ("beid3", FILE_BEID3, FILE_FMT_NUM),
    ("indirect", FILE_INDIRECT, FILE_FMT_NUM),
    ("qwdate", FILE_QWDATE, FILE_FMT_STR),
    ("leqwdate", FILE_LEQWDATE, FILE_FMT_STR),
    ("beqwdate", FILE_BEQWDATE, FILE_FMT_STR),
    ("name", FILE_NAME, FILE_FMT_NONE),
    ("use", FILE_USE, FILE_FMT_NONE),
    ("clear", FILE_CLEAR, FILE_FMT_NONE),
    ("der", FILE_DER, FILE_FMT_STR),
    ("guid", FILE_GUID, FILE_FMT_STR),
    ("offset", FILE_OFFSET, FILE_FMT_QUAD),
    ("bevarint", FILE_BEVARINT, FILE_FMT_STR),
    ("levarint", FILE_LEVARINT, FILE_FMT_STR),
    ("msdosdate", FILE_MSDOSDATE, FILE_FMT_STR),
    ("lemsdosdate", FILE_LEMSDOSDATE, FILE_FMT_STR),
    ("bemsdosdate", FILE_BEMSDOSDATE, FILE_FMT_STR),
    ("msdostime", FILE_MSDOSTIME, FILE_FMT_STR),
    ("lemsdostime", FILE_LEMSDOSTIME, FILE_FMT_STR),
    ("bemsdostime", FILE_BEMSDOSTIME, FILE_FMT_STR),
    ("octal", FILE_OCTAL, FILE_FMT_STR),
];

/// `special_tbl`: tipos que não aceitam o prefixo `u`.
const SPECIAL_TBL: &[(&str, u8)] = &[("der", FILE_DER), ("name", FILE_NAME), ("use", FILE_USE), ("octal", FILE_OCTAL)];

/// Nome de um tipo (`file_names`; a tabela do C está na ordem dos códigos).
pub fn type_name(t: u8) -> &'static str {
    TYPE_TBL.get(usize::from(t)).map(|e| e.0).unwrap_or("invalid")
}

fn type_format(t: u8) -> u8 {
    TYPE_TBL.get(usize::from(t)).map(|e| e.2).unwrap_or(FILE_FMT_NONE)
}

/// `&s[i..]` sem pânico depois do fim (a cadeia C acaba no NUL implícito).
fn tail(s: &[u8], i: usize) -> &[u8] {
    s.get(i..).unwrap_or(&[])
}

/// Um `struct magic`, com os campos na ordem e no tamanho do C.
#[derive(Clone)]
pub struct Magic {
    pub flag: u16,
    pub cont_level: u8,
    pub factor: u8,
    pub reln: u8,
    pub vallen: u8,
    pub typ: u8,
    pub in_type: u8,
    pub in_op: u8,
    pub mask_op: u8,
    pub cond: u8,
    pub factor_op: u8,
    pub offset: i32,
    pub in_offset: i32,
    pub lineno: u32,
    /// A união `_u`: `num_mask` dos tipos numéricos, ou (`str_range`, `str_flags`) dos de cadeia.
    pub u: u64,
    pub value: [u8; MAXSTRING],
    pub desc: [u8; MAXDESC],
    pub mimetype: [u8; MAXMIME],
    pub apple: [u8; 8],
    pub ext: [u8; MAXEXT],
}

impl std::fmt::Debug for Magic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Magic(line {}, level {}, {} {} {:?})",
            self.lineno,
            self.cont_level,
            type_name(self.typ),
            self.reln as char,
            String::from_utf8_lossy(self.desc_bytes())
        )
    }
}

impl Magic {
    pub fn zeroed() -> Magic {
        Magic {
            flag: 0,
            cont_level: 0,
            factor: 0,
            reln: 0,
            vallen: 0,
            typ: 0,
            in_type: 0,
            in_op: 0,
            mask_op: 0,
            cond: 0,
            factor_op: 0,
            offset: 0,
            in_offset: 0,
            lineno: 0,
            u: 0,
            value: [0; MAXSTRING],
            desc: [0; MAXDESC],
            mimetype: [0; MAXMIME],
            apple: [0; 8],
            ext: [0; MAXEXT],
        }
    }

    pub fn num_mask(&self) -> u64 {
        self.u
    }

    pub fn str_range(&self) -> u32 {
        self.u as u32
    }

    pub fn str_flags(&self) -> u32 {
        (self.u >> 32) as u32
    }

    pub fn set_str_range(&mut self, v: u32) {
        self.u = (self.u & 0xffff_ffff_0000_0000) | u64::from(v);
    }

    pub fn set_str_flags(&mut self, v: u32) {
        self.u = (self.u & 0x0000_0000_ffff_ffff) | (u64::from(v) << 32);
    }

    pub fn value_q(&self) -> u64 {
        u64::from_le_bytes(self.value[..8].try_into().unwrap_or([0; 8]))
    }

    pub fn set_value_q(&mut self, v: u64) {
        self.value[..8].copy_from_slice(&v.to_le_bytes());
    }

    pub fn value_f(&self) -> f32 {
        f32::from_le_bytes(self.value[..4].try_into().unwrap_or([0; 4]))
    }

    pub fn value_d(&self) -> f64 {
        f64::from_le_bytes(self.value[..8].try_into().unwrap_or([0; 8]))
    }

    /// `value.s` até o NUL.
    pub fn value_str(&self) -> &[u8] {
        cstr(&self.value)
    }

    pub fn desc_bytes(&self) -> &[u8] {
        cstr(&self.desc)
    }

    pub fn mime_bytes(&self) -> &[u8] {
        cstr(&self.mimetype)
    }

    pub fn ext_bytes(&self) -> &[u8] {
        cstr(&self.ext)
    }

    /// `%.8s` do `apple`.
    pub fn apple_bytes(&self) -> &[u8] {
        cstr(&self.apple)
    }

    /// A entrada como o C a guarda na memória (e no `.mgc`).
    pub fn image(&self) -> [u8; MAGIC_SIZE] {
        let mut b = [0u8; MAGIC_SIZE];
        b[0..2].copy_from_slice(&self.flag.to_le_bytes());
        b[2] = self.cont_level;
        b[3] = self.factor;
        b[4] = self.reln;
        b[5] = self.vallen;
        b[6] = self.typ;
        b[7] = self.in_type;
        b[8] = self.in_op;
        b[9] = self.mask_op;
        b[10] = self.cond;
        b[11] = self.factor_op;
        b[12..16].copy_from_slice(&self.offset.to_le_bytes());
        b[16..20].copy_from_slice(&self.in_offset.to_le_bytes());
        b[20..24].copy_from_slice(&self.lineno.to_le_bytes());
        b[24..32].copy_from_slice(&self.u.to_le_bytes());
        b[32..160].copy_from_slice(&self.value);
        b[160..224].copy_from_slice(&self.desc);
        b[224..304].copy_from_slice(&self.mimetype);
        b[304..312].copy_from_slice(&self.apple);
        b[312..432].copy_from_slice(&self.ext);
        b
    }

    /// O inverso de [`Magic::image`] (pra ler um `.mgc`).
    pub fn from_image(b: &[u8], swap: bool) -> Magic {
        let u16_at = |o: usize| {
            let v = [b[o], b[o + 1]];
            if swap { u16::from_be_bytes(v) } else { u16::from_le_bytes(v) }
        };
        let u32_at = |o: usize| {
            let v = [b[o], b[o + 1], b[o + 2], b[o + 3]];
            if swap { u32::from_be_bytes(v) } else { u32::from_le_bytes(v) }
        };
        let mut m = Magic::zeroed();
        m.flag = u16_at(0);
        m.cont_level = b[2];
        m.factor = b[3];
        m.reln = b[4];
        m.vallen = b[5];
        m.typ = b[6];
        m.in_type = b[7];
        m.in_op = b[8];
        m.mask_op = b[9];
        m.cond = b[10];
        m.factor_op = b[11];
        m.offset = u32_at(12) as i32;
        m.in_offset = u32_at(16) as i32;
        m.lineno = u32_at(20);
        let mut u = [0u8; 8];
        u.copy_from_slice(&b[24..32]);
        m.u = if swap {
            if is_string_type(m.typ) {
                (u64::from(u32_at(28)) << 32) | u64::from(u32_at(24))
            } else {
                u64::from_be_bytes(u)
            }
        } else {
            u64::from_le_bytes(u)
        };
        m.value.copy_from_slice(&b[32..160]);
        if swap && !is_string_type(m.typ) {
            let mut q = [0u8; 8];
            q.copy_from_slice(&b[32..40]);
            m.set_value_q(u64::from_be_bytes(q));
        }
        m.desc.copy_from_slice(&b[160..224]);
        m.mimetype.copy_from_slice(&b[224..304]);
        m.apple.copy_from_slice(&b[304..312]);
        m.ext.copy_from_slice(&b[312..432]);
        m
    }
}

/// `IS_STRING()`.
pub fn is_string_type(t: u8) -> bool {
    matches!(
        t,
        FILE_STRING | FILE_PSTRING | FILE_BESTRING16 | FILE_LESTRING16 | FILE_REGEX | FILE_SEARCH | FILE_INDIRECT | FILE_NAME | FILE_USE | FILE_OCTAL
    )
}

/// `typesize()`.
pub fn typesize(t: u8) -> u64 {
    match t {
        FILE_BYTE => 1,
        FILE_SHORT | FILE_LESHORT | FILE_BESHORT | FILE_MSDOSDATE | FILE_BEMSDOSDATE | FILE_LEMSDOSDATE | FILE_MSDOSTIME
        | FILE_BEMSDOSTIME | FILE_LEMSDOSTIME => 2,
        FILE_LONG | FILE_LELONG | FILE_BELONG | FILE_MELONG => 4,
        FILE_DATE | FILE_LEDATE | FILE_BEDATE | FILE_MEDATE | FILE_LDATE | FILE_LELDATE | FILE_BELDATE | FILE_MELDATE | FILE_FLOAT
        | FILE_BEFLOAT | FILE_LEFLOAT | FILE_BEID3 | FILE_LEID3 => 4,
        FILE_QUAD | FILE_BEQUAD | FILE_LEQUAD | FILE_QDATE | FILE_LEQDATE | FILE_BEQDATE | FILE_QLDATE | FILE_LEQLDATE | FILE_BEQLDATE
        | FILE_QWDATE | FILE_LEQWDATE | FILE_BEQWDATE | FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE | FILE_OFFSET | FILE_BEVARINT
        | FILE_LEVARINT => 8,
        FILE_GUID => 16,
        _ => FILE_BADSIZE,
    }
}

/// `file_signextend()`; `Err` é o `FILE_BADSIZE` com o aviso "cannot happen".
pub fn signextend(m: &Magic, v: u64) -> Result<u64, ()> {
    if m.flag & UNSIGNED != 0 {
        return Ok(v);
    }
    Ok(match m.typ {
        FILE_BYTE => v as i8 as i64 as u64,
        FILE_SHORT | FILE_BESHORT | FILE_LESHORT => v as i16 as i64 as u64,
        FILE_DATE | FILE_BEDATE | FILE_LEDATE | FILE_MEDATE | FILE_LDATE | FILE_BELDATE | FILE_LELDATE | FILE_MELDATE | FILE_LONG
        | FILE_BELONG | FILE_LELONG | FILE_MELONG | FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT | FILE_MSDOSDATE | FILE_BEMSDOSDATE
        | FILE_LEMSDOSDATE | FILE_MSDOSTIME | FILE_BEMSDOSTIME | FILE_LEMSDOSTIME => v as i32 as i64 as u64,
        FILE_QUAD | FILE_BEQUAD | FILE_LEQUAD | FILE_QDATE | FILE_QLDATE | FILE_QWDATE | FILE_BEQDATE | FILE_BEQLDATE | FILE_BEQWDATE
        | FILE_LEQDATE | FILE_LEQLDATE | FILE_LEQWDATE | FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE | FILE_OFFSET | FILE_BEVARINT
        | FILE_LEVARINT => v,
        FILE_STRING | FILE_PSTRING | FILE_BESTRING16 | FILE_LESTRING16 | FILE_REGEX | FILE_SEARCH | FILE_DEFAULT | FILE_INDIRECT
        | FILE_NAME | FILE_USE | FILE_CLEAR | FILE_DER | FILE_GUID | FILE_OCTAL => v,
        _ => return Err(()),
    })
}

/// `nonmagic()`: comprimento "real" de uma regex pra força da regra.
pub fn nonmagic(s: &[u8]) -> usize {
    let mut rv = 0usize;
    let mut p = 0usize;
    while p < s.len() && s[p] != 0 {
        match s[p] {
            b'\\' => {
                p += 1;
                if at(s, p) == 0 {
                    p -= 1;
                }
                rv += 1;
            }
            b'?' | b'*' | b'.' | b'+' | b'^' | b'$' => {}
            b'[' => {
                while at(s, p) != 0 && s[p] != b']' {
                    p += 1;
                }
                p = p.wrapping_sub(1);
            }
            b'{' => {
                while at(s, p) != 0 && s[p] != b'}' {
                    p += 1;
                }
                if at(s, p) == 0 {
                    p = p.wrapping_sub(1);
                }
            }
            _ => rv += 1,
        }
        p = p.wrapping_add(1);
    }
    if rv == 0 { 1 } else { rv }
}

/// `apprentice_magic_strength_1()`.
fn magic_strength_1(m: &Magic) -> isize {
    const MULT: usize = 10;
    let mut val: isize = 2 * MULT as isize;
    let vallen = usize::from(m.vallen);
    match m.typ {
        FILE_DEFAULT => return 0,
        FILE_BYTE | FILE_SHORT | FILE_LESHORT | FILE_BESHORT | FILE_LONG | FILE_LELONG | FILE_BELONG | FILE_MELONG | FILE_DATE
        | FILE_LEDATE | FILE_BEDATE | FILE_MEDATE | FILE_LDATE | FILE_LELDATE | FILE_BELDATE | FILE_MELDATE | FILE_FLOAT | FILE_BEFLOAT
        | FILE_LEFLOAT | FILE_QUAD | FILE_BEQUAD | FILE_LEQUAD | FILE_QDATE | FILE_LEQDATE | FILE_BEQDATE | FILE_QLDATE
        | FILE_LEQLDATE | FILE_BEQLDATE | FILE_QWDATE | FILE_LEQWDATE | FILE_BEQWDATE | FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE
        | FILE_BEVARINT | FILE_LEVARINT | FILE_GUID | FILE_BEID3 | FILE_LEID3 | FILE_OFFSET | FILE_MSDOSDATE | FILE_BEMSDOSDATE
        | FILE_LEMSDOSDATE | FILE_MSDOSTIME | FILE_BEMSDOSTIME | FILE_LEMSDOSTIME => {
            val += (typesize(m.typ) as usize * MULT) as isize;
        }
        FILE_PSTRING | FILE_STRING | FILE_OCTAL => val += (vallen * MULT) as isize,
        FILE_BESTRING16 | FILE_LESTRING16 => val += (vallen * MULT / 2) as isize,
        FILE_SEARCH => {
            if vallen != 0 {
                val += (vallen * (MULT / vallen).max(1)) as isize;
            }
        }
        FILE_REGEX => {
            let v = nonmagic(m.value_str());
            val += (v * (MULT / v).max(1)) as isize;
        }
        FILE_INDIRECT | FILE_NAME | FILE_USE | FILE_CLEAR => {}
        FILE_DER => val += MULT as isize,
        _ => {}
    }
    match m.reln {
        b'x' | b'!' => val = 0,
        b'=' => val += MULT as isize,
        b'>' | b'<' => val -= 2 * MULT as isize,
        b'^' | b'&' => val -= MULT as isize,
        _ => {}
    }
    val
}

/// `file_magic_strength()`.
pub fn magic_strength(m: &Magic) -> usize {
    let mut val = magic_strength_1(m);
    let f = isize::from(m.factor);
    match m.factor_op {
        b'+' => val += f,
        b'-' => val -= f,
        b'*' => val *= f,
        b'/' => {
            if f != 0 {
                val /= f;
            }
        }
        _ => {}
    }
    if val <= 0 {
        val = 1;
    }
    if m.desc[0] == 0 {
        val += 1;
    }
    val as usize
}

/// Uma regra completa: a entrada de nível 0 e as continuações (`struct magic_entry`).
#[derive(Clone, Debug)]
pub struct Entry {
    pub mp: Vec<Magic>,
}

/// O banco carregado de um arquivo ou do embutido: os dois conjuntos (`MAGIC_SETS`) já
/// ordenados e achatados, como o `struct magic_map`.
#[derive(Clone, Debug, Default)]
pub struct MagicMap {
    pub sets: [Vec<Magic>; 2],
}

impl MagicMap {
    /// O `.mgc` deste mapa (cabeçalho + conjuntos), como o `apprentice_compile` grava.
    pub fn compile(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(MAGIC_SIZE * (1 + self.sets[0].len() + self.sets[1].len()));
        let mut hdr = [0u8; MAGIC_SIZE];
        hdr[0..4].copy_from_slice(&MAGICNO.to_le_bytes());
        hdr[4..8].copy_from_slice(&VERSIONNO.to_le_bytes());
        hdr[8..12].copy_from_slice(&(self.sets[0].len() as u32).to_le_bytes());
        hdr[12..16].copy_from_slice(&(self.sets[1].len() as u32).to_le_bytes());
        out.extend_from_slice(&hdr);
        for set in &self.sets {
            for m in set {
                out.extend_from_slice(&m.image());
            }
        }
        out
    }

    /// Lê um `.mgc` (`check_buffer`). `Err` traz a mensagem do libmagic.
    pub fn from_compiled(data: &[u8], dbname: &str) -> Result<MagicMap, String> {
        let entries = data.len() / MAGIC_SIZE;
        if entries < 3 {
            return Err(format!("Too few magic entries {entries} in `{dbname}'"));
        }
        if entries * MAGIC_SIZE != data.len() {
            return Err(format!("Size of `{dbname}' {} is not a multiple of {MAGIC_SIZE}", data.len()));
        }
        let w = |o: usize| u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        let swap = if w(0) == MAGICNO {
            false
        } else if w(0).swap_bytes() == MAGICNO {
            true
        } else {
            return Err(format!("bad magic in `{dbname}'"));
        };
        let fix = |v: u32| if swap { v.swap_bytes() } else { v };
        let version = fix(w(4));
        if version != VERSIONNO {
            return Err(format!(
                "File 5.46 supports only version {VERSIONNO} magic files. `{dbname}' is version {version}"
            ));
        }
        let n0 = fix(w(8)) as usize;
        let n1 = fix(w(12)) as usize;
        if entries != n0 + n1 + 1 {
            return Err(format!("Inconsistent entries in `{dbname}' {entries} != {}", n0 + n1 + 1));
        }
        let mut map = MagicMap::default();
        for i in 0..n0 + n1 {
            let o = MAGIC_SIZE * (i + 1);
            let m = Magic::from_image(&data[o..o + MAGIC_SIZE], swap);
            map.sets[usize::from(i >= n0)].push(m);
        }
        Ok(map)
    }
}

/// Avisos e erros do carregamento, com o arquivo e a linha como o libmagic imprime.
#[derive(Clone, Debug, Default)]
pub struct LoadReport {
    /// Linhas de aviso já formatadas (`<arquivo>, <linha>: Warning: ...`).
    pub warnings: Vec<String>,
    /// Primeiro erro (o `file_error` só guarda o primeiro).
    pub error: Option<String>,
    pub errs: usize,
}

/// Estado do carregamento (a parte do `struct magic_set` que o `apprentice.c` usa).
pub struct Loader {
    /// `MAGIC_CHECK`: liga as verificações (o `apprentice_load` sempre liga).
    pub check: bool,
    /// `action == FILE_COMPILE`: liga os avisos de escape do `getstr`.
    pub compile: bool,
    pub magwarn_max: usize,
    magwarn: usize,
    file: String,
    line: usize,
    li_last_cond: Vec<u8>,
    last_cont_level: u32,
    pub report: LoadReport,
    sets: [Vec<Entry>; 2],
}

impl Default for Loader {
    fn default() -> Self {
        Loader::new()
    }
}

macro_rules! eatab {
    ($s:expr, $l:expr) => {
        while at($s, $l) < 0x80 && is_space(at($s, $l)) {
            $l += 1;
        }
    };
}

/// Saída de `parse()`.
enum ParseResult {
    Ok,
    /// Nova entrada de nível 0 com outra em andamento: guardar a atual e reprocessar a linha.
    NewEntry,
    Err,
}

impl Loader {
    pub fn new() -> Loader {
        Loader {
            check: true,
            compile: false,
            magwarn_max: 64,
            magwarn: 0,
            file: String::new(),
            line: 0,
            li_last_cond: Vec::new(),
            last_cont_level: 0,
            report: LoadReport::default(),
            sets: [Vec::new(), Vec::new()],
        }
    }

    /// `file_magwarn()`.
    fn magwarn(&mut self, msg: String) {
        self.magwarn += 1;
        if self.magwarn == self.magwarn_max {
            self.report.warnings.push(format!("{}, {}: Maximum number of warnings ({}) exceeded.", self.file, self.line, self.magwarn_max));
            self.report.warnings.push(format!("{}, {}: Additional warnings are suppressed.", self.file, self.line));
        }
        if self.magwarn >= self.magwarn_max {
            return;
        }
        self.report.warnings.push(format!("{}, {}: Warning: {msg}", self.file, self.line));
    }

    /// `file_error()` (só o primeiro conta).
    fn error(&mut self, msg: String) {
        if self.report.error.is_none() {
            self.report.error = Some(msg);
        }
    }

    /// `file_magerror()`: erro com o número da linha.
    fn magerror(&mut self, msg: String) {
        let line = self.line;
        self.error(format!("line {line}: {msg}"));
    }

    /// `load_1()`: lê um arquivo de regras. `name` é o caminho como o libmagic o vê (entra nas
    /// mensagens e na descrição vazia).
    pub fn load_text(&mut self, name: &str, text: &[u8]) {
        self.file = name.to_string();
        let mut me: Option<Entry> = None;
        let mut lineno = 0usize;
        self.line = 1;
        let mut rest = text;
        while !rest.is_empty() {
            if self.magwarn >= self.magwarn_max {
                break;
            }
            let (raw, next) = match rest.iter().position(|&b| b == b'\n') {
                Some(n) => (&rest[..=n], &rest[n + 1..]),
                None => (rest, &rest[rest.len()..]),
            };
            rest = next;
            let len = raw.len();
            let mut line = raw.to_vec();
            if line.last() == Some(&b'\n') {
                lineno += 1;
                let l = line.len();
                line[l - 1] = 0;
            }
            match at(&line, 0) {
                0 | b'#' => {}
                b'!' if at(&line, 1) == b':' => {
                    const BANG: [&str; 4] = ["mime", "apple", "ext", "strength"];
                    let found = BANG.iter().position(|b| len - 2 > b.len() && line[2..].starts_with(b.as_bytes()));
                    match found {
                        None => {
                            let shown = String::from_utf8_lossy(cstr(&line)).into_owned();
                            self.error(format!("Unknown !: entry `{shown}'"));
                            self.report.errs += 1;
                        }
                        Some(i) => match me.as_mut() {
                            None => {
                                self.error(format!("No current entry for :!{} type", BANG[i]));
                                self.report.errs += 1;
                            }
                            Some(entry) => {
                                let arg = &line[2 + BANG[i].len()..];
                                let arglen = len - BANG[i].len() - 2;
                                let ok = match i {
                                    0 => self.parse_extra(entry, arg, arglen, Field::Mime),
                                    1 => self.parse_extra(entry, arg, arglen, Field::Apple),
                                    2 => self.parse_extra(entry, arg, arglen, Field::Ext),
                                    _ => self.parse_strength(entry, arg),
                                };
                                if !ok {
                                    self.report.errs += 1;
                                }
                            }
                        },
                    }
                }
                _ => loop {
                    match self.parse(&mut me, &line, lineno) {
                        ParseResult::Ok => break,
                        ParseResult::NewEntry => {
                            if let Some(e) = me.take() {
                                self.addentry(e);
                            }
                            continue;
                        }
                        ParseResult::Err => {
                            self.report.errs += 1;
                            break;
                        }
                    }
                },
            }
            self.line += 1;
        }
        if let Some(e) = me.take() {
            self.addentry(e);
        }
    }

    fn addentry(&mut self, e: Entry) {
        let i = usize::from(e.mp[0].typ == FILE_NAME);
        self.sets[i].push(e);
    }

    /// Fim do `apprentice_load()`: tipo de teste, ordenação e achatamento.
    pub fn finish(mut self) -> (MagicMap, LoadReport) {
        let mut map = MagicMap::default();
        for j in 0..2 {
            let mut entries = std::mem::take(&mut self.sets[j]);
            for e in entries.iter_mut() {
                set_text_binary(e);
            }
            let keyed: Vec<(usize, [u8; MAGIC_SIZE], Entry)> = entries
                .into_iter()
                .map(|e| {
                    let mut img = e.mp[0].image();
                    img[20..24].fill(0);
                    (magic_strength(&e.mp[0]), img, e)
                })
                .collect();
            let mut keyed = keyed;
            keyed.sort_by(|a, b| apprentice_sort(a, b));
            for (_, _, e) in keyed {
                map.sets[j].extend(e.mp);
            }
        }
        (map, self.report)
    }

    /// `parse()`: uma linha de regra.
    fn parse(&mut self, me: &mut Option<Entry>, line: &[u8], lineno: usize) -> ParseResult {
        let s = line;
        let mut l = 0usize;
        let mut cont_level: u32 = 0;
        while at(s, l) == b'>' {
            l += 1;
            cont_level += 1;
        }
        if cont_level == 0 || cont_level > self.last_cont_level {
            self.check_mem(cont_level as usize);
        }
        self.last_cont_level = cont_level;
        let idx;
        if cont_level != 0 {
            let Some(entry) = me.as_mut() else {
                self.magerror("No current entry for continuation".into());
                return ParseResult::Err;
            };
            let prev = entry.mp[entry.mp.len() - 1].cont_level;
            let diff = cont_level as i32 - i32::from(prev);
            if diff > 1 {
                self.magwarn(format!(
                    "New continuation level {cont_level} is more than one larger than current level {prev}"
                ));
            }
            let mut m = Magic::zeroed();
            m.cont_level = cont_level as u8;
            entry.mp.push(m);
            idx = entry.mp.len() - 1;
        } else {
            if me.is_some() {
                return ParseResult::NewEntry;
            }
            let m = Magic::zeroed();
            *me = Some(Entry { mp: vec![m] });
            idx = 0;
        }
        let entry = me.as_mut().map(|e| &mut e.mp).expect("entrada");
        let mut m = entry[idx].clone();
        m.lineno = lineno as u32;
        let r = self.parse_into(&mut m, s, &mut l, cont_level);
        let entry = me.as_mut().map(|e| &mut e.mp).expect("entrada");
        entry[idx] = m;
        if r { ParseResult::Ok } else { ParseResult::Err }
    }

    /// Corpo do `parse()` depois da criação da entrada.
    fn parse_into(&mut self, m: &mut Magic, s: &[u8], l: &mut usize, cont_level: u32) -> bool {
        if at(s, *l) == b'&' {
            *l += 1;
            m.flag |= OFFADD;
        }
        if at(s, *l) == b'(' {
            *l += 1;
            m.flag |= INDIR;
            if m.flag & OFFADD != 0 {
                m.flag = (m.flag & !OFFADD) | INDIROFFADD;
            }
            if at(s, *l) == b'&' {
                *l += 1;
                m.flag |= OFFADD;
            }
        }
        if m.cont_level == 0 && m.flag & (OFFADD | INDIROFFADD) != 0 {
            if self.check {
                self.magwarn("relative offset at level 0".into());
            }
            return false;
        }
        if at(s, *l) == b'-' || at(s, *l) == b'+' {
            m.flag |= if at(s, *l) == b'-' { OFFNEGATIVE } else { OFFPOSITIVE };
            *l += 1;
        }
        let c = strtol(tail(s, *l), 0);
        m.offset = c.value as i32;
        if c.used == 0 {
            if self.check {
                let rest = String::from_utf8_lossy(cstr(tail(s, *l))).into_owned();
                self.magwarn(format!("offset `{rest}' invalid"));
            }
            return false;
        }
        *l += c.used;

        if m.flag & INDIR != 0 {
            m.in_type = FILE_LONG;
            m.in_offset = 0;
            m.in_op = 0;
            if at(s, *l) == b'.' || at(s, *l) == b',' {
                if at(s, *l) == b',' {
                    m.in_op |= FILE_OPSIGNED;
                }
                *l += 1;
                m.in_type = match at(s, *l) {
                    b'l' => FILE_LELONG,
                    b'L' => FILE_BELONG,
                    b'm' => FILE_MELONG,
                    b'h' | b's' => FILE_LESHORT,
                    b'H' | b'S' => FILE_BESHORT,
                    b'c' | b'b' | b'C' | b'B' => FILE_BYTE,
                    b'e' | b'f' | b'g' => FILE_LEDOUBLE,
                    b'E' | b'F' | b'G' => FILE_BEDOUBLE,
                    b'i' => FILE_LEID3,
                    b'I' => FILE_BEID3,
                    b'o' => FILE_OCTAL,
                    b'q' => FILE_LEQUAD,
                    b'Q' => FILE_BEQUAD,
                    other => {
                        if self.check {
                            self.magwarn(format!("indirect offset type `{}' invalid", other as char));
                        }
                        return false;
                    }
                };
                *l += 1;
            }
            if at(s, *l) == b'~' {
                m.in_op |= FILE_OPINVERSE;
                *l += 1;
            }
            if let Some(op) = get_op(at(s, *l)) {
                m.in_op |= op;
                *l += 1;
            }
            if at(s, *l) == b'(' {
                m.in_op |= FILE_OPINDIRECT;
                *l += 1;
            }
            if is_digit(at(s, *l)) || at(s, *l) == b'-' {
                let c = strtol(tail(s, *l), 0);
                m.in_offset = c.value as i32;
                if c.used == 0 {
                    if self.check {
                        let rest = String::from_utf8_lossy(cstr(tail(s, *l))).into_owned();
                        self.magwarn(format!("in_offset `{rest}' invalid"));
                    }
                    return false;
                }
                *l += c.used;
            }
            let close1 = at(s, *l);
            *l += 1;
            let bad = close1 != b')' || {
                if m.in_op & FILE_OPINDIRECT != 0 {
                    let c2 = at(s, *l);
                    *l += 1;
                    c2 != b')'
                } else {
                    false
                }
            };
            if bad {
                if self.check {
                    self.magwarn("missing ')' in indirect offset".into());
                }
                return false;
            }
        }
        eatab!(s, *l);

        // ENABLE_CONDITIONALS
        m.cond = get_cond(s, l);
        if !self.check_cond(m.cond, cont_level) {
            return false;
        }
        eatab!(s, *l);

        // Tipo.
        if at(s, *l) == b'u' {
            let (t, adv) = get_type(TYPE_TBL_REF, tail(s, *l + 1));
            m.typ = t;
            if m.typ == FILE_INVALID {
                let (t, adv) = get_standard_integer_type(tail(s, *l));
                m.typ = t;
                if t != FILE_INVALID {
                    *l += adv;
                }
            } else {
                *l += 1 + adv;
            }
            if m.typ != FILE_INVALID {
                m.flag |= UNSIGNED;
            }
        } else {
            let (t, adv) = get_type(TYPE_TBL_REF, tail(s, *l));
            m.typ = t;
            if m.typ == FILE_INVALID {
                if at(s, *l) == b'd' {
                    let (t, adv) = get_standard_integer_type(tail(s, *l));
                    m.typ = t;
                    if t != FILE_INVALID {
                        *l += adv;
                    }
                } else if at(s, *l) == b's' && !is_alpha(at(s, *l + 1)) {
                    m.typ = FILE_STRING;
                    *l += 1;
                }
            } else {
                *l += adv;
            }
        }
        if m.typ == FILE_INVALID {
            let (t, adv) = get_special_type(tail(s, *l));
            m.typ = t;
            if t != FILE_INVALID {
                *l += adv;
            }
        }
        if m.typ == FILE_INVALID {
            if self.check {
                let rest = String::from_utf8_lossy(cstr(tail(s, *l))).into_owned();
                self.magwarn(format!("type `{rest}' invalid"));
            }
            return false;
        }
        if m.typ == FILE_NAME && cont_level != 0 {
            if self.check {
                let rest = String::from_utf8_lossy(cstr(tail(s, *l))).into_owned();
                self.magwarn(format!("`name{rest}' entries can only be declared at top level"));
            }
            return false;
        }

        m.mask_op = 0;
        if at(s, *l) == b'~' {
            if !is_string_type(m.typ) {
                m.mask_op |= FILE_OPINVERSE;
            } else if self.check {
                self.magwarn("'~' invalid for string types".into());
            }
            *l += 1;
        }
        m.set_str_range(0);
        m.set_str_flags(if m.typ == FILE_PSTRING { PSTRING_1_LE } else { 0 });
        if let Some(op) = get_op(at(s, *l)) {
            if is_string_type(m.typ) {
                if op != FILE_OPDIVIDE {
                    if self.check {
                        // O C imprime `*t` (lixo do strtol anterior); o efeito que importa é o erro.
                        self.magwarn(format!("invalid string/indirect op: `{}'", at(s, *l) as char));
                    }
                    return false;
                }
                let r = if m.typ == FILE_INDIRECT { self.parse_indirect_modifier(m, s, l) } else { self.parse_string_modifier(m, s, l) };
                if !r {
                    return false;
                }
            } else {
                self.parse_op_modifier(m, s, l, op);
            }
        }
        eatab!(s, *l);

        match at(s, *l) {
            b'>' | b'<' => {
                m.reln = at(s, *l);
                *l += 1;
                if at(s, *l) == b'=' {
                    if self.check {
                        self.magwarn(format!("{}= not supported", m.reln as char));
                        return false;
                    }
                    *l += 1;
                }
            }
            b'&' | b'^' | b'=' => {
                m.reln = at(s, *l);
                *l += 1;
                if at(s, *l) == b'=' {
                    *l += 1;
                }
            }
            b'!' => {
                m.reln = b'!';
                *l += 1;
            }
            _ => {
                m.reln = b'=';
                if at(s, *l) == b'x' && ((at(s, *l + 1) < 0x80 && is_space(at(s, *l + 1))) || at(s, *l + 1) == 0) {
                    m.reln = b'x';
                    *l += 1;
                }
            }
        }
        if m.reln != b'x' && !self.getvalue(m, s, l) {
            return false;
        }

        eatab!(s, *l);
        if at(s, *l) == 0x08 {
            *l += 1;
            m.flag |= NOSPACE;
        } else if at(s, *l) == b'\\' && at(s, *l + 1) == b'b' {
            *l += 2;
            m.flag |= NOSPACE;
        }
        let rest = cstr(tail(s, *l));
        let n = rest.len().min(MAXDESC);
        m.desc[..n].copy_from_slice(&rest[..n]);
        if n == 0 {
            // O nome do arquivo vai escondido depois do NUL (só pra depuração no C, mas pesa no
            // desempate da ordenação).
            let f = self.file.as_bytes();
            let k = f.len().min(MAXDESC - 2);
            m.desc[1..1 + k].copy_from_slice(&f[..k]);
        }
        // O laço do C copia até o NUL ou 64 bytes; com 63 caracteres ou mais o contador chega a
        // 64 e o aviso de truncado sai (mesmo quando nada foi cortado).
        if rest.len() >= MAXDESC - 1 {
            m.desc[MAXDESC - 1] = 0;
            if self.check {
                let d = String::from_utf8_lossy(cstr(&m.desc)).into_owned();
                self.magwarn(format!("description `{d}' truncated"));
            }
        }
        if self.check && !self.check_format(m) {
            return false;
        }
        m.mimetype[0] = 0;
        true
    }

    /// `file_check_mem()` no carregamento: só o estado das condicionais por nível.
    fn check_mem(&mut self, level: usize) {
        if self.li_last_cond.len() <= level {
            self.li_last_cond.resize(level + 20, COND_NONE);
        }
        self.li_last_cond[level] = COND_NONE;
    }

    /// `check_cond()`.
    fn check_cond(&mut self, cond: u8, cont_level: u32) -> bool {
        let lvl = cont_level as usize;
        if self.li_last_cond.len() <= lvl {
            self.li_last_cond.resize(lvl + 20, COND_NONE);
        }
        let mut last = self.li_last_cond[lvl];
        match cond {
            COND_IF => {
                if last != COND_NONE && last != COND_ELIF {
                    if self.check {
                        self.magwarn("syntax error: `if'".into());
                    }
                    return false;
                }
                last = COND_IF;
            }
            COND_ELIF => {
                if last != COND_IF && last != COND_ELIF {
                    if self.check {
                        self.magwarn("syntax error: `elif'".into());
                    }
                    return false;
                }
                last = COND_ELIF;
            }
            COND_ELSE => {
                if last != COND_IF && last != COND_ELIF {
                    if self.check {
                        self.magwarn("syntax error: `else'".into());
                    }
                    return false;
                }
                last = COND_NONE;
            }
            _ => last = COND_NONE,
        }
        self.li_last_cond[lvl] = last;
        true
    }

    fn parse_indirect_modifier(&mut self, m: &mut Magic, s: &[u8], l: &mut usize) -> bool {
        loop {
            *l += 1;
            let c = at(s, *l);
            if is_space(c) {
                break;
            }
            if c == b'r' {
                m.set_str_flags(m.str_flags() | INDIRECT_RELATIVE);
            } else {
                if self.check {
                    self.magwarn(format!("indirect modifier `{}' invalid", c as char));
                }
                return false;
            }
        }
        true
    }

    fn parse_op_modifier(&mut self, m: &mut Magic, s: &[u8], l: &mut usize, op: u8) {
        *l += 1;
        m.mask_op |= op;
        let c = strtoull(tail(s, *l), 0);
        *l += c.used;
        m.u = match signextend(m, c.value) {
            Ok(v) => v,
            Err(()) => {
                if self.check {
                    self.magwarn(format!("cannot happen: m->type={}\n", m.typ));
                }
                FILE_BADSIZE
            }
        };
        eatsize(s, l);
    }

    fn parse_string_modifier(&mut self, m: &mut Magic, s: &[u8], l: &mut usize) -> bool {
        let mut have_range = false;
        loop {
            *l += 1;
            let c = at(s, *l);
            if is_space(c) {
                break;
            }
            let mut bad = false;
            match c {
                b'0'..=b'9' => {
                    if have_range && self.check {
                        self.magwarn("multiple ranges".into());
                    }
                    have_range = true;
                    let conv = strtoul(tail(s, *l), 0);
                    m.set_str_range(conv.value as u32);
                    if m.str_range() == 0 {
                        self.magwarn("zero range".into());
                    }
                    *l += conv.used - 1;
                }
                b'W' => m.set_str_flags(m.str_flags() | STRING_COMPACT_WHITESPACE),
                b'w' => m.set_str_flags(m.str_flags() | STRING_COMPACT_OPTIONAL_WHITESPACE),
                b'c' => m.set_str_flags(m.str_flags() | STRING_IGNORE_LOWERCASE),
                b'C' => m.set_str_flags(m.str_flags() | STRING_IGNORE_UPPERCASE),
                b's' => m.set_str_flags(m.str_flags() | REGEX_OFFSET_START),
                b'b' => m.set_str_flags(m.str_flags() | STRING_BINTEST),
                b't' => m.set_str_flags(m.str_flags() | STRING_TEXTTEST),
                b'T' => m.set_str_flags(m.str_flags() | STRING_TRIM),
                b'f' => m.set_str_flags(m.str_flags() | STRING_FULL_WORD),
                b'B' => {
                    if m.typ != FILE_PSTRING {
                        bad = true;
                    } else {
                        m.set_str_flags((m.str_flags() & !PSTRING_LEN) | PSTRING_1_LE);
                    }
                }
                b'H' => {
                    if m.typ != FILE_PSTRING {
                        bad = true;
                    } else {
                        m.set_str_flags((m.str_flags() & !PSTRING_LEN) | PSTRING_2_BE);
                    }
                }
                b'h' => {
                    if m.typ != FILE_PSTRING {
                        bad = true;
                    } else {
                        m.set_str_flags((m.str_flags() & !PSTRING_LEN) | PSTRING_2_LE);
                    }
                }
                b'L' => {
                    if m.typ != FILE_PSTRING {
                        bad = true;
                    } else {
                        m.set_str_flags((m.str_flags() & !PSTRING_LEN) | PSTRING_4_BE);
                    }
                }
                b'l' => {
                    if m.typ != FILE_PSTRING && m.typ != FILE_REGEX {
                        bad = true;
                    } else {
                        m.set_str_flags((m.str_flags() & !PSTRING_LEN) | PSTRING_4_LE);
                    }
                }
                b'J' => {
                    if m.typ != FILE_PSTRING {
                        bad = true;
                    } else {
                        m.set_str_flags(m.str_flags() | PSTRING_LENGTH_INCLUDES_ITSELF);
                    }
                }
                _ => bad = true,
            }
            if bad {
                if self.check {
                    self.magwarn(format!("string modifier `{}' invalid", c as char));
                }
                return false;
            }
            if at(s, *l + 1) == b'/' && !is_space(at(s, *l + 2)) {
                *l += 1;
            }
        }
        if !self.string_modifier_check(m) {
            return false;
        }
        true
    }

    fn string_modifier_check(&mut self, m: &mut Magic) -> bool {
        if !self.check {
            return true;
        }
        let f = m.str_flags();
        if (m.typ != FILE_REGEX || f & REGEX_LINE_COUNT == 0) && (m.typ != FILE_PSTRING && f & PSTRING_LEN != 0) {
            self.magwarn("'/BHhLl' modifiers are only allowed for pascal strings\n".into());
            return false;
        }
        match m.typ {
            FILE_BESTRING16 | FILE_LESTRING16 => {
                if f != 0 {
                    self.magwarn("no modifiers allowed for 16-bit strings\n".into());
                    return false;
                }
            }
            FILE_STRING | FILE_PSTRING => {
                if f & REGEX_OFFSET_START != 0 {
                    self.magwarn("'/s' only allowed on regex and search\n".into());
                    return false;
                }
            }
            FILE_SEARCH => {
                if m.str_range() == 0 {
                    self.magwarn(format!("missing range; defaulting to {STRING_DEFAULT_RANGE}\n"));
                    m.set_str_range(STRING_DEFAULT_RANGE);
                    return false;
                }
            }
            FILE_REGEX => {
                if f & STRING_COMPACT_WHITESPACE != 0 {
                    self.magwarn("'/W' not allowed on regex\n".into());
                    return false;
                }
                if f & STRING_COMPACT_OPTIONAL_WHITESPACE != 0 {
                    self.magwarn("'/w' not allowed on regex\n".into());
                    return false;
                }
            }
            _ => {
                self.magwarn(format!("coding error: m->type={}\n", m.typ));
                return false;
            }
        }
        true
    }

    /// `getvalue()`.
    fn getvalue(&mut self, m: &mut Magic, s: &[u8], l: &mut usize) -> bool {
        match m.typ {
            FILE_BESTRING16 | FILE_LESTRING16 | FILE_STRING | FILE_PSTRING | FILE_REGEX | FILE_SEARCH | FILE_NAME | FILE_USE | FILE_DER
            | FILE_OCTAL => {
                let warn = self.compile;
                match self.getstr(m, s, *l, warn) {
                    Some(end) => *l = end,
                    None => {
                        if self.check {
                            let v = String::from_utf8_lossy(m.value_str()).into_owned();
                            self.magwarn(format!("cannot get string from `{v}'"));
                        }
                        return false;
                    }
                }
                if m.typ == FILE_REGEX {
                    let pat = m.value_str().to_vec();
                    return self.regcomp_check(&pat);
                }
                return true;
            }
            _ => {
                if m.reln == b'x' {
                    return true;
                }
            }
        }
        match m.typ {
            FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT => {
                let c = strtof(tail(s, *l));
                m.value[..4].copy_from_slice(&c.value.to_le_bytes());
                if !c.overflow {
                    *l += c.used;
                }
                true
            }
            FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => {
                let c = strtod(tail(s, *l));
                m.value[..8].copy_from_slice(&c.value.to_le_bytes());
                if !c.overflow {
                    *l += c.used;
                }
                true
            }
            FILE_GUID => {
                let Some(g) = parse_guid(tail(s, *l)) else { return false };
                m.value[..16].copy_from_slice(&g);
                *l += 36;
                true
            }
            _ => {
                let c = strtoull(tail(s, *l), 0);
                let mut ull = c.value;
                m.set_value_q(match signextend(m, ull) {
                    Ok(v) => v,
                    Err(()) => {
                        if self.check {
                            self.magwarn(format!("cannot happen: m->type={}\n", m.typ));
                        }
                        FILE_BADSIZE
                    }
                });
                if c.used == 0 {
                    let rest = String::from_utf8_lossy(cstr(tail(s, *l))).into_owned();
                    self.magwarn(format!("Unparsable number `{rest}'"));
                    return false;
                }
                let ts = typesize(m.typ);
                if ts == FILE_BADSIZE {
                    self.magwarn(format!("Expected numeric type got `{}'", TYPE_TBL.get(usize::from(m.typ)).map(|t| t.0).unwrap_or("?")));
                    return false;
                }
                let mut q = *l;
                while is_space(at(s, q)) {
                    q += 1;
                }
                if at(s, q) == b'-' && ull != u64::MAX {
                    ull = (ull as i64).wrapping_neg() as u64;
                }
                let (x, y) = match ts {
                    1 => {
                        let x = ull & !0xff;
                        (x, (x & !0xff) != !0xff)
                    }
                    2 => {
                        let x = ull & !0xffff;
                        (x, (x & !0xffff) != !0xffff)
                    }
                    4 => {
                        let x = ull & !0xffff_ffff;
                        (x, (x & !0xffff_ffff) != !0xffff_ffff)
                    }
                    _ => (0, false),
                };
                if x != 0 && y {
                    self.magwarn(format!(
                        "Overflow for numeric type `{}' value {:#x}",
                        TYPE_TBL.get(usize::from(m.typ)).map(|t| t.0).unwrap_or("?"),
                        ull
                    ));
                    return false;
                }
                if !c.overflow {
                    *l += c.used;
                    eatsize(s, l);
                }
                true
            }
        }
    }

    /// `file_regcomp(..., REG_EXTENDED)` só pra validar a regex no carregamento.
    fn regcomp_check(&mut self, pat: &[u8]) -> bool {
        if let Err(w) = regex::check_regex(pat) {
            self.magwarn(w);
            return false;
        }
        match regex::Regex::compile(pat, false, false) {
            Ok(_) => true,
            Err(e) => {
                if self.check {
                    let shown = printable(pat);
                    self.magerror(format!("regex error {} for `{shown}', ({})", e.code, e.message));
                }
                false
            }
        }
    }

    /// `getstr()`: converte os escapes do C e guarda em `value.s`. Devolve onde parou.
    fn getstr(&mut self, m: &mut Magic, s: &[u8], start: usize, mut warn: bool) -> Option<usize> {
        let mut out: Vec<u8> = Vec::new();
        let pmax = MAXSTRING - 1;
        let mut i = start;
        let mut bracket_nesting = 0usize;
        loop {
            let c = at(s, i);
            i += 1;
            if c == 0 {
                break;
            }
            if is_space(c) {
                break;
            }
            if out.len() >= pmax {
                let orig = String::from_utf8_lossy(cstr(tail(s, start))).into_owned();
                self.error(format!("string too long: `{orig}'"));
                m.value = [0; MAXSTRING];
                return None;
            }
            if c != b'\\' {
                if c == b'[' {
                    bracket_nesting += 1;
                }
                if c == b']' && bracket_nesting > 0 {
                    bracket_nesting -= 1;
                }
                out.push(c);
                continue;
            }
            let c = at(s, i);
            i += 1;
            match c {
                0 => {
                    if warn {
                        self.magwarn("incomplete escape".into());
                    }
                    i -= 1;
                    // `goto out`: sem o `--s` do fim do laço.
                    self.store_value(m, &out);
                    return self.finish_getstr(m, i);
                }
                b'.' | b'\t' | b' ' | b'>' | b'<' | b'&' | b'^' | b'=' | b'!' | b'\\' => {
                    if c == b'.' {
                        if m.typ == FILE_REGEX && bracket_nesting == 0 && warn {
                            self.magwarn("escaped dot ('.') found, use \\\\. instead".into());
                        }
                        warn = false;
                    }
                    if c == b'\t' && warn {
                        self.magwarn("escaped tab found, use \\\\t instead".into());
                        warn = false;
                    }
                    out.push(c);
                }
                b'a' => out.push(0x07),
                b'b' => out.push(0x08),
                b'f' => out.push(0x0c),
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'v' => out.push(0x0b),
                b'0'..=b'7' => {
                    let mut val = u32::from(c - b'0');
                    let c2 = at(s, i);
                    i += 1;
                    if (b'0'..=b'7').contains(&c2) {
                        val = (val << 3) | u32::from(c2 - b'0');
                        let c3 = at(s, i);
                        i += 1;
                        if (b'0'..=b'7').contains(&c3) {
                            val = (val << 3) | u32::from(c3 - b'0');
                        } else {
                            i -= 1;
                        }
                    } else {
                        i -= 1;
                    }
                    out.push(val as u8);
                }
                b'x' => {
                    let mut val = u32::from(b'x');
                    let c2 = hextoint(at(s, i));
                    i += 1;
                    if let Some(d) = c2 {
                        val = d;
                        let c3 = hextoint(at(s, i));
                        i += 1;
                        if let Some(d3) = c3 {
                            val = (val << 4) + d3;
                        } else {
                            i -= 1;
                        }
                    } else {
                        i -= 1;
                    }
                    out.push(val as u8);
                }
                other => {
                    if warn {
                        if is_print(other) {
                            if !b"<>&^=!".contains(&other) && (m.typ != FILE_REGEX || !b"[]().*?^$|{}".contains(&other)) {
                                self.magwarn(format!("no need to escape `{}'", other as char));
                            }
                        } else {
                            self.magwarn(format!("unknown escape sequence: \\{other:03o}"));
                        }
                    }
                    out.push(other);
                }
            }
        }
        i -= 1;
        self.store_value(m, &out);
        self.finish_getstr(m, i)
    }

    fn store_value(&mut self, m: &mut Magic, out: &[u8]) {
        m.value = [0; MAXSTRING];
        m.value[..out.len()].copy_from_slice(out);
        m.vallen = out.len() as u8;
    }

    fn finish_getstr(&mut self, m: &mut Magic, i: usize) -> Option<usize> {
        if m.typ == FILE_PSTRING {
            match pstring_length_size(m) {
                Some(l) => m.vallen = m.vallen.wrapping_add(l as u8),
                None => {
                    self.error(format!("corrupt magic file (bad pascal string length {})", m.str_flags() & PSTRING_LEN));
                    return None;
                }
            }
        }
        Some(i)
    }

    /// `parse_strength()`.
    fn parse_strength(&mut self, entry: &mut Entry, arg: &[u8]) -> bool {
        let m = &mut entry.mp[0];
        if m.factor_op != FILE_FACTOR_OP_NONE {
            let (op, f) = (m.factor_op as char, m.factor);
            self.magwarn(format!("Current entry already has a strength type: {op} {f}"));
            return false;
        }
        if m.typ == FILE_NAME {
            let v = printable(m.value_str());
            self.magwarn(format!("{v}: Strength setting is not supported in \"name\" magic entries"));
            return false;
        }
        let m = &mut entry.mp[0];
        let mut l = 0usize;
        eatab!(arg, l);
        match at(arg, l) {
            0 => {}
            c @ (b'+' | b'-' | b'*' | b'/') => {
                m.factor_op = c;
                l += 1;
            }
            c => {
                self.magwarn(format!("Unknown factor op `{}'", c as char));
                return false;
            }
        }
        let m = &mut entry.mp[0];
        eatab!(arg, l);
        let conv = strtoul(tail(arg, l), 0);
        let el = l + conv.used;
        let factor = conv.value;
        let fail = |this: &mut Loader, m: &mut Magic, msg: String| {
            this.magwarn(msg);
            m.factor_op = FILE_FACTOR_OP_NONE;
            m.factor = 0;
            false
        };
        if factor > 255 {
            return fail(self, m, format!("Too large factor `{factor}'"));
        }
        if at(arg, el) != 0 && !is_space(at(arg, el)) {
            let rest = String::from_utf8_lossy(cstr(tail(arg, l))).into_owned();
            return fail(self, m, format!("Bad factor `{rest}'"));
        }
        m.factor = factor as u8;
        if m.factor == 0 && m.factor_op == b'/' {
            let (op, f) = (m.factor_op as char, m.factor);
            return fail(self, m, format!("Cannot have factor op `{op}' and factor {f}"));
        }
        true
    }

    /// `parse_extra()` pra `!:mime`, `!:apple` e `!:ext`.
    fn parse_extra(&mut self, entry: &mut Entry, line: &[u8], llen: usize, field: Field) -> bool {
        let last = entry.mp.len() - 1;
        let (name, extra, nt, cap) = match field {
            Field::Apple => ("APPLE", &b"!+-./?"[..], false, 8),
            Field::Ext => ("EXTENSION", &b",!+-/@?_$&~"[..], false, MAXEXT),
            Field::Mime => ("MIME", &b"+-/.$?:{};="[..], true, MAXMIME),
        };
        let current = match field {
            Field::Apple => entry.mp[last].apple.to_vec(),
            Field::Ext => entry.mp[last].ext.to_vec(),
            Field::Mime => entry.mp[last].mimetype.to_vec(),
        };
        let mut l = 0usize;
        if current[0] != 0 {
            let shown_len = if nt { cstr(&current).len() } else { cap };
            let old = String::from_utf8_lossy(&current[..shown_len.min(current.len())]).into_owned();
            let new = String::from_utf8_lossy(cstr(line)).into_owned();
            self.magwarn(format!("Current entry already has a {name} type `{old}', new type `{new}'"));
            return false;
        }
        if entry.mp[last].desc[0] == 0 {
            self.magwarn(format!("Current entry does not yet have a description for adding a {name} type"));
            return false;
        }
        eatab!(line, l);
        // `goodchar()`: `strchr(extra, '\0')` acha o terminador, então o NUL também é "bom".
        let good = |x: u8| (x < 0x80 && is_alnum(x)) || x == 0 || extra.contains(&x);
        let mut buf = vec![0u8; cap];
        let mut i = 0usize;
        while at(line, l) != 0 && i < llen && i < cap && good(at(line, l)) {
            buf[i] = at(line, l);
            i += 1;
            l += 1;
        }
        if i == cap && at(line, l) != 0 {
            if nt {
                buf[cap - 1] = 0;
            }
            if self.check {
                let shown = String::from_utf8_lossy(cstr(line)).into_owned();
                self.magwarn(format!("{name} type `{shown}' truncated {i}"));
            }
        } else {
            let c = at(line, l);
            if !is_space(c) && !good(c) {
                let shown = String::from_utf8_lossy(cstr(line)).into_owned();
                self.magwarn(format!("{name} type `{shown}' has bad char '{}'", c as char));
            }
            if nt && i < cap {
                buf[i] = 0;
            }
        }
        match field {
            Field::Apple => entry.mp[last].apple.copy_from_slice(&buf),
            Field::Ext => entry.mp[last].ext.copy_from_slice(&buf),
            Field::Mime => entry.mp[last].mimetype.copy_from_slice(&buf),
        }
        if i > 0 {
            return true;
        }
        let shown = String::from_utf8_lossy(cstr(line)).into_owned();
        self.magerror(format!("Bad magic entry '{shown}'"));
        false
    }

    /// `check_format()`: a descrição só pode ter um `%` compatível com o tipo.
    fn check_format(&mut self, m: &Magic) -> bool {
        let d = m.desc_bytes();
        let Some(p) = d.iter().position(|&c| c == b'%') else {
            return true;
        };
        let fmt = type_format(m.typ);
        let tname = type_name(m.typ);
        let shown = String::from_utf8_lossy(d).into_owned();
        if fmt == FILE_FMT_NONE {
            self.magwarn(format!("No format string for `{shown}' with description `{tname}'"));
            return false;
        }
        let rest = &d[p + 1..];
        match check_format_type(rest, m.typ, fmt) {
            Ok(used) => {
                if rest[used..].contains(&b'%') {
                    self.magwarn(format!("Too many format strings (should have at most one) for `{tname}' with description `{shown}'"));
                    return false;
                }
                true
            }
            Err(estr) => {
                self.magwarn(format!("Printf format is {estr} for type `{tname}' in description `{shown}'"));
                false
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Field {
    Mime,
    Apple,
    Ext,
}

const TYPE_TBL_REF: &[(&str, u8, u8)] = TYPE_TBL;

/// `get_type()`: primeira entrada da tabela que é prefixo da linha.
fn get_type(tbl: &[(&str, u8, u8)], l: &[u8]) -> (u8, usize) {
    for (name, t, _) in tbl {
        // A sentinela "invalid" casa por prefixo no C também e dá FILE_INVALID: o efeito é o
        // mesmo de não achar nada.
        if *t != FILE_INVALID && l.starts_with(name.as_bytes()) {
            return (*t, name.len());
        }
    }
    (FILE_INVALID, 0)
}

fn get_special_type(l: &[u8]) -> (u8, usize) {
    for (name, t) in SPECIAL_TBL {
        if l.starts_with(name.as_bytes()) {
            return (*t, name.len());
        }
    }
    (FILE_INVALID, 0)
}

/// `get_standard_integer_type()`: `d`/`u` seguidos de `C S I L Q` ou `1 2 4 8`. `l[0]` é o
/// `d`/`u`; devolve o tipo e quantos bytes consumiu.
fn get_standard_integer_type(l: &[u8]) -> (u8, usize) {
    let c1 = at(l, 1);
    if is_alpha(c1) {
        let t = match c1 {
            b'C' => FILE_BYTE,
            b'S' => FILE_SHORT,
            b'I' | b'L' => FILE_LONG,
            b'Q' => FILE_QUAD,
            _ => return (FILE_INVALID, 0),
        };
        (t, 2)
    } else if is_digit(c1) {
        if is_digit(at(l, 2)) {
            return (FILE_INVALID, 0);
        }
        let t = match c1 {
            b'1' => FILE_BYTE,
            b'2' => FILE_SHORT,
            b'4' => FILE_LONG,
            b'8' => FILE_QUAD,
            _ => return (FILE_INVALID, 0),
        };
        (t, 2)
    } else {
        (FILE_LONG, 1)
    }
}

fn get_op(c: u8) -> Option<u8> {
    Some(match c {
        b'&' => FILE_OPAND,
        b'|' => FILE_OPOR,
        b'^' => FILE_OPXOR,
        b'+' => FILE_OPADD,
        b'-' => FILE_OPMINUS,
        b'*' => FILE_OPMULTIPLY,
        b'/' => FILE_OPDIVIDE,
        b'%' => FILE_OPMODULO,
        _ => return None,
    })
}

/// `get_cond()`: `if`, `elif`, `else` seguidos de espaço.
fn get_cond(s: &[u8], l: &mut usize) -> u8 {
    for (name, cond) in [("if", COND_IF), ("elif", COND_ELIF), ("else", COND_ELSE)] {
        let n = name.len();
        if s[(*l).min(s.len())..].starts_with(name.as_bytes()) && is_space(at(s, *l + n)) {
            *l += n;
            return cond;
        }
    }
    COND_NONE
}

/// `eatsize()`: sufixo de tamanho de um número (`10UL`).
fn eatsize(s: &[u8], l: &mut usize) {
    let lower = |c: u8| if is_upper(c) { c.to_ascii_lowercase() } else { c };
    if lower(at(s, *l)) == b'u' {
        *l += 1;
    }
    if matches!(lower(at(s, *l)), b'l' | b's' | b'h' | b'b' | b'c') {
        *l += 1;
    }
}

fn hextoint(c: u8) -> Option<u32> {
    if c >= 0x80 {
        return None;
    }
    match c {
        b'0'..=b'9' => Some(u32::from(c - b'0')),
        b'a'..=b'f' => Some(u32::from(c - b'a') + 10),
        b'A'..=b'F' => Some(u32::from(c - b'A') + 10),
        _ => None,
    }
}

/// `file_parse_guid()`: o `sscanf("%8x-%4hx-%4hx-%2hhx%2hhx-%2hhx...")` guardando os campos
/// na ordem do `struct guid` (little-endian).
fn parse_guid(s: &[u8]) -> Option<[u8; 16]> {
    fn hex(s: &[u8], i: &mut usize, max: usize) -> Option<u32> {
        let mut v: u32 = 0;
        let mut n = 0;
        while n < max {
            match hextoint(at(s, *i)) {
                Some(d) => {
                    v = (v << 4) | d;
                    *i += 1;
                    n += 1;
                }
                None => break,
            }
        }
        (n > 0).then_some(v)
    }
    let mut i = 0usize;
    let mut out = [0u8; 16];
    let d1 = hex(s, &mut i, 8)?;
    if at(s, i) != b'-' {
        return None;
    }
    i += 1;
    let d2 = hex(s, &mut i, 4)? as u16;
    if at(s, i) != b'-' {
        return None;
    }
    i += 1;
    let d3 = hex(s, &mut i, 4)? as u16;
    if at(s, i) != b'-' {
        return None;
    }
    i += 1;
    out[0..4].copy_from_slice(&d1.to_le_bytes());
    out[4..6].copy_from_slice(&d2.to_le_bytes());
    out[6..8].copy_from_slice(&d3.to_le_bytes());
    for k in 0..8 {
        if k == 2 {
            if at(s, i) != b'-' {
                return None;
            }
            i += 1;
        }
        out[8 + k] = hex(s, &mut i, 2)? as u8;
    }
    Some(out)
}

/// `check_format_type()`: devolve quantos bytes depois do `%` o formato ocupou.
fn check_format_type(p: &[u8], typ: u8, fmt: u8) -> Result<usize, &'static str> {
    let mut i = 0usize;
    if at(p, 0) == 0 {
        return Err("missing format spec");
    }
    let checklen = |i: &mut usize| -> Result<(), &'static str> {
        let mut len = 0usize;
        let mut cnt = 0usize;
        while is_digit(at(p, *i)) {
            len = len * 10 + usize::from(at(p, *i) - b'0');
            *i += 1;
            cnt += 1;
        }
        if cnt > 5 || len > 1024 { Err("too long") } else { Ok(()) }
    };
    match fmt {
        FILE_FMT_QUAD | FILE_FMT_NUM => {
            let quad = fmt == FILE_FMT_QUAD;
            let h = if quad {
                0
            } else {
                match typ {
                    FILE_BYTE => 2,
                    FILE_SHORT | FILE_BESHORT | FILE_LESHORT => 1,
                    FILE_LONG | FILE_BELONG | FILE_LELONG | FILE_MELONG | FILE_LEID3 | FILE_BEID3 | FILE_INDIRECT => 0,
                    _ => 0,
                }
            };
            while at(p, i) != 0 && b"-.#".contains(&at(p, i)) {
                i += 1;
            }
            checklen(&mut i)?;
            if at(p, i) == b'.' {
                i += 1;
            }
            checklen(&mut i)?;
            if quad {
                if at(p, i) != b'l' {
                    return Err("not valid");
                }
                i += 1;
                if at(p, i) != b'l' {
                    return Err("not valid");
                }
                i += 1;
            }
            let c = at(p, i);
            i += 1;
            match c {
                b'c' => {
                    if h == 2 {
                        Ok(i)
                    } else {
                        Err("not valid")
                    }
                }
                b'i' | b'd' | b'u' | b'o' | b'x' | b'X' => Ok(i),
                _ => Err("not valid"),
            }
        }
        FILE_FMT_FLOAT | FILE_FMT_DOUBLE => {
            if at(p, i) == b'-' {
                i += 1;
            }
            if at(p, i) == b'.' {
                i += 1;
            }
            checklen(&mut i)?;
            if at(p, i) == b'.' {
                i += 1;
            }
            checklen(&mut i)?;
            let c = at(p, i);
            i += 1;
            match c {
                b'e' | b'E' | b'f' | b'F' | b'g' | b'G' => Ok(i),
                _ => Err("not valid"),
            }
        }
        FILE_FMT_STR => {
            if at(p, i) == b'-' {
                i += 1;
            }
            while is_digit(at(p, i)) {
                i += 1;
            }
            if at(p, i) == b'.' {
                i += 1;
                while is_digit(at(p, i)) {
                    i += 1;
                }
            }
            let c = at(p, i);
            i += 1;
            if c == b's' { Ok(i) } else { Err("not valid") }
        }
        _ => Err("not valid"),
    }
}

/// `file_pstring_length_size()`.
pub fn pstring_length_size(m: &Magic) -> Option<usize> {
    match m.str_flags() & PSTRING_LEN {
        PSTRING_1_LE => Some(1),
        PSTRING_2_LE | PSTRING_2_BE => Some(2),
        PSTRING_4_LE | PSTRING_4_BE => Some(4),
        _ => None,
    }
}

/// `file_pstring_get_length()`.
pub fn pstring_get_length(m: &Magic, s: &[u8]) -> Option<u64> {
    let b = |i: usize| u64::from(at(s, i));
    let mut len = match m.str_flags() & PSTRING_LEN {
        PSTRING_1_LE => b(0),
        PSTRING_2_LE => (b(1) << 8) | b(0),
        PSTRING_2_BE => (b(0) << 8) | b(1),
        PSTRING_4_LE => (b(3) << 24) | (b(2) << 16) | (b(1) << 8) | b(0),
        PSTRING_4_BE => (b(0) << 24) | (b(1) << 16) | (b(2) << 8) | b(3),
        _ => return None,
    };
    if m.str_flags() & PSTRING_LENGTH_INCLUDES_ITSELF != 0 {
        let l = pstring_length_size(m)? as u64;
        len = len.wrapping_sub(l);
    }
    Some(len)
}

/// `set_text_binary()`: no 5.46 o laço anda sobre as *regras* (`magic_entry`) e para na próxima
/// de nível 0, então só a entrada de nível 0 decide se a regra é de teste binário ou de texto
/// (as continuações não entram, ao contrário do que o comentário do C sugere).
fn set_text_binary(e: &mut Entry) {
    let mut flag = e.mp[0].flag;
    let sflags0 = e.mp[0].str_flags();
    if let Some(m) = e.mp.first() {
        match m.typ {
            FILE_BYTE | FILE_SHORT | FILE_LONG | FILE_DATE | FILE_BESHORT | FILE_BELONG | FILE_BEDATE | FILE_LESHORT | FILE_LELONG
            | FILE_LEDATE | FILE_LDATE | FILE_BELDATE | FILE_LELDATE | FILE_MEDATE | FILE_MELDATE | FILE_MELONG | FILE_QUAD
            | FILE_LEQUAD | FILE_BEQUAD | FILE_QDATE | FILE_LEQDATE | FILE_BEQDATE | FILE_QLDATE | FILE_LEQLDATE | FILE_BEQLDATE
            | FILE_QWDATE | FILE_LEQWDATE | FILE_BEQWDATE | FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT | FILE_DOUBLE | FILE_BEDOUBLE
            | FILE_LEDOUBLE | FILE_BEVARINT | FILE_LEVARINT | FILE_DER | FILE_GUID | FILE_OFFSET | FILE_MSDOSDATE
            | FILE_BEMSDOSDATE | FILE_LEMSDOSDATE | FILE_MSDOSTIME | FILE_BEMSDOSTIME | FILE_LEMSDOSTIME | FILE_OCTAL => {
                flag |= BINTEST;
            }
            FILE_STRING | FILE_PSTRING | FILE_BESTRING16 | FILE_LESTRING16 => {
                if sflags0 & STRING_TEXTTEST != 0 {
                    flag |= TEXTTEST;
                } else {
                    flag |= BINTEST;
                }
            }
            FILE_REGEX | FILE_SEARCH => {
                if sflags0 & STRING_BINTEST != 0 {
                    flag |= BINTEST;
                }
                if sflags0 & STRING_TEXTTEST != 0 {
                    flag |= TEXTTEST;
                }
                if flag & (TEXTTEST | BINTEST) == 0 {
                    let v = &m.value[..usize::from(m.vallen)];
                    if looks_utf8(v, None) <= 0 {
                        flag |= BINTEST;
                    } else {
                        flag |= TEXTTEST;
                    }
                }
            }
            _ => {}
        }
    }
    e.mp[0].flag = flag;
}

/// `apprentice_sort()`: força decrescente; empate pelo `memcmp` da entrada (sem a linha),
/// maior primeiro.
fn apprentice_sort(a: &(usize, [u8; MAGIC_SIZE], Entry), b: &(usize, [u8; MAGIC_SIZE], Entry)) -> Ordering {
    if a.0 == b.0 {
        return b.1.as_slice().cmp(a.1.as_slice());
    }
    b.0.cmp(&a.0)
}

/// `file_printable()` sem limite de tamanho (pras mensagens do carregamento).
pub fn printable(s: &[u8]) -> String {
    let mut out = String::new();
    for &c in cstr(s) {
        if is_print(c) {
            out.push(c as char);
        } else {
            out.push_str(&format!("\\{:o}{:o}{:o}", (c >> 6) & 7, (c >> 3) & 7, c & 7));
        }
    }
    out
}

/// Carrega o banco embutido (o `magic.mgc` do Debian, a partir do texto).
pub fn load_builtin() -> (MagicMap, LoadReport) {
    let mut loader = Loader::new();
    loader.compile = true;
    for (name, text) in super::magdir::FRAGMENTS {
        sysabi::sys::checkpoint();
        loader.load_text(&format!("{}/{}", super::magdir::DIR_NAME, name), text);
    }
    loader.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_one(text: &str) -> (MagicMap, LoadReport) {
        let mut l = Loader::new();
        l.load_text("t", text.as_bytes());
        l.finish()
    }

    #[test]
    fn parses_simple_rule_with_continuations() {
        let (map, rep) = parse_one("0\tstring\t\\x89PNG\tPNG image data\n!:mime\timage/png\n>16\tbelong\tx\t\\b, %d x\n");
        assert!(rep.warnings.is_empty(), "{:?}", rep.warnings);
        assert_eq!(rep.errs, 0);
        let set = &map.sets[0];
        assert_eq!(set.len(), 2);
        assert_eq!(set[0].typ, FILE_STRING);
        assert_eq!(set[0].value_str(), b"\x89PNG");
        assert_eq!(set[0].vallen, 4);
        assert_eq!(set[0].mime_bytes(), b"image/png");
        assert_eq!(set[1].cont_level, 1);
        assert_eq!(set[1].reln, b'x');
        assert_ne!(set[1].flag & NOSPACE, 0);
        assert_eq!(set[1].desc_bytes(), b", %d x");
        // Força: string de 4 bytes = 20 + 40 + 10.
        assert_eq!(magic_strength(&set[0]), 70);
        assert_ne!(set[0].flag & BINTEST, 0);
    }

    #[test]
    fn indirect_offsets_and_masks() {
        let (map, rep) = parse_one("0\tbyte\t1\tx\n>(4.L+8)\tbelong&0xff\t>3\ty\n>>&(2.s-1)\tleshort\t!0\tz\n");
        assert_eq!(rep.errs, 0, "{:?} {:?}", rep.error, rep.warnings);
        let m = &map.sets[0][1];
        assert_eq!(m.offset, 4);
        assert_eq!(m.in_type, FILE_BELONG);
        assert_eq!(m.in_op & FILE_OPS_MASK, FILE_OPADD);
        assert_eq!(m.in_offset, 8);
        assert_eq!(m.mask_op & FILE_OPS_MASK, FILE_OPAND);
        assert_eq!(m.num_mask(), 0xff);
        assert_eq!(m.reln, b'>');
        assert_eq!(m.value_q(), 3);
        let m2 = &map.sets[0][2];
        assert_ne!(m2.flag & INDIR, 0);
        // `&(`: o OFFADD vira INDIROFFADD.
        assert_eq!(m2.flag & OFFADD, 0);
        assert_ne!(m2.flag & INDIROFFADD, 0);
        assert_eq!(m2.in_type, FILE_LESHORT);
        assert_eq!(m2.in_op & FILE_OPS_MASK, FILE_OPMINUS);
    }

    #[test]
    fn sorts_by_strength() {
        let (map, _) = parse_one("0\tbyte\t1\tweak\n0\tstring\tLONGSTRING\tstrong\n");
        assert_eq!(map.sets[0][0].desc_bytes(), b"strong");
        assert_eq!(map.sets[0][1].desc_bytes(), b"weak");
    }

    #[test]
    fn names_go_to_second_set_and_text_rules_are_flagged() {
        let (map, rep) = parse_one("0\tname\tfoo\n>0\tbyte\t1\tone\n0\tsearch/10\thello\tgreeting\n");
        assert_eq!(rep.errs, 0, "{:?}", rep.error);
        assert_eq!(map.sets[1].len(), 2);
        assert_eq!(map.sets[1][0].typ, FILE_NAME);
        assert_eq!(map.sets[0][0].typ, FILE_SEARCH);
        assert_ne!(map.sets[0][0].flag & TEXTTEST, 0);
    }

    #[test]
    fn rejects_bad_format() {
        let (_, rep) = parse_one("0\tbyte\t1\t%s bad\n");
        assert_eq!(rep.errs, 1);
        assert!(rep.warnings[0].contains("Printf format is not valid"), "{:?}", rep.warnings);
    }

    /// O banco embutido carrega sem aviso nem erro (o `file -C` do oráculo também não avisa nada)
    /// e com o mesmo número de entradas do `/usr/lib/file/magic.mgc` (16760 + 7205).
    #[test]
    fn builtin_loads_clean() {
        let (map, rep) = load_builtin();
        assert!(rep.warnings.is_empty(), "{:?}", &rep.warnings[..rep.warnings.len().min(10)]);
        assert_eq!(rep.errs, 0, "{:?}", rep.error);
        assert_eq!(map.sets[0].len(), 16760);
        assert_eq!(map.sets[1].len(), 7205);
    }

    /// Com `MISC_FILE_ORACLE_MGC` apontando pra uma cópia do `/usr/lib/file/magic.mgc` do oráculo,
    /// o banco compilado aqui tem que ser idêntico, entrada por entrada.
    #[test]
    fn builtin_matches_oracle_mgc() {
        let Ok(path) = std::env::var("MISC_FILE_ORACLE_MGC") else { return };
        let oracle = std::fs::read(path).expect("ler o mgc do oráculo");
        let (map, _) = load_builtin();
        let ours = map.compile();
        assert_eq!(ours.len(), oracle.len());
        let mut bad = 0;
        for (i, (a, b)) in ours.chunks(MAGIC_SIZE).zip(oracle.chunks(MAGIC_SIZE)).enumerate() {
            if a != b {
                bad += 1;
                if bad <= 5 {
                    let ma = Magic::from_image(a, false);
                    let mb = Magic::from_image(b, false);
                    let diff: Vec<usize> = (0..MAGIC_SIZE).filter(|&k| a[k] != b[k]).collect();
                    eprintln!("entrada {i}: nosso {ma:?} oráculo {mb:?} bytes {:?}", &diff[..diff.len().min(12)]);
                }
            }
        }
        assert_eq!(bad, 0, "{bad} entradas diferentes");
    }

    #[test]
    fn nonmagic_counts_like_c() {
        assert_eq!(nonmagic(b"^#!.*python"), 8);
        assert_eq!(nonmagic(b"[0-9]+"), 1);
        assert_eq!(nonmagic(b"a{1,2}b"), 2);
        assert_eq!(nonmagic(b"*"), 1);
    }
}
