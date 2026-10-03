//! Front-end do `date`, nosso e igual pra todos os candidatos: linha de comando (getopt_long do GNU),
//! "agora" e fuso vindos do caso (nunca do host), -d/-f/-r, formatos de saída e mensagens de erro.
//! A biblioteca entra só em duas peças: [`DateParser`] (gramática do `-d`) e [`DateFormatter`]
//! (strftime e fusos).

use std::time::{SystemTime, UNIX_EPOCH};

use harness::{Candidate, Invocation, Outcome};

/// Instante neutro entre bibliotecas: segundos desde a época (piso) e nanossegundos em `0..1e9`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Instant {
    pub secs: i64,
    pub nanos: u32,
}

impl Instant {
    pub fn from_nanos(total: i128) -> Instant {
        let secs = total.div_euclid(1_000_000_000) as i64;
        let nanos = total.rem_euclid(1_000_000_000) as u32;
        Instant { secs, nanos }
    }

    pub fn as_nanos(self) -> i128 {
        self.secs as i128 * 1_000_000_000 + self.nanos as i128
    }
}

/// Data e hora civis (sem fuso), como no `FAKETIME`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil {
    pub year: i16,
    pub month: i8,
    pub day: i8,
    pub hour: i8,
    pub minute: i8,
    pub second: i8,
}

/// Gramática do `date -d` vinda de uma biblioteca.
pub trait DateParser: Send + Sync {
    fn name(&self) -> String;
    /// Interpreta `input` com `now` como base e `tz` (valor cru de TZ) como fuso local.
    fn parse(&self, input: &str, now: Instant, tz: &str) -> Result<Instant, String>;
}

/// strftime e banco de fusos vindos de uma biblioteca.
pub trait DateFormatter: Send + Sync {
    fn name(&self) -> String;
    fn format(&self, t: Instant, tz: &str, fmt: &str) -> Result<String, String>;
    /// Hora civil no fuso `tz` pra instante (usado pro relógio congelado do caso).
    fn civil_to_instant(&self, civil: Civil, tz: &str) -> Option<Instant>;
}

/// Um candidato: o front-end com um parser e um formatador.
pub struct DateCandidate {
    pub parser: Box<dyn DateParser>,
    pub formatter: Box<dyn DateFormatter>,
}

impl Candidate for DateCandidate {
    fn name(&self) -> String {
        let p = self.parser.name();
        let f = self.formatter.name();
        if p == f { p } else { format!("{p} + {f}") }
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        run_date(self.parser.as_ref(), self.formatter.as_ref(), inv)
    }
}

const TRY_HELP: &str = "Try 'date --help' for more information.\n";
const DEFAULT_FORMAT: &str = "%a %b %e %H:%M:%S %Z %Y";
const RFC_EMAIL_FORMAT: &str = "%a, %d %b %Y %H:%M:%S %z";
const ISO_ARGS: &[(&str, &str)] = &[
    ("hours", "%Y-%m-%dT%H%:z"),
    ("minutes", "%Y-%m-%dT%H:%M%:z"),
    ("date", "%Y-%m-%d"),
    ("seconds", "%Y-%m-%dT%H:%M:%S%:z"),
    ("ns", "%Y-%m-%dT%H:%M:%S,%N%:z"),
];
const RFC3339_ARGS: &[(&str, &str)] = &[
    ("date", "%Y-%m-%d"),
    ("seconds", "%Y-%m-%d %H:%M:%S%:z"),
    ("ns", "%Y-%m-%d %H:%M:%S.%N%:z"),
];

/// Opções longas do `date` do coreutils 9.7: (nome, argumento: 0 nenhum, 1 obrigatório, 2 opcional).
const LONG_OPTIONS: &[(&str, u8)] = &[
    ("date", 1),
    ("debug", 0),
    ("file", 1),
    ("iso-8601", 2),
    ("reference", 1),
    ("resolution", 0),
    ("rfc-email", 0),
    ("rfc-822", 0),
    ("rfc-2822", 0),
    ("rfc-3339", 1),
    ("set", 1),
    ("uct", 0),
    ("utc", 0),
    ("universal", 0),
    ("help", 0),
    ("version", 0),
];

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub date: Option<String>,
    pub file: Option<String>,
    pub reference: Option<String>,
    pub utc: bool,
    pub format: Option<String>,
    pub operands: Vec<String>,
}

/// Resultado de parsear a linha de comando: ou os argumentos, ou a saída de erro pronta.
#[derive(Debug, PartialEq, Eq)]
pub enum ParsedArgs {
    Ok(Args),
    Fail { stderr: String, exit: i32 },
    Unsupported(String),
}

fn usage_error(msg: &str) -> ParsedArgs {
    ParsedArgs::Fail { stderr: format!("date: {msg}\n{TRY_HELP}"), exit: 1 }
}

/// `argmatch` do gnulib: nome exato ou prefixo sem ambiguidade.
fn argmatch<'a>(value: &str, table: &'a [(&'a str, &'a str)]) -> Option<&'a str> {
    if let Some((_, f)) = table.iter().find(|(k, _)| *k == value) {
        return Some(f);
    }
    let hits: Vec<&str> = table.iter().filter(|(k, _)| k.starts_with(value) && !value.is_empty()).map(|(_, f)| *f).collect();
    match hits.as_slice() {
        [first, rest @ ..] if rest.iter().all(|f| f == first) => Some(first),
        _ => None,
    }
}

fn invalid_argument(value: &str, option: &str, table: &[(&str, &str)]) -> ParsedArgs {
    let mut msg = format!("date: invalid argument {} for {}\nValid arguments are:\n", quote(value), quote(option));
    let mut seen: Vec<&str> = Vec::new();
    for (name, fmt) in table {
        if seen.contains(fmt) {
            continue;
        }
        seen.push(fmt);
        msg.push_str(&format!("  - {}\n", quote(name)));
    }
    msg.push_str(TRY_HELP);
    ParsedArgs::Fail { stderr: msg, exit: 1 }
}

/// Aplica um formato vindo de opção (-I, -R, --rfc-3339) com a checagem do GNU.
fn set_new_format(args: &mut Args, fmt: &str) -> Option<ParsedArgs> {
    if args.format.is_some() {
        return Some(ParsedArgs::Fail { stderr: "date: multiple output formats specified\n".into(), exit: 1 });
    }
    args.format = Some(fmt.to_string());
    None
}

/// getopt_long com permutação, no formato de mensagens da glibc.
pub fn parse_args(argv: &[String]) -> ParsedArgs {
    let mut args = Args::default();
    let mut i = 0;
    let mut only_operands = false;
    while i < argv.len() {
        let arg = &argv[i];
        i += 1;
        if only_operands || arg == "-" || !arg.starts_with('-') {
            args.operands.push(arg.clone());
            continue;
        }
        if arg == "--" {
            only_operands = true;
            continue;
        }
        // (opção normalizada, argumento)
        let (opt, value): (String, Option<String>) = if let Some(long) = arg.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let exact = LONG_OPTIONS.iter().find(|(n, _)| *n == name);
            let found = match exact {
                Some(o) => *o,
                None => {
                    let hits: Vec<&(&str, u8)> = LONG_OPTIONS.iter().filter(|(n, _)| n.starts_with(name)).collect();
                    match hits.as_slice() {
                        [one] => **one,
                        [] => return usage_error(&format!("unrecognized option '--{name}'")),
                        many => {
                            let names: Vec<String> = many.iter().map(|(n, _)| format!("'--{n}'")).collect();
                            return usage_error(&format!("option '--{name}' is ambiguous; possibilities: {}", names.join(" ")));
                        }
                    }
                }
            };
            let (full, kind) = found;
            let value = match (kind, inline) {
                (0, Some(_)) => return usage_error(&format!("option '--{full}' doesn't allow an argument")),
                (0, None) => None,
                (1, Some(v)) => Some(v),
                (1, None) => {
                    if i < argv.len() {
                        i += 1;
                        Some(argv[i - 1].clone())
                    } else {
                        return usage_error(&format!("option '--{full}' requires an argument"));
                    }
                }
                (_, v) => v,
            };
            (format!("--{full}"), value)
        } else {
            // Grupo de opções curtas: -ud STR, -Iseconds, -d2026...
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut j = 0;
            let mut last: Option<(String, Option<String>)> = None;
            while j < chars.len() {
                let c = chars[j];
                j += 1;
                let rest: String = chars[j..].iter().collect();
                match c {
                    'R' | 'u' => {
                        let item = (format!("-{c}"), None);
                        if let Some(fail) = apply_option(&mut args, &item.0, item.1) {
                            return fail;
                        }
                    }
                    'I' => {
                        let value = if rest.is_empty() { None } else { Some(rest.clone()) };
                        last = Some(("-I".into(), value));
                        break;
                    }
                    'd' | 'f' | 'r' | 's' => {
                        let value = if !rest.is_empty() {
                            rest.clone()
                        } else if i < argv.len() {
                            i += 1;
                            argv[i - 1].clone()
                        } else {
                            return usage_error(&format!("option requires an argument -- '{c}'"));
                        };
                        last = Some((format!("-{c}"), Some(value)));
                        break;
                    }
                    other => return usage_error(&format!("invalid option -- '{other}'")),
                }
            }
            match last {
                Some(item) => item,
                None => continue,
            }
        };
        if let Some(fail) = apply_option(&mut args, &opt, value) {
            return fail;
        }
    }
    // Checagens do main() do date.c, na mesma ordem.
    let specified = [&args.date, &args.file, &args.reference].iter().filter(|o| o.is_some()).count();
    if specified > 1 {
        return usage_error("the options to specify dates for printing are mutually exclusive");
    }
    if args.operands.len() > 1 {
        return usage_error(&format!("extra operand {}", quote(&args.operands[1])));
    }
    if let Some(op) = args.operands.first().cloned() {
        if let Some(fmt) = op.strip_prefix('+') {
            if args.format.is_some() {
                return ParsedArgs::Fail { stderr: "date: multiple output formats specified\n".into(), exit: 1 };
            }
            args.format = Some(fmt.to_string());
            args.operands.clear();
        } else if specified > 0 {
            return usage_error(&format!(
                "the argument {} lacks a leading '+';\nwhen using an option to specify date(s), any non-option\nargument must be a format string beginning with '+'",
                quote(&op)
            ));
        } else {
            return ParsedArgs::Unsupported("acertar o relógio (operando MMDDhhmm) não é suportado".into());
        }
    }
    ParsedArgs::Ok(args)
}

/// Aplica uma opção já normalizada. Devolve `Some` quando a linha de comando falha.
fn apply_option(args: &mut Args, opt: &str, value: Option<String>) -> Option<ParsedArgs> {
    match opt {
        "-d" | "--date" => args.date = value,
        "-f" | "--file" => args.file = value,
        "-r" | "--reference" => args.reference = value,
        "-u" | "--utc" | "--uct" | "--universal" => args.utc = true,
        "-R" | "--rfc-email" | "--rfc-822" | "--rfc-2822" => return set_new_format(args, RFC_EMAIL_FORMAT),
        "-I" | "--iso-8601" => {
            let fmt = match value.as_deref() {
                None => "%Y-%m-%d",
                Some(v) => match argmatch(v, ISO_ARGS) {
                    Some(f) => f,
                    None => return Some(invalid_argument(v, "--iso-8601", ISO_ARGS)),
                },
            };
            return set_new_format(args, fmt);
        }
        "--rfc-3339" => {
            let v = value.unwrap_or_default();
            let fmt = match argmatch(&v, RFC3339_ARGS) {
                Some(f) => f,
                None => return Some(invalid_argument(&v, "--rfc-3339", RFC3339_ARGS)),
            };
            return set_new_format(args, fmt);
        }
        "-s" | "--set" => return Some(ParsedArgs::Unsupported("--set não é suportado".into())),
        "--debug" | "--resolution" | "--help" | "--version" => {
            return Some(ParsedArgs::Unsupported(format!("{opt} não é suportado")));
        }
        other => return Some(usage_error(&format!("unrecognized option '{other}'"))),
    }
    None
}

/// `quote()` do gnulib em locale UTF-8: aspas tipográficas simples.
pub fn quote(s: &str) -> String {
    format!("\u{2018}{s}\u{2019}")
}

/// `quotef()` do gnulib: aspas de shell só quando o nome precisa.
pub fn quotef(s: &str) -> String {
    let safe = !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "._/@%+=:,-".contains(c));
    if safe { s.to_string() } else { format!("'{}'", s.replace('\'', "'\\''")) }
}

/// "YYYY-MM-DD HH:MM:SS" (fração ignorada), como no `FAKETIME` e no campo `faketime` do caso.
pub fn parse_civil(s: &str) -> Option<Civil> {
    let s = s.trim().trim_start_matches('@');
    let (date, time) = s.split_once(' ')?;
    let mut d = date.split('-');
    let year = d.next()?.parse().ok()?;
    let month = d.next()?.parse().ok()?;
    let day = d.next()?.parse().ok()?;
    let time = time.split('.').next()?;
    let mut t = time.split(':');
    let hour = t.next()?.parse().ok()?;
    let minute = t.next()?.parse().ok()?;
    let second = t.next().unwrap_or("0").parse().ok()?;
    Some(Civil { year, month, day, hour, minute, second })
}

/// Separa um prefixo `TZ="..."` da string do -d (com os escapes `\"` e `\\` do GNU).
/// Devolve (fuso, resto). O front-end resolve o fuso pelo banco da biblioteca, sem ler o host.
pub fn split_tz_prefix(input: &str) -> Option<(String, &str)> {
    let rest = input.trim_start().strip_prefix("TZ=\"")?;
    let mut tz = String::new();
    let mut chars = rest.char_indices();
    while let Some((idx, c)) = chars.next() {
        match c {
            '\\' => {
                let (_, next) = chars.next()?;
                tz.push(next);
            }
            '"' => return Some((tz, &rest[idx + 1..])),
            other => tz.push(other),
        }
    }
    None
}

/// O "agora" do caso: relógio congelado (`FAKETIME` absoluto) ou campo `faketime`, interpretados no TZ
/// do ambiente do caso; sem nenhum dos dois, o relógio real.
fn case_now(inv: &Invocation, env_tz: &str, formatter: &dyn DateFormatter) -> Result<Instant, String> {
    let spec = inv.env.get("FAKETIME").cloned().or_else(|| inv.faketime.clone());
    match spec {
        Some(s) => {
            let civil = parse_civil(&s).ok_or_else(|| format!("FAKETIME inválido: {s}"))?;
            formatter.civil_to_instant(civil, env_tz).ok_or_else(|| format!("FAKETIME inexistente no fuso: {s}"))
        }
        None => {
            let d = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?;
            Ok(Instant { secs: d.as_secs() as i64, nanos: d.subsec_nanos() })
        }
    }
}

/// Interpreta uma string de data como o `parse_datetime2` do GNU: prefixo TZ="..." opcional.
fn parse_date(parser: &dyn DateParser, input: &str, now: Instant, tz: &str) -> Result<Instant, String> {
    match split_tz_prefix(input) {
        Some((zone, rest)) => parser.parse(rest, now, &zone),
        None => parser.parse(input, now, tz),
    }
}

pub fn run_date(parser: &dyn DateParser, formatter: &dyn DateFormatter, inv: &Invocation) -> Outcome {
    let files = inv.files.clone();
    let args = match parse_args(inv.args()) {
        ParsedArgs::Ok(a) => a,
        ParsedArgs::Fail { stderr, exit } => return Outcome::exited("", stderr, exit, files),
        ParsedArgs::Unsupported(why) => return Outcome::unsupported(why),
    };
    let env = inv.full_env();
    let env_tz = env.get("TZ").cloned().unwrap_or_default();
    // -u faz putenv("TZ=UTC0") antes de tudo.
    let tz = if args.utc { "UTC0".to_string() } else { env_tz.clone() };
    let format = args.format.clone().unwrap_or_else(|| DEFAULT_FORMAT.to_string());
    let mut stdout = String::new();
    let mut stderr = String::new();

    let show = |t: Instant, out: &mut String, err: &mut String| -> bool {
        match formatter.format(t, &tz, &format) {
            Ok(s) => {
                out.push_str(&s);
                out.push('\n');
                true
            }
            Err(e) => {
                err.push_str(&format!("date: formatação: {e}\n"));
                false
            }
        }
    };

    if let Some(path) = &args.file {
        let text = if path == "-" {
            Some(inv.stdin.clone())
        } else {
            crate::common::fixture_file(inv, path).map(<[u8]>::to_vec)
        };
        let Some(text) = text else {
            let msg = format!("date: {}: No such file or directory\n", quotef(path));
            return Outcome::exited("", msg, 1, files);
        };
        let now = match case_now(inv, &env_tz, formatter) {
            Ok(n) => n,
            Err(e) => return Outcome::unsupported(e),
        };
        let mut ok = true;
        let text = String::from_utf8_lossy(&text).into_owned();
        for line in text.split_inclusive('\n') {
            let line = line.strip_suffix('\n').unwrap_or(line);
            match parse_date(parser, line, now, &tz) {
                Ok(t) => ok &= show(t, &mut stdout, &mut stderr),
                Err(_) => {
                    stderr.push_str(&format!("date: invalid date {}\n", quote(line)));
                    ok = false;
                }
            }
        }
        return Outcome::exited(stdout, stderr, if ok { 0 } else { 1 }, files);
    }

    let when = if let Some(path) = &args.reference {
        // A fixture tem mtime fixo; o MemTree não guarda mtime, então todo arquivo ou diretório dela vale FIXTURE_MTIME.
        let rel = crate::common::relative(path);
        if !rel.is_empty() && inv.files.get(&rel).is_none() {
            let msg = format!("date: {}: No such file or directory\n", quotef(path));
            return Outcome::exited("", msg, 1, files);
        }
        Instant { secs: harness::FIXTURE_MTIME as i64, nanos: 0 }
    } else {
        let now = match case_now(inv, &env_tz, formatter) {
            Ok(n) => n,
            Err(e) => return Outcome::unsupported(e),
        };
        match &args.date {
            Some(d) => match parse_date(parser, d, now, &tz) {
                Ok(t) => t,
                Err(_) => {
                    let msg = format!("date: invalid date {}\n", quote(d));
                    return Outcome::exited("", msg, 1, files);
                }
            },
            None => now,
        }
    };
    let ok = show(when, &mut stdout, &mut stderr);
    Outcome::exited(stdout, stderr, if ok { 0 } else { 1 }, files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_combined_short_options_and_format() {
        let ParsedArgs::Ok(a) = parse_args(&argv(&["-ud", "2026-02-03", "+%F"])) else { panic!() };
        assert!(a.utc);
        assert_eq!(a.date.as_deref(), Some("2026-02-03"));
        assert_eq!(a.format.as_deref(), Some("%F"));
    }

    #[test]
    fn permutes_format_before_options() {
        let ParsedArgs::Ok(a) = parse_args(&argv(&["+%F", "-d", "x"])) else { panic!() };
        assert_eq!(a.date.as_deref(), Some("x"));
        assert_eq!(a.format.as_deref(), Some("%F"));
    }

    #[test]
    fn iso_optional_argument_and_abbreviation() {
        let ParsedArgs::Ok(a) = parse_args(&argv(&["-Isec"])) else { panic!() };
        assert_eq!(a.format.as_deref(), Some("%Y-%m-%dT%H:%M:%S%:z"));
        let ParsedArgs::Ok(a) = parse_args(&argv(&["--iso-8601"])) else { panic!() };
        assert_eq!(a.format.as_deref(), Some("%Y-%m-%d"));
        let ParsedArgs::Ok(a) = parse_args(&argv(&["--rfc-3=ns"])) else { panic!("prefixo de opção longa") };
        assert_eq!(a.format.as_deref(), Some("%Y-%m-%d %H:%M:%S.%N%:z"));
        assert!(matches!(parse_args(&argv(&["--rfc"])), ParsedArgs::Fail { .. }), "--rfc é ambíguo");
    }

    #[test]
    fn gnu_error_messages() {
        let fail = |items: &[&str]| match parse_args(&argv(items)) {
            ParsedArgs::Fail { stderr, exit } => (stderr, exit),
            other => panic!("{other:?}"),
        };
        assert_eq!(fail(&["-x"]).0, "date: invalid option -- 'x'\nTry 'date --help' for more information.\n");
        assert_eq!(fail(&["-d"]).0, "date: option requires an argument -- 'd'\nTry 'date --help' for more information.\n");
        assert_eq!(fail(&["-I", "-R"]).0, "date: multiple output formats specified\n");
        assert_eq!(fail(&["+%F", "+%T"]).0, "date: extra operand \u{2018}+%T\u{2019}\nTry 'date --help' for more information.\n");
        assert!(fail(&["--rfc-3339=minutes"]).0.contains("  - \u{2018}ns\u{2019}\n"));
    }

    #[test]
    fn civil_and_tz_prefix() {
        assert_eq!(
            parse_civil("2026-01-15 12:00:00"),
            Some(Civil { year: 2026, month: 1, day: 15, hour: 12, minute: 0, second: 0 })
        );
        assert_eq!(split_tz_prefix(r#"TZ="Asia/Tokyo" 2026-01-15 09:00"#), Some(("Asia/Tokyo".to_string(), " 2026-01-15 09:00")));
        assert_eq!(split_tz_prefix(r#"TZ="a\"b" x"#), Some(("a\"b".to_string(), " x")));
        assert_eq!(split_tz_prefix("2026-01-15"), None);
    }

    #[test]
    fn instant_floor_semantics() {
        let t = Instant::from_nanos(-1_500_000_000);
        assert_eq!(t, Instant { secs: -2, nanos: 500_000_000 });
        assert_eq!(t.as_nanos(), -1_500_000_000);
    }

    #[test]
    fn quoting() {
        assert_eq!(quotef("missing.txt"), "missing.txt");
        assert_eq!(quotef("a b"), "'a b'");
        assert_eq!(quote("foo"), "\u{2018}foo\u{2019}");
    }
}
