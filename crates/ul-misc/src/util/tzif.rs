//! Fuso horário com a semântica do `tzset`/`localtime_r` da glibc 2.41: leitura do TZif (v1, v2 e
//! v3) de `$TZDIR` (padrão `/usr/share/zoneinfo`), regra POSIX do rodapé ou do próprio `TZ`, e o
//! `__offtime` que falha quando o ano não cabe num `int`.
//!
//! Detalhes da glibc reproduzidos:
//!
//! - antes da primeira transição vale o primeiro tipo sem horário de verão (ou o tipo 0);
//! - depois da última, a regra do rodapé, com o ano tirado da hora UTC (`__tz_compute`);
//! - `compute_change` conta a partir de 1970 e, para anos até 1970, parte de 0 (o dia 1 de janeiro
//!   de 1970), quirk do original;
//! - nome que não abre como TZif é lido como regra POSIX; regra que não passa no parser deixa o fuso
//!   em UTC com o nome que chegou a ser lido (`TZ=Foo` dá `Foo`);
//! - `TZ` vazio vira `Universal`; `:` na frente é ignorado.
//!
//! Fora do escopo: segundos bissextos (fusos `right/`), que a glibc aplica e aqui são ignorados, e
//! o `posixrules` do `__tzfile_default`.

use sysabi::{Errno, sys};

use crate::util::io;

/// Hora civil quebrada, como a `struct tm` (ano por extenso, mês de 0 a 11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tm {
    pub year: i64,
    pub mon: u32,
    pub mday: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
    pub wday: u32,
    pub yday: u32,
    pub isdst: i32,
    pub gmtoff: i64,
    pub zone: Vec<u8>,
}

const SECS_PER_DAY: i128 = 86_400;

/// Dias desde 1970-01-01 para uma data civil (calendário gregoriano proléptico).
pub fn days_from_civil(y: i128, m: u32, d: u32) -> i128 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i128::from(m);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i128::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i128) -> (i128, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

/// O `__offtime` da glibc: `t + off` em hora civil; `None` quando o ano menos 1900 não cabe num
/// `int` (o `EOVERFLOW` do original).
pub fn offtime(t: i64, off: i64) -> Option<Tm> {
    let total = i128::from(t) + i128::from(off);
    let days = total.div_euclid(SECS_PER_DAY);
    let rem = total.rem_euclid(SECS_PER_DAY) as u32;
    let (y, m, d) = civil_from_days(days);
    let tm_year = y - 1900;
    if tm_year < i128::from(i32::MIN) || tm_year > i128::from(i32::MAX) {
        return None;
    }
    let yday = (days - days_from_civil(y, 1, 1)) as u32;
    Some(Tm {
        year: y as i64,
        mon: m - 1,
        mday: d,
        hour: rem / 3600,
        min: rem / 60 % 60,
        sec: rem % 60,
        wday: (days + 4).rem_euclid(7) as u32,
        yday,
        isdst: 0,
        gmtoff: off,
        zone: Vec::new(),
    })
}

/// `gmtime_r`.
pub fn gmtime(t: i64) -> Option<Tm> {
    let mut tm = offtime(t, 0)?;
    tm.zone = b"GMT".to_vec();
    Some(tm)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RuleKind {
    /// `Jn`: dia juliano de 1 a 365, sem 29 de fevereiro.
    J1,
    /// `n`: dia de 0 a 365, contando 29 de fevereiro.
    #[default]
    J0,
    /// `Mm.n.d`.
    M,
}

#[derive(Clone, Debug, Default)]
struct Rule {
    name: Vec<u8>,
    /// Segundos a leste de UTC (o sinal oposto ao do texto POSIX).
    offset: i64,
    kind: RuleKind,
    m: u16,
    n: u16,
    d: u16,
    secs: i64,
}

/// As duas regras do `tz_rules[]` da glibc.
#[derive(Clone, Debug)]
pub struct PosixTz {
    rules: [Rule; 2],
}

const MON_YDAY: [[i64; 13]; 2] = [
    [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365],
    [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335, 366],
];

/// `sscanf("%hu")`: brancos, sinal opcional e dígitos; o valor é truncado em 16 bits.
fn scan_hu(s: &[u8]) -> Option<(u16, usize)> {
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_whitespace() {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let start = i;
    let mut v: u64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = v.saturating_mul(10).saturating_add(u64::from(s[i] - b'0'));
        i += 1;
    }
    if i == start {
        return None;
    }
    let v = if neg { v.wrapping_neg() } else { v };
    Some((v as u16, i))
}

/// `sscanf(tz, "%hu%n:%hu%n:%hu%n", ...)`: quantos valores leu, os valores e o `consumed`.
fn scan_hms(s: &[u8]) -> (usize, [u16; 3], usize) {
    let mut vals = [0u16; 3];
    let mut consumed = 0;
    let mut pos = 0;
    let mut count = 0;
    for (k, slot) in vals.iter_mut().enumerate() {
        if k > 0 {
            if s.get(pos) != Some(&b':') {
                break;
            }
            pos += 1;
        }
        match scan_hu(&s[pos..]) {
            Some((v, n)) => {
                *slot = v;
                pos += n;
                consumed = pos;
                count += 1;
            }
            None => break,
        }
    }
    (count, vals, consumed)
}

fn compute_offset(ss: u16, mm: u16, hh: u16) -> i64 {
    i64::from(ss.min(59)) + i64::from(mm.min(59)) * 60 + i64::from(hh.min(24)) * 3600
}

impl PosixTz {
    fn empty() -> PosixTz {
        PosixTz {
            rules: [Rule::default(), Rule::default()],
        }
    }

    /// UTC com o nome dado nos dois lados (o caso "nada especificado" do `tzset_internal`).
    pub fn utc_named(name: &[u8]) -> PosixTz {
        let mut p = PosixTz::empty();
        p.rules[0].name = name.to_vec();
        p.rules[1].name = name.to_vec();
        p
    }

    fn parse_tzname(&mut self, s: &[u8], pos: &mut usize, which: usize) -> bool {
        let start = *pos;
        let mut p = start;
        while p < s.len() && s[p].is_ascii_alphabetic() {
            p += 1;
        }
        let (name_start, len, end) = if p - start < 3 {
            let mut p = start;
            if s.get(p) != Some(&b'<') {
                return false;
            }
            p += 1;
            let ns = p;
            while p < s.len() && (s[p].is_ascii_alphanumeric() || s[p] == b'+' || s[p] == b'-') {
                p += 1;
            }
            let len = p - ns;
            if s.get(p) != Some(&b'>') || len < 3 {
                return false;
            }
            (ns, len, p + 1)
        } else {
            (start, p - start, p)
        };
        self.rules[which].name = s[name_start..name_start + len].to_vec();
        *pos = end;
        true
    }

    fn parse_offset(&mut self, s: &[u8], pos: &mut usize, which: usize) -> bool {
        let mut p = *pos;
        let c = s.get(p).copied();
        if which == 0 && !matches!(c, Some(b'+') | Some(b'-') | Some(b'0'..=b'9')) {
            return false;
        }
        let mut sign = -1i64;
        if matches!(c, Some(b'+') | Some(b'-')) {
            sign = if c == Some(b'-') { 1 } else { -1 };
            p += 1;
        }
        *pos = p;
        let (count, v, consumed) = scan_hms(&s[p..]);
        if count > 0 {
            self.rules[which].offset = sign * compute_offset(v[2], v[1], v[0]);
        } else if which == 0 {
            self.rules[0].offset = 0;
            return false;
        } else {
            self.rules[1].offset = self.rules[0].offset + 3600;
        }
        *pos = p + consumed;
        true
    }

    fn parse_rule(&mut self, s: &[u8], pos: &mut usize, which: usize) -> bool {
        let mut p = *pos;
        if s.get(p) == Some(&b',') {
            p += 1;
        }
        let c = s.get(p).copied();
        let r = &mut self.rules[which];
        match c {
            Some(b'J') | Some(b'0'..=b'9') => {
                r.kind = if c == Some(b'J') { RuleKind::J1 } else { RuleKind::J0 };
                if r.kind == RuleKind::J1 {
                    p += 1;
                    if !s.get(p).is_some_and(u8::is_ascii_digit) {
                        return false;
                    }
                }
                let st = p;
                let mut d: u64 = 0;
                while p < s.len() && s[p].is_ascii_digit() {
                    d = d.saturating_mul(10).saturating_add(u64::from(s[p] - b'0'));
                    p += 1;
                }
                if p == st || d > 365 {
                    return false;
                }
                if r.kind == RuleKind::J1 && d == 0 {
                    return false;
                }
                r.d = d as u16;
            }
            Some(b'M') => {
                r.kind = RuleKind::M;
                p += 1;
                let mut vals = [0u16; 3];
                for (k, slot) in vals.iter_mut().enumerate() {
                    if k > 0 {
                        if s.get(p) != Some(&b'.') {
                            return false;
                        }
                        p += 1;
                    }
                    match scan_hu(&s[p..]) {
                        Some((v, n)) => {
                            *slot = v;
                            p += n;
                        }
                        None => return false,
                    }
                }
                let [m, n, d] = vals;
                if !(1..=12).contains(&m) || !(1..=5).contains(&n) || d > 6 {
                    return false;
                }
                r.m = m;
                r.n = n;
                r.d = d;
            }
            None => {
                r.kind = RuleKind::M;
                if which == 0 {
                    (r.m, r.n, r.d) = (3, 2, 0);
                } else {
                    (r.m, r.n, r.d) = (11, 1, 0);
                }
            }
            _ => return false,
        }
        if s.get(p) == Some(&b'/') {
            p += 1;
            if p >= s.len() {
                return false;
            }
            let negative = s[p] == b'-';
            if negative {
                p += 1;
            }
            let mut v = [2u16, 0, 0];
            let (count, got, consumed) = scan_hms(&s[p..]);
            for (k, slot) in v.iter_mut().enumerate().take(count) {
                *slot = got[k];
            }
            p += consumed;
            let secs = i64::from(v[0]) * 3600 + i64::from(v[1]) * 60 + i64::from(v[2]);
            r.secs = if negative { -secs } else { secs };
        } else {
            r.secs = 2 * 3600;
        }
        *pos = p;
        true
    }

    /// `__tzset_parse_tz`.
    pub fn parse(s: &[u8]) -> PosixTz {
        let mut tz = PosixTz::empty();
        let mut pos = 0;
        if tz.parse_tzname(s, &mut pos, 0) && tz.parse_offset(s, &mut pos, 0) {
            if pos < s.len() {
                if tz.parse_tzname(s, &mut pos, 1) {
                    tz.parse_offset(s, &mut pos, 1);
                }
                if tz.parse_rule(s, &mut pos, 0) {
                    tz.parse_rule(s, &mut pos, 1);
                }
            } else {
                tz.rules[1].name = tz.rules[0].name.clone();
                tz.rules[1].offset = tz.rules[0].offset;
            }
        }
        tz
    }

    /// `compute_change` de uma regra para o ano dado.
    fn change(&self, which: usize, year: i64) -> i64 {
        let r = &self.rules[which];
        let mut t: i64 = if year > 1970 {
            ((year - 1970) * 365
                + ((year - 1) / 4 - 1970 / 4)
                - ((year - 1) / 100 - 1970 / 100)
                + ((year - 1) / 400 - 1970 / 400))
                * 86_400
        } else {
            0
        };
        let leap = usize::from(is_leap(year));
        match r.kind {
            RuleKind::J1 => {
                t += (i64::from(r.d) - 1) * 86_400;
                if r.d >= 60 && leap == 1 {
                    t += 86_400;
                }
            }
            RuleKind::J0 => t += i64::from(r.d) * 86_400,
            RuleKind::M => {
                let m = usize::from(r.m);
                let m1 = (i64::from(r.m) + 9) % 12 + 1;
                let yy0 = if r.m <= 2 { year - 1 } else { year };
                let yy1 = yy0 / 100;
                let yy2 = yy0 % 100;
                let mut dow = ((26 * m1 - 2) / 10 + 1 + yy2 + yy2 / 4 + yy1 / 4 - 2 * yy1) % 7;
                if dow < 0 {
                    dow += 7;
                }
                let mut d = i64::from(r.d) - dow;
                if d < 0 {
                    d += 7;
                }
                let mlen = MON_YDAY[leap][m] - MON_YDAY[leap][m - 1];
                for _ in 1..r.n {
                    if d + 7 >= mlen {
                        break;
                    }
                    d += 7;
                }
                t += (MON_YDAY[leap][m - 1] + d) * 86_400;
            }
        }
        t - r.offset + r.secs
    }

    /// `__tz_compute` com `use_localtime`: (deslocamento, isdst, abreviação). `None` quando a hora
    /// UTC não cabe na `struct tm`.
    fn compute(&self, t: i64) -> Option<(i64, i32, Vec<u8>)> {
        let ut = offtime(t, 0)?;
        let c0 = self.change(0, ut.year);
        let c1 = self.change(1, ut.year);
        let isdst = if c0 > c1 { t < c1 || t >= c0 } else { t >= c0 && t < c1 };
        let i = usize::from(isdst);
        Some((self.rules[i].offset, i32::from(isdst), self.rules[i].name.clone()))
    }
}

#[derive(Clone, Copy, Debug)]
struct TType {
    off: i64,
    isdst: bool,
    idx: usize,
}

/// Um TZif lido.
#[derive(Clone, Debug)]
pub struct TzFile {
    transitions: Vec<i64>,
    type_idxs: Vec<usize>,
    types: Vec<TType>,
    chars: Vec<u8>,
    spec: Option<PosixTz>,
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn be64(b: &[u8]) -> i64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[..8]);
    i64::from_be_bytes(a)
}

struct Header {
    version: u8,
    isutcnt: usize,
    isstdcnt: usize,
    leapcnt: usize,
    timecnt: usize,
    typecnt: usize,
    charcnt: usize,
}

fn header(b: &[u8]) -> Option<Header> {
    if b.len() < 44 || &b[..4] != b"TZif" {
        return None;
    }
    let n = |k: usize| be32(&b[20 + 4 * k..]) as usize;
    Some(Header {
        version: b[4],
        isutcnt: n(0),
        isstdcnt: n(1),
        leapcnt: n(2),
        timecnt: n(3),
        typecnt: n(4),
        charcnt: n(5),
    })
}

impl TzFile {
    /// Lê um TZif. Com versão 2 ou mais usa o bloco de 64 bits e o rodapé, como a glibc.
    pub fn parse(b: &[u8]) -> Option<TzFile> {
        let h1 = header(b)?;
        let v1_len = h1.timecnt * 5 + h1.typecnt * 6 + h1.charcnt + h1.leapcnt * 8 + h1.isstdcnt + h1.isutcnt;
        let (h, data, tsize) = if h1.version >= b'2' {
            let rest = b.get(44 + v1_len..)?;
            let h2 = header(rest)?;
            (h2, rest.get(44..)?, 8)
        } else {
            (h1, b.get(44..)?, 4)
        };
        if h.typecnt == 0 || h.charcnt == 0 {
            return None;
        }
        let mut p = 0;
        fn take<'d>(data: &'d [u8], p: &mut usize, n: usize) -> Option<&'d [u8]> {
            let s = data.get(*p..p.checked_add(n)?)?;
            *p += n;
            Some(s)
        }
        let tr = take(data, &mut p, h.timecnt * tsize)?;
        let transitions: Vec<i64> = tr
            .chunks(tsize)
            .map(|c| if tsize == 8 { be64(c) } else { i64::from(be32(c) as i32) })
            .collect();
        let idx = take(data, &mut p, h.timecnt)?;
        let type_idxs: Vec<usize> = idx.iter().map(|&i| usize::from(i)).collect();
        if type_idxs.iter().any(|&i| i >= h.typecnt) {
            return None;
        }
        let tt = take(data, &mut p, h.typecnt * 6)?;
        let mut types = Vec::new();
        for c in tt.chunks(6) {
            let idx = usize::from(c[5]);
            if idx >= h.charcnt {
                return None;
            }
            types.push(TType {
                off: i64::from(be32(c) as i32),
                isdst: c[4] != 0,
                idx,
            });
        }
        let chars = take(data, &mut p, h.charcnt)?.to_vec();
        take(data, &mut p, h.leapcnt * (tsize + 4) + h.isstdcnt + h.isutcnt)?;
        let mut spec = None;
        if tsize == 8 && data.get(p) == Some(&b'\n') {
            let rest = &data[p + 1..];
            if let Some(end) = rest.iter().position(|&c| c == b'\n')
                && end > 0 {
                    spec = Some(PosixTz::parse(&rest[..end]));
                }
        }
        Some(TzFile {
            transitions,
            type_idxs,
            types,
            chars,
            spec,
        })
    }

    fn abbr(&self, idx: usize) -> Vec<u8> {
        let s = &self.chars[idx..];
        let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
        s[..end].to_vec()
    }

    fn type_info(&self, i: usize) -> (i64, i32, Vec<u8>) {
        let ty = self.types[i];
        (ty.off, i32::from(ty.isdst), self.abbr(ty.idx))
    }

    /// `__tzfile_compute`: (deslocamento, isdst, abreviação).
    fn compute(&self, t: i64) -> (i64, i32, Vec<u8>) {
        let n = self.transitions.len();
        if n == 0 || t < self.transitions[0] {
            let mut i = 0;
            while i < self.types.len() && self.types[i].isdst {
                i += 1;
            }
            if i == self.types.len() {
                i = 0;
            }
            return self.type_info(i);
        }
        if t >= self.transitions[n - 1] {
            if let Some(spec) = &self.spec
                && let Some(r) = spec.compute(t) {
                    return r;
                }
            return self.type_info(self.type_idxs[n - 1]);
        }
        // Primeiro índice com transição maior que t.
        let i = self.transitions.partition_point(|&x| x <= t);
        self.type_info(self.type_idxs[i - 1])
    }
}

/// O fuso do processo depois do `tzset`.
#[derive(Clone, Debug)]
pub enum Zone {
    File(TzFile),
    Rules(PosixTz),
}

impl Zone {
    /// `localtime_r`.
    pub fn localtime(&self, t: i64) -> Option<Tm> {
        let (off, isdst, zone) = match self {
            Zone::File(f) => f.compute(t),
            Zone::Rules(p) => p.compute(t)?,
        };
        let mut tm = offtime(t, off)?;
        tm.isdst = isdst;
        tm.zone = zone;
        Some(tm)
    }

    /// `setenv("TZ", value); tzset()`.
    pub fn from_tz(value: &[u8]) -> Zone {
        let mut tz: &[u8] = if value.is_empty() { b"Universal" } else { value };
        if tz.first() == Some(&b':') {
            tz = &tz[1..];
        }
        if let Some(bytes) = read_tzfile(tz)
            && let Some(f) = TzFile::parse(&bytes) {
                return Zone::File(f);
            }
        if tz.is_empty() || tz == b"/etc/localtime" {
            return Zone::Rules(PosixTz::utc_named(b"UTC"));
        }
        Zone::Rules(PosixTz::parse(tz))
    }
}

const DEFAULT_TZDIR: &[u8] = b"/usr/share/zoneinfo";

/// O `__tzfile_read` até o conteúdo do arquivo: caminho relativo vai pra `$TZDIR`.
///
/// Se o sandbox não tiver `/usr/share/zoneinfo` (a imagem sem o pacote tzdata), os nomes do banco
/// saem do tzdata embutido no binário (`jiff-tzdb`), que é o mesmo formato TZif.
fn read_tzfile(name: &[u8]) -> Option<Vec<u8>> {
    let tzdir_env = sys::try_current().and_then(|s| s.getenv(b"TZDIR")).map(|v| v.to_vec());
    let custom = tzdir_env.as_ref().is_some_and(|d| !d.is_empty());
    let path = if name.first() == Some(&b'/') {
        name.to_vec()
    } else {
        let dir = if custom {
            tzdir_env.clone().unwrap_or_default()
        } else {
            DEFAULT_TZDIR.to_vec()
        };
        let mut p = dir;
        p.push(b'/');
        p.extend_from_slice(name);
        p
    };
    match io::read_path(&path) {
        Ok(b) => Some(b),
        Err(Errno::ENOENT) if !custom && name.first() != Some(&b'/') => {
            if io::File::open(DEFAULT_TZDIR).is_ok() {
                return None;
            }
            let text = std::str::from_utf8(name).ok()?;
            let (canon, data) = jiff_tzdb::get(text)?;
            (canon == text).then(|| data.to_vec())
        }
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offtime_limits_and_fields() {
        let tm = offtime(0, 0).unwrap();
        assert_eq!((tm.year, tm.mon, tm.mday, tm.wday, tm.yday), (1970, 0, 1, 4, 0));
        assert!(offtime(i64::MIN, 0).is_none());
        assert!(offtime(i64::MAX, 0).is_none());
        let tm = offtime(-62_135_596_800, 0).unwrap();
        assert_eq!((tm.year, tm.mon, tm.mday), (1, 0, 1));
    }

    #[test]
    fn posix_rules_like_glibc() {
        let ny = Zone::Rules(PosixTz::parse(b"EST5EDT,M3.2.0,M11.1.0"));
        // 2026-07-01 12:00 UTC: EDT.
        let tm = ny.localtime(1_782_907_200).unwrap();
        assert_eq!((tm.hour, tm.isdst, tm.gmtoff, tm.zone.as_slice()), (8, 1, -14_400, &b"EDT"[..]));
        let tm = ny.localtime(1_768_478_400).unwrap();
        assert_eq!((tm.hour, tm.isdst, tm.zone.as_slice()), (7, 0, &b"EST"[..]));
        let foo = Zone::Rules(PosixTz::parse(b"Nowhere/Land"));
        let tm = foo.localtime(0).unwrap();
        assert_eq!((tm.gmtoff, tm.zone.as_slice()), (0, &b"Nowhere"[..]));
        let neg = Zone::Rules(PosixTz::parse(b"<-03>3"));
        let tm = neg.localtime(0).unwrap();
        assert_eq!((tm.gmtoff, tm.zone.as_slice(), tm.hour), (-10_800, &b"-03"[..], 21));
    }

    #[test]
    fn bundled_tzif_reads() {
        let (_, data) = jiff_tzdb::get("America/Sao_Paulo").unwrap();
        let f = Zone::File(TzFile::parse(data).unwrap());
        let tm = f.localtime(1_768_478_400).unwrap();
        assert_eq!((tm.hour, tm.zone.as_slice()), (9, &b"-03"[..]));
        let tm = f.localtime(-2_000_000_000).unwrap();
        assert_eq!(tm.zone.as_slice(), b"LMT");
    }
}
