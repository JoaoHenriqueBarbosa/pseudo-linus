//! Conformidade do motor contra o corpus de borda do F01 (`testbench/corpus/cases/regex`).
//!
//! Cada caso é uma sonda (um comando GNU real: `grep -n`, `grep -ob`, `sed -n '\cREcp'`,
//! `sed s///g`, `sed s//\1.../`, `gawk match()`). Aqui a sonda é emulada direto sobre a
//! biblioteca, sem os programas, pra medir só o motor (é o número comparável ao do F01). Os
//! programas de verdade rodam o mesmo corpus em `crates/ul-textproc/tests`.
//!
//! Rode com `cargo test -p regex-posix --test conformance -- --nocapture` pra ver o placar.

use std::collections::BTreeMap;

use harness::{Candidate, Invocation, Outcome};
use regex_posix::{Error, Regex, RegexBuilder, Syntax};

const DELIM: char = '\u{1}';
const OPEN: char = '\u{2}';
const SEP: char = '\u{3}';
const CLOSE: char = '\u{4}';

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    GrepN,
    GrepO,
    SedN,
    SedG,
    SedSub,
    Gawk,
}

struct Probe {
    kind: Kind,
    ere: bool,
    icase: bool,
    pattern: String,
    groups: usize,
    subjects: Vec<String>,
}

fn probe(inv: &Invocation) -> Result<Probe, String> {
    let text = |name: &str| -> Result<String, String> {
        let data = inv.files.read(name).ok_or_else(|| format!("sem {name}"))?;
        String::from_utf8(data.to_vec()).map_err(|_| format!("{name} não é UTF-8"))
    };
    let lines = |raw: String| -> Vec<String> {
        let mut v: Vec<String> = raw.split('\n').map(str::to_string).collect();
        if v.last().is_some_and(String::is_empty) {
            v.pop();
        }
        v
    };
    let a = &inv.argv;
    match a.first().map(String::as_str) {
        Some("grep") => {
            let kind = if a[1] == "-n" { Kind::GrepN } else { Kind::GrepO };
            let ere = a[2] == "-E";
            let icase = a[3] == "-i";
            let pattern = a[if icase { 5 } else { 4 }].clone();
            Ok(Probe { kind, ere, icase, pattern, groups: 0, subjects: lines(text("s.txt")?) })
        }
        Some("sed") => {
            let ere = a[2] == "-E";
            let script = &a[if ere { 3 } else { 2 }];
            let parts: Vec<&str> = script.split(DELIM).collect();
            let (kind, pattern, flags, groups) = if script.starts_with('\\') {
                (Kind::SedN, parts[1], parts[2], 0)
            } else if parts[2] == format!("{OPEN}&{SEP}") {
                (Kind::SedG, parts[1], parts[3], 0)
            } else {
                (Kind::SedSub, parts[1], parts[3], parts[2].matches(SEP).count())
            };
            Ok(Probe { kind, ere, icase: flags.contains('I'), pattern: pattern.to_string(), groups, subjects: lines(text("s.txt")?) })
        }
        Some("gawk") => {
            let groups = a[2].strip_prefix("N=").and_then(|n| n.parse().ok()).ok_or("sem N")?;
            let icase = a.iter().any(|s| s == "IGNORECASE=1");
            let mut pattern = text("re.txt")?;
            if pattern.ends_with('\n') {
                pattern.pop();
            }
            Ok(Probe { kind: Kind::Gawk, ere: true, icase, pattern, groups, subjects: lines(text("s.txt")?) })
        }
        other => Err(format!("sonda desconhecida: {other:?}")),
    }
}

/// `normalize_text(TEXT_REGEX)` do sed 4.9 (só pro teste; o sed de verdade está no ul-textproc).
fn sed_normalize(pattern: &str) -> Vec<u8> {
    let s: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c != '\\' || i + 1 >= s.len() {
            out.push(c);
            i += 1;
            continue;
        }
        let e = s[i + 1];
        let simple = match e {
            'a' => Some('\x07'),
            'f' => Some('\x0c'),
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            'v' => Some('\x0b'),
            _ => None,
        };
        if let Some(ch) = simple {
            out.push(ch);
            i += 2;
            continue;
        }
        let base = match e {
            'd' => Some(10u32),
            'x' => Some(16),
            'o' => Some(8),
            _ => None,
        };
        if let Some(base) = base {
            let (mut n, mut max, mut j) = (0u32, 1u32, i + 2);
            while j < s.len() && max <= 255 {
                match s[j].to_digit(16) {
                    Some(d) if d < base => {
                        n = n * base + d;
                        j += 1;
                        max *= base;
                    }
                    _ => break,
                }
            }
            if j == i + 2 {
                out.push(e);
            } else {
                let ch = char::from((n & 0xff) as u8);
                if ch == '\\' {
                    out.push('\\');
                }
                out.push(ch);
            }
            i = j;
            continue;
        }
        if e == 'c' && i + 2 < s.len() {
            let x = s[i + 2].to_ascii_uppercase() as u32 ^ 0x40;
            out.push(char::from_u32(x).unwrap_or('?'));
            i += 3;
            continue;
        }
        out.push('\\');
        out.push(e);
        i += 2;
    }
    out.into_bytes()
}

struct Compiled {
    /// Seleção de linha (grep: visão do dfa.c quando difere e não há referência).
    select: Regex,
    spans: Regex,
}

fn compile(p: &Probe) -> Result<Compiled, Error> {
    match p.kind {
        Kind::GrepN | Kind::GrepO => {
            let syntax = if p.ere { Syntax::EGREP } else { Syntax::GREP };
            let b = RegexBuilder::new(syntax).icase(p.icase).confusing_brackets_error(true);
            let spans = b.build(p.pattern.as_bytes())?;
            let select = if spans.has_backrefs() { spans.clone() } else { b.clone().dfa_view(true).build(p.pattern.as_bytes())? };
            Ok(Compiled { select, spans })
        }
        Kind::SedN | Kind::SedG | Kind::SedSub => {
            let syntax = if p.ere { Syntax::SED_EXTENDED } else { Syntax::SED_BASIC };
            let re = RegexBuilder::new(syntax).icase(p.icase).confusing_brackets_error(true).build(&sed_normalize(&p.pattern))?;
            Ok(Compiled { select: re.clone(), spans: re })
        }
        Kind::Gawk => {
            let re = RegexBuilder::new(Syntax::GNU_AWK).icase(p.icase).build(p.pattern.as_bytes())?;
            Ok(Compiled { select: re.clone(), spans: re })
        }
    }
}

fn run(p: &Probe, files: harness::MemTree) -> Outcome {
    let c = match compile(p) {
        Ok(c) => c,
        Err(e) => {
            let msg = e.message();
            return match p.kind {
                Kind::GrepN | Kind::GrepO => Outcome::exited("", format!("grep: {msg}\n"), 2, files),
                Kind::Gawk => Outcome::exited("", format!("gawk: fatal: {msg}\n"), 2, files),
                _ if matches!(e, Error::ConfusingBrackets) => Outcome::exited("", format!("sed: {msg}\n"), 4, files),
                _ => Outcome::exited("", format!("sed: -e expression #1, char 0: {msg}\n"), 1, files),
            };
        }
    };
    let mut err = String::new();
    if matches!(p.kind, Kind::GrepN | Kind::GrepO) {
        for w in c.select.warnings() {
            err.push_str(&format!("grep: warning: {}\n", w.message()));
        }
    }
    let mut out = String::new();
    let mut any = false;
    let mut offset = 0;
    for (i, line) in p.subjects.iter().enumerate() {
        let b = line.as_bytes();
        match p.kind {
            Kind::GrepN | Kind::SedN => {
                if c.select.is_match(b) {
                    any = true;
                    if p.kind == Kind::GrepN {
                        out.push_str(&format!("{}:", i + 1));
                    }
                    out.push_str(line);
                    out.push('\n');
                }
            }
            Kind::GrepO => {
                if c.select.is_match(b) {
                    any = true;
                    for m in c.spans.find_iter(b) {
                        out.push_str(&format!("{}:{}\n", offset + m.start, &line[m.start..m.end]));
                    }
                }
            }
            Kind::SedG => {
                // A iteração do s///g do sed 4.9 (`do_subst`).
                let (mut start, mut last_end, mut count) = (0usize, 0usize, 0usize);
                let mut acc = String::new();
                let mut replaced = false;
                while start <= b.len() {
                    let Some(m) = c.spans.find_at(b, start) else { break };
                    if start < m.start {
                        acc.push_str(&line[start..m.start]);
                        start = m.start;
                    }
                    let mut matched = m.len();
                    if matched > 0 || count == 0 || m.start > last_end {
                        count += 1;
                        replaced = true;
                        acc.push(OPEN);
                        acc.push_str(&line[m.start..m.end]);
                        acc.push(SEP);
                    } else {
                        if matched == 0 {
                            if start < b.len() {
                                matched = line[start..].chars().next().map(char::len_utf8).unwrap_or(1);
                            } else {
                                break;
                            }
                        }
                        acc.push_str(&line[m.start..m.start + matched]);
                    }
                    start = m.start + matched;
                    last_end = m.end;
                }
                if start < b.len() {
                    acc.push_str(&line[start..]);
                }
                if replaced {
                    out.push_str(&acc);
                    out.push('\n');
                }
            }
            Kind::SedSub => {
                if let Some(caps) = c.spans.captures(b) {
                    let w = caps.whole();
                    out.push_str(&line[..w.start]);
                    out.push(OPEN);
                    out.push_str(&line[w.start..w.end]);
                    for k in 1..=p.groups.min(9) {
                        out.push(SEP);
                        if let Some(g) = caps.get(k) {
                            out.push_str(&line[g.start..g.end]);
                        }
                    }
                    out.push(CLOSE);
                    out.push_str(&line[w.end..]);
                    out.push('\n');
                }
            }
            Kind::Gawk => {
                let chars = |upto: usize| line[..upto].chars().count();
                match c.spans.captures(b) {
                    Some(caps) => {
                        let w = caps.whole();
                        let mut row = format!("{} {} {}", i + 1, chars(w.start) + 1, line[w.start..w.end].chars().count());
                        for k in 1..=p.groups {
                            match caps.get(k) {
                                Some(g) => row.push_str(&format!(" {}:{}", chars(g.start) + 1, line[g.start..g.end].chars().count())),
                                None => row.push_str(" -"),
                            }
                        }
                        out.push_str(&row);
                        out.push('\n');
                    }
                    None => out.push_str(&format!("{} 0\n", i + 1)),
                }
            }
        }
        offset += b.len() + 1;
    }
    let exit = match p.kind {
        Kind::GrepN | Kind::GrepO => {
            if any {
                0
            } else {
                1
            }
        }
        _ => 0,
    };
    Outcome::exited(out, err, exit, files)
}

struct Engine;

impl Candidate for Engine {
    fn name(&self) -> String {
        "regex-posix".into()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        match probe(inv) {
            Ok(p) => run(&p, inv.files.clone()),
            Err(e) => Outcome::unsupported(e),
        }
    }
}

/// Id da regex de um caso (o id do caso sem o sufixo da sonda).
fn regex_id(case_id: &str) -> String {
    for suffix in ["-grep-n", "-grep-o", "-sed-n", "-sed-g", "-sed-sub", "-gawk"] {
        if let Some(s) = case_id.strip_suffix(suffix) {
            return s.to_string();
        }
    }
    case_id.to_string()
}

/// Corpus minerado do F01 (uso real de agentes; não é commitado). Gere os pares (caso, golden)
/// com a ferramenta descrita no `STATUS.md` e aponte `REGEX_POSIX_MINED` pro JSON.
#[test]
fn mined_corpus() {
    let Some(path) = std::env::var_os("REGEX_POSIX_MINED") else {
        eprintln!("REGEX_POSIX_MINED não definido: corpus minerado pulado");
        return;
    };
    #[derive(serde::Deserialize)]
    struct Pair {
        case: harness::Case,
        golden: Outcome,
        weight: u64,
        rid: String,
    }
    let text = std::fs::read(&path).expect("ler pares minerados");
    let pairs: Vec<Pair> = serde_json::from_slice(&text).expect("JSON dos pares");
    let mut per_regex: BTreeMap<String, (bool, u64)> = BTreeMap::new();
    let mut fails = Vec::new();
    for p in &pairs {
        let inv = p.case.invocation().expect("invocação");
        let actual = Engine.run(&inv);
        let cmp = harness::compare_outcome(&p.case, &p.golden, &actual);
        let e = per_regex.entry(p.rid.clone()).or_insert((true, p.weight));
        e.0 &= cmp.lenient;
        if !cmp.lenient {
            fails.push(cmp);
        }
    }
    let total = per_regex.len();
    let ok = per_regex.values().filter(|v| v.0).count();
    let wtotal: u64 = per_regex.values().map(|v| v.1).sum();
    let wok: u64 = per_regex.values().filter(|v| v.0).map(|v| v.1).sum();
    eprintln!(
        "regex-posix no corpus minerado: regex {ok}/{total} ({:.2}%), ponderado {:.2}%; {} sondas",
        100.0 * ok as f64 / total as f64,
        100.0 * wok as f64 / wtotal as f64,
        pairs.len()
    );
    for f in fails.iter().take(40) {
        eprintln!("  FALHA {}: {}", f.id, f.detail.join(" | "));
    }
    assert_eq!(ok, total, "o F01 media 100% no uso real");
}

#[test]
fn regex_corpus() {
    let (cases, missing) = harness::paths::load_tool("regex").expect("corpus de regex");
    let (conf, comparisons) = harness::score(&Engine, &cases);
    let mut per_regex: BTreeMap<String, (bool, bool)> = BTreeMap::new();
    for c in &comparisons {
        let e = per_regex.entry(regex_id(&c.id)).or_insert((true, true));
        e.0 &= c.strict;
        e.1 &= c.lenient;
    }
    let regexes = per_regex.len();
    let strict_re = per_regex.values().filter(|v| v.0).count();
    let lenient_re = per_regex.values().filter(|v| v.1).count();
    eprintln!(
        "regex-posix no corpus de borda: sondas {}/{} estrito ({:.2}%), {}/{} leniente ({:.2}%); regex {}/{} estrito ({:.2}%), {}/{} leniente ({:.2}%); {} sem golden",
        conf.strict_pass,
        conf.total,
        100.0 * conf.strict_rate(),
        conf.lenient_pass,
        conf.total,
        100.0 * conf.lenient_rate(),
        strict_re,
        regexes,
        100.0 * strict_re as f64 / regexes as f64,
        lenient_re,
        regexes,
        100.0 * lenient_re as f64 / regexes as f64,
        missing
    );
    for c in comparisons.iter().filter(|c| !c.lenient) {
        eprintln!("  FALHA {}: {}", c.id, c.detail.join(" | "));
    }
    let lenient_rate = lenient_re as f64 / regexes as f64;
    assert!(lenient_rate >= 0.9927, "conformidade abaixo da do F01: {lenient_rate}");
}
