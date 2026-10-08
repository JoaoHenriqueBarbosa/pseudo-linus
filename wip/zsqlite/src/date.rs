//! `date.c`: as funções SQL de data e hora do SQLite 3.46.1 (`date`, `time`, `datetime`,
//! `julianday`, `unixepoch`, `strftime`, `timediff`, `current_time`, `current_date`,
//! `current_timestamp`), no modelo v2 (ver `CONVENTIONS.md`).
//!
//! Desvios do C, todos decorrentes do modelo v2:
//!
//! * `sqlite3_value **argv` com `argc` vira `&[Mem]` (o `argc` é `argv.len()`). O texto de um
//!   argumento é emprestado quando já é UTF-8 e convertido numa cópia quando não é (ver
//!   [`text_of`]); a cadeia C vira fatia e o fim da fatia faz o papel do NUL (`at()` lê zero
//!   depois do fim, e o texto é cortado no primeiro byte zero antes de ser interpretado).
//! * `current_time()`, `current_date()` e `current_timestamp()` chamam `timeFunc`, `dateFunc` e
//!   `datetimeFunc` com `argc == 0`: aqui chamam as mesmas funções com uma fatia vazia.
//! * `sqlite3NotPureFunc` precisa do `p5` do opcode em execução só para escolher o texto do erro
//!   de `OP_PureFunc` (CHECK, coluna gerada ou índice). O `Context` do modelo v2 não carrega o
//!   `p5`, então a chamada passa zero e o texto do erro sai sempre como "an index". LACUNA
//!   registrada: quando o `Context` ganhar o `p5`, basta trocar o zero em [`not_pure`].
//! * `osLocaltime` usa `localtime_r` da libc no C. Aqui o fuso vem do `sysabi` (a variável `TZ`
//!   do pseudo-processo, ou `local_timezone()`), o arquivo TZif é lido por `sysabi::sys::read_file`
//!   e a conversão é a do glibc: transições do bloco de 64 bits, regra POSIX do rodapé além da
//!   última transição, primeiro tipo padrão antes da primeira. Sem o banco da tzdata embutido do
//!   `jiff` (o crate não depende do `ul-common`), um nome de fuso sem arquivo em
//!   `/usr/share/zoneinfo` e que não seja uma regra POSIX válida cai em UTC, como o glibc.
//! * Sem `SQLITE_OMIT_DATETIME_FUNCS` e sem `SQLITE_DEBUG` (`datedebug` não existe). O gancho de
//!   teste `bLocaltimeFault`/`xAltLocaltime` (`SQLITE_TESTCTRL_LOCALTIME_FAULT`) não existe aqui:
//!   o `global` do projeto ainda não o expõe. LACUNA registrada.

use std::borrow::Cow;
use std::cell::Cell;
use std::rc::Rc;

use crate::connection::{Context, FuncDef, ScalarFn, UserData};
use crate::consts::{
    SQLITE_ERROR, SQLITE_FLOAT, SQLITE_FUNC_BUILTIN, SQLITE_FUNC_CONSTANT, SQLITE_FUNC_SLOCHNG,
    SQLITE_INTEGER, SQLITE_LIMIT_LENGTH, SQLITE_UTF8,
};
use crate::consts::{MEM_NULL, MEM_STR};
use crate::ctype::{is_digit, is_space, to_lower};
use crate::mem::{value_type, Mem, StrDtor, ENC_UTF8, USE_LONG_DOUBLE};
use crate::printf::{result_str_accum, PrintfArg, StrAccum};
use crate::util::{at, atof, str_icmp, strlen30, strnicmp};
use crate::vdbeapi::text_of;
use crate::vdbeapi::{
    result_double, result_error, result_int64, result_text, stmt_current_time, value_double,
    value_text,
};
use crate::vdbeaux3::not_pure_func;

// ---------------------------------------------------------------------------------------------
// Auxiliares do modelo v2
// ---------------------------------------------------------------------------------------------


/// O texto até o primeiro NUL (a cadeia C).
fn cstr(z: &[u8]) -> &[u8] {
    let n = z.iter().position(|&c| c == 0).unwrap_or(z.len());
    &z[..n]
}

/// `z + n` do C: a fatia a partir de `n`, vazia se `n` passa do fim.
fn off(z: &[u8], n: usize) -> &[u8] {
    &z[n.min(z.len())..]
}

/// `sqlite3NotPureFunc` (ver o desvio do `p5` no cabeçalho): verdadeiro se a função pode seguir.
fn not_pure(ctx: &mut Context<'_>) -> bool {
    not_pure_func(ctx, 0) != 0
}

// ---------------------------------------------------------------------------------------------
// chunk 000: DateTime, getDigits, parseTimezone, parseHhMmSs, computeJD, computeFloor
// ---------------------------------------------------------------------------------------------

/// `struct DateTime`: uma data e hora.
#[derive(Clone, Copy, Default)]
struct DateTime {
    /// O número do dia juliano vezes 86400000.
    i_jd: i64,
    /// Ano.
    year: i32,
    /// Mês.
    month: i32,
    /// Dia.
    day: i32,
    /// Hora.
    hour: i32,
    /// Minuto.
    minute: i32,
    /// Deslocamento do fuso em minutos.
    tz: i32,
    /// Segundos.
    s: f64,
    /// `iJD` é válido.
    valid_jd: bool,
    /// `Y`, `M`, `D` são válidos.
    valid_ymd: bool,
    /// `h`, `m`, `s` são válidos.
    valid_hms: bool,
    /// Dias a recuar para implementar "floor".
    n_floor: i32,
    /// Valor numérico bruto guardado em `s`.
    raw_s: bool,
    /// Houve estouro.
    is_error: bool,
    /// Mostrar precisão de subsegundo.
    use_subsec: bool,
    /// O horário é sabidamente UTC.
    is_utc: bool,
    /// O horário é sabidamente local.
    is_local: bool,
}

/// `getDigits`: converte `z` em inteiros conforme `fmt` (quatro caracteres por inteiro, o último
/// com três; ver o comentário do C) e devolve quantas conversões deram certo.
fn get_digits(z: &[u8], fmt: &[u8], out: &mut [i32]) -> i32 {
    // A tradução do terceiro caractere de cada especificação num máximo: a b c d e f.
    const A_MX: [i32; 6] = [12, 14, 24, 31, 59, 14712];
    let mut cnt = 0i32;
    let mut zp = 0usize;
    let mut fp = 0usize;
    loop {
        let n = at(fmt, fp).wrapping_sub(b'0');
        let min = at(fmt, fp + 1).wrapping_sub(b'0') as i32;
        debug_assert!(at(fmt, fp + 2) >= b'a' && at(fmt, fp + 2) <= b'f');
        let max = A_MX[(at(fmt, fp + 2).wrapping_sub(b'a') as usize).min(5)];
        let next_c = at(fmt, fp + 3);
        let mut val = 0i32;
        for _ in 0..n {
            if !is_digit(at(z, zp)) {
                return cnt;
            }
            val = val * 10 + (at(z, zp) - b'0') as i32;
            zp += 1;
        }
        if val < min || val > max || (next_c != 0 && next_c != at(z, zp)) {
            return cnt;
        }
        out[cnt as usize] = val;
        zp += 1;
        cnt += 1;
        fp += 4;
        if next_c == 0 {
            break;
        }
    }
    cnt
}

/// `parseTimezone`: o sufixo de fuso `(+/-)HH:MM` ou `Z`. Grava os minutos em `p.tz` e devolve 0;
/// devolve diferente de zero em erro de sintaxe. A ausência do sufixo não é erro.
fn parse_timezone(z: &[u8], p: &mut DateTime) -> i32 {
    let mut zp = 0usize;
    let sgn: i32;
    while is_space(at(z, zp)) {
        zp += 1;
    }
    p.tz = 0;
    let c = at(z, zp);
    if c == b'-' {
        sgn = -1;
    } else if c == b'+' {
        sgn = 1;
    } else if c == b'Z' || c == b'z' {
        zp += 1;
        p.is_local = false;
        p.is_utc = true;
        while is_space(at(z, zp)) {
            zp += 1;
        }
        return (at(z, zp) != 0) as i32;
    } else {
        return (c != 0) as i32;
    }
    zp += 1;
    let mut v = [0i32; 2];
    if get_digits(off(z, zp), b"20b:20e", &mut v) != 2 {
        return 1;
    }
    zp += 5;
    p.tz = sgn * (v[1] + v[0] * 60);
    while is_space(at(z, zp)) {
        zp += 1;
    }
    (at(z, zp) != 0) as i32
}

/// `parseHhMmSs`: horas no formato HH:MM, HH:MM:SS ou HH:MM:SS.FFFF. Devolve 1 em erro de
/// sintaxe e 0 se deu certo.
fn parse_hh_mm_ss(z: &[u8], p: &mut DateTime) -> i32 {
    let mut v = [0i32; 2];
    let s: i32;
    let mut ms = 0.0f64;
    if get_digits(z, b"20c:20e", &mut v) != 2 {
        return 1;
    }
    let mut zp = 5usize;
    if at(z, zp) == b':' {
        zp += 1;
        let mut sv = [0i32; 1];
        if get_digits(off(z, zp), b"20e", &mut sv) != 1 {
            return 1;
        }
        s = sv[0];
        zp += 2;
        if at(z, zp) == b'.' && is_digit(at(z, zp + 1)) {
            let mut r_scale = 1.0f64;
            zp += 1;
            while is_digit(at(z, zp)) {
                ms = ms * 10.0 + at(z, zp) as f64 - b'0' as f64;
                r_scale *= 10.0;
                zp += 1;
            }
            ms /= r_scale;
        }
    } else {
        s = 0;
    }
    p.valid_jd = false;
    p.raw_s = false;
    p.valid_hms = true;
    p.hour = v[0];
    p.minute = v[1];
    p.s = s as f64 + ms;
    if parse_timezone(off(z, zp), p) != 0 {
        return 1;
    }
    0
}

/// `datetimeError`: põe o objeto no estado de erro.
fn datetime_error(p: &mut DateTime) {
    *p = DateTime::default();
    p.is_error = true;
}

/// `computeJD`: de AAAA-MM-DD HH:MM:SS para o dia juliano (calendário gregoriano sempre).
///
/// Referência: Meeus, página 61.
fn compute_jd(p: &mut DateTime) {
    if p.valid_jd {
        return;
    }
    let (mut y, mut m, d): (i32, i32, i32);
    if p.valid_ymd {
        y = p.year;
        m = p.month;
        d = p.day;
    } else {
        // Sem AAAA-MM-DD, supõe 2000-01-01.
        y = 2000;
        m = 1;
        d = 1;
    }
    if y < -4713 || y > 9999 || p.raw_s {
        datetime_error(p);
        return;
    }
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let a = y / 100;
    let b = 2 - a + (a / 4);
    let x1 = 36525 * (y + 4716) / 100;
    let x2 = 306001 * (m + 1) / 10000;
    p.i_jd = (((x1 + x2 + d + b) as f64 - 1524.5) * 86400000.0) as i64;
    p.valid_jd = true;
    if p.valid_hms {
        p.i_jd += (p.hour * 3600000 + p.minute * 60000) as i64 + (p.s * 1000.0 + 0.5) as i64;
        if p.tz != 0 {
            p.i_jd -= (p.tz * 60000) as i64;
            p.valid_ymd = false;
            p.valid_hms = false;
            p.tz = 0;
            p.is_utc = true;
            p.is_local = false;
        }
    }
}

/// `computeFloor`: dado AAAA-MM-DD de `p`, vê se o dia do mês estoura e fixa `n_floor` com o
/// número de dias a subtrair para voltar ao fim do mês.
fn compute_floor(p: &mut DateTime) {
    debug_assert!(p.valid_ymd || p.is_error);
    debug_assert!(p.day >= 0 && p.day <= 31);
    debug_assert!(p.month >= 0 && p.month <= 12);
    if p.day <= 28 {
        p.n_floor = 0;
    } else if (1u32.wrapping_shl(p.month as u32)) & 0x15aa != 0 {
        p.n_floor = 0;
    } else if p.month != 2 {
        p.n_floor = (p.day == 31) as i32;
    } else if p.year % 4 != 0 || (p.year % 100 == 0 && p.year % 400 != 0) {
        p.n_floor = p.day - 28;
    } else {
        p.n_floor = p.day - 29;
    }
}

/// `parseYyyyMmDd`: datas AAAA-MM-DD HH:MM:SS.FFF (e as formas mais curtas). Devolve 0 se deu
/// certo e 1 se o texto não é uma data bem formada.
fn parse_yyyy_mm_dd(z: &[u8], p: &mut DateTime) -> i32 {
    let mut z = z;
    let neg: bool;
    if at(z, 0) == b'-' {
        z = off(z, 1);
        neg = true;
    } else {
        neg = false;
    }
    let mut v = [0i32; 3];
    if get_digits(z, b"40f-21a-21d", &mut v) != 3 {
        return 1;
    }
    z = off(z, 10);
    let mut zp = 0usize;
    while is_space(at(z, zp)) || b'T' == at(z, zp) {
        zp += 1;
    }
    let z = off(z, zp);
    if parse_hh_mm_ss(z, p) == 0 {
        // Veio a hora.
    } else if at(z, 0) == 0 {
        p.valid_hms = false;
    } else {
        return 1;
    }
    p.valid_jd = false;
    p.valid_ymd = true;
    p.year = if neg { -v[0] } else { v[0] };
    p.month = v[1];
    p.day = v[2];
    compute_floor(p);
    if p.tz != 0 {
        compute_jd(p);
    }
    0
}

// ---------------------------------------------------------------------------------------------
// chunk 001: setDateTimeToCurrent, parseDateOrTime, computeYMD, computeHMS, localtime
// ---------------------------------------------------------------------------------------------

/// `setDateTimeToCurrent`: põe a hora corrente do VFS (fixa por comando). Devolve o número de
/// erros.
fn set_date_time_to_current(ctx: &mut Context<'_>, p: &mut DateTime) -> i32 {
    p.i_jd = stmt_current_time(ctx);
    if p.i_jd > 0 {
        p.valid_jd = true;
        p.is_utc = true;
        p.is_local = false;
        clear_ymd_hms_tz(p);
        0
    } else {
        1
    }
}

/// `setRawDateNumber`: `r` pode ser um dia juliano ou um número de segundos desde 1970. Se está
/// na faixa de um dia juliano, instala-o como tal e marca `valid_jd`; de todo modo vai para `s`
/// com `raw_s`.
fn set_raw_date_number(p: &mut DateTime, r: f64) {
    p.s = r;
    p.raw_s = true;
    if r >= 0.0 && r < 5373484.5 {
        p.i_jd = (r * 86400000.0 + 0.5) as i64;
        p.valid_jd = true;
    }
}

/// `parseDateOrTime`: interpreta o texto como data. Devolve o número de erros.
fn parse_date_or_time(ctx: &mut Context<'_>, z: &[u8], p: &mut DateTime) -> i32 {
    if parse_yyyy_mm_dd(z, p) == 0 {
        return 0;
    } else if parse_hh_mm_ss(z, p) == 0 {
        return 0;
    } else if str_icmp(z, b"now") == 0 && not_pure(ctx) {
        return set_date_time_to_current(ctx, p);
    }
    let (rc, r) = atof(z, strlen30(z), SQLITE_UTF8 as u8, USE_LONG_DOUBLE);
    if rc > 0 {
        set_raw_date_number(p, r);
        return 0;
    } else if (str_icmp(z, b"subsec") == 0 || str_icmp(z, b"subsecond") == 0) && not_pure(ctx) {
        p.use_subsec = true;
        return set_date_time_to_current(ctx, p);
    }
    1
}

/// O dia juliano de 9999-12-31 23:59:59.999 vezes 86400000 (`INT_464269060799999`).
const INT_464269060799999: i64 = (0x1a640i64 << 32) | 0x1072fdff;

/// `validJulianDay`: verdadeiro se `i_jd` (o dia juliano vezes 86400000) está na faixa.
fn valid_julian_day(i_jd: i64) -> bool {
    i_jd >= 0 && i_jd <= INT_464269060799999
}

/// `computeYMD`: ano, mês e dia a partir do dia juliano.
fn compute_ymd(p: &mut DateTime) {
    if p.valid_ymd {
        return;
    }
    if !p.valid_jd {
        p.year = 2000;
        p.month = 1;
        p.day = 1;
    } else if !valid_julian_day(p.i_jd) {
        datetime_error(p);
        return;
    } else {
        let z = ((p.i_jd + 43200000) / 86400000) as i32;
        let mut a = ((z as f64 - 1867216.25) / 36524.25) as i32;
        a = z + 1 + a - (a / 4);
        let b = a + 1524;
        let c = ((b as f64 - 122.1) / 365.25) as i32;
        let d = (36525 * (c & 32767)) / 100;
        let e = ((b - d) as f64 / 30.6001) as i32;
        let x1 = (30.6001 * e as f64) as i32;
        p.day = b - d - x1;
        p.month = if e < 14 { e - 1 } else { e - 13 };
        p.year = if p.month > 2 { c - 4716 } else { c - 4715 };
    }
    p.valid_ymd = true;
}

/// `computeHMS`: hora, minuto e segundos a partir do dia juliano.
fn compute_hms(p: &mut DateTime) {
    if p.valid_hms {
        return;
    }
    compute_jd(p);
    let day_ms = ((p.i_jd + 43200000) % 86400000) as i32;
    p.s = (day_ms % 60000) as f64 / 1000.0;
    let day_min = day_ms / 60000;
    p.minute = day_min % 60;
    p.hour = day_min / 60;
    p.raw_s = false;
    p.valid_hms = true;
}

/// `computeYMD_HMS`.
fn compute_ymd_hms(p: &mut DateTime) {
    compute_ymd(p);
    compute_hms(p);
}

/// `clearYMD_HMS_TZ`.
fn clear_ymd_hms_tz(p: &mut DateTime) {
    p.valid_ymd = false;
    p.valid_hms = false;
    p.tz = 0;
}

/// O `struct tm` de `localtime_r`, só com os campos que o `toLocaltime` lê.
struct Tm {
    /// `tm_year`: anos desde 1900.
    year: i64,
    /// `tm_mon`: mês de 0 a 11.
    mon: i32,
    /// `tm_mday`.
    mday: i32,
    /// `tm_hour`.
    hour: i32,
    /// `tm_min`.
    min: i32,
    /// `tm_sec`.
    sec: i32,
}

/// `osLocaltime`: o equivalente do `localtime_r`. `None` é erro (o `rc != 0` do C).
fn os_localtime(t: i64) -> Option<Tm> {
    let off = tz_offset_at(t);
    let local = t.checked_add(off as i64)?;
    let days = local.div_euclid(86400);
    let secs = local.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    Some(Tm {
        year: y - 1900,
        mon: m as i32 - 1,
        mday: d as i32,
        hour: (secs / 3600) as i32,
        min: ((secs % 3600) / 60) as i32,
        sec: (secs % 60) as i32,
    })
}

/// `toLocaltime`: supondo que `p` é UTC, move-o para o equivalente em hora local.
fn to_localtime(p: &mut DateTime, ctx: &mut Context<'_>) -> i32 {
    let i_year_diff: i32;
    let t: i64;

    compute_jd(p);
    if p.i_jd < 2108667600i64 * 100000 || p.i_jd > 2130141456i64 * 100000 {
        // O `localtime_r` costuma só funcionar de 1970 a 2037: fora disso mapeia o ano para um
        // equivalente dentro da faixa, calcula e desfaz o mapa.
        let mut x = *p;
        compute_ymd_hms(&mut x);
        i_year_diff = (2000 + x.year % 4) - x.year;
        x.year += i_year_diff;
        x.valid_jd = false;
        compute_jd(&mut x);
        t = x.i_jd / 1000 - 21086676i64 * 10000;
    } else {
        i_year_diff = 0;
        t = p.i_jd / 1000 - 21086676i64 * 10000;
    }
    let Some(s_local) = os_localtime(t) else {
        result_error(ctx, b"local time unavailable", -1);
        return SQLITE_ERROR;
    };
    p.year = (s_local.year + 1900 - i_year_diff as i64) as i32;
    p.month = s_local.mon + 1;
    p.day = s_local.mday;
    p.hour = s_local.hour;
    p.minute = s_local.min;
    p.s = s_local.sec as f64 + (p.i_jd % 1000) as f64 * 0.001;
    p.valid_ymd = true;
    p.valid_hms = true;
    p.valid_jd = false;
    p.raw_s = false;
    p.tz = 0;
    p.is_error = false;
    0
}

/// Uma linha de `aXformType`: as transformações do tipo `NNN days`.
struct XformType {
    /// Comprimento do nome.
    n_name: usize,
    /// Nome da transformação.
    z_name: &'static [u8],
    /// Valor máximo de NNN (um `float` do C, promovido a `double` na comparação).
    r_limit: f32,
    /// Constante da transformação (um `float` do C).
    r_xform: f32,
}

/// `aXformType`.
const A_XFORM_TYPE: [XformType; 6] = [
    XformType { n_name: 6, z_name: b"second", r_limit: 4.6427e+14, r_xform: 1.0 },
    XformType { n_name: 6, z_name: b"minute", r_limit: 7.7379e+12, r_xform: 60.0 },
    XformType { n_name: 4, z_name: b"hour", r_limit: 1.2897e+11, r_xform: 3600.0 },
    XformType { n_name: 3, z_name: b"day", r_limit: 5373485.0, r_xform: 86400.0 },
    XformType { n_name: 5, z_name: b"month", r_limit: 176546.0, r_xform: 30.0 * 86400.0 },
    XformType { n_name: 4, z_name: b"year", r_limit: 14713.0, r_xform: 365.0 * 86400.0 },
];

/// `autoAdjustDate`: se `p` é um número bruto, decide se é dia juliano ou timestamp unix.
fn auto_adjust_date(p: &mut DateTime) {
    if !p.raw_s || p.valid_jd {
        p.raw_s = false;
    } else if p.s >= -21086676i64 as f64 * 10000.0 && p.s <= (25340230i64 * 10000 + 799) as f64 {
        let r = p.s * 1000.0 + 210866760000000.0;
        clear_ymd_hms_tz(p);
        p.i_jd = (r + 0.5) as i64;
        p.valid_jd = true;
        p.raw_s = false;
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 002: parseModifier
// ---------------------------------------------------------------------------------------------

/// `parseModifier`: processa um modificador de data e hora (`NNN days`, `start of month`,
/// `weekday N`, `localtime`, `utc`, ...). Devolve 0 se deu certo e 1 em erro. Se o erro é de
/// chamada de sistema (`localtime`), grava a mensagem no contexto; se é um modificador
/// desconhecido, não grava nada.
fn parse_modifier(
    ctx: &mut Context<'_>,
    z: &[u8],
    n: i32,
    p: &mut DateTime,
    idx: i32,
) -> i32 {
    let mut n = n;
    let mut rc = 1i32;
    let mut z = z;
    match to_lower(at(z, 0)) {
        b'a' => {
            // auto: se há `raw_s`, interpreta como dia juliano ou timestamp unix, conforme a
            // magnitude.
            if str_icmp(z, b"auto") == 0 {
                if idx > 1 {
                    return 1; // IMP: R-33611-57934
                }
                auto_adjust_date(p);
                rc = 0;
            }
        }
        b'c' => {
            // ceiling: resolve o estouro do dia do mês avançando para o mês seguinte. É o padrão,
            // então o modificador não faz nada e existe só por simetria com "floor".
            if str_icmp(z, b"ceiling") == 0 {
                compute_jd(p);
                clear_ymd_hms_tz(p);
                rc = 0;
                p.n_floor = 0;
            }
        }
        b'f' => {
            // floor: resolve o estouro do dia do mês recuando para o fim do mês anterior.
            if str_icmp(z, b"floor") == 0 {
                compute_jd(p);
                p.i_jd -= p.n_floor as i64 * 86400000;
                clear_ymd_hms_tz(p);
                rc = 0;
            }
        }
        b'j' => {
            // julianday: interpreta sempre o número anterior como dia juliano. Se não é o
            // primeiro modificador, ou se o argumento anterior não é numérico na faixa 0..5373484.5,
            // o resultado é NULL.
            if str_icmp(z, b"julianday") == 0 {
                if idx > 1 {
                    return 1; // IMP: R-31176-64601
                }
                if p.valid_jd && p.raw_s {
                    rc = 0;
                    p.raw_s = false;
                }
            }
        }
        b'l' => {
            // localtime: supondo que o valor corrente é UTC, mostra-o em hora local.
            if str_icmp(z, b"localtime") == 0 && not_pure(ctx) {
                rc = if p.is_local { 0 } else { to_localtime(p, ctx) };
                p.is_utc = false;
                p.is_local = true;
            }
        }
        b'u' => {
            // unixepoch: trata o valor corrente de `s` como segundos desde 1970 e converte num
            // dia juliano de verdade.
            if str_icmp(z, b"unixepoch") == 0 && p.raw_s {
                if idx > 1 {
                    return 1; // IMP: R-49255-55373
                }
                let r = p.s * 1000.0 + 210866760000000.0;
                if r >= 0.0 && r < 464269060800000.0 {
                    clear_ymd_hms_tz(p);
                    p.i_jd = (r + 0.5) as i64;
                    p.valid_jd = true;
                    p.raw_s = false;
                    rc = 0;
                }
            } else if str_icmp(z, b"utc") == 0 && not_pure(ctx) {
                if !p.is_utc {
                    let mut cnt = 0i32; // segurança contra laço infinito
                    compute_jd(p);
                    let i_orig_jd = p.i_jd;
                    let mut i_guess = i_orig_jd;
                    let mut i_err = 0i64;
                    loop {
                        let mut nw = DateTime::default();
                        i_guess -= i_err;
                        nw.i_jd = i_guess;
                        nw.valid_jd = true;
                        rc = to_localtime(&mut nw, ctx);
                        if rc != 0 {
                            return rc;
                        }
                        compute_jd(&mut nw);
                        i_err = nw.i_jd - i_orig_jd;
                        let again = i_err != 0 && cnt < 3;
                        cnt += 1;
                        if !again {
                            break;
                        }
                    }
                    *p = DateTime::default();
                    p.i_jd = i_guess;
                    p.valid_jd = true;
                    p.is_utc = true;
                    p.is_local = false;
                }
                rc = 0;
            }
        }
        b'w' => {
            // weekday N: move a data para a próxima ocorrência do dia da semana N (0 é domingo).
            // Se já está nele, não faz nada.
            if strnicmp(Some(z), Some(b"weekday "), 8) == 0 {
                let (arc, r) = atof(off(z, 8), strlen30(off(z, 8)), SQLITE_UTF8 as u8, USE_LONG_DOUBLE);
                if arc > 0 && r >= 0.0 && r < 7.0 && {
                    n = r as i32;
                    n as f64 == r
                } {
                    compute_ymd_hms(p);
                    p.tz = 0;
                    p.valid_jd = false;
                    compute_jd(p);
                    let mut zz = ((p.i_jd + 129600000) / 86400000) % 7;
                    if zz > n as i64 {
                        zz -= 7;
                    }
                    p.i_jd += (n as i64 - zz) * 86400000;
                    clear_ymd_hms_tz(p);
                    rc = 0;
                }
            }
        }
        b's' => {
            // start of TTTTT: recua ao começo do dia, mês ou ano corrente.
            // subsecond, subsec: mostra a precisão de subsegundo em datetime(), unixepoch() e
            // strftime('%s').
            if strnicmp(Some(z), Some(b"start of "), 9) != 0 {
                if str_icmp(z, b"subsec") == 0 || str_icmp(z, b"subsecond") == 0 {
                    p.use_subsec = true;
                    rc = 0;
                }
                return rc;
            }
            if !p.valid_jd && !p.valid_ymd && !p.valid_hms {
                return rc;
            }
            z = off(z, 9);
            compute_ymd(p);
            p.valid_hms = true;
            p.hour = 0;
            p.minute = 0;
            p.s = 0.0;
            p.raw_s = false;
            p.tz = 0;
            p.valid_jd = false;
            if str_icmp(z, b"month") == 0 {
                p.day = 1;
                rc = 0;
            } else if str_icmp(z, b"year") == 0 {
                p.month = 1;
                p.day = 1;
                rc = 0;
            } else if str_icmp(z, b"day") == 0 {
                rc = 0;
            }
        }
        b'+' | b'-' | b'0'..=b'9' => {
            let mut v = [0i32; 3];
            let mut z2 = z;
            let z0 = at(z, 0);
            n = 1;
            while at(z, n as usize) != 0 {
                let c = at(z, n as usize);
                if c == b':' {
                    break;
                }
                if is_space(c) {
                    break;
                }
                if c == b'-' {
                    if n == 5 && get_digits(off(z, 1), b"40f", &mut v) == 1 {
                        break;
                    }
                    if n == 6 && get_digits(off(z, 1), b"50f", &mut v) == 1 {
                        break;
                    }
                }
                n += 1;
            }
            let (arc, mut r) = atof(z, n, SQLITE_UTF8 as u8, USE_LONG_DOUBLE);
            if arc <= 0 {
                debug_assert!(rc == 1);
                return rc;
            }
            if at(z, n as usize) == b'-' {
                // Um modificador (+|-)AAAA-MM-DD soma ou subtrai anos, meses e dias. MM vai de 0
                // a 11 e DD de 0 a 30.
                if z0 != b'+' && z0 != b'-' {
                    return rc; // tem de começar com +/-
                }
                if n == 5 {
                    if get_digits(off(z, 1), b"40f-20a-20d", &mut v) != 3 {
                        return rc;
                    }
                } else {
                    debug_assert!(n == 6);
                    if get_digits(off(z, 1), b"50f-20a-20d", &mut v) != 3 {
                        return rc;
                    }
                    z = off(z, 1);
                }
                let (yy, mm, mut dd) = (v[0], v[1], v[2]);
                if mm >= 12 {
                    return rc; // M na faixa 0..11
                }
                if dd >= 31 {
                    return rc; // D na faixa 0..30
                }
                compute_ymd_hms(p);
                p.valid_jd = false;
                if z0 == b'-' {
                    p.year -= yy;
                    p.month -= mm;
                    dd = -dd;
                } else {
                    p.year += yy;
                    p.month += mm;
                }
                let x = if p.month > 0 { (p.month - 1) / 12 } else { (p.month - 12) / 12 };
                p.year += x;
                p.month -= x * 12;
                compute_floor(p);
                compute_jd(p);
                p.valid_hms = false;
                p.valid_ymd = false;
                p.i_jd += dd as i64 * 86400000;
                if at(z, 11) == 0 {
                    return 0;
                }
                let mut hm = [0i32; 2];
                if is_space(at(z, 11)) && get_digits(off(z, 12), b"20c:20e", &mut hm) == 2 {
                    z2 = off(z, 12);
                    n = 2;
                } else {
                    return rc;
                }
            }
            if at(z2, n as usize) == b':' {
                // Um modificador (+|-)HH:MM:SS.FFF soma ou subtrai horas, minutos, segundos e
                // fração à hora. O ".FFF" e o ":SS.FFF" podem faltar.
                let mut tx = DateTime::default();
                if !is_digit(at(z2, 0)) {
                    z2 = off(z2, 1);
                }
                if parse_hh_mm_ss(z2, &mut tx) != 0 {
                    return rc;
                }
                compute_jd(&mut tx);
                tx.i_jd -= 43200000;
                let day = tx.i_jd / 86400000;
                tx.i_jd -= day * 86400000;
                if z0 == b'-' {
                    tx.i_jd = -tx.i_jd;
                }
                compute_jd(p);
                clear_ymd_hms_tz(p);
                p.i_jd += tx.i_jd;
                return 0;
            }

            // Se chegou aqui, a transformação é uma das formas "+NNN days".
            z = off(z, n as usize);
            let mut zp = 0usize;
            while is_space(at(z, zp)) {
                zp += 1;
            }
            z = off(z, zp);
            n = strlen30(z);
            if n < 3 || n > 10 {
                return rc;
            }
            if to_lower(at(z, (n - 1) as usize)) == b's' {
                n -= 1;
            }
            compute_jd(p);
            debug_assert!(rc == 1);
            let r_rounder = if r < 0.0 { -0.5 } else { 0.5 };
            p.n_floor = 0;
            for (i, xf) in A_XFORM_TYPE.iter().enumerate() {
                if xf.n_name as i32 == n
                    && strnicmp(Some(xf.z_name), Some(z), n) == 0
                    && r > -(xf.r_limit as f64)
                    && r < xf.r_limit as f64
                {
                    match i {
                        4 => {
                            // Tratamento especial para somar meses.
                            compute_ymd_hms(p);
                            p.month += r as i32;
                            let x = if p.month > 0 { (p.month - 1) / 12 } else { (p.month - 12) / 12 };
                            p.year += x;
                            p.month -= x * 12;
                            compute_floor(p);
                            p.valid_jd = false;
                            r -= (r as i32) as f64;
                        }
                        5 => {
                            // Tratamento especial para somar anos.
                            let y = r as i32;
                            compute_ymd_hms(p);
                            debug_assert!(p.month >= 0 && p.month <= 12);
                            p.year += y;
                            compute_floor(p);
                            p.valid_jd = false;
                            r -= (r as i32) as f64;
                        }
                        _ => {}
                    }
                    compute_jd(p);
                    p.i_jd += (r * 1000.0 * xf.r_xform as f64 + r_rounder) as i64;
                    rc = 0;
                    break;
                }
            }
            clear_ymd_hms_tz(p);
        }
        _ => {}
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// chunk 003: isDate e as funções SQL
// ---------------------------------------------------------------------------------------------

/// `isDate`: processa os argumentos de uma função de data. `argv[0]` é o carimbo e `argv[1..]`
/// os modificadores. Grava o resultado em `p`; devolve 0 se deu certo e 1 se houve erro. Sem
/// argumentos supõe "now".
fn is_date(ctx: &mut Context<'_>, argv: &[Mem], p: &mut DateTime) -> i32 {
    *p = DateTime::default();
    if argv.is_empty() {
        if !not_pure(ctx) {
            return 1;
        }
        return set_date_time_to_current(ctx, p);
    }
    let e_type = value_type(&argv[0]);
    if e_type == SQLITE_FLOAT || e_type == SQLITE_INTEGER {
        set_raw_date_number(p, value_double(&argv[0]));
    } else {
        let Some(z) = text_of(&argv[0]) else { return 1 };
        if parse_date_or_time(ctx, cstr(&z), p) != 0 {
            return 1;
        }
    }
    for (i, arg) in argv.iter().enumerate().skip(1) {
        let Some(z) = text_of(arg) else { return 1 };
        if parse_modifier(ctx, cstr(&z), z.len() as i32, p, i as i32) != 0 {
            return 1;
        }
    }
    compute_jd(p);
    if p.is_error || !valid_julian_day(p.i_jd) {
        return 1;
    }
    if argv.len() == 1 && p.valid_ymd && p.day > 28 {
        // Garante que AAAA-MM-DD sai normalizado. Exemplo: 2023-02-31 vira 2023-03-03.
        debug_assert!(p.valid_jd);
        p.valid_ymd = false;
    }
    0
}

/// O dígito decimal de `v` (`'0' + v % 10`).
fn digit(v: i32) -> u8 {
    b'0' + (v % 10) as u8
}

/// `juliandayFunc`: `julianday(CARIMBO, MOD, MOD, ...)`, o dia juliano da data.
fn julianday_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let mut x = DateTime::default();
    if is_date(ctx, argv, &mut x) == 0 {
        compute_jd(&mut x);
        result_double(ctx, x.i_jd as f64 / 86400000.0);
    }
}

/// `unixepochFunc`: `unixepoch(CARIMBO, MOD, MOD, ...)`, os segundos (com fração se houver
/// `subsec`) desde 1970-01-01 00:00:00 GMT.
fn unixepoch_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let mut x = DateTime::default();
    if is_date(ctx, argv, &mut x) == 0 {
        compute_jd(&mut x);
        if x.use_subsec {
            result_double(ctx, (x.i_jd - 21086676i64 * 10000000) as f64 / 1000.0);
        } else {
            result_int64(ctx, x.i_jd / 1000 - 21086676i64 * 10000);
        }
    }
}

/// `datetimeFunc`: `datetime(CARIMBO, MOD, MOD, ...)`, devolve AAAA-MM-DD HH:MM:SS.
fn datetime_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let mut x = DateTime::default();
    if is_date(ctx, argv, &mut x) == 0 {
        let mut z_buf = [0u8; 32];
        let n: usize;
        compute_ymd_hms(&mut x);
        let mut y = x.year;
        if y < 0 {
            y = -y;
        }
        z_buf[1] = digit(y / 1000);
        z_buf[2] = digit(y / 100);
        z_buf[3] = digit(y / 10);
        z_buf[4] = digit(y);
        z_buf[5] = b'-';
        z_buf[6] = digit(x.month / 10);
        z_buf[7] = digit(x.month);
        z_buf[8] = b'-';
        z_buf[9] = digit(x.day / 10);
        z_buf[10] = digit(x.day);
        z_buf[11] = b' ';
        z_buf[12] = digit(x.hour / 10);
        z_buf[13] = digit(x.hour);
        z_buf[14] = b':';
        z_buf[15] = digit(x.minute / 10);
        z_buf[16] = digit(x.minute);
        z_buf[17] = b':';
        if x.use_subsec {
            let s = (1000.0 * x.s + 0.5) as i32;
            z_buf[18] = digit(s / 10000);
            z_buf[19] = digit(s / 1000);
            z_buf[20] = b'.';
            z_buf[21] = digit(s / 100);
            z_buf[22] = digit(s / 10);
            z_buf[23] = digit(s);
            n = 24;
        } else {
            let s = x.s as i32;
            z_buf[18] = digit(s / 10);
            z_buf[19] = digit(s);
            n = 20;
        }
        if x.year < 0 {
            z_buf[0] = b'-';
            result_text(ctx, Some(&z_buf[..n]), n as i32, StrDtor::Transient);
        } else {
            result_text(ctx, Some(&z_buf[1..n]), (n - 1) as i32, StrDtor::Transient);
        }
    }
}

/// `timeFunc`: `time(CARIMBO, MOD, MOD, ...)`, devolve HH:MM:SS.
fn time_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let mut x = DateTime::default();
    if is_date(ctx, argv, &mut x) == 0 {
        let mut z_buf = [0u8; 16];
        let n: usize;
        compute_hms(&mut x);
        z_buf[0] = digit(x.hour / 10);
        z_buf[1] = digit(x.hour);
        z_buf[2] = b':';
        z_buf[3] = digit(x.minute / 10);
        z_buf[4] = digit(x.minute);
        z_buf[5] = b':';
        if x.use_subsec {
            let s = (1000.0 * x.s + 0.5) as i32;
            z_buf[6] = digit(s / 10000);
            z_buf[7] = digit(s / 1000);
            z_buf[8] = b'.';
            z_buf[9] = digit(s / 100);
            z_buf[10] = digit(s / 10);
            z_buf[11] = digit(s);
            n = 12;
        } else {
            let s = x.s as i32;
            z_buf[6] = digit(s / 10);
            z_buf[7] = digit(s);
            n = 8;
        }
        result_text(ctx, Some(&z_buf[..n]), n as i32, StrDtor::Transient);
    }
}

/// `dateFunc`: `date(CARIMBO, MOD, MOD, ...)`, devolve AAAA-MM-DD.
fn date_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let mut x = DateTime::default();
    if is_date(ctx, argv, &mut x) == 0 {
        let mut z_buf = [0u8; 16];
        compute_ymd(&mut x);
        let mut y = x.year;
        if y < 0 {
            y = -y;
        }
        z_buf[1] = digit(y / 1000);
        z_buf[2] = digit(y / 100);
        z_buf[3] = digit(y / 10);
        z_buf[4] = digit(y);
        z_buf[5] = b'-';
        z_buf[6] = digit(x.month / 10);
        z_buf[7] = digit(x.month);
        z_buf[8] = b'-';
        z_buf[9] = digit(x.day / 10);
        z_buf[10] = digit(x.day);
        if x.year < 0 {
            z_buf[0] = b'-';
            result_text(ctx, Some(&z_buf[..11]), 11, StrDtor::Transient);
        } else {
            result_text(ctx, Some(&z_buf[1..11]), 10, StrDtor::Transient);
        }
    }
}

/// `daysAfterJan01`: os dias desde o último 1º de janeiro (Jan01 = 0, Jan02 = 1, ...).
fn days_after_jan01(p_date: &DateTime) -> i32 {
    let mut jan01 = *p_date;
    debug_assert!(jan01.valid_ymd);
    debug_assert!(jan01.valid_hms);
    debug_assert!(p_date.valid_jd);
    jan01.valid_jd = false;
    jan01.month = 1;
    jan01.day = 1;
    compute_jd(&mut jan01);
    ((p_date.i_jd - jan01.i_jd + 43200000) / 86400000) as i32
}

/// `daysAfterMonday`: os dias desde a última segunda-feira (0 é segunda, 6 é domingo).
fn days_after_monday(p_date: &DateTime) -> i32 {
    debug_assert!(p_date.valid_jd);
    (((p_date.i_jd + 43200000) / 86400000) as i32) % 7
}

/// `daysAfterSunday`: os dias desde o último domingo (0 é domingo, 6 é sábado).
fn days_after_sunday(p_date: &DateTime) -> i32 {
    debug_assert!(p_date.valid_jd);
    (((p_date.i_jd + 129600000) / 86400000) as i32) % 7
}

/// O `Int` de `PrintfArg` para um `int` do C.
fn int_arg(v: i32) -> PrintfArg {
    PrintfArg::Int(v as i64)
}

/// `strftimeFunc`: `strftime(FORMATO, CARIMBO, MOD, MOD, ...)`. As conversões estão listadas no
/// comentário do C (`%d %e %f %F %G %g %H %k %I %j %J %l %m %M %p %P %R %s %S %T %u %w %U %V %W %Y
/// %%`).
fn strftime_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let mut x = DateTime::default();
    if argv.is_empty() {
        return;
    }
    let Some(z_fmt_full) = text_of(&argv[0]) else { return };
    if is_date(ctx, &argv[1..], &mut x) != 0 {
        return;
    }
    let z_fmt = cstr(&z_fmt_full);
    let mut s_res = StrAccum::new(ctx.db.a_limit[SQLITE_LIMIT_LENGTH as usize] as u32);

    compute_jd(&mut x);
    compute_ymd_hms(&mut x);
    let mut i = 0usize;
    let mut j = 0usize;
    while at(z_fmt, i) != 0 {
        if at(z_fmt, i) != b'%' {
            i += 1;
            continue;
        }
        if j < i {
            s_res.append(&z_fmt[j..i]);
        }
        i += 1;
        j = i + 1;
        let cf = at(z_fmt, i);
        match cf {
            b'd' | b'e' => {
                s_res.appendf(if cf == b'd' { &b"%02d"[..] } else { &b"%2d"[..] }, &[int_arg(x.day)]);
            }
            b'f' => {
                // Segundos fracionários (não padronizado).
                let mut s = x.s;
                if s > 59.999 {
                    s = 59.999;
                }
                s_res.appendf(b"%06.3f", &[PrintfArg::Double(s)]);
            }
            b'F' => {
                s_res.appendf(
                    b"%04d-%02d-%02d",
                    &[int_arg(x.year), int_arg(x.month), int_arg(x.day)],
                );
            }
            b'G' | b'g' => {
                let mut y = x;
                debug_assert!(y.valid_jd);
                // Move `y` para a quinta-feira da mesma semana de `x`.
                y.i_jd += (3 - days_after_monday(&x)) as i64 * 86400000;
                y.valid_ymd = false;
                compute_ymd(&mut y);
                if cf == b'g' {
                    s_res.appendf(b"%02d", &[int_arg(y.year % 100)]);
                } else {
                    s_res.appendf(b"%04d", &[int_arg(y.year)]);
                }
            }
            b'H' | b'k' => {
                s_res.appendf(if cf == b'H' { &b"%02d"[..] } else { &b"%2d"[..] }, &[int_arg(x.hour)]);
            }
            b'I' | b'l' => {
                let mut h = x.hour;
                if h > 12 {
                    h -= 12;
                }
                if h == 0 {
                    h = 12;
                }
                s_res.appendf(if cf == b'I' { &b"%02d"[..] } else { &b"%2d"[..] }, &[int_arg(h)]);
            }
            b'j' => {
                // Dia do ano: Jan01 é 1, Jan02 é 2, e assim por diante.
                s_res.appendf(b"%03d", &[int_arg(days_after_jan01(&x) + 1)]);
            }
            b'J' => {
                // Dia juliano (não padronizado).
                s_res.appendf(b"%.16g", &[PrintfArg::Double(x.i_jd as f64 / 86400000.0)]);
            }
            b'm' => {
                s_res.appendf(b"%02d", &[int_arg(x.month)]);
            }
            b'M' => {
                s_res.appendf(b"%02d", &[int_arg(x.minute)]);
            }
            b'p' | b'P' => {
                if x.hour >= 12 {
                    s_res.append(if cf == b'p' { b"PM" } else { b"pm" });
                } else {
                    s_res.append(if cf == b'p' { b"AM" } else { b"am" });
                }
            }
            b'R' => {
                s_res.appendf(b"%02d:%02d", &[int_arg(x.hour), int_arg(x.minute)]);
            }
            b's' => {
                if x.use_subsec {
                    s_res.appendf(
                        b"%.3f",
                        &[PrintfArg::Double((x.i_jd - 21086676i64 * 10000000) as f64 / 1000.0)],
                    );
                } else {
                    let i_s = x.i_jd / 1000 - 21086676i64 * 10000;
                    s_res.appendf(b"%lld", &[PrintfArg::Int(i_s)]);
                }
            }
            b'S' => {
                s_res.appendf(b"%02d", &[int_arg(x.s as i32)]);
            }
            b'T' => {
                s_res.appendf(
                    b"%02d:%02d:%02d",
                    &[int_arg(x.hour), int_arg(x.minute), int_arg(x.s as i32)],
                );
            }
            b'u' | b'w' => {
                // Dia da semana. `%u`: 1 a 7, segunda é 1 e domingo é 7. `%w`: 0 a 6, domingo é 0.
                let mut c = (days_after_sunday(&x) as u8).wrapping_add(b'0');
                if c == b'0' && cf == b'u' {
                    c = b'7';
                }
                s_res.append_char(1, c);
            }
            b'U' => {
                // Semana 00-53. O primeiro domingo do ano começa a semana 01.
                s_res.appendf(
                    b"%02d",
                    &[int_arg((days_after_jan01(&x) - days_after_sunday(&x) + 7) / 7)],
                );
            }
            b'V' => {
                // Semana 01-53. A primeira semana com uma quinta-feira é a 01.
                let mut y = x;
                // Ajusta `y` para a quinta-feira da mesma semana de `x`.
                debug_assert!(y.valid_jd);
                y.i_jd += (3 - days_after_monday(&x)) as i64 * 86400000;
                y.valid_ymd = false;
                compute_ymd(&mut y);
                s_res.appendf(b"%02d", &[int_arg(days_after_jan01(&y) / 7 + 1)]);
            }
            b'W' => {
                // Semana 00-53. A primeira segunda-feira do ano começa a semana 01.
                s_res.appendf(
                    b"%02d",
                    &[int_arg((days_after_jan01(&x) - days_after_monday(&x) + 7) / 7)],
                );
            }
            b'Y' => {
                s_res.appendf(b"%04d", &[int_arg(x.year)]);
            }
            b'%' => {
                s_res.append_char(1, b'%');
            }
            _ => {
                s_res.reset();
                return;
            }
        }
        i += 1;
    }
    if j < i {
        s_res.append(&z_fmt[j..i]);
    }
    result_str_accum(ctx, &mut s_res);
}

// ---------------------------------------------------------------------------------------------
// chunk 004: current_*, timediff e o registro
// ---------------------------------------------------------------------------------------------

/// `ctimeFunc`: `current_time()`, o mesmo valor de `time('now')`.
fn ctime_func(ctx: &mut Context<'_>, _argv: &[Mem]) {
    time_func(ctx, &[]);
}

/// `cdateFunc`: `current_date()`, o mesmo valor de `date('now')`.
fn cdate_func(ctx: &mut Context<'_>, _argv: &[Mem]) {
    date_func(ctx, &[]);
}

/// `timediffFunc`: `timediff(DATA1, DATA2)`, o tempo que se soma a DATA2 para chegar a DATA1, no
/// formato `+AAAA-MM-DD HH:MM:SS.SSS` (o `+` vira `-` se DATA1 é anterior a DATA2). Os dois
/// argumentos têm de ser dia juliano ou texto ISO-8601; timestamps unix não valem.
fn timediff_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let sign: u8;
    let mut y: i32;
    let mut m: i32;
    let mut d1 = DateTime::default();
    let mut d2 = DateTime::default();
    if is_date(ctx, &argv[0..1], &mut d1) != 0 {
        return;
    }
    if is_date(ctx, &argv[1..2], &mut d2) != 0 {
        return;
    }
    compute_ymd_hms(&mut d1);
    compute_ymd_hms(&mut d2);
    if d1.i_jd >= d2.i_jd {
        sign = b'+';
        y = d1.year - d2.year;
        if y != 0 {
            d2.year = d1.year;
            d2.valid_jd = false;
            compute_jd(&mut d2);
        }
        m = d1.month - d2.month;
        if m < 0 {
            y -= 1;
            m += 12;
        }
        if m != 0 {
            d2.month = d1.month;
            d2.valid_jd = false;
            compute_jd(&mut d2);
        }
        while d1.i_jd < d2.i_jd {
            m -= 1;
            if m < 0 {
                m = 11;
                y -= 1;
            }
            d2.month -= 1;
            if d2.month < 1 {
                d2.month = 12;
                d2.year -= 1;
            }
            d2.valid_jd = false;
            compute_jd(&mut d2);
        }
        d1.i_jd -= d2.i_jd;
        d1.i_jd += 1486995408i64 * 100000;
    } else {
        // d1 < d2
        sign = b'-';
        y = d2.year - d1.year;
        if y != 0 {
            d2.year = d1.year;
            d2.valid_jd = false;
            compute_jd(&mut d2);
        }
        m = d2.month - d1.month;
        if m < 0 {
            y -= 1;
            m += 12;
        }
        if m != 0 {
            d2.month = d1.month;
            d2.valid_jd = false;
            compute_jd(&mut d2);
        }
        while d1.i_jd > d2.i_jd {
            m -= 1;
            if m < 0 {
                m = 11;
                y -= 1;
            }
            d2.month += 1;
            if d2.month > 12 {
                d2.month = 1;
                d2.year += 1;
            }
            d2.valid_jd = false;
            compute_jd(&mut d2);
        }
        d1.i_jd = d2.i_jd - d1.i_jd;
        d1.i_jd += 1486995408i64 * 100000;
    }
    clear_ymd_hms_tz(&mut d1);
    compute_ymd_hms(&mut d1);
    let mut s_res = StrAccum::new(100);
    s_res.appendf(
        b"%c%04d-%02d-%02d %02d:%02d:%06.3f",
        &[
            PrintfArg::Char(sign as u32),
            int_arg(y),
            int_arg(m),
            int_arg(d1.day - 1),
            int_arg(d1.hour),
            int_arg(d1.minute),
            PrintfArg::Double(d1.s),
        ],
    );
    result_str_accum(ctx, &mut s_res);
}

/// `ctimestampFunc`: `current_timestamp()`, o mesmo valor de `datetime('now')`.
fn ctimestamp_func(ctx: &mut Context<'_>, _argv: &[Mem]) {
    datetime_func(ctx, &[]);
}

/// Monta um `FuncDef` embutido de data e hora.
fn date_def(name: &str, n_arg: i8, flags: u32, user: UserData, f: ScalarFn) -> FuncDef {
    FuncDef {
        n_arg,
        func_flags: SQLITE_FUNC_BUILTIN | flags,
        p_user_data: user,
        x_s_func: Some(f),
        x_finalize: None,
        x_value: None,
        x_inverse: None,
        z_name: name.as_bytes().to_vec(),
        p_destructor: None,
    }
}

/// Macro `PURE_DATE`: o `pUserData` do C é o endereço de `sqlite3Config` (só importa não ser
/// nulo), aqui `UserData::Int(1)`.
fn pure_date(name: &str, n_arg: i8, f: ScalarFn) -> FuncDef {
    date_def(
        name,
        n_arg,
        SQLITE_FUNC_SLOCHNG | SQLITE_UTF8 as u32 | SQLITE_FUNC_CONSTANT,
        UserData::Int(1),
        f,
    )
}

/// Macro `DFUNCTION`.
fn dfunction(name: &str, n_arg: i8, f: ScalarFn) -> FuncDef {
    date_def(name, n_arg, SQLITE_FUNC_SLOCHNG | SQLITE_UTF8 as u32, UserData::None, f)
}

/// `sqlite3RegisterDateTimeFunctions`: registra as funções de data e hora na tabela das funções
/// embutidas (chamada por `register_builtin_functions`).
pub fn register_date_time_functions() {
    let defs = vec![
        pure_date("julianday", -1, julianday_func),
        pure_date("unixepoch", -1, unixepoch_func),
        pure_date("date", -1, date_func),
        pure_date("time", -1, time_func),
        pure_date("datetime", -1, datetime_func),
        pure_date("strftime", -1, strftime_func),
        pure_date("timediff", 2, timediff_func),
        dfunction("current_time", 0, ctime_func),
        dfunction("current_timestamp", 0, ctimestamp_func),
        dfunction("current_date", 0, cdate_func),
    ];
    crate::callback::insert_builtin_funcs(defs);
}

// ---------------------------------------------------------------------------------------------
// Fuso local: o `localtime_r` do glibc sobre o `sysabi`
// ---------------------------------------------------------------------------------------------

/// `days_from_civil`: dias desde 1970-01-01 do dia `y-m-d` do calendário gregoriano proléptico.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// `civil_from_days`: a inversa de [`days_from_civil`], devolve `(ano, mês 1..12, dia)`.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Ano bissexto no calendário gregoriano.
fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

/// Dias do mês `m` (1..12) do ano `y`.
fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// O dia em que uma regra de transição POSIX cai.
enum RuleDay {
    /// `Jn`: dia 1..365, sem contar o 29 de fevereiro.
    Julian1(i64),
    /// `n`: dia 0..365, contando o 29 de fevereiro.
    Julian0(i64),
    /// `Mm.w.d`: o dia `d` (0 é domingo) da semana `w` (5 é a última) do mês `m`.
    Month { m: i64, w: i64, d: i64 },
}

/// Uma regra de transição POSIX: o dia e a hora local (em segundos) da virada.
struct Rule {
    /// O dia.
    day: RuleDay,
    /// A hora local da virada, em segundos.
    time: i64,
}

impl Rule {
    /// O dia (desde 1970-01-01) da virada no ano `y`.
    fn epoch_day(&self, y: i64) -> i64 {
        let jan1 = days_from_civil(y, 1, 1);
        match self.day {
            RuleDay::Julian1(n) => jan1 + (n - 1) + (is_leap(y) && n >= 60) as i64,
            RuleDay::Julian0(n) => jan1 + n,
            RuleDay::Month { m, w, d } => {
                let first = days_from_civil(y, m, 1);
                // 1970-01-01 foi uma quinta-feira (4, com domingo em 0).
                let wd_first = (first + 4).rem_euclid(7);
                let day = first + (d - wd_first).rem_euclid(7) + (w - 1) * 7;
                if day >= first + days_in_month(y, m) { day - 7 } else { day }
            }
        }
    }
}

/// O horário de verão de uma regra POSIX.
struct PosixDst {
    /// Deslocamento a leste de UTC, em segundos.
    east: i64,
    /// A virada para o horário de verão.
    start: Rule,
    /// A volta ao horário padrão.
    end: Rule,
}

/// Uma regra POSIX de `TZ` (`EST5EDT,M3.2.0,M11.1.0`).
struct PosixTz {
    /// Deslocamento padrão a leste de UTC, em segundos.
    std_east: i64,
    /// O horário de verão, se há.
    dst: Option<PosixDst>,
}

impl PosixTz {
    /// O deslocamento (a leste de UTC, em segundos) vigente no instante `t`.
    fn offset_at(&self, t: i64) -> i64 {
        let Some(dst) = &self.dst else { return self.std_east };
        let (y, _, _) = civil_from_days((t + self.std_east).div_euclid(86400));
        let s_utc = dst.start.epoch_day(y) * 86400 + dst.start.time - self.std_east;
        let e_utc = dst.end.epoch_day(y) * 86400 + dst.end.time - dst.east;
        let in_dst = if s_utc < e_utc { t >= s_utc && t < e_utc } else { !(t >= e_utc && t < s_utc) };
        if in_dst { dst.east } else { self.std_east }
    }
}

/// O nome de fuso de uma regra POSIX: três ou mais letras, ou `<...>`.
fn px_name(s: &[u8], p: &mut usize) -> Option<()> {
    if at(s, *p) == b'<' {
        let start = *p + 1;
        let mut q = start;
        while q < s.len() && s[q] != b'>' {
            q += 1;
        }
        if q >= s.len() || q == start {
            return None;
        }
        *p = q + 1;
        Some(())
    } else {
        let start = *p;
        while at(s, *p).is_ascii_alphabetic() {
            *p += 1;
        }
        if *p - start < 3 { None } else { Some(()) }
    }
}

/// Um inteiro decimal sem sinal (ao menos um dígito).
fn px_num(s: &[u8], p: &mut usize) -> Option<i64> {
    let start = *p;
    let mut v = 0i64;
    while is_digit(at(s, *p)) {
        v = v.checked_mul(10)?.checked_add((at(s, *p) - b'0') as i64)?;
        *p += 1;
    }
    if *p == start { None } else { Some(v) }
}

/// Um horário `[+-]hh[:mm[:ss]]`, em segundos.
fn px_time(s: &[u8], p: &mut usize) -> Option<i64> {
    let mut sign = 1i64;
    if at(s, *p) == b'+' {
        *p += 1;
    } else if at(s, *p) == b'-' {
        sign = -1;
        *p += 1;
    }
    let mut secs = px_num(s, p)? * 3600;
    if at(s, *p) == b':' {
        *p += 1;
        secs += px_num(s, p)? * 60;
        if at(s, *p) == b':' {
            *p += 1;
            secs += px_num(s, p)?;
        }
    }
    Some(sign * secs)
}

/// Uma regra de transição `Mm.w.d`, `Jn` ou `n`, com `/hora` opcional (padrão 02:00:00).
fn px_rule(s: &[u8], p: &mut usize) -> Option<Rule> {
    let day = match at(s, *p) {
        b'M' => {
            *p += 1;
            let m = px_num(s, p)?;
            if at(s, *p) != b'.' {
                return None;
            }
            *p += 1;
            let w = px_num(s, p)?;
            if at(s, *p) != b'.' {
                return None;
            }
            *p += 1;
            let d = px_num(s, p)?;
            if !(1..=12).contains(&m) || !(1..=5).contains(&w) || !(0..=6).contains(&d) {
                return None;
            }
            RuleDay::Month { m, w, d }
        }
        b'J' => {
            *p += 1;
            let n = px_num(s, p)?;
            if !(1..=365).contains(&n) {
                return None;
            }
            RuleDay::Julian1(n)
        }
        c if is_digit(c) => {
            let n = px_num(s, p)?;
            if n > 365 {
                return None;
            }
            RuleDay::Julian0(n)
        }
        _ => return None,
    };
    let mut time = 7200;
    if at(s, *p) == b'/' {
        *p += 1;
        time = px_time(s, p)?;
    }
    Some(Rule { day, time })
}

/// Interpreta uma regra POSIX de `TZ`. Sem as regras de virada, vale a norma dos EUA
/// (`M3.2.0,M11.1.0`), como no glibc.
fn parse_posix_tz(s: &[u8]) -> Option<PosixTz> {
    let mut p = 0usize;
    px_name(s, &mut p)?;
    let std_east = -px_time(s, &mut p)?;
    if p >= s.len() {
        return Some(PosixTz { std_east, dst: None });
    }
    px_name(s, &mut p)?;
    let c = at(s, p);
    let east = if is_digit(c) || c == b'+' || c == b'-' { -px_time(s, &mut p)? } else { std_east + 3600 };
    let (start, end);
    if at(s, p) == b',' {
        p += 1;
        start = px_rule(s, &mut p)?;
        if at(s, p) != b',' {
            return None;
        }
        p += 1;
        end = px_rule(s, &mut p)?;
    } else {
        start = Rule { day: RuleDay::Month { m: 3, w: 2, d: 0 }, time: 7200 };
        end = Rule { day: RuleDay::Month { m: 11, w: 1, d: 0 }, time: 7200 };
    }
    if p != s.len() {
        return None;
    }
    Some(PosixTz { std_east, dst: Some(PosixDst { east, start, end }) })
}

/// Um fuso carregado: o conteúdo de um arquivo TZif (ou uma regra POSIX sozinha).
struct TzInfo {
    /// Instantes das transições, em ordem crescente.
    trans: Vec<i64>,
    /// Para cada transição, o índice do tipo que passa a valer.
    idx: Vec<u8>,
    /// Os tipos: deslocamento a leste de UTC em segundos e se é horário de verão.
    types: Vec<(i64, bool)>,
    /// A regra POSIX do rodapé, que vale depois da última transição.
    footer: Option<PosixTz>,
}

impl TzInfo {
    /// UTC puro.
    fn utc() -> TzInfo {
        TzInfo { trans: Vec::new(), idx: Vec::new(), types: vec![(0, false)], footer: None }
    }

    /// O deslocamento (a leste de UTC, em segundos) vigente no instante `t`, como o
    /// `__tzfile_compute` do glibc.
    fn offset_at(&self, t: i64) -> i64 {
        if let Some(f) = &self.footer {
            if self.trans.last().is_none_or(|&last| t >= last) {
                return f.offset_at(t);
            }
        }
        if self.trans.is_empty() || t < self.trans[0] {
            // Antes da primeira transição vale o primeiro tipo que não é horário de verão.
            let i = self.types.iter().position(|ty| !ty.1).unwrap_or(0);
            return self.types.get(i).map_or(0, |ty| ty.0);
        }
        let k = self.trans.partition_point(|&x| x <= t) - 1;
        self.types.get(self.idx[k] as usize).map_or(0, |ty| ty.0)
    }
}

/// Lê um inteiro de 32 bits big endian.
fn be32(d: &[u8], pos: usize) -> Option<u32> {
    let b = d.get(pos..pos.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Lê um inteiro de 64 bits big endian com sinal.
fn be64(d: &[u8], pos: usize) -> Option<i64> {
    let b = d.get(pos..pos.checked_add(8)?)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(b);
    Some(i64::from_be_bytes(a))
}

/// Interpreta um arquivo TZif (versões 1 a 4), usando o bloco de 64 bits quando há.
fn parse_tzif(d: &[u8]) -> Option<TzInfo> {
    if d.get(..4)? != b"TZif" {
        return None;
    }
    let ver = *d.get(4)?;
    // Contagens do cabeçalho: isutcnt, isstdcnt, leapcnt, timecnt, typecnt, charcnt.
    let counts = |h: usize| -> Option<[usize; 6]> {
        let mut c = [0usize; 6];
        for (k, slot) in c.iter_mut().enumerate() {
            *slot = be32(d, h + 20 + 4 * k)? as usize;
        }
        Some(c)
    };
    let mut hdr = 0usize;
    let mut c = counts(hdr)?;
    let mut tsize = 4usize;
    if ver >= b'2' {
        // Pula o bloco de 32 bits.
        let skip = c[3] * 4 + c[3] + c[4] * 6 + c[5] + c[2] * 8 + c[1] + c[0];
        hdr = 44usize.checked_add(skip)?;
        if d.get(hdr..hdr.checked_add(4)?)? != b"TZif" {
            return None;
        }
        c = counts(hdr)?;
        tsize = 8;
    }
    let [isutcnt, isstdcnt, leapcnt, timecnt, typecnt, charcnt] = c;
    let mut pos = hdr + 44;
    let mut trans = Vec::with_capacity(timecnt);
    for _ in 0..timecnt {
        trans.push(if tsize == 8 { be64(d, pos)? } else { be32(d, pos)? as i32 as i64 });
        pos += tsize;
    }
    let idx = d.get(pos..pos.checked_add(timecnt)?)?.to_vec();
    pos += timecnt;
    let mut types = Vec::with_capacity(typecnt);
    for _ in 0..typecnt {
        let off = be32(d, pos)? as i32 as i64;
        let isdst = *d.get(pos + 4)? != 0;
        types.push((off, isdst));
        pos += 6;
    }
    if types.is_empty() || idx.iter().any(|&i| i as usize >= types.len()) {
        return None;
    }
    pos = pos.checked_add(charcnt + leapcnt * (tsize + 4) + isstdcnt + isutcnt)?;
    let mut footer = None;
    if ver >= b'2' && d.get(pos) == Some(&b'\n') {
        let rest = &d[pos + 1..];
        if let Some(end) = rest.iter().position(|&c| c == b'\n') {
            footer = parse_posix_tz(&rest[..end]);
        }
    }
    Some(TzInfo { trans, idx, types, footer })
}

/// O valor de `TZ` do pseudo-processo, ou o fuso local do sandbox quando `TZ` não existe.
fn tz_spec() -> Vec<u8> {
    match sysabi::sys::try_current() {
        Some(s) => s.getenv(b"TZ").unwrap_or_else(|| s.local_timezone()),
        None => Vec::new(),
    }
}

/// Carrega o fuso descrito por `spec` (formato da variável `TZ`): vazio ou `UTC` é UTC; nome de
/// arquivo (absoluto, ou relativo a `/usr/share/zoneinfo`) é lido como TZif; senão tenta a regra
/// POSIX; o que não for entendido vira UTC, como no glibc.
fn load_tz(spec: &[u8]) -> TzInfo {
    let s = spec.strip_prefix(b":").unwrap_or(spec);
    if s.is_empty() || s == b"UTC" {
        return TzInfo::utc();
    }
    let path: Option<Vec<u8>> = if s[0] == b'/' {
        Some(s.to_vec())
    } else if s.split(|&c| c == b'/').any(|c| c == b"..") {
        None
    } else {
        let mut p = b"/usr/share/zoneinfo/".to_vec();
        p.extend_from_slice(s);
        Some(p)
    };
    if let Some(path) = path {
        if sysabi::sys::try_current().is_some() {
            if let Some(tz) = sysabi::sys::read_file(&path).ok().and_then(|data| parse_tzif(&data)) {
                return tz;
            }
        }
    }
    if s[0] != b'/' {
        if let Some(px) = parse_posix_tz(s) {
            return TzInfo {
                trans: Vec::new(),
                idx: Vec::new(),
                types: vec![(px.std_east, false)],
                footer: Some(px),
            };
        }
    }
    TzInfo::utc()
}

thread_local! {
    /// O último fuso carregado, com a especificação que o originou.
    static TZ_CACHE: Cell<Option<(Vec<u8>, Rc<TzInfo>)>> = const { Cell::new(None) };
}

/// O fuso corrente, relido só quando a especificação (`TZ` ou o fuso local) muda.
fn current_tz() -> Rc<TzInfo> {
    let spec = tz_spec();
    let cached = TZ_CACHE.with(|c| {
        let v = c.take();
        let hit = match &v {
            Some((k, tz)) if *k == spec => Some(tz.clone()),
            _ => None,
        };
        c.set(v);
        hit
    });
    if let Some(tz) = cached {
        return tz;
    }
    let tz = Rc::new(load_tz(&spec));
    TZ_CACHE.with(|c| c.set(Some((spec, tz.clone()))));
    tz
}

/// O deslocamento do fuso local, em segundos a leste de UTC, no instante `t` (segundos unix).
fn tz_offset_at(t: i64) -> i64 {
    current_tz().offset_at(t)
}
