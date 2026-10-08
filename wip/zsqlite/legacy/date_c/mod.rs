// Mesclado das partes traduzidas de date_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Estrutura para manter uma data e hora individual.
#[derive(Clone, Debug)]
pub struct DateTime {
    /// O número do dia juliano multiplicado por 86400000.
    pub i_jd: i64,
    /// Ano, mês e dia.
    pub y: i32,
    pub m: i32,
    pub d: i32,
    /// Hora e minuto.
    pub h: i32,
    pub min: i32,
    /// Deslocamento de fuso horário em minutos.
    pub tz: i32,
    /// Segundos.
    pub s: f64,
    /// Verdadeiro (1) se i_jd é válido.
    pub valid_jd: bool,
    /// Verdadeiro (1) se Y, M, D são válidos.
    pub valid_ymd: bool,
    /// Verdadeiro (1) se h, m, s são válidos.
    pub valid_hms: bool,
    /// Dias para implementar "floor".
    pub n_floor: i32,
    /// Valor numérico bruto armazenado em s.
    pub raw_s: bool,
    /// Um overflow ocorreu.
    pub is_error: bool,
    /// Exibir precisão de subsegundo.
    pub use_subsec: bool,
    /// A hora é conhecida como UTC.
    pub is_utc: bool,
    /// A hora é conhecida como horário local.
    pub is_local: bool,
}

impl DateTime {
    /// Cria uma nova instância de DateTime com valores padrão.
    pub fn new() -> Self {
        DateTime {
            i_jd: 0,
            y: 0,
            m: 0,
            d: 0,
            h: 0,
            min: 0,
            tz: 0,
            s: 0.0,
            valid_jd: false,
            valid_ymd: false,
            valid_hms: false,
            n_floor: 0,
            raw_s: false,
            is_error: false,
            use_subsec: false,
            is_utc: false,
            is_local: false,
        }
    }
}

/// Máximos para cada especificador de formato em getDigits.
/// Os valores correspondem aos caracteres de formato 'a' a 'f': 12, 14, 24, 31, 59, 14712.
const A_MX: &[u16] = &[12, 14, 24, 31, 59, 14712];

/// Converte zDate em um ou mais inteiros de acordo com o especificador de conversão zFormat.
///
/// zFormat[] contém 4 caracteres para cada inteiro convertido, exceto para
/// o último inteiro que é especificado por três caracteres. O significado
/// de um especificador de formato de quatro caracteres ABCD é:
///
///    A: número de dígitos para converter. Sempre "2" ou "4".
///    B: valor mínimo. Sempre "0" ou "1".
///    C: valor máximo, decodificado como:
///           a: 12
///           b: 14
///           c: 24
///           d: 31
///           e: 59
///           f: 9999
///    D: o caractere separador, ou \000 para indicar este é o
///       último número para converter.
///
/// Exemplo: Para traduzir uma data ISO-8601 YYYY-MM-DD, o formato seria
/// "40f-21a-20c". O "40f-" indica o ano de 4 dígitos seguido por "-".
/// O "21a-" indica o mês de 2 dígitos seguido por "-".
/// O "20c" indica o dia de 2 dígitos que é o último inteiro no conjunto.
///
/// A função retorna o número de conversões bem sucedidas.
pub fn get_digits(
    mut z_date: &[u8],
    z_format: &[u8],
    values: &mut [Option<i32>],
) -> usize {
    let mut cnt = 0;
    let mut format_idx = 0;

    while format_idx < z_format.len() && cnt < values.len() {
        // O último especificador tem só três caracteres (sem separador).
        if format_idx + 2 >= z_format.len() {
            break;
        }

        let n = (z_format[format_idx] - b'0') as usize;
        let min = (z_format[format_idx + 1] - b'0') as i32;
        let max_char = z_format[format_idx + 2];
        let next_c = if format_idx + 3 < z_format.len() {
            z_format[format_idx + 3]
        } else {
            0
        };

        // Validar max_char está em faixa 'a'..'f'.
        if max_char < b'a' || max_char > b'f' {
            break;
        }

        let max = A_MX[(max_char - b'a') as usize] as i32;

        let mut val = 0;
        let mut digits_read = 0;

        // Ler n dígitos.
        while digits_read < n && !z_date.is_empty() {
            let c = z_date[0];
            if !isdigit(c) {
                break;
            }
            val = val * 10 + (c - b'0') as i32;
            z_date = &z_date[1..];
            digits_read += 1;
        }

        // Verificar se conseguimos ler n dígitos e se o valor está na faixa.
        if digits_read < n || val < min || val > max {
            break;
        }

        // Verificar o separador (se esperado).
        if next_c != 0 {
            if z_date.is_empty() || z_date[0] != next_c {
                break;
            }
            z_date = &z_date[1..];
        }

        values[cnt] = Some(val);
        cnt += 1;
        format_idx += 4;
        if next_c == 0 {
            break;
        }
    }

    cnt
}

/// Analisa uma extensão de fuso horário no final de uma data-hora.
/// A extensão é da forma:
///
///        (+/-)HH:MM
///
/// Ou a notação "zulu":
///
///        Z
///
/// Se a análise for bem sucedida, escreve o número de minutos
/// de mudança em p.tz e retorna 0. Se um erro de análise ocorrer,
/// retorna um valor diferente de zero.
///
/// Um especificador ausente não é considerado um erro.
pub fn parse_timezone(z_date: &[u8], p: &mut DateTime) -> bool {
    let mut z_date = z_date;

    // Pular espaços em branco.
    while !z_date.is_empty() && isspace(z_date[0]) {
        z_date = &z_date[1..];
    }

    p.tz = 0;

    if z_date.is_empty() {
        return false;
    }

    let c = z_date[0] as char;

    if c == '-' {
        z_date = &z_date[1..];
        let mut values: [Option<i32>; 2] = [None; 2];
        if get_digits(z_date, b"20b:20e", &mut values) != 2 {
            return true;
        }
        let n_hr = values[0].unwrap_or(0);
        let n_mn = values[1].unwrap_or(0);
        p.tz = -(n_mn + n_hr * 60);
        z_date = &z_date[5..];
    } else if c == '+' {
        z_date = &z_date[1..];
        let mut values: [Option<i32>; 2] = [None; 2];
        if get_digits(z_date, b"20b:20e", &mut values) != 2 {
            return true;
        }
        let n_hr = values[0].unwrap_or(0);
        let n_mn = values[1].unwrap_or(0);
        p.tz = n_mn + n_hr * 60;
        z_date = &z_date[5..];
    } else if c == 'Z' || c == 'z' {
        z_date = &z_date[1..];
        p.is_local = false;
        p.is_utc = true;
    } else {
        return !z_date.is_empty();
    }

    // Pular espaços em branco finais.
    while !z_date.is_empty() && isspace(z_date[0]) {
        z_date = &z_date[1..];
    }

    !z_date.is_empty()
}

/// Analisa tempos da forma HH:MM ou HH:MM:SS ou HH:MM:SS.FFFF.
/// O HH, MM e SS devem cada um ser exatamente 2 dígitos. Os
/// segundos fracionários FFFF podem ser um ou mais dígitos.
///
/// Retorna verdadeiro se há um erro de análise e falso no sucesso.
pub fn parse_hh_mm_ss(z_date: &[u8], p: &mut DateTime) -> bool {
    let mut z_date = z_date;
    let mut values: [Option<i32>; 2] = [None; 2];

    if get_digits(z_date, b"20c:20e", &mut values) != 2 {
        return true;
    }

    let h = values[0].unwrap_or(0);
    let m = values[1].unwrap_or(0);
    z_date = &z_date[5..];

    let mut s = 0;
    let mut ms = 0.0;

    if !z_date.is_empty() && z_date[0] == b':' {
        z_date = &z_date[1..];
        let mut s_values: [Option<i32>; 1] = [None; 1];
        if get_digits(z_date, b"20e", &mut s_values) != 1 {
            return true;
        }
        s = s_values[0].unwrap_or(0);
        z_date = &z_date[2..];

        if !z_date.is_empty() && z_date[0] == b'.' && z_date.len() > 1 && isdigit(z_date[1]) {
            let mut r_scale = 1.0;
            z_date = &z_date[1..];
            while !z_date.is_empty() && isdigit(z_date[0]) {
                ms = ms * 10.0 + (z_date[0] - b'0') as f64;
                r_scale *= 10.0;
                z_date = &z_date[1..];
            }
            ms /= r_scale;
        }
    }

    p.valid_jd = false;
    p.raw_s = false;
    p.valid_hms = true;
    p.h = h;
    p.min = m;
    p.s = s as f64 + ms;

    if parse_timezone(z_date, p) {
        return true;
    }

    false
}

/// Coloca o objeto DateTime em seu estado de erro.
pub fn datetime_error(p: &mut DateTime) {
    *p = DateTime::new();
    p.is_error = true;
}

/// Converte de YYYY-MM-DD HH:MM:SS para dia juliano. Sempre assumimos
/// que YYYY-MM-DD está de acordo com o calendário gregoriano.
///
/// Referência: Meeus página 61
pub fn compute_jd(p: &mut DateTime) {
    if p.valid_jd {
        return;
    }

    let (mut y, mut m, d) = if p.valid_ymd {
        (p.y, p.m, p.d)
    } else {
        // Se nenhum YMD especificado, assume 2000-01-01.
        (2000, 1, 1)
    };

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
        p.i_jd += (p.h as i64) * 3600000 + (p.min as i64) * 60000 + (p.s * 1000.0 + 0.5) as i64;
        if p.tz != 0 {
            p.i_jd -= (p.tz as i64) * 60000;
            p.valid_ymd = false;
            p.valid_hms = false;
            p.tz = 0;
            p.is_utc = true;
            p.is_local = false;
        }
    }
}

/// Dada a informação YYYY-MM-DD atual em p, determina se há
/// transbordamento de dia do mês e define n_floor para o número de dias que
/// precisariam ser subtraídos da data para trazer a
/// data de volta para o final do mês.
pub fn compute_floor(p: &mut DateTime) {
    debug_assert!(p.valid_ymd || p.is_error);
    debug_assert!(p.d >= 0 && p.d <= 31);
    debug_assert!(p.m >= 0 && p.m <= 12);

    if p.d <= 28 {
        p.n_floor = 0;
    } else if (1 << p.m) & 0x15aa != 0 {
        // Meses com 31 dias: jan, mar, mai, jul, ago, out, dez
        // Meses com 30 dias: abr, jun, set, nov
        // (1 << 1) | (1 << 3) | (1 << 5) | (1 << 7) | (1 << 8) | (1 << 10) | (1 << 12) = 0x15aa
        p.n_floor = 0;
    } else if p.m != 2 {
        // Mês com 30 dias.
        p.n_floor = if p.d == 31 { 1 } else { 0 };
    } else if p.y % 4 != 0 || (p.y % 100 == 0 && p.y % 400 != 0) {
        // Fevereiro em ano não bissexto.
        p.n_floor = p.d - 28;
    } else {
        // Fevereiro em ano bissexto.
        p.n_floor = p.d - 29;
    }
}

/// Analisa datas da forma
///
///     YYYY-MM-DD HH:MM:SS.FFF
///     YYYY-MM-DD HH:MM:SS
///     YYYY-MM-DD HH:MM
///     YYYY-MM-DD
///
/// Escreve o resultado na estrutura DateTime e retorna falso
/// no sucesso e verdadeiro se a string de entrada não está bem formada.
pub fn parse_yyyy_mm_dd(z_date: &[u8], p: &mut DateTime) -> bool {
    let mut z_date = z_date;

    let neg = if !z_date.is_empty() && z_date[0] == b'-' {
        z_date = &z_date[1..];
        true
    } else {
        false
    };

    let mut values: [Option<i32>; 3] = [None; 3];
    if get_digits(z_date, b"40f-21a-21d", &mut values) != 3 {
        return true;
    }

    let y = values[0].unwrap_or(0);
    let m = values[1].unwrap_or(0);
    let d = values[2].unwrap_or(0);

    z_date = &z_date[10..];

    // Pular espaço em branco e 'T'.
    while !z_date.is_empty() && (isspace(z_date[0]) || z_date[0] == b'T') {
        z_date = &z_date[1..];
    }

    let hms_result = parse_hh_mm_ss(z_date, p);

    if hms_result == false {
        // Conseguimos analisar a hora.
    } else if z_date.is_empty() {
        p.valid_hms = false;
    } else {
        return true;
    }

    p.valid_jd = false;
    p.valid_ymd = true;
    p.y = if neg { -y } else { y };
    p.m = m;
    p.d = d;
    compute_floor(p);

    if p.tz != 0 {
        compute_jd(p);
    }

    false
}


// ---- part_001.rs ----

/// Equivalente do `struct tm` do C (só os campos que o date.c lê). Preenchido pelo `localtime_r`
/// do módulo `os`, que implementa o fuso do sistema sem FFI.
#[derive(Clone, Debug, Default)]
pub struct Tm {
    pub tm_sec: i32,
    pub tm_min: i32,
    pub tm_hour: i32,
    pub tm_mday: i32,
    pub tm_mon: i32,
    pub tm_year: i32,
    pub tm_wday: i32,
    pub tm_yday: i32,
    pub tm_isdst: i32,
}

/// Corta a fatia no primeiro NUL, como o C faz ao tratar `const char*` como string.
pub fn cstr_slice(z: &[u8]) -> &[u8] {
    match z.iter().position(|&c| c == 0) {
        Some(n) => &z[..n],
        None => z,
    }
}

/// Lê o byte `i` de `z` devolvendo NUL além do fim (o terminador implícito do C).
#[inline]
pub fn at(z: &[u8], i: usize) -> u8 {
    z.get(i).copied().unwrap_or(0)
}

/// Define a hora como a hora atual relatada pelo VFS. Retorna o número de erros.
fn set_date_time_to_current(context: &mut Sqlite3Context, p: &mut DateTime) -> i32 {
    p.i_jd = stmt_current_time(context);
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

/// A entrada "r" é uma quantidade numérica que pode ser um número do dia juliano ou o número de
/// segundos desde 1970. Se r está no intervalo de um dia juliano, instala como tal e marca
/// valid_jd. Se for um timestamp Unix válido, coloca em p.s e marca raw_s.
fn set_raw_date_number(p: &mut DateTime, r: f64) {
    p.s = r;
    p.raw_s = true;
    if r >= 0.0 && r < 5373484.5 {
        p.i_jd = (r * 86400000.0 + 0.5) as i64;
        p.valid_jd = true;
    }
}

/// Tenta analisar a string num número do dia juliano. Retorna o número de erros.
///
/// Formas aceitas:
///      YYYY-MM-DD HH:MM:SS.FFF  +/-HH:MM
///      DDDD.DD
///      now
///
/// Na primeira forma, o +/-HH:MM é sempre opcional, assim como os segundos fracionários e a
/// porção de segundos. O ano e a data podem ser omitidos se houver hora, e a hora pode ser
/// omitida se houver ano e data.
fn parse_date_or_time(context: &mut Sqlite3Context, z_date: &[u8], p: &mut DateTime) -> i32 {
    let mut r: f64 = 0.0;
    if !parse_yyyy_mm_dd(z_date, p) {
        0
    } else if !parse_hh_mm_ss(z_date, p) {
        0
    } else if str_i_cmp(z_date, b"now") == 0 && not_pure_func(context) != 0 {
        set_date_time_to_current(context, p)
    } else if ato_f(z_date, &mut r, strlen30(z_date), SQLITE_UTF8) > 0 {
        set_raw_date_number(p, r);
        0
    } else if (str_i_cmp(z_date, b"subsec") == 0 || str_i_cmp(z_date, b"subsecond") == 0)
        && not_pure_func(context) != 0
    {
        p.use_subsec = true;
        set_date_time_to_current(context, p)
    } else {
        1
    }
}

/// O dia juliano de 9999-12-31 23:59:59.999 é 5373484.4999999. Multiplicado por 86400000 dá
/// 464269060799999, o valor máximo de DateTime.i_jd.
pub const INT_464269060799999: i64 = (0x1a640_i64 << 32) | 0x1072fdff;

/// Retorna verdadeiro se o dia juliano (vezes 86400000) está dentro do intervalo.
pub fn valid_julian_day(i_jd: i64) -> bool {
    i_jd >= 0 && i_jd <= INT_464269060799999
}

/// Calcula Ano, Mês e Dia a partir do número do dia juliano.
pub fn compute_ymd(p: &mut DateTime) {
    if p.valid_ymd {
        return;
    }
    if !p.valid_jd {
        p.y = 2000;
        p.m = 1;
        p.d = 1;
    } else if !valid_julian_day(p.i_jd) {
        datetime_error(p);
        return;
    } else {
        let z: i32 = ((p.i_jd + 43200000) / 86400000) as i32;
        let mut a: i32 = ((z as f64 - 1867216.25) / 36524.25) as i32;
        a = z + 1 + a - (a / 4);
        let b: i32 = a + 1524;
        let c: i32 = ((b as f64 - 122.1) / 365.25) as i32;
        let d: i32 = (36525 * (c & 32767)) / 100;
        let e: i32 = ((b - d) as f64 / 30.6001) as i32;
        let x1: i32 = (30.6001 * e as f64) as i32;
        p.d = b - d - x1;
        p.m = if e < 14 { e - 1 } else { e - 13 };
        p.y = if p.m > 2 { c - 4716 } else { c - 4715 };
    }
    p.valid_ymd = true;
}

/// Calcula Hora, Minuto e Segundos a partir do número do dia juliano.
pub fn compute_hms(p: &mut DateTime) {
    if p.valid_hms {
        return;
    }
    compute_jd(p);
    let day_ms: i32 = ((p.i_jd + 43200000) % 86400000) as i32;
    p.s = (day_ms % 60000) as f64 / 1000.0;
    let day_min: i32 = day_ms / 60000;
    p.min = day_min % 60;
    p.h = day_min / 60;
    p.raw_s = false;
    p.valid_hms = true;
}

/// Calcula YMD e HMS.
pub fn compute_ymd_hms(p: &mut DateTime) {
    compute_ymd(p);
    compute_hms(p);
}

/// Limpa YMD, HMS e TZ.
pub fn clear_ymd_hms_tz(p: &mut DateTime) {
    p.valid_ymd = false;
    p.valid_hms = false;
    p.tz = 0;
}

/// Equivalente aproximado do localtime_r() (HAVE_LOCALTIME_R é definido no Debian). Retorna 0 no
/// sucesso e diferente de zero em qualquer erro.
///
/// Se `b_localtime_fault` da configuração global é diferente de zero a rotina sempre falha, ou
/// chama `x_alt_localtime` quando ele existe.
fn os_localtime(t: i64, p_tm: &mut Tm) -> i32 {
    let (fault, alt) = {
        let cfg = global_config();
        (cfg.b_localtime_fault, cfg.x_alt_localtime.clone())
    };
    if fault != 0 {
        return match alt {
            Some(f) => f(t, p_tm),
            None => 1,
        };
    }
    match localtime_r(t) {
        Some(x) => {
            *p_tm = x;
            0
        }
        None => 1,
    }
}

/// Supondo que o DateTime de entrada seja UTC, move-o para o equivalente em hora local.
fn to_localtime(p: &mut DateTime, p_ctx: &mut Sqlite3Context) -> i32 {
    let i_year_diff: i32;
    let t: i64;

    compute_jd(p);
    if p.i_jd < 2108667600_i64 * 100000 /* 1970-01-01 */
        || p.i_jd > 2130141456_i64 * 100000 /* 2038-01-18 */
    {
        // O localtime_r() normalmente só funciona para anos entre 1970 e 2037. Fora disso o
        // SQLite mapeia o ano para um equivalente dentro do intervalo, calcula e desfaz o mapa.
        let mut x = p.clone();
        compute_ymd_hms(&mut x);
        i_year_diff = (2000 + x.y % 4) - x.y;
        x.y += i_year_diff;
        x.valid_jd = false;
        compute_jd(&mut x);
        t = x.i_jd / 1000 - 21086676_i64 * 10000;
    } else {
        i_year_diff = 0;
        t = p.i_jd / 1000 - 21086676_i64 * 10000;
    }
    let mut s_local = Tm::default();
    if os_localtime(t, &mut s_local) != 0 {
        result_error(p_ctx, b"local time unavailable");
        return SQLITE_ERROR;
    }
    p.y = s_local.tm_year + 1900 - i_year_diff;
    p.m = s_local.tm_mon + 1;
    p.d = s_local.tm_mday;
    p.h = s_local.tm_hour;
    p.min = s_local.tm_min;
    p.s = s_local.tm_sec as f64 + (p.i_jd % 1000) as f64 * 0.001;
    p.valid_ymd = true;
    p.valid_hms = true;
    p.valid_jd = false;
    p.raw_s = false;
    p.tz = 0;
    p.is_error = false;
    SQLITE_OK
}

/// Uma linha da tabela de transformações da forma 'NNN days'.
pub struct XformType {
    /// Comprimento do nome.
    pub n_name: u8,
    /// Nome da transformação.
    pub z_name: [u8; 7],
    /// Valor máximo de NNN para esta transformação.
    pub r_limit: f32,
    /// Constante usada nesta transformação.
    pub r_xform: f32,
}

/// A tabela abaixo define as transformações 'NNN days', onde NNN é um número de ponto flutuante
/// arbitrário e "days" pode ser uma de várias unidades de tempo.
pub const A_XFORM_TYPE: [XformType; 6] = [
    XformType { n_name: 6, z_name: *b"second\0", r_limit: 4.6427e+14, r_xform: 1.0 },
    XformType { n_name: 6, z_name: *b"minute\0", r_limit: 7.7379e+12, r_xform: 60.0 },
    XformType { n_name: 4, z_name: *b"hour\0\0\0", r_limit: 1.2897e+11, r_xform: 3600.0 },
    XformType { n_name: 3, z_name: *b"day\0\0\0\0", r_limit: 5373485.0, r_xform: 86400.0 },
    XformType { n_name: 5, z_name: *b"month\0\0", r_limit: 176546.0, r_xform: 30.0 * 86400.0 },
    XformType { n_name: 4, z_name: *b"year\0\0\0", r_limit: 14713.0, r_xform: 365.0 * 86400.0 },
];

/// Se o DateTime p é um número bruto, tenta descobrir se é um dia juliano ou um timestamp Unix
/// e ajusta o valor de p de acordo.
pub fn auto_adjust_date(p: &mut DateTime) {
    if !p.raw_s || p.valid_jd {
        p.raw_s = false;
    } else if p.s >= (-21086676_i64 * 10000) as f64 /* -4713-11-24 12:00:00 */
        && p.s <= ((25340230_i64 * 10000) + 799) as f64 /* 9999-12-31 23:59:59 */
    {
        let r = p.s * 1000.0 + 210866760000000.0;
        clear_ymd_hms_tz(p);
        p.i_jd = (r + 0.5) as i64;
        p.valid_jd = true;
        p.raw_s = false;
    }
}


// ---- part_002.rs ----

/// Processa um modificador de um carimbo de data e hora. Os modificadores são:
///
///     NNN days
///     NNN hours
///     NNN minutes
///     NNN.NNNN seconds
///     NNN months
///     NNN years
///     +/-YYYY-MM-DD HH:MM:SS.SSS
///     ceiling
///     floor
///     start of month
///     start of year
///     start of week
///     start of day
///     weekday N
///     unixepoch
///     auto
///     localtime
///     utc
///     subsec
///     subsecond
///
/// Retorna 0 no sucesso e 1 em qualquer erro. Se o erro está numa chamada de sistema (ex:
/// localtime()), uma mensagem de erro é escrita no contexto p_ctx. Se o modificador não é
/// reconhecido, nada é escrito em p_ctx.
fn parse_modifier(
    p_ctx: &mut Sqlite3Context,
    z: &[u8],
    _n: i32,
    p: &mut DateTime,
    idx: i32,
) -> i32 {
    let z = cstr_slice(z);
    let mut rc: i32 = 1;
    let mut r: f64 = 0.0;
    match UPPER_TO_LOWER[at(z, 0) as usize] {
        b'a' => {
            // auto: se raw_s está disponível, interpreta como dia juliano ou timestamp Unix,
            // dependendo da magnitude.
            if str_i_cmp(z, b"auto") == 0 {
                if idx > 1 {
                    return 1; /* IMP: R-33611-57934 */
                }
                auto_adjust_date(p);
                rc = 0;
            }
        }
        b'c' => {
            // ceiling: resolve o estouro do dia do mês avançando para o mês seguinte. Como é a
            // ação padrão, este modificador é um no-op incluído por simetria com "floor".
            if str_i_cmp(z, b"ceiling") == 0 {
                compute_jd(p);
                clear_ymd_hms_tz(p);
                rc = 0;
                p.n_floor = 0;
            }
        }
        b'f' => {
            // floor: resolve o estouro do dia do mês voltando ao fim do mês anterior.
            if str_i_cmp(z, b"floor") == 0 {
                compute_jd(p);
                p.i_jd -= p.n_floor as i64 * 86400000;
                clear_ymd_hms_tz(p);
                rc = 0;
            }
        }
        b'j' => {
            // julianday: sempre interpreta o número anterior como dia juliano. Se não for o
            // primeiro modificador, ou se o argumento anterior não for numérico no intervalo
            // permitido (0..5373484.5), o resultado é NULL.
            if str_i_cmp(z, b"julianday") == 0 {
                if idx > 1 {
                    return 1; /* IMP: R-31176-64601 */
                }
                if p.valid_jd && p.raw_s {
                    rc = 0;
                    p.raw_s = false;
                }
            }
        }
        b'l' => {
            // localtime: supondo que o valor atual seja UTC (GMT), desloca para hora local.
            if str_i_cmp(z, b"localtime") == 0 && not_pure_func(p_ctx) != 0 {
                rc = if p.is_local { SQLITE_OK } else { to_localtime(p, p_ctx) };
                p.is_utc = false;
                p.is_local = true;
            }
        }
        b'u' => {
            // unixepoch: trata o valor atual de p.s como segundos desde 1970 e converte para um
            // dia juliano de verdade.
            if str_i_cmp(z, b"unixepoch") == 0 && p.raw_s {
                if idx > 1 {
                    return 1; /* IMP: R-49255-55373 */
                }
                r = p.s * 1000.0 + 210866760000000.0;
                if r >= 0.0 && r < 464269060800000.0 {
                    clear_ymd_hms_tz(p);
                    p.i_jd = (r + 0.5) as i64;
                    p.valid_jd = true;
                    p.raw_s = false;
                    rc = 0;
                }
            } else if str_i_cmp(z, b"utc") == 0 && not_pure_func(p_ctx) != 0 {
                if !p.is_utc {
                    let mut cnt: i32 = 0; /* Segurança contra laço infinito */
                    compute_jd(p);
                    let i_orig_jd: i64 = p.i_jd; /* Hora local original */
                    let mut i_guess: i64 = i_orig_jd; /* Palpite do UTC correspondente */
                    let mut i_err: i64 = 0; /* O palpite erra por tanto */
                    loop {
                        let mut new = DateTime::new();
                        i_guess -= i_err;
                        new.i_jd = i_guess;
                        new.valid_jd = true;
                        rc = to_localtime(&mut new, p_ctx);
                        if rc != 0 {
                            return rc;
                        }
                        compute_jd(&mut new);
                        i_err = new.i_jd - i_orig_jd;
                        // while( iErr && cnt++<3 )
                        if i_err == 0 {
                            break;
                        }
                        let c = cnt;
                        cnt += 1;
                        if c >= 3 {
                            break;
                        }
                    }
                    *p = DateTime::new();
                    p.i_jd = i_guess;
                    p.valid_jd = true;
                    p.is_utc = true;
                    p.is_local = false;
                }
                rc = SQLITE_OK;
            }
        }
        b'w' => {
            // weekday N: move a data para a mesma hora na próxima ocorrência do dia da semana N,
            // onde 0==domingo, 1==segunda e assim por diante. Se a data já está no dia certo,
            // é um no-op.
            if str_ni_cmp(z, b"weekday ", 8) == 0 {
                let zr = &z[8..];
                if ato_f(zr, &mut r, strlen30(zr), SQLITE_UTF8) > 0
                    && r >= 0.0
                    && r < 7.0
                    && (r as i32) as f64 == r
                {
                    let n: i64 = (r as i32) as i64;
                    compute_ymd_hms(p);
                    p.tz = 0;
                    p.valid_jd = false;
                    compute_jd(p);
                    let mut zz: i64 = ((p.i_jd + 129600000) / 86400000) % 7;
                    if zz > n {
                        zz -= 7;
                    }
                    p.i_jd += (n - zz) * 86400000;
                    clear_ymd_hms_tz(p);
                    rc = 0;
                }
            }
        }
        b's' => {
            // start of TTTTT: move a data para trás até o início do dia, mês ou ano atual.
            //
            // subsecond / subsec: mostra precisão de subsegundo na saída de datetime(),
            // unixepoch() e strftime('%s').
            if str_ni_cmp(z, b"start of ", 9) != 0 {
                if str_i_cmp(z, b"subsec") == 0 || str_i_cmp(z, b"subsecond") == 0 {
                    p.use_subsec = true;
                    rc = 0;
                }
                return rc;
            }
            if !p.valid_jd && !p.valid_ymd && !p.valid_hms {
                return rc;
            }
            let z = &z[9..];
            compute_ymd(p);
            p.valid_hms = true;
            p.h = 0;
            p.min = 0;
            p.s = 0.0;
            p.raw_s = false;
            p.tz = 0;
            p.valid_jd = false;
            if str_i_cmp(z, b"month") == 0 {
                p.d = 1;
                rc = 0;
            } else if str_i_cmp(z, b"year") == 0 {
                p.m = 1;
                p.d = 1;
                rc = 0;
            } else if str_i_cmp(z, b"day") == 0 {
                rc = 0;
            }
        }
        b'+' | b'-' | b'0'..=b'9' => {
            let mut z = z;
            let mut z2 = z;
            let z0 = z[0];
            let mut n: usize = 1;
            while at(z, n) != 0 {
                if z[n] == b':' {
                    break;
                }
                if isspace(z[n]) {
                    break;
                }
                if z[n] == b'-' {
                    let mut v: [Option<i32>; 1] = [None];
                    if n == 5 && get_digits(&z[1..], b"40f", &mut v) == 1 {
                        break;
                    }
                    if n == 6 && get_digits(&z[1..], b"50f", &mut v) == 1 {
                        break;
                    }
                }
                n += 1;
            }
            if ato_f(&z[..n], &mut r, n as i32, SQLITE_UTF8) <= 0 {
                debug_assert!(rc == 1);
                return rc;
            }
            if at(z, n) == b'-' {
                // Um modificador (+|-)YYYY-MM-DD soma ou subtrai anos, meses e dias. MM vai de
                // 0 a 11 e DD de 0 a 30.
                if z0 != b'+' && z0 != b'-' {
                    return rc; /* Precisa começar com +/- */
                }
                let mut v: [Option<i32>; 3] = [None; 3];
                if n == 5 {
                    if get_digits(&z[1..], b"40f-20a-20d", &mut v) != 3 {
                        return rc;
                    }
                } else {
                    debug_assert!(n == 6);
                    if get_digits(&z[1..], b"50f-20a-20d", &mut v) != 3 {
                        return rc;
                    }
                    z = &z[1..];
                }
                let yy = v[0].unwrap_or(0);
                let mm = v[1].unwrap_or(0);
                let mut dd = v[2].unwrap_or(0);
                if mm >= 12 {
                    return rc; /* M no intervalo 0..11 */
                }
                if dd >= 31 {
                    return rc; /* D no intervalo 0..30 */
                }
                compute_ymd_hms(p);
                p.valid_jd = false;
                if z0 == b'-' {
                    p.y -= yy;
                    p.m -= mm;
                    dd = -dd;
                } else {
                    p.y += yy;
                    p.m += mm;
                }
                let x = if p.m > 0 { (p.m - 1) / 12 } else { (p.m - 12) / 12 };
                p.y += x;
                p.m -= x * 12;
                compute_floor(p);
                compute_jd(p);
                p.valid_hms = false;
                p.valid_ymd = false;
                p.i_jd += dd as i64 * 86400000;
                if at(z, 11) == 0 {
                    return 0;
                }
                let mut v2: [Option<i32>; 2] = [None; 2];
                if isspace(at(z, 11)) && get_digits(&z[12..], b"20c:20e", &mut v2) == 2 {
                    z2 = &z[12..];
                    n = 2;
                } else {
                    return rc;
                }
            }
            if at(z2, n) == b':' {
                // Um modificador (+|-)HH:MM:SS.FFF soma (ou subtrai) horas, minutos, segundos e
                // segundos fracionários. O ".FFF" e o ":SS.FFF" podem ser omitidos.
                if !isdigit(at(z2, 0)) {
                    z2 = &z2[1..];
                }
                let mut tx = DateTime::new();
                if parse_hh_mm_ss(z2, &mut tx) {
                    return rc;
                }
                compute_jd(&mut tx);
                tx.i_jd -= 43200000;
                let day: i64 = tx.i_jd / 86400000;
                tx.i_jd -= day * 86400000;
                if z0 == b'-' {
                    tx.i_jd = -tx.i_jd;
                }
                compute_jd(p);
                clear_ymd_hms_tz(p);
                p.i_jd += tx.i_jd;
                return 0;
            }

            // Se o controle chega aqui, a transformação é de uma forma como "+NNN days".
            z = &z[n..];
            while isspace(at(z, 0)) {
                z = &z[1..];
            }
            let mut n: usize = strlen30(z) as usize;
            if n < 3 || n > 10 {
                return rc;
            }
            if UPPER_TO_LOWER[z[n - 1] as usize] == b's' {
                n -= 1;
            }
            compute_jd(p);
            debug_assert!(rc == 1);
            let r_rounder: f64 = if r < 0.0 { -0.5 } else { 0.5 };
            p.n_floor = 0;
            for i in 0..A_XFORM_TYPE.len() {
                let xf = &A_XFORM_TYPE[i];
                if xf.n_name as usize == n
                    && str_ni_cmp(&xf.z_name, z, n) == 0
                    && r > -(xf.r_limit as f64)
                    && r < xf.r_limit as f64
                {
                    match i {
                        4 => {
                            // Processamento especial para somar meses.
                            compute_ymd_hms(p);
                            p.m += r as i32;
                            let x = if p.m > 0 { (p.m - 1) / 12 } else { (p.m - 12) / 12 };
                            p.y += x;
                            p.m -= x * 12;
                            compute_floor(p);
                            p.valid_jd = false;
                            r -= (r as i32) as f64;
                        }
                        5 => {
                            // Processamento especial para somar anos.
                            let y = r as i32;
                            compute_ymd_hms(p);
                            debug_assert!(p.m >= 0 && p.m <= 12);
                            p.y += y;
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


// ---- part_003.rs ----

/// Processa os argumentos das funções de tempo. argv[0] é um carimbo de data e hora; argv[1] em
/// diante são modificadores. Analisa todos e escreve o tempo resultante no DateTime p. Retorna 0
/// no sucesso e 1 se houver erros.
///
/// Se não há parâmetros (nem mesmo argv[0]), assume o valor padrão "now" para argv[0].
fn is_date(context: &mut Sqlite3Context, argc: i32, argv: &[MemRef], p: &mut DateTime) -> i32 {
    *p = DateTime::new();
    if argc == 0 {
        if not_pure_func(context) == 0 {
            return 1;
        }
        return set_date_time_to_current(context, p);
    }
    let e_type = value_type(&argv[0]);
    if e_type == SQLITE_FLOAT || e_type == SQLITE_INTEGER {
        set_raw_date_number(p, value_double(&argv[0]));
    } else {
        match value_text(&argv[0]) {
            None => return 1,
            Some(z) => {
                if parse_date_or_time(context, cstr_slice(&z), p) != 0 {
                    return 1;
                }
            }
        }
    }
    for i in 1..argc {
        let z = value_text(&argv[i as usize]);
        let n = value_bytes(&argv[i as usize]);
        match z {
            None => return 1,
            Some(z) => {
                if parse_modifier(context, &z, n, p, i) != 0 {
                    return 1;
                }
            }
        }
    }
    compute_jd(p);
    if p.is_error || !valid_julian_day(p.i_jd) {
        return 1;
    }
    if argc == 1 && p.valid_ymd && p.d > 28 {
        // Garante que um YYYY-MM-DD fique normalizado. Exemplo: 2023-02-31 -> 2023-03-03.
        debug_assert!(p.valid_jd);
        p.valid_ymd = false;
    }
    0
}

/// As rotinas a seguir implementam as várias funções de data e hora do SQLite.

/// Converte um dígito decimal em caractere ASCII, como `'0' + (v)%10` do C.
#[inline]
fn digit_char(v: i32) -> u8 {
    (b'0' as i32 + (v % 10)) as u8
}

/// julianday( TIMESTRING, MOD, MOD, ...): devolve o dia juliano da data dos argumentos.
pub fn julianday_func(context: &mut Sqlite3Context, argc: i32, argv: &[MemRef]) {
    let mut x = DateTime::new();
    if is_date(context, argc, argv, &mut x) == 0 {
        compute_jd(&mut x);
        result_double(context, x.i_jd as f64 / 86400000.0);
    }
}

/// unixepoch( TIMESTRING, MOD, MOD, ...): devolve o número de segundos (com frações) desde a
/// época Unix de 1970-01-01 00:00:00 GMT.
pub fn unixepoch_func(context: &mut Sqlite3Context, argc: i32, argv: &[MemRef]) {
    let mut x = DateTime::new();
    if is_date(context, argc, argv, &mut x) == 0 {
        compute_jd(&mut x);
        if x.use_subsec {
            result_double(context, (x.i_jd - 21086676_i64 * 10000000) as f64 / 1000.0);
        } else {
            result_int64(context, x.i_jd / 1000 - 21086676_i64 * 10000);
        }
    }
}

/// datetime( TIMESTRING, MOD, MOD, ...): devolve YYYY-MM-DD HH:MM:SS.
pub fn datetime_func(context: &mut Sqlite3Context, argc: i32, argv: &[MemRef]) {
    let mut x = DateTime::new();
    if is_date(context, argc, argv, &mut x) == 0 {
        let mut z_buf = [0u8; 32];
        compute_ymd_hms(&mut x);
        let mut y = x.y;
        if y < 0 {
            y = -y;
        }
        z_buf[1] = digit_char((y / 1000) % 10);
        z_buf[2] = digit_char((y / 100) % 10);
        z_buf[3] = digit_char((y / 10) % 10);
        z_buf[4] = digit_char(y % 10);
        z_buf[5] = b'-';
        z_buf[6] = digit_char((x.m / 10) % 10);
        z_buf[7] = digit_char(x.m % 10);
        z_buf[8] = b'-';
        z_buf[9] = digit_char((x.d / 10) % 10);
        z_buf[10] = digit_char(x.d % 10);
        z_buf[11] = b' ';
        z_buf[12] = digit_char((x.h / 10) % 10);
        z_buf[13] = digit_char(x.h % 10);
        z_buf[14] = b':';
        z_buf[15] = digit_char((x.min / 10) % 10);
        z_buf[16] = digit_char(x.min % 10);
        z_buf[17] = b':';
        let n: usize;
        if x.use_subsec {
            let s = (1000.0 * x.s + 0.5) as i32;
            z_buf[18] = digit_char((s / 10000) % 10);
            z_buf[19] = digit_char((s / 1000) % 10);
            z_buf[20] = b'.';
            z_buf[21] = digit_char((s / 100) % 10);
            z_buf[22] = digit_char((s / 10) % 10);
            z_buf[23] = digit_char(s % 10);
            z_buf[24] = 0;
            n = 24;
        } else {
            let s = x.s as i32;
            z_buf[18] = digit_char((s / 10) % 10);
            z_buf[19] = digit_char(s % 10);
            z_buf[20] = 0;
            n = 20;
        }
        if x.y < 0 {
            z_buf[0] = b'-';
            result_text(context, &z_buf[..n]);
        } else {
            result_text(context, &z_buf[1..n]);
        }
    }
}

/// time( TIMESTRING, MOD, MOD, ...): devolve HH:MM:SS.
pub fn time_func(context: &mut Sqlite3Context, argc: i32, argv: &[MemRef]) {
    let mut x = DateTime::new();
    if is_date(context, argc, argv, &mut x) == 0 {
        let mut z_buf = [0u8; 16];
        compute_hms(&mut x);
        z_buf[0] = digit_char((x.h / 10) % 10);
        z_buf[1] = digit_char(x.h % 10);
        z_buf[2] = b':';
        z_buf[3] = digit_char((x.min / 10) % 10);
        z_buf[4] = digit_char(x.min % 10);
        z_buf[5] = b':';
        let n: usize;
        if x.use_subsec {
            let s = (1000.0 * x.s + 0.5) as i32;
            z_buf[6] = digit_char((s / 10000) % 10);
            z_buf[7] = digit_char((s / 1000) % 10);
            z_buf[8] = b'.';
            z_buf[9] = digit_char((s / 100) % 10);
            z_buf[10] = digit_char((s / 10) % 10);
            z_buf[11] = digit_char(s % 10);
            z_buf[12] = 0;
            n = 12;
        } else {
            let s = x.s as i32;
            z_buf[6] = digit_char((s / 10) % 10);
            z_buf[7] = digit_char(s % 10);
            z_buf[8] = 0;
            n = 8;
        }
        result_text(context, &z_buf[..n]);
    }
}

/// date( TIMESTRING, MOD, MOD, ...): devolve YYYY-MM-DD.
pub fn date_func(context: &mut Sqlite3Context, argc: i32, argv: &[MemRef]) {
    let mut x = DateTime::new();
    if is_date(context, argc, argv, &mut x) == 0 {
        let mut z_buf = [0u8; 16];
        compute_ymd(&mut x);
        let mut y = x.y;
        if y < 0 {
            y = -y;
        }
        z_buf[1] = digit_char((y / 1000) % 10);
        z_buf[2] = digit_char((y / 100) % 10);
        z_buf[3] = digit_char((y / 10) % 10);
        z_buf[4] = digit_char(y % 10);
        z_buf[5] = b'-';
        z_buf[6] = digit_char((x.m / 10) % 10);
        z_buf[7] = digit_char(x.m % 10);
        z_buf[8] = b'-';
        z_buf[9] = digit_char((x.d / 10) % 10);
        z_buf[10] = digit_char(x.d % 10);
        z_buf[11] = 0;
        if x.y < 0 {
            z_buf[0] = b'-';
            result_text(context, &z_buf[..11]);
        } else {
            result_text(context, &z_buf[1..11]);
        }
    }
}

/// Calcula o número de dias depois do 1º de janeiro mais recente, ou seja, o número do dia
/// do ano começando em zero: Jan01 = 0, Jan02 = 1, ..., Dec31 = 364 ou 365.
fn days_after_jan01(p_date: &DateTime) -> i32 {
    let mut jan01 = p_date.clone();
    debug_assert!(jan01.valid_ymd);
    debug_assert!(jan01.valid_hms);
    debug_assert!(p_date.valid_jd);
    jan01.valid_jd = false;
    jan01.m = 1;
    jan01.d = 1;
    compute_jd(&mut jan01);
    ((p_date.i_jd - jan01.i_jd + 43200000) / 86400000) as i32
}

/// Retorna o número de dias depois da segunda-feira mais recente:
/// 0=segunda, 1=terça, 2=quarta, ..., 6=domingo.
fn days_after_monday(p_date: &DateTime) -> i32 {
    debug_assert!(p_date.valid_jd);
    ((p_date.i_jd + 43200000) / 86400000) as i32 % 7
}

/// Retorna o número de dias depois do domingo mais recente:
/// 0=domingo, 1=segunda, 2=terça, ..., 6=sábado.
fn days_after_sunday(p_date: &DateTime) -> i32 {
    debug_assert!(p_date.valid_jd);
    ((p_date.i_jd + 129600000) / 86400000) as i32 % 7
}

/// strftime( FORMAT, TIMESTRING, MOD, MOD, ...): devolve a string descrita por FORMAT.
///
///   %d  dia do mês  01-31
///   %e  dia do mês  1-31
///   %f  ** segundos fracionários  SS.SSS
///   %F  data ISO.  YYYY-MM-DD
///   %G  ano ISO correspondente a %V 0000-9999.
///   %g  ano ISO de 2 dígitos correspondente a %V 00-99
///   %H  hora 00-24
///   %k  hora  0-24  (zero à esquerda vira espaço)
///   %I  hora 01-12
///   %j  dia do ano 001-366
///   %J  ** dia juliano
///   %l  hora  1-12  (zero à esquerda vira espaço)
///   %m  mês 01-12
///   %M  minuto 00-59
///   %p  "am" ou "pm"
///   %P  "AM" ou "PM"
///   %R  hora como HH:MM
///   %s  segundos desde 1970-01-01
///   %S  segundos 00-59
///   %T  hora como HH:MM:SS
///   %u  dia da semana 1-7  segunda==1, domingo==7
///   %w  dia da semana 0-6  domingo==0, segunda==1
///   %U  semana do ano 00-53  (o primeiro domingo começa a semana 01)
///   %V  semana do ano 01-53  (a primeira semana com uma quinta é a semana 01)
///   %W  semana do ano 00-53  (a primeira segunda começa a semana 01)
///   %Y  ano 0000-9999
///   %%  %
pub fn strftime_func(context: &mut Sqlite3Context, argc: i32, argv: &[MemRef]) {
    let mut x = DateTime::new();
    if argc == 0 {
        return;
    }
    let z_fmt_owned = match value_text(&argv[0]) {
        None => return,
        Some(z) => z,
    };
    if is_date(context, argc - 1, &argv[1..], &mut x) != 0 {
        return;
    }
    let z_fmt = cstr_slice(&z_fmt_owned);
    let db = context_db_handle(context);
    let mut s_res = Sqlite3Str::default();
    let mx_len = db.borrow().a_limit[SQLITE_LIMIT_LENGTH as usize];
    str_accum_init(&mut s_res, Some(db.clone()), mx_len);

    compute_jd(&mut x);
    compute_ymd_hms(&mut x);
    let mut i: usize = 0;
    let mut j: usize = 0;
    while i < z_fmt.len() {
        if z_fmt[i] != b'%' {
            i += 1;
            continue;
        }
        if j < i {
            str_append(&mut s_res, &z_fmt[j..i]);
        }
        i += 1;
        j = i + 1;
        let cf = at(z_fmt, i);
        match cf {
            b'd' | b'e' => {
                let f: &[u8] = if cf == b'd' { b"%02d" } else { b"%2d" };
                str_appendf(&mut s_res, f, &[PrintfArg::Int(x.d as i64)]);
            }
            b'f' => {
                // Segundos fracionários (não padrão).
                let mut s = x.s;
                if s > 59.999 {
                    s = 59.999;
                }
                str_appendf(&mut s_res, b"%06.3f", &[PrintfArg::Double(s)]);
            }
            b'F' => {
                str_appendf(
                    &mut s_res,
                    b"%04d-%02d-%02d",
                    &[PrintfArg::Int(x.y as i64), PrintfArg::Int(x.m as i64), PrintfArg::Int(x.d as i64)],
                );
            }
            b'G' | b'g' => {
                let mut y = x.clone();
                debug_assert!(y.valid_jd);
                // Move y para a quinta-feira da mesma semana de x.
                y.i_jd += ((3 - days_after_monday(&x)) * 86400000) as i64;
                y.valid_ymd = false;
                compute_ymd(&mut y);
                if cf == b'g' {
                    str_appendf(&mut s_res, b"%02d", &[PrintfArg::Int((y.y % 100) as i64)]);
                } else {
                    str_appendf(&mut s_res, b"%04d", &[PrintfArg::Int(y.y as i64)]);
                }
            }
            b'H' | b'k' => {
                let f: &[u8] = if cf == b'H' { b"%02d" } else { b"%2d" };
                str_appendf(&mut s_res, f, &[PrintfArg::Int(x.h as i64)]);
            }
            b'I' | b'l' => {
                let mut h = x.h;
                if h > 12 {
                    h -= 12;
                }
                if h == 0 {
                    h = 12;
                }
                let f: &[u8] = if cf == b'I' { b"%02d" } else { b"%2d" };
                str_appendf(&mut s_res, f, &[PrintfArg::Int(h as i64)]);
            }
            b'j' => {
                // Dia do ano. Jan01==1, Jan02==2 e assim por diante.
                str_appendf(&mut s_res, b"%03d", &[PrintfArg::Int((days_after_jan01(&x) + 1) as i64)]);
            }
            b'J' => {
                // Dia juliano (não padrão).
                str_appendf(&mut s_res, b"%.16g", &[PrintfArg::Double(x.i_jd as f64 / 86400000.0)]);
            }
            b'm' => {
                str_appendf(&mut s_res, b"%02d", &[PrintfArg::Int(x.m as i64)]);
            }
            b'M' => {
                str_appendf(&mut s_res, b"%02d", &[PrintfArg::Int(x.min as i64)]);
            }
            b'p' | b'P' => {
                if x.h >= 12 {
                    str_append(&mut s_res, if cf == b'p' { b"PM" } else { b"pm" });
                } else {
                    str_append(&mut s_res, if cf == b'p' { b"AM" } else { b"am" });
                }
            }
            b'R' => {
                str_appendf(
                    &mut s_res,
                    b"%02d:%02d",
                    &[PrintfArg::Int(x.h as i64), PrintfArg::Int(x.min as i64)],
                );
            }
            b's' => {
                if x.use_subsec {
                    str_appendf(
                        &mut s_res,
                        b"%.3f",
                        &[PrintfArg::Double((x.i_jd - 21086676_i64 * 10000000) as f64 / 1000.0)],
                    );
                } else {
                    let i_s: i64 = x.i_jd / 1000 - 21086676_i64 * 10000;
                    str_appendf(&mut s_res, b"%lld", &[PrintfArg::Int(i_s)]);
                }
            }
            b'S' => {
                str_appendf(&mut s_res, b"%02d", &[PrintfArg::Int((x.s as i32) as i64)]);
            }
            b'T' => {
                str_appendf(
                    &mut s_res,
                    b"%02d:%02d:%02d",
                    &[
                        PrintfArg::Int(x.h as i64),
                        PrintfArg::Int(x.min as i64),
                        PrintfArg::Int((x.s as i32) as i64),
                    ],
                );
            }
            b'u' | b'w' => {
                // u: dia da semana 1 a 7 (segunda==1, domingo==7).
                // w: dia da semana 0 a 6 (domingo==0, segunda==1).
                let mut c = (days_after_sunday(&x) as u8).wrapping_add(b'0');
                if c == b'0' && cf == b'u' {
                    c = b'7';
                }
                str_appendchar(&mut s_res, 1, c);
            }
            b'U' => {
                // Semana 00-53. O primeiro domingo do ano começa a semana 01.
                str_appendf(
                    &mut s_res,
                    b"%02d",
                    &[PrintfArg::Int(((days_after_jan01(&x) - days_after_sunday(&x) + 7) / 7) as i64)],
                );
            }
            b'V' => {
                // Semana 01-53. A primeira semana com uma quinta é a semana 01.
                let mut y = x.clone();
                // Ajusta y para a quinta-feira da mesma semana de x.
                debug_assert!(y.valid_jd);
                y.i_jd += ((3 - days_after_monday(&x)) * 86400000) as i64;
                y.valid_ymd = false;
                compute_ymd(&mut y);
                str_appendf(&mut s_res, b"%02d", &[PrintfArg::Int((days_after_jan01(&y) / 7 + 1) as i64)]);
            }
            b'W' => {
                // Semana 00-53. A primeira segunda do ano começa a semana 01.
                str_appendf(
                    &mut s_res,
                    b"%02d",
                    &[PrintfArg::Int(((days_after_jan01(&x) - days_after_monday(&x) + 7) / 7) as i64)],
                );
            }
            b'Y' => {
                str_appendf(&mut s_res, b"%04d", &[PrintfArg::Int(x.y as i64)]);
            }
            b'%' => {
                str_appendchar(&mut s_res, 1, b'%');
            }
            _ => {
                str_reset(&mut s_res);
                return;
            }
        }
        i += 1;
    }
    if j < i {
        str_append(&mut s_res, &z_fmt[j..i]);
    }
    result_str_accum(context, &mut s_res);
}


// ---- part_004.rs ----

/// current_time()
///
/// Esta função retorna o mesmo valor que time('now').
fn ctime_func(context: &mut Sqlite3Context, _argv: &[Sqlite3ValueRef]) {
    time_func(context, &[]);
}

/// current_date()
///
/// Esta função retorna o mesmo valor que date('now').
fn cdate_func(context: &mut Sqlite3Context, _argv: &[Sqlite3ValueRef]) {
    date_func(context, &[]);
}

/// timediff(DATE1, DATE2)
///
/// Retorna a quantidade de tempo que deve ser adicionada a DATE2 para convertê-la
/// em DATE1. O formato da diferença de tempo é:
///
///     +YYYY-MM-DD HH:MM:SS.SSS
///
/// O "+" inicial torna-se "-" se DATE1 ocorre antes de DATE2. Para valores de
/// data/hora A e B, a seguinte invariante deve ser mantida:
///
///     datetime(A) == datetime(B, timediff(A, B))
///
/// Ambos os argumentos DATE devem ser número de dia juliano ou string ISO-8601.
/// Os timestamps Unix não são suportados por esta rotina.
fn timediff_func(context: &mut Sqlite3Context, argv: &[Sqlite3ValueRef]) {
    let mut d1 = DateTime::default();
    let mut d2 = DateTime::default();

    if is_date(context, 1, &argv[0..1], &mut d1) != 0 {
        return;
    }
    if is_date(context, 1, &argv[1..2], &mut d2) != 0 {
        return;
    }

    compute_ymd_hms(&mut d1);
    compute_ymd_hms(&mut d2);

    let sign: u8;
    let mut y: i32;
    let mut m: i32;

    if d1.ijd >= d2.ijd {
        sign = b'+';
        y = d1.y - d2.y;
        if y != 0 {
            d2.y = d1.y;
            d2.valid_jd = 0;
            compute_jd(&mut d2);
        }
        m = d1.m - d2.m;
        if m < 0 {
            y -= 1;
            m += 12;
        }
        if m != 0 {
            d2.m = d1.m;
            d2.valid_jd = 0;
            compute_jd(&mut d2);
        }
        while d1.ijd < d2.ijd {
            m -= 1;
            if m < 0 {
                m = 11;
                y -= 1;
            }
            d2.m -= 1;
            if d2.m < 1 {
                d2.m = 12;
                d2.y -= 1;
            }
            d2.valid_jd = 0;
            compute_jd(&mut d2);
        }
        d1.ijd = d1.ijd.wrapping_sub(d2.ijd);
        d1.ijd = d1.ijd.wrapping_add((1486995408u64).wrapping_mul(100000) as i64);
    } else {
        sign = b'-';
        y = d2.y - d1.y;
        if y != 0 {
            d2.y = d1.y;
            d2.valid_jd = 0;
            compute_jd(&mut d2);
        }
        m = d2.m - d1.m;
        if m < 0 {
            y -= 1;
            m += 12;
        }
        if m != 0 {
            d2.m = d1.m;
            d2.valid_jd = 0;
            compute_jd(&mut d2);
        }
        while d1.ijd > d2.ijd {
            m -= 1;
            if m < 0 {
                m = 11;
                y -= 1;
            }
            d2.m += 1;
            if d2.m > 12 {
                d2.m = 1;
                d2.y += 1;
            }
            d2.valid_jd = 0;
            compute_jd(&mut d2);
        }
        d1.ijd = d2.ijd.wrapping_sub(d1.ijd);
        d1.ijd = d1.ijd.wrapping_add((1486995408u64).wrapping_mul(100000) as i64);
    }

    clear_ymd_hms_tz(&mut d1);
    compute_ymd_hms(&mut d1);

    let mut s_res = Sqlite3Str::new();
    api::str_appendf(
        &mut s_res,
        &format!(
            "{}{:04}-{:02}-{:02} {:02}:{:02}:{:06.3}",
            sign as char, y, m, d1.d - 1, d1.h, d1.m, d1.s
        ),
    );
    result_str_accum(context, s_res);
}

/// current_timestamp()
///
/// Esta função retorna o mesmo valor que datetime('now').
fn ctimestamp_func(context: &mut Sqlite3Context, _argv: &[Sqlite3ValueRef]) {
    datetime_func(context, &[]);
}

/// Esta função registra todas as funções acima como funções SQL.
/// Esta deve ser a única rotina neste arquivo com ligação externa.
/// (As variantes SQLITE_OMIT_DATETIME_FUNCS e SQLITE_DEBUG não existem no Debian 13.)
pub fn register_date_time_functions() {
    let a_date_time_funcs = vec![
        pure_date("julianday", -1, 0, 0, Rc::new(julian_day_func)),
        pure_date("unixepoch", -1, 0, 0, Rc::new(unix_epoch_func)),
        pure_date("date", -1, 0, 0, Rc::new(date_func)),
        pure_date("time", -1, 0, 0, Rc::new(time_func)),
        pure_date("datetime", -1, 0, 0, Rc::new(datetime_func)),
        pure_date("strftime", -1, 0, 0, Rc::new(strftime_func)),
        pure_date("timediff", 2, 0, 0, Rc::new(timediff_func)),
        dfunction("current_time", 0, 0, 0, Rc::new(ctime_func)),
        dfunction("current_timestamp", 0, 0, 0, Rc::new(ctimestamp_func)),
        dfunction("current_date", 0, 0, 0, Rc::new(cdate_func)),
    ];
    insert_builtin_funcs(&a_date_time_funcs);
}

