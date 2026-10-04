//! Funções de tempo do gawk 5.2.1: `strftime`, `mktime` e a resolução do fuso (`TZ`).
//!
//! Contrato (não mude as assinaturas sem combinar com o integrador):
//!
//! - Nada aqui toca o host: o relógio vem do interpretador (via `sysabi`) e os arquivos de fuso são
//!   lidos pelo `read_zoneinfo` que o interpretador passa (ele lê pelo `sysabi`).
//! - [`TimeZone::resolve`] segue o glibc: `TZ` ausente usa `/etc/localtime` (sem ele, UTC com
//!   abreviação `UTC`); `TZ` vazio vira o nome `Universal` (no Debian 13, sem `tzdata-legacy`, é
//!   UTC com abreviação `Universal`); `:nome` ou `nome` procura `/usr/share/zoneinfo/nome` (ou o
//!   caminho absoluto); senão é string POSIX (`EST5EDT`, `<-03>3`); inválido vira UTC com a
//!   abreviação que o glibc extrair (`TZ=Foo/Bar` dá `Foo`). `TZ` com horário de verão e sem regra
//!   (`EST5EDT`, `ABC3DEF`) usa o `posixrules`, como o glibc.
//! - [`strftime`] tem todas as conversões do glibc 2.41 em `C.UTF-8`, inclusive flags `_`, `-`, `0`,
//!   `^`, `#`, largura e os modificadores `E`/`O`, e o limite de buffer do gawk (saída maior que
//!   1024 vezes o formato, arredondado para potência de dois, vira `""`).
//! - [`mktime`] recebe o texto `"AAAA MM DD HH MM SS [DST]"` do gawk e devolve -1 em erro.
//!
//! Itens públicos além do contrato original (todos opcionais para quem chama):
//!
//! - [`strftime_utc`]: o `strftime(fmt, t, 1)` do gawk. O glibc calcula `%s` passando a hora UTC
//!   pelo `mktime` do fuso local (com `TZ=America/Sao_Paulo`, `strftime("%s", 0, 1)` dá `10800`),
//!   então o modo UTC precisa conhecer o fuso local. [`strftime`] com [`TimeZone::utc`] continua
//!   valendo e só difere nesse `%s` (que sai como se o fuso local fosse UTC).
//! - [`time_from_number`]: a conversão que o gawk faz do número do awk para `time_t` (trunca em
//!   direção a zero; NaN, infinito e valores fora do `time_t` fazem o `strftime` devolver `""`).
//! - [`TimeZone::inherit_mktime_state`]: o `mktime` do glibc guarda, por processo, o deslocamento
//!   do último resultado e começa a próxima busca por ele; isso decide a hora repetida da volta do
//!   horário de verão. Cada [`TimeZone`] (e seus clones) carrega esse estado; quando o `TZ` muda e
//!   o interpretador resolve um fuso novo, chame este método para manter o estado do processo.
//!
//! O fuso também guarda, como o glibc, estado de regra POSIX entre chamadas. Por isso o
//! interpretador deve resolver o fuso uma vez por valor de `TZ` (na partida e a cada mudança de
//! `ENVIRON["TZ"]`), e não a cada chamada.
//!
//! Divergências conhecidas (nenhuma alcançável com o tzdata do Debian 13 pelo `read_zoneinfo`):
//!
//! - Registros de segundo bissexto em TZif (`right/...`, só no `tzdata-legacy`) são ignorados.
//! - `TZDIR` não é consultado: os nomes relativos sempre vão para `/usr/share/zoneinfo/`.
//! - Regra POSIX inválida com mês fora de 1 a 12 (`M0.1.0`, `M14.1.0`) faz o glibc ler fora da sua
//!   tabela de dias por mês. O que cai dentro das duas tabelas (ano comum e bissexto, lado a lado) e
//!   o zero logo antes delas são reproduzidos; leituras além disso (`M4464.1.0`, ou `M14` em ano
//!   bissexto) dependem da memória do glibc e usam um valor fixo que bate com o observado.
//! - Sem o arquivo no sandbox, o banco embutido do jiff responde pelo nome (só os nomes que o
//!   Debian 13 instala). Ele é do tzdata 2026c (o Debian 13 tem o 2026b), sem `backzone` (o Debian
//!   compila com ele, o que muda o horário local antes de 1970 em uns cem fusos que são links) e no
//!   formato "rearguard" (`isdst` invertido em `Europe/Dublin` e no Marrocos). Depois da última
//!   transição usa as regras do próprio jiff, que não reproduzem o estouro de `int` do glibc em
//!   anos acima de 5 milhões.

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

/// Fuso horário resolvido.
#[derive(Clone, Debug)]
pub struct TimeZone {
    zone: Arc<Zone>,
    /// Deslocamento (segundos a leste) do último resultado do `mktime`, compartilhado entre clones.
    mktime_guess: Arc<AtomicI64>,
}

impl TimeZone {
    /// UTC (o que o `strftime` usa com o terceiro argumento verdadeiro; `%Z` sai `GMT`).
    pub fn utc() -> TimeZone {
        TimeZone::from_zone(Zone::Gmt)
    }

    /// Resolve o fuso a partir do valor de `TZ` (`None` se a variável não existe).
    /// `read_zoneinfo` recebe um caminho absoluto e devolve o conteúdo do arquivo, se existir.
    pub fn resolve(tz: Option<&[u8]>, read_zoneinfo: &mut dyn FnMut(&[u8]) -> Option<Vec<u8>>) -> TimeZone {
        let mut loader = Loader { read: read_zoneinfo, posixrules: None };
        TimeZone::from_zone(loader.resolve(tz))
    }

    /// Passa a usar o estado do `mktime` de `previous` (o glibc tem um só por processo).
    pub fn inherit_mktime_state(&mut self, previous: &TimeZone) {
        self.mktime_guess = Arc::clone(&previous.mktime_guess);
    }

    fn from_zone(zone: Zone) -> TimeZone {
        TimeZone { zone: Arc::new(zone), mktime_guess: Arc::new(AtomicI64::new(0)) }
    }

    fn is_gmt(&self) -> bool {
        matches!(*self.zone, Zone::Gmt)
    }
}

/// `strftime(fmt, t)` no fuso `tz` (quem chama passa [`TimeZone::utc`] quando o gawk pede UTC).
pub fn strftime(fmt: &[u8], t: i64, tz: &TimeZone) -> Vec<u8> {
    format_time(fmt, t, tz, tz)
}

/// `strftime(fmt, t, 1)` do gawk: hora em UTC (`%Z` é `GMT`), com o `%s` calculado pelo `mktime`
/// do fuso `local`, como o glibc faz.
pub fn strftime_utc(fmt: &[u8], t: i64, local: &TimeZone) -> Vec<u8> {
    let gmt = TimeZone { zone: Arc::new(Zone::Gmt), mktime_guess: Arc::clone(&local.mktime_guess) };
    format_time(fmt, t, &gmt, local)
}

/// `mktime(spec)` do gawk no fuso `tz`; -1 se o texto não for válido.
///
/// Com [`TimeZone::utc`] é o `mktime(spec, 1)` do gawk (`timegm`, que ignora o campo DST).
pub fn mktime(spec: &[u8], tz: &TimeZone) -> i64 {
    let Some(mut tm) = parse_mktime_spec(spec) else {
        return -1;
    };
    if tz.is_gmt() {
        tm.isdst = 0;
    }
    mktime_tm(&tm, &tz.zone, &tz.mktime_guess).unwrap_or(-1)
}

/// Converte o número do awk no `time_t` que o gawk passa ao `strftime`: trunca em direção a zero.
/// `None` quando o gawk devolve a string vazia sem formatar (NaN, infinito, fora do `time_t`).
pub fn time_from_number(value: f64) -> Option<i64> {
    // 2^63 é exatamente representável em f64; o intervalo do time_t é [-2^63, 2^63).
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if !value.is_finite() || value < -LIMIT || value >= LIMIT {
        return None;
    }
    Some(value.trunc() as i64)
}

// ---------------------------------------------------------------------------------------------
// Calendário
// ---------------------------------------------------------------------------------------------

const SECS_PER_DAY: i64 = 86_400;

/// Dias desde 1970-01-01 do dia `d` do mês `m` (1 a 12) do ano `y` (gregoriano proléptico).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverso de [`days_from_civil`]: (ano, mês 1 a 12, dia).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Ano bissexto com a aritmética do `int` em C (o resto pode ser negativo, só o zero importa).
fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

/// Hora decomposta, como a `struct tm` que o glibc preenche.
#[derive(Clone, Debug)]
struct Tm {
    /// `tm_year`: ano menos 1900, sempre dentro do `int`.
    tm_year: i32,
    /// Mês de 0 a 11.
    mon: i64,
    mday: i64,
    hour: i64,
    min: i64,
    sec: i64,
    wday: i64,
    yday: i64,
    isdst: bool,
    gmtoff: i64,
    zone: Vec<u8>,
}

impl Tm {
    /// O `__offtime` do glibc: decompõe `t + offset`; falha se o ano não cabe no `int` da `tm`.
    fn decompose(t: i64, offset: i64) -> Option<Tm> {
        let total = i128::from(t) + i128::from(offset);
        let days = i64::try_from(total.div_euclid(i128::from(SECS_PER_DAY))).ok()?;
        let rem = i64::try_from(total.rem_euclid(i128::from(SECS_PER_DAY))).ok()?;
        let (y, m, d) = civil_from_days(days);
        let tm_year = i32::try_from(y - 1900).ok()?;
        Some(Tm {
            tm_year,
            mon: m - 1,
            mday: d,
            hour: rem / 3600,
            min: rem % 3600 / 60,
            sec: rem % 60,
            wday: (days + 4).rem_euclid(7),
            yday: days - days_from_civil(y, 1, 1),
            isdst: false,
            gmtoff: offset,
            zone: Vec::new(),
        })
    }

    /// `tm_year + 1900` com o estouro do `int` que o glibc tem ao imprimir o ano.
    fn wrapped_year(&self) -> i32 {
        self.tm_year.wrapping_add(1900)
    }
}

/// Ano UTC de `t` como o glibc o obtém para avaliar regras POSIX (`1900 + tm_year` em `int`).
fn utc_year(t: i64) -> Option<i32> {
    Tm::decompose(t, 0).map(|tm| tm.wrapped_year())
}

// ---------------------------------------------------------------------------------------------
// Modelo de fuso
// ---------------------------------------------------------------------------------------------

#[derive(Debug)]
enum Zone {
    /// O `gmtime`: deslocamento zero, abreviação `GMT`.
    Gmt,
    /// String POSIX do `TZ`.
    Rule(PosixTz),
    /// Arquivo TZif (do sistema, do banco embutido ou o `posixrules` remapeado).
    File(TzFile),
}

/// O que vale num instante: deslocamento, horário de verão e abreviação.
#[derive(Debug)]
struct Local<'a> {
    offset: i64,
    isdst: bool,
    abbr: Cow<'a, [u8]>,
}

impl Zone {
    fn lookup(&self, t: i64) -> Option<Local<'_>> {
        match self {
            Zone::Gmt => Some(Local { offset: 0, isdst: false, abbr: Cow::Borrowed(b"GMT") }),
            Zone::Rule(rule) => rule.lookup(t),
            Zone::File(file) => file.lookup(t),
        }
    }

    /// O `localtime`: `None` quando o glibc devolve NULL (ano fora do `int`).
    fn localtime(&self, t: i64) -> Option<Tm> {
        let local = self.lookup(t)?;
        let mut tm = Tm::decompose(t, local.offset)?;
        tm.isdst = local.isdst;
        tm.zone = local.abbr.into_owned();
        Some(tm)
    }

    /// Deslocamento e DST de `t`, só se a conversão completa funciona (como o `localtime`).
    fn convert(&self, t: i64) -> Option<(i64, bool)> {
        let local = self.lookup(t)?;
        Tm::decompose(t, local.offset)?;
        Some((local.offset, local.isdst))
    }

    /// Fuso que o glibc passa a usar, dentro da mesma chamada, depois de converter `t` (só muda
    /// com o rodapé sem regra descrito em [`Loader::footer_rule`]).
    fn after_convert(&self, t: i64) -> Option<&Zone> {
        let Zone::File(file) = self else {
            return None;
        };
        let last = *file.transitions.last()?;
        if t >= last { file.footer_switch.as_deref() } else { None }
    }
}

/// [`Zone::convert`] dentro de uma chamada do `mktime`, acompanhando a troca de fuso.
fn convert_in_call(current: &mut &Zone, t: i64) -> Option<(i64, bool)> {
    let result = current.convert(t);
    if let Some(next) = current.after_convert(t) {
        *current = next;
    }
    result
}

// ---------------------------------------------------------------------------------------------
// Strings POSIX de fuso (o `__tzset_parse_tz` do glibc, reproduzido pelo comportamento)
// ---------------------------------------------------------------------------------------------

/// Forma da data de uma regra. O estado inicial do glibc (tudo zerado) é `ZeroBased` com dia 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DateKind {
    /// `n`: dia do ano contado de zero, com 29 de fevereiro.
    #[default]
    ZeroBased,
    /// `Jn`: dia do ano de 1 a 365, sem 29 de fevereiro.
    Julian,
    /// `Mm.w.d`: dia `d` da semana `w` do mês `m`.
    Month,
}

/// Metade de uma regra POSIX: nome, deslocamento e quando ela começa a valer.
#[derive(Clone, Debug, Default)]
struct PosixRule {
    name: Vec<u8>,
    /// Segundos a leste de UTC.
    offset: i64,
    kind: DateKind,
    month: u16,
    week: u16,
    day: u16,
    /// Hora local da troca, em segundos.
    secs: i64,
}

/// Regras de um `TZ` POSIX: `[0]` é o horário padrão e `[1]` o de verão.
#[derive(Debug, Default)]
struct PosixTz {
    rules: [PosixRule; 2],
    /// Regra que o parse não gravou com sucesso: o glibc a deixa marcada como já calculada para
    /// o ano 0, com o instante de troca zerado, até a primeira conta em outro ano.
    stale: [AtomicBool; 2],
    /// O `TZ` do processo guarda esse estado entre chamadas; o rodapé de um TZif é relido (e
    /// volta ao estado inicial) a cada consulta.
    persistent: bool,
}

impl Clone for PosixTz {
    fn clone(&self) -> PosixTz {
        PosixTz {
            rules: self.rules.clone(),
            stale: [0, 1].map(|i| AtomicBool::new(self.stale[i].load(Ordering::Relaxed))),
            persistent: self.persistent,
        }
    }
}

/// Resultado do parse de uma string POSIX.
enum PosixParse {
    Done(PosixTz),
    /// Tem nome de horário de verão mas não tem regra: o glibc tenta o `posixrules`.
    NeedsDefaultRules(PosixTz),
}

fn is_c_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Um número como o `sscanf` lê (`%d`, `%ld`, `%hu`): brancos, sinal opcional e dígitos decimais.
struct Scanned {
    negative: bool,
    /// Magnitude; `None` se passou de `u64`.
    magnitude: Option<u64>,
    end: usize,
}

fn scan_number(s: &[u8], mut i: usize) -> Option<Scanned> {
    while i < s.len() && is_c_space(s[i]) {
        i += 1;
    }
    let mut negative = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        negative = s[i] == b'-';
        i += 1;
    }
    let start = i;
    let mut magnitude = Some(0u64);
    while i < s.len() && s[i].is_ascii_digit() {
        let digit = u64::from(s[i] - b'0');
        magnitude = magnitude.and_then(|m| m.checked_mul(10)).and_then(|m| m.checked_add(digit));
        i += 1;
    }
    if i == start {
        return None;
    }
    Some(Scanned { negative, magnitude, end: i })
}

impl Scanned {
    /// O valor do `strtol`, saturado em `LONG_MIN`/`LONG_MAX`.
    fn as_long(&self) -> i64 {
        match self.magnitude {
            Some(m) if !self.negative && m <= i64::MAX as u64 => m as i64,
            Some(m) if self.negative && m <= 1u64 << 63 => (m as i64).wrapping_neg(),
            _ if self.negative => i64::MIN,
            _ => i64::MAX,
        }
    }

    /// O valor do `strtoul` (saturado em `ULONG_MAX`, sinal negativo em aritmética sem sinal).
    fn as_ulong(&self) -> u64 {
        match self.magnitude {
            Some(m) if self.negative => m.wrapping_neg(),
            Some(m) => m,
            None => u64::MAX,
        }
    }
}

/// O `sscanf(s, "%hu%n:%hu%n:%hu%n", ...)` do glibc: quantos campos leu, os valores (os que não
/// foram lidos mantêm o valor inicial) e quantos bytes consumiu até o último campo lido.
fn scan_hms(s: &[u8], init: [u16; 3]) -> (usize, [u16; 3], usize) {
    let mut values = init;
    let mut consumed = 0;
    let mut pos = 0;
    let mut count = 0;
    for (k, value) in values.iter_mut().enumerate() {
        if k > 0 {
            if s.get(pos) != Some(&b':') {
                break;
            }
            pos += 1;
        }
        let Some(n) = scan_number(s, pos) else {
            break;
        };
        *value = n.as_ulong() as u16;
        pos = n.end;
        consumed = pos;
        count += 1;
    }
    (count, values, consumed)
}

/// Cursor sobre a string do `TZ`; o fim da string faz o papel do `'\0'` do C.
struct Cursor<'a> {
    s: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> u8 {
        self.s.get(self.pos).copied().unwrap_or(0)
    }

    fn rest(&self) -> &[u8] {
        &self.s[self.pos.min(self.s.len())..]
    }
}

impl PosixTz {
    fn utc_named(name: &[u8]) -> PosixTz {
        let mut tz = PosixTz::default();
        tz.rules[0].name = name.to_vec();
        tz.rules[1].name = name.to_vec();
        tz
    }

    /// Lê uma string POSIX. Partes inválidas deixam o estado que o glibc deixaria. `persistent`
    /// diz se é o `TZ` do processo (e não o rodapé de um TZif).
    fn parse(spec: &[u8], persistent: bool) -> PosixParse {
        let mut tz = PosixTz { persistent, ..PosixTz::default() };
        let mut parsed = [false, false];
        let mut cur = Cursor { s: spec, pos: 0 };
        if tz.parse_name(&mut cur, 0) && tz.parse_offset(&mut cur, 0) {
            if cur.peek() != 0 {
                if tz.parse_name(&mut cur, 1) {
                    tz.parse_offset(&mut cur, 1);
                    let rest = cur.rest();
                    if rest.is_empty() || rest == b"," {
                        tz.stale = [AtomicBool::new(true), AtomicBool::new(true)];
                        return PosixParse::NeedsDefaultRules(tz);
                    }
                }
                parsed[0] = tz.parse_rule(&mut cur, 0);
                if parsed[0] {
                    parsed[1] = tz.parse_rule(&mut cur, 1);
                }
            } else {
                tz.rules[1].name = tz.rules[0].name.clone();
                tz.rules[1].offset = tz.rules[0].offset;
            }
        }
        tz.stale = parsed.map(|ok| AtomicBool::new(!ok));
        PosixParse::Done(tz)
    }

    /// Completa um `TZ` com horário de verão sem regra com o padrão do glibc (`M3.2.0,M11.1.0`).
    fn with_default_rules(mut self) -> PosixTz {
        for (which, rule) in self.rules.iter_mut().enumerate() {
            rule.kind = DateKind::Month;
            (rule.month, rule.week) = if which == 0 { (3, 2) } else { (11, 1) };
            rule.day = 0;
            rule.secs = 2 * 3600;
        }
        self.stale = [AtomicBool::new(false), AtomicBool::new(false)];
        self
    }

    /// Nome: três ou mais letras, ou `<...>` com três ou mais letras, dígitos, `+` ou `-`.
    fn parse_name(&mut self, cur: &mut Cursor<'_>, which: usize) -> bool {
        let s = cur.rest();
        let alpha = s.iter().take_while(|c| c.is_ascii_alphabetic()).count();
        if alpha >= 3 {
            self.rules[which].name = s[..alpha].to_vec();
            cur.pos += alpha;
            return true;
        }
        if s.first() != Some(&b'<') {
            return false;
        }
        let inner = s[1..]
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric() || **c == b'+' || **c == b'-')
            .count();
        if s.get(1 + inner) != Some(&b'>') || inner < 3 {
            return false;
        }
        self.rules[which].name = s[1..1 + inner].to_vec();
        cur.pos += inner + 2;
        true
    }

    /// Deslocamento `[+-]hh[:mm[:ss]]` (horas até 24, minutos e segundos até 59). Sem número, o
    /// padrão falha e o de verão vira o padrão mais uma hora.
    fn parse_offset(&mut self, cur: &mut Cursor<'_>, which: usize) -> bool {
        let c = cur.peek();
        if which == 0 && (c == 0 || (c != b'+' && c != b'-' && !c.is_ascii_digit())) {
            return false;
        }
        let sign = if c == b'+' || c == b'-' {
            cur.pos += 1;
            if c == b'-' { 1 } else { -1 }
        } else {
            -1
        };
        let (count, [hh, mm, ss], consumed) = scan_hms(cur.rest(), [0, 0, 0]);
        if count > 0 {
            let secs = i64::from(ss.min(59)) + i64::from(mm.min(59)) * 60 + i64::from(hh.min(24)) * 3600;
            self.rules[which].offset = sign * secs;
        } else if which == 0 {
            self.rules[0].offset = 0;
            return false;
        } else {
            self.rules[1].offset = self.rules[0].offset + 3600;
        }
        cur.pos += consumed;
        true
    }

    /// Regra `,data[/hora]`. Em erro os campos já lidos ficam gravados, como no glibc.
    fn parse_rule(&mut self, cur: &mut Cursor<'_>, which: usize) -> bool {
        let rule = &mut self.rules[which];
        if cur.peek() == b',' {
            cur.pos += 1;
        }
        let c = cur.peek();
        if c == b'J' || c.is_ascii_digit() {
            let julian = c == b'J';
            rule.kind = if julian { DateKind::Julian } else { DateKind::ZeroBased };
            if julian {
                cur.pos += 1;
                if !cur.peek().is_ascii_digit() {
                    return false;
                }
            }
            let Some(n) = scan_number(cur.s, cur.pos) else {
                return false;
            };
            let d = n.as_ulong();
            if d > 365 || (julian && d == 0) {
                return false;
            }
            rule.day = d as u16;
            cur.pos = n.end;
        } else if c == b'M' {
            rule.kind = DateKind::Month;
            let s = cur.rest();
            let mut pos = 1;
            let mut count = 0;
            for k in 0..3 {
                if k > 0 {
                    if s.get(pos) != Some(&b'.') {
                        break;
                    }
                    pos += 1;
                }
                let Some(n) = scan_number(s, pos) else {
                    break;
                };
                let v = n.as_ulong() as u16;
                match k {
                    0 => rule.month = v,
                    1 => rule.week = v,
                    _ => rule.day = v,
                }
                pos = n.end;
                count += 1;
            }
            if count != 3
                || !(1..=12).contains(&rule.month)
                || !(1..=5).contains(&rule.week)
                || rule.day > 6
            {
                return false;
            }
            cur.pos += pos;
        } else if c == 0 {
            rule.kind = DateKind::Month;
            (rule.month, rule.week) = if which == 0 { (3, 2) } else { (11, 1) };
            rule.day = 0;
        } else {
            return false;
        }

        match cur.peek() {
            0 | b',' => rule.secs = 2 * 3600,
            b'/' => {
                cur.pos += 1;
                if cur.peek() == 0 {
                    return false;
                }
                let negative = cur.peek() == b'-';
                if negative {
                    cur.pos += 1;
                }
                let (_, [hh, mm, ss], consumed) = scan_hms(cur.rest(), [2, 0, 0]);
                cur.pos += consumed;
                let secs = i64::from(hh) * 3600 + i64::from(mm) * 60 + i64::from(ss);
                rule.secs = if negative { -secs } else { secs };
            }
            _ => return false,
        }
        true
    }

    /// O instante (UTC) em que a regra começa a valer no ano `year`, com as contas do glibc:
    /// antes de 1971 a base é sempre 1970-01-01 e a contagem de dias é feita em `int`.
    fn change(rule: &PosixRule, year: i32) -> i64 {
        let mut t: i64 = if year > 1970 {
            let y = year;
            let days = (y - 1970)
                .wrapping_mul(365)
                .wrapping_add((y - 1) / 4 - 1970 / 4)
                .wrapping_sub((y - 1) / 100 - 1970 / 100)
                .wrapping_add((y - 1) / 400 - 1970 / 400);
            i64::from(days) * SECS_PER_DAY
        } else {
            0
        };
        let leap = is_leap(i64::from(year));
        match rule.kind {
            DateKind::Julian => {
                t += (i64::from(rule.day) - 1) * SECS_PER_DAY;
                if rule.day >= 60 && leap {
                    t += SECS_PER_DAY;
                }
            }
            DateKind::ZeroBased => t += i64::from(rule.day) * SECS_PER_DAY,
            DateKind::Month => {
                let idx = i64::from(leap) * 13 + i64::from(rule.month);
                let days_before = cumulative_days(idx - 1);
                let month_end = cumulative_days(idx);
                t += days_before * SECS_PER_DAY;
                // Dia da semana do dia 1 pela congruência de Zeller, em aritmética de `int`.
                let m = i32::from(rule.month);
                let m1 = (m + 9) % 12 + 1;
                let yy0 = if m <= 2 { year.wrapping_sub(1) } else { year };
                let yy1 = yy0 / 100;
                let yy2 = yy0 % 100;
                let mut dow = ((26 * m1 - 2) / 10 + 1 + yy2 + yy2 / 4 + yy1 / 4 - 2 * yy1) % 7;
                if dow < 0 {
                    dow += 7;
                }
                let mut d = i64::from(rule.day) - i64::from(dow);
                if d < 0 {
                    d += 7;
                }
                for _ in 1..rule.week {
                    if d + 7 >= month_end - days_before {
                        break;
                    }
                    d += 7;
                }
                t += d * SECS_PER_DAY;
            }
        }
        t - rule.offset + rule.secs
    }

    /// Instante de troca da regra `which` no ano, respeitando o valor velho de regra não lida.
    fn cached_change(&self, which: usize, year: i32) -> i64 {
        if self.stale[which].load(Ordering::Relaxed) {
            if year == 0 {
                return 0;
            }
            if self.persistent {
                self.stale[which].store(false, Ordering::Relaxed);
            }
        }
        PosixTz::change(&self.rules[which], year)
    }

    fn lookup(&self, t: i64) -> Option<Local<'_>> {
        let year = utc_year(t)?;
        let c0 = self.cached_change(0, year);
        let c1 = self.cached_change(1, year);
        let isdst = if c0 > c1 { t < c1 || t >= c0 } else { t >= c0 && t < c1 };
        let rule = &self.rules[usize::from(isdst)];
        Some(Local { offset: rule.offset, isdst, abbr: Cow::Borrowed(&rule.name) })
    }
}

/// Dias acumulados antes de cada mês, ano comum e bissexto lado a lado (13 entradas cada). Meses
/// fora de 1 a 12 em regras inválidas fazem o glibc ler além da tabela: antes dela há zero e,
/// depois, o que observamos equivale a um valor grande.
fn cumulative_days(index: i64) -> i64 {
    const TABLE: [i64; 26] = [
        0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365, //
        0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335, 366,
    ];
    match usize::try_from(index) {
        Ok(i) if i < TABLE.len() => TABLE[i],
        Ok(_) => i64::from(u16::MAX),
        Err(_) => 0,
    }
}

// ---------------------------------------------------------------------------------------------
// Arquivos TZif
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
struct LocalType {
    offset: i64,
    isdst: bool,
    abbr: Vec<u8>,
}

impl LocalType {
    fn local(&self) -> Local<'_> {
        Local { offset: self.offset, isdst: self.isdst, abbr: Cow::Borrowed(&self.abbr) }
    }
}

/// O que vale depois da última transição explícita.
#[derive(Debug)]
enum Tail {
    /// Sem rodapé: continua o tipo da última transição.
    Last,
    /// Rodapé POSIX do TZif.
    Rule(PosixTz),
    /// Fuso do banco embutido do jiff (fallback sem o arquivo no sandbox).
    Bundled(jiff::tz::TimeZone),
}

#[derive(Debug)]
struct TzFile {
    transitions: Vec<i64>,
    trans_types: Vec<usize>,
    types: Vec<LocalType>,
    isstd: Vec<bool>,
    isut: Vec<bool>,
    tail: Tail,
    /// Fuso que o glibc carrega por efeito colateral ao ler um rodapé sem regra.
    footer_switch: Option<Box<Zone>>,
}

/// Conteúdo de um TZif antes de interpretar o rodapé.
struct RawTzif {
    file: TzFile,
    footer: Option<Vec<u8>>,
}

struct TzifHeader {
    version: u8,
    isutcnt: usize,
    isstdcnt: usize,
    leapcnt: usize,
    timecnt: usize,
    typecnt: usize,
    charcnt: usize,
}

impl TzifHeader {
    fn read(data: &[u8], off: usize) -> Option<TzifHeader> {
        let head = data.get(off..off.checked_add(44)?)?;
        if &head[..4] != b"TZif" {
            return None;
        }
        let count = |k: usize| -> Option<usize> {
            let b = &head[20 + 4 * k..24 + 4 * k];
            usize::try_from(i32::from_be_bytes([b[0], b[1], b[2], b[3]])).ok()
        };
        Some(TzifHeader {
            version: head[4],
            isutcnt: count(0)?,
            isstdcnt: count(1)?,
            leapcnt: count(2)?,
            timecnt: count(3)?,
            typecnt: count(4)?,
            charcnt: count(5)?,
        })
    }

    fn body_len(&self, tsize: usize) -> Option<usize> {
        let parts = [
            self.timecnt.checked_mul(tsize)?,
            self.timecnt,
            self.typecnt.checked_mul(6)?,
            self.charcnt,
            self.leapcnt.checked_mul(tsize + 4)?,
            self.isstdcnt,
            self.isutcnt,
        ];
        parts.iter().try_fold(0usize, |acc, n| acc.checked_add(*n))
    }
}

fn parse_tzif(data: &[u8]) -> Option<RawTzif> {
    let first = TzifHeader::read(data, 0)?;
    let (header, body_off, tsize) = if first.version >= b'2' {
        let second_off = 44usize.checked_add(first.body_len(4)?)?;
        (TzifHeader::read(data, second_off)?, second_off + 44, 8)
    } else {
        (first, 44, 4)
    };
    let h = &header;
    if h.typecnt == 0 || (h.isstdcnt != 0 && h.isstdcnt != h.typecnt) || (h.isutcnt != 0 && h.isutcnt != h.typecnt) {
        return None;
    }
    let body = data.get(body_off..body_off.checked_add(h.body_len(tsize)?)?)?;
    let mut p = 0;
    let mut transitions = Vec::with_capacity(h.timecnt);
    for _ in 0..h.timecnt {
        let b = &body[p..p + tsize];
        transitions.push(if tsize == 4 {
            i64::from(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        } else {
            i64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
        });
        p += tsize;
    }
    let mut trans_types = Vec::with_capacity(h.timecnt);
    for &idx in &body[p..p + h.timecnt] {
        if usize::from(idx) >= h.typecnt {
            return None;
        }
        trans_types.push(usize::from(idx));
    }
    p += h.timecnt;
    let raw_types = &body[p..p + 6 * h.typecnt];
    p += 6 * h.typecnt;
    let chars = &body[p..p + h.charcnt];
    p += h.charcnt + h.leapcnt * (tsize + 4);
    let isstd: Vec<bool> = body[p..p + h.isstdcnt].iter().map(|&b| b != 0).collect();
    p += h.isstdcnt;
    let isut: Vec<bool> = body[p..p + h.isutcnt].iter().map(|&b| b != 0).collect();

    let mut types = Vec::with_capacity(h.typecnt);
    for raw in raw_types.as_chunks::<6>().0 {
        let offset = i64::from(i32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]));
        let isdst = match raw[4] {
            0 => false,
            1 => true,
            _ => return None,
        };
        let start = usize::from(raw[5]);
        if start >= chars.len() {
            return None;
        }
        let abbr: Vec<u8> = chars[start..].iter().copied().take_while(|&c| c != 0).collect();
        types.push(LocalType { offset, isdst, abbr });
    }

    let footer = if tsize == 8 {
        let after = &data[body_off + body.len()..];
        match after.split_first() {
            Some((b'\n', rest)) => {
                let spec: Vec<u8> = rest.iter().copied().take_while(|&c| c != b'\n').collect();
                (!spec.is_empty()).then_some(spec)
            }
            _ => None,
        }
    } else {
        None
    };

    let file = TzFile { transitions, trans_types, types, isstd, isut, tail: Tail::Last, footer_switch: None };
    Some(RawTzif { file, footer })
}

impl TzFile {
    /// Antes da primeira transição o glibc usa o primeiro tipo sem horário de verão.
    fn initial_type(&self) -> &LocalType {
        self.types.iter().find(|ty| !ty.isdst).unwrap_or(&self.types[0])
    }

    fn last_type(&self) -> &LocalType {
        &self.types[self.trans_types[self.trans_types.len() - 1]]
    }

    fn lookup(&self, t: i64) -> Option<Local<'_>> {
        let first = self.transitions.first().copied();
        if first.is_none_or(|first| t < first) {
            return Some(self.initial_type().local());
        }
        let last = self.transitions[self.transitions.len() - 1];
        if t >= last {
            // O rodapé usa o ano UTC; se ele estoura, o glibc fica com o último tipo.
            return Some(match &self.tail {
                Tail::Last => self.last_type().local(),
                Tail::Rule(rule) => rule.lookup(t).unwrap_or_else(|| {
                    // Rodapé sem regra: o "último tipo" já é o do `posixrules` carregado por ele.
                    match self.footer_switch.as_deref() {
                        Some(Zone::File(switched)) => switched.last_type().local(),
                        _ => self.last_type().local(),
                    }
                }),
                Tail::Bundled(tz) => match utc_year(t) {
                    Some(_) => bundled_lookup(tz, t),
                    None => self.last_type().local(),
                },
            });
        }
        let i = self.transitions.partition_point(|&x| x <= t) - 1;
        Some(self.types[self.trans_types[i]].local())
    }

    /// O `posixrules` adaptado a um `TZ` com horário de verão e sem regra (o `__tzfile_default`
    /// do glibc, medido como caixa-preta): os tipos viram só o padrão e o de verão do usuário, e as
    /// transições em hora local cujo relógio anterior era padrão (ou marcadas como padrão) andam
    /// a diferença entre o deslocamento padrão do usuário e o do arquivo. As que vêm de horário de
    /// verão não andam, e as marcadas como UT ficam como estão. O rodapé continua o do arquivo.
    fn remap_for(&self, tz: &PosixTz) -> Option<TzFile> {
        if self.types.len() < 2 {
            return None;
        }
        let rule_std = if self.transitions.is_empty() {
            self.types[0].offset
        } else {
            self.trans_types
                .iter()
                .rev()
                .map(|&i| &self.types[i])
                .find(|ty| !ty.isdst)
                .map_or(0, |ty| ty.offset)
        };
        let std = &tz.rules[0];
        let dst = &tz.rules[1];
        let mut transitions = Vec::with_capacity(self.transitions.len());
        let mut trans_types = Vec::with_capacity(self.transitions.len());
        let mut prev_dst = false;
        for (&at, &idx) in self.transitions.iter().zip(&self.trans_types) {
            let ty = &self.types[idx];
            let isut = self.isut.get(idx).copied().unwrap_or(false);
            let isstd = self.isstd.get(idx).copied().unwrap_or(false);
            let shifted = if isut || (prev_dst && !isstd) { at } else { at.saturating_add(std.offset - rule_std) };
            transitions.push(shifted);
            trans_types.push(usize::from(ty.isdst));
            prev_dst = ty.isdst;
        }
        let tail = match &self.tail {
            Tail::Last => Tail::Last,
            Tail::Rule(rule) => Tail::Rule(rule.clone()),
            Tail::Bundled(tz) => Tail::Bundled(tz.clone()),
        };
        Some(TzFile {
            transitions,
            trans_types,
            types: vec![
                LocalType { offset: std.offset, isdst: false, abbr: std.name.clone() },
                LocalType { offset: dst.offset, isdst: true, abbr: dst.name.clone() },
            ],
            isstd: Vec::new(),
            isut: Vec::new(),
            tail,
            footer_switch: None,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Banco embutido do jiff (só quando o arquivo não vem pelo `read_zoneinfo`)
// ---------------------------------------------------------------------------------------------

/// Nomes que o jiff conhece mas que, no Debian 13, só vêm no pacote `tzdata-legacy` (ausente).
const LEGACY_NAMES: &[&str] = &[
    "Africa/Asmera", "America/Argentina/ComodRivadavia", "America/Buenos_Aires", "America/Catamarca",
    "America/Cordoba", "America/Fort_Wayne", "America/Godthab", "America/Indianapolis", "America/Jujuy",
    "America/Knox_IN", "America/Louisville", "America/Mendoza", "America/Rosario", "Antarctica/South_Pole",
    "Asia/Ashkhabad", "Asia/Calcutta", "Asia/Choibalsan", "Asia/Chungking", "Asia/Dacca", "Asia/Katmandu",
    "Asia/Macao", "Asia/Rangoon", "Asia/Saigon", "Asia/Thimbu", "Asia/Ujung_Pandang", "Asia/Ulan_Bator",
    "Atlantic/Faeroe", "Australia/ACT", "Australia/LHI", "Australia/NSW", "Australia/North",
    "Australia/Queensland", "Australia/South", "Australia/Tasmania", "Australia/Victoria", "Australia/West",
    "Brazil/Acre", "Brazil/DeNoronha", "Brazil/East", "Brazil/West", "CET", "CST6CDT", "Canada/Atlantic",
    "Canada/Central", "Canada/Eastern", "Canada/Mountain", "Canada/Newfoundland", "Canada/Pacific",
    "Canada/Saskatchewan", "Canada/Yukon", "Chile/Continental", "Chile/EasterIsland", "Cuba", "EET", "EST",
    "EST5EDT", "Egypt", "Eire", "Europe/Kiev", "Europe/Uzhgorod", "Europe/Zaporozhye", "GB", "GB-Eire",
    "GMT+0", "GMT-0", "GMT0", "Greenwich", "HST", "Hongkong", "Iceland", "Iran", "Israel", "Jamaica", "Japan",
    "Kwajalein", "Libya", "MET", "MST", "MST7MDT", "Mexico/BajaNorte", "Mexico/BajaSur", "Mexico/General",
    "NZ", "NZ-CHAT", "Navajo", "PRC", "PST8PDT", "Pacific/Enderbury", "Pacific/Ponape", "Pacific/Truk",
    "Poland", "Portugal", "ROC", "ROK", "Singapore", "Turkey", "UCT", "US/Alaska", "US/Aleutian", "US/Arizona",
    "US/Central", "US/East-Indiana", "US/Eastern", "US/Hawaii", "US/Indiana-Starke", "US/Michigan",
    "US/Mountain", "US/Pacific", "US/Samoa", "Universal", "W-SU", "WET", "Zulu",
];

/// Período do calendário gregoriano (400 anos), em segundos.
const GREGORIAN_CYCLE: i64 = 146_097 * SECS_PER_DAY;

fn bundled_lookup(tz: &jiff::tz::TimeZone, t: i64) -> Local<'static> {
    let min = jiff::Timestamp::MIN.as_second();
    let max = jiff::Timestamp::MAX.as_second();
    // Depois da última transição as regras se repetem a cada 400 anos.
    let t = if t > max {
        let cycles = (t - max).unsigned_abs().div_ceil(GREGORIAN_CYCLE.unsigned_abs());
        t - cycles as i64 * GREGORIAN_CYCLE
    } else {
        t.max(min)
    };
    let ts = jiff::Timestamp::from_second(t).unwrap_or(jiff::Timestamp::MIN);
    let info = tz.to_offset_info(ts);
    Local {
        offset: i64::from(info.offset().seconds()),
        isdst: info.dst().is_dst(),
        abbr: Cow::Owned(info.abbreviation().as_bytes().to_vec()),
    }
}

/// Monta um TZif a partir do banco embutido, com transições explícitas até 2038 como os arquivos
/// "fat" do Debian (o que importa para o `posixrules`).
fn bundled_zone(name: &str) -> Option<TzFile> {
    // `Etc/Unknown` é invenção do jiff, não existe no tzdata.
    if LEGACY_NAMES.contains(&name) || name == "Etc/Unknown" {
        return None;
    }
    let db = jiff::tz::TimeZoneDatabase::bundled();
    let tz = db.get(name).ok()?;
    if tz.iana_name() != Some(name) {
        // O jiff ignora maiúsculas; o sistema de arquivos não.
        return None;
    }
    let initial = bundled_lookup(&tz, jiff::Timestamp::MIN.as_second());
    let mut types = vec![LocalType { offset: initial.offset, isdst: initial.isdst, abbr: initial.abbr.into_owned() }];
    let mut transitions = Vec::new();
    let mut trans_types = Vec::new();
    for tr in tz.following(jiff::Timestamp::MIN) {
        let at = tr.timestamp().as_second();
        if at >= 1i64 << 31 {
            break;
        }
        let ty = LocalType {
            offset: i64::from(tr.offset().seconds()),
            isdst: tr.dst().is_dst(),
            abbr: tr.abbreviation().as_bytes().to_vec(),
        };
        let idx = match types.iter().position(|known| *known == ty) {
            Some(i) => i,
            None => {
                types.push(ty);
                types.len() - 1
            }
        };
        transitions.push(at);
        trans_types.push(idx);
    }
    Some(TzFile {
        transitions,
        trans_types,
        types,
        isstd: Vec::new(),
        isut: Vec::new(),
        tail: Tail::Bundled(tz),
        footer_switch: None,
    })
}

/// Normaliza o caminho que o glibc abriria e, se ele cai em `/usr/share/zoneinfo/`, devolve o nome
/// do fuso. Cada componente intermediário tem de ser um diretório que existe nessa árvore.
fn zoneinfo_name(path: &[u8]) -> Option<String> {
    let path = std::str::from_utf8(path).ok()?;
    if !path.starts_with('/') || path.ends_with('/') {
        return None;
    }
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    let mut stack: Vec<&str> = Vec::new();
    for (k, part) in parts.iter().enumerate() {
        if *part == ".." {
            stack.pop();
            continue;
        }
        stack.push(part);
        if k + 1 < parts.len() && !is_known_dir(&stack) {
            return None;
        }
    }
    let rest = stack.strip_prefix(&["usr", "share", "zoneinfo"][..])?;
    if rest.is_empty() {
        return None;
    }
    Some(rest.join("/"))
}

fn is_known_dir(stack: &[&str]) -> bool {
    const BASE: [&str; 3] = ["usr", "share", "zoneinfo"];
    if stack.len() <= BASE.len() {
        return stack == &BASE[..stack.len()];
    }
    if stack[..3] != BASE {
        return false;
    }
    let prefix = stack[3..].join("/") + "/";
    jiff::tz::TimeZoneDatabase::bundled()
        .available()
        .any(|name| name.as_str().starts_with(&prefix) && !LEGACY_NAMES.contains(&name.as_str()))
}

// ---------------------------------------------------------------------------------------------
// Resolução do `TZ`
// ---------------------------------------------------------------------------------------------

const ZONEINFO_DIR: &[u8] = b"/usr/share/zoneinfo/";
const TZDEFAULT: &[u8] = b"/etc/localtime";
const POSIXRULES_PATH: &[u8] = b"/usr/share/zoneinfo/posixrules";
/// No Debian, `posixrules` é um link para este fuso.
const POSIXRULES_BUNDLED: &str = "America/New_York";

struct Loader<'a> {
    read: &'a mut dyn FnMut(&[u8]) -> Option<Vec<u8>>,
    /// `posixrules` já carregado (`Some(None)` se não existe ou é inválido).
    posixrules: Option<Option<Arc<TzFile>>>,
}

impl Loader<'_> {
    fn resolve(&mut self, tz: Option<&[u8]>) -> Zone {
        let name: &[u8] = match tz.map(until_nul) {
            None => TZDEFAULT,
            Some(b"") => b"Universal",
            Some(v) => v.strip_prefix(b":").unwrap_or(v),
        };
        if let Some(file) = self.load_file(name) {
            return Zone::File(file);
        }
        if name.is_empty() || name == TZDEFAULT {
            return Zone::Rule(PosixTz::utc_named(b"UTC"));
        }
        match PosixTz::parse(name, true) {
            PosixParse::Done(tz) => Zone::Rule(tz),
            PosixParse::NeedsDefaultRules(tz) => match self.posixrules() {
                Some(rules) => match rules.remap_for(&tz) {
                    Some(file) => Zone::File(file),
                    None => Zone::Rule(tz.with_default_rules()),
                },
                None => Zone::Rule(tz.with_default_rules()),
            },
        }
    }

    /// Lê e interpreta o arquivo que o glibc abriria para `name`.
    fn load_file(&mut self, name: &[u8]) -> Option<TzFile> {
        if name.is_empty() {
            return None;
        }
        let path = if name.starts_with(b"/") { name.to_vec() } else { [ZONEINFO_DIR, name].concat() };
        self.load_path(&path)
    }

    fn load_path(&mut self, path: &[u8]) -> Option<TzFile> {
        match (self.read)(path) {
            Some(data) => {
                let raw = parse_tzif(&data)?;
                let mut file = raw.file;
                if let Some(spec) = raw.footer {
                    let (rule, switch) = self.footer_rule(&spec);
                    file.tail = Tail::Rule(rule);
                    file.footer_switch = switch;
                }
                Some(file)
            }
            None => {
                let name = zoneinfo_name(path)?;
                let name = if name == "posixrules" { POSIXRULES_BUNDLED.to_string() } else { name };
                bundled_zone(&name)
            }
        }
    }

    /// Rodapé do TZif. Sem regra, o glibc só usa o padrão `M3.2.0,M11.1.0` se o `posixrules` não
    /// carrega. Se carrega, as regras ficam zeradas (horário de verão quase o ano todo) e, de
    /// quebra, o estado global do glibc passa a ser o `posixrules` adaptado aos nomes do rodapé até
    /// o próximo `tzset`, o que só se nota dentro de uma mesma chamada do `mktime` (o segundo valor).
    fn footer_rule(&mut self, spec: &[u8]) -> (PosixTz, Option<Box<Zone>>) {
        match PosixTz::parse(spec, false) {
            PosixParse::Done(tz) => (tz, None),
            PosixParse::NeedsDefaultRules(tz) => match self.posixrules().and_then(|f| f.remap_for(&tz)) {
                Some(remapped) => (tz, Some(Box::new(Zone::File(remapped)))),
                None => (tz.with_default_rules(), None),
            },
        }
    }

    fn posixrules(&mut self) -> Option<Arc<TzFile>> {
        if self.posixrules.is_none() {
            // Evita recursão: o rodapé do próprio posixrules não pode depender dele.
            self.posixrules = Some(None);
            let loaded = self.load_path(POSIXRULES_PATH).map(Arc::new);
            self.posixrules = Some(loaded);
        }
        self.posixrules.clone().flatten()
    }
}

fn until_nul(s: &[u8]) -> &[u8] {
    s.iter().position(|&c| c == 0).map_or(s, |n| &s[..n])
}

// ---------------------------------------------------------------------------------------------
// mktime
// ---------------------------------------------------------------------------------------------

/// Campos de entrada do `mktime`, com os tipos da `struct tm` do C.
#[derive(Clone, Copy, Debug)]
struct TmFields {
    tm_year: i32,
    mon: i32,
    mday: i32,
    hour: i32,
    min: i32,
    sec: i32,
    isdst: i32,
}

/// O `sscanf(spec, "%ld %d %d %d %d %d %d")` do gawk e as checagens que ele faz antes do `mktime`.
fn parse_mktime_spec(spec: &[u8]) -> Option<TmFields> {
    let s = until_nul(spec);
    let mut values = [0i64; 7];
    let mut count = 0;
    let mut pos = 0;
    for (k, value) in values.iter_mut().enumerate() {
        let Some(n) = scan_number(s, pos) else {
            break;
        };
        // `%ld` para o ano; `%d` guarda o `long` lido num `int` (trunca).
        *value = if k == 0 { n.as_long() } else { i64::from(n.as_long() as i32) };
        pos = n.end;
        count += 1;
    }
    if count < 6 {
        return None;
    }
    let year = values[0];
    let month = values[1] as i32;
    if month == i32::MIN || year < i64::from(i32::MIN) + 1900 || year - 1900 > i64::from(i32::MAX) {
        return None;
    }
    Some(TmFields {
        tm_year: (year - 1900) as i32,
        mon: month - 1,
        mday: values[2] as i32,
        hour: values[3] as i32,
        min: values[4] as i32,
        sec: values[5] as i32,
        isdst: if count == 7 { values[6] as i32 } else { -1 },
    })
}

/// Converte `t` como o `mktime` do glibc: se o instante não cabe na `struct tm`, troca `t` pelo
/// instante convertível mais próximo dele, achado por bisseção a partir de zero.
fn ranged_convert(zone: &mut &Zone, t: &mut i64) -> Option<(i64, bool)> {
    if let Some(result) = convert_in_call(zone, *t) {
        return Some(result);
    }
    let (mut ok, mut bad) = (0i64, *t);
    let mut found = None;
    loop {
        let mid = (ok >> 1) + (bad >> 1) + ((ok | bad) & 1);
        if mid == ok || mid == bad {
            break;
        }
        match convert_in_call(zone, mid) {
            Some(result) => {
                ok = mid;
                found = Some(result);
            }
            None => bad = mid,
        }
    }
    let result = found.filter(|_| ok != 0)?;
    *t = ok;
    Some(result)
}

/// Distância entre as sondas que procuram o `tm_isdst` pedido e o alcance da busca (medidos no
/// glibc 2.41 com fusos artesanais).
const ISDST_PROBE_STRIDE: i64 = 601_200;
const ISDST_PROBE_BOUND: i64 = 457_243_200 / 2 + 601_200;

/// O `mktime` do glibc reproduzido pelo comportamento: normaliza os campos, acha o instante por
/// iteração de ponto fixo a partir do deslocamento do resultado anterior e trata horas que caem
/// em buracos (oscilação) ou com `tm_isdst` diferente do pedido.
fn mktime_tm(tm: &TmFields, zone: &Zone, guess: &AtomicI64) -> Option<i64> {
    // Os segundos fora de 0..=59 entram só no fim, depois de achar o deslocamento.
    let sec_requested = i64::from(tm.sec);
    let sec = sec_requested.clamp(0, 59);
    let mon = i64::from(tm.mon);
    let year = 1900 + i64::from(tm.tm_year) + mon.div_euclid(12);
    let days = days_from_civil(year, mon.rem_euclid(12) + 1, 1) + i64::from(tm.mday) - 1;
    let local = days * SECS_PER_DAY + i64::from(tm.hour) * 3600 + i64::from(tm.min) * 60 + sec;

    let mut zone = zone;
    let mut t = local - guess.load(Ordering::Relaxed);
    let (mut older, mut previous) = (t, t);
    let mut previous_dst = false;
    let mut probes_left = 6;
    let mut oscillated = false;
    let (mut offset, mut isdst);
    loop {
        (offset, isdst) = ranged_convert(&mut zone, &mut t)?;
        let next = local - offset;
        if next == t {
            break;
        }
        if t == older && t != previous {
            // Buraco (horário que não existe): entre os dois candidatos, fica o que tem `tm_isdst`
            // diferente do pedido; sem pedido, o de verão, se só um deles for.
            let stop = if tm.isdst < 0 { isdst || !previous_dst } else { isdst != (tm.isdst != 0) };
            if stop {
                oscillated = true;
                break;
            }
        }
        probes_left -= 1;
        if probes_left == 0 {
            return None;
        }
        older = previous;
        previous = t;
        previous_dst = isdst;
        t = next;
    }

    if !oscillated && tm.isdst >= 0 && (tm.isdst != 0) != isdst {
        let want = tm.isdst != 0;
        let mut found = None;
        let mut delta = ISDST_PROBE_STRIDE;
        'search: while delta < ISDST_PROBE_BOUND {
            for direction in [-1, 1] {
                let Some(mut probe) = t.checked_add(delta * direction) else {
                    continue;
                };
                let (probe_offset, probe_dst) = ranged_convert(&mut zone, &mut probe)?;
                if probe_dst == want {
                    let candidate = local - probe_offset;
                    if let Some(result) = convert_in_call(&mut zone, candidate) {
                        found = Some((candidate, result.0));
                        break 'search;
                    }
                }
            }
            delta += ISDST_PROBE_STRIDE;
        }
        (t, offset) = match found {
            Some(found) => found,
            None => {
                // Nenhuma sonda com o `tm_isdst` pedido: supõe uma hora de diferença.
                let dst_difference = i64::from(tm.isdst == 0) - i64::from(!isdst);
                let candidate = t + 3600 * dst_difference;
                let (candidate_offset, _) = convert_in_call(&mut zone, candidate)?;
                (candidate, candidate_offset)
            }
        };
    }

    guess.store(local - t, Ordering::Relaxed);
    // Os segundos pedidos voltam no fim (o glibc só refaz a conversão se eles diferem dos da
    // hora achada).
    if sec_requested != (t + offset).rem_euclid(60) {
        let t = t.checked_add(sec_requested - sec)?;
        convert_in_call(&mut zone, t)?;
        return Some(t);
    }
    Some(t)
}

// ---------------------------------------------------------------------------------------------
// strftime
// ---------------------------------------------------------------------------------------------

const WEEKDAYS: [&[u8]; 7] = [b"Sunday", b"Monday", b"Tuesday", b"Wednesday", b"Thursday", b"Friday", b"Saturday"];
const MONTHS: [&[u8]; 12] = [
    b"January", b"February", b"March", b"April", b"May", b"June", b"July", b"August", b"September", b"October",
    b"November", b"December",
];

fn format_time(fmt: &[u8], t: i64, display: &TimeZone, local: &TimeZone) -> Vec<u8> {
    // O gawk dobra o buffer até 1024 vezes o tamanho do formato; se não couber, devolve "".
    if fmt.is_empty() {
        return Vec::new();
    }
    let Some(budget) = fmt.len().checked_mul(1024).and_then(usize::checked_next_power_of_two) else {
        return Vec::new();
    };
    let Some(tm) = display.zone.localtime(t) else {
        return Vec::new();
    };
    let mut seconds = || -> i64 {
        let fields = TmFields {
            tm_year: tm.tm_year,
            mon: tm.mon as i32,
            mday: tm.mday as i32,
            hour: tm.hour as i32,
            min: tm.min as i32,
            sec: tm.sec as i32,
            isdst: i32::from(tm.isdst),
        };
        mktime_tm(&fields, &local.zone, &local.mktime_guess).unwrap_or(-1)
    };
    let mut out = Output { buf: Vec::new(), max: budget - 1, overflow: false };
    format_into(&mut out, until_nul(fmt), &tm, &mut seconds);
    if out.overflow { Vec::new() } else { out.buf }
}

struct Output {
    buf: Vec<u8>,
    max: usize,
    overflow: bool,
}

impl Output {
    fn room(&mut self, n: usize) -> bool {
        if self.overflow || self.buf.len().saturating_add(n) > self.max {
            self.overflow = true;
            return false;
        }
        true
    }

    fn push(&mut self, bytes: &[u8]) {
        if self.room(bytes.len()) {
            self.buf.extend_from_slice(bytes);
        }
    }

    fn fill(&mut self, c: u8, n: usize) {
        if self.room(n) {
            self.buf.resize(self.buf.len() + n, c);
        }
    }
}

/// Preenchimento pedido pelas flags `_` (espaço), `-` (nenhum) e `0` (zeros).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pad {
    Default,
    Space,
    None,
    Zero,
}

#[derive(Clone, Copy, Debug)]
struct Spec {
    pad: Pad,
    width: Option<usize>,
    upcase: bool,
    swap_case: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    Keep,
    Upper,
    Lower,
}

/// Texto com largura: completa à esquerda com zeros se a flag for `0`, senão com espaços.
fn put_text(out: &mut Output, text: &[u8], spec: &Spec, case: Case) {
    if let Some(width) = spec.width
        && width > text.len()
    {
        out.fill(if spec.pad == Pad::Zero { b'0' } else { b' ' }, width - text.len());
    }
    match case {
        Case::Keep => out.push(text),
        Case::Upper => out.push(&text.to_ascii_uppercase()),
        Case::Lower => out.push(&text.to_ascii_lowercase()),
    }
}

/// Número com `digits` dígitos mínimos. Com zeros (o padrão), a largura vira o mínimo de dígitos,
/// depois do sinal; com `_`, espaços até `digits` e depois até a largura; com `-`, só a largura.
fn put_number(out: &mut Output, value: i64, digits: usize, spec: &Spec, default_pad: Pad) {
    let pad = if spec.pad == Pad::Default { default_pad } else { spec.pad };
    let sign: &[u8] = if value < 0 { b"-" } else { b"" };
    let magnitude = value.unsigned_abs().to_string();
    let len = sign.len() + magnitude.len();
    let mut text = Vec::with_capacity(len.max(digits));
    match pad {
        Pad::Space => {
            text.resize(digits.saturating_sub(len), b' ');
            text.extend_from_slice(sign);
            text.extend_from_slice(magnitude.as_bytes());
        }
        Pad::None => {
            text.extend_from_slice(sign);
            text.extend_from_slice(magnitude.as_bytes());
        }
        Pad::Zero | Pad::Default => {
            let total = digits.max(spec.width.unwrap_or(0));
            if !out.room(total.max(len)) {
                return;
            }
            text.extend_from_slice(sign);
            text.resize(sign.len() + total.saturating_sub(len), b'0');
            text.extend_from_slice(magnitude.as_bytes());
            out.push(&text);
            return;
        }
    }
    put_text(out, &text, &Spec { pad, ..*spec }, Case::Keep);
}

/// Conversões aceitas com cada modificador (`E`, `O`) no glibc 2.41, medidas uma a uma.
fn conversion_allowed(conv: u8, modifier: u8) -> bool {
    let plain = b"ABCDFGHIMPRSTUVWXYZabcdeghjklmnprstuwxyz%".contains(&conv);
    match modifier {
        0 => plain,
        b'E' => b"CPRTXYZcnprstuxyz%".contains(&conv),
        _ => b"BCGHIMPRSTUVWZbdeghjklmnprstuwyz%".contains(&conv),
    }
}

fn iso_week_days(yday: i64, wday: i64) -> i64 {
    // Dias desde a segunda-feira da semana 1 do ISO 8601 (a semana que contém a quinta-feira).
    yday - (yday - wday + 4 + 378) % 7 + 3
}

/// Ano e semana ISO com a aritmética de `int` do glibc sobre o ano já estourado.
fn iso_week(tm: &Tm) -> (i32, i64) {
    let mut year = tm.wrapped_year();
    let mut days = iso_week_days(tm.yday, tm.wday);
    if days < 0 {
        year = year.wrapping_sub(1);
        days = iso_week_days(tm.yday + 365 + i64::from(is_leap(i64::from(year))), tm.wday);
    } else {
        let next = iso_week_days(tm.yday - (365 + i64::from(is_leap(i64::from(year)))), tm.wday);
        if next >= 0 {
            year = year.wrapping_add(1);
            days = next;
        }
    }
    (year, days / 7 + 1)
}

fn format_into(out: &mut Output, fmt: &[u8], tm: &Tm, seconds: &mut dyn FnMut() -> i64) {
    let mut i = 0;
    while i < fmt.len() && !out.overflow {
        if fmt[i] != b'%' {
            let next = fmt[i..].iter().position(|&c| c == b'%').map_or(fmt.len(), |n| i + n);
            out.push(&fmt[i..next]);
            i = next;
            continue;
        }
        let start = i;
        i += 1;
        let mut spec = Spec { pad: Pad::Default, width: None, upcase: false, swap_case: false };
        while let Some(&c) = fmt.get(i) {
            match c {
                b'_' => spec.pad = Pad::Space,
                b'-' => spec.pad = Pad::None,
                b'0' => spec.pad = Pad::Zero,
                b'^' => spec.upcase = true,
                b'#' => spec.swap_case = true,
                _ => break,
            }
            i += 1;
        }
        if fmt.get(i).is_some_and(u8::is_ascii_digit) {
            let mut width: usize = 0;
            while let Some(&c) = fmt.get(i).filter(|c| c.is_ascii_digit()) {
                width = width.saturating_mul(10).saturating_add(usize::from(c - b'0')).min(i32::MAX as usize);
                i += 1;
            }
            spec.width = Some(width);
        }
        let mut modifier = 0;
        if let Some(&c @ (b'E' | b'O')) = fmt.get(i) {
            modifier = c;
            i += 1;
        }
        let Some(&conv) = fmt.get(i) else {
            // `%` incompleto no fim: sai literal.
            let case = if spec.upcase { Case::Upper } else { Case::Keep };
            put_text(out, &fmt[start..], &spec, case);
            break;
        };
        i += 1;
        if !conversion_allowed(conv, modifier) {
            // Conversão desconhecida ou modificador recusado: o trecho sai literal, mas a caixa
            // já decidida vale (`^` em qualquer uma; `#` em nome de mês, que o glibc trata antes
            // de recusar o modificador, ao contrário do nome do dia).
            let names = b"bBh".contains(&conv);
            let case = if spec.upcase || (names && spec.swap_case) { Case::Upper } else { Case::Keep };
            put_text(out, &fmt[start..i], &spec, case);
            continue;
        }
        format_one(out, conv, &spec, tm, seconds);
    }
}

fn format_one(out: &mut Output, conv: u8, spec: &Spec, tm: &Tm, seconds: &mut dyn FnMut() -> i64) {
    let name_case = if spec.upcase || spec.swap_case { Case::Upper } else { Case::Keep };
    let hour12 = if tm.hour % 12 == 0 { 12 } else { tm.hour % 12 };
    match conv {
        b'a' => put_text(out, &WEEKDAYS[tm.wday as usize][..3], spec, name_case),
        b'A' => put_text(out, WEEKDAYS[tm.wday as usize], spec, name_case),
        b'b' | b'h' => put_text(out, &MONTHS[tm.mon as usize][..3], spec, name_case),
        b'B' => put_text(out, MONTHS[tm.mon as usize], spec, name_case),
        b'c' => put_compound(out, b"%a %b %e %H:%M:%S %Y", spec, tm, seconds),
        b'C' => put_number(out, i64::from(tm.wrapped_year()).div_euclid(100), 1, spec, Pad::Zero),
        b'd' => put_number(out, tm.mday, 2, spec, Pad::Zero),
        b'D' | b'x' => put_compound(out, b"%m/%d/%y", spec, tm, seconds),
        b'e' => put_number(out, tm.mday, 2, spec, Pad::Space),
        b'F' => put_compound(out, b"%Y-%m-%d", spec, tm, seconds),
        b'g' => put_number(out, i64::from(iso_week(tm).0).rem_euclid(100), 2, spec, Pad::Zero),
        b'G' => put_number(out, i64::from(iso_week(tm).0), 1, spec, Pad::Zero),
        b'H' => put_number(out, tm.hour, 2, spec, Pad::Zero),
        b'I' => put_number(out, hour12, 2, spec, Pad::Zero),
        b'j' => put_number(out, tm.yday + 1, 3, spec, Pad::Zero),
        b'k' => put_number(out, tm.hour, 2, spec, Pad::Space),
        b'l' => put_number(out, hour12, 2, spec, Pad::Space),
        b'm' => put_number(out, tm.mon + 1, 2, spec, Pad::Zero),
        b'M' => put_number(out, tm.min, 2, spec, Pad::Zero),
        b'n' => put_text(out, b"\n", spec, Case::Keep),
        b'p' | b'P' => {
            let text: &[u8] = if tm.hour < 12 { b"AM" } else { b"PM" };
            let case = if conv == b'P' || spec.swap_case { Case::Lower } else { Case::Keep };
            put_text(out, text, spec, case);
        }
        b'r' => put_compound(out, b"%I:%M:%S %p", spec, tm, seconds),
        b'R' => put_compound(out, b"%H:%M", spec, tm, seconds),
        b's' => put_text(out, seconds().to_string().as_bytes(), spec, Case::Keep),
        b'S' => put_number(out, tm.sec, 2, spec, Pad::Zero),
        b't' => put_text(out, b"\t", spec, Case::Keep),
        b'T' | b'X' => put_compound(out, b"%H:%M:%S", spec, tm, seconds),
        b'u' => put_number(out, (tm.wday + 6) % 7 + 1, 1, spec, Pad::Zero),
        b'U' => put_number(out, (tm.yday - tm.wday + 7) / 7, 2, spec, Pad::Zero),
        b'V' => put_number(out, iso_week(tm).1, 2, spec, Pad::Zero),
        b'w' => put_number(out, tm.wday, 1, spec, Pad::Zero),
        b'W' => put_number(out, (tm.yday - (tm.wday + 6) % 7 + 7) / 7, 2, spec, Pad::Zero),
        b'y' => put_number(out, i64::from(tm.tm_year).rem_euclid(100), 2, spec, Pad::Zero),
        b'Y' => put_number(out, i64::from(tm.wrapped_year()), 1, spec, Pad::Zero),
        b'z' => {
            let (sign, diff): (&[u8], i64) = if tm.gmtoff < 0 { (b"-", -tm.gmtoff) } else { (b"+", tm.gmtoff) };
            put_text(out, sign, spec, Case::Keep);
            let minutes = diff / 60;
            put_number(out, minutes / 60 * 100 + minutes % 60, 4, spec, Pad::Zero);
        }
        b'Z' => {
            let case = if spec.swap_case {
                Case::Lower
            } else if spec.upcase {
                Case::Upper
            } else {
                Case::Keep
            };
            put_text(out, &tm.zone, spec, case);
        }
        _ => put_text(out, b"%", spec, Case::Keep),
    }
}

/// Conversões compostas (`%c`, `%D`, `%F`, `%r`, `%R`, `%T`, `%x`, `%X`): formata o subformato e
/// aplica a largura ao resultado inteiro; só a flag `^` passa adiante.
fn put_compound(out: &mut Output, sub: &[u8], spec: &Spec, tm: &Tm, seconds: &mut dyn FnMut() -> i64) {
    let mut inner = Output { buf: Vec::new(), max: out.max, overflow: false };
    format_into(&mut inner, sub, tm, seconds);
    if inner.overflow {
        out.overflow = true;
        return;
    }
    let case = if spec.upcase { Case::Upper } else { Case::Keep };
    put_text(out, &inner.buf, spec, case);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_files(_: &[u8]) -> Option<Vec<u8>> {
        None
    }

    fn zone(tz: &str) -> TimeZone {
        TimeZone::resolve(Some(tz.as_bytes()), &mut no_files)
    }

    fn fmt(f: &str, t: i64, tz: &TimeZone) -> String {
        String::from_utf8(strftime(f.as_bytes(), t, tz)).unwrap()
    }

    #[test]
    fn time_zone_is_send_and_sync() {
        fn check<T: Send + Sync + Clone + std::fmt::Debug>() {}
        check::<TimeZone>();
    }

    #[test]
    fn utc_basics() {
        let utc = TimeZone::utc();
        let t = 1_775_635_445; // 2026-04-08 08:04:05, quarta-feira
        assert_eq!(fmt("%a %A %b %B %h", t, &utc), "Wed Wednesday Apr April Apr");
        assert_eq!(fmt("%c", t, &utc), "Wed Apr  8 08:04:05 2026");
        assert_eq!(fmt("%C %d %D %e %F", t, &utc), "20 08 04/08/26  8 2026-04-08");
        assert_eq!(fmt("%g %G %H %I %j %k %l %m %M", t, &utc), "26 2026 08 08 098  8  8 04 04");
        assert_eq!(fmt("%p %P %r %R %s %S %T", t, &utc), "AM am 08:04:05 AM 08:04 1775635445 05 08:04:05");
        assert_eq!(
            fmt("%u %U %V %w %W %x %X %y %Y %z %Z %%", t, &utc),
            "3 14 15 3 14 04/08/26 08:04:05 26 2026 +0000 GMT %"
        );
        assert_eq!(fmt("%n%t", t, &utc), "\n\t");
        assert_eq!(fmt("%H:%M %Z", 3600, &utc), "01:00 GMT");
    }

    #[test]
    fn flags_width_and_bad_formats() {
        let utc = TimeZone::utc();
        let t = 1_775_635_445;
        assert_eq!(
            fmt("[%10Y|%-d|%_H|%012z|%_z|%-3z]", t, &utc),
            "[0000002026|8| 8|00000000000+000000000000|+   0|  +  0]"
        );
        assert_eq!(
            fmt("[%^a|%#a|%#Z|%^#p|%^P|%#10Z|%010a]", t, &utc),
            "[WED|WED|gmt|am|am|       gmt|0000000Wed]"
        );
        assert_eq!(
            fmt("[%Ed|%OY|%Oa|%EOd|%+|%q|%10q|%5_d|%5]", t, &utc),
            "[%Ed|%OY|%Oa|%EOd|%+|%q|      %10q|  %5_d|  %5]"
        );
        // Literal de conversão recusada: `^` sempre muda a caixa, `#` só no nome do mês.
        assert_eq!(fmt("[%^q|%#Eb|%#Ea|%ET|%Eu]", t, &utc), "[%^Q|%#EB|%#Ea|08:04:05|3]");
        assert_eq!(fmt("[%5", t, &utc), "[   %5");
        assert_eq!(fmt("[%-E", t, &utc), "[%-E");
        assert_eq!(fmt("[%Ec|%EY|%Od|%Ob|%Oz]", t, &utc), "[Wed Apr  8 08:04:05 2026|2026|08|Apr|+0000]");
        assert_eq!(fmt("[%05s|%5s]", -1, &utc), "[000-1|   -1]");
        assert_eq!(fmt("[%012R|%^c]", t, &utc), "[000000008:04|WED APR  8 08:04:05 2026]");
        assert_eq!(fmt("%", t, &utc), "%");
        assert_eq!(strftime(b"a\0b%Y", t, &utc), b"a");
        assert_eq!(strftime(b"", t, &utc), b"");
        // Saída maior que 1024 vezes o formato (arredondado para potência de dois) vira "".
        assert_eq!(strftime(b"%8191d", 0, &utc).len(), 8191);
        assert!(strftime(b"%8192d", 0, &utc).is_empty());
    }

    #[test]
    fn extreme_years() {
        let utc = TimeZone::utc();
        assert_eq!(fmt("%Y %C %y %G %g %F", -62_167_219_200, &utc), "0 0 00 -1 99 0-01-01");
        assert_eq!(fmt("%Y %C %y %3Y %12Y", -62_167_219_201, &utc), "-1 -1 99 -01 -00000000001");
        assert_eq!(fmt("%Y|%C|%y|%G|%g", 67_768_036_191_676_792, &utc), "-2147481749|-21474818|47|-2147481748|52");
        assert_eq!(fmt("%Y", -67_768_040_609_740_800, &utc), "-2147481748");
        // Ano fora do `int` da `struct tm`: o `localtime` falha e o gawk devolve "".
        assert_eq!(fmt("[%Y]", 67_768_036_191_676_800, &utc), "");
        assert_eq!(time_from_number(1.9), Some(1));
        assert_eq!(time_from_number(-1.9), Some(-1));
        assert_eq!(time_from_number(f64::NAN), None);
        assert_eq!(time_from_number(1e20), None);
    }

    #[test]
    fn posix_strings() {
        let t = 1_768_478_400; // 2026-01-15 12:00:00 UTC
        let summer = 1_783_080_000; // 2026-07-03 12:00:00 UTC
        let cases = [
            ("EST5EDT,M3.2.0,M11.1.0", "07 EST -0500", "08 EDT -0400"),
            ("<-03>3", "09 -03 -0300", "09 -03 -0300"),
            ("JST-9", "21 JST +0900", "21 JST +0900"),
            ("GMT+3", "09 GMT -0300", "09 GMT -0300"),
            ("UTC0", "12 UTC +0000", "12 UTC +0000"),
            ("Foo/Bar", "12 Foo +0000", "12 Foo +0000"),
            ("ABC5:", "12  +0000", "12  +0000"),
            ("EST5EDT:", "08 EDT -0400", "08 EDT -0400"),
            ("ABC99", "12 ABC -2400", "12 ABC -2400"),
            ("ABC5:99:99", "06 ABC -0559", "06 ABC -0559"),
            ("AAA3BBB,M10.1.0,M3.1.0", "10 BBB -0200", "09 AAA -0300"),
            ("EST5EDT,366,300", "08 EDT -0400", "08 EDT -0400"),
            ("EST5EDT,J0,J300", "07 EST -0500", "07 EST -0500"),
            ("a1b2", "12  +0000", "12  +0000"),
        ];
        for (tz, winter, summer_out) in cases {
            let z = zone(tz);
            assert_eq!(fmt("%H %Z %z", t, &z), winter, "TZ={tz}");
            assert_eq!(fmt("%H %Z %z", summer, &z), summer_out, "TZ={tz}");
        }
        // Antes de 1971 o glibc nunca aplica horário de verão de regra POSIX.
        assert_eq!(fmt("%Z", -300_000_000, &zone("EST5EDT,M3.2.0,M11.1.0")), "EST");
    }

    #[test]
    fn special_tz_values() {
        assert_eq!(fmt("%H %Z", 0, &TimeZone::resolve(Some(b""), &mut no_files)), "00 Universal");
        assert_eq!(fmt("%H %Z", 0, &TimeZone::resolve(Some(b":"), &mut no_files)), "00 UTC");
        assert_eq!(fmt("%H %Z", 0, &TimeZone::resolve(None, &mut no_files)), "00 UTC");
        assert_eq!(fmt("%H %Z|", 0, &TimeZone::resolve(Some(b"/nonexistent"), &mut no_files)), "00 |");
        // Nomes só do tzdata-legacy não existem no Debian 13.
        assert_eq!(fmt("%Z|", 0, &zone("US/Eastern")), "|");
        assert_eq!(fmt("%Z", 0, &zone("EST")), "EST");
    }

    #[test]
    fn bundled_database() {
        let sp = zone("America/Sao_Paulo");
        assert_eq!(fmt("%F %T %Z %z", 0, &sp), "1969-12-31 21:00:00 -03 -0300");
        let berlin = zone(":Europe/Berlin");
        assert_eq!(fmt("%T %Z", 1_768_478_400, &berlin), "13:00:00 CET");
        assert_eq!(fmt("%T %Z", 1_783_080_000, &berlin), "14:00:00 CEST");
        assert_eq!(fmt("%T %Z %z", 0, &zone("Asia/Kolkata")), "05:30:00 IST +0530");
        assert_eq!(fmt("%Z", 0, &zone("america/sao_paulo")), "america");
        // EST5EDT sem regra usa o posixrules (New_York): em 2000 ainda valia a regra antiga.
        let est = zone("EST5EDT");
        assert_eq!(fmt("%F %H %Z", 953_553_600, &est), "2000-03-20 07 EST");
        // ABC3DEF: transições de início deslocadas, de fim não; depois de 2037 vale o rodapé.
        let abc = zone("ABC3DEF");
        assert_eq!(fmt("%Z", 1_899_363_599, &abc), "ABC");
        assert_eq!(fmt("%Z", 1_899_363_600, &abc), "DEF");
        assert_eq!(fmt("%Z", 1_919_915_999, &abc), "DEF");
        assert_eq!(fmt("%Z", 1_919_916_000, &abc), "ABC");
        assert_eq!(fmt("%Z %z", 2_208_988_800, &abc), "EST -0500");
    }

    #[test]
    fn mktime_utc_and_parsing() {
        let utc = TimeZone::utc();
        let m = |s: &str| mktime(s.as_bytes(), &utc);
        assert_eq!(m("2026 01 15 12 00 00"), 1_768_478_400);
        assert_eq!(m("2026 13 01 00 00 00"), 1_798_761_600);
        assert_eq!(m("garbage"), -1);
        assert_eq!(m("2026 01 15 12 00"), -1);
        assert_eq!(m("2026 01 15 12 00 00 garbage"), 1_768_478_400);
        assert_eq!(m("+2026 +1 +15 +12 +0 +0"), 1_768_478_400);
        assert_eq!(m("2026.5 01 15 12 00 00"), -1);
        assert_eq!(m("2026 01 15 12 00 3600000000"), 1_073_511_104);
        assert_eq!(m("2026 01 15 12 00 99999999999999999999"), 1_768_478_399);
        assert_eq!(m("2026 -2147483648 1 0 0 0"), -1);
        assert_eq!(m("2147485548 1 1 0 0 0"), -1);
        assert_eq!(m("-2147481748 1 1 0 0 0"), -67_768_040_609_740_800);
        assert_eq!(m("-2147481748 1 1 0 0 -1"), -1);
        assert_eq!(m("2017 1 1 0 0 0"), 1_483_228_800);
        assert_eq!(m("2026 01 15 12 00 00 1"), 1_768_478_400);
    }

    #[test]
    fn mktime_dst_rules() {
        let ny = zone("America/New_York");
        let m = |s: &str| mktime(s.as_bytes(), &ny);
        let f = |t: i64| fmt("%F %T %Z", t, &ny);
        // Buraco da primavera.
        assert_eq!(f(m("2026 03 08 02 30 00")), "2026-03-08 03:30:00 EDT");
        assert_eq!(f(m("2026 03 08 02 30 00 0")), "2026-03-08 03:30:00 EDT");
        assert_eq!(f(m("2026 03 08 02 30 00 1")), "2026-03-08 01:30:00 EST");
        // Hora repetida: depende do resultado anterior, como no glibc.
        m("2026 12 01 00 00 00");
        assert_eq!(f(m("2026 11 01 01 30 00")), "2026-11-01 01:30:00 EST");
        m("2026 07 01 00 00 00");
        assert_eq!(f(m("2026 11 01 01 30 00")), "2026-11-01 01:30:00 EDT");
        assert_eq!(f(m("2026 11 01 01 30 00 0")), "2026-11-01 01:30:00 EST");
        // tm_isdst contrário ao vigente.
        assert_eq!(f(m("2026 07 01 12 00 00 0")), "2026-07-01 13:00:00 EDT");
        assert_eq!(f(m("2026 01 01 12 00 00 1")), "2026-01-01 11:00:00 EST");
        // Segundos fora do intervalo entram depois de achar o deslocamento.
        assert_eq!(f(m("2026 03 08 03 00 -1800")), "2026-03-08 01:30:00 EST");
        // %s no modo UTC passa pelo mktime local.
        let sp = zone("America/Sao_Paulo");
        assert_eq!(strftime_utc(b"%s", 0, &sp), b"10800");
        assert_eq!(strftime_utc(b"%H:%M %Z", 3600, &sp), b"01:00 GMT");
    }

    #[test]
    fn gawk_suite_cases() {
        // test/strftlng.awk (TZ=UTC): formato longo repetido.
        let utc = zone("UTC");
        let simple = "%m/%d/%y %H:%M:%S\n";
        let format = simple.repeat(58);
        assert_eq!(fmt(&format, 0, &utc), "01/01/70 00:00:00\n".repeat(58));
        // test/negtime.awk (TZ=GMT).
        let gmt = zone("GMT");
        let then = mktime(b"1959 12 15 7 00 00", &gmt);
        assert_eq!(fmt("%a %b %e %H:%M:%S %Z %Y", then, &gmt), "Tue Dec 15 07:00:00 GMT 1959");
        // test/mktime.awk: mktime($0, 1).
        assert_eq!(mktime(b"2017 1 1 0 0 0", &TimeZone::utc()), 1_483_228_800);
    }

    #[test]
    fn posix_rule_state_like_glibc() {
        // Regra não lida fica com a troca zerada para o ano 0 até a primeira conta de outro ano.
        let tz = zone("ABC5:");
        let year0 = -62_167_219_200;
        assert_eq!(fmt("%F %Z", year0, &tz), "-1-12-31 ABC");
        assert_eq!(fmt("%F %Z|", 1_000_000_000, &tz), "2001-09-09 |");
        assert_eq!(fmt("%F %Z|", year0, &tz), "0-01-01 |");
        assert_eq!(fmt("%F %Z", year0, &zone("ABC5:")), "-1-12-31 ABC");
    }

    /// Monta um TZif v2 (o bloco v1 fica vazio) com tipos `(deslocamento, isdst, índice)`.
    fn tzif(times: &[i64], idx: &[u8], types: &[(i32, u8, u8)], chars: &[u8], footer: &[u8]) -> Vec<u8> {
        let header = |timecnt: usize, typecnt: usize, charcnt: usize| -> Vec<u8> {
            let mut h = b"TZif2".to_vec();
            h.resize(20, 0);
            for n in [0, 0, 0, timecnt, typecnt, charcnt] {
                h.extend_from_slice(&(n as i32).to_be_bytes());
            }
            h
        };
        let mut data = header(0, 1, 1);
        data.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]);
        data.extend(header(times.len(), types.len(), chars.len()));
        for t in times {
            data.extend_from_slice(&t.to_be_bytes());
        }
        data.extend_from_slice(idx);
        for &(off, dst, abbr) in types {
            data.extend_from_slice(&off.to_be_bytes());
            data.extend_from_slice(&[dst, abbr]);
        }
        data.extend_from_slice(chars);
        data.push(b'\n');
        data.extend_from_slice(footer);
        data.push(b'\n');
        data
    }

    fn zone_from_file(data: Vec<u8>) -> TimeZone {
        let mut read = |path: &[u8]| (path == b"/tz/test").then(|| data.clone());
        TimeZone::resolve(Some(b"/tz/test"), &mut read)
    }

    #[test]
    fn tzif_files_like_glibc() {
        let chars = b"AAA\0BBB\0CCC\0";
        let types = [(3600, 1, 0), (7200, 0, 4), (0, 0, 8)];
        let at = 1_000_000_000;
        // Antes da primeira transição vale o primeiro tipo sem horário de verão.
        let f1 = zone_from_file(tzif(&[at], &[2], &types, chars, b""));
        assert_eq!(fmt("%Z %z", 0, &f1), "BBB +0200");
        assert_eq!(fmt("%Z %z", at, &f1), "CCC +0000");
        // Rodapé inválido passa pelo parse POSIX; rodapé vazio mantém o último tipo.
        let garbage = zone_from_file(tzif(&[at], &[1], &types, chars, b"garbage"));
        assert_eq!(fmt("%Z %z", at, &garbage), "garbage +0000");
        let empty = zone_from_file(tzif(&[at], &[1], &types, chars, b""));
        assert_eq!(fmt("%Z %z", at + 1, &empty), "BBB +0200");
        // Sem transições o rodapé não é usado.
        let only = zone_from_file(tzif(&[], &[], &[(7200, 0, 4)], chars, b"EST5EDT,M3.2.0,M11.1.0"));
        assert_eq!(fmt("%Z", 2_000_000_000, &only), "BBB");
        // isdst diferente de 0 e 1 invalida o arquivo: vira parse POSIX do caminho (nome vazio).
        let bad = zone_from_file(tzif(&[at], &[1], &[(3600, 1, 0), (7200, 2, 4)], chars, b""));
        assert_eq!(fmt("%Z|%z", 0, &bad), "|+0000");
    }

    #[test]
    fn ruleless_footer_side_effect() {
        // Rodapé `EST5EDT` sem regra: horário de verão quase o ano todo e, dentro do mesmo
        // `mktime`, as conversões seguintes já usam o posixrules (aqui, o New_York embutido).
        let chars = b"AAA\0BBB\0CCC\0";
        let types = [(3600, 1, 0), (7200, 0, 4), (0, 0, 8)];
        let f8 = zone_from_file(tzif(&[1_000_000_000], &[1], &types, chars, b"EST5EDT"));
        let t = mktime(b"2035 2 14 0 15 00", &f8);
        assert_eq!(t, 2_055_042_900);
        assert_eq!(fmt("%F %T %Z %z", t, &f8), "2035-02-14 01:15:00 EDT -0400");
    }

    #[test]
    fn mktime_without_dst_falls_back_one_hour() {
        let kolkata = zone("Asia/Kolkata");
        let a = mktime(b"2000 06 01 00 00 00 1", &kolkata);
        let b = mktime(b"2000 06 01 00 00 00 0", &kolkata);
        assert_eq!(a - b, -3600);
    }
}
