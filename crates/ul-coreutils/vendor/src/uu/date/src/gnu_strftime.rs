// Porte pseudo-linus: formatador de `+FORMAT` do `date` com o comportamento do `nstrftime` do GNU
// coreutils 9.7 (a base do `date` do Debian 13), no lugar do strtime do jiff e do pós-processador
// de modificadores do uutils, que aproximavam a glibc com heurísticas. As regras abaixo foram
// medidas no oráculo (flags `_ - 0 + ^ #`, largura, modificadores `E` e `O`, `%z` com dois pontos,
// `%N` com largura, `%F` com ano largo, diretivas inválidas copiadas como estão).

//! Formatação de datas no estilo do `nstrftime` do GNU.
//!
//! Sintaxe de uma diretiva: `%[flags][largura][E|O]<letra>`, com as flags `_` (espaço), `-`
//! (sem preenchimento), `0` (zeros), `+` (sinal para anos largos), `^` (maiúsculas) e `#`
//! (inverte a caixa onde o GNU inverte).

use jiff::Zoned;
use uucore::translate;

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const WEEKDAYS_ABBR: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTHS_ABBR: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Maior preenchimento aceito. O GNU escreve até estourar a memória; aqui vira erro de formato.
const MAX_WIDTH: i64 = 1 << 24;

/// O instante já decomposto no fuso de saída (o `struct tm` da glibc mais o que o `date` passa
/// ao `nstrftime`: nanossegundos e o epoch).
pub struct Tm {
    /// Ano completo (pode passar de 9999 nas datas estendidas, ou ser negativo).
    pub year: i64,
    /// Mês de 0 a 11.
    pub mon: i32,
    pub mday: i32,
    pub hour: i32,
    pub min: i32,
    pub sec: i32,
    /// Dia da semana, domingo é 0.
    pub wday: i32,
    /// Dia do ano a partir de 0.
    pub yday: i32,
    /// Deslocamento para o UTC, em segundos.
    pub gmtoff: i32,
    /// Sigla do fuso como o tzfile ou o TZ POSIX a dá (`%Z`).
    pub zone: String,
    /// Fração do segundo, de 0 a 999999999.
    pub nanos: i32,
    /// Segundos desde o epoch, arredondados para baixo (`%s`).
    pub epoch: i64,
}

impl Tm {
    /// Decompõe um instante com o fuso que ele carrega.
    pub fn from_zoned(z: &Zoned) -> Tm {
        let ts = z.timestamp();
        let mut epoch = ts.as_second();
        if ts.subsec_nanosecond() < 0 {
            epoch -= 1;
        }
        Tm {
            year: i64::from(z.year()),
            mon: i32::from(z.month()) - 1,
            mday: i32::from(z.day()),
            hour: i32::from(z.hour()),
            min: i32::from(z.minute()),
            sec: i32::from(z.second()),
            wday: i32::from(z.weekday().to_sunday_zero_offset()),
            yday: i32::from(z.day_of_year()) - 1,
            gmtoff: z.offset().seconds(),
            zone: z.time_zone().to_offset_info(ts).abbreviation().to_string(),
            nanos: z.subsec_nanosecond(),
            epoch,
        }
    }
}

/// Flags e largura de uma diretiva.
#[derive(Clone, Copy)]
struct Spec {
    /// `_`, `-`, `0` ou `+`; `'\0'` quando nenhuma foi dada.
    pad: char,
    /// Largura pedida, ou -1.
    width: i64,
    upper: bool,
    lower: bool,
    change_case: bool,
}

/// Formata `tm` segundo `fmt`.
pub fn format(fmt: &str, tm: &Tm) -> Result<String, String> {
    let chars: Vec<char> = fmt.chars().collect();
    let mut out = String::with_capacity(fmt.len() + 16);
    run(&mut out, &chars, tm, '\0')?;
    Ok(out)
}

/// Percorre um formato. `yr_spec` é a flag de ano herdada de quem chamou (o `%F` pede `+`).
fn run(out: &mut String, fmt: &[char], tm: &Tm, yr_spec: char) -> Result<(), String> {
    let mut i = 0;
    while i < fmt.len() {
        if fmt[i] == '%' {
            i = directive(out, fmt, i, tm, yr_spec)?;
        } else {
            out.push(fmt[i]);
            i += 1;
        }
    }
    Ok(())
}

fn width_error(width: i64) -> String {
    translate!(
        "date-error-format-modifier-width-too-large",
        "width" => width,
        "specifier" => ""
    )
}

/// Escreve `s` preenchido à esquerda até a largura pedida. O preenchimento é de zeros com as
/// flags `0` e `+`, nenhum com `-` e espaços nos demais casos.
fn add(out: &mut String, s: &str, sp: &Spec) -> Result<(), String> {
    let len = s.chars().count() as i64;
    let delta = sp.width - len;
    if delta > MAX_WIDTH {
        return Err(width_error(sp.width));
    }
    if delta > 0 && sp.pad != '-' {
        let fill = if sp.pad == '0' || sp.pad == '+' {
            '0'
        } else {
            ' '
        };
        out.extend(std::iter::repeat_n(fill, delta as usize));
    }
    out.push_str(s);
    Ok(())
}

/// Texto com a caixa pedida (`^` maiúsculas, `#` e `%P` minúsculas), depois `add`.
fn cpy(out: &mut String, s: &str, sp: &Spec) -> Result<(), String> {
    if sp.lower {
        add(out, &s.to_ascii_lowercase(), sp)
    } else if sp.upper {
        add(out, &s.to_ascii_uppercase(), sp)
    } else {
        add(out, s, sp)
    }
}

/// Diretiva inválida: o texto de `%` até `end` (inclusive) sai como está, com a largura pedida.
/// Devolve o índice onde o formato continua.
fn bad(
    out: &mut String,
    fmt: &[char],
    start: usize,
    end: usize,
    sp: &Spec,
) -> Result<usize, String> {
    let raw: String = fmt[start..=end].iter().collect();
    cpy(out, &raw, sp)?;
    Ok(end + 1)
}

/// Número com sinal e preenchimento. `colon_mask` marca onde entram os dois pontos do `%:z`
/// (bit k: antes do dígito k contado da direita). Sem largura pedida vale `digits`, que conta o
/// sinal. `-` não preenche, `_` põe espaços antes do sinal e os demais põem zeros depois dele.
fn emit_number(
    out: &mut String,
    sp: &Spec,
    digits: i64,
    negative: bool,
    value: u64,
    always_sign: bool,
    colon_mask: u32,
) -> Result<(), String> {
    let mut rev: Vec<char> = Vec::with_capacity(24);
    let mut u = value;
    let mut mask = colon_mask;
    loop {
        if mask & 1 != 0 {
            rev.push(':');
        }
        mask >>= 1;
        rev.push(char::from(b'0' + (u % 10) as u8));
        u /= 10;
        if u == 0 && mask == 0 {
            break;
        }
    }
    let pad = if sp.pad == '\0' { '0' } else { sp.pad };
    let width = if sp.width < 0 { digits } else { sp.width };
    let sign = if negative {
        Some('-')
    } else if always_sign {
        Some('+')
    } else {
        None
    };
    let shortage = width - i64::from(sign.is_some()) - rev.len() as i64;
    let padding = if pad == '-' || shortage <= 0 {
        0
    } else {
        shortage
    };
    if padding > MAX_WIDTH {
        return Err(width_error(width));
    }
    let fill = padding as usize;
    if let Some(sign) = sign {
        if pad == '_' {
            out.extend(std::iter::repeat_n(' ', fill));
            out.push(sign);
        } else {
            out.push(sign);
            out.extend(std::iter::repeat_n('0', fill));
        }
    } else if pad == '_' {
        out.extend(std::iter::repeat_n(' ', fill));
    } else {
        out.extend(std::iter::repeat_n('0', fill));
    }
    out.extend(rev.iter().rev());
    Ok(())
}

/// Número comum (o `DO_NUMBER` do GNU): zeros por padrão, largura `digits`.
fn plain(out: &mut String, sp: &Spec, digits: i64, v: i64) -> Result<(), String> {
    emit_number(out, sp, digits, v < 0, v.unsigned_abs(), false, 0)
}

/// Número que por padrão leva espaços (`%e`, `%k`, `%l`): vira `_` salvo `-` ou `0`.
fn spacepad(out: &mut String, sp: &mut Spec, digits: i64, v: i64) -> Result<(), String> {
    if sp.pad != '-' && sp.pad != '0' {
        sp.pad = '_';
    }
    plain(out, sp, digits, v)
}

/// Número de ano (`%Y`, `%C`, `%y`, `%G`, `%g`): com a flag `+` leva sinal quando passa da
/// largura padrão ou da largura pedida.
fn yearish(
    out: &mut String,
    sp: &mut Spec,
    yr_spec: char,
    digits: i64,
    negative: bool,
    value: u64,
) -> Result<(), String> {
    if sp.pad == '\0' {
        sp.pad = yr_spec;
    }
    let limit: u64 = if digits == 2 { 99 } else { 9999 };
    let always = sp.pad == '+' && (limit < value || digits < sp.width);
    emit_number(out, sp, digits, negative, value, always, 0)
}

/// Formato composto: expande `subfmt`, aplica `^` ao resultado e preenche com `add`.
fn sub(
    out: &mut String,
    sp: &Spec,
    subfmt: &str,
    tm: &Tm,
    yr_spec: char,
) -> Result<(), String> {
    let chars: Vec<char> = subfmt.chars().collect();
    let mut inner = String::new();
    run(&mut inner, &chars, tm, yr_spec)?;
    if sp.upper {
        inner.make_ascii_uppercase();
    }
    add(out, &inner, sp)
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Dias desde o início da semana ISO 1 (a que contém a primeira quinta-feira), pode ser negativo.
fn iso_week_days(yday: i64, wday: i64) -> i64 {
    // 378 é um múltiplo de 7 grande o bastante para o resto nunca ser negativo.
    yday - (yday - wday + 4 + 378) % 7 + 4 - 1
}

/// Ano ISO e dias desde o início da semana ISO 1 (`%G`, `%g`, `%V`).
fn iso_year_days(tm: &Tm) -> (i64, i64) {
    let mut year = tm.year;
    let yday = i64::from(tm.yday);
    let wday = i64::from(tm.wday);
    let mut days = iso_week_days(yday, wday);
    if days < 0 {
        // A semana pertence ao ano anterior.
        year -= 1;
        days = iso_week_days(yday + 365 + i64::from(is_leap(year)), wday);
    } else {
        let d = iso_week_days(yday - (365 + i64::from(is_leap(year))), wday);
        if d >= 0 {
            // A semana pertence ao ano seguinte.
            year += 1;
            days = d;
        }
    }
    (year, days)
}

/// Deslocamento do `%z` com `colons` dois pontos: dígitos padrão (contando o sinal), valor e
/// máscara dos dois pontos. `None` para mais de três.
fn tz_number(tm: &Tm, colons: usize) -> Option<(i64, u64, u32)> {
    let diff = i64::from(tm.gmtoff).abs();
    let (hh, mm, ss) = (diff / 3600, diff / 60 % 60, diff % 60);
    let hhmm = (hh * 100 + mm) as u64;
    let hhmmss = (hh * 10000 + mm * 100 + ss) as u64;
    match colons {
        0 => Some((5, hhmm, 0)),
        1 => Some((6, hhmm, 4)),
        2 => Some((9, hhmmss, 20)),
        3 => Some(if ss != 0 {
            (9, hhmmss, 20)
        } else if mm != 0 {
            (6, hhmm, 4)
        } else {
            (3, hh as u64, 0)
        }),
        _ => None,
    }
}

/// `%z` e variantes: sempre com sinal; o zero negativo de um TZ como `<-00>0` sai `-0000`.
fn emit_tz(out: &mut String, sp: &Spec, tm: &Tm, colons: usize) -> Result<bool, String> {
    let Some((digits, value, mask)) = tz_number(tm, colons) else {
        return Ok(false);
    };
    let negative = tm.gmtoff < 0 || (tm.gmtoff == 0 && tm.zone.starts_with('-'));
    emit_number(out, sp, digits, negative, value, true, mask)?;
    Ok(true)
}

/// `%N`: sem largura são os 9 dígitos. Com largura, corta ou completa com zeros à direita. A
/// flag `-` tira os zeros finais (sem largura, não), e `_` também, completando com espaços.
fn emit_nanos(out: &mut String, sp: &Spec, nanos: i32) -> Result<(), String> {
    fn strip(s: &mut String) {
        while s.len() > 1 && s.ends_with('0') {
            s.pop();
        }
    }
    let mut s = format!("{nanos:09}");
    if sp.width < 0 {
        if sp.pad == '_' {
            strip(&mut s);
            while s.len() < 9 {
                s.push(' ');
            }
        }
        out.push_str(&s);
        return Ok(());
    }
    if sp.width > MAX_WIDTH {
        return Err(width_error(sp.width));
    }
    let width = sp.width as usize;
    if width <= 9 {
        s.truncate(width);
    } else {
        while s.len() < width {
            s.push('0');
        }
    }
    match sp.pad {
        '_' => {
            strip(&mut s);
            while s.len() < width {
                s.push(' ');
            }
        }
        '-' => strip(&mut s),
        _ => {}
    }
    out.push_str(&s);
    Ok(())
}

/// Uma diretiva que começa em `start` (o `%`). Devolve o índice onde o formato continua.
fn directive(
    out: &mut String,
    fmt: &[char],
    start: usize,
    tm: &Tm,
    yr_spec: char,
) -> Result<usize, String> {
    let mut sp = Spec {
        pad: '\0',
        width: -1,
        upper: false,
        lower: false,
        change_case: false,
    };
    let mut f = start + 1;
    loop {
        match fmt.get(f) {
            Some(&c) if matches!(c, '_' | '-' | '+' | '0') => sp.pad = c,
            Some('^') => sp.upper = true,
            Some('#') => sp.change_case = true,
            _ => break,
        }
        f += 1;
    }
    if fmt.get(f).is_some_and(char::is_ascii_digit) {
        let mut w: i64 = 0;
        while let Some(d) = fmt.get(f).and_then(|c| c.to_digit(10)) {
            w = (w * 10 + i64::from(d)).min(i64::from(i32::MAX));
            f += 1;
        }
        sp.width = w;
    }
    let mut modifier = '\0';
    if let Some(&m) = fmt.get(f) {
        if m == 'E' || m == 'O' {
            modifier = m;
            f += 1;
        }
    }
    // `%` no fim do formato: o que veio depois do `%` sai como texto.
    let Some(&fc) = fmt.get(f) else {
        return bad(out, fmt, start, f - 1, &sp);
    };
    let year = tm.year;
    let hour12 = match tm.hour % 12 {
        0 => 12,
        h => h,
    };
    let e_bad = modifier == 'E';
    match fc {
        '%' => {
            // `%%` puro vale `%`. Com flag, largura ou modificador no meio, o segundo `%` não é
            // consumido: o que veio antes sai como texto e ele abre a próxima diretiva.
            if f != start + 1 {
                return bad(out, fmt, start, f - 1, &sp);
            }
            add(out, "%", &sp)?;
        }
        'a' | 'A' => {
            if modifier != '\0' {
                return bad(out, fmt, start, f, &sp);
            }
            if sp.change_case {
                sp.upper = true;
                sp.lower = false;
            }
            let i = tm.wday.rem_euclid(7) as usize;
            cpy(out, if fc == 'a' { WEEKDAYS_ABBR[i] } else { WEEKDAYS[i] }, &sp)?;
        }
        'b' | 'h' | 'B' => {
            if e_bad {
                return bad(out, fmt, start, f, &sp);
            }
            if sp.change_case {
                sp.upper = true;
                sp.lower = false;
            }
            let i = tm.mon.rem_euclid(12) as usize;
            cpy(out, if fc == 'B' { MONTHS[i] } else { MONTHS_ABBR[i] }, &sp)?;
        }
        'c' => {
            if modifier == 'O' {
                return bad(out, fmt, start, f, &sp);
            }
            sub(out, &sp, "%a %b %e %H:%M:%S %-Y", tm, yr_spec)?;
        }
        'C' => {
            yearish(out, &mut sp, yr_spec, 2, year < 0, year.unsigned_abs() / 100)?;
        }
        'd' | 'H' | 'I' | 'M' | 'm' | 'S' | 'j' | 'u' | 'U' | 'w' | 'W' => {
            if e_bad {
                return bad(out, fmt, start, f, &sp);
            }
            let (digits, v) = match fc {
                'd' => (2, tm.mday),
                'H' => (2, tm.hour),
                'I' => (2, hour12),
                'M' => (2, tm.min),
                'm' => (2, tm.mon + 1),
                'S' => (2, tm.sec),
                'j' => (3, tm.yday + 1),
                'u' => (1, (tm.wday + 6) % 7 + 1),
                'U' => (2, (tm.yday - tm.wday + 7) / 7),
                'w' => (1, tm.wday),
                _ => (2, (tm.yday - (tm.wday + 6) % 7 + 7) / 7),
            };
            plain(out, &sp, digits, i64::from(v))?;
        }
        'e' | 'k' | 'l' => {
            if e_bad {
                return bad(out, fmt, start, f, &sp);
            }
            let v = match fc {
                'e' => tm.mday,
                'k' => tm.hour,
                _ => hour12,
            };
            spacepad(out, &mut sp, 2, i64::from(v))?;
        }
        'D' | 'x' => {
            if modifier != '\0' && (fc == 'D' || modifier == 'O') {
                return bad(out, fmt, start, f, &sp);
            }
            sub(out, &sp, "%m/%d/%y", tm, yr_spec)?;
        }
        'X' => {
            if modifier == 'O' {
                return bad(out, fmt, start, f, &sp);
            }
            sub(out, &sp, "%H:%M:%S", tm, yr_spec)?;
        }
        'T' => sub(out, &sp, "%H:%M:%S", tm, yr_spec)?,
        'R' => sub(out, &sp, "%H:%M", tm, yr_spec)?,
        'r' => sub(out, &sp, "%I:%M:%S %p", tm, yr_spec)?,
        'F' => {
            if modifier != '\0' {
                return bad(out, fmt, start, f, &sp);
            }
            if sp.pad == '\0' && sp.width < 0 {
                // Sem flag nem largura: ano de quatro dígitos, com sinal quando passa de 9999.
                sub(out, &sp, "%Y-%m-%d", tm, '+')?;
            } else {
                // A largura pedida vale para o ano (o resto, `-MM-DD`, ocupa 6).
                let mut ysp = sp;
                ysp.width = if sp.width > 6 { sp.width - 6 } else { 0 };
                yearish(out, &mut ysp, '\0', 4, year < 0, year.unsigned_abs())?;
                out.push_str(&format!("-{:02}-{:02}", tm.mon + 1, tm.mday));
            }
        }
        'g' | 'G' | 'V' => {
            if e_bad {
                return bad(out, fmt, start, f, &sp);
            }
            let (iso_year, days) = iso_year_days(tm);
            match fc {
                'g' => yearish(out, &mut sp, yr_spec, 2, false, iso_year.unsigned_abs() % 100)?,
                'G' => yearish(out, &mut sp, yr_spec, 4, iso_year < 0, iso_year.unsigned_abs())?,
                _ => plain(out, &sp, 2, days / 7 + 1)?,
            }
        }
        'n' => add(out, "\n", &sp)?,
        't' => add(out, "\t", &sp)?,
        'N' => {
            if e_bad {
                return bad(out, fmt, start, f, &sp);
            }
            emit_nanos(out, &sp, tm.nanos)?;
        }
        'p' | 'P' => {
            if fc == 'P' {
                sp.lower = true;
            }
            if sp.change_case {
                sp.upper = false;
                sp.lower = true;
            }
            cpy(out, if tm.hour >= 12 { "PM" } else { "AM" }, &sp)?;
        }
        'q' => {
            if modifier == 'O' {
                return bad(out, fmt, start, f, &sp);
            }
            plain(out, &sp, 1, i64::from(tm.mon / 3 + 1))?;
        }
        's' => emit_number(out, &sp, 1, tm.epoch < 0, tm.epoch.unsigned_abs(), false, 0)?,
        'y' => yearish(out, &mut sp, yr_spec, 2, false, year.unsigned_abs() % 100)?,
        'Y' => {
            if modifier == 'O' {
                return bad(out, fmt, start, f, &sp);
            }
            yearish(out, &mut sp, yr_spec, 4, year < 0, year.unsigned_abs())?;
        }
        'z' => {
            emit_tz(out, &sp, tm, 0)?;
        }
        'Z' => {
            if sp.change_case {
                sp.upper = false;
                sp.lower = true;
            }
            cpy(out, &tm.zone, &sp)?;
        }
        ':' => {
            // `:`, `::` e `:::` só valem logo antes do `z`.
            let mut colons = 1;
            while fmt.get(f + colons) == Some(&':') {
                colons += 1;
            }
            if fmt.get(f + colons) != Some(&'z') {
                return bad(out, fmt, start, f, &sp);
            }
            f += colons;
            if !emit_tz(out, &sp, tm, colons)? {
                return bad(out, fmt, start, f, &sp);
            }
        }
        _ => return bad(out, fmt, start, f, &sp),
    }
    Ok(f + 1)
}
