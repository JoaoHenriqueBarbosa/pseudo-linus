// Porte para Rust de partes do magic.c, funcs.c e print.c do file 5.46.
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

//! O `struct magic_set` do libmagic: as bandeiras, os parâmetros (`-P`), o buffer de saída com o
//! `file_printf` e o `file_error`, e o banco carregado. O resto do motor (`softmagic`, `ascmagic`,
//! `fsmagic`...) é implementado como métodos sobre ele em outros módulos.

use std::sync::{Arc, OnceLock};

use super::apprentice::{self, Magic, MagicMap};
use super::cfmt::{self, Arg};
use super::regex::Regex;

pub const MAGIC_DEBUG: u32 = 0x000_0001;
pub const MAGIC_SYMLINK: u32 = 0x000_0002;
pub const MAGIC_COMPRESS: u32 = 0x000_0004;
pub const MAGIC_DEVICES: u32 = 0x000_0008;
pub const MAGIC_MIME_TYPE: u32 = 0x000_0010;
pub const MAGIC_CONTINUE: u32 = 0x000_0020;
pub const MAGIC_CHECK: u32 = 0x000_0040;
pub const MAGIC_PRESERVE_ATIME: u32 = 0x000_0080;
pub const MAGIC_RAW: u32 = 0x000_0100;
pub const MAGIC_ERROR: u32 = 0x000_0200;
pub const MAGIC_MIME_ENCODING: u32 = 0x000_0400;
pub const MAGIC_MIME: u32 = MAGIC_MIME_TYPE | MAGIC_MIME_ENCODING;
pub const MAGIC_APPLE: u32 = 0x000_0800;
pub const MAGIC_EXTENSION: u32 = 0x100_0000;
pub const MAGIC_COMPRESS_TRANSP: u32 = 0x200_0000;
pub const MAGIC_NO_COMPRESS_FORK: u32 = 0x400_0000;
pub const MAGIC_NODESC: u32 = MAGIC_EXTENSION | MAGIC_MIME | MAGIC_APPLE;
pub const MAGIC_NO_CHECK_COMPRESS: u32 = 0x000_1000;
pub const MAGIC_NO_CHECK_TAR: u32 = 0x000_2000;
pub const MAGIC_NO_CHECK_SOFT: u32 = 0x000_4000;
pub const MAGIC_NO_CHECK_APPTYPE: u32 = 0x000_8000;
pub const MAGIC_NO_CHECK_ELF: u32 = 0x001_0000;
pub const MAGIC_NO_CHECK_TEXT: u32 = 0x002_0000;
pub const MAGIC_NO_CHECK_CDF: u32 = 0x004_0000;
pub const MAGIC_NO_CHECK_CSV: u32 = 0x008_0000;
pub const MAGIC_NO_CHECK_TOKENS: u32 = 0x010_0000;
pub const MAGIC_NO_CHECK_ENCODING: u32 = 0x020_0000;
pub const MAGIC_NO_CHECK_JSON: u32 = 0x040_0000;
pub const MAGIC_NO_CHECK_SIMH: u32 = 0x080_0000;

pub const FILE_BYTES_MAX: usize = 7 * 1024 * 1024;
pub const FILE_ELF_NOTES_MAX: usize = 256;
pub const FILE_ELF_PHNUM_MAX: usize = 2048;
pub const FILE_ELF_SHNUM_MAX: usize = 32768;
pub const FILE_ELF_SHSIZE_MAX: usize = 128 * 1024 * 1024;
pub const FILE_INDIR_MAX: usize = 50;
pub const FILE_NAME_MAX: usize = 100;
pub const FILE_REGEX_MAX: usize = 8192;
pub const FILE_ENCODING_MAX: usize = 64 * 1024;
pub const FILE_MAGWARN_MAX: usize = 64;

/// Um banco carregado (um `struct mlist` de cada conjunto), com o cache das regex compiladas.
pub struct Db {
    pub map: MagicMap,
    rx: [Vec<OnceLock<Option<Arc<Regex>>>>; 2],
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Db({} + {})", self.map.sets[0].len(), self.map.sets[1].len())
    }
}

impl Db {
    pub fn new(map: MagicMap) -> Db {
        let rx = [
            (0..map.sets[0].len()).map(|_| OnceLock::new()).collect(),
            (0..map.sets[1].len()).map(|_| OnceLock::new()).collect(),
        ];
        Db { map, rx }
    }

    pub fn list(&self, set: usize) -> MagicList<'_> {
        MagicList { magic: &self.map.sets[set], rx: &self.rx[set] }
    }
}

/// Uma fatia de regras com o cache de regex alinhado.
#[derive(Clone, Copy)]
pub struct MagicList<'a> {
    pub magic: &'a [Magic],
    pub rx: &'a [OnceLock<Option<Arc<Regex>>>],
}

impl<'a> MagicList<'a> {
    pub fn sub(&self, start: usize, end: usize) -> MagicList<'a> {
        MagicList { magic: &self.magic[start..end], rx: &self.rx[start..end] }
    }
}

/// O banco embutido, carregado uma vez por processo do host (é dado imutável, compartilhado
/// por todos os pseudo-processos).
pub fn builtin_db() -> Arc<Db> {
    static DB: OnceLock<Arc<Db>> = OnceLock::new();
    DB.get_or_init(|| {
        let (map, _report) = apprentice::load_builtin();
        Arc::new(Db::new(map))
    })
    .clone()
}

/// Estado de um nível de continuação (`struct level_info`).
#[derive(Clone, Copy, Debug, Default)]
pub struct LevelInfo {
    pub off: i32,
    pub got_match: bool,
    pub last_match: bool,
    pub last_cond: u8,
}

/// O resultado de uma busca (`ms->search`): posição dentro do buffer da regra corrente.
#[derive(Clone, Copy, Debug, Default)]
pub struct Search {
    /// Onde a janela começa no buffer (o ponteiro `s`); `None` é o `NULL`.
    pub s: Option<usize>,
    pub s_len: usize,
    pub offset: usize,
    pub rm_len: usize,
}

/// Parâmetros do `-P`.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub bytes_max: usize,
    pub elf_notes_max: u16,
    pub elf_phnum_max: u16,
    pub elf_shnum_max: u16,
    pub elf_shsize_max: usize,
    pub encoding_max: usize,
    pub indir_max: u16,
    pub name_max: u16,
    pub regex_max: u16,
    pub magwarn_max: usize,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            bytes_max: FILE_BYTES_MAX,
            elf_notes_max: FILE_ELF_NOTES_MAX as u16,
            elf_phnum_max: FILE_ELF_PHNUM_MAX as u16,
            elf_shnum_max: FILE_ELF_SHNUM_MAX as u16,
            elf_shsize_max: FILE_ELF_SHSIZE_MAX,
            encoding_max: FILE_ENCODING_MAX,
            indir_max: FILE_INDIR_MAX as u16,
            name_max: FILE_NAME_MAX as u16,
            regex_max: FILE_REGEX_MAX as u16,
            magwarn_max: FILE_MAGWARN_MAX,
        }
    }
}

/// O `struct magic_set`.
pub struct MagicSet {
    pub flags: u32,
    /// Bancos carregados, na ordem do caminho de magic (o `mlist`).
    pub mlist: Vec<Arc<Db>>,
    /// O buffer de saída (`ms->o.buf`); `None` é o `NULL` do C.
    pub o: Option<Vec<u8>>,
    pub had_err: bool,
    pub error: i32,
    pub offset: i32,
    pub eoffset: i32,
    pub ms_value: [u8; 128],
    pub search: Search,
    pub li: Vec<LevelInfo>,
    /// `st_mode` do arquivo corrente (o `${x?...}` olha os bits de execução).
    pub mode: u32,
    pub line: u32,
    pub params: Params,
    /// Fuso local (pras datas `ldate`).
    pub tz: jiff::tz::TimeZone,
}

/// Erro do `file_printf` que encerra a operação (o `-1` do C).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fail;

impl MagicSet {
    pub fn new(flags: u32, mlist: Vec<Arc<Db>>) -> MagicSet {
        MagicSet {
            flags,
            mlist,
            o: None,
            had_err: false,
            error: -1,
            offset: 0,
            eoffset: 0,
            ms_value: [0; 128],
            search: Search::default(),
            li: Vec::new(),
            mode: 0,
            line: 0,
            params: Params::default(),
            tz: jiff::tz::TimeZone::UTC,
        }
    }

    /// `file_reset()`.
    pub fn reset(&mut self) {
        self.o = None;
        self.had_err = false;
        self.error = -1;
    }

    /// `file_printf()` com o texto já formatado.
    pub fn print(&mut self, s: &[u8]) -> Result<(), Fail> {
        if self.had_err {
            return Ok(());
        }
        let blen = self.o.as_ref().map_or(0, Vec::len);
        if s.len() > 1024 || s.len() + blen > 1024 * 1024 {
            self.o = None;
            self.error_msg(0, &format!("Output buffer space exceeded {}+{blen}", s.len()), 0);
            return Err(Fail);
        }
        self.o.get_or_insert_with(Vec::new).extend_from_slice(s);
        Ok(())
    }

    pub fn print_str(&mut self, s: &str) -> Result<(), Fail> {
        self.print(s.as_bytes())
    }

    /// `file_printf(ms, fmt, arg)`.
    pub fn printf(&mut self, fmt: &[u8], arg: Arg<'_>) -> Result<(), Fail> {
        let s = cfmt::format(fmt, arg);
        self.print(&s)
    }

    /// `file_error_core()`: só o primeiro erro conta.
    pub fn error_msg(&mut self, errno: i32, msg: &str, lineno: u32) {
        if self.had_err {
            return;
        }
        if lineno != 0 {
            self.o = None;
            let _ = self.print(format!("line {lineno}:").as_bytes());
        }
        if self.o.as_ref().is_some_and(|b| !b.is_empty()) {
            let _ = self.print(b" ");
        }
        let _ = self.print(msg.as_bytes());
        if errno > 0 {
            let m = sysabi::Errno(errno).message();
            let _ = self.print(format!(" ({m})").as_bytes());
        }
        self.had_err = true;
        self.error = errno;
    }

    /// `file_error()`.
    pub fn file_error(&mut self, errno: i32, msg: &str) {
        self.error_msg(errno, msg, 0);
    }

    /// `file_magerror()`: com o número da linha da regra.
    pub fn magerror(&mut self, msg: &str) {
        let line = self.line;
        self.error_msg(0, msg, line);
    }

    /// `file_printedlen()`.
    pub fn printedlen(&self) -> usize {
        self.o.as_ref().map_or(0, Vec::len)
    }

    /// `file_separator()`.
    pub fn separator(&mut self) -> Result<(), Fail> {
        self.print(b"\n- ")
    }

    /// `trim_separator()`.
    pub fn trim_separator(&mut self) {
        if let Some(b) = self.o.as_mut() {
            const SEP: &[u8] = b"\n- ";
            if b.len() > SEP.len() && b.ends_with(SEP) {
                let n = b.len() - SEP.len();
                b.truncate(n);
            }
        }
    }

    /// `file_push_buffer()`: guarda a saída e o offset e começa de novo.
    pub fn push_buffer(&mut self) -> Option<(Option<Vec<u8>>, i32)> {
        if self.had_err {
            return None;
        }
        let saved = (self.o.take(), self.offset);
        self.offset = 0;
        Some(saved)
    }

    /// `file_pop_buffer()`: devolve o que foi escrito desde o push e restaura o anterior.
    pub fn pop_buffer(&mut self, pb: (Option<Vec<u8>>, i32)) -> Option<Vec<u8>> {
        if self.had_err {
            return None;
        }
        let r = std::mem::replace(&mut self.o, pb.0);
        self.offset = pb.1;
        Some(r.unwrap_or_default())
    }

    /// `file_check_mem()`: garante o nível e zera o estado dele.
    pub fn check_mem(&mut self, level: usize) {
        if level >= self.li.len() {
            self.li.resize(20 + level, LevelInfo::default());
        }
        self.li[level].got_match = false;
        self.li[level].last_match = false;
        self.li[level].last_cond = apprentice::COND_NONE;
    }

    /// `file_replace()`: troca o padrão (regex estendida) na saída; devolve quantas vezes trocou.
    pub fn replace(&mut self, pat: &[u8], rep: &[u8]) -> Result<usize, Fail> {
        let Ok(rx) = Regex::compile(pat, false, false) else {
            return Err(Fail);
        };
        let mut nm = 0;
        while let Some(buf) = self.o.as_ref() {
            let hay = super::cutil::cstr(buf);
            let Some((so, eo)) = rx.find(hay) else { break };
            let mut nb = hay[..so].to_vec();
            nb.extend_from_slice(rep);
            if eo != 0 {
                nb.extend_from_slice(&hay[eo..]);
            }
            self.o = Some(nb);
            nm += 1;
        }
        Ok(nm)
    }

    /// `file_getbuffer()`: a descrição final, com o escape dos não imprimíveis (sem `-r`).
    pub fn getbuffer(&self) -> Option<Vec<u8>> {
        if self.had_err {
            return None;
        }
        let buf = self.o.as_ref()?;
        if self.flags & MAGIC_RAW != 0 {
            return Some(super::cutil::cstr(buf).to_vec());
        }
        Some(super::wchar::escape_output(buf))
    }

    /// `magic_error()`: a mensagem do erro, se houve.
    pub fn error_text(&self) -> Option<Vec<u8>> {
        if self.had_err { Some(self.o.clone().unwrap_or_default()) } else { None }
    }
}

/// `file_printable()`: `\ooo` pros não imprimíveis (sem `-r`), parando no NUL, no fim ou quando o
/// buffer de `bufsiz` encheria.
pub fn printable(raw: bool, bufsiz: usize, s: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let emax = bufsiz.saturating_sub(1);
    for &c in s {
        if out.len() >= emax || c == 0 {
            break;
        }
        if raw || super::cutil::is_print(c) {
            out.push(c);
            continue;
        }
        if out.len() + 3 >= emax {
            break;
        }
        super::wchar::push_octal(&mut out, c);
    }
    out
}

/// `file_strtrim()`: tira espaços (isspace) das pontas, até o NUL.
pub fn strtrim(s: &[u8]) -> &[u8] {
    let s = super::cutil::cstr(s);
    let start = s.iter().position(|&c| !super::cutil::is_space(c)).unwrap_or(s.len());
    let mut end = s.len();
    while end > start && super::cutil::is_space(s[end - 1]) {
        end -= 1;
    }
    &s[start..end]
}

/// `file_fmtdatetime()` com `FILE_T_LOCAL` (`local`) ou UTC, e `windows` pro FILETIME.
pub fn fmtdatetime(v: u64, local: bool, windows: bool, tz: &jiff::tz::TimeZone) -> String {
    const MAX_CTIME: i64 = 0x3a_fff4_87cf;
    let t: i64 = if windows {
        match cdf_timestamp_to_secs(v as i64, tz) {
            Some(t) => t,
            None => return "*Invalid datetime*".to_string(),
        }
    } else {
        v as i64
    };
    if t > MAX_CTIME {
        return "*Invalid datetime*".to_string();
    }
    // Abaixo do ano -9999 o jiff não representa; a glibc ainda formata, mas nenhuma regra chega lá.
    if t < -377_705_116_800 {
        return "*Invalid datetime*".to_string();
    }
    let zone = if local { tz.clone() } else { jiff::tz::TimeZone::UTC };
    crate::util::time::ctime(t, &zone)
}

/// `cdf_timestamp_to_timespec()`: FILETIME (centenas de ns desde 1601) pra segundos, com a
/// aproximação de ano/mês/dia do cdf_time.c e o `mktime` no fuso local.
fn cdf_timestamp_to_secs(t: i64, tz: &jiff::tz::TimeZone) -> Option<i64> {
    const BASE: i64 = 1601;
    let isleap = |y: i64| y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    const MDAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut t = t / 10_000_000;
    let sec = t % 60;
    t /= 60;
    let min = t % 60;
    t /= 60;
    let hour = t % 24;
    t /= 24;
    let year = BASE + t / 365;
    let mut rdays = 0i64;
    let mut y = BASE;
    while y < year {
        rdays += 365 + i64::from(isleap(y));
        y += 1;
        if y - BASE > 100_000 {
            return None;
        }
    }
    t -= rdays - 1;
    let mut days = t;
    let mut mday = days;
    for (m, md) in MDAYS.iter().enumerate() {
        let sub = md + i64::from(m == 1 && isleap(year));
        if days < sub {
            mday = days;
            break;
        }
        days -= sub;
        mday = days;
    }
    let mut d2 = t;
    let mut mon = 12i64;
    for (m, md) in MDAYS.iter().enumerate() {
        d2 -= md;
        if m == 1 && isleap(year) {
            d2 -= 1;
        }
        if d2 <= 0 {
            mon = m as i64;
            break;
        }
    }
    // mktime normaliza dia/mês fora da faixa.
    let y0 = year + mon.div_euclid(12);
    let m0 = mon.rem_euclid(12) + 1;
    let first = jiff::civil::Date::new(y0 as i16, m0 as i8, 1).ok()?;
    let date = first.checked_add(jiff::Span::new().days(mday - 1)).ok()?;
    let dt = date.at(hour as i8, min as i8, sec as i8, 0);
    let ts = tz.to_ambiguous_timestamp(dt).compatible().ok()?;
    Some(ts.as_second())
}

/// `file_fmtdate()` (com o patch do Debian: `%b %d %Y`, sem o dia da semana).
pub fn fmtdate(v: u16) -> String {
    let mday = v & 0x1f;
    let mon = i32::from((v >> 5) & 0xf) - 1;
    let year = 1980 + i32::from(v >> 9);
    let mname = if (0..12).contains(&mon) { crate::util::time::MONTHS[mon as usize] } else { "?" };
    format!("{mname} {mday:02} {year}")
}

/// `file_fmttime()`: `%T` sem validar as faixas.
pub fn fmttime(v: u16) -> String {
    let sec = (v & 0x1f) * 2;
    let min = (v >> 5) & 0x3f;
    let hour = v >> 11;
    format!("{hour:02}:{min:02}:{sec:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dos_dates_and_times() {
        // 2026-01-15 12:00:00 em DOS.
        let d: u16 = ((2026 - 1980) << 9) | (1 << 5) | 15;
        assert_eq!(fmtdate(d), "Jan 15 2026");
        assert_eq!(fmtdate(0), "? 00 1980");
        assert_eq!(fmttime(12 << 11), "12:00:00");
    }

    #[test]
    fn datetimes() {
        let utc = jiff::tz::TimeZone::UTC;
        assert_eq!(fmtdatetime(1_768_478_400, false, false, &utc), "Thu Jan 15 12:00:00 2026");
        assert_eq!(fmtdatetime(0x3b_0000_0000, false, false, &utc), "*Invalid datetime*");
        // 2021-08-02 13:10:27 UTC em FILETIME.
        assert_eq!(fmtdatetime(132_723_834_270_000_000, false, true, &utc), "Mon Aug  2 13:10:27 2021");
    }

    #[test]
    fn output_buffer_rules() {
        let mut ms = MagicSet::new(0, Vec::new());
        ms.print(b"abc").unwrap();
        ms.separator().unwrap();
        ms.trim_separator();
        assert_eq!(ms.o.as_deref(), Some(&b"abc"[..]));
        ms.print(b" text").unwrap();
        assert_eq!(ms.replace(b" text$", b", ").unwrap(), 1);
        assert_eq!(ms.o.as_deref(), Some(&b"abc, "[..]));
        ms.magerror("boom");
        assert!(ms.had_err);
        assert!(ms.getbuffer().is_none());
    }

    #[test]
    fn printable_limits() {
        assert_eq!(printable(false, 512, b"a\x01b\0c"), b"a\\001b");
        assert_eq!(printable(true, 512, b"a\x01b"), b"a\x01b");
        assert_eq!(printable(false, 6, b"abc\x01"), b"abc");
    }
}
